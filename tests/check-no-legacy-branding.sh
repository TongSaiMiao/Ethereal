#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

old_r_title='R''Patch'
old_r_lower='r''patch'
old_r_upper='R''PATCH'
old_a_title='A''Patch'
old_a_lower='a''patch'
old_a_upper='A''PATCH'
legacy_auth_title='Super''Key'
legacy_auth_lower='super''key'
legacy_auth_short='s''key'
legacy_plugin_title='K''PM'
legacy_plugin_module='KP''Module'
legacy_android_origin='Android''Patch'
legacy_policy_origin='ap_''policy'
old_su='r''p/su'
legacy_r_re="${old_r_title}|${old_r_lower}|${old_r_upper}"
legacy_a_re="${old_a_title}|${old_a_lower}|${old_a_upper}"
legacy_auth_re="${legacy_auth_title}|${legacy_auth_lower}"
legacy_auth_word_re="(^|[^[:alnum:]_])${legacy_auth_short}([^[:alnum:]_]|$)"
legacy_module_re="${legacy_plugin_title}|${legacy_plugin_module}"
legacy_origin_re="${legacy_android_origin}|${legacy_policy_origin}"
legacy_name_re="${legacy_r_re}|${legacy_a_re}|${legacy_auth_re}|${legacy_module_re}"
legacy_content_re="${legacy_name_re}|${legacy_auth_word_re}|/?${old_su}"
legacy_magic2_hex='5250''5443'
legacy_ioctl_hex='5250''0001'
legacy_ap_config_hex='4150''544D'
legacy_magic2_le='43545052'
legacy_ioctl_le='01005052'
legacy_ap_config_le='4d545041'
allowed_origin_ack="- Thanks to [${old_a_title}](https://github.com/bmax121/${old_a_title}) for its work in the Android root ecosystem."
failed=0
declare -a files=()

for safe_identifier in PairPatchArgs boot_patch_pair; do
  if [[ "$safe_identifier" =~ $legacy_name_re ]]; then
    printf 'internal error: safe identifier matched legacy brand: %s\n' "$safe_identifier" >&2
    exit 2
  fi
done

while IFS= read -r -d '' path; do
  [[ -e "$path" ]] || continue
  files+=("$path")
  if [[ "$path" =~ $legacy_name_re ]]; then
    printf 'legacy-branded path: %s\n' "$path" >&2
    failed=1
  fi
done < <(git ls-files -co --exclude-standard -z)

for path in "${files[@]}"; do
  if ! LC_ALL=C grep -Iq . "$path"; then
    if LC_ALL=C grep -aqE -- "$legacy_content_re" "$path"; then
      printf 'legacy brand bytes found in binary: %s\n' "$path" >&2
      failed=1
    fi
    hex="$(od -An -tx1 -v -- "$path" | tr -d ' \n')"
    if [[ "$hex" == *"$legacy_magic2_le"* || "$hex" == *"$legacy_ioctl_le"* ||
          "$hex" == *"$legacy_ap_config_le"* ]]; then
      printf 'legacy protocol bytes found in binary: %s\n' "$path" >&2
      failed=1
    fi
    continue
  fi

  while IFS=: read -r line_number line; do
    if [[ "$path" == README.md && "$line" == "$allowed_origin_ack" ]]; then
      continue
    fi
    printf 'legacy brand text found: %s:%s:%s\n' "$path" "$line_number" "$line" >&2
    failed=1
  done < <(LC_ALL=C grep -anE -- "$legacy_content_re" "$path" || true)

  while IFS=: read -r line_number line; do
    printf 'legacy source origin found: %s:%s:%s\n' "$path" "$line_number" "$line" >&2
    failed=1
  done < <(LC_ALL=C grep -anE -- "$legacy_origin_re" "$path" || true)

  if LC_ALL=C grep -aiqE -- "${legacy_magic2_hex}|${legacy_ioctl_hex}|${legacy_ap_config_hex}" "$path"; then
    printf 'legacy protocol constant found: %s\n' "$path" >&2
    failed=1
  fi
done

if [[ "$(LC_ALL=C grep -Fxc -- "$allowed_origin_ack" README.md || true)" != 1 ]]; then
  printf 'README must contain exactly one approved %s acknowledgement\n' "$old_a_title" >&2
  failed=1
fi

if (( failed != 0 )); then
  exit 1
fi

echo "OK: repository-owned paths and contents use Ethereal branding"
