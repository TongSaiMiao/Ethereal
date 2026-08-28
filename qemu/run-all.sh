#!/bin/bash
# Build and boot every supported GKI KMI. Physical partitions are never touched.
set -u
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"
LIST=(
  android12-5.4
  android12-5.10
  android13-5.10
  android13-5.15
  android14-5.15
  android14-6.1
  android15-6.6
  android16-6.12
)
pass=()
fail=()
mkdir -p "$ROOT/qemu/out"
bash "$ROOT/kmod/verify-prebuilt.sh"
echo "GKI matrix $(date -Is)" > "$ROOT/qemu/out/matrix.log"
for kmi in "${LIST[@]}"; do
  echo "======== $kmi ========"
  if bash "$ROOT/qemu/build-and-run.sh" "$kmi"; then
    pass+=("$kmi")
    echo "PASS $kmi" >> "$ROOT/qemu/out/matrix.log"
  else
    fail+=("$kmi")
    echo "FAIL $kmi" >> "$ROOT/qemu/out/matrix.log"
  fi
done
echo "======== summary ========"
echo "PASS: ${pass[*]:-none}"
echo "FAIL: ${fail[*]:-none}"
echo "PASS: ${pass[*]:-none}" >> "$ROOT/qemu/out/matrix.log"
echo "FAIL: ${fail[*]:-none}" >> "$ROOT/qemu/out/matrix.log"
[[ ${#fail[@]} -eq 0 ]]
