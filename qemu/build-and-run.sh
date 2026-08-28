#!/bin/bash
# Boot the exact official GKI Image with the unchanged release ethereal.ko.
# Usage: bash qemu/build-and-run.sh android14-6.1
set -euo pipefail

KMI="${1:?usage: build-and-run.sh android<release>-<kernel>}"
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"
ROOT="$(cd -- "$ROOT" && pwd -P)"
LOCKS="$ROOT/kmod/gki-locks.tsv"
OFFICIAL_CACHE="${GKI_OFFICIAL_CACHE:-/root/gki-official}"

[[ "$KMI" =~ ^android[0-9]+-[0-9]+\.[0-9]+$ ]] &&
  awk -F '\t' -v wanted="$KMI" '
    NR > 1 && $1 == wanted { found = 1; exit }
    END { exit !found }
  ' "$LOCKS" || {
    echo "KMI is not pinned in $LOCKS: $KMI" >&2
    exit 2
  }

OUT="$ROOT/qemu/out/$KMI"
RELEASE_KO="$ROOT/kmod/prebuilt/$KMI/ethereal.ko"
PROVENANCE="$ROOT/kmod/prebuilt/$KMI/provenance.env"
CANONICAL_SYMVERS="$ROOT/kmod/prebuilt/$KMI/Module.symvers"

log() { printf '>> %s\n' "$*"; }

provenance_value() {
  local key="$1"
  awk -F '=' -v wanted="$key" '
    $1 == wanted && !found {
      print substr($0, index($0, "=") + 1)
      found = 1
    }
  ' "$PROVENANCE"
}

for tool in awk aarch64-linux-gnu-gcc file modinfo sha256sum strings; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "missing required tool: $tool" >&2
    exit 2
  }
done

for file in "$RELEASE_KO" "$PROVENANCE" "$CANONICAL_SYMVERS"; do
  [[ -s "$file" ]] || {
    echo "missing release input: $file (run kmod/build-gki.sh $KMI)" >&2
    exit 2
  }
done

[[ "$(provenance_value kmi)" == "$KMI" ]] || {
  echo "$KMI provenance identity mismatch" >&2
  exit 1
}
KERNEL_ARTIFACT="$(provenance_value kernel_artifact)"
KERNEL_SHA256="$(provenance_value kernel_sha256)"
OFFICIAL_RELEASE="$(provenance_value official_release)"
OFFICIAL_IMAGE="$OFFICIAL_CACHE/$KMI/$KERNEL_ARTIFACT"
[[ -n "$KERNEL_ARTIFACT" && -n "$KERNEL_SHA256" && -n "$OFFICIAL_RELEASE" ]] || {
  echo "$KMI provenance lacks official Image identity" >&2
  exit 1
}
[[ -s "$OFFICIAL_IMAGE" ]] || {
  echo "missing official Image: $OFFICIAL_IMAGE (run kmod/build-gki.sh $KMI)" >&2
  exit 2
}
[[ "$(sha256sum "$OFFICIAL_IMAGE" | awk '{ print $1 }')" == "$KERNEL_SHA256" ]] || {
  echo "$KMI official Image SHA-256 mismatch" >&2
  exit 1
}
strings "$OFFICIAL_IMAGE" | awk -v expected="$OFFICIAL_RELEASE" '
  $0 == expected { found = 1 }
  END { exit !found }
' || {
  echo "$KMI official Image release mismatch: $OFFICIAL_RELEASE" >&2
  exit 1
}

log "verify $KMI release prebuilt and canonical symbol CRCs"
bash "$ROOT/kmod/verify-prebuilt.sh" "$KMI"

RELEASE_SHA="$(sha256sum "$RELEASE_KO" | awk '{ print $1 }')"
log "official release=$OFFICIAL_RELEASE"
log "official Image sha256=$KERNEL_SHA256"
log "release KO sha256=$RELEASE_SHA"

mkdir -p "$OUT" "$ROOT/qemu/out"
aarch64-linux-gnu-gcc -static -O2 -s -o "$ROOT/qemu/out/init" "$ROOT/qemu/init.c"
chmod 755 "$ROOT/qemu/out/init"
file "$ROOT/qemu/out/init" || true
bash "$ROOT/qemu/pack-initramfs.sh" "$KMI" "$RELEASE_KO"
[[ "$(sha256sum "$OUT/root/ethereal.ko" | awk '{ print $1 }')" == "$RELEASE_SHA" ]] || {
  echo "packed module does not match $RELEASE_KO" >&2
  exit 1
}

log "boot official GKI in QEMU (180s timeout)"
set +e
QEMU_KERNEL="$OFFICIAL_IMAGE" timeout --signal=KILL 180s \
  bash "$ROOT/qemu/run-gki.sh" "$KMI" | tee "$OUT/serial.log"
rc=${PIPESTATUS[0]}
set -e
log "qemu exit=$rc"

if grep -q 'ETHEREAL_QEMU_RESULT=PASS' "$OUT/serial.log"; then
  [[ "$(sha256sum "$RELEASE_KO" | awk '{ print $1 }')" == "$RELEASE_SHA" ]] || {
    echo "$RELEASE_KO changed during QEMU verification" >&2
    exit 1
  }
  echo "PASS $KMI official_release=$OFFICIAL_RELEASE ko_sha256=$RELEASE_SHA"
  exit 0
fi

echo "FAIL $KMI (see $OUT/serial.log)" >&2
grep -E 'ETHEREAL_QEMU_RESULT=|ethereal-qemu:|Kernel panic|Oops|ethereal:' \
  "$OUT/serial.log" | tail -80 >&2 || true
tail -80 "$OUT/serial.log" >&2 || true
exit 1
