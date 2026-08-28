#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  verify-module-crc.sh MODULE.ko CANONICAL-Module.symvers [OVERLAY-Module.symvers]
  verify-module-crc.sh --write-projection MODULE.ko FULL-Module.symvers OUTPUT
  verify-module-crc.sh --self-test

Extract every versioned dependency from MODULE.ko and compare its CRC with the
canonical Module.symvers. If an overlay is supplied, its projection onto the
module dependency set must be complete and identical to the canonical one.
With --write-projection, atomically write the matching canonical rows for only
the module's actual dependencies after every dependency CRC has been verified.
EOF
}

self_test() {
  local script_dir script_path tmp cc
  script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  script_path="$script_dir/$(basename -- "${BASH_SOURCE[0]}")"
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/ethereal-crc-selftest.XXXXXX")"
  cc="${CC:-cc}"

  cleanup_self_test() {
    if [[ -n "${tmp:-}" && -d "$tmp" && "$tmp" == */ethereal-crc-selftest.* ]]; then
      rm -rf -- "$tmp"
    fi
  }
  trap cleanup_self_test EXIT

  command -v python3 >/dev/null 2>&1 || {
    echo "SELFTEST FAIL: python3 is required" >&2
    return 1
  }
  command -v "$cc" >/dev/null 2>&1 || {
    echo "SELFTEST FAIL: C compiler '$cc' is required" >&2
    return 1
  }

  cat >"$tmp/basic.c" <<'EOF'
struct modversion_info {
    unsigned long crc;
    char name[64 - sizeof(unsigned long)];
};

_Static_assert(sizeof(struct modversion_info) == 64, "bad fixture ABI");

__attribute__((used, section("__versions")))
static const struct modversion_info versions[] = {
    { 0x11223344UL, "alpha" },
    { 0xaabbccddUL, "beta" },
};
EOF

  cat >"$tmp/extended.c" <<'EOF'
typedef unsigned int u32;

__attribute__((used, section("__version_ext_crcs")))
static const u32 crcs[] = {
    0x11223344U,
    0x55667788U,
};

__attribute__((used, section("__version_ext_names")))
static const char names[] =
    "alpha\0"
    "symbol_name_longer_than_the_basic_modversion_record_name_field_123\0";
EOF

  "$cc" -c "$tmp/basic.c" -o "$tmp/basic.ko"
  "$cc" -c "$tmp/extended.c" -o "$tmp/extended.ko"

  cat >"$tmp/basic.symvers" <<'EOF'
0x11223344	alpha	vmlinux	EXPORT_SYMBOL
0xaabbccdd	beta	vmlinux	EXPORT_SYMBOL_GPL
0xdeadbeef	unrelated	vmlinux	EXPORT_SYMBOL
EOF
  cat >"$tmp/basic-overlay.symvers" <<'EOF'
0x01020304	unrelated	vendor	EXPORT_SYMBOL
0xaabbccdd	beta	vmlinux	EXPORT_SYMBOL_GPL
0x11223344	alpha	vmlinux	EXPORT_SYMBOL
EOF
  cat >"$tmp/basic-mismatch.symvers" <<'EOF'
0x11223344	alpha	vmlinux	EXPORT_SYMBOL
0xaabbccde	beta	vmlinux	EXPORT_SYMBOL_GPL
EOF
  cat >"$tmp/basic-missing.symvers" <<'EOF'
0x11223344	alpha	vmlinux	EXPORT_SYMBOL
EOF
  cat >"$tmp/basic-conflict.symvers" <<'EOF'
0x11223344	alpha	vmlinux	EXPORT_SYMBOL
0xaabbccdd	beta	vmlinux	EXPORT_SYMBOL_GPL
0xaabbccde	beta	vendor	EXPORT_SYMBOL_GPL
EOF
  cat >"$tmp/basic-overlay-mismatch.symvers" <<'EOF'
0x11223344	alpha	vmlinux	EXPORT_SYMBOL
0x01010101	beta	vendor	EXPORT_SYMBOL_GPL
EOF
  cat >"$tmp/extended.symvers" <<'EOF'
0x11223344	alpha	vmlinux	EXPORT_SYMBOL
0x55667788	symbol_name_longer_than_the_basic_modversion_record_name_field_123	vmlinux	EXPORT_SYMBOL
EOF

  expect_pass() {
    local name="$1"
    shift
    if bash "$script_path" "$@" >"$tmp/$name.log" 2>&1; then
      printf 'SELFTEST OK: %s\n' "$name"
    else
      sed 's/^/  /' "$tmp/$name.log" >&2
      printf 'SELFTEST FAIL: %s unexpectedly failed\n' "$name" >&2
      return 1
    fi
  }

  expect_fail() {
    local name="$1"
    shift
    if bash "$script_path" "$@" >"$tmp/$name.log" 2>&1; then
      sed 's/^/  /' "$tmp/$name.log" >&2
      printf 'SELFTEST FAIL: %s unexpectedly passed\n' "$name" >&2
      return 1
    fi
    printf 'SELFTEST OK: %s rejected\n' "$name"
  }

  expect_pass basic "$tmp/basic.ko" "$tmp/basic.symvers" "$tmp/basic-overlay.symvers"
  expect_pass extended "$tmp/extended.ko" "$tmp/extended.symvers"
  expect_pass write-projection --write-projection \
    "$tmp/basic.ko" "$tmp/basic.symvers" "$tmp/projected.symvers"
  cat >"$tmp/expected-projection.symvers" <<'EOF'
0x11223344	alpha	vmlinux	EXPORT_SYMBOL
0xaabbccdd	beta	vmlinux	EXPORT_SYMBOL_GPL
EOF
  if cmp -s "$tmp/expected-projection.symvers" "$tmp/projected.symvers"; then
    echo "SELFTEST OK: projection contains only canonical dependency rows"
  else
    diff -u "$tmp/expected-projection.symvers" "$tmp/projected.symvers" >&2 || true
    echo "SELFTEST FAIL: projection output is not the expected canonical subset" >&2
    return 1
  fi

  printf '%s\n' 'existing-output-must-survive' >"$tmp/protected.symvers"
  expect_fail projection-conflict-preserves-output --write-projection \
    "$tmp/basic.ko" "$tmp/basic-conflict.symvers" "$tmp/protected.symvers"
  if [[ "$(cat "$tmp/protected.symvers")" == "existing-output-must-survive" ]]; then
    echo "SELFTEST OK: failed projection did not replace existing output"
  else
    echo "SELFTEST FAIL: failed projection replaced existing output" >&2
    return 1
  fi

  expect_fail canonical-crc-mismatch "$tmp/basic.ko" "$tmp/basic-mismatch.symvers"
  expect_fail canonical-missing-symbol "$tmp/basic.ko" "$tmp/basic-missing.symvers"
  expect_fail canonical-conflicting-symbol "$tmp/basic.ko" "$tmp/basic-conflict.symvers"
  expect_fail overlay-projection-mismatch "$tmp/basic.ko" "$tmp/basic.symvers" "$tmp/basic-overlay-mismatch.symvers"
  echo "SELFTEST PASS: 8 command cases plus projection content checks"
}

if [[ "${1:-}" == "--self-test" ]]; then
  [[ $# -eq 1 ]] || {
    usage >&2
    exit 2
  }
  self_test
  exit
fi

mode="verify"
module_path=""
canonical_path=""
overlay_path=""
projection_path=""
if [[ "${1:-}" == "--write-projection" ]]; then
  [[ $# -eq 4 ]] || {
    usage >&2
    exit 2
  }
  mode="write-projection"
  module_path="$2"
  canonical_path="$3"
  projection_path="$4"
else
  [[ $# -ge 2 && $# -le 3 ]] || {
    usage >&2
    exit 2
  }
  module_path="$1"
  canonical_path="$2"
  overlay_path="${3:-}"
fi

command -v python3 >/dev/null 2>&1 || {
  echo "ERROR: python3 is required" >&2
  exit 2
}

python3 - "$mode" "$module_path" "$canonical_path" "$overlay_path" "$projection_path" <<'PY'
import os
import re
import struct
import sys
import tempfile


class InputError(Exception):
    pass


def read_file(path, label):
    try:
        with open(path, "rb") as stream:
            return stream.read()
    except OSError as exc:
        raise InputError("cannot read {} '{}': {}".format(label, path, exc))


def checked_slice(blob, offset, size, label):
    if offset < 0 or size < 0 or offset > len(blob) or size > len(blob) - offset:
        raise InputError(
            "{} is outside the file (offset=0x{:x}, size=0x{:x}, file=0x{:x})".format(
                label, offset, size, len(blob)
            )
        )
    return blob[offset:offset + size]


def decode_ascii(value, label):
    try:
        return value.decode("ascii")
    except UnicodeDecodeError:
        raise InputError("{} is not ASCII: {!r}".format(label, value))


def parse_elf_sections(path):
    blob = read_file(path, "module")
    if len(blob) < 16 or blob[:4] != b"\x7fELF":
        raise InputError("module '{}' is not an ELF file".format(path))

    elf_class = blob[4]
    elf_data = blob[5]
    if elf_class == 1:
        bits = 32
        header_format = "16sHHIIIIIHHHHHH"
        section_format = "IIIIIIIIII"
        word_format = "I"
        word_size = 4
    elif elf_class == 2:
        bits = 64
        header_format = "16sHHIQQQIHHHHHH"
        section_format = "IIQQQQIIQQ"
        word_format = "Q"
        word_size = 8
    else:
        raise InputError("module '{}' has unsupported ELF class {}".format(path, elf_class))

    if elf_data == 1:
        endian = "<"
        endian_name = "little-endian"
    elif elf_data == 2:
        endian = ">"
        endian_name = "big-endian"
    else:
        raise InputError("module '{}' has unsupported ELF data encoding {}".format(path, elf_data))

    header_struct = struct.Struct(endian + header_format)
    if len(blob) < header_struct.size:
        raise InputError("module '{}' has a truncated ELF header".format(path))
    header = header_struct.unpack_from(blob, 0)
    if header[3] != 1:
        raise InputError("module '{}' has unsupported ELF version {}".format(path, header[3]))
    if header[1] != 1:
        raise InputError(
            "module '{}' is ELF type {}, expected ET_REL".format(path, header[1])
        )

    section_offset = header[6]
    section_entry_size = header[11]
    section_count = header[12]
    string_section_index = header[13]
    section_struct = struct.Struct(endian + section_format)
    if section_offset == 0:
        raise InputError("module '{}' has no section table".format(path))
    if section_entry_size < section_struct.size:
        raise InputError(
            "module '{}' has invalid section entry size {}".format(path, section_entry_size)
        )

    def unpack_section(index):
        entry_offset = section_offset + index * section_entry_size
        checked_slice(blob, entry_offset, section_struct.size, "ELF section header {}".format(index))
        return section_struct.unpack_from(blob, entry_offset)

    section_zero = unpack_section(0)
    if section_count == 0:
        section_count = section_zero[5]
    if string_section_index == 0xffff:
        string_section_index = section_zero[6]
    if section_count <= 0:
        raise InputError("module '{}' has an empty section table".format(path))
    checked_slice(
        blob,
        section_offset,
        section_count * section_entry_size,
        "ELF section table",
    )
    if string_section_index <= 0 or string_section_index >= section_count:
        raise InputError(
            "module '{}' has invalid section-name table index {}".format(
                path, string_section_index
            )
        )

    raw_sections = [unpack_section(index) for index in range(section_count)]
    string_section = raw_sections[string_section_index]
    if string_section[1] != 3:
        raise InputError("module '{}' section-name table is not STRTAB".format(path))
    string_table = checked_slice(
        blob, string_section[4], string_section[5], "ELF section-name table"
    )

    sections = {}
    for index, raw in enumerate(raw_sections):
        name_offset = raw[0]
        if name_offset >= len(string_table):
            raise InputError(
                "module '{}' section {} has an invalid name offset".format(path, index)
            )
        name_end = string_table.find(b"\0", name_offset)
        if name_end < 0:
            raise InputError("module '{}' has an unterminated section name".format(path))
        name = decode_ascii(string_table[name_offset:name_end], "ELF section name")
        sections.setdefault(name, []).append(raw)

    def section_bytes(name):
        matches = sections.get(name, [])
        if not matches:
            return None
        if len(matches) != 1:
            raise InputError("module '{}' contains duplicate {} sections".format(path, name))
        raw = matches[0]
        if raw[1] != 1:
            raise InputError("module '{}' section {} is not PROGBITS".format(path, name))
        if raw[2] & 0x800:
            raise InputError("module '{}' section {} is compressed".format(path, name))
        return checked_slice(blob, raw[4], raw[5], "ELF section {}".format(name))

    return {
        "bits": bits,
        "endian": endian,
        "endian_name": endian_name,
        "word_format": word_format,
        "word_size": word_size,
        "basic": section_bytes("__versions"),
        "ext_crcs": section_bytes("__version_ext_crcs"),
        "ext_names": section_bytes("__version_ext_names"),
    }


def add_unique(entries, symbol, crc, source):
    if not symbol:
        raise InputError("{} contains an empty symbol name".format(source))
    if symbol in entries:
        if entries[symbol] != crc:
            raise InputError(
                "{} contains conflicting CRCs for {}: 0x{:08x} and 0x{:08x}".format(
                    source, symbol, entries[symbol], crc
                )
            )
        raise InputError("{} contains duplicate symbol {}".format(source, symbol))
    entries[symbol] = crc


def parse_basic(info, path):
    data = info["basic"]
    entries = {}
    if data is None:
        return entries
    if len(data) % 64 != 0:
        raise InputError(
            "module '{}' __versions size {} is not a multiple of 64".format(path, len(data))
        )

    crc_struct = struct.Struct(info["endian"] + info["word_format"])
    name_offset = info["word_size"]
    for index in range(len(data) // 64):
        record = data[index * 64:(index + 1) * 64]
        raw_crc = crc_struct.unpack_from(record, 0)[0]
        if raw_crc > 0xffffffff:
            raise InputError(
                "module '{}' __versions record {} has a non-32-bit CRC 0x{:x}".format(
                    path, index, raw_crc
                )
            )
        name_field = record[name_offset:]
        name_end = name_field.find(b"\0")
        if name_end < 0:
            raise InputError(
                "module '{}' __versions record {} has no terminated name".format(path, index)
            )
        if any(name_field[name_end + 1:]):
            raise InputError(
                "module '{}' __versions record {} has non-zero name padding".format(path, index)
            )
        symbol = decode_ascii(
            name_field[:name_end], "module __versions record {} symbol".format(index)
        )
        add_unique(entries, symbol, raw_crc, "module '{}' __versions".format(path))
    return entries


def parse_extended(info, path):
    crc_data = info["ext_crcs"]
    name_data = info["ext_names"]
    entries = {}
    if crc_data is None and name_data is None:
        return entries
    if crc_data is None or name_data is None:
        raise InputError(
            "module '{}' must contain both extended modversion sections".format(path)
        )
    if len(crc_data) % 4 != 0:
        raise InputError(
            "module '{}' __version_ext_crcs size {} is not a multiple of 4".format(
                path, len(crc_data)
            )
        )

    crc_struct = struct.Struct(info["endian"] + "I")
    crc_count = len(crc_data) // 4
    position = 0
    for index in range(crc_count):
        name_end = name_data.find(b"\0", position)
        if name_end < 0:
            raise InputError(
                "module '{}' has fewer extended names than CRCs".format(path)
            )
        symbol = decode_ascii(
            name_data[position:name_end],
            "module extended record {} symbol".format(index),
        )
        crc = crc_struct.unpack_from(crc_data, index * 4)[0]
        add_unique(entries, symbol, crc, "module '{}' extended versions".format(path))
        position = name_end + 1
    if any(name_data[position:]):
        raise InputError(
            "module '{}' has more extended names than CRCs or non-zero name padding".format(path)
        )
    return entries


def parse_module(path):
    info = parse_elf_sections(path)
    basic = parse_basic(info, path)
    extended = parse_extended(info, path)
    merged = dict(basic)
    for symbol, crc in extended.items():
        if symbol in merged and merged[symbol] != crc:
            raise InputError(
                "module '{}' basic/extended CRC conflict for {}: 0x{:08x} vs 0x{:08x}".format(
                    path, symbol, merged[symbol], crc
                )
            )
        merged[symbol] = crc
    if not merged:
        raise InputError("module '{}' has no versioned symbol dependencies".format(path))
    return info, basic, extended, merged


def parse_symvers(path, label):
    blob = read_file(path, label)
    entries = {}
    rows = {}
    duplicate_rows = 0
    for line_number, raw_line in enumerate(blob.splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith(b"#"):
            continue
        fields = line.split()
        if len(fields) < 2:
            raise InputError(
                "{} '{}' line {} has fewer than two fields".format(label, path, line_number)
            )
        crc_match = re.fullmatch(br"(?:0[xX])?([0-9a-fA-F]{1,8})", fields[0])
        if not crc_match:
            raise InputError(
                "{} '{}' line {} has invalid CRC {!r}".format(
                    label, path, line_number, fields[0]
                )
            )
        crc = int(crc_match.group(1), 16)
        symbol = decode_ascii(
            fields[1], "{} '{}' line {} symbol".format(label, path, line_number)
        )
        if not symbol:
            raise InputError("{} '{}' line {} has an empty symbol".format(label, path, line_number))
        if symbol in entries:
            if entries[symbol] != crc:
                raise InputError(
                    "{} '{}' has conflicting rows for {}: 0x{:08x} and 0x{:08x}".format(
                        label, path, symbol, entries[symbol], crc
                    )
                )
            duplicate_rows += 1
        else:
            entries[symbol] = crc
            rows[symbol] = line
    if not entries:
        raise InputError("{} '{}' contains no symbol CRCs".format(label, path))
    return entries, rows, duplicate_rows


def crc_text(value):
    return "-" if value is None else "0x{:08x}".format(value)


def same_path(left, right):
    if os.path.normcase(os.path.realpath(left)) == os.path.normcase(os.path.realpath(right)):
        return True
    try:
        return os.path.samefile(left, right)
    except (FileNotFoundError, OSError):
        return False


def write_projection(output_path, module_path, canonical_path, dependencies, canonical_rows):
    if same_path(output_path, module_path) or same_path(output_path, canonical_path):
        raise InputError("projection output must differ from the module and full symvers inputs")

    missing_rows = sorted(set(dependencies) - set(canonical_rows))
    if missing_rows:
        raise InputError(
            "canonical symvers rows disappeared for: {}".format(", ".join(missing_rows))
        )
    projection = b"".join(canonical_rows[symbol] + b"\n" for symbol in sorted(dependencies))
    output_path = os.path.abspath(output_path)
    output_dir = os.path.dirname(output_path)
    output_name = os.path.basename(output_path)
    temporary_path = None
    try:
        fd, temporary_path = tempfile.mkstemp(
            prefix=".{}.".format(output_name), suffix=".tmp", dir=output_dir
        )
        try:
            os.fchmod(fd, 0o644)
            with os.fdopen(fd, "wb") as stream:
                fd = -1
                stream.write(projection)
                stream.flush()
                os.fsync(stream.fileno())
        finally:
            if fd >= 0:
                os.close(fd)
        os.replace(temporary_path, output_path)
        temporary_path = None
    except OSError as exc:
        raise InputError(
            "cannot atomically write projection '{}': {}".format(output_path, exc)
        )
    finally:
        if temporary_path is not None:
            try:
                os.unlink(temporary_path)
            except FileNotFoundError:
                pass
    return len(projection)


def verify(module_path, canonical_path, overlay_path, projection_path):
    info, basic, extended, dependencies = parse_module(module_path)
    canonical, canonical_rows, canonical_duplicates = parse_symvers(
        canonical_path, "canonical symvers"
    )
    overlay = None
    overlay_duplicates = 0
    if overlay_path:
        overlay, _, overlay_duplicates = parse_symvers(overlay_path, "overlay symvers")

    print("module: {}".format(os.path.abspath(module_path)))
    print(
        "elf: ELF{} {}; basic-records={}; extended-records={}; dependencies={}".format(
            info["bits"], info["endian_name"], len(basic), len(extended), len(dependencies)
        )
    )
    print(
        "canonical: {} entries={} duplicate-identical-rows={}".format(
            os.path.abspath(canonical_path), len(canonical), canonical_duplicates
        )
    )
    if overlay is not None:
        print(
            "overlay: {} entries={} duplicate-identical-rows={}".format(
                os.path.abspath(overlay_path), len(overlay), overlay_duplicates
            )
        )

    if overlay is None:
        print("STATUS\tKO_CRC\tCANONICAL_CRC\tSYMBOL")
    else:
        print("STATUS\tKO_CRC\tCANONICAL_CRC\tOVERLAY_CRC\tSYMBOL")

    failed = False
    for symbol in sorted(dependencies):
        ko_crc = dependencies[symbol]
        canonical_crc = canonical.get(symbol)
        statuses = []
        if canonical_crc is None:
            statuses.append("MISSING_CANONICAL")
            failed = True
        elif canonical_crc != ko_crc:
            statuses.append("CANONICAL_CRC_MISMATCH")
            failed = True

        overlay_crc = None
        if overlay is not None:
            overlay_crc = overlay.get(symbol)
            if overlay_crc is None:
                statuses.append("MISSING_OVERLAY")
                failed = True
            elif overlay_crc != ko_crc:
                statuses.append("OVERLAY_CRC_MISMATCH")
                failed = True
            if canonical_crc is None or overlay_crc is None or canonical_crc != overlay_crc:
                statuses.append("PROJECTION_MISMATCH")
                failed = True

        status = "+".join(statuses) if statuses else "OK"
        if overlay is None:
            print(
                "{}\t{}\t{}\t{}".format(
                    status, crc_text(ko_crc), crc_text(canonical_crc), symbol
                )
            )
        else:
            print(
                "{}\t{}\t{}\t{}\t{}".format(
                    status,
                    crc_text(ko_crc),
                    crc_text(canonical_crc),
                    crc_text(overlay_crc),
                    symbol,
                )
            )

    if failed:
        print(
            "FAIL: module dependency CRC verification failed (dependencies={})".format(
                len(dependencies)
            ),
            file=sys.stderr,
        )
        return 1

    if projection_path:
        projection_size = write_projection(
            projection_path,
            module_path,
            canonical_path,
            dependencies,
            canonical_rows,
        )
        print(
            "projection: {} entries={} bytes={}".format(
                os.path.abspath(projection_path), len(dependencies), projection_size
            )
        )

    if overlay is None:
        print(
            "PASS: {} module dependency CRCs match canonical symvers".format(
                len(dependencies)
            )
        )
    else:
        print(
            "PASS: {} module dependency CRCs match; overlay projection is identical".format(
                len(dependencies)
            )
        )
    return 0


try:
    mode = sys.argv[1]
    if mode not in ("verify", "write-projection"):
        raise InputError("unsupported mode '{}'".format(mode))
    overlay_path = sys.argv[4] if mode == "verify" else ""
    projection_path = sys.argv[5] if mode == "write-projection" else ""
    sys.exit(verify(sys.argv[2], sys.argv[3], overlay_path, projection_path))
except InputError as exc:
    print("ERROR: {}".format(exc), file=sys.stderr)
    sys.exit(2)
PY
