#!/usr/bin/env bash
# zipalign + sign an APK using ONLY the APK Signature Scheme v2.
#
# The v1 (JAR) and v3/v4 schemes are explicitly disabled so the signed APK
# carries exactly one signature block (v2). Replaces the old
# kevin-david/zipalign-sign-android-release action, which signed with
# apksigner defaults (v1+v2+v3) and offered no way to pick schemes.
#
# Usage: sign_v2_only.sh <apk-dir>
#
# Env (required):
#   ANDROID_HOME          - Android SDK root (set by android-actions/setup-android)
#   BUILD_TOOLS_VERSION   - build-tools version, e.g. 35.0.0
#   KEYSTORE_FILE         - path to the decoded keystore (.jks)
#   KEY_STORE_PASSWORD    - keystore password
#   KEY_ALIAS             - key alias
#   KEY_PASSWORD          - key password
#
# Stdout: path of the signed APK

set -euo pipefail

[[ $# -eq 1 ]] || { echo "usage: $0 <apk-dir>" >&2; exit 2; }
DIR="$1"
: "${ANDROID_HOME:?}"
: "${BUILD_TOOLS_VERSION:?}"
: "${KEYSTORE_FILE:?}"
: "${KEY_STORE_PASSWORD:?}"
: "${KEY_ALIAS:?}"
: "${KEY_PASSWORD:?}"

BUILD_TOOLS="$ANDROID_HOME/build-tools/$BUILD_TOOLS_VERSION"
ZIPALIGN="$BUILD_TOOLS/zipalign"
APKSIGNER="$BUILD_TOOLS/apksigner"

[ -d "$BUILD_TOOLS" ] || { echo "build-tools not found @ $BUILD_TOOLS" >&2; exit 1; }
[ -f "$KEYSTORE_FILE" ] || { echo "keystore not found @ $KEYSTORE_FILE" >&2; exit 1; }
[ -x "$ZIPALIGN" ] || { echo "zipalign not found @ $ZIPALIGN" >&2; exit 1; }
[ -x "$APKSIGNER" ] || { echo "apksigner not found @ $APKSIGNER" >&2; exit 1; }
[ -d "$DIR" ] || { echo "APK directory not found @ $DIR" >&2; exit 1; }

# This project emits exactly one APK per output directory.
shopt -s nullglob
APKS=("$DIR"/*.apk)
if [ "${#APKS[@]}" -ne 1 ]; then
    echo "expected exactly one APK in $DIR, found ${#APKS[@]}" >&2
    exit 1
fi
APK="${APKS[0]}"

# Zipalign (4-byte, page-align uncompressed .so). zipalign cannot align in
# place. Keep the Gradle output untouched and use a private temporary copy.
# Its verbose listing goes to stderr so stdout carries only the signed path.
TEMP_DIR="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/ethereal-apksign.XXXXXX")"
cleanup() {
    local base="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
    if [[ -n "${TEMP_DIR:-}" && -d "$TEMP_DIR" &&
          "$TEMP_DIR" == "$base"/ethereal-apksign.* ]]; then
        rm -rf -- "$TEMP_DIR"
    fi
}
trap cleanup EXIT
ALIGNED="$TEMP_DIR/aligned.apk"
"$ZIPALIGN" -p -v 4 "$APK" "$ALIGNED" 1>&2

# Sign with ONLY the v2 scheme.
SIGNED="${APK%.apk}-signed.apk"
"$APKSIGNER" sign \
    --ks "$KEYSTORE_FILE" \
    --ks-pass env:KEY_STORE_PASSWORD \
    --ks-key-alias "$KEY_ALIAS" \
    --key-pass env:KEY_PASSWORD \
    --v1-signing-enabled false \
    --v2-signing-enabled true \
    --v3-signing-enabled false \
    --v4-signing-enabled false \
    --out "$SIGNED" \
    "$ALIGNED"

# Verify, and assert that exactly one signer and only the v2 scheme are present.
VERIFY_OUTPUT=$("$APKSIGNER" verify --verbose "$SIGNED" 2>&1)
if ! grep -q "Verified using v1 scheme.*: false" <<< "$VERIFY_OUTPUT" \
    || ! grep -q "Verified using v2 scheme.*: true" <<< "$VERIFY_OUTPUT" \
    || ! grep -q "Verified using v3 scheme.*: false" <<< "$VERIFY_OUTPUT" \
    || ! grep -q "Verified using v3.1 scheme.*: false" <<< "$VERIFY_OUTPUT" \
    || ! grep -q "Verified using v4 scheme.*: false" <<< "$VERIFY_OUTPUT" \
    || ! grep -q "Number of signers: 1" <<< "$VERIFY_OUTPUT"; then
    echo "ERROR: signed APK is not v2-only:" >&2
    echo "$VERIFY_OUTPUT" >&2
    exit 1
fi

echo "$SIGNED"
