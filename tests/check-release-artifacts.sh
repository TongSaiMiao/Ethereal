#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
EXPECTED_APP_ID="me.ethereal.app"
EXPECTED_MODULE_VERSION="1.3"
EXPECTED_FEATURE_MARKER="ethereal-protocol-v1,uid-token-auth-v3,kstorage-v2"
EXPECTED_KMIS=(
  android12-5.4
  android12-5.10
  android13-5.10
  android13-5.15
  android14-5.15
  android14-6.1
  android15-6.6
  android16-6.12
)

old_r_title='R''Patch'
old_r_lower='r''patch'
old_r_upper='R''PATCH'
old_a_title='A''Patch'
old_a_lower='a''patch'
old_a_upper='A''PATCH'
legacy_auth_title='Super''Key'
legacy_auth_lower='super''key'
legacy_auth_upper='SUPER''KEY'
legacy_auth_short='s''key'
legacy_plugin_title='K''PM'
legacy_plugin_lower='k''pm'
legacy_plugin_module='KP''Module'
legacy_plugin_module_lower='kp''module'
legacy_plugin_monitor='kp''mon'
old_kernel_patch='Kernel''Patch'
old_kernel_patch_lower='kernel''patch'
old_su_path='r''p/su'

legacy_brand_re="${old_r_title}|${old_r_lower}|${old_r_upper}|${old_a_title}|${old_a_lower}|${old_a_upper}|${legacy_auth_title}|${legacy_auth_lower}|${legacy_auth_upper}|${legacy_plugin_module}|${legacy_plugin_module_lower}|${legacy_plugin_monitor}|${old_kernel_patch}|${old_kernel_patch_lower}"
legacy_word_re="(^|[^[:alnum:]_])(${legacy_auth_short}|${legacy_plugin_title}|${legacy_plugin_lower})([^[:alnum:]_]|$)"
legacy_path_re="me\\.${old_r_lower}\\.app|me\\.bmax\\.${old_a_lower}|/data/adb/(rp|ap|${legacy_plugin_lower})(/|$)|/dev/\\.(${old_r_lower}|${old_a_lower})(/|$)|/sys/module/(${old_r_lower}|${old_a_lower})(/|$)|(^|/)${old_su_path}(/|$)|(^|/)(rpd|apd)\\.full($|/)|(^|/)lib(rpd|rpjni|apjni)\\.so($|/)|(^|/)${old_r_lower}-init($|/)"
legacy_content_re="${legacy_brand_re}|${legacy_word_re}|${legacy_path_re}"

legacy_magic2_hex='5250''5443'
legacy_ioctl_hex='5250''0001'
legacy_ap_config_hex='4150''544D'
legacy_magic_re="${legacy_magic2_hex}|${legacy_ioctl_hex}|${legacy_ap_config_hex}"
legacy_magic_word_re="(^|[[:space:]])(${legacy_magic2_hex}|43545052|${legacy_ioctl_hex}|01005052|${legacy_ap_config_hex}|4d545041)([[:space:]]|$)"

failed=0
temp_dir=""

fail() {
  printf 'FAIL\t%s\n' "$*" >&2
  failed=1
}

cleanup() {
  local candidate="${temp_dir:-}"
  local base="${TMPDIR:-/tmp}"

  if [[ -n "$candidate" && -d "$candidate" &&
        "$candidate" == "$base"/ethereal-release-check.* ]]; then
    rm -rf -- "$candidate"
  fi
}

trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' HUP TERM

usage() {
  printf 'usage: %s <release.apk>\n' "$0" >&2
  exit 2
}

[[ $# -eq 1 ]] || usage
[[ -s "$1" ]] || {
  printf 'release APK is missing or empty: %s\n' "$1" >&2
  exit 2
}

apk_dir="$(cd -- "$(dirname -- "$1")" && pwd -P)"
apk="$apk_dir/$(basename -- "$1")"
feature_marker="$(tr -d '\r\n' < "$ROOT/kmod/feature-marker.txt")"
[[ -n "$feature_marker" ]] || {
  printf 'Ethereal feature marker is empty\n' >&2
  exit 2
}
[[ "$feature_marker" == "$EXPECTED_FEATURE_MARKER" ]] || {
  printf 'unexpected Ethereal feature marker: %s\n' "$feature_marker" >&2
  exit 2
}

for command_name in unzip grep od tr find sort modinfo mktemp cmp readelf; do
  command -v "$command_name" >/dev/null 2>&1 ||
    fail "required command is unavailable: $command_name"
done
(( failed == 0 )) || exit 1

temp_dir="$(mktemp -d "${TMPDIR:-/tmp}/ethereal-release-check.XXXXXX")"
unpack_dir="$temp_dir/apk"
entries_file="$temp_dir/entries.txt"
mkdir -p -- "$unpack_dir"

scan_name() {
  local label="$1"
  local value="$2"

  if [[ "$value" =~ $legacy_content_re ]]; then
    fail "$label contains a legacy package, library, brand, or runtime path: $value"
  fi
}

scan_file() {
  local label="$1"
  local path="$2"

  if [[ ! -s "$path" ]]; then
    fail "$label is missing or empty: $path"
    return
  fi
  if LC_ALL=C grep -aE -- "$legacy_content_re" "$path" >/dev/null 2>&1; then
    fail "$label contains a legacy package, library, brand, or runtime path"
  fi
  if LC_ALL=C grep -aiE -- "$legacy_magic_re" "$path" >/dev/null 2>&1; then
    fail "$label contains a textual legacy protocol magic"
  fi
  # Protocol constants are 32-bit words. Byte-stream matching can join two
  # unrelated entries (for example in .eh_frame_hdr) and report a false hit.
  if LC_ALL=C od -An -tx4 -v -- "$path" |
      grep -EiE -- "$legacy_magic_word_re" >/dev/null; then
    fail "$label contains legacy protocol magic bytes"
  fi
}

check_standalone_elf() {
  local label="$1"
  local elf="$2"
  local dynamic symbols

  if [[ ! -s "$elf" ]]; then
    fail "$label is missing or empty: $elf"
    return
  fi
  if ! dynamic="$(LC_ALL=C readelf --dynamic --wide "$elf" 2>&1)"; then
    fail "$label has an unreadable ELF dynamic section"
    return
  fi
  if grep -Fq 'Shared library: [libc++_shared.so]' <<< "$dynamic"; then
    fail "$label depends on libc++_shared.so but is executed as a standalone binary"
  fi
  if ! symbols="$(LC_ALL=C readelf --dyn-syms --wide "$elf" 2>&1)"; then
    fail "$label has an unreadable ELF dynamic symbol table"
    return
  fi
  if grep -E 'UND[[:space:]]+.*(_Z|__cxa_|__gxx_personality)' <<< "$symbols" |
      grep -Ev '__cxa_[[:alnum:]_]+@LIBC([[:space:]]|$)' >/dev/null; then
    fail "$label leaves C++ runtime symbols unresolved for the Android linker"
  fi
}

check_module() {
  local label="$1"
  local ko="$2"
  local name version features

  if [[ ! -s "$ko" ]]; then
    fail "$label is missing or empty: $ko"
    return
  fi
  name="$(modinfo -F name "$ko" 2>/dev/null | tr -d '\r\n' || true)"
  version="$(modinfo -F version "$ko" 2>/dev/null | tr -d '\r\n' || true)"
  features="$(modinfo -F ethereal_features "$ko" 2>/dev/null | tr -d '\r\n' || true)"
  [[ "$name" == ethereal ]] ||
    fail "$label module name is '$name', expected 'ethereal'"
  [[ "$version" == "$EXPECTED_MODULE_VERSION" ]] ||
    fail "$label module version is '$version', expected '$EXPECTED_MODULE_VERSION'"
  [[ "$features" == "$feature_marker" ]] ||
    fail "$label feature marker is '$features', expected '$feature_marker'"
  scan_file "$label" "$ko"
}

find_android_tool() {
  local tool="$1"
  local sdk candidate

  if command -v "$tool" >/dev/null 2>&1; then
    command -v "$tool"
    return 0
  fi
  for sdk in "${ANDROID_SDK_ROOT:-}" "${ANDROID_HOME:-}"; do
    [[ -n "$sdk" ]] || continue
    candidate="$sdk/cmdline-tools/latest/bin/$tool"
    if [[ -x "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return 0
    fi
    for candidate in "$sdk"/cmdline-tools/*/bin/"$tool"; do
      if [[ -x "$candidate" ]]; then
        printf '%s\n' "$candidate"
        return 0
      fi
    done
  done
  return 1
}

find_aapt() {
  local sdk candidate found=""

  for candidate in aapt aapt2; do
    if command -v "$candidate" >/dev/null 2>&1; then
      command -v "$candidate"
      return 0
    fi
  done
  for sdk in "${ANDROID_SDK_ROOT:-}" "${ANDROID_HOME:-}"; do
    [[ -n "$sdk" ]] || continue
    for candidate in "$sdk"/build-tools/*/aapt "$sdk"/build-tools/*/aapt2; do
      [[ -x "$candidate" ]] && found="$candidate"
    done
  done
  [[ -n "$found" ]] || return 1
  printf '%s\n' "$found"
}

read_application_id() {
  local analyzer aapt badging

  if analyzer="$(find_android_tool apkanalyzer)"; then
    "$analyzer" manifest application-id "$apk" 2>/dev/null | tr -d '\r\n'
    return
  fi
  if aapt="$(find_aapt)"; then
    badging="$("$aapt" dump badging "$apk" 2>/dev/null || true)"
    printf '%s\n' "$badging" |
      sed -n "s/^package: name='\\([^']*\\)'.*/\\1/p" |
      head -n 1
    return
  fi
  return 1
}

payload_files=(
  "$ROOT/app/src/main/assets/ethd.full"
  "$ROOT/app/src/main/assets/ethereal-init"
  "$ROOT/app/src/main/assets/su"
  "$ROOT/app/src/main/jniLibs/arm64-v8a/libsu.so"
  "$ROOT/app/libs/arm64-v8a/libethd.so"
  "$ROOT/app/libs/arm64-v8a/libramtool.so"
  "$ROOT/ethd/embedded/ethinit"
  "$ROOT/ethd/embedded/ramtool"
)
for payload in "${payload_files[@]}"; do
  scan_file "generated payload ${payload#"$ROOT"/}" "$payload"
done
check_standalone_elf "generated payload app/src/main/assets/ethd.full" \
  "$ROOT/app/src/main/assets/ethd.full"
check_standalone_elf "generated payload app/libs/arm64-v8a/libethd.so" \
  "$ROOT/app/libs/arm64-v8a/libethd.so"

declare -A expected_module_names=()
generated_kmod_dir="$ROOT/app/build/generated/kmod-assets/kmod"
for kmi in "${EXPECTED_KMIS[@]}"; do
  module_name="ethereal.$kmi.ko"
  prebuilt_module="$ROOT/kmod/prebuilt/$kmi/ethereal.ko"
  check_module "release prebuilt kmod/prebuilt/$kmi/ethereal.ko" \
    "$prebuilt_module"
  expected_module_names["$module_name"]=1
  check_module "generated payload assets/kmod/$module_name" \
    "$generated_kmod_dir/$module_name"
  cmp -s "$prebuilt_module" "$generated_kmod_dir/$module_name" ||
    fail "generated payload assets/kmod/$module_name differs from its release prebuilt"
done
if [[ -d "$generated_kmod_dir" ]]; then
  generated_count=0
  while IFS= read -r -d '' generated_ko; do
    generated_name="$(basename -- "$generated_ko")"
    generated_count=$((generated_count + 1))
    if [[ -z "${expected_module_names[$generated_name]:-}" ]]; then
      fail "generated payload has unexpected kernel module: assets/kmod/$generated_name"
      scan_file "unexpected generated module assets/kmod/$generated_name" "$generated_ko"
    fi
  done < <(find "$generated_kmod_dir" -maxdepth 1 -type f -name '*.ko' -print0)
  [[ "$generated_count" -eq "${#EXPECTED_KMIS[@]}" ]] ||
    fail "generated payload has $generated_count kernel modules, expected ${#EXPECTED_KMIS[@]}"
else
  fail "generated kernel module directory is missing: $generated_kmod_dir"
fi

if ! unzip -Z1 "$apk" > "$entries_file"; then
  fail "cannot list APK entries: $apk"
else
  unsafe_entries=0
  while IFS= read -r entry; do
    [[ -n "$entry" ]] || continue
    scan_name "APK entry" "$entry"
    if [[ "$entry" == /* || "/$entry/" == *'/../'* ]]; then
      fail "APK contains an unsafe archive entry: $entry"
      unsafe_entries=1
    fi
  done < "$entries_file"

  if (( unsafe_entries == 0 )); then
    if ! unzip -qq "$apk" -d "$unpack_dir"; then
      fail "cannot extract APK: $apk"
    fi
  fi
fi

application_id="$(read_application_id || true)"
if [[ -z "$application_id" ]]; then
  fail "cannot read APK applicationId; install apkanalyzer or aapt"
elif [[ "$application_id" != "$EXPECTED_APP_ID" ]]; then
  fail "APK applicationId is '$application_id', expected '$EXPECTED_APP_ID'"
fi

if [[ -d "$unpack_dir" && -n "$(find "$unpack_dir" -mindepth 1 -print -quit)" ]]; then
  while IFS= read -r -d '' extracted; do
    relative="${extracted#"$unpack_dir"/}"
    case "$relative" in
      AndroidManifest.xml|resources.arsc|classes*.dex|assets/*|lib/*)
        scan_file "APK content $relative" "$extracted"
        ;;
    esac
  done < <(find "$unpack_dir" -type f -print0)

  expected_asset_entries=(
    assets/ethd.full
    assets/ethereal-init
    assets/su
  )
  expected_asset_sources=(
    "$ROOT/app/src/main/assets/ethd.full"
    "$ROOT/app/src/main/assets/ethereal-init"
    "$ROOT/app/src/main/assets/su"
  )
  for index in "${!expected_asset_entries[@]}"; do
    entry="${expected_asset_entries[$index]}"
    source="${expected_asset_sources[$index]}"
    if [[ ! -s "$unpack_dir/$entry" ]]; then
      fail "APK required payload is missing or empty: $entry"
    elif ! cmp -s "$source" "$unpack_dir/$entry"; then
      fail "APK payload $entry differs from the freshly generated source"
    fi
  done

  required_native_entries=(
    lib/arm64-v8a/libbusybox.so
    lib/arm64-v8a/libethd.so
    lib/arm64-v8a/libetherealjni.so
    lib/arm64-v8a/libramtool.so
  )
  for entry in "${required_native_entries[@]}"; do
    [[ -s "$unpack_dir/$entry" ]] ||
      fail "APK required native payload is missing or empty: $entry"
  done
  check_standalone_elf "APK payload assets/ethd.full" \
    "$unpack_dir/assets/ethd.full"
  check_standalone_elf "APK native patcher lib/arm64-v8a/libethd.so" \
    "$unpack_dir/lib/arm64-v8a/libethd.so"

  apk_kmod_dir="$unpack_dir/assets/kmod"
  apk_module_count=0
  if [[ -d "$apk_kmod_dir" ]]; then
    while IFS= read -r -d '' apk_ko; do
      apk_name="$(basename -- "$apk_ko")"
      apk_module_count=$((apk_module_count + 1))
      if [[ -z "${expected_module_names[$apk_name]:-}" ]]; then
        fail "APK has unexpected kernel module asset: assets/kmod/$apk_name"
      fi
    done < <(find "$apk_kmod_dir" -maxdepth 1 -type f -name '*.ko' -print0)
  else
    fail "APK kernel module asset directory is missing"
  fi
  [[ "$apk_module_count" -eq "${#EXPECTED_KMIS[@]}" ]] ||
    fail "APK has $apk_module_count kernel module assets, expected ${#EXPECTED_KMIS[@]}"

  for kmi in "${EXPECTED_KMIS[@]}"; do
    module_name="ethereal.$kmi.ko"
    check_module "APK asset assets/kmod/$module_name" \
      "$apk_kmod_dir/$module_name"
    cmp -s "$ROOT/kmod/prebuilt/$kmi/ethereal.ko" "$apk_kmod_dir/$module_name" ||
      fail "APK asset assets/kmod/$module_name differs from its release prebuilt"
  done
else
  fail "APK was not extracted"
fi

if (( failed != 0 )); then
  exit 1
fi

printf 'OK: generated payloads and release APK use the Ethereal v1 protocol (%s)\n' \
  "$feature_marker"
