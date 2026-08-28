#!/bin/bash
# Compatibility entry point: the official-Image test is already incremental.
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
exec bash "$SCRIPT_DIR/build-and-run.sh" "$@"
