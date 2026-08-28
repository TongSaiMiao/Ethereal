#!/usr/bin/env bash
# Reproducible, offline GKI 1.0 boot.img patch -> audit -> QEMU E2E.
# No physical partition is read or written, and ethd runs without uid 0 or caps.
set -euo pipefail

KMI="${1:-android12-5.4}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"
ROOT="$(cd -- "$ROOT" && pwd -P)"
LOCKS="$ROOT/kmod/gki-locks.tsv"
OUT_BASE="$ROOT/tests/out/gki1-boot-patch-e2e"
OFFICIAL_CACHE="${GKI_OFFICIAL_CACHE:-/root/gki-official}"
TARGETS="$ROOT/tests/out/targets"
RUST_BIN="${RUST_BIN:-/opt/ethereal-rust/bin}"
CARGO="${CARGO:-$RUST_BIN/cargo}"
CARGO_HOME="${CARGO_HOME:-$ROOT/.tools/cargo}"
QEMU="${QEMU:-qemu-system-aarch64}"
REQUIRED_KMIS=(
  android12-5.4
  android12-5.10
  android13-5.10
  android13-5.15
  android14-5.15
  android14-6.1
  android15-6.6
  android16-6.12
)

declare -a ARM64_CC=()
declare -a ARM64_LD_FLAGS=()

select_arm64_compiler() {
  local ndk_root candidate

  if command -v aarch64-linux-android26-clang >/dev/null 2>&1; then
    ARM64_CC=("$(command -v aarch64-linux-android26-clang)")
    ARM64_LD_FLAGS=(-fuse-ld=lld)
    return
  fi

  for ndk_root in "${ANDROID_NDK_HOME:-}" "${ANDROID_NDK_ROOT:-}"; do
    [[ -n "$ndk_root" ]] || continue
    for candidate in "$ndk_root"/toolchains/llvm/prebuilt/*/bin/clang; do
      if [[ -x "$candidate" ]]; then
        ARM64_CC=("$candidate" --target=aarch64-linux-android26)
        ARM64_LD_FLAGS=(-fuse-ld=lld)
        return
      fi
    done
  done

  for ndk_root in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}"; do
    [[ -n "$ndk_root" ]] || continue
    for candidate in "$ndk_root"/ndk/*/toolchains/llvm/prebuilt/*/bin/clang; do
      if [[ -x "$candidate" ]]; then
        ARM64_CC=("$candidate" --target=aarch64-linux-android26)
        ARM64_LD_FLAGS=(-fuse-ld=lld)
        return
      fi
    done
  done

  if command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
    ARM64_CC=("$(command -v aarch64-linux-gnu-gcc)")
    ARM64_LD_FLAGS=()
    return
  fi

  echo "missing ARM64 compiler: install Android NDK clang or aarch64-linux-gnu-gcc" >&2
  exit 2
}

build_test_ethinit() {
  local output="$1"

  [[ -s "$ROOT/ethinit/start.S" && -s "$ROOT/ethinit/ethinit.c" ]] || {
    echo "missing ethinit sources" >&2
    exit 2
  }
  mkdir -p -- "$(dirname -- "$output")"
  "${ARM64_CC[@]}" \
    -nostdlib -nostartfiles -ffreestanding -fPIC -fno-builtin \
    -fvisibility=hidden -fno-stack-protector -fno-unwind-tables \
    -fomit-frame-pointer -mbranch-protection=none -Os -static \
    "${ARM64_LD_FLAGS[@]}" -Wl,-e,_start -Wl,--gc-sections \
    -Wl,--build-id=none -Wl,--no-dynamic-linker -Wl,-z,norelro \
    -Wl,-z,max-page-size=16384 \
    -o "$output" "$ROOT/ethinit/start.S" "$ROOT/ethinit/ethinit.c"
  chmod 0755 "$output"
  [[ -s "$output" ]] || { echo "failed to build temporary ethinit" >&2; exit 2; }
}

build_test_ethsu() {
  local output="$1"

  [[ -s "$ROOT/ethsu/ethsu.c" ]] || { echo "missing ethsu source" >&2; exit 2; }
  mkdir -p -- "$(dirname -- "$output")"
  "${ARM64_CC[@]}" -std=gnu17 -static -O2 -s "${ARM64_LD_FLAGS[@]}" \
    -Wl,--build-id=none -o "$output" "$ROOT/ethsu/ethsu.c"
  chmod 0755 "$output"
  [[ -s "$output" ]] || { echo "failed to build temporary ethsu" >&2; exit 2; }
}

for tool in awk basename cmp cpio find grep id mktemp python3 realpath sha256sum sort \
  strings timeout touch tr wc "$CARGO" "$QEMU"; do
  if ! command -v "$tool" >/dev/null 2>&1 && [[ ! -x "$tool" ]]; then
    echo "missing required tool: $tool" >&2
    exit 2
  fi
done
select_arm64_compiler
if [[ "$(id -u)" -eq 0 ]] && ! command -v setpriv >/dev/null 2>&1; then
  echo "missing required tool for rootless patch execution: setpriv" >&2
  exit 2
fi
[[ -s "$LOCKS" ]] || { echo "missing GKI lock file: $LOCKS" >&2; exit 2; }
[[ "$KMI" =~ ^android[0-9]+-[0-9]+\.[0-9]+$ ]] &&
  awk -F '\t' -v wanted="$KMI" '
    NR > 1 && $1 == wanted { found = 1; exit }
    END { exit !found }
  ' "$LOCKS" || {
    echo "KMI is not pinned in $LOCKS: $KMI" >&2
    exit 2
  }

mkdir -p -- "$OUT_BASE"
OUT_BASE_ABS="$(cd -- "$OUT_BASE" && pwd -P)"
[[ "$OUT_BASE_ABS" == "$OUT_BASE" ]] || {
  echo "refusing symlinked test output root: $OUT_BASE -> $OUT_BASE_ABS" >&2
  exit 2
}
OUT="$(realpath -m -- "$OUT_BASE/$KMI")"
[[ "$OUT" == "$OUT_BASE_ABS/$KMI" ]] || {
  echo "refusing unsafe KMI output path: $OUT" >&2
  exit 2
}
if [[ -e "$OUT" ]]; then
  OUT_REAL="$(realpath -- "$OUT")"
  [[ "$OUT_REAL" == "$OUT_BASE_ABS/$KMI" && -d "$OUT_REAL" ]] || {
    echo "refusing unsafe output reset: $OUT -> $OUT_REAL" >&2
    exit 2
  }
  rm -rf -- "$OUT_REAL"
fi
mkdir -p -- "$OUT" "$TARGETS"

PROVENANCE="$ROOT/kmod/prebuilt/$KMI/provenance.env"
KO="$ROOT/kmod/prebuilt/$KMI/ethereal.ko"
ETHINIT="${ETHEREAL_ETHINIT:-$ROOT/ethd/embedded/ethinit}"
SU="${ETHEREAL_SU:-$ROOT/app/src/main/assets/su}"
[[ -s "$PROVENANCE" ]] || { echo "missing provenance: $PROVENANCE" >&2; exit 2; }

provenance_value() {
  local key="$1"
  awk -F '=' -v wanted="$key" '
    $1 == wanted && !found {
      print substr($0, index($0, "=") + 1)
      found = 1
    }
  ' "$PROVENANCE"
}

KERNEL_ARTIFACT="$(provenance_value kernel_artifact)"
KERNEL_SHA256="$(provenance_value kernel_sha256)"
OFFICIAL_RELEASE="$(provenance_value official_release)"
KO_SHA256="$(provenance_value ko_sha256)"
[[ -n "$KERNEL_ARTIFACT" && "$KERNEL_ARTIFACT" == "$(basename -- "$KERNEL_ARTIFACT")" ]] || {
  echo "invalid official kernel artifact in $PROVENANCE" >&2
  exit 2
}
[[ "$KERNEL_SHA256" =~ ^[0-9a-f]{64}$ && "$KO_SHA256" =~ ^[0-9a-f]{64}$ &&
   -n "$OFFICIAL_RELEASE" ]] || {
  echo "incomplete official kernel/module identity in $PROVENANCE" >&2
  exit 2
}
KERNEL="${QEMU_KERNEL:-$OFFICIAL_CACHE/$KMI/$KERNEL_ARTIFACT}"
for file in "$KERNEL" "$KO"; do
  [[ -s "$file" ]] || { echo "missing required input: $file" >&2; exit 2; }
done
PAYLOAD_OUT="$OUT/generated-payloads"
if [[ ! -s "$ETHINIT" ]]; then
  ETHINIT="$PAYLOAD_OUT/ethereal-init"
  build_test_ethinit "$ETHINIT"
fi
if [[ ! -s "$SU" ]]; then
  SU="$PAYLOAD_OUT/su"
  build_test_ethsu "$SU"
fi
[[ "$(sha256sum "$KERNEL" | awk '{ print $1 }')" == "$KERNEL_SHA256" ]] || {
  echo "official kernel SHA-256 mismatch: $KERNEL" >&2
  exit 2
}
[[ "$(sha256sum "$KO" | awk '{ print $1 }')" == "$KO_SHA256" ]] || {
  echo "release KO SHA-256 mismatch: $KO" >&2
  exit 2
}
strings "$KERNEL" | awk -v expected="$OFFICIAL_RELEASE" '
  $0 == expected { found = 1 }
  END { exit !found }
' || {
  echo "official kernel release mismatch: $OFFICIAL_RELEASE" >&2
  exit 2
}
bash "$ROOT/kmod/verify-prebuilt.sh" "$KMI"

MANAGER_TOKEN_FILE="$OUT/manager-token.bin"
python3 - "$MANAGER_TOKEN_FILE" <<'PY'
import sys
from pathlib import Path

Path(sys.argv[1]).write_bytes(bytes(range(1, 33)))
PY
chmod 0644 "$MANAGER_TOKEN_FILE"

export PATH="$RUST_BIN:/usr/bin:/bin:$PATH"
export CARGO_HOME
export CARGO_TARGET_DIR="$TARGETS/ethd"
"$CARGO" build --manifest-path "$ROOT/ethd/Cargo.toml" --release --locked
ETHD="$CARGO_TARGET_DIR/release/ethd"
export CARGO_TARGET_DIR="$TARGETS/ramtool"
"$CARGO" build --manifest-path "$ROOT/ramtool/Cargo.toml" --release --locked
RAMTOOL="$CARGO_TARGET_DIR/release/ramtool"
[[ -x "$ETHD" && -x "$RAMTOOL" ]] || {
  echo "missing freshly built ethd or ramtool" >&2
  exit 2
}

ROOTFS="$OUT/stock-root"
mkdir -p -- "$ROOTFS"
SOURCE_DATE_EPOCH=0 "${ARM64_CC[@]}" -static -O2 -s "${ARM64_LD_FLAGS[@]}" \
  -Wl,--build-id=none \
  "-DETHEREAL_EXPECTED_KMI=\"$KMI\"" \
  -o "$ROOTFS/init" "$ROOT/tests/gki1-boot-patch-e2e-init.c"
chmod 0755 "$ROOTFS/init"
mkdir -p "$ROOTFS/eth" "$ROOTFS/debug_ramdisk"
printf '%s\n' stock-root-su > "$ROOTFS/su"
printf '%s\n' stock-eth-su > "$ROOTFS/eth/su"
printf '%s\n' stock-debug-su > "$ROOTFS/debug_ramdisk/su"
touch -d @0 "$ROOTFS" "$ROOTFS/init"
(
  cd "$ROOTFS"
  find . -mindepth 1 -print0 | sort -z | \
    cpio --null --create --format=newc --quiet --reproducible --owner=0:0
) > "$OUT/stock.cpio"

STOCK_IMAGE="$OUT/boot.img"
python3 - "$OUT/stock.cpio" "$KERNEL" "$STOCK_IMAGE" <<'PY'
import struct
import sys
from pathlib import Path

ramdisk_path, kernel_path, image_path = map(Path, sys.argv[1:])
ramdisk = ramdisk_path.read_bytes()
kernel = kernel_path.read_bytes()

def align(value, boundary=4096):
    return (value + boundary - 1) // boundary * boundary

def padded(data, boundary=4096):
    return data + bytes((-len(data)) % boundary)

cmdline = b"console=ttyAMA0 ethereal.fixture=gki1"
header = bytearray(4096)
header[:8] = b"ANDROID!"
struct.pack_into("<I", header, 0x08, len(kernel))
struct.pack_into("<I", header, 0x0C, len(ramdisk))
struct.pack_into("<I", header, 0x14, 1580)
struct.pack_into("<I", header, 0x28, 3)
header[0x2C:0x2C + len(cmdline)] = cmdline
body = bytes(header) + padded(kernel) + padded(ramdisk)

vbmeta_offset = align(len(body) + 8 * 1024 * 1024)
image = bytearray([0xA5]) * (vbmeta_offset + 4096)
image[:len(body)] = body
image[vbmeta_offset:vbmeta_offset + 4] = b"AVB0"
footer = len(image) - 64
struct.pack_into(
    ">4sIIQQQ28s",
    image,
    footer,
    b"AVBf",
    1,
    0,
    len(body),
    vbmeta_offset,
    256,
    bytes(28),
)
image_path.write_bytes(image)
PY

run_unprivileged_patch() {
  local work="$1" output="$2"
  local identity_script

  mkdir -p -- "$work"
  cp -f -- "$RAMTOOL" "$work/ramtool"
  cp -f -- "$ETHINIT" "$work/ethinit"
  cp -f -- "$SU" "$work/su"
  for bundled_kmi in "${REQUIRED_KMIS[@]}"; do
    bundled_ko="$ROOT/kmod/prebuilt/$bundled_kmi/ethereal.ko"
    [[ -s "$bundled_ko" ]] || {
      echo "missing bundled release KO: $bundled_ko" >&2
      return 2
    }
    cp -f -- "$bundled_ko" "$work/ethereal.$bundled_kmi.ko"
  done
  chmod 0755 "$work/ramtool" "$work/ethinit" "$work/su"
  if [[ "$(id -u)" -eq 0 ]]; then
    chown 65534:65534 "$work"
  fi
  identity_script='
    uid="$(id -u)"
    cap_eff="$(awk '\''/^CapEff:/ { print $2 }'\'' /proc/self/status)"
    printf "uid=%s\ncap_eff=%s\n" "$uid" "$cap_eff" > patch-identity.txt
    test "$uid" -ne 0
    test "$cap_eff" = 0000000000000000
    exec "$@"
  '
  (
    cd "$work"
    if [[ "$(id -u)" -eq 0 ]]; then
      setpriv --reuid=65534 --regid=65534 --clear-groups \
        --bounding-set=-all --inh-caps=-all --ambient-caps=-all --no-new-privs \
        /bin/sh -c "$identity_script" sh \
        "$ETHD" boot-patch \
          --image "$STOCK_IMAGE" \
          --out "$output" \
          --manager-uid 2000 \
          --manager-token-file "$MANAGER_TOKEN_FILE" \
          --ethinit "$work/ethinit"
    else
      /bin/sh -c "$identity_script" sh \
        "$ETHD" boot-patch \
          --image "$STOCK_IMAGE" \
          --out "$output" \
          --manager-uid 2000 \
          --manager-token-file "$MANAGER_TOKEN_FILE" \
          --ethinit "$work/ethinit"
    fi
  )
  grep -Eq '^uid=[1-9][0-9]*$' "$work/patch-identity.txt"
  grep -qx 'cap_eff=0000000000000000' "$work/patch-identity.txt"
}

PATCHED_IMAGE="$OUT/Ethereal-boot.img"
REPRO_IMAGE="$OUT/Ethereal-boot.repro.img"
PATCH_TMP="$(mktemp -d /tmp/ethereal-gki1-boot-patch.XXXXXX)"
PATCH_TMP_REAL="$(realpath -- "$PATCH_TMP")"
case "$PATCH_TMP_REAL" in
  /tmp/ethereal-gki1-boot-patch.*) ;;
  *) echo "refusing unsafe temporary patch root: $PATCH_TMP_REAL" >&2; exit 2 ;;
esac
cleanup_patch_tmp() {
  case "$PATCH_TMP_REAL" in
    /tmp/ethereal-gki1-boot-patch.*) rm -rf -- "$PATCH_TMP_REAL" ;;
  esac
}
trap cleanup_patch_tmp EXIT
if [[ "$(id -u)" -eq 0 ]]; then
  chown 65534:65534 "$PATCH_TMP_REAL"
fi
FIRST_WORK="$PATCH_TMP_REAL/work-first"
REPRO_WORK="$PATCH_TMP_REAL/work-repro"
FIRST_OUTPUT="$PATCH_TMP_REAL/Ethereal-boot.img"
REPRO_OUTPUT="$PATCH_TMP_REAL/Ethereal-boot.repro.img"
run_unprivileged_patch "$FIRST_WORK" "$FIRST_OUTPUT"
run_unprivileged_patch "$REPRO_WORK" "$REPRO_OUTPUT"
cmp -s "$FIRST_OUTPUT" "$REPRO_OUTPUT" || {
  echo "identical offline inputs produced different patched images" >&2
  exit 1
}
cp -f -- "$FIRST_OUTPUT" "$PATCHED_IMAGE"
cp -f -- "$REPRO_OUTPUT" "$REPRO_IMAGE"
cp -f -- "$FIRST_WORK/patch-identity.txt" "$OUT/patch-identity-first.txt"
cp -f -- "$REPRO_WORK/patch-identity.txt" "$OUT/patch-identity-repro.txt"

UNPACKED="$OUT/unpacked"
mkdir -p -- "$UNPACKED"
(
  cd "$UNPACKED"
  "$RAMTOOL" unpack "$PATCHED_IMAGE"
)

python3 - "$STOCK_IMAGE" "$PATCHED_IMAGE" <<'PY'
import struct
import sys
from pathlib import Path

original = Path(sys.argv[1]).read_bytes()
patched = Path(sys.argv[2]).read_bytes()
assert len(patched) == len(original), (len(original), len(patched))
assert original[:8] == patched[:8] == b"ANDROID!"
assert struct.unpack_from("<I", patched, 0x28)[0] == 3
assert struct.unpack_from("<I", patched, 0x08)[0] == struct.unpack_from("<I", original, 0x08)[0]
assert struct.unpack_from("<I", patched, 0x0C)[0] > struct.unpack_from("<I", original, 0x0C)[0]

footer = len(original) - 64
magic, major, minor, _, vbmeta_offset, vbmeta_size = struct.unpack_from(
    ">4sIIQQQ", original, footer
)
assert magic == b"AVBf" and (major, minor) == (1, 0)
assert vbmeta_offset + vbmeta_size <= footer
assert patched[vbmeta_offset:] == original[vbmeta_offset:]
assert patched[footer:footer + 4] == b"AVBf"
PY

grep -qw 'ethereal.fixture=gki1' "$UNPACKED/cmdline.txt"
grep -qw 'rdinit=/ethereal-init' "$UNPACKED/cmdline.txt"
test "$(grep -o 'rdinit=/ethereal-init' "$UNPACKED/cmdline.txt" | wc -l)" -eq 1
cmp -s "$KERNEL" "$UNPACKED/kernel"

cpio_exists() {
  "$RAMTOOL" cpio "$UNPACKED/ramdisk.cpio" "exists $1"
}

for entry in init ethereal-init ethereal.manager_uid ethereal.manager_token ethereal.patch_state ethereal-su; do
  cpio_exists "$entry"
done
for bundled_kmi in "${REQUIRED_KMIS[@]}"; do
  cpio_exists "ethereal.$bundled_kmi.ko"
done
if cpio_exists ethereal.ko; then
  echo "generic ethereal.ko unexpectedly present; exact KMI selection was bypassed" >&2
  exit 1
fi
if cpio_exists init.ethereal.bak; then
  echo "OEM init was unexpectedly replaced or hooked" >&2
  exit 1
fi

"$RAMTOOL" cpio "$UNPACKED/ramdisk.cpio" \
  "extract init $UNPACKED/init.extracted" \
  "extract ethereal-init $UNPACKED/ethereal-init.extracted" \
  "extract ethereal.$KMI.ko $UNPACKED/ethereal.$KMI.ko.extracted" \
  "extract ethereal.manager_uid $UNPACKED/manager-uid.extracted" \
  "extract ethereal.manager_token $UNPACKED/manager-token.extracted" \
  "extract ethereal.patch_state $UNPACKED/patch-state.extracted" \
  "extract ethereal-su $UNPACKED/su.extracted"
cmp -s "$ROOTFS/init" "$UNPACKED/init.extracted"
cmp -s "$ETHINIT" "$UNPACKED/ethereal-init.extracted"
cmp -s "$KO" "$UNPACKED/ethereal.$KMI.ko.extracted"
cmp -s "$MANAGER_TOKEN_FILE" "$UNPACKED/manager-token.extracted"
grep -qx 'mode=gki1-single' "$UNPACKED/patch-state.extracted"
cmp -s "$SU" "$UNPACKED/su.extracted"
for entry in su eth/su debug_ramdisk/su; do
  extracted="$UNPACKED/preserved-${entry//\//-}"
  "$RAMTOOL" cpio "$UNPACKED/ramdisk.cpio" "extract $entry $extracted"
  cmp -s "$ROOTFS/$entry" "$extracted"
done
test "$(cat "$UNPACKED/manager-uid.extracted")" = 2000
cpio -itv < "$UNPACKED/ramdisk.cpio" 2>/dev/null | \
  awk '$NF == "ethereal.manager_uid" { print $1 }' | grep -qx -- '-r--------'
cpio -itv < "$UNPACKED/ramdisk.cpio" 2>/dev/null | \
  awk '$NF == "ethereal.manager_token" { print $1 }' | grep -qx -- '-r--------'

CMDLINE="$(tr '\000\r\n' '   ' < "$UNPACKED/cmdline.txt")"
SERIAL="$OUT/serial.log"
set +e
timeout --signal=KILL 180s "$QEMU" \
  -machine virt,gic-version=3 \
  -cpu max,pauth-impdef=on \
  -m 1536 \
  -smp 2 \
  -no-reboot \
  -kernel "$UNPACKED/kernel" \
  -initrd "$UNPACKED/ramdisk.cpio" \
  -append "console=ttyAMA0 earlycon=pl011,0x9000000 $CMDLINE ignore_loglevel panic=1 nokaslr" \
  -serial "file:$SERIAL" \
  -monitor none \
  -display none
QEMU_RC=$?
set -e

if ! grep -Fq 'ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=PASS' "$SERIAL"; then
  echo "GKI 1.0 patched boot QEMU failed (qemu rc=$QEMU_RC)" >&2
  grep -E 'ethereal-stub:|ethereal-gki1-e2e:|ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=|Kernel panic' \
    "$SERIAL" | tail -100 >&2 || true
  exit 1
fi

require_serial_marker() {
  local marker="$1"
  if ! grep -Fq "$marker" "$SERIAL"; then
    echo "missing QEMU serial marker: $marker" >&2
    grep -E 'ethereal-stub:|ethereal:|ethereal-gki1-e2e:|ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=' \
      "$SERIAL" | tail -100 >&2 || true
    exit 1
  fi
}

require_serial_marker "ethereal-stub: osrelease=$OFFICIAL_RELEASE"
require_serial_marker "ethereal-stub: kmi=$KMI"
require_serial_marker "ethereal-stub: finit /ethereal.$KMI.ko"
require_serial_marker "ethereal-stub: loaded /ethereal.$KMI.ko"
require_serial_marker ' from /ethereal-su'
require_serial_marker 'ethereal: ready'
require_serial_marker 'ethereal-gki1-e2e: OEM init handoff OK'
require_serial_marker "ethereal-gki1-e2e: exact KMI $KMI KO load OK"
require_serial_marker 'ethereal-gki1-e2e: protocol and authorization OK'
if grep -Fq 'ethereal-stub: finit /ethereal.ko' "$SERIAL"; then
  echo "ethereal-init used the generic module path" >&2
  exit 1
fi

PATCHED_SHA256="$(sha256sum "$PATCHED_IMAGE" | awk '{ print $1 }')"
grep -E 'ethereal-stub: (osrelease|kmi|finit|loaded)|ethereal-gki1-e2e:|ETHEREAL_GKI1_BOOT_PATCH_E2E_RESULT=' \
  "$SERIAL"
echo "PASS: GKI 1.0 offline rootless boot patch KMI=$KMI image_sha256=$PATCHED_SHA256 qemu_rc=$QEMU_RC"
