#!/bin/bash
# Boot one official GKI Image + ethereal.ko in QEMU virt. No Android userspace.
set -euo pipefail
KMI="${1:?usage: run-gki.sh android<release>-<kernel>}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"
ROOT="$(cd -- "$ROOT" && pwd -P)"
LOCKS="$ROOT/kmod/gki-locks.tsv"

[[ "$KMI" =~ ^android[0-9]+-[0-9]+\.[0-9]+$ ]] &&
  awk -F '\t' -v wanted="$KMI" '
    NR > 1 && $1 == wanted { found = 1; exit }
    END { exit !found }
  ' "$LOCKS" || {
    echo "KMI is not pinned in $LOCKS: $KMI" >&2
    exit 2
  }

OUT="$ROOT/qemu/out/$KMI"
PROVENANCE="$ROOT/kmod/prebuilt/$KMI/provenance.env"
OFFICIAL_CACHE="${GKI_OFFICIAL_CACHE:-/root/gki-official}"
if [[ -z "${QEMU_KERNEL:-}" ]]; then
  [[ -s "$PROVENANCE" ]] || {
    echo "missing provenance: $PROVENANCE" >&2
    exit 1
  }
  KERNEL_ARTIFACT="$(awk -F '=' '$1 == "kernel_artifact" && !found {
    print substr($0, index($0, "=") + 1); found = 1
  }' "$PROVENANCE")"
  QEMU_KERNEL="$OFFICIAL_CACHE/$KMI/$KERNEL_ARTIFACT"
fi
KERNEL="$QEMU_KERNEL"
INITRD="$OUT/initramfs.cpio"
QEMU="${QEMU:-qemu-system-aarch64}"
CPU="${QEMU_CPU:-max,pauth-impdef=on}"
MEM="${QEMU_MEM:-1536}"

if [[ ! -f "$KERNEL" ]]; then
  echo "missing official kernel Image: $KERNEL" >&2
  exit 1
fi
if [[ ! -f "$INITRD" ]]; then
  echo "missing initrd: $INITRD (run pack-initramfs.sh $KMI)" >&2
  exit 1
fi

echo ">> QEMU $KMI cpu=$CPU mem=$MEM kernel=$KERNEL"

exec "$QEMU" \
  -machine virt,gic-version=3 \
  -cpu "$CPU" \
  -m "$MEM" \
  -smp 2 \
  -nographic \
  -no-reboot \
  -kernel "$KERNEL" \
  -initrd "$INITRD" \
  -append "console=ttyAMA0 earlycon=pl011,0x9000000 rdinit=/init ignore_loglevel panic=1 nokaslr" \
  -serial mon:stdio \
  -display none
