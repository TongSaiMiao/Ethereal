#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/tests/out/ethinit-kmi-test"
mkdir -p "$(dirname -- "$OUT")"
"${CC:-cc}" -std=c17 -Wall -Wextra -Werror -O2 \
  "$ROOT/tests/ethinit-kmi-test.c" -o "$OUT"
"$OUT"

stage_su_source="$(sed -n '/^static void stage_su/,/^static int name_is_ko/p' \
  "$ROOT/ethinit/ethinit.c")"
if grep -Eq 'O_TRUNC|SYS_OPENAT|SYS_WRITE|copy_file' <<<"$stage_su_source"; then
  echo "stage_su must not write shared su paths" >&2
  exit 1
fi
grep -Fq 'const ETHEREAL_SU_PATH: &str = "/dev/.ethereal/su";' \
  "$ROOT/ethd/src/event.rs"
if grep -Fq 'run_command("/eth/su"' "$ROOT/ethd/src/event.rs"; then
  echo "ethd must not execute the shared /eth/su path" >&2
  exit 1
fi

echo "ethinit KMI selection and private su path tests passed"
