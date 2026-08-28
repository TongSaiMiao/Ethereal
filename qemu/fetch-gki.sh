#!/usr/bin/env bash
# Fetch the exact source commits pinned by kmod/gki-locks.tsv.
set -euo pipefail
SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"

exec bash "$ROOT/kmod/build-gki.sh" --fetch-only "$@"
