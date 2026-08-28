#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/tests/out/ethinit-kmi-test"
mkdir -p "$(dirname -- "$OUT")"
"${CC:-cc}" -std=c17 -Wall -Wextra -Werror -O2 \
  "$ROOT/tests/ethinit-kmi-test.c" -o "$OUT"
"$OUT"
echo "ethinit KMI selection tests passed"
