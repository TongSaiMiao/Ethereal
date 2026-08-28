#!/usr/bin/env python3
"""Convert Android GKI abi.xml/abi.stg exports into Module.symvers rows."""

import argparse
import json
import os
import re
import tempfile
import xml.etree.ElementTree as ET


SYMBOL_RE = re.compile(r"^[A-Za-z0-9_.$]+$")
STG_FIELD_RE = re.compile(r'^  ([a-z_]+): (.+)$')


class AbiError(Exception):
    pass


def add_symbol(symbols, name, crc, source):
    if not SYMBOL_RE.fullmatch(name):
        raise AbiError(f"{source}: invalid symbol name {name!r}")
    try:
        value = int(crc, 0)
    except ValueError as exc:
        raise AbiError(f"{source}: invalid CRC {crc!r} for {name}") from exc
    if not 0 <= value <= 0xFFFFFFFF:
        raise AbiError(f"{source}: CRC outside u32 for {name}: {crc}")
    previous = symbols.get(name)
    if previous is not None and previous != value:
        raise AbiError(
            f"{source}: conflicting CRCs for {name}: "
            f"0x{previous:08x} and 0x{value:08x}"
        )
    symbols[name] = value


def parse_xml(path):
    symbols = {}
    try:
        iterator = ET.iterparse(path, events=("end",))
        for _, element in iterator:
            if element.tag.rsplit("}", 1)[-1] == "elf-symbol":
                attrs = element.attrib
                if attrs.get("is-defined") == "yes" and attrs.get("crc"):
                    name = attrs.get("name")
                    if not name:
                        raise AbiError(f"{path}: defined elf-symbol has no name")
                    add_symbol(symbols, name, attrs["crc"], path)
            element.clear()
    except ET.ParseError as exc:
        raise AbiError(f"{path}: malformed XML: {exc}") from exc
    return symbols


def parse_stg(path):
    symbols = {}
    block = None
    with open(path, "r", encoding="utf-8", newline="") as stream:
        for line_number, raw in enumerate(stream, 1):
            line = raw.rstrip("\r\n")
            if block is None:
                if line == "elf_symbol {":
                    block = {}
                continue
            if line == "}":
                if block.get("is_defined") == "true" and "crc" in block:
                    if "name" not in block:
                        raise AbiError(
                            f"{path}:{line_number}: defined elf_symbol has no name"
                        )
                    add_symbol(symbols, block["name"], block["crc"], path)
                block = None
                continue
            match = STG_FIELD_RE.fullmatch(line)
            if not match:
                raise AbiError(f"{path}:{line_number}: malformed elf_symbol field")
            key, value = match.groups()
            if key in block:
                raise AbiError(f"{path}:{line_number}: duplicate field {key}")
            if key in ("name", "full_name", "namespace"):
                try:
                    value = json.loads(value)
                except json.JSONDecodeError as exc:
                    raise AbiError(
                        f"{path}:{line_number}: invalid quoted {key}"
                    ) from exc
            block[key] = value
    if block is not None:
        raise AbiError(f"{path}: unterminated elf_symbol block")
    return symbols


def parse_abi(path):
    with open(path, "rb") as stream:
        prefix = stream.read(64).lstrip()
    if prefix.startswith(b"<"):
        symbols = parse_xml(path)
    elif prefix.startswith(b"version:"):
        symbols = parse_stg(path)
    else:
        raise AbiError(f"{path}: unsupported ABI artifact magic {prefix[:16]!r}")
    if not symbols:
        raise AbiError(f"{path}: no defined symbols with CRCs")
    return symbols


def write_symvers(path, symbols):
    destination = os.path.abspath(path)
    directory = os.path.dirname(destination)
    os.makedirs(directory, exist_ok=True)
    fd, temporary = tempfile.mkstemp(
        prefix=f".{os.path.basename(path)}.", suffix=".tmp", dir=directory
    )
    try:
        with os.fdopen(fd, "w", encoding="ascii", newline="\n") as stream:
            for name in sorted(symbols):
                stream.write(
                    f"0x{symbols[name]:08x}\t{name}\tvmlinux\tEXPORT_SYMBOL\t\n"
                )
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, destination)
        temporary = None
    finally:
        if temporary is not None:
            try:
                os.unlink(temporary)
            except FileNotFoundError:
                pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("abi")
    parser.add_argument("output")
    args = parser.parse_args()
    try:
        symbols = parse_abi(args.abi)
        write_symvers(args.output, symbols)
    except (AbiError, OSError) as exc:
        parser.error(str(exc))
    print(f"wrote {len(symbols)} official ABI CRC rows to {args.output}")


if __name__ == "__main__":
    main()
