#!/usr/bin/env bash
# Compatibility entry point. build-gki.sh is the only release prebuilt writer.
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"

bash "$ROOT/kmod/build-gki.sh" "$@"
exec bash "$ROOT/kmod/verify-prebuilt.sh"
