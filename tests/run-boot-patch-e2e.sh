#!/bin/bash
# Offline single init_boot hook + direct-install pair -> unpack audit -> QEMU.
# This never reads or writes a physical Android partition.
set -euo pipefail

KMI="${1:-android14-6.1}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"
ROOT="$(cd -- "$ROOT" && pwd -P)"
LOCKS="$ROOT/kmod/gki-locks.tsv"
OUT_BASE="$ROOT/tests/out/boot-patch-e2e"
OFFICIAL_CACHE="${GKI_OFFICIAL_CACHE:-/root/gki-official}"

for tool in awk basename realpath; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "missing required tool: $tool" >&2
    exit 2
  }
done
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
TARGETS="$ROOT/tests/out/targets"
PROVENANCE="$ROOT/kmod/prebuilt/$KMI/provenance.env"
KERNEL_SHA256=""
OFFICIAL_RELEASE=""
if [[ -z "${QEMU_KERNEL:-}" ]]; then
  [[ -s "$PROVENANCE" ]] || {
    echo "missing provenance: $PROVENANCE" >&2
    exit 2
  }
  KERNEL_ARTIFACT="$(awk -F '=' '$1 == "kernel_artifact" && !found {
    print substr($0, index($0, "=") + 1); found = 1
  }' "$PROVENANCE")"
  KERNEL_SHA256="$(awk -F '=' '$1 == "kernel_sha256" && !found {
    print substr($0, index($0, "=") + 1); found = 1
  }' "$PROVENANCE")"
  OFFICIAL_RELEASE="$(awk -F '=' '$1 == "official_release" && !found {
    print substr($0, index($0, "=") + 1); found = 1
  }' "$PROVENANCE")"
  [[ -n "$KERNEL_ARTIFACT" && "$KERNEL_ARTIFACT" == "$(basename -- "$KERNEL_ARTIFACT")" ]] || {
    echo "invalid official kernel artifact in $PROVENANCE" >&2
    exit 2
  }
  [[ "$KERNEL_SHA256" =~ ^[0-9a-f]{64}$ && -n "$OFFICIAL_RELEASE" ]] || {
    echo "incomplete official kernel identity in $PROVENANCE" >&2
    exit 2
  }
  QEMU_KERNEL="$OFFICIAL_CACHE/$KMI/$KERNEL_ARTIFACT"
fi
KERNEL="$QEMU_KERNEL"
KO="$ROOT/kmod/prebuilt/$KMI/ethereal.ko"
ETHINIT="${ETHEREAL_ETHINIT:-$ROOT/ethd/embedded/ethinit}"
SU="${ETHEREAL_SU:-$ROOT/app/src/main/assets/su}"
RUST_BIN="${RUST_BIN:-/opt/ethereal-rust/bin}"
CARGO="${CARGO:-$RUST_BIN/cargo}"
CARGO_HOME="${CARGO_HOME:-$ROOT/.tools/cargo}"
QEMU="${QEMU:-qemu-system-aarch64}"
MANAGER_TOKEN_FILE="$OUT/manager_token.bin"

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

for tool in "$CARGO" python3 cpio sha256sum strings "$QEMU"; do
  if ! command -v "$tool" >/dev/null 2>&1 && [[ ! -x "$tool" ]]; then
    echo "missing required tool: $tool" >&2
    exit 2
  fi
done
select_arm64_compiler
for file in "$KERNEL" "$KO"; do
  if [[ ! -f "$file" ]]; then
    echo "missing required input: $file" >&2
    exit 2
  fi
done
if [[ -n "$KERNEL_SHA256" ]]; then
  [[ "$(sha256sum "$KERNEL" | awk '{ print $1 }')" == "$KERNEL_SHA256" ]] || {
    echo "official kernel SHA-256 mismatch: $KERNEL" >&2
    exit 2
  }
  strings "$KERNEL" | awk -v expected="$OFFICIAL_RELEASE" '
    $0 == expected { found = 1 }
    END { exit !found }
  ' || {
    echo "official kernel release mismatch: $OFFICIAL_RELEASE" >&2
    exit 2
  }
fi

OUT_REAL="$(realpath -m -- "$OUT")"
[[ "$OUT_REAL" == "$OUT_BASE_ABS/$KMI" ]] || {
  echo "refusing unsafe output reset: $OUT_REAL" >&2
  exit 2
}
rm -rf -- "$OUT_REAL"
mkdir -p "$OUT" "$TARGETS"
PAYLOAD_OUT="$OUT/generated-payloads"
if [[ ! -s "$ETHINIT" ]]; then
  ETHINIT="$PAYLOAD_OUT/ethereal-init"
  build_test_ethinit "$ETHINIT"
fi
if [[ ! -s "$SU" ]]; then
  SU="$PAYLOAD_OUT/su"
  build_test_ethsu "$SU"
fi
python3 - "$MANAGER_TOKEN_FILE" <<'PY'
import sys
from pathlib import Path

Path(sys.argv[1]).write_bytes(bytes(range(1, 33)))
PY
chmod 600 "$MANAGER_TOKEN_FILE"

export PATH="$RUST_BIN:/usr/bin:/bin:$PATH"
export CARGO_HOME
export CARGO_TARGET_DIR="$TARGETS/ethd"
"$CARGO" build --manifest-path "$ROOT/ethd/Cargo.toml" --release --locked
ETHD="$CARGO_TARGET_DIR/release/ethd"
export CARGO_TARGET_DIR="$TARGETS/ramtool"
"$CARGO" build --manifest-path "$ROOT/ramtool/Cargo.toml" --release --locked
RAMTOOL="$CARGO_TARGET_DIR/release/ramtool"

ROOTFS="$OUT/stock-root"
mkdir -p "$ROOTFS"
"${ARM64_CC[@]}" -static -O2 -s "${ARM64_LD_FLAGS[@]}" \
  -Wl,--build-id=none \
  -o "$ROOTFS/init" "$ROOT/tests/boot-patch-e2e-init.c"
# The synthetic pid-1 fixture models the first-stage landmark present in AOSP
# init binaries so the production hook path is exercised end to end.
printf '%s\0' 'init first stage started!' >> "$ROOTFS/init"
mkdir -p "$ROOTFS/eth" "$ROOTFS/debug_ramdisk"
printf '%s\n' stock-root-su > "$ROOTFS/su"
printf '%s\n' stock-eth-su > "$ROOTFS/eth/su"
printf '%s\n' stock-debug-su > "$ROOTFS/debug_ramdisk/su"
printf '%s\n' stock-eth-keep > "$ROOTFS/eth/keep"
(
  cd "$ROOTFS"
  find . -print0 | cpio --null -o -H newc --quiet
) > "$OUT/stock.cpio" 2>/dev/null

python3 - "$OUT/stock.cpio" "$KERNEL" "$OUT/init_boot.img" "$OUT/boot.img" "$OUT/vendor_boot-v3.img" <<'PY'
import struct
import sys
from pathlib import Path

ramdisk_path, kernel_path, init_boot_path, boot_path, vendor_path = map(Path, sys.argv[1:])
ramdisk = ramdisk_path.read_bytes()
kernel = kernel_path.read_bytes()

def pad(data, page=4096):
    return data + bytes((-len(data)) % page)

def boot_image(kernel_blob, ramdisk_blob, cmdline=b""):
    if len(cmdline) >= 1536:
        raise ValueError("cmdline too long")
    header = bytearray(4096)
    header[:8] = b"ANDROID!"
    struct.pack_into("<I", header, 0x08, len(kernel_blob))
    struct.pack_into("<I", header, 0x0C, len(ramdisk_blob))
    struct.pack_into("<I", header, 0x14, 1584)
    struct.pack_into("<I", header, 0x28, 4)
    header[0x2C:0x2C + len(cmdline)] = cmdline
    return bytes(header) + pad(kernel_blob) + pad(ramdisk_blob)

def partition_image(body, slack=4 * 1024 * 1024):
    vbmeta_offset = ((len(body) + slack + 4095) // 4096) * 4096
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
    return bytes(image)

Path(init_boot_path).write_bytes(partition_image(boot_image(b"", ramdisk)))
Path(boot_path).write_bytes(partition_image(boot_image(kernel, b"")))

vendor = bytearray(4096)
vendor[:8] = b"VNDRBOOT"
struct.pack_into("<I", vendor, 0x08, 3)
struct.pack_into("<I", vendor, 0x0C, 4096)
struct.pack_into("<I", vendor, 0x18, len(ramdisk))
struct.pack_into("<I", vendor, 0x830, 2112)
Path(vendor_path).write_bytes(bytes(vendor) + pad(ramdisk))
PY

mkdir -p "$OUT/work-pair"
cp -f "$RAMTOOL" "$OUT/work-pair/ramtool"
cp -f "$SU" "$OUT/work-pair/su"
chmod 755 "$OUT/work-pair/ramtool" "$OUT/work-pair/su"
(
  cd "$OUT/work-pair"
  "$ETHD" boot-patch-pair \
    --init-boot "$OUT/init_boot.img" \
    --boot "$OUT/boot.img" \
    --out-init-boot "$OUT/Ethereal-init_boot.img" \
    --out-boot "$OUT/Ethereal-boot.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
)

mkdir -p "$OUT/work-vendor"
cp -f "$RAMTOOL" "$OUT/work-vendor/ramtool"
if (
  cd "$OUT/work-vendor"
  "$ETHD" boot-patch \
    --image "$OUT/vendor_boot-v3.img" \
    --out "$OUT/should-not-exist.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/vendor-reject.log" 2>&1; then
  echo "vendor_boot v3 was incorrectly accepted" >&2
  exit 1
fi
grep -q "vendor_boot is not a standalone Ethereal patch target" "$OUT/vendor-reject.log"
test ! -e "$OUT/should-not-exist.img"

unpack_image() {
  local image="$1" unpacked="$2"
  mkdir -p "$unpacked"
  (
    cd "$unpacked"
    "$RAMTOOL" unpack "$image"
  )
}

verify_fixed_tail() {
  local original="$1" patched="$2"
  python3 - "$original" "$patched" <<'PY'
import struct
import sys
from pathlib import Path

original = Path(sys.argv[1]).read_bytes()
patched = Path(sys.argv[2]).read_bytes()
assert len(patched) == len(original)
footer = len(original) - 64
magic, _, _, _, vbmeta_offset, vbmeta_size = struct.unpack_from(">4sIIQQQ", original, footer)
assert magic == b"AVBf"
assert vbmeta_offset + vbmeta_size <= footer
assert patched[vbmeta_offset:] == original[vbmeta_offset:]
PY
}

verify_init_boot() {
  local image="$1" unpacked="$2"
  unpack_image "$image" "$unpacked"
  ! grep -qw "rdinit=/ethereal-init" "$unpacked/cmdline.txt"
  test ! -e "$unpacked/kernel"
  for entry in init ethereal-init ethereal.manager_uid ethereal.manager_token ethereal.patch_state ethereal.ko ethereal-su; do
    "$RAMTOOL" cpio "$unpacked/ramdisk.cpio" "exists $entry"
  done
  if "$RAMTOOL" cpio "$unpacked/ramdisk.cpio" "exists init.ethereal.bak"; then
    echo "paired init_boot unexpectedly used the offline ELF hook" >&2
    exit 1
  fi
  legacy_su='r''p/su'
  if "$RAMTOOL" cpio "$unpacked/ramdisk.cpio" "exists $legacy_su"; then
    echo "legacy su path unexpectedly present in patched ramdisk" >&2
    exit 1
  fi
  for bundled_kmi in \
    android12-5.4 android12-5.10 android13-5.10 android13-5.15 \
    android14-5.15 android14-6.1 android15-6.6 android16-6.12; do
    "$RAMTOOL" cpio "$unpacked/ramdisk.cpio" "exists ethereal.$bundled_kmi.ko"
  done
  "$RAMTOOL" cpio "$unpacked/ramdisk.cpio" \
    "extract init $unpacked/init.extracted" \
    "extract ethereal.manager_uid $unpacked/manager_uid.extracted" \
    "extract ethereal.manager_token $unpacked/manager_token.extracted" \
    "extract ethereal.patch_state $unpacked/patch_state.extracted" \
    "extract ethereal.ko $unpacked/ethereal.ko.extracted" \
    "extract ethereal-su $unpacked/su.extracted"
  cmp -s "$ROOTFS/init" "$unpacked/init.extracted"
  cmp -s "$KO" "$unpacked/ethereal.ko.extracted"
  cmp -s "$SU" "$unpacked/su.extracted"
  test "$(cat "$unpacked/manager_uid.extracted")" = "2000"
  cmp -s "$MANAGER_TOKEN_FILE" "$unpacked/manager_token.extracted"
  grep -qx 'mode=gki2-pair' "$unpacked/patch_state.extracted"
  verify_preserved_su_entries "$unpacked"
  cpio -itv < "$unpacked/ramdisk.cpio" 2>/dev/null | \
    awk '$NF == "ethereal.manager_uid" { print $1 }' | grep -qx -- "-r--------"
  cpio -itv < "$unpacked/ramdisk.cpio" 2>/dev/null | \
    awk '$NF == "ethereal.manager_token" { print $1 }' | grep -qx -- "-r--------"
  verify_fixed_tail "$OUT/init_boot.img" "$image"
}

verify_preserved_su_entries() {
  local unpacked="$1" entry extracted
  for entry in su eth/su debug_ramdisk/su eth/keep; do
    extracted="$unpacked/preserved-${entry//\//-}"
    "$RAMTOOL" cpio "$unpacked/ramdisk.cpio" "extract $entry $extracted"
    cmp -s "$ROOTFS/$entry" "$extracted"
  done
}

verify_boot() {
  local image="$1" unpacked="$2"
  unpack_image "$image" "$unpacked"
  grep -qw "rdinit=/ethereal-init" "$unpacked/cmdline.txt"
  test "$(grep -o "rdinit=/ethereal-init" "$unpacked/cmdline.txt" | wc -l)" -eq 1
  cmp -s "$KERNEL" "$unpacked/kernel"
  test ! -e "$unpacked/ramdisk.cpio"
  verify_fixed_tail "$OUT/boot.img" "$image"
  python3 - "$OUT/boot.img" "$image" <<'PY'
import sys
from pathlib import Path

original = Path(sys.argv[1]).read_bytes()
patched = Path(sys.argv[2]).read_bytes()
assert len(patched) == len(original)
cmdline = range(0x2C, 0x2C + 1536)
assert all(original[i] == patched[i] for i in range(len(original)) if i not in cmdline)
assert patched[-64:-60] == b"AVBf"
PY
}

verify_single_init_boot() {
  local image="$1" unpacked="$2"
  unpack_image "$image" "$unpacked"
  ! grep -qw "rdinit=/ethereal-init" "$unpacked/cmdline.txt"
  test ! -e "$unpacked/kernel"
  for entry in \
    init init.ethereal.bak ethereal-init ethereal.manager_uid \
    ethereal.manager_token ethereal.patch_state ethereal.ko ethereal-su; do
    "$RAMTOOL" cpio "$unpacked/ramdisk.cpio" "exists $entry"
  done
  "$RAMTOOL" cpio "$unpacked/ramdisk.cpio" \
    "extract init $unpacked/init.hooked" \
    "extract init.ethereal.bak $unpacked/init.original" \
    "extract ethereal.manager_uid $unpacked/manager_uid.extracted" \
    "extract ethereal.manager_token $unpacked/manager_token.extracted" \
    "extract ethereal.patch_state $unpacked/patch_state.extracted" \
    "extract ethereal.ko $unpacked/ethereal.ko.extracted" \
    "extract ethereal-su $unpacked/su.extracted"
  cmp -s "$ROOTFS/init" "$unpacked/init.original"
  if cmp -s "$ROOTFS/init" "$unpacked/init.hooked"; then
    echo "single init_boot left /init unhooked" >&2
    exit 1
  fi
  cmp -s "$KO" "$unpacked/ethereal.ko.extracted"
  cmp -s "$SU" "$unpacked/su.extracted"
  test "$(cat "$unpacked/manager_uid.extracted")" = "2000"
  cmp -s "$MANAGER_TOKEN_FILE" "$unpacked/manager_token.extracted"
  grep -qx 'mode=gki2-single' "$unpacked/patch_state.extracted"
  verify_preserved_su_entries "$unpacked"
  python3 - "$unpacked/init.original" "$unpacked/init.hooked" <<'PY'
import struct
import sys
from pathlib import Path

original = Path(sys.argv[1]).read_bytes()
hooked = Path(sys.argv[2]).read_bytes()

def elf64_layout(data):
    assert data[:4] == b"\x7fELF" and data[4] == 2 and data[5] == 1
    entry = struct.unpack_from("<Q", data, 24)[0]
    phoff = struct.unpack_from("<Q", data, 32)[0]
    phentsize = struct.unpack_from("<H", data, 54)[0]
    phnum = struct.unpack_from("<H", data, 56)[0]
    loads = sum(
        struct.unpack_from("<I", data, phoff + index * phentsize)[0] == 1
        for index in range(phnum)
    )
    return entry, loads

original_entry, original_loads = elf64_layout(original)
hooked_entry, hooked_loads = elf64_layout(hooked)
assert hooked_entry != original_entry
assert hooked_loads == original_loads + 1
assert b"ETHRL01\0" in hooked
assert b"ETHRL01\0" not in original
PY
  cpio -itv < "$unpacked/ramdisk.cpio" 2>/dev/null | \
    awk '$NF == "ethereal.manager_uid" { print $1 }' | grep -qx -- "-r--------"
  cpio -itv < "$unpacked/ramdisk.cpio" 2>/dev/null | \
    awk '$NF == "ethereal.manager_token" { print $1 }' | grep -qx -- "-r--------"
  verify_fixed_tail "$OUT/init_boot.img" "$image"
}

# v0.1.1 left no ownership receipt behind. Rebuild its exact luggage here so
# migration has to identify the old layout from real CPIO entries, not a mock.
LEGACY_BUILD="$OUT/work-legacy-v011-build"
mkdir -p "$LEGACY_BUILD"
cp -f "$RAMTOOL" "$LEGACY_BUILD/ramtool"
printf '%s\n' 2000 > "$LEGACY_BUILD/manager_uid"
(
  cd "$LEGACY_BUILD"
  ./ramtool unpack "$OUT/init_boot.img"
  ./ramtool cpio ramdisk.cpio \
    "add 0755 ethereal-init $ETHINIT" \
    "add 0400 ethereal.manager_uid $LEGACY_BUILD/manager_uid" \
    "add 0400 ethereal.manager_token $MANAGER_TOKEN_FILE" \
    "add 0755 ethereal.ko $KO"
  ./ramtool repack "$OUT/init_boot.img" "$OUT/legacy-v011-init_boot.img"
)
for absent in ethereal.patch_state ethereal-su init.ethereal.bak; do
  if "$RAMTOOL" cpio "$LEGACY_BUILD/ramdisk.cpio" "exists $absent"; then
    echo "legacy v0.1.1 fixture unexpectedly contains $absent" >&2
    exit 1
  fi
done
# Shared paths are somebody else's drawer. A migration may replace Ethereal's
# own payload, but it does not get to tidy /su or the stock eth directories.
verify_preserved_su_entries "$LEGACY_BUILD"

LEGACY_PATCH="$OUT/work-legacy-v011-patch"
mkdir -p "$LEGACY_PATCH"
cp -f "$RAMTOOL" "$LEGACY_PATCH/ramtool"
cp -f "$SU" "$LEGACY_PATCH/su"
chmod 755 "$LEGACY_PATCH/ramtool" "$LEGACY_PATCH/su"
(
  cd "$LEGACY_PATCH"
  "$ETHD" boot-patch-pair \
    --init-boot "$OUT/legacy-v011-init_boot.img" \
    --boot "$OUT/boot.img" \
    --out-init-boot "$OUT/Ethereal-legacy-v011-init_boot.img" \
    --out-boot "$OUT/Ethereal-legacy-v011-boot.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/legacy-v011-migrate.log" 2>&1
grep -q "migrating complete v0.1.1 ramdisk layout" \
  "$OUT/legacy-v011-migrate.log"
verify_init_boot \
  "$OUT/Ethereal-legacy-v011-init_boot.img" "$OUT/unpack-legacy-v011"
verify_boot \
  "$OUT/Ethereal-legacy-v011-boot.img" "$OUT/unpack-legacy-v011-boot"
grep -qx 'format=2' "$OUT/unpack-legacy-v011/patch_state.extracted"

# A couple of familiar filenames are not ownership. This half-patched fixture
# must be rejected before either member of the pair becomes visible.
INCOMPLETE_BUILD="$OUT/work-legacy-incomplete-build"
mkdir -p "$INCOMPLETE_BUILD"
cp -f "$RAMTOOL" "$INCOMPLETE_BUILD/ramtool"
(
  cd "$INCOMPLETE_BUILD"
  ./ramtool unpack "$OUT/init_boot.img"
  ./ramtool cpio ramdisk.cpio \
    "add 0755 ethereal-init $ETHINIT" \
    "add 0755 ethereal.ko $KO"
  ./ramtool repack "$OUT/init_boot.img" "$OUT/legacy-incomplete-init_boot.img"
)
INCOMPLETE_PATCH="$OUT/work-legacy-incomplete-patch"
mkdir -p "$INCOMPLETE_PATCH"
cp -f "$RAMTOOL" "$INCOMPLETE_PATCH/ramtool"
cp -f "$SU" "$INCOMPLETE_PATCH/su"
if (
  cd "$INCOMPLETE_PATCH"
  "$ETHD" boot-patch-pair \
    --init-boot "$OUT/legacy-incomplete-init_boot.img" \
    --boot "$OUT/boot.img" \
    --out-init-boot "$OUT/legacy-incomplete-init-should-not-exist.img" \
    --out-boot "$OUT/legacy-incomplete-boot-should-not-exist.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/legacy-incomplete-reject.log" 2>&1; then
  echo "incomplete v0.1.1 layout was incorrectly claimed by Ethereal" >&2
  exit 1
fi
grep -q "already contains ethereal-init without an Ethereal ownership state" \
  "$OUT/legacy-incomplete-reject.log"
test ! -e "$OUT/legacy-incomplete-init-should-not-exist.img"
test ! -e "$OUT/legacy-incomplete-boot-should-not-exist.img"

verify_init_boot "$OUT/Ethereal-init_boot.img" "$OUT/unpack-init"
verify_boot "$OUT/Ethereal-boot.img" "$OUT/unpack-boot"

cp -f "$OUT/boot.img" "$OUT/boot-before-single-init.img"
mkdir -p "$OUT/work-single-init"
cp -f "$RAMTOOL" "$OUT/work-single-init/ramtool"
cp -f "$SU" "$OUT/work-single-init/su"
chmod 755 "$OUT/work-single-init/ramtool" "$OUT/work-single-init/su"
(
  cd "$OUT/work-single-init"
  "$ETHD" boot-patch \
    --image "$OUT/init_boot.img" \
    --out "$OUT/Ethereal-single-init_boot.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/single-init.log" 2>&1
grep -q "GKI 2.0 single init_boot; root /init entry hook" "$OUT/single-init.log"
grep -q "HOOKED_INITS    \[1\]" "$OUT/single-init.log"
cmp -s "$OUT/boot-before-single-init.img" "$OUT/boot.img" || {
  echo "single init_boot patch changed the stock boot image" >&2
  exit 1
}
verify_single_init_boot \
  "$OUT/Ethereal-single-init_boot.img" "$OUT/unpack-single-init"

mkdir -p "$OUT/work-single-boot-reject"
cp -f "$RAMTOOL" "$OUT/work-single-boot-reject/ramtool"
if (
  cd "$OUT/work-single-boot-reject"
  "$ETHD" boot-patch \
    --image "$OUT/boot.img" \
    --out "$OUT/single-boot-should-not-exist.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/single-boot-reject.log" 2>&1; then
  echo "single-image GKI 2.0 kernel-only boot was incorrectly accepted" >&2
  exit 1
fi
grep -q "selected boot image is kernel-only; patch its matching init_boot image instead" \
  "$OUT/single-boot-reject.log"
test ! -e "$OUT/single-boot-should-not-exist.img"
cmp -s "$OUT/boot-before-single-init.img" "$OUT/boot.img"

for missing in init_boot boot; do
  work="$OUT/work-missing-$missing"
  out_init="$OUT/missing-$missing-init-should-not-exist.img"
  out_boot="$OUT/missing-$missing-boot-should-not-exist.img"
  mkdir -p "$work"
  cp -f "$RAMTOOL" "$work/ramtool"
  cp -f "$SU" "$work/su"
  init_input="$OUT/init_boot.img"
  boot_input="$OUT/boot.img"
  if [[ "$missing" == init_boot ]]; then
    init_input="$OUT/does-not-exist-init_boot.img"
  else
    boot_input="$OUT/does-not-exist-boot.img"
  fi
  if (
    cd "$work"
    "$ETHD" boot-patch-pair \
      --init-boot "$init_input" \
      --boot "$boot_input" \
      --out-init-boot "$out_init" \
      --out-boot "$out_boot" \
      --manager-uid 2000 \
      --manager-token-file "$MANAGER_TOKEN_FILE" \
      --ethinit "$ETHINIT" \
      --ko "$KO"
  ) >"$OUT/missing-$missing-reject.log" 2>&1; then
    echo "pair patch accepted missing $missing" >&2
    exit 1
  fi
  test ! -e "$out_init"
  test ! -e "$out_boot"
done

python3 - "$OUT/boot.img" "$OUT/conflicting-boot.img" <<'PY'
import sys
from pathlib import Path

image = bytearray(Path(sys.argv[1]).read_bytes())
image[0x2C:0x2C + 1536] = bytes(1536)
conflict = b"console=ttyAMA0 rdinit=/init"
image[0x2C:0x2C + len(conflict)] = conflict
Path(sys.argv[2]).write_bytes(image)
PY
mkdir -p "$OUT/work-conflict"
cp -f "$RAMTOOL" "$OUT/work-conflict/ramtool"
cp -f "$SU" "$OUT/work-conflict/su"
if (
  cd "$OUT/work-conflict"
  "$ETHD" boot-patch-pair \
    --init-boot "$OUT/init_boot.img" \
    --boot "$OUT/conflicting-boot.img" \
    --out-init-boot "$OUT/conflict-init-should-not-exist.img" \
    --out-boot "$OUT/conflict-boot-should-not-exist.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/conflict-reject.log" 2>&1; then
  echo "pair patch replaced a conflicting rdinit" >&2
  exit 1
fi
grep -q "conflicting rdinit=/init" "$OUT/conflict-reject.log"
test ! -e "$OUT/conflict-init-should-not-exist.img"
test ! -e "$OUT/conflict-boot-should-not-exist.img"

python3 - "$OUT/init_boot.img" "$OUT/foreign-rdinit-init_boot.img" <<'PY'
import sys
from pathlib import Path

image = bytearray(Path(sys.argv[1]).read_bytes())
image[0x2C:0x2C + 1536] = bytes(1536)
conflict = b"rdinit=/foreign-init"
image[0x2C:0x2C + len(conflict)] = conflict
Path(sys.argv[2]).write_bytes(image)
PY
mkdir -p "$OUT/work-foreign-rdinit"
cp -f "$RAMTOOL" "$OUT/work-foreign-rdinit/ramtool"
cp -f "$SU" "$OUT/work-foreign-rdinit/su"
if (
  cd "$OUT/work-foreign-rdinit"
  "$ETHD" boot-patch \
    --image "$OUT/foreign-rdinit-init_boot.img" \
    --out "$OUT/foreign-rdinit-should-not-exist.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/foreign-rdinit-reject.log" 2>&1; then
  echo "single init_boot patch accepted a foreign rdinit" >&2
  exit 1
fi
grep -q "already defines rdinit=/foreign-init" "$OUT/foreign-rdinit-reject.log"
test ! -e "$OUT/foreign-rdinit-should-not-exist.img"

mkdir -p "$OUT/work-reserved-collision"
cp -f "$RAMTOOL" "$OUT/work-reserved-collision/ramtool"
cp -f "$SU" "$OUT/work-reserved-collision/su"
printf '%s\n' foreign > "$OUT/work-reserved-collision/collision"
(
  cd "$OUT/work-reserved-collision"
  ./ramtool unpack "$OUT/init_boot.img"
  ./ramtool cpio ramdisk.cpio \
    "add 0400 ethereal.manager_uid collision"
  ./ramtool repack "$OUT/init_boot.img" "$OUT/reserved-collision-init_boot.img"
)
if (
  cd "$OUT/work-reserved-collision"
  "$ETHD" boot-patch \
    --image "$OUT/reserved-collision-init_boot.img" \
    --out "$OUT/reserved-collision-should-not-exist.img" \
    --manager-uid 2000 \
    --manager-token-file "$MANAGER_TOKEN_FILE" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/reserved-collision-reject.log" 2>&1; then
  echo "patch overwrote a foreign Ethereal-reserved path" >&2
  exit 1
fi
grep -q "already contains ethereal.manager_uid without an Ethereal ownership state" \
  "$OUT/reserved-collision-reject.log"
test ! -e "$OUT/reserved-collision-should-not-exist.img"

head -c 31 "$MANAGER_TOKEN_FILE" > "$OUT/invalid-token.bin"
mkdir -p "$OUT/work-invalid-token"
cp -f "$RAMTOOL" "$OUT/work-invalid-token/ramtool"
cp -f "$SU" "$OUT/work-invalid-token/su"
if (
  cd "$OUT/work-invalid-token"
  "$ETHD" boot-patch-pair \
    --init-boot "$OUT/init_boot.img" \
    --boot "$OUT/boot.img" \
    --out-init-boot "$OUT/invalid-token-init-should-not-exist.img" \
    --out-boot "$OUT/invalid-token-boot-should-not-exist.img" \
    --manager-uid 2000 \
    --manager-token-file "$OUT/invalid-token.bin" \
    --ethinit "$ETHINIT" \
    --ko "$KO"
) >"$OUT/invalid-token-reject.log" 2>&1; then
  echo "31-byte manager token was incorrectly accepted" >&2
  exit 1
fi
grep -q "manager token file must contain exactly 32 bytes" "$OUT/invalid-token-reject.log"
test ! -e "$OUT/invalid-token-init-should-not-exist.img"
test ! -e "$OUT/invalid-token-boot-should-not-exist.img"

mkdir -p "$OUT/work-stock-unpatch"
cp -f "$RAMTOOL" "$OUT/work-stock-unpatch/ramtool"
if (
  cd "$OUT/work-stock-unpatch"
  "$ETHD" boot-unpatch \
    --image "$OUT/init_boot.img" \
    --out "$OUT/stock-unpatch-should-not-exist.img"
) >"$OUT/stock-unpatch-reject.log" 2>&1; then
  echo "unpatch accepted an image without Ethereal ownership state" >&2
  exit 1
fi
grep -q "no supported Ethereal ownership state" "$OUT/stock-unpatch-reject.log"
test ! -e "$OUT/stock-unpatch-should-not-exist.img"

mkdir -p "$OUT/work-single-unpatch" "$OUT/unpack-single-restored"
cp -f "$RAMTOOL" "$OUT/work-single-unpatch/ramtool"
(
  cd "$OUT/work-single-unpatch"
  "$ETHD" boot-unpatch \
    --image "$OUT/Ethereal-single-init_boot.img" \
    --out "$OUT/restored-single-init_boot.img"
)
unpack_image "$OUT/restored-single-init_boot.img" "$OUT/unpack-single-restored"
! grep -qw "rdinit=/ethereal-init" "$OUT/unpack-single-restored/cmdline.txt"
test ! -e "$OUT/unpack-single-restored/kernel"
"$RAMTOOL" cpio "$OUT/unpack-single-restored/ramdisk.cpio" \
  "extract init $OUT/unpack-single-restored/init.extracted"
cmp -s "$ROOTFS/init" "$OUT/unpack-single-restored/init.extracted"
verify_preserved_su_entries "$OUT/unpack-single-restored"
for removed in \
  init.ethereal.bak ethereal-init ethereal.manager_uid ethereal.manager_token \
  ethereal.patch_state ethereal.ko ethereal-su; do
  if "$RAMTOOL" cpio "$OUT/unpack-single-restored/ramdisk.cpio" "exists $removed"; then
    echo "single init_boot unpatch left $removed in ramdisk" >&2
    exit 1
  fi
done
for bundled_kmi in \
  android12-5.4 android12-5.10 android13-5.10 android13-5.15 \
  android14-5.15 android14-6.1 android15-6.6 android16-6.12; do
  if "$RAMTOOL" cpio "$OUT/unpack-single-restored/ramdisk.cpio" \
      "exists ethereal.$bundled_kmi.ko"; then
    echo "single init_boot unpatch left ethereal.$bundled_kmi.ko" >&2
    exit 1
  fi
done
verify_fixed_tail "$OUT/init_boot.img" "$OUT/restored-single-init_boot.img"

mkdir -p "$OUT/work-pair-unpatch" "$OUT/unpack-pair-restored-boot"
cp -f "$RAMTOOL" "$OUT/work-pair-unpatch/ramtool"
(
  cd "$OUT/work-pair-unpatch"
  "$ETHD" boot-unpatch \
    --image "$OUT/Ethereal-boot.img" \
    --out "$OUT/restored-pair-boot.img"
)
unpack_image "$OUT/restored-pair-boot.img" "$OUT/unpack-pair-restored-boot"
! grep -qw "rdinit=/ethereal-init" "$OUT/unpack-pair-restored-boot/cmdline.txt"
test ! -e "$OUT/unpack-pair-restored-boot/ramdisk.cpio"
verify_fixed_tail "$OUT/boot.img" "$OUT/restored-pair-boot.img"

unpack_image "$OUT/boot.img" "$OUT/unpack-stock-boot"
PAIR_CMDLINE="$(tr '\000\r\n' '   ' < "$OUT/unpack-boot/cmdline.txt")"
SINGLE_CMDLINE="$(tr '\000\r\n' '   ' < "$OUT/unpack-stock-boot/cmdline.txt")"
grep -qw "rdinit=/ethereal-init" "$OUT/unpack-boot/cmdline.txt"
! grep -qw "rdinit=/ethereal-init" "$OUT/unpack-stock-boot/cmdline.txt"

run_qemu_handoff() {
  local label="$1" initrd="$2" cmdline="$3" serial="$4" qemu_rc
  set +e
  timeout --signal=KILL 180s "$QEMU" \
    -machine virt,gic-version=3 \
    -cpu max,pauth-impdef=on \
    -m 1536 \
    -smp 2 \
    -no-reboot \
    -kernel "$KERNEL" \
    -initrd "$initrd" \
    -append "console=ttyAMA0 earlycon=pl011,0x9000000 $cmdline ignore_loglevel panic=1 nokaslr" \
    -serial "file:$serial" \
    -monitor none \
    -display none
  qemu_rc=$?
  set -e

  if ! grep -q "ETHEREAL_BOOT_PATCH_E2E_RESULT=PASS" "$serial"; then
    echo "QEMU $label handoff failed (qemu rc=$qemu_rc)" >&2
    grep -E "ethereal-stub:|ethereal-boot-e2e:|ETHEREAL_BOOT_PATCH_E2E_RESULT=|Kernel panic" \
      "$serial" | tail -80 >&2 || true
    exit 1
  fi
  echo "QEMU $label handoff PASS (qemu rc=$qemu_rc)"
  grep -E "ethereal-stub:|ethereal-boot-e2e:|ETHEREAL_BOOT_PATCH_E2E_RESULT=" "$serial"
}

run_qemu_handoff \
  "paired rdinit" "$OUT/unpack-init/ramdisk.cpio" "$PAIR_CMDLINE" "$OUT/serial.log"
grep -Fq ' from /ethereal-su' "$OUT/serial.log"
run_qemu_handoff \
  "single init_boot ELF hook" "$OUT/unpack-single-init/ramdisk.cpio" \
  "$SINGLE_CMDLINE" "$OUT/serial-single-init.log"
grep -Fq ' from /ethereal-su' "$OUT/serial-single-init.log"
grep -q "ethereal-stub: hooked FirstStageMain" "$OUT/serial-single-init.log"
grep -q "ethereal-boot-e2e: OEM init handoff OK" "$OUT/serial-single-init.log"

echo "PASS: single GKI 2.0 init_boot hook, kernel-only boot rejection, Direct pair transaction, unpatch restore, and both QEMU handoffs"
