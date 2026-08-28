#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"
KMOD="$ROOT/kmod"
LOCKS="$KMOD/gki-locks.tsv"
TOOLCHAIN_LOCKS="$KMOD/toolchain-locks.tsv"
MODULE_BUILD="${GKI_MODULE_BUILD:-/root/gki-module-build}"
TOOLCHAIN_CACHE="${GKI_TOOLCHAIN_CACHE:-/root/gki-toolchains}"
CLANG_REPO_URL="https://android.googlesource.com/platform/prebuilts/clang/host/linux-x86"
RUST_REPO_URL="https://android.googlesource.com/platform/prebuilts/rust"
CLANG_TOOLS_REPO_URL="https://android.googlesource.com/platform/prebuilts/clang-tools"
FEATURE_MARKER="$(tr -d '\r\n' < "$KMOD/feature-marker.txt")"
HOST_BUILD_PATH_RE='[[:alpha:]]:\\|(^|[^[:alpha:]])[[:alpha:]]:/|/(mnt/[[:alpha:]]/|home/[^/[:space:][:cntrl:]]+/|Users/[^/[:space:][:cntrl:]]+/|__w/|tmp/ethereal-kmod([./]|$)|root/(gki-src|qemu-build|gki-module-build|android-ndk|gki-toolchains)(/|$))'

DEFAULT_KMIS=(
  android12-5.4
  android12-5.10
  android13-5.10
  android13-5.15
  android14-5.15
  android14-6.1
  android15-6.6
  android16-6.12
)
if [[ $# -gt 0 ]]; then
  KMIS=("$@")
else
  KMIS=("${DEFAULT_KMIS[@]}")
fi

for command in awk diff modinfo readelf sha256sum strings; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 2
  }
done
[[ -s "$LOCKS" && -s "$TOOLCHAIN_LOCKS" ]] || {
  echo "missing GKI or toolchain lock table" >&2
  exit 2
}
if grep -Eq -- '-mbranch-protection(=|[[:space:]])' "$KMOD/Makefile"; then
  echo "kmod/Makefile must inherit -mbranch-protection from the locked ACK tree" >&2
  exit 2
fi

awk -F '\t' '
  BEGIN {
    header = "# kmi\tclang_commit\tclang_dir\tclang_tree\tclang_sha256\tclang_version_sha256\trust_commit\trust_dir\trust_tree\trustc_sha256\trustc_version_sha256\tclang_tools_commit\tclang_tools_dir\tclang_tools_tree\tbindgen_sha256\tbindgen_version_sha256\tpahole_version\tpahole_sha256\tpahole_version_sha256"
    expected["android12-5.4"] = 1
    expected["android12-5.10"] = 1
    expected["android13-5.10"] = 1
    expected["android13-5.15"] = 1
    expected["android14-5.15"] = 1
    expected["android14-6.1"] = 1
    expected["android15-6.6"] = 1
    expected["android16-6.12"] = 1
  }
  NR == 1 {
    if ($0 != header) {
      print "invalid toolchain lock header" > "/dev/stderr"
      bad = 1
    }
    next
  }
  /^#/ || NF == 0 { next }
  {
    if (NF != 19 || !($1 in expected) || seen[$1]++) {
      print "invalid toolchain lock row for " $1 > "/dev/stderr"
      bad = 1
    }
  }
  END {
    for (kmi in expected) {
      if (!(kmi in seen)) {
        print "missing toolchain lock row for " kmi > "/dev/stderr"
        bad = 1
      }
    }
    exit bad
  }
' "$TOOLCHAIN_LOCKS" || exit 2

failed=0
fail() {
  printf 'FAIL\t%s\t%s\n' "$1" "$2" >&2
  failed=1
}

sha256() { sha256sum "$1" | awk '{ print $1 }'; }

provenance_value() {
  local file="$1" key="$2"
  awk -F '=' -v wanted="$key" '
    $1 == wanted && !found {
      print substr($0, index($0, "=") + 1)
      found = 1
    }
  ' "$file"
}

validate_provenance() {
  awk '
    !/^[a-z0-9_]+=[ -~]*$/ {
      print "malformed provenance line " NR > "/dev/stderr"
      bad = 1
      next
    }
    {
      key = $0
      sub(/=.*/, "", key)
      if (seen[key]++) {
        print "duplicate provenance key " key > "/dev/stderr"
        bad = 1
      }
    }
    END { exit bad }
  ' "$1"
}

check_provenance_value() {
  local kmi="$1" provenance="$2" key="$3" expected="$4" actual
  actual="$(provenance_value "$provenance" "$key")"
  [[ "$actual" == "$expected" ]] ||
    fail "$kmi" "provenance $key is '$actual', expected '$expected'"
}

read_toolchain_lock() {
  local kmi="$1" row
  row="$(awk -F '\t' -v wanted="$kmi" '
    $1 == wanted && !found { print; found = 1 }
    END { if (!found) exit 1 }
  ' "$TOOLCHAIN_LOCKS")" || return 1
  IFS=$'\t' read -r LOCK_TOOLCHAIN_KMI LOCK_CLANG_COMMIT LOCK_CLANG_DIR \
    LOCK_CLANG_TREE LOCK_CLANG_SHA256 LOCK_CLANG_VERSION_SHA256 \
    LOCK_RUST_COMMIT LOCK_RUST_DIR LOCK_RUST_TREE LOCK_RUSTC_SHA256 \
    LOCK_RUSTC_VERSION_SHA256 LOCK_CLANG_TOOLS_COMMIT \
    LOCK_CLANG_TOOLS_DIR LOCK_CLANG_TOOLS_TREE LOCK_BINDGEN_SHA256 \
    LOCK_BINDGEN_VERSION_SHA256 LOCK_PAHOLE_VERSION LOCK_PAHOLE_SHA256 \
    LOCK_PAHOLE_VERSION_SHA256 <<<"$row"
  [[ "$LOCK_TOOLCHAIN_KMI" == "$kmi" &&
     "$LOCK_CLANG_COMMIT" =~ ^[0-9a-f]{40}$ &&
     "$LOCK_CLANG_DIR" =~ ^clang-r[0-9]+[a-z]?$ &&
     "$LOCK_CLANG_TREE" =~ ^[0-9a-f]{40}$ &&
     "$LOCK_CLANG_SHA256" =~ ^[0-9a-f]{64}$ &&
     "$LOCK_CLANG_VERSION_SHA256" =~ ^[0-9a-f]{64}$ &&
     "$LOCK_PAHOLE_VERSION" =~ ^[0-9]+\.[0-9]+(\.[0-9]+)?$ &&
     "$LOCK_PAHOLE_SHA256" =~ ^[0-9a-f]{64}$ &&
     "$LOCK_PAHOLE_VERSION_SHA256" =~ ^[0-9a-f]{64}$ ]] || return 1
  if [[ "$LOCK_RUST_COMMIT" == - ]]; then
    [[ "$LOCK_RUST_DIR" == - && "$LOCK_RUST_TREE" == - &&
       "$LOCK_RUSTC_SHA256" == - && "$LOCK_RUSTC_VERSION_SHA256" == - &&
       "$LOCK_CLANG_TOOLS_COMMIT" == - && "$LOCK_CLANG_TOOLS_DIR" == - &&
       "$LOCK_CLANG_TOOLS_TREE" == - && "$LOCK_BINDGEN_SHA256" == - &&
       "$LOCK_BINDGEN_VERSION_SHA256" == - ]] || return 1
  else
    [[ "$LOCK_RUST_COMMIT" =~ ^[0-9a-f]{40}$ &&
       "$LOCK_RUST_DIR" =~ ^linux-x86/[0-9]+\.[0-9]+\.[0-9]+b?$ &&
       "$LOCK_RUST_TREE" =~ ^[0-9a-f]{40}$ &&
       "$LOCK_RUSTC_SHA256" =~ ^[0-9a-f]{64}$ &&
       "$LOCK_RUSTC_VERSION_SHA256" =~ ^[0-9a-f]{64}$ &&
       "$LOCK_CLANG_TOOLS_COMMIT" =~ ^[0-9a-f]{40}$ &&
       "$LOCK_CLANG_TOOLS_DIR" == linux-x86 &&
       "$LOCK_CLANG_TOOLS_TREE" =~ ^[0-9a-f]{40}$ &&
       "$LOCK_BINDGEN_SHA256" =~ ^[0-9a-f]{64}$ &&
       "$LOCK_BINDGEN_VERSION_SHA256" =~ ^[0-9a-f]{64}$ ]] || return 1
  fi
}

config_projection() {
  awk '
    /^CONFIG_[A-Z0-9_]+=/ {
      key = $0
      sub(/=.*/, "", key)
    }
    /^# CONFIG_[A-Z0-9_]+ is not set$/ {
      key = $0
      sub(/^# /, "", key)
      sub(/ is not set$/, "", key)
    }
    !key { next }
    key == "CONFIG_UNUSED_KSYMS_WHITELIST" { key = ""; next }
    key == "CONFIG_PAHOLE_VERSION" { key = ""; next }
    key == "CONFIG_MODULE_ALLOW_BTF_MISMATCH" { key = ""; next }
    key == "CONFIG_UAPI_HEADER_TEST" { key = ""; next }
    key == "CONFIG_CC_CAN_LINK" { key = ""; next }
    key == "CONFIG_CC_CAN_LINK_STATIC" { key = ""; next }
    key ~ /^CONFIG_PAHOLE_HAS_/ { key = ""; next }
    { print; key = "" }
  ' "$1" | LC_ALL=C sort
}

section_size() {
  local ko="$1" name="$2"
  readelf -SW "$ko" 2>/dev/null | awk -v wanted="$name" '
    {
      for (i = 1; i <= NF; i++) {
        if ($i == wanted) {
          count++
          size = $(i + 4)
        }
      }
    }
    END {
      if (count > 1) exit 2
      if (count == 1) print size
    }
  '
}

this_module_section_info() {
  readelf -SW "$1" 2>/dev/null | awk '
    {
      for (i = 1; i <= NF; i++) {
        if ($i == ".gnu.linkonce.this_module") {
          count++
          type = $(i + 1)
          size = $(i + 4)
          flags = $(i + 6)
          alignment = $(i + 9)
        }
      }
    }
    END {
      if (count != 1 || type != "PROGBITS") exit 2
      print size, alignment, flags
    }
  '
}

this_module_symbol_size() {
  readelf -sW "$1" 2>/dev/null | awk '
    $8 == "__this_module" && $4 == "OBJECT" {
      count++
      size = $3
    }
    END {
      if (count != 1) exit 2
      print size
    }
  '
}

inspect_this_module() {
  local ko="$1" info size_hex alignment flags symbol_size
  info="$(this_module_section_info "$ko")" || return 1
  read -r size_hex alignment flags <<<"$info"
  [[ "$size_hex" =~ ^[0-9a-fA-F]+$ && "$alignment" =~ ^[0-9]+$ &&
     "$flags" == *W* && "$flags" == *A* ]] || return 1
  THIS_MODULE_SIZE=$((16#$size_hex))
  THIS_MODULE_ALIGNMENT=$((10#$alignment))
  (( THIS_MODULE_SIZE >= 256 && THIS_MODULE_SIZE <= 8192 &&
     THIS_MODULE_ALIGNMENT >= 8 &&
     (THIS_MODULE_ALIGNMENT & (THIS_MODULE_ALIGNMENT - 1)) == 0 )) || return 1
  symbol_size="$(this_module_symbol_size "$ko")" || return 1
  [[ "$symbol_size" =~ ^[0-9]+$ && "$symbol_size" == "$THIS_MODULE_SIZE" ]]
}

inspect_modversions() {
  local ko="$1" config="$2" basic_hex ext_crc_hex ext_names_hex
  basic_hex="$(section_size "$ko" __versions)" || return 1
  ext_crc_hex="$(section_size "$ko" __version_ext_crcs)" || return 1
  ext_names_hex="$(section_size "$ko" __version_ext_names)" || return 1
  VERSIONS_SECTION_SIZE=0
  VERSION_EXT_CRCS_SIZE=0
  VERSION_EXT_NAMES_SIZE=0
  [[ -z "$basic_hex" ]] || {
    [[ "$basic_hex" =~ ^[0-9a-fA-F]+$ ]] || return 1
    VERSIONS_SECTION_SIZE=$((16#$basic_hex))
  }
  [[ -z "$ext_crc_hex" ]] || {
    [[ "$ext_crc_hex" =~ ^[0-9a-fA-F]+$ ]] || return 1
    VERSION_EXT_CRCS_SIZE=$((16#$ext_crc_hex))
  }
  [[ -z "$ext_names_hex" ]] || {
    [[ "$ext_names_hex" =~ ^[0-9a-fA-F]+$ ]] || return 1
    VERSION_EXT_NAMES_SIZE=$((16#$ext_names_hex))
  }
  if grep -qxF CONFIG_EXTENDED_MODVERSIONS=y "$config"; then
    MODVERSIONS_FORMAT=extended
    (( VERSION_EXT_CRCS_SIZE > 0 && VERSION_EXT_CRCS_SIZE % 4 == 0 &&
       VERSION_EXT_NAMES_SIZE > 0 )) || return 1
  else
    MODVERSIONS_FORMAT=basic
    (( VERSION_EXT_CRCS_SIZE == 0 && VERSION_EXT_NAMES_SIZE == 0 )) || return 1
  fi
  (( VERSIONS_SECTION_SIZE > 0 || VERSION_EXT_CRCS_SIZE > 0 ))
}

printf 'KMI\tSIZE\tVERMAGIC\tVERSION_BYTES\tTHIS_MODULE\tSHA256\n'
for kmi in "${KMIS[@]}"; do
  if ! read_toolchain_lock "$kmi"; then
    fail "$kmi" "missing or malformed toolchain lock"
    continue
  fi
  lock_row="$(awk -F '\t' -v wanted="$kmi" '
    $1 == wanted && !found { print; found = 1 }
    END { if (!found) exit 1 }
  ' "$LOCKS")" || {
    fail "$kmi" "missing source lock"
    continue
  }
  IFS=$'\t' read -r lock_kmi source_ref source_commit source_tree \
    prebuilt_ref prebuilt_commit prebuilt_build_id manifest_artifact \
    manifest_sha256 prebuilt_info_blob abi_artifact abi_blob \
    kernel_artifact kernel_blob symvers_artifact symvers_blob <<<"$lock_row"

  outdir="$KMOD/prebuilt/$kmi"
  ko="$outdir/ethereal.ko"
  provenance="$outdir/provenance.env"
  symvers="$outdir/Module.symvers"
  abi_symvers="$outdir/official-abi.symvers"
  projection="$outdir/canonical.projected.symvers"
  abi_projection="$outdir/abi.projected.symvers"
  config="$outdir/canonical.config"
  prepared_config="$outdir/prepared.config"
  defconfig="$outdir/gki_defconfig"
  manifest="$outdir/manifest.xml"
  missing=0
  for required_file in "$ko" "$provenance" "$symvers" "$abi_symvers" \
    "$projection" "$abi_projection" "$config" "$prepared_config" \
    "$defconfig" "$manifest"; do
    [[ -s "$required_file" ]] || { fail "$kmi" "missing $required_file"; missing=1; }
  done
  (( missing == 0 )) || continue
  if ! validate_provenance "$provenance"; then
    fail "$kmi" "malformed or duplicate provenance entries"
    continue
  fi

  readelf -h "$ko" >/dev/null 2>&1 || fail "$kmi" "not an ELF file"
  name="$(modinfo -F name "$ko" 2>/dev/null || true)"
  vermagic="$(modinfo -F vermagic "$ko" 2>/dev/null || true)"
  features="$(modinfo -F ethereal_features "$ko" 2>/dev/null || true)"
  official_release="$(provenance_value "$provenance" official_release)"
  bootstrap="$(provenance_value "$provenance" bootstrap)"
  [[ "$name" == ethereal ]] || fail "$kmi" "module name is '$name'"
  [[ "$features" == "$FEATURE_MARKER" ]] || fail "$kmi" "feature marker is '$features'"
  [[ -n "$official_release" && "$vermagic" == "$official_release "* ]] ||
    fail "$kmi" "vermagic '$vermagic' does not match '$official_release'"
  locked_release_suffix="-g${source_commit:0:12}-ab$prebuilt_build_id"
  case "$official_release" in
    *"$locked_release_suffix" | *"$locked_release_suffix-4k" | *"$locked_release_suffix-16k") ;;
    *) fail "$kmi" "official release does not identify locked source/build" ;;
  esac
  [[ "$vermagic" == *modversions* ]] || fail "$kmi" "vermagic lacks modversions"

  THIS_MODULE_SIZE=0
  THIS_MODULE_ALIGNMENT=0
  if ! inspect_this_module "$ko"; then
    fail "$kmi" "invalid .gnu.linkonce.this_module layout/symbol"
  fi
  VERSIONS_SECTION_SIZE=0
  VERSION_EXT_CRCS_SIZE=0
  VERSION_EXT_NAMES_SIZE=0
  MODVERSIONS_FORMAT=""
  if ! inspect_modversions "$ko" "$prepared_config"; then
    fail "$kmi" "modversion sections do not match CONFIG_EXTENDED_MODVERSIONS"
  fi

  if grep -aFq "$ROOT" "$ko" ||
     grep -aFq "${GKI_WORKDIR:-/root/gki-src}" "$ko" ||
     grep -aFq "${QEMU_BUILD:-/root/qemu-build}" "$ko" ||
     grep -aFq "$MODULE_BUILD" "$ko" ||
     grep -aEiq "$HOST_BUILD_PATH_RE" "$ko"; then
    fail "$kmi" "contains an absolute host build path"
  fi
  if grep -aEiq '(r''patch|a''patch|super''key|s''key|k''pm|kp''module|/?(r''p|a''p)/su)' "$ko"; then
    fail "$kmi" "contains a legacy brand identifier"
  fi

  compiler_line="$(strings "$ko" | awk '/clang version/ && !found { print; found = 1 }')"
  [[ -n "$compiler_line" ]] || fail "$kmi" "module has no embedded Clang identity"
  compiler_sha="$(printf '%s' "$compiler_line" | sha256sum | awk '{ print $1 }')"
  [[ "$compiler_sha" == "$LOCK_CLANG_VERSION_SHA256" ]] ||
    fail "$kmi" "embedded Clang identity does not match toolchain lock"
  clang_revision="${LOCK_CLANG_DIR#clang-}"
  [[ "$compiler_line" == *"based on $clang_revision) clang version"* ]] ||
    fail "$kmi" "embedded compiler is not $LOCK_CLANG_DIR"

  canonical_equivalence_sha="$(config_projection "$config" | sha256sum | awk '{ print $1 }')"
  prepared_equivalence_sha="$(config_projection "$prepared_config" | sha256sum | awk '{ print $1 }')"
  if [[ "$canonical_equivalence_sha" != "$prepared_equivalence_sha" ]]; then
    diff -u <(config_projection "$config") <(config_projection "$prepared_config") >&2 || true
    fail "$kmi" "prepared config is not ABI-equivalent to official config"
  fi
  config_compiler_line="$(awk '
    /^CONFIG_CC_VERSION_TEXT=/ && !found {
      line = $0
      sub(/^CONFIG_CC_VERSION_TEXT="/, "", line)
      sub(/"$/, "", line)
      print line
      found = 1
    }
  ' "$prepared_config")"
  [[ "$config_compiler_line" == "$compiler_line" ]] ||
    fail "$kmi" "prepared config compiler identity differs from the KO"
  if grep -qxF CONFIG_DEBUG_INFO_BTF_MODULES=y "$config"; then
    grep -qxF CONFIG_DEBUG_INFO_BTF_MODULES=y "$prepared_config" ||
      fail "$kmi" "prepared config lost CONFIG_DEBUG_INFO_BTF_MODULES"
  fi
  pahole_config="$(awk -F= '$1 == "CONFIG_PAHOLE_VERSION" { print $2; found = 1 }
    END { if (!found) print "" }' "$prepared_config")"
  if [[ -n "$pahole_config" ]]; then
    [[ "$pahole_config" =~ ^[0-9]+$ ]] && (( 10#$pahole_config >= 125 )) ||
      fail "$kmi" "prepared config did not detect pahole >= 1.25"
  fi
  if [[ "$LOCK_RUST_COMMIT" != - ]]; then
    grep -qxF CONFIG_RUST=y "$config" && grep -qxF CONFIG_RUST=y "$prepared_config" ||
      fail "$kmi" "locked Rust KMI did not preserve CONFIG_RUST=y"
  fi

  IFS=. read -r pahole_major pahole_minor _ <<<"$LOCK_PAHOLE_VERSION"
  (( 10#$pahole_major > 1 ||
     (10#$pahole_major == 1 && 10#$pahole_minor >= 25) )) ||
    fail "$kmi" "toolchain lock permits pahole older than 1.25"

  check_provenance_value "$kmi" "$provenance" format ethereal-gki-provenance-v3
  check_provenance_value "$kmi" "$provenance" kmi "$kmi"
  check_provenance_value "$kmi" "$provenance" source_ref "refs/heads/$source_ref"
  check_provenance_value "$kmi" "$provenance" source_commit "$source_commit"
  check_provenance_value "$kmi" "$provenance" source_tree "$source_tree"
  check_provenance_value "$kmi" "$provenance" prebuilt_ref "refs/heads/$prebuilt_ref"
  check_provenance_value "$kmi" "$provenance" prebuilt_commit "$prebuilt_commit"
  check_provenance_value "$kmi" "$provenance" prebuilt_build_id "$prebuilt_build_id"
  check_provenance_value "$kmi" "$provenance" manifest_artifact "$manifest_artifact"
  check_provenance_value "$kmi" "$provenance" manifest_sha256 "$manifest_sha256"
  check_provenance_value "$kmi" "$provenance" prebuilt_info_blob "$prebuilt_info_blob"
  check_provenance_value "$kmi" "$provenance" abi_artifact "$abi_artifact"
  check_provenance_value "$kmi" "$provenance" abi_blob "$abi_blob"
  check_provenance_value "$kmi" "$provenance" kernel_artifact "$kernel_artifact"
  check_provenance_value "$kmi" "$provenance" kernel_blob "$kernel_blob"
  check_provenance_value "$kmi" "$provenance" symvers_artifact "$symvers_artifact"
  check_provenance_value "$kmi" "$provenance" symvers_blob "$symvers_blob"
  check_provenance_value "$kmi" "$provenance" build_config official-image-ikconfig
  check_provenance_value "$kmi" "$provenance" gki_defconfig_sha256 "$(sha256 "$defconfig")"
  check_provenance_value "$kmi" "$provenance" host_extract_cert_cflags -DUSE_PKCS11_ENGINE
  check_provenance_value "$kmi" "$provenance" config_sha256 "$(sha256 "$config")"
  check_provenance_value "$kmi" "$provenance" prepared_config_sha256 "$(sha256 "$prepared_config")"
  check_provenance_value "$kmi" "$provenance" config_equivalence_sha256 "$canonical_equivalence_sha"
  check_provenance_value "$kmi" "$provenance" toolchain_id "android-$LOCK_CLANG_DIR"
  check_provenance_value "$kmi" "$provenance" clang_repo "$CLANG_REPO_URL"
  check_provenance_value "$kmi" "$provenance" clang_commit "$LOCK_CLANG_COMMIT"
  check_provenance_value "$kmi" "$provenance" clang_dir "$LOCK_CLANG_DIR"
  check_provenance_value "$kmi" "$provenance" clang_subtree_tree "$LOCK_CLANG_TREE"
  check_provenance_value "$kmi" "$provenance" clang_sha256 "$LOCK_CLANG_SHA256"
  check_provenance_value "$kmi" "$provenance" clang_version_sha256 "$LOCK_CLANG_VERSION_SHA256"
  if [[ "$LOCK_RUST_COMMIT" == - ]]; then
    rust_repo=-
    clang_tools_repo=-
  else
    rust_repo="$RUST_REPO_URL"
    clang_tools_repo="$CLANG_TOOLS_REPO_URL"
  fi
  check_provenance_value "$kmi" "$provenance" rust_repo "$rust_repo"
  check_provenance_value "$kmi" "$provenance" rust_commit "$LOCK_RUST_COMMIT"
  check_provenance_value "$kmi" "$provenance" rust_dir "$LOCK_RUST_DIR"
  check_provenance_value "$kmi" "$provenance" rust_subtree_tree "$LOCK_RUST_TREE"
  check_provenance_value "$kmi" "$provenance" rustc_sha256 "$LOCK_RUSTC_SHA256"
  check_provenance_value "$kmi" "$provenance" rustc_version_sha256 "$LOCK_RUSTC_VERSION_SHA256"
  check_provenance_value "$kmi" "$provenance" clang_tools_repo "$clang_tools_repo"
  check_provenance_value "$kmi" "$provenance" clang_tools_commit "$LOCK_CLANG_TOOLS_COMMIT"
  check_provenance_value "$kmi" "$provenance" clang_tools_dir "$LOCK_CLANG_TOOLS_DIR"
  check_provenance_value "$kmi" "$provenance" clang_tools_subtree_tree "$LOCK_CLANG_TOOLS_TREE"
  check_provenance_value "$kmi" "$provenance" bindgen_sha256 "$LOCK_BINDGEN_SHA256"
  check_provenance_value "$kmi" "$provenance" bindgen_version_sha256 "$LOCK_BINDGEN_VERSION_SHA256"
  check_provenance_value "$kmi" "$provenance" pahole_version "$LOCK_PAHOLE_VERSION"
  check_provenance_value "$kmi" "$provenance" pahole_sha256 "$LOCK_PAHOLE_SHA256"
  check_provenance_value "$kmi" "$provenance" pahole_version_sha256 "$LOCK_PAHOLE_VERSION_SHA256"
  check_provenance_value "$kmi" "$provenance" this_module_size "$THIS_MODULE_SIZE"
  check_provenance_value "$kmi" "$provenance" this_module_alignment "$THIS_MODULE_ALIGNMENT"
  check_provenance_value "$kmi" "$provenance" struct_module_dwarf_size "$THIS_MODULE_SIZE"
  check_provenance_value "$kmi" "$provenance" modversions_format "$MODVERSIONS_FORMAT"
  check_provenance_value "$kmi" "$provenance" versions_section_size "$VERSIONS_SECTION_SIZE"
  check_provenance_value "$kmi" "$provenance" version_ext_crcs_size "$VERSION_EXT_CRCS_SIZE"
  check_provenance_value "$kmi" "$provenance" version_ext_names_size "$VERSION_EXT_NAMES_SIZE"
  check_provenance_value "$kmi" "$provenance" module_symvers_sha256 "$(sha256 "$symvers")"
  check_provenance_value "$kmi" "$provenance" official_abi_symvers_sha256 "$(sha256 "$abi_symvers")"
  check_provenance_value "$kmi" "$provenance" canonical_projection_sha256 "$(sha256 "$projection")"
  check_provenance_value "$kmi" "$provenance" abi_projection_sha256 "$(sha256 "$abi_projection")"
  check_provenance_value "$kmi" "$provenance" ethereal_c_sha256 "$(sha256 "$KMOD/ethereal.c")"
  check_provenance_value "$kmi" "$provenance" kmod_makefile_sha256 "$(sha256 "$KMOD/Makefile")"
  check_provenance_value "$kmi" "$provenance" abi_to_symvers_sha256 "$(sha256 "$KMOD/abi-to-symvers.py")"
  check_provenance_value "$kmi" "$provenance" manager_cert_sha256 "$(sha256 "$KMOD/manager_cert.h")"
  check_provenance_value "$kmi" "$provenance" feature_marker_sha256 "$(sha256 "$KMOD/feature-marker.txt")"
  check_provenance_value "$kmi" "$provenance" gki_locks_sha256 "$(sha256 "$LOCKS")"
  check_provenance_value "$kmi" "$provenance" toolchain_locks_sha256 "$(sha256 "$TOOLCHAIN_LOCKS")"
  check_provenance_value "$kmi" "$provenance" build_gki_sha256 "$(sha256 "$KMOD/build-gki.sh")"
  check_provenance_value "$kmi" "$provenance" verify_module_crc_sha256 "$(sha256 "$KMOD/verify-module-crc.sh")"
  check_provenance_value "$kmi" "$provenance" feature_marker "$FEATURE_MARKER"
  ko_sha="$(sha256 "$ko")"
  check_provenance_value "$kmi" "$provenance" ko_sha256 "$ko_sha"
  [[ "$(sha256 "$manifest")" == "$manifest_sha256" ]] ||
    fail "$kmi" "manifest SHA does not match gki-locks.tsv"
  for hash_key in source_archive_sha256 abi_sha256 kernel_sha256; do
    hash_value="$(provenance_value "$provenance" "$hash_key")"
    [[ "$hash_value" =~ ^[0-9a-f]{64}$ ]] || fail "$kmi" "invalid $hash_key"
  done

  clang_bin="$TOOLCHAIN_CACHE/clang-prebuilt/$LOCK_CLANG_DIR/bin/clang"
  if [[ -e "$clang_bin" ]]; then
    [[ -x "$clang_bin" && "$(sha256 "$clang_bin")" == "$LOCK_CLANG_SHA256" ]] ||
      fail "$kmi" "local locked Clang cache has wrong binary"
    if [[ -x "$clang_bin" ]]; then
      local_clang_line="$("$clang_bin" --version | awk 'NR == 1 { line = $0 } END { print line }')"
      [[ "$(printf '%s' "$local_clang_line" | sha256sum | awk '{ print $1 }')" == \
         "$LOCK_CLANG_VERSION_SHA256" ]] || fail "$kmi" "local Clang identity mismatch"
    fi
  fi
  if [[ "$LOCK_RUST_COMMIT" != - ]]; then
    rust_version="${LOCK_RUST_DIR##*/}"
    rustc_bin="$TOOLCHAIN_CACHE/rust-prebuilt/$rust_version/bin/rustc"
    bindgen_bin="$TOOLCHAIN_CACHE/clang-tools-prebuilt/$LOCK_CLANG_TOOLS_COMMIT/bin/bindgen"
    if [[ -e "$rustc_bin" ]]; then
      [[ -x "$rustc_bin" && "$(sha256 "$rustc_bin")" == "$LOCK_RUSTC_SHA256" ]] ||
        fail "$kmi" "local Rust cache has wrong rustc"
    fi
    if [[ -e "$bindgen_bin" ]]; then
      [[ -x "$bindgen_bin" && "$(sha256 "$bindgen_bin")" == "$LOCK_BINDGEN_SHA256" ]] ||
        fail "$kmi" "local clang-tools cache has wrong bindgen"
    fi
  fi
  if [[ -n "${PAHOLE:-}" ]]; then
    [[ -x "$PAHOLE" && "$(sha256 "$PAHOLE")" == "$LOCK_PAHOLE_SHA256" ]] ||
      fail "$kmi" "explicit PAHOLE does not match toolchain lock"
  fi

  grep -Eq '^0x[0-9a-fA-F]{8}[[:space:]]' "$symvers" ||
    fail "$kmi" "canonical Module.symvers is not a real symbol table"
  grep -Eiq "$HOST_BUILD_PATH_RE" "$symvers" &&
    fail "$kmi" "canonical Module.symvers contains a host path"
  bash "$KMOD/verify-module-crc.sh" "$ko" "$symvers" >/dev/null ||
    fail "$kmi" "dependency CRCs differ from canonical Module.symvers"
  bash "$KMOD/verify-module-crc.sh" "$ko" "$projection" >/dev/null ||
    fail "$kmi" "committed dependency projection differs from module"
  bash "$KMOD/verify-module-crc.sh" "$ko" "$abi_symvers" >/dev/null ||
    fail "$kmi" "a dependency is outside the official GKI ABI"
  bash "$KMOD/verify-module-crc.sh" "$ko" "$abi_projection" >/dev/null ||
    fail "$kmi" "committed ABI projection differs from module"
  case "$bootstrap" in
    sprint-symbol-scan)
      awk '$2 == "sprint_symbol" { s = 1 }
           $2 == "register_kprobe" || $2 == "unregister_kprobe" { bad = 1 }
           END { exit !(s && !bad) }' "$projection" ||
        fail "$kmi" "sprint bootstrap has wrong static dependencies"
      ;;
    kprobe)
      awk '$2 == "register_kprobe" { r = 1 }
           $2 == "unregister_kprobe" { u = 1 }
           END { exit !(r && u) }' "$projection" ||
        fail "$kmi" "kprobe bootstrap dependencies are incomplete"
      ;;
    *) fail "$kmi" "unknown bootstrap '$bootstrap'" ;;
  esac

  cached_symvers="$MODULE_BUILD/$kmi/Module.symvers"
  cached_config="$MODULE_BUILD/$kmi/.config"
  if [[ -s "$cached_symvers" && "$(sha256 "$cached_symvers")" != "$(sha256 "$symvers")" ]]; then
    fail "$kmi" "committed Module.symvers differs from build cache"
  fi
  if [[ -s "$cached_config" && "$(sha256 "$cached_config")" != "$(sha256 "$prepared_config")" ]]; then
    fail "$kmi" "committed prepared config differs from build cache"
  fi
  size="$(wc -c < "$ko")"
  version_bytes=$((VERSIONS_SECTION_SIZE + VERSION_EXT_CRCS_SIZE + VERSION_EXT_NAMES_SIZE))
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$kmi" "$size" "$vermagic" \
    "$version_bytes" "$THIS_MODULE_SIZE" "$ko_sha"
done

exit "$failed"
