#!/usr/bin/env bash
# Build release ethereal.ko files against locked official Android GKI prebuilts.
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${ETHEREAL_ROOT:-$(cd -- "$SCRIPT_DIR/.." && pwd)}"
KMOD="$ROOT/kmod"
LOCKS="$KMOD/gki-locks.tsv"
TOOLCHAIN_LOCKS="$KMOD/toolchain-locks.tsv"
PREBUILT="$KMOD/prebuilt"
FEATURE_MARKER="$(tr -d '\r\n' < "$KMOD/feature-marker.txt")"
WORKDIR="${GKI_WORKDIR:-/root/gki-src}"
MODULE_BUILD="${GKI_MODULE_BUILD:-/root/gki-module-build}"
MODULE_SOURCE_ROOT="/workspace/Ethereal/kmod-build"
OFFICIAL_CACHE="${GKI_OFFICIAL_CACHE:-/root/gki-official}"
TOOLCHAIN_CACHE="${GKI_TOOLCHAIN_CACHE:-/root/gki-toolchains}"
CLANG_CACHE="$TOOLCHAIN_CACHE/clang-prebuilt"
RUST_CACHE="$TOOLCHAIN_CACHE/rust-prebuilt"
CLANG_TOOLS_CACHE="$TOOLCHAIN_CACHE/clang-tools-prebuilt"
TOOLCHAIN_TARBALLS="$TOOLCHAIN_CACHE/tarballs"
CLANG_REPO_URL="https://android.googlesource.com/platform/prebuilts/clang/host/linux-x86"
RUST_REPO_URL="https://android.googlesource.com/platform/prebuilts/rust"
CLANG_TOOLS_REPO_URL="https://android.googlesource.com/platform/prebuilts/clang-tools"
PAHOLE_BIN="${PAHOLE:-$(command -v pahole || true)}"
TOOLCHAIN_BIN=""
CLANG_BIN=""
STRIP_BIN=""
OBJCOPY_BIN=""
RUST_TOOLCHAIN_ROOT=""
CLANG_TOOLS_ROOT=""
RUSTC_BIN=""
BINDGEN_BIN=""
TOOLCHAIN_ID=""
KBUILD_TOOL_ARGS=()
CLANG_VERSION_LINE=""
RUSTC_VERSION_LINE="-"
BINDGEN_VERSION_LINE="-"
CONFIG_EQUIVALENCE_SHA256=""
THIS_MODULE_SIZE=""
THIS_MODULE_ALIGNMENT=""
THIS_MODULE_DWARF_SIZE=""
MODVERSIONS_FORMAT=""
VERSIONS_SECTION_SIZE=""
VERSION_EXT_CRCS_SIZE=""
VERSION_EXT_NAMES_SIZE=""
JOBS="${KBUILD_JOBS:-1}"
SOURCE_REPO="https://android.googlesource.com/kernel/common"
HOST_PATH_RE='[[:alpha:]]:\\|(^|[^[:alpha:]])[[:alpha:]]:/|/(mnt/[[:alpha:]]/|home/[^/[:space:][:cntrl:]]+/|Users/[^/[:space:][:cntrl:]]+/|__w/|tmp/ethereal-kmod([./]|$)|root/(gki-src|qemu-build|gki-module-build|android-ndk|gki-toolchains)(/|$))'
HOST_EXTRACT_CERT_CFLAGS="-DUSE_PKCS11_ENGINE"

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

FETCH_ONLY=0
if [[ "${1:-}" == "--fetch-only" ]]; then
  FETCH_ONLY=1
  shift
fi
if [[ $# -gt 0 ]]; then
  KMIS=("$@")
else
  KMIS=("${DEFAULT_KMIS[@]}")
fi

mkdir -p "$PREBUILT" "$WORKDIR/tarballs" "$WORKDIR/.ethereal-source-locks" \
  "$MODULE_BUILD" "$MODULE_SOURCE_ROOT" "$OFFICIAL_CACHE" \
  "$CLANG_CACHE" "$RUST_CACHE" "$CLANG_TOOLS_CACHE" "$TOOLCHAIN_TARBALLS"
export PATH="/usr/bin:/bin:/usr/sbin:/sbin"
export ARCH=arm64 LLVM=1 LLVM_IAS=1
export CROSS_COMPILE=aarch64-linux-gnu-
export CROSS_COMPILE_COMPAT=arm-linux-gnueabi-
export CLANG_TRIPLE=aarch64-linux-gnu-

log() { printf '>> %s\n' "$*"; }

for command in awk base64 curl diff git make modinfo python3 readelf realpath \
  sha256sum strings tar; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "missing required command: $command" >&2
    exit 1
  }
done
[[ -s "$LOCKS" ]] || { echo "missing GKI lock file: $LOCKS" >&2; exit 1; }
[[ -s "$TOOLCHAIN_LOCKS" ]] || {
  echo "missing toolchain lock file: $TOOLCHAIN_LOCKS" >&2
  exit 1
}
# Branch protection is an ACK-wide code-generation choice. Setting it only in
# this external module makes the LTO metadata of ethereal.o and ethereal.mod.o
# disagree on older branches such as android12-5.4.
if grep -Eq -- '-mbranch-protection(=|[[:space:]])' "$KMOD/Makefile"; then
  echo "kmod/Makefile must inherit -mbranch-protection from the locked ACK tree" >&2
  exit 1
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
' "$TOOLCHAIN_LOCKS" || exit 1
[[ -x "$PAHOLE_BIN" ]] || { echo "missing pahole" >&2; exit 1; }
PAHOLE_VERSION_LINE="$($PAHOLE_BIN --version | awk 'NR == 1 { print; exit }')"
PAHOLE_VERSION="$(sed -nE 's/^v?([0-9]+\.[0-9]+(\.[0-9]+)?).*/\1/p' \
  <<<"$PAHOLE_VERSION_LINE")"
read -r PAHOLE_MAJOR PAHOLE_MINOR _ <<<"${PAHOLE_VERSION//./ }"
[[ -n "${PAHOLE_MAJOR:-}" && -n "${PAHOLE_MINOR:-}" ]] || {
  echo "cannot parse pahole version: $PAHOLE_VERSION_LINE" >&2
  exit 1
}
(( 10#$PAHOLE_MAJOR > 1 || (10#$PAHOLE_MAJOR == 1 && 10#$PAHOLE_MINOR >= 25) )) || {
  echo "pahole >= 1.25 is required, found $PAHOLE_VERSION_LINE" >&2
  exit 1
}
PAHOLE_SHA256="$(sha256sum "$PAHOLE_BIN" | awk '{ print $1 }')"
PAHOLE_VERSION_SHA256="$(printf '%s' "$PAHOLE_VERSION_LINE" | sha256sum | awk '{ print $1 }')"

safe_remove_dir() {
  local target="$1" base="$2" target_abs base_abs
  target_abs="$(realpath -m -- "$target")"
  base_abs="$(realpath -m -- "$base")"
  case "$target_abs" in
    "$base_abs"/*) ;;
    *) echo "refusing to reset path outside $base_abs: $target_abs" >&2; return 1 ;;
  esac
  [[ "$target_abs" != "$base_abs" ]] || {
    echo "refusing to remove build root: $base_abs" >&2
    return 1
  }
  rm -rf -- "$target_abs"
}

safe_reset_dir() {
  local target="$1" base="$2" target_abs
  target_abs="$(realpath -m -- "$target")"
  safe_remove_dir "$target_abs" "$base"
  mkdir -p -- "$target_abs"
}

read_lock() {
  local kmi="$1" row
  row="$(awk -F '\t' -v wanted="$kmi" '
    $1 == wanted && !found { print; found = 1 }
    END { if (!found) exit 1 }
  ' "$LOCKS")" || { echo "no pinned GKI lock for $kmi" >&2; return 1; }
  IFS=$'\t' read -r LOCK_KMI LOCK_SOURCE_REF LOCK_SOURCE_COMMIT \
    LOCK_SOURCE_TREE LOCK_PREBUILT_REF LOCK_PREBUILT_COMMIT \
    LOCK_PREBUILT_BUILD_ID LOCK_MANIFEST_ARTIFACT LOCK_MANIFEST_SHA256 \
    LOCK_PREBUILT_INFO_BLOB LOCK_ABI_ARTIFACT LOCK_ABI_BLOB \
    LOCK_KERNEL_ARTIFACT LOCK_KERNEL_BLOB LOCK_SYMVERS_ARTIFACT \
    LOCK_SYMVERS_BLOB <<<"$row"
}

read_toolchain_lock() {
  local kmi="$1" row
  row="$(awk -F '\t' -v wanted="$kmi" '
    $1 == wanted && !found { print; found = 1 }
    END { if (!found) exit 1 }
  ' "$TOOLCHAIN_LOCKS")" || {
    echo "no pinned toolchain lock for $kmi" >&2
    return 1
  }
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
     "$LOCK_PAHOLE_VERSION_SHA256" =~ ^[0-9a-f]{64}$ ]] || {
    echo "malformed toolchain lock for $kmi" >&2
    return 1
  }
  if [[ "$LOCK_RUST_COMMIT" == - ]]; then
    [[ "$LOCK_RUST_DIR" == - && "$LOCK_RUST_TREE" == - &&
       "$LOCK_RUSTC_SHA256" == - && "$LOCK_RUSTC_VERSION_SHA256" == - &&
       "$LOCK_CLANG_TOOLS_COMMIT" == - && "$LOCK_CLANG_TOOLS_DIR" == - &&
       "$LOCK_CLANG_TOOLS_TREE" == - &&
       "$LOCK_BINDGEN_SHA256" == - && "$LOCK_BINDGEN_VERSION_SHA256" == - ]] || {
      echo "$kmi has a partial no-Rust toolchain lock" >&2
      return 1
    }
  else
    [[ "$LOCK_RUST_COMMIT" =~ ^[0-9a-f]{40}$ &&
       "$LOCK_RUST_DIR" =~ ^linux-x86/[0-9]+\.[0-9]+\.[0-9]+b?$ &&
       "$LOCK_RUST_TREE" =~ ^[0-9a-f]{40}$ &&
       "$LOCK_RUSTC_SHA256" =~ ^[0-9a-f]{64}$ &&
       "$LOCK_RUSTC_VERSION_SHA256" =~ ^[0-9a-f]{64}$ &&
       "$LOCK_CLANG_TOOLS_COMMIT" =~ ^[0-9a-f]{40}$ &&
       "$LOCK_CLANG_TOOLS_DIR" =~ ^linux-x86(/[A-Za-z0-9._+-]+)*$ &&
       "$LOCK_CLANG_TOOLS_TREE" =~ ^[0-9a-f]{40}$ &&
       "$LOCK_BINDGEN_SHA256" =~ ^[0-9a-f]{64}$ &&
       "$LOCK_BINDGEN_VERSION_SHA256" =~ ^[0-9a-f]{64}$ ]] || {
      echo "$kmi has a malformed Rust toolchain lock" >&2
      return 1
    }
  fi
}

have_tree() {
  local dest="$1"
  [[ -f "$dest/Makefile" && -d "$dest/arch/arm64" && -d "$dest/scripts" && -d "$dest/include" ]]
}

flatten_tree() {
  local dest="$1" nested tmp
  [[ -f "$dest/Makefile" ]] && return 0
  nested="$(find "$dest" -mindepth 1 -maxdepth 2 -name Makefile -printf '%h\n' |
    awk 'NR == 1 { first = $0 } END { print first }')"
  if [[ -n "$nested" && -d "$nested" && "$nested" != "$dest" ]]; then
    tmp="${dest}.flat.$$"
    mv -- "$nested" "$tmp"
    safe_reset_dir "$dest" "$WORKDIR"
    rmdir -- "$dest"
    mv -- "$tmp" "$dest"
  fi
}

local_tree_hash() {
  local tree="$1" tmp hash
  if [[ -d "$tree/.git" ]]; then
    [[ -z "$(git -C "$tree" status --porcelain)" ]] || return 1
    git -C "$tree" rev-parse HEAD^{tree}
    return
  fi
  tmp="$(mktemp -d /tmp/ethereal-tree-hash.XXXXXX)"
  git init -q --bare "$tmp/repo.git"
  GIT_DIR="$tmp/repo.git" GIT_WORK_TREE="$tree" git add -f -A
  hash="$(GIT_DIR="$tmp/repo.git" git write-tree)"
  rm -rf -- "$tmp"
  printf '%s\n' "$hash"
}

fetch_locked_subtree() {
  local label="$1" repo="$2" commit="$3" subtree="$4" expected_tree="$5"
  local dest="$6" base="$7" tarpath partial actual_tree

  if [[ -d "$dest" ]]; then
    actual_tree="$(local_tree_hash "$dest" || true)"
    if [[ "$actual_tree" == "$expected_tree" ]]; then
      log "reuse pinned $label tree=$actual_tree"
      return 0
    fi
    log "$label cache tree mismatch; replacing $dest"
  fi

  safe_reset_dir "$dest" "$base"
  tarpath="$TOOLCHAIN_TARBALLS/${label}-${commit}.tar.gz"
  if [[ ! -s "$tarpath" ]] || ! tar -tzf "$tarpath" >/dev/null 2>&1; then
    partial="${tarpath}.part"
    rm -f -- "$partial"
    log "fetch pinned $label@$commit/$subtree"
    curl -L --fail --retry 8 --retry-all-errors --retry-delay 2 \
      --connect-timeout 30 -o "$partial" \
      "$repo/+archive/$commit/$subtree.tar.gz"
    tar -tzf "$partial" >/dev/null
    mv -f -- "$partial" "$tarpath"
  fi
  tar -xzf "$tarpath" -C "$dest"
  actual_tree="$(local_tree_hash "$dest")" || {
    echo "failed to hash $label subtree" >&2
    return 1
  }
  [[ "$actual_tree" == "$expected_tree" ]] || {
    echo "$label subtree mismatch: expected $expected_tree got $actual_tree" >&2
    return 1
  }
  log "verified $label tree=$actual_tree"
}

select_toolchain() {
  local kmi="$1" clang_revision rust_version

  read_toolchain_lock "$kmi" || return 1
  [[ "$PAHOLE_VERSION" == "$LOCK_PAHOLE_VERSION" &&
     "$PAHOLE_SHA256" == "$LOCK_PAHOLE_SHA256" &&
     "$PAHOLE_VERSION_SHA256" == "$LOCK_PAHOLE_VERSION_SHA256" ]] || {
    echo "$kmi pahole identity does not match toolchain-locks.tsv" >&2
    return 1
  }
  TOOLCHAIN_BIN="$CLANG_CACHE/$LOCK_CLANG_DIR/bin"
  CLANG_BIN="$TOOLCHAIN_BIN/clang"
  STRIP_BIN="$TOOLCHAIN_BIN/llvm-strip"
  OBJCOPY_BIN="$TOOLCHAIN_BIN/llvm-objcopy"
  TOOLCHAIN_ID="android-$LOCK_CLANG_DIR"
  clang_revision="${LOCK_CLANG_DIR#clang-}"

  fetch_locked_subtree "clang-${clang_revision}" "$CLANG_REPO_URL" \
    "$LOCK_CLANG_COMMIT" "$LOCK_CLANG_DIR" "$LOCK_CLANG_TREE" \
    "$CLANG_CACHE/$LOCK_CLANG_DIR" "$CLANG_CACHE" || return 1
  [[ -x "$CLANG_BIN" && -x "$STRIP_BIN" && -x "$OBJCOPY_BIN" &&
     -x "$TOOLCHAIN_BIN/llvm-nm" ]] || {
    echo "$kmi pinned Clang subtree is incomplete: $TOOLCHAIN_BIN" >&2
    return 1
  }
  [[ "$(sha256sum "$CLANG_BIN" | awk '{ print $1 }')" == "$LOCK_CLANG_SHA256" ]] || {
    echo "$kmi Clang binary does not match the locked SHA-256" >&2
    return 1
  }
  CLANG_VERSION_LINE="$($CLANG_BIN --version | awk 'NR == 1 { print; exit }')"
  grep -Fq "based on $clang_revision) clang version" <<<"$CLANG_VERSION_LINE" || {
    echo "$kmi Clang identity does not match $LOCK_CLANG_DIR: $CLANG_VERSION_LINE" >&2
    return 1
  }
  [[ "$(printf '%s' "$CLANG_VERSION_LINE" | sha256sum | awk '{ print $1 }')" == \
      "$LOCK_CLANG_VERSION_SHA256" ]] || {
    echo "$kmi Clang version line does not match the lock" >&2
    return 1
  }

  RUST_TOOLCHAIN_ROOT=""
  CLANG_TOOLS_ROOT=""
  RUSTC_BIN=""
  BINDGEN_BIN=""
  RUSTC_VERSION_LINE="-"
  BINDGEN_VERSION_LINE="-"
  unset RUSTC BINDGEN
  unset LIBCLANG_PATH RUST_LIB_SRC
  if [[ "$LOCK_RUST_COMMIT" != - ]]; then
    rust_version="${LOCK_RUST_DIR##*/}"
    RUST_TOOLCHAIN_ROOT="$RUST_CACHE/$rust_version"
    fetch_locked_subtree "rust-${rust_version}" "$RUST_REPO_URL" \
      "$LOCK_RUST_COMMIT" "$LOCK_RUST_DIR" "$LOCK_RUST_TREE" \
      "$RUST_TOOLCHAIN_ROOT" "$RUST_CACHE" || return 1
    RUSTC_BIN="$RUST_TOOLCHAIN_ROOT/bin/rustc"
    CLANG_TOOLS_ROOT="$CLANG_TOOLS_CACHE/$LOCK_CLANG_TOOLS_COMMIT"
    fetch_locked_subtree "clang-tools-${LOCK_CLANG_TOOLS_COMMIT:0:12}" \
      "$CLANG_TOOLS_REPO_URL" "$LOCK_CLANG_TOOLS_COMMIT" \
      "$LOCK_CLANG_TOOLS_DIR" "$LOCK_CLANG_TOOLS_TREE" \
      "$CLANG_TOOLS_ROOT" "$CLANG_TOOLS_CACHE" || return 1
    BINDGEN_BIN="$CLANG_TOOLS_ROOT/bin/bindgen"
    [[ -x "$RUSTC_BIN" ]] || {
      echo "$kmi pinned Rust subtree is incomplete: $RUST_TOOLCHAIN_ROOT" >&2
      return 1
    }
    [[ -x "$BINDGEN_BIN" ]] || {
      echo "$kmi pinned clang-tools subtree has no bindgen: $CLANG_TOOLS_ROOT" >&2
      return 1
    }
    [[ "$(sha256sum "$RUSTC_BIN" | awk '{ print $1 }')" == "$LOCK_RUSTC_SHA256" &&
       "$(sha256sum "$BINDGEN_BIN" | awk '{ print $1 }')" == "$LOCK_BINDGEN_SHA256" ]] || {
      echo "$kmi Rust or bindgen binary does not match the locked SHA-256" >&2
      return 1
    }
    RUSTC_VERSION_LINE="$($RUSTC_BIN --version | awk 'NR == 1 { print; exit }')"
    BINDGEN_VERSION_LINE="$($BINDGEN_BIN --version | awk 'NR == 1 { print; exit }')"
    grep -Fq "rustc ${rust_version%b}" <<<"$RUSTC_VERSION_LINE" || {
      echo "$kmi Rust identity does not match $rust_version: $RUSTC_VERSION_LINE" >&2
      return 1
    }
    [[ "$(printf '%s' "$RUSTC_VERSION_LINE" | sha256sum | awk '{ print $1 }')" == \
        "$LOCK_RUSTC_VERSION_SHA256" &&
       "$(printf '%s' "$BINDGEN_VERSION_LINE" | sha256sum | awk '{ print $1 }')" == \
        "$LOCK_BINDGEN_VERSION_SHA256" ]] || {
      echo "$kmi Rust or bindgen version line does not match the lock" >&2
      return 1
    }
    export LIBCLANG_PATH="$CLANG_CACHE/$LOCK_CLANG_DIR/lib"
    if [[ -d "$RUST_TOOLCHAIN_ROOT/lib/rustlib/src/rust/library" ]]; then
      export RUST_LIB_SRC="$RUST_TOOLCHAIN_ROOT/lib/rustlib/src/rust/library"
    fi
  fi
  KBUILD_TOOL_ARGS=("PAHOLE=$PAHOLE_BIN")
  if [[ -n "$RUSTC_BIN" ]]; then
    KBUILD_TOOL_ARGS+=("RUSTC=$RUSTC_BIN" "BINDGEN=$BINDGEN_BIN")
  fi
  export PATH="$TOOLCHAIN_BIN:/usr/bin:/bin:/usr/sbin:/sbin"
  log "$kmi clang=$LOCK_CLANG_DIR tree=$LOCK_CLANG_TREE sha256=$LOCK_CLANG_SHA256 pahole=$PAHOLE_VERSION"
}

write_source_marker() {
  local marker="$1" archive_sha="$2" tmp="${marker}.tmp.$$"
  {
    printf 'source_commit=%s\n' "$LOCK_SOURCE_COMMIT"
    printf 'source_tree=%s\n' "$LOCK_SOURCE_TREE"
    printf 'source_archive_sha256=%s\n' "$archive_sha"
  } >"$tmp"
  mv -f -- "$tmp" "$marker"
}

marker_value() {
  local marker="$1" key="$2"
  awk -F '=' -v wanted="$key" '
    $1 == wanted && !found { print substr($0, index($0, "=") + 1); found = 1 }
  ' "$marker"
}

fetch_locked_source() {
  local kmi="$1" dest marker tarpath partial archive_sha actual_tree head tree
  read_lock "$kmi" || return 1
  dest="$WORKDIR/$kmi"
  marker="$WORKDIR/.ethereal-source-locks/$kmi.env"
  tarpath="$WORKDIR/tarballs/${kmi}-${LOCK_SOURCE_COMMIT}.tar.gz"

  if have_tree "$dest" && [[ -s "$marker" ]] &&
     [[ "$(marker_value "$marker" source_commit)" == "$LOCK_SOURCE_COMMIT" ]] &&
     [[ "$(marker_value "$marker" source_tree)" == "$LOCK_SOURCE_TREE" ]]; then
    SOURCE_ARCHIVE_SHA256="$(marker_value "$marker" source_archive_sha256)"
    log "reuse pinned source $kmi@$LOCK_SOURCE_COMMIT"
    return 0
  fi

  if have_tree "$dest" && [[ -d "$dest/.git" ]] &&
     [[ -z "$(git -C "$dest" status --porcelain)" ]]; then
    head="$(git -C "$dest" rev-parse HEAD)"
    tree="$(git -C "$dest" rev-parse HEAD^{tree})"
    if [[ "$head" == "$LOCK_SOURCE_COMMIT" && "$tree" == "$LOCK_SOURCE_TREE" ]]; then
      SOURCE_ARCHIVE_SHA256="$(printf 'git:%s' "$head" | sha256sum | awk '{ print $1 }')"
      write_source_marker "$marker" "$SOURCE_ARCHIVE_SHA256"
      log "adopt verified Git source $kmi@$head"
      return 0
    fi
  fi

  safe_reset_dir "$dest" "$WORKDIR"
  if [[ ! -s "$tarpath" ]] || ! tar -tzf "$tarpath" >/dev/null 2>&1; then
    partial="${tarpath}.part"
    log "fetch pinned source $kmi@$LOCK_SOURCE_COMMIT"
    curl -L --fail --retry 5 --retry-all-errors --retry-delay 2 \
      --connect-timeout 30 --continue-at - \
      -o "$partial" "$SOURCE_REPO/+archive/$LOCK_SOURCE_COMMIT.tar.gz"
    tar -tzf "$partial" >/dev/null
    mv -f -- "$partial" "$tarpath"
  fi
  tar -xzf "$tarpath" -C "$dest"
  flatten_tree "$dest"
  have_tree "$dest" || { echo "pinned archive is not a kernel tree: $tarpath" >&2; return 1; }
  actual_tree="$(local_tree_hash "$dest")" || {
    echo "failed to hash extracted source tree for $kmi" >&2
    return 1
  }
  [[ "$actual_tree" == "$LOCK_SOURCE_TREE" ]] || {
    echo "$kmi source tree mismatch: expected $LOCK_SOURCE_TREE got $actual_tree" >&2
    return 1
  }
  archive_sha="$(sha256sum "$tarpath" | awk '{ print $1 }')"
  SOURCE_ARCHIVE_SHA256="$archive_sha"
  write_source_marker "$marker" "$archive_sha"
  log "verified source tree $actual_tree archive sha256=$archive_sha"
}

git_blob_sha1() {
  git hash-object --no-filters "$1"
}

fetch_prebuilt_blob() {
  local version="$1" artifact="$2" expected="$3" output="$4"
  local partial="${output}.part" actual
  if [[ -s "$output" ]] && [[ "$(git_blob_sha1 "$output")" == "$expected" ]]; then
    return 0
  fi
  rm -f -- "$partial"
  log "fetch official $LOCK_KMI $artifact"
  curl -L --fail --retry 8 --retry-all-errors --retry-delay 2 \
    --connect-timeout 30 \
    "https://android.googlesource.com/kernel/prebuilts/$version/arm64/+/$LOCK_PREBUILT_COMMIT/$artifact?format=TEXT" |
    base64 -d >"$partial"
  [[ -s "$partial" ]] || { echo "$artifact decoded to an empty file" >&2; return 1; }
  actual="$(git_blob_sha1 "$partial")"
  [[ "$actual" == "$expected" ]] || {
    echo "$artifact Git blob mismatch: expected $expected got $actual" >&2
    return 1
  }
  mv -f -- "$partial" "$output"
}

verify_manifest() {
  python3 - "$1" "$LOCK_SOURCE_COMMIT" "$LOCK_PREBUILT_BUILD_ID" \
    "$LOCK_CLANG_COMMIT" "$LOCK_RUST_COMMIT" "$LOCK_CLANG_TOOLS_COMMIT" <<'PY'
import sys
import xml.etree.ElementTree as ET

(
    path,
    expected_revision,
    expected_build,
    expected_clang,
    expected_rust,
    expected_clang_tools,
) = sys.argv[1:]
root = ET.parse(path).getroot()

def revision(name, required=True):
    matches = [node for node in root.findall("project") if node.attrib.get("name") == name]
    if len(matches) != (1 if required else 0) and not (not required and len(matches) == 1):
        raise SystemExit(f"manifest has an invalid {name} project count")
    return matches[0].attrib.get("revision") if matches else None

if revision("kernel/common") != expected_revision:
    raise SystemExit("manifest kernel/common revision does not match the lock")
if revision("platform/prebuilts/clang/host/linux-x86") != expected_clang:
    raise SystemExit("manifest Clang revision does not match the toolchain lock")
rust = revision("platform/prebuilts/rust", required=expected_rust != "-")
if expected_rust != "-" and rust != expected_rust:
    raise SystemExit("manifest Rust revision does not match the toolchain lock")
clang_tools = revision(
    "platform/prebuilts/clang-tools", required=expected_clang_tools != "-"
)
if expected_clang_tools != "-" and clang_tools != expected_clang_tools:
    raise SystemExit("manifest clang-tools revision does not match the toolchain lock")
if expected_build not in path and not path.endswith("manifest.xml"):
    raise SystemExit("unexpected committed manifest path")
PY
}

prepare_official_artifacts() {
  local kmi="$1" kdir="$2" version official manifest info abi kernel
  local abi_symvers source_prefix
  version="${kmi##*-}"
  official="$OFFICIAL_CACHE/$kmi"
  manifest="$PREBUILT/$kmi/manifest.xml"
  info="$official/prebuilt-info.txt"
  abi="$official/$LOCK_ABI_ARTIFACT"
  kernel="$official/$LOCK_KERNEL_ARTIFACT"
  abi_symvers="$official/abi.symvers"
  mkdir -p "$official"

  [[ -s "$manifest" ]] || { echo "$kmi missing committed manifest.xml" >&2; return 1; }
  [[ "$(sha256sum "$manifest" | awk '{ print $1 }')" == "$LOCK_MANIFEST_SHA256" ]] || {
    echo "$kmi committed manifest SHA-256 mismatch" >&2
    return 1
  }
  verify_manifest "$manifest"

  fetch_prebuilt_blob "$version" prebuilt-info.txt "$LOCK_PREBUILT_INFO_BLOB" "$info"
  grep -Eq "\"kernel-build-id\"[[:space:]]*:[[:space:]]*\"?$LOCK_PREBUILT_BUILD_ID\"?" "$info" || {
    echo "$kmi prebuilt-info build id mismatch" >&2
    return 1
  }
  fetch_prebuilt_blob "$version" "$LOCK_ABI_ARTIFACT" "$LOCK_ABI_BLOB" "$abi"
  fetch_prebuilt_blob "$version" "$LOCK_KERNEL_ARTIFACT" "$LOCK_KERNEL_BLOB" "$kernel"
  if [[ "$LOCK_SYMVERS_ARTIFACT" != - ]]; then
    fetch_prebuilt_blob "$version" "$LOCK_SYMVERS_ARTIFACT" \
      "$LOCK_SYMVERS_BLOB" "$official/$LOCK_SYMVERS_ARTIFACT"
    grep -Eq '^0x[0-9a-fA-F]{8}[[:space:]]' "$official/$LOCK_SYMVERS_ARTIFACT" || {
      echo "$kmi locked symvers is not a real symbol table" >&2
      return 1
    }
  fi

  python3 "$KMOD/abi-to-symvers.py" "$abi" "$abi_symvers"
  if [[ "$LOCK_SYMVERS_ARTIFACT" != - ]]; then
    python3 - "$abi_symvers" "$official/$LOCK_SYMVERS_ARTIFACT" <<'PY'
import sys

def read(path):
    result = {}
    with open(path, "r", encoding="ascii") as stream:
        for line in stream:
            fields = line.split()
            if len(fields) >= 2:
                result[fields[1]] = int(fields[0], 0)
    return result

abi, real = map(read, sys.argv[1:])
mismatch = sorted(name for name in abi.keys() & real.keys() if abi[name] != real[name])
if mismatch:
    raise SystemExit(
        "official ABI/symvers CRC disagreement: mismatch={}".format(len(mismatch))
    )
PY
    OFFICIAL_SYMVERS="$official/$LOCK_SYMVERS_ARTIFACT"
  else
    OFFICIAL_SYMVERS="$abi_symvers"
  fi

  OFFICIAL_RELEASE="$(strings "$kernel" | awk \
    -v prefix="$version." -v build_id="$LOCK_PREBUILT_BUILD_ID" '
      index($0, prefix) == 1 && index($0, "-android") > 0 &&
      $0 ~ ("-ab" build_id "(-(4k|16k))?$") && !found { print; found = 1 }
    ')"
  [[ -n "$OFFICIAL_RELEASE" ]] || {
    echo "$kmi official Image has no exact Android release string" >&2
    return 1
  }
  source_prefix="${LOCK_SOURCE_COMMIT:0:12}"
  case "$OFFICIAL_RELEASE" in
    *"-g$source_prefix-ab$LOCK_PREBUILT_BUILD_ID"|\
    *"-g$source_prefix-ab$LOCK_PREBUILT_BUILD_ID-4k"|\
    *"-g$source_prefix-ab$LOCK_PREBUILT_BUILD_ID-16k") ;;
    *)
      echo "$kmi Image release does not identify the locked source/build" >&2
      return 1
      ;;
  esac
  OFFICIAL_CONFIG="$official/official.config"
  bash "$kdir/scripts/extract-ikconfig" "$kernel" >"${OFFICIAL_CONFIG}.tmp"
  grep -qxF CONFIG_MODULES=y "${OFFICIAL_CONFIG}.tmp" &&
    grep -qxF CONFIG_MODVERSIONS=y "${OFFICIAL_CONFIG}.tmp" || {
      echo "$kmi official Image has no usable embedded module config" >&2
      return 1
    }
  mv -f -- "${OFFICIAL_CONFIG}.tmp" "$OFFICIAL_CONFIG"
  OFFICIAL_ABI_SYMVERS="$abi_symvers"

  if awk '$2 == "register_kprobe" { r = 1 } $2 == "unregister_kprobe" { u = 1 }
      END { exit !(r && u) }' "$OFFICIAL_SYMVERS" &&
     awk '$2 == "register_kprobe" { r = 1 } $2 == "unregister_kprobe" { u = 1 }
      END { exit !(r && u) }' "$OFFICIAL_ABI_SYMVERS"; then
    BOOTSTRAP_CPPFLAGS=""
    BOOTSTRAP_KIND=kprobe
  elif awk '$2 == "sprint_symbol" { found = 1 } END { exit !found }' \
      "$OFFICIAL_SYMVERS" &&
       awk '$2 == "sprint_symbol" { found = 1 } END { exit !found }' \
      "$OFFICIAL_ABI_SYMVERS"; then
    BOOTSTRAP_CPPFLAGS="-DETHEREAL_KLN_VIA_SPRINT_SYMBOL=1"
    BOOTSTRAP_KIND=sprint-symbol-scan
  else
    echo "$kmi official ABI exposes neither kprobe bootstrap nor sprint_symbol" >&2
    return 1
  fi
  log "$kmi official release=$OFFICIAL_RELEASE bootstrap=$BOOTSTRAP_KIND"
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

verify_config_equivalence() {
  local kmi="$1" canonical="$2" prepared="$3" diff_output
  local canonical_sha prepared_sha pahole_config

  canonical_sha="$(config_projection "$canonical" | sha256sum | awk '{ print $1 }')"
  prepared_sha="$(config_projection "$prepared" | sha256sum | awk '{ print $1 }')"
  if [[ "$canonical_sha" != "$prepared_sha" ]]; then
    diff_output="$(diff -u <(config_projection "$canonical") \
      <(config_projection "$prepared") || true)"
    printf '%s\n' "$diff_output" >&2
    echo "$kmi prepared config is not ABI-equivalent to the official Image config" >&2
    return 1
  fi
  CONFIG_EQUIVALENCE_SHA256="$canonical_sha"

  grep -qxF "CONFIG_CC_VERSION_TEXT=\"$CLANG_VERSION_LINE\"" "$prepared" || {
    echo "$kmi prepared config does not identify the locked Clang" >&2
    return 1
  }
  if grep -qxF CONFIG_DEBUG_INFO_BTF_MODULES=y "$canonical"; then
    grep -qxF CONFIG_DEBUG_INFO_BTF_MODULES=y "$prepared" || {
      echo "$kmi lost CONFIG_DEBUG_INFO_BTF_MODULES while preparing headers" >&2
      return 1
    }
  fi
  pahole_config="$(awk -F= '$1 == "CONFIG_PAHOLE_VERSION" { print $2; exit }' "$prepared")"
  if [[ -n "$pahole_config" ]]; then
    [[ "$pahole_config" =~ ^[0-9]+$ ]] && (( 10#$pahole_config >= 125 )) || {
      echo "$kmi prepared config did not detect pahole >= 1.25" >&2
      return 1
    }
  fi
  if [[ "$LOCK_RUST_COMMIT" != - ]]; then
    grep -qxF CONFIG_RUST=y "$canonical" && grep -qxF CONFIG_RUST=y "$prepared" || {
      echo "$kmi locked Rust toolchain did not preserve CONFIG_RUST=y" >&2
      return 1
    }
  fi
}

prepare_canonical_build() {
  local kmi="$1" kdir="$2" bdir="$3" defconfig build_key marker
  local config_sha defconfig_sha prepare_marker prepare_key=""
  local -a prepare_make_args=()
  defconfig="$kdir/arch/arm64/configs/gki_defconfig"
  [[ -s "$defconfig" ]] || {
    echo "$kmi pinned source has no arch/arm64/configs/gki_defconfig" >&2
    return 1
  }
  defconfig_sha="$(sha256sum "$defconfig" | awk '{ print $1 }')"
  config_sha="$(sha256sum "$OFFICIAL_CONFIG" | awk '{ print $1 }')"
  build_key="${LOCK_SOURCE_COMMIT}:${LOCK_SOURCE_TREE}:${LOCK_KERNEL_BLOB}:${LOCK_ABI_BLOB}:${LOCK_SYMVERS_BLOB}:${LOCK_CLANG_SHA256}:${LOCK_RUSTC_SHA256}:${LOCK_BINDGEN_SHA256}:${LOCK_PAHOLE_SHA256}:$(sha256sum "$TOOLCHAIN_LOCKS" | awk '{ print $1 }'):${config_sha}:${OFFICIAL_RELEASE}:${BOOTSTRAP_KIND}:official-modules-prepare-v4"
  marker="$bdir/.ethereal-build-key"
  prepare_marker="$bdir/.ethereal-modules-prepare-key"
  if [[ ! -f "$marker" || "$(tr -d '\r\n' < "$marker")" != "$build_key" ]]; then
    safe_reset_dir "$bdir" "$MODULE_BUILD"
    printf '%s\n' "$build_key" >"$marker"
  fi
  if [[ -f "$prepare_marker" ]]; then
    prepare_key="$(tr -d '\r\n' < "$prepare_marker")"
  fi
  if [[ "$kmi" == android16-6.12 ]]; then
    prepare_make_args+=(skip_gendwarfksyms=1)
  fi
  if [[ ! -s "$bdir/include/config/auto.conf" || "$prepare_key" != "$build_key" ]]; then
    rm -f -- "$prepare_marker"
    log "$kmi prepare exact official Image config"
    cp -f "$OFFICIAL_CONFIG" "$bdir/.config"
    # Official 5.4/5.10 configs retain a buildbot-only absolute whitelist
    # path. The locked ABI symvers supplies the exact CRC/export surface, so
    # modules_prepare must not depend on that unavailable host file.
    if grep -q '^CONFIG_UNUSED_KSYMS_WHITELIST=' "$bdir/.config"; then
      "$kdir/scripts/config" --file "$bdir/.config" \
        --set-str UNUSED_KSYMS_WHITELIST ""
    fi
    make -C "$kdir" O="$bdir" HOSTCC=gcc HOSTCXX=g++ \
      "HOSTCFLAGS_extract-cert.o=$HOST_EXTRACT_CERT_CFLAGS" \
      LLVM=1 LLVM_IAS=1 ARCH=arm64 KERNELRELEASE="$OFFICIAL_RELEASE" \
      "${KBUILD_TOOL_ARGS[@]}" olddefconfig || return 1
    verify_config_equivalence "$kmi" "$OFFICIAL_CONFIG" "$bdir/.config" || return 1
    make -C "$kdir" O="$bdir" HOSTCC=gcc HOSTCXX=g++ \
      "HOSTCFLAGS_extract-cert.o=$HOST_EXTRACT_CERT_CFLAGS" \
      LLVM=1 LLVM_IAS=1 ARCH=arm64 KERNELRELEASE="$OFFICIAL_RELEASE" \
      "${KBUILD_TOOL_ARGS[@]}" "${prepare_make_args[@]}" \
      modules_prepare -j"$JOBS" || return 1
    printf '%s\n' "$build_key" >"$prepare_marker"
  fi
  verify_config_equivalence "$kmi" "$OFFICIAL_CONFIG" "$bdir/.config" || return 1
  for required in CONFIG_MODULES=y CONFIG_MODVERSIONS=y CONFIG_KPROBES=y CONFIG_KALLSYMS=y; do
    grep -qxF "$required" "$bdir/.config" || {
      echo "$kmi canonical config lacks $required" >&2
      return 1
    }
  done
  if [[ "$kmi" == android16-6.12 ]]; then
    for required in CONFIG_CLANG_VERSION=190001 CONFIG_AUTOFDO_CLANG=y \
      CONFIG_CFI_CLANG=y CONFIG_CFI_ICALL_NORMALIZE_INTEGERS=y \
      CONFIG_LTO_NONE=y; do
      grep -qxF "$required" "$bdir/.config" || {
        echo "$kmi canonical config lacks $required" >&2
        return 1
      }
    done
  fi
  mkdir -p "$bdir/include/config" "$bdir/include/generated"
  printf '%s\n' "$OFFICIAL_RELEASE" >"$bdir/include/config/kernel.release"
  printf '#define UTS_RELEASE "%s"\n' "$OFFICIAL_RELEASE" >"$bdir/include/generated/utsrelease.h"
  cp -f "$OFFICIAL_SYMVERS" "$bdir/Module.symvers"
  CANONICAL_CONFIG_SHA256="$config_sha"
  PREPARED_CONFIG_SHA256="$(sha256sum "$bdir/.config" | awk '{ print $1 }')"
  GKI_DEFCONFIG_SHA256="$defconfig_sha"
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

dwarf_struct_module_size() {
  local ko="$1" dwarf_copy="${1}.dwarf.$$" size

  cp -f -- "$ko" "$dwarf_copy"
  if ! "$OBJCOPY_BIN" --decompress-debug-sections "$dwarf_copy"; then
    rm -f -- "$dwarf_copy"
    return 1
  fi
  if ! size="$("$PAHOLE_BIN" -C module "$dwarf_copy" 2>/dev/null | sed -nE \
    's@.*\/\* size: ([0-9]+),.*@\1@p' | awk '
      NR == 1 { size = $1 }
      $1 != size { mismatch = 1 }
      END {
        if (NR == 0 || mismatch) exit 2
        print size
      }
    ')"; then
    rm -f -- "$dwarf_copy"
    return 1
  fi
  rm -f -- "$dwarf_copy"
  printf '%s\n' "$size"
}

inspect_this_module() {
  local ko="$1" require_dwarf="$2" info size_hex alignment flags symbol_size
  local dwarf_size=""
  info="$(this_module_section_info "$ko")" || {
    echo "$ko has no unique PROGBITS .gnu.linkonce.this_module section" >&2
    return 1
  }
  read -r size_hex alignment flags <<<"$info"
  [[ "$size_hex" =~ ^[0-9a-fA-F]+$ && "$alignment" =~ ^[0-9]+$ &&
     "$flags" == *W* && "$flags" == *A* ]] || {
    echo "$ko has malformed __this_module section metadata: $info" >&2
    return 1
  }
  THIS_MODULE_SIZE=$((16#$size_hex))
  THIS_MODULE_ALIGNMENT=$((10#$alignment))
  (( THIS_MODULE_SIZE >= 256 && THIS_MODULE_SIZE <= 8192 &&
     THIS_MODULE_ALIGNMENT >= 8 &&
     (THIS_MODULE_ALIGNMENT & (THIS_MODULE_ALIGNMENT - 1)) == 0 )) || {
    echo "$ko has implausible __this_module layout size=$THIS_MODULE_SIZE align=$THIS_MODULE_ALIGNMENT" >&2
    return 1
  }
  symbol_size="$(this_module_symbol_size "$ko")" || {
    echo "$ko has no unique OBJECT __this_module symbol" >&2
    return 1
  }
  [[ "$symbol_size" =~ ^[0-9]+$ && "$symbol_size" == "$THIS_MODULE_SIZE" ]] || {
    echo "$ko __this_module symbol/section size mismatch: $symbol_size/$THIS_MODULE_SIZE" >&2
    return 1
  }
  if (( require_dwarf )); then
    dwarf_size="$(dwarf_struct_module_size "$ko")" || {
      echo "$ko has no unique DWARF struct module layout" >&2
      return 1
    }
    [[ "$dwarf_size" =~ ^[0-9]+$ && "$dwarf_size" == "$THIS_MODULE_SIZE" ]] || {
      echo "$ko DWARF/ELF struct module size mismatch: $dwarf_size/$THIS_MODULE_SIZE" >&2
      return 1
    }
    THIS_MODULE_DWARF_SIZE="$dwarf_size"
  fi
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
       VERSION_EXT_NAMES_SIZE > 0 )) || {
      echo "$ko lacks a valid extended modversions section pair" >&2
      return 1
    }
  else
    MODVERSIONS_FORMAT=basic
    (( VERSION_EXT_CRCS_SIZE == 0 && VERSION_EXT_NAMES_SIZE == 0 )) || {
      echo "$ko has unexpected extended modversions sections" >&2
      return 1
    }
  fi
  (( VERSIONS_SECTION_SIZE > 0 || VERSION_EXT_CRCS_SIZE > 0 )) || {
    echo "$ko has no versioned dependencies" >&2
    return 1
  }
}

write_provenance() {
  local kmi="$1" outdir="$2" ko="$3"
  local tmp="$outdir/provenance.env.tmp.$$"
  local rust_repo=- clang_tools_repo=-
  if [[ "$LOCK_RUST_COMMIT" != - ]]; then
    rust_repo="$RUST_REPO_URL"
    clang_tools_repo="$CLANG_TOOLS_REPO_URL"
  fi
  {
    printf 'format=ethereal-gki-provenance-v3\n'
    printf 'kmi=%s\n' "$kmi"
    printf 'source_ref=refs/heads/%s\n' "$LOCK_SOURCE_REF"
    printf 'source_commit=%s\n' "$LOCK_SOURCE_COMMIT"
    printf 'source_tree=%s\n' "$LOCK_SOURCE_TREE"
    printf 'source_archive_sha256=%s\n' "$SOURCE_ARCHIVE_SHA256"
    printf 'prebuilt_ref=refs/heads/%s\n' "$LOCK_PREBUILT_REF"
    printf 'prebuilt_commit=%s\n' "$LOCK_PREBUILT_COMMIT"
    printf 'prebuilt_build_id=%s\n' "$LOCK_PREBUILT_BUILD_ID"
    printf 'manifest_artifact=%s\n' "$LOCK_MANIFEST_ARTIFACT"
    printf 'manifest_sha256=%s\n' "$LOCK_MANIFEST_SHA256"
    printf 'prebuilt_info_blob=%s\n' "$LOCK_PREBUILT_INFO_BLOB"
    printf 'abi_artifact=%s\n' "$LOCK_ABI_ARTIFACT"
    printf 'abi_blob=%s\n' "$LOCK_ABI_BLOB"
    printf 'abi_sha256=%s\n' "$(sha256sum "$OFFICIAL_CACHE/$kmi/$LOCK_ABI_ARTIFACT" | awk '{ print $1 }')"
    printf 'kernel_artifact=%s\n' "$LOCK_KERNEL_ARTIFACT"
    printf 'kernel_blob=%s\n' "$LOCK_KERNEL_BLOB"
    printf 'kernel_sha256=%s\n' "$(sha256sum "$OFFICIAL_CACHE/$kmi/$LOCK_KERNEL_ARTIFACT" | awk '{ print $1 }')"
    printf 'symvers_artifact=%s\n' "$LOCK_SYMVERS_ARTIFACT"
    printf 'symvers_blob=%s\n' "$LOCK_SYMVERS_BLOB"
    printf 'official_release=%s\n' "$OFFICIAL_RELEASE"
    printf 'bootstrap=%s\n' "$BOOTSTRAP_KIND"
    printf 'build_config=official-image-ikconfig\n'
    printf 'gki_defconfig_sha256=%s\n' "$GKI_DEFCONFIG_SHA256"
    printf 'host_extract_cert_cflags=%s\n' "$HOST_EXTRACT_CERT_CFLAGS"
    printf 'config_sha256=%s\n' "$CANONICAL_CONFIG_SHA256"
    printf 'prepared_config_sha256=%s\n' "$PREPARED_CONFIG_SHA256"
    printf 'config_equivalence_sha256=%s\n' "$CONFIG_EQUIVALENCE_SHA256"
    printf 'toolchain_id=%s\n' "$TOOLCHAIN_ID"
    printf 'clang_repo=%s\n' "$CLANG_REPO_URL"
    printf 'clang_commit=%s\n' "$LOCK_CLANG_COMMIT"
    printf 'clang_dir=%s\n' "$LOCK_CLANG_DIR"
    printf 'clang_subtree_tree=%s\n' "$LOCK_CLANG_TREE"
    printf 'clang_sha256=%s\n' "$LOCK_CLANG_SHA256"
    printf 'clang_version_sha256=%s\n' "$LOCK_CLANG_VERSION_SHA256"
    printf 'rust_repo=%s\n' "$rust_repo"
    printf 'rust_commit=%s\n' "$LOCK_RUST_COMMIT"
    printf 'rust_dir=%s\n' "$LOCK_RUST_DIR"
    printf 'rust_subtree_tree=%s\n' "$LOCK_RUST_TREE"
    printf 'rustc_sha256=%s\n' "$LOCK_RUSTC_SHA256"
    printf 'rustc_version_sha256=%s\n' "$LOCK_RUSTC_VERSION_SHA256"
    printf 'clang_tools_repo=%s\n' "$clang_tools_repo"
    printf 'clang_tools_commit=%s\n' "$LOCK_CLANG_TOOLS_COMMIT"
    printf 'clang_tools_dir=%s\n' "$LOCK_CLANG_TOOLS_DIR"
    printf 'clang_tools_subtree_tree=%s\n' "$LOCK_CLANG_TOOLS_TREE"
    printf 'bindgen_sha256=%s\n' "$LOCK_BINDGEN_SHA256"
    printf 'bindgen_version_sha256=%s\n' "$LOCK_BINDGEN_VERSION_SHA256"
    printf 'pahole_version=%s\n' "$LOCK_PAHOLE_VERSION"
    printf 'pahole_sha256=%s\n' "$LOCK_PAHOLE_SHA256"
    printf 'pahole_version_sha256=%s\n' "$LOCK_PAHOLE_VERSION_SHA256"
    printf 'this_module_size=%s\n' "$THIS_MODULE_SIZE"
    printf 'this_module_alignment=%s\n' "$THIS_MODULE_ALIGNMENT"
    printf 'struct_module_dwarf_size=%s\n' "$THIS_MODULE_DWARF_SIZE"
    printf 'modversions_format=%s\n' "$MODVERSIONS_FORMAT"
    printf 'versions_section_size=%s\n' "$VERSIONS_SECTION_SIZE"
    printf 'version_ext_crcs_size=%s\n' "$VERSION_EXT_CRCS_SIZE"
    printf 'version_ext_names_size=%s\n' "$VERSION_EXT_NAMES_SIZE"
    printf 'module_symvers_sha256=%s\n' "$(sha256sum "$outdir/Module.symvers" | awk '{ print $1 }')"
    printf 'official_abi_symvers_sha256=%s\n' "$(sha256sum "$outdir/official-abi.symvers" | awk '{ print $1 }')"
    printf 'canonical_projection_sha256=%s\n' "$(sha256sum "$outdir/canonical.projected.symvers" | awk '{ print $1 }')"
    printf 'abi_projection_sha256=%s\n' "$(sha256sum "$outdir/abi.projected.symvers" | awk '{ print $1 }')"
    printf 'ethereal_c_sha256=%s\n' "$(sha256sum "$KMOD/ethereal.c" | awk '{ print $1 }')"
    printf 'kmod_makefile_sha256=%s\n' "$(sha256sum "$KMOD/Makefile" | awk '{ print $1 }')"
    printf 'abi_to_symvers_sha256=%s\n' "$(sha256sum "$KMOD/abi-to-symvers.py" | awk '{ print $1 }')"
    printf 'manager_cert_sha256=%s\n' "$(sha256sum "$KMOD/manager_cert.h" | awk '{ print $1 }')"
    printf 'feature_marker_sha256=%s\n' "$(sha256sum "$KMOD/feature-marker.txt" | awk '{ print $1 }')"
    printf 'gki_locks_sha256=%s\n' "$(sha256sum "$LOCKS" | awk '{ print $1 }')"
    printf 'toolchain_locks_sha256=%s\n' "$(sha256sum "$TOOLCHAIN_LOCKS" | awk '{ print $1 }')"
    printf 'build_gki_sha256=%s\n' "$(sha256sum "$KMOD/build-gki.sh" | awk '{ print $1 }')"
    printf 'verify_module_crc_sha256=%s\n' "$(sha256sum "$KMOD/verify-module-crc.sh" | awk '{ print $1 }')"
    printf 'feature_marker=%s\n' "$FEATURE_MARKER"
    printf 'ko_sha256=%s\n' "$(sha256sum "$ko" | awk '{ print $1 }')"
  } >"$tmp"
  mv -f -- "$tmp" "$outdir/provenance.env"
}

build_one() {
  local kmi="$1" kdir bdir outdir tmp module_flags name vermagic undefined
  local unstripped_this_module_size unstripped_this_module_alignment
  local unstripped_struct_module_size
  select_toolchain "$kmi" || return 1
  fetch_locked_source "$kmi" || return 1
  kdir="$WORKDIR/$kmi"
  bdir="$MODULE_BUILD/$kmi"
  outdir="$PREBUILT/$kmi"
  prepare_official_artifacts "$kmi" "$kdir" || return 1
  prepare_canonical_build "$kmi" "$kdir" "$bdir" || return 1

  tmp="$MODULE_SOURCE_ROOT/$kmi"
  safe_reset_dir "$tmp" "$MODULE_SOURCE_ROOT"
  cp -f "$KMOD/Makefile" "$KMOD/ethereal.c" "$KMOD/manager_cert.h" "$tmp/"
  module_flags="-Wno-error $BOOTSTRAP_CPPFLAGS \
-ffile-prefix-map=$ROOT=/workspace/Ethereal \
-fdebug-prefix-map=$ROOT=/workspace/Ethereal \
-fmacro-prefix-map=$ROOT=/workspace/Ethereal \
-ffile-prefix-map=$tmp=/workspace/Ethereal/kmod \
-fdebug-prefix-map=$tmp=/workspace/Ethereal/kmod \
-fmacro-prefix-map=$tmp=/workspace/Ethereal/kmod \
-ffile-prefix-map=$kdir=/workspace/GKI/$kmi \
-fdebug-prefix-map=$kdir=/workspace/GKI/$kmi \
-fmacro-prefix-map=$kdir=/workspace/GKI/$kmi \
-ffile-prefix-map=$bdir=/workspace/GKI/out/$kmi \
-fdebug-prefix-map=$bdir=/workspace/GKI/out/$kmi \
-fmacro-prefix-map=$bdir=/workspace/GKI/out/$kmi"
  if [[ "$kmi" == android16-6.12 ]]; then
    module_flags+=" -gz=none"
  fi
  log "$kmi build release ethereal.ko"
  if ! make -C "$kdir" O="$bdir" M="$tmp" HOSTCC=gcc HOSTCXX=g++ \
      LLVM=1 LLVM_IAS=1 ARCH=arm64 KERNELRELEASE="$OFFICIAL_RELEASE" \
      "${KBUILD_TOOL_ARGS[@]}" KCFLAGS="$module_flags" modules -j"$JOBS"; then
    safe_remove_dir "$tmp" "$MODULE_SOURCE_ROOT"
    return 1
  fi
  [[ -s "$tmp/ethereal.ko" ]] || {
    safe_remove_dir "$tmp" "$MODULE_SOURCE_ROOT"
    echo "$kmi module build produced no ethereal.ko" >&2
    return 1
  }
  if ! inspect_this_module "$tmp/ethereal.ko" 1; then
    safe_remove_dir "$tmp" "$MODULE_SOURCE_ROOT"
    return 1
  fi
  unstripped_this_module_size="$THIS_MODULE_SIZE"
  unstripped_this_module_alignment="$THIS_MODULE_ALIGNMENT"
  unstripped_struct_module_size="$THIS_MODULE_DWARF_SIZE"
  mkdir -p "$outdir"
  cp -f "$tmp/ethereal.ko" "$outdir/ethereal.ko"
  safe_remove_dir "$tmp" "$MODULE_SOURCE_ROOT"
  "$STRIP_BIN" --strip-debug "$outdir/ethereal.ko" || return 1

  name="$(modinfo -F name "$outdir/ethereal.ko" 2>/dev/null || true)"
  vermagic="$(modinfo -F vermagic "$outdir/ethereal.ko" 2>/dev/null || true)"
  inspect_this_module "$outdir/ethereal.ko" 0 || return 1
  [[ "$THIS_MODULE_SIZE" == "$unstripped_this_module_size" &&
     "$THIS_MODULE_ALIGNMENT" == "$unstripped_this_module_alignment" ]] || {
    echo "$kmi stripping changed the __this_module layout" >&2
    return 1
  }
  THIS_MODULE_DWARF_SIZE="$unstripped_struct_module_size"
  inspect_modversions "$outdir/ethereal.ko" "$bdir/.config" || return 1
  [[ "$name" == ethereal ]] || { echo "$kmi module name is '$name'" >&2; return 1; }
  [[ "$vermagic" == "$OFFICIAL_RELEASE "* && "$vermagic" == *modversions* ]] || {
    echo "$kmi incompatible vermagic: $vermagic" >&2
    return 1
  }
  undefined="$($TOOLCHAIN_BIN/llvm-nm -u --format=posix "$outdir/ethereal.ko" | awk '{ print $1 }')"
  if [[ "$BOOTSTRAP_KIND" == sprint-symbol-scan ]]; then
    grep -qxF sprint_symbol <<<"$undefined" &&
      ! grep -qxF register_kprobe <<<"$undefined" &&
      ! grep -qxF unregister_kprobe <<<"$undefined" || {
        echo "$kmi sprint bootstrap has unsafe static kprobe dependencies" >&2
        return 1
      }
  else
    grep -qxF register_kprobe <<<"$undefined" &&
      grep -qxF unregister_kprobe <<<"$undefined" || {
        echo "$kmi kprobe bootstrap dependencies are incomplete" >&2
        return 1
      }
  fi
  grep -aFq "$FEATURE_MARKER" "$outdir/ethereal.ko" || {
    echo "$kmi module lacks feature marker $FEATURE_MARKER" >&2
    return 1
  }
  if grep -aEiq "$HOST_PATH_RE" "$outdir/ethereal.ko"; then
    echo "$kmi module leaks an absolute host build path" >&2
    return 1
  fi
  if grep -aEiq '(r''patch|a''patch|super''key|s''key|k''pm|kp''module|/?(r''p|a''p)/su)' \
      "$outdir/ethereal.ko"; then
    echo "$kmi module contains a legacy brand identifier" >&2
    return 1
  fi
  cp -f "$OFFICIAL_CONFIG" "$outdir/canonical.config"
  cp -f "$bdir/.config" "$outdir/prepared.config"
  cp -f "$kdir/arch/arm64/configs/gki_defconfig" "$outdir/gki_defconfig"
  cp -f "$bdir/Module.symvers" "$outdir/Module.symvers"
  cp -f "$OFFICIAL_ABI_SYMVERS" "$outdir/official-abi.symvers"
  if grep -Eiq "$HOST_PATH_RE" "$outdir/Module.symvers"; then
    echo "$kmi canonical Module.symvers leaks an absolute host build path" >&2
    return 1
  fi
  bash "$KMOD/verify-module-crc.sh" --write-projection "$outdir/ethereal.ko" \
    "$outdir/Module.symvers" "$outdir/canonical.projected.symvers" || return 1
  bash "$KMOD/verify-module-crc.sh" --write-projection "$outdir/ethereal.ko" \
    "$outdir/official-abi.symvers" "$outdir/abi.projected.symvers" || return 1
  bash "$KMOD/verify-module-crc.sh" "$outdir/ethereal.ko" \
    "$outdir/Module.symvers" || return 1
  bash "$KMOD/verify-module-crc.sh" "$outdir/ethereal.ko" \
    "$outdir/official-abi.symvers" || return 1
  write_provenance "$kmi" "$outdir" "$outdir/ethereal.ko" || return 1
  log "$kmi release sha256=$(sha256sum "$outdir/ethereal.ko" | awk '{ print $1 }')"
}

if (( FETCH_ONLY )); then
  for kmi in "${KMIS[@]}"; do
    select_toolchain "$kmi"
    fetch_locked_source "$kmi"
    prepare_official_artifacts "$kmi" "$WORKDIR/$kmi"
  done
  exit 0
fi

passed=()
for kmi in "${KMIS[@]}"; do
  build_one "$kmi"
  passed+=("$kmi")
done
log "release build PASS: ${passed[*]:-none}"
