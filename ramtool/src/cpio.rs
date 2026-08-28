use anyhow::{bail, ensure, Result};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;

const MAGIC_NEWC: &[u8] = b"070701";
const MAGIC_CRC: &[u8] = b"070702";
const TRAILER: &str = "TRAILER!!!";

const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;
const S_IFLNK: u32 = 0o120000;
const S_IFMT: u32 = 0o170000;
/// Linux initramfs counts the trailing NUL in its 4096-byte PATH_MAX check.
const MAX_ENTRY_NAME: usize = 4095;
const MAX_COMPONENT_NAME: usize = 255;
const MAX_SYMLINK_TARGET: usize = 4096;
const MAX_ENTRIES: usize = 65_536;
const MAX_ARCHIVES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub ino: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u32,
    pub mtime: u32,
    pub rdevmajor: u32,
    pub rdevminor: u32,
    pub data: Vec<u8>,
}

impl Entry {
    pub fn is_dir(&self) -> bool {
        self.mode & S_IFMT == S_IFDIR
    }

    pub fn is_regular(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    pub fn is_symlink(&self) -> bool {
        self.mode & S_IFMT == S_IFLNK
    }
}

fn align4(n: usize) -> usize {
    (n + 3) & !3
}

fn hex8(buf: &[u8]) -> Result<u32> {
    ensure!(buf.len() >= 8, "truncated cpio header field");
    let s = std::str::from_utf8(&buf[..8])?;
    Ok(u32::from_str_radix(s, 16)?)
}

fn push_hex8(out: &mut Vec<u8>, v: u32) {
    let _ = write!(out, "{v:08x}");
}

fn pad4(out: &mut Vec<u8>) {
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn requires_directory(name: &str) -> bool {
    name.ends_with('/') || name.rsplit('/').next() == Some(".")
}

fn normalize_name(name: &str) -> Result<Option<String>> {
    ensure!(!name.is_empty(), "cpio: empty entry name");
    ensure!(
        name.len() <= MAX_ENTRY_NAME,
        "cpio: entry name exceeds {MAX_ENTRY_NAME} bytes"
    );
    ensure!(
        !name.as_bytes().contains(&0),
        "cpio: entry name contains NUL"
    );
    ensure!(
        !name.split('/').any(|part| part == ".."),
        "cpio: parent traversal is not allowed in entry {name:?}"
    );
    ensure!(
        name.split('/')
            .filter(|part| !part.is_empty() && *part != ".")
            .all(|part| part.len() <= MAX_COMPONENT_NAME),
        "cpio: path component exceeds {MAX_COMPONENT_NAME} bytes in entry {name:?}"
    );
    let normalized = name
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/");
    if normalized.is_empty() {
        return Ok(None);
    }
    ensure!(
        !requires_directory(name),
        "cpio: trailing directory syntax is not allowed in entry {name:?}"
    );
    ensure!(
        normalized != TRAILER,
        "cpio: reserved trailer name is not allowed in entry {name:?}"
    );
    Ok(Some(normalized))
}

fn required_name(name: &str) -> Result<String> {
    normalize_name(name)?.ok_or_else(|| anyhow::anyhow!("cpio: root entry is not a file name"))
}

pub fn parse(data: &[u8]) -> Result<Vec<Entry>> {
    let entries = parse_from(data, 0)?.0;
    validate_symlink_ancestors(std::slice::from_ref(&entries))?;
    Ok(entries)
}

fn validate_symlink_ancestors(archives: &[Vec<Entry>]) -> Result<()> {
    let mut symlinks = HashSet::new();
    for entry in archives.iter().flat_map(|entries| entries.iter()) {
        let mut prefix = String::new();
        let parts: Vec<&str> = entry.name.split('/').collect();
        for part in parts.iter().take(parts.len().saturating_sub(1)) {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            ensure!(
                !symlinks.contains(&prefix),
                "cpio: entry {:?} descends through symlink ancestor {:?}",
                entry.name,
                prefix
            );
        }
        if entry.is_symlink() {
            symlinks.insert(entry.name.clone());
        } else {
            symlinks.remove(&entry.name);
        }
    }
    Ok(())
}

/// One newc archive starting at `start`. Returns entries and offset after TRAILER.
fn parse_from(data: &[u8], start: usize) -> Result<(Vec<Entry>, usize)> {
    let mut off = start;
    let mut entries = Vec::new();
    while off + 110 <= data.len() {
        let magic = &data[off..off + 6];
        if magic != MAGIC_NEWC && magic != MAGIC_CRC {
            bail!("cpio: bad magic at {off}");
        }
        let ino = hex8(&data[off + 6..])?;
        let mode = hex8(&data[off + 14..])?;
        let uid = hex8(&data[off + 22..])?;
        let gid = hex8(&data[off + 30..])?;
        let nlink = hex8(&data[off + 38..])?;
        let mtime = hex8(&data[off + 46..])?;
        let filesize = hex8(&data[off + 54..])? as usize;
        let rdevmajor = hex8(&data[off + 62..])?;
        let rdevminor = hex8(&data[off + 70..])?;
        let namesize = hex8(&data[off + 94..])? as usize;
        ensure!(namesize > 0, "cpio: zero-length name");
        ensure!(namesize <= MAX_ENTRY_NAME + 1, "cpio: name is too long");
        let name_off = off + 110;
        let name_end = name_off
            .checked_add(namesize)
            .ok_or_else(|| anyhow::anyhow!("cpio: name range overflow"))?;
        ensure!(name_end <= data.len(), "cpio: truncated name");
        let name_bytes = &data[name_off..name_end];
        ensure!(
            name_bytes.last() == Some(&0),
            "cpio: name is not NUL terminated"
        );
        ensure!(
            !name_bytes[..name_bytes.len() - 1].contains(&0),
            "cpio: name contains an embedded NUL"
        );
        let source_name = std::str::from_utf8(&name_bytes[..name_bytes.len() - 1])?.to_string();
        let data_off = align4(name_end);
        ensure!(data_off <= data.len(), "cpio: truncated name padding");
        let file_end = data_off
            .checked_add(filesize)
            .ok_or_else(|| anyhow::anyhow!("cpio: file range overflow"))?;
        ensure!(file_end <= data.len(), "cpio: truncated file {source_name}");
        let file_data = data[data_off..file_end].to_vec();
        let file_type = mode & S_IFMT;
        ensure!(
            file_type == S_IFREG
                || (file_type == S_IFLNK && filesize <= MAX_SYMLINK_TARGET)
                || filesize == 0,
            "cpio: non-regular entry has an unsupported body: {source_name:?}"
        );
        off = align4(file_end);
        ensure!(
            off <= data.len(),
            "cpio: truncated file padding for {source_name}"
        );
        if source_name == TRAILER {
            return Ok((entries, off));
        }
        let Some(name) = normalize_name(&source_name)? else {
            continue;
        };
        ensure!(entries.len() < MAX_ENTRIES, "cpio: too many entries");
        entries.push(Entry {
            name,
            ino,
            mode,
            uid,
            gid,
            nlink,
            mtime,
            rdevmajor,
            rdevminor,
            data: file_data,
        });
    }
    bail!("cpio: missing {TRAILER}")
}

/// Concatenated initramfs: vendor + generic (or vendor_boot fragments).
pub fn parse_all(data: &[u8]) -> Result<Vec<Vec<Entry>>> {
    let mut archives = Vec::new();
    let mut off = 0usize;
    let mut total_entries = 0usize;
    while off + 110 <= data.len() {
        while off < data.len() && data[off] == 0 {
            off += 1;
        }
        if off + 6 > data.len() {
            break;
        }
        let magic = &data[off..off + 6];
        if magic != MAGIC_NEWC && magic != MAGIC_CRC {
            bail!("cpio: unexpected non-padding data at {off}");
        }
        let (ents, next) = parse_from(data, off)?;
        ensure!(archives.len() < MAX_ARCHIVES, "cpio: too many archives");
        total_entries = total_entries
            .checked_add(ents.len())
            .ok_or_else(|| anyhow::anyhow!("cpio: entry count overflow"))?;
        ensure!(total_entries <= MAX_ENTRIES, "cpio: too many total entries");
        archives.push(ents);
        if next <= off {
            break;
        }
        off = next;
    }
    if !archives.is_empty() {
        ensure!(
            data[off..].iter().all(|byte| *byte == 0),
            "cpio: unexpected non-padding data at {off}"
        );
    }
    if archives.is_empty() {
        archives.push(parse(data)?);
    }
    validate_symlink_ancestors(&archives)?;
    Ok(archives)
}

pub fn serialize_all(archives: &[Vec<Entry>]) -> Vec<u8> {
    let mut out = Vec::new();
    for a in archives {
        out.extend(serialize(a));
    }
    out
}

pub(crate) fn serialize_all_checked(archives: &[Vec<Entry>]) -> Result<Vec<u8>> {
    validate_symlink_ancestors(archives)?;
    Ok(serialize_all(archives))
}

pub fn serialize(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut ino = 300000u32;
    for e in entries {
        write_entry(&mut out, e, if e.ino == 0 { ino } else { e.ino });
        ino += 1;
    }
    let trailer = Entry {
        name: TRAILER.to_string(),
        ino: 0,
        mode: 0,
        uid: 0,
        gid: 0,
        nlink: 1,
        mtime: 0,
        rdevmajor: 0,
        rdevminor: 0,
        data: Vec::new(),
    };
    write_entry(&mut out, &trailer, 0);
    out
}

fn write_entry(out: &mut Vec<u8>, e: &Entry, ino: u32) {
    let name = format!("{}\0", e.name);
    out.extend_from_slice(MAGIC_NEWC);
    push_hex8(out, ino);
    push_hex8(out, e.mode);
    push_hex8(out, e.uid);
    push_hex8(out, e.gid);
    push_hex8(out, e.nlink.max(1));
    push_hex8(out, e.mtime);
    push_hex8(out, e.data.len() as u32);
    push_hex8(out, 0); // devmajor
    push_hex8(out, 0);
    push_hex8(out, e.rdevmajor);
    push_hex8(out, e.rdevminor);
    push_hex8(out, name.len() as u32);
    push_hex8(out, 0); // checksum
    out.extend_from_slice(name.as_bytes());
    pad4(out);
    out.extend_from_slice(&e.data);
    pad4(out);
}

fn find_mut<'a>(entries: &'a mut [Entry], name: &str) -> Option<&'a mut Entry> {
    entries.iter_mut().rev().find(|entry| entry.name == name)
}

fn find_any<'a>(archives: &'a [Vec<Entry>], name: &str) -> Option<&'a Entry> {
    archives
        .iter()
        .rev()
        .find_map(|entries| entries.iter().rev().find(|entry| entry.name == name))
}

pub fn exists(data: &[u8], name: &str) -> Result<bool> {
    let name = required_name(name)?;
    Ok(find_any(&parse_all(data)?, &name).is_some())
}

pub fn extract_to(data: &[u8], name: &str, out: &Path) -> Result<()> {
    let name = required_name(name)?;
    let archives = parse_all(data)?;
    let e = find_any(&archives, &name).ok_or_else(|| anyhow::anyhow!("cpio: {name} not found"))?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(out, &e.data)?;
    Ok(())
}

pub fn add_file(data: &[u8], mode: u32, name: &str, file: &Path) -> Result<Vec<u8>> {
    let mut archives = parse_all(data)?;
    if archives.is_empty() {
        archives.push(Vec::new());
    }
    let n = required_name(name)?;
    let blob = fs::read(file)?;
    let file_mode = S_IFREG | (mode & 0o7777);
    // Last archive wins when the bootloader concatenates ramdisks.
    // Drop every older alias first so a duplicate entry later in the archive
    // cannot replace a freshly embedded loader, module, or manager token.
    for existing in &mut archives {
        existing.retain(|entry| entry.name != n);
    }
    let entries = archives.last_mut().unwrap();
    entries.push(Entry {
        name: n,
        ino: 0,
        mode: file_mode,
        uid: 0,
        gid: 0,
        nlink: 1,
        mtime: 0,
        rdevmajor: 0,
        rdevminor: 0,
        data: blob,
    });
    serialize_all_checked(&archives)
}

pub fn mkdir(data: &[u8], mode: u32, name: &str) -> Result<Vec<u8>> {
    let mut archives = parse_all(data)?;
    if archives.is_empty() {
        archives.push(Vec::new());
    }
    let n = required_name(name)?;
    for existing in &mut archives {
        existing.retain(|entry| entry.name != n);
    }
    let entries = archives.last_mut().unwrap();
    entries.push(Entry {
        name: n,
        ino: 0,
        mode: S_IFDIR | (mode & 0o7777),
        uid: 0,
        gid: 0,
        nlink: 2,
        mtime: 0,
        rdevmajor: 0,
        rdevminor: 0,
        data: Vec::new(),
    });
    serialize_all_checked(&archives)
}

pub fn rm(data: &[u8], name: &str, recursive: bool) -> Result<Vec<u8>> {
    let n = required_name(name)?;
    let archives = parse_all(data)?;
    let archives: Vec<Vec<Entry>> = archives
        .into_iter()
        .map(|entries| {
            entries
                .into_iter()
                .filter(|e| {
                    if e.name == n {
                        false
                    } else if recursive && e.name.starts_with(&(n.clone() + "/")) {
                        false
                    } else {
                        true
                    }
                })
                .collect()
        })
        .collect();
    serialize_all_checked(&archives)
}

pub fn mv(data: &[u8], from: &str, to: &str) -> Result<Vec<u8>> {
    let mut archives = parse_all(data)?;
    let src = required_name(from)?;
    let dst = required_name(to)?;
    let mut found = false;
    for entries in archives.iter_mut().rev() {
        if let Some(e) = find_mut(entries, &src) {
            e.name = dst.clone();
            found = true;
            break;
        }
    }
    ensure!(found, "cpio: {from} not found");
    serialize_all_checked(&archives)
}

pub fn apply_command(mut archive: Vec<u8>, cmd: &str) -> Result<Vec<u8>> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    ensure!(!parts.is_empty(), "empty cpio command");
    match parts[0] {
        "exists" => {
            ensure!(parts.len() == 2, "usage: exists <entry>");
            if !exists(&archive, parts[1])? {
                std::process::exit(1);
            }
            Ok(archive)
        }
        "extract" => {
            ensure!(parts.len() >= 2, "usage: extract <entry> [outfile]");
            let out = Path::new(if parts.len() >= 3 { parts[2] } else { parts[1] });
            extract_to(&archive, parts[1], out)?;
            Ok(archive)
        }
        "add" => {
            ensure!(parts.len() == 4, "usage: add <mode> <entry> <infile>");
            let mode = u32::from_str_radix(parts[1].trim_start_matches('0'), 8).unwrap_or(0o755);
            archive = add_file(&archive, mode, parts[2], Path::new(parts[3]))?;
            Ok(archive)
        }
        "mkdir" => {
            ensure!(parts.len() == 3, "usage: mkdir <mode> <entry>");
            let mode = u32::from_str_radix(parts[1].trim_start_matches('0'), 8).unwrap_or(0o755);
            archive = mkdir(&archive, mode, parts[2])?;
            Ok(archive)
        }
        "rm" => {
            let (recursive, name) = if parts.get(1) == Some(&"-r") {
                ensure!(parts.len() == 3, "usage: rm -r <entry>");
                (true, parts[2])
            } else {
                ensure!(parts.len() == 2, "usage: rm <entry>");
                (false, parts[1])
            };
            archive = rm(&archive, name, recursive)?;
            Ok(archive)
        }
        "mv" => {
            ensure!(parts.len() == 3, "usage: mv <from> <to>");
            archive = mv(&archive, parts[1], parts[2])?;
            Ok(archive)
        }
        "hook-inits" => {
            ensure!(parts.len() == 2, "usage: hook-inits <ethinit-stub>");
            let stub = fs::read(parts[1])?;
            let (next, n) = crate::hook::hook_cpio(&archive, &stub)?;
            println!("HOOKED_INITS    [{n}]");
            Ok(next)
        }
        "restore-init-hook" => {
            ensure!(parts.len() == 1, "usage: restore-init-hook");
            crate::hook::restore_cpio(&archive)
        }
        other => bail!("unknown cpio command: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn put_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u64(data: &mut [u8], offset: usize, value: u64) {
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn synthetic_aarch64_elf(first_stage: bool, stub: bool) -> Vec<u8> {
        let mut data = vec![0u8; 512];
        data[..4].copy_from_slice(b"\x7fELF");
        data[4] = 2; // ELFCLASS64
        data[5] = 1; // ELFDATA2LSB
        data[6] = 1; // EV_CURRENT
        put_u16(&mut data, 16, 3); // ET_DYN
        put_u16(&mut data, 18, 183); // EM_AARCH64
        put_u32(&mut data, 20, 1);
        put_u64(&mut data, 24, 0x100);
        put_u64(&mut data, 32, 64);
        put_u16(&mut data, 52, 64);
        put_u16(&mut data, 54, 56);
        put_u16(&mut data, 56, 1);

        let phdr = 64;
        put_u32(&mut data, phdr, 1); // PT_LOAD
        put_u32(&mut data, phdr + 4, 5); // PF_R | PF_X
        let data_len = data.len() as u64;
        put_u64(&mut data, phdr + 32, data_len);
        put_u64(&mut data, phdr + 40, data_len);
        put_u64(&mut data, phdr + 48, 0x1000);

        if first_stage {
            let landmark = b"init first stage started!";
            data[320..320 + landmark.len()].copy_from_slice(landmark);
        }
        if stub {
            put_u64(&mut data, 384, crate::elfpatch::MAGIC_ORIG_ENTRY);
            put_u64(&mut data, 392, crate::elfpatch::MAGIC_STUB_VADDR);
        }
        data
    }

    fn entry(name: &str, data: &[u8]) -> Entry {
        Entry {
            name: name.to_string(),
            ino: 1,
            mode: S_IFREG | 0o600,
            uid: 0,
            gid: 0,
            nlink: 1,
            mtime: 0,
            rdevmajor: 0,
            rdevminor: 0,
            data: data.to_vec(),
        }
    }

    fn symlink(name: &str, target: &str) -> Entry {
        let mut entry = entry(name, target.as_bytes());
        entry.mode = S_IFLNK | 0o777;
        entry
    }

    #[test]
    fn rejects_missing_trailer_and_parent_traversal() {
        assert!(parse(&[])
            .unwrap_err()
            .to_string()
            .contains("missing TRAILER"));
        let archive = serialize(&[entry("../manager_token", b"old")]);
        assert!(parse(&archive)
            .unwrap_err()
            .to_string()
            .contains("parent traversal"));

        for name in ["init/", "init/.", "./TRAILER!!!"] {
            let archive = serialize(&[entry(name, b"ambiguous")]);
            assert!(parse(&archive).unwrap_err().to_string().contains(
                if name.contains("TRAILER") {
                    "reserved trailer"
                } else {
                    "trailing directory syntax"
                }
            ));
        }

        let mut directory = entry("system//./", b"");
        directory.mode = S_IFDIR | 0o755;
        assert!(parse(&serialize(&[directory])).is_err());

        let max_name = format!("/{}init", "./".repeat(2045));
        assert_eq!(max_name.len(), MAX_ENTRY_NAME);
        assert!(parse(&serialize(&[entry(&max_name, b"ok")])).is_ok());
        let skipped_by_linux = format!("{}init", "./".repeat(2046));
        assert_eq!(skipped_by_linux.len(), MAX_ENTRY_NAME + 1);
        assert!(parse(&serialize(&[entry(&skipped_by_linux, b"too long")])).is_err());
        let long_component = format!("{}/init", "a".repeat(MAX_COMPONENT_NAME + 1));
        assert!(parse(&serialize(&[entry(&long_component, b"too wide")])).is_err());
    }

    #[test]
    fn add_replaces_all_duplicate_entries_with_one_final_regular_file() {
        let archive = serialize_all(&[
            vec![entry("./ethereal.manager_token", b"first")],
            vec![
                entry("//ethereal.manager_token", b"second"),
                entry("ethereal.manager_token", b"last"),
            ],
        ]);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let source = std::env::temp_dir().join(format!(
            "ethereal-cpio-token-{}-{nonce}",
            std::process::id()
        ));
        fs::write(&source, b"new").unwrap();

        let patched = add_file(&archive, 0o400, "ethereal.manager_token", &source).unwrap();
        let archives = parse_all(&patched).unwrap();
        let matching: Vec<&Entry> = archives
            .iter()
            .flat_map(|entries| entries.iter())
            .filter(|entry| entry.name == "ethereal.manager_token")
            .collect();
        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].data, b"new");
        assert_eq!(matching[0].mode, S_IFREG | 0o400);

        fs::remove_file(source).unwrap();
    }

    #[test]
    fn path_aliases_resolve_to_the_last_extracted_entry() {
        let archive = serialize_all(&[vec![entry("init", b"old")], vec![entry("./init", b"new")]]);
        let parsed = parse_all(&archive).unwrap();
        assert_eq!(parsed[0][0].name, "init");
        assert_eq!(parsed[1][0].name, "init");

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let output = std::env::temp_dir().join(format!(
            "ethereal-cpio-extract-{}-{nonce}",
            std::process::id()
        ));
        extract_to(&archive, "/./init", &output).unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"new");
        fs::remove_file(output).unwrap();
    }

    #[test]
    fn rejects_entries_that_descend_through_a_live_symlink() {
        let archive = serialize(&[
            entry("init", b"older root init"),
            symlink("x", "/"),
            entry("x/init", b"would overwrite root init"),
        ]);
        let error = parse_all(&archive).unwrap_err();
        assert!(error.to_string().contains("symlink ancestor"));

        let mut real_directory = entry("x", b"");
        real_directory.mode = S_IFDIR | 0o755;
        let replaced = serialize(&[
            symlink("x", "/"),
            real_directory,
            entry("x/child", b"ordinary child"),
        ]);
        assert!(parse_all(&replaced).is_ok());
        assert!(mv(&replaced, "x", "y").is_err());

        let mut skipped_directory = entry("x", b"Linux skips this malformed directory");
        skipped_directory.mode = S_IFDIR | 0o755;
        let fake_replacement = serialize(&[
            entry("init", b"older root init"),
            symlink("x", "/"),
            skipped_directory,
            entry("x/init", b"would overwrite root init"),
        ]);
        assert!(parse_all(&fake_replacement)
            .unwrap_err()
            .to_string()
            .contains("unsupported body"));

        let oversized_target = "a".repeat(MAX_SYMLINK_TARGET + 1);
        assert!(parse(&serialize(&[symlink("long-link", &oversized_target)])).is_err());
    }

    #[test]
    fn cpio_commands_share_the_same_terminal_path_rules() {
        let archive = serialize(&[entry("init", b"stock")]);
        assert!(exists(&archive, "init/").is_err());
        assert!(rm(&archive, "init/.", false).is_err());
        assert!(mkdir(&archive, 0o755, "system/").is_err());
        assert!(mv(&archive, "init", "./TRAILER!!!").is_err());
        assert!(add_file(&archive, 0o600, "init/", Path::new("unused-missing-input")).is_err());
    }

    #[test]
    fn hook_targets_only_the_effective_root_init_and_repatches_from_backup() {
        let older_root = entry("init", b"older concatenated root init");
        let mut effective_root = entry("init", &synthetic_aarch64_elf(true, false));
        effective_root.mode = S_IFREG | 0o750;
        effective_root.uid = 123;
        effective_root.gid = 456;
        effective_root.mtime = 789;
        let lookalike = entry("system/bin/init", &synthetic_aarch64_elf(true, false));
        let archive = serialize_all(&[
            vec![older_root.clone()],
            vec![lookalike.clone(), effective_root.clone()],
        ]);
        let stub = synthetic_aarch64_elf(false, true);

        let (patched, count) = crate::hook::hook_cpio(&archive, &stub).unwrap();
        assert_eq!(count, 1);
        let parsed = parse_all(&patched).unwrap();
        assert_eq!(parsed[0][0], older_root);
        assert_eq!(parsed[1][0], lookalike);

        let patched_root = parsed[1].iter().find(|entry| entry.name == "init").unwrap();
        assert!(crate::elfpatch::is_patched(&patched_root.data));
        assert_eq!(patched_root.mode, effective_root.mode);
        assert_eq!(patched_root.uid, effective_root.uid);
        assert_eq!(patched_root.gid, effective_root.gid);
        assert_eq!(patched_root.mtime, effective_root.mtime);

        let backup = parsed[1]
            .iter()
            .find(|entry| entry.name == "init.ethereal.bak")
            .unwrap();
        let mut expected_backup = effective_root.clone();
        expected_backup.name = "init.ethereal.bak".to_string();
        expected_backup.ino = backup.ino;
        expected_backup.nlink = 1;
        assert_eq!(*backup, expected_backup);
        assert_ne!(backup.ino, patched_root.ino);

        let (repatched, count) = crate::hook::hook_cpio(&patched, &stub).unwrap();
        assert_eq!(count, 1);
        assert_eq!(repatched, patched);
        let restored = crate::hook::restore_cpio(&repatched).unwrap();
        assert_eq!(restored, archive);
    }

    #[test]
    fn hook_failure_does_not_fall_back_to_non_root_init() {
        let root = entry("init", &synthetic_aarch64_elf(false, false));
        let lookalike = entry("system/bin/init", &synthetic_aarch64_elf(true, false));
        let archive = serialize(&[root, lookalike]);
        let stub = synthetic_aarch64_elf(false, true);

        let error = crate::hook::hook_cpio(&archive, &stub).unwrap_err();
        assert!(error.to_string().contains("refusing to patch"));
        assert!(!archive
            .windows(crate::elfpatch::PATCH_MARKER.len())
            .any(|window| window == crate::elfpatch::PATCH_MARKER));
        assert!(!exists(&archive, "init.ethereal.bak").unwrap());
    }

    #[test]
    fn hook_treats_dot_prefixed_init_as_the_effective_root() {
        let older_root = entry("init", &synthetic_aarch64_elf(true, false));
        let effective_root = entry("./init", &synthetic_aarch64_elf(true, false));
        let archive = serialize_all(&[vec![older_root.clone()], vec![effective_root.clone()]]);
        let stub = synthetic_aarch64_elf(false, true);

        let (patched, count) = crate::hook::hook_cpio(&archive, &stub).unwrap();
        assert_eq!(count, 1);
        let parsed = parse_all(&patched).unwrap();
        assert!(!crate::elfpatch::is_patched(&parsed[0][0].data));
        assert!(crate::elfpatch::is_patched(&parsed[1][0].data));

        let restored = parse_all(&crate::hook::restore_cpio(&patched).unwrap()).unwrap();
        assert_eq!(restored[0][0].data, older_root.data);
        assert_eq!(restored[1][0].data, effective_root.data);
        assert!(!restored[1]
            .iter()
            .any(|entry| entry.name == "init.ethereal.bak"));
    }

    #[test]
    fn hook_rejects_a_hard_linked_root_init() {
        let mut root = entry("init", &synthetic_aarch64_elf(true, false));
        root.nlink = 2;
        let archive = serialize(&[root]);
        let stub = synthetic_aarch64_elf(false, true);

        let error = crate::hook::hook_cpio(&archive, &stub).unwrap_err();
        assert!(error.to_string().contains("hard-linked"));
        assert!(!exists(&archive, "init.ethereal.bak").unwrap());
    }

    #[test]
    fn hook_rejects_hardlinks_elsewhere_in_the_ramdisk() {
        let root = entry("init", &synthetic_aarch64_elf(true, false));
        let mut linked = entry("other", b"shared inode");
        linked.nlink = 2;
        let archive = serialize(&[root, linked]);
        let stub = synthetic_aarch64_elf(false, true);

        let error = crate::hook::hook_cpio(&archive, &stub).unwrap_err();
        assert!(error.to_string().contains("hard-linked non-directory"));
    }
}
