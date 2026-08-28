#!/bin/bash
set -euo pipefail
KMI="${1:?usage: pack-initramfs.sh android<release>-<kernel> [ethereal.ko]}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"
ROOT="$(cd -- "$ROOT" && pwd -P)"
LOCKS="$ROOT/kmod/gki-locks.tsv"
OUT_BASE="$ROOT/qemu/out"

validate_kmi() {
  local candidate="$1"

  [[ "$candidate" =~ ^android[0-9]+-[0-9]+\.[0-9]+$ ]] || {
    echo "invalid KMI: $candidate" >&2
    return 1
  }
  awk -F '\t' -v wanted="$candidate" '
    NR > 1 && $1 == wanted { found = 1; exit }
    END { exit !found }
  ' "$LOCKS" || {
    echo "KMI is not pinned in $LOCKS: $candidate" >&2
    return 1
  }
}

safe_reset_child_dir() {
  local target="$1" base="$2" target_abs base_abs expected

  target_abs="$(realpath -m -- "$target")"
  base_abs="$(realpath -m -- "$base")"
  expected="$base_abs/$(basename -- "$target")"
  [[ "$target_abs" == "$expected" && "$target_abs" != "$base_abs" ]] || {
    echo "refusing unsafe output reset: $target_abs" >&2
    return 1
  }
  rm -rf -- "$target_abs"
  mkdir -p -- "$target_abs"
}

for tool in awk basename cpio find mktemp realpath; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "missing required tool: $tool" >&2
    exit 2
  }
done
[[ -s "$LOCKS" ]] || { echo "missing GKI lock file: $LOCKS" >&2; exit 2; }
validate_kmi "$KMI" || exit 2

mkdir -p -- "$OUT_BASE"
OUT_BASE_ABS="$(cd -- "$OUT_BASE" && pwd -P)"
[[ "$OUT_BASE_ABS" == "$OUT_BASE" ]] || {
  echo "refusing symlinked QEMU output root: $OUT_BASE -> $OUT_BASE_ABS" >&2
  exit 2
}
OUT="$(realpath -m -- "$OUT_BASE/$KMI")"
[[ "$OUT" == "$OUT_BASE_ABS/$KMI" ]] || {
  echo "refusing unsafe KMI output path: $OUT" >&2
  exit 2
}
KO="${2:-$OUT/ethereal.ko}"
INIT="$ROOT/qemu/out/init"
ROOTFS="$OUT/root"
INITRAMFS="$OUT/initramfs.cpio"
temp_initramfs=""

cleanup() {
  [[ -z "$temp_initramfs" ]] || rm -f -- "$temp_initramfs"
}
trap cleanup EXIT

mkdir -p -- "$OUT"
if [[ ! -x "$INIT" ]]; then
  echo "missing $INIT (build qemu/init.c first)" >&2
  exit 1
fi
if [[ ! -f "$KO" ]]; then
  echo "missing $KO" >&2
  exit 1
fi
safe_reset_child_dir "$ROOTFS" "$OUT"
cp -a -- "$INIT" "$ROOTFS/init"
cp -a -- "$KO" "$ROOTFS/ethereal.ko"
chmod 755 "$ROOTFS/init"
temp_initramfs="$(mktemp "$OUT/.initramfs.cpio.XXXXXX")"
( cd "$ROOTFS" && find . -print0 | cpio --null -o -H newc ) > "$temp_initramfs" 2>/dev/null
mv -f -- "$temp_initramfs" "$INITRAMFS"
temp_initramfs=""
echo "packed $INITRAMFS ko=$(wc -c < "$KO") initrd=$(wc -c < "$INITRAMFS")"
