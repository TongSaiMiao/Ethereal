use anyhow::{bail, ensure, Result};
use std::fs;
use std::io::Write;
use std::path::Path;

const MAGIC_NEWC: &[u8] = b"070701";
const MAGIC_CRC: &[u8] = b"070702";
const TRAILER: &str = "TRAILER!!!";

const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;
const MAX_ENTRY_NAME: usize = 4096;
const MAX_ENTRIES: usize = 65_536;
const MAX_ARCHIVES: usize = 64;

#[derive(Clone, Debug)]
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
        self.mode & S_IFDIR != 0
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

fn normalize(name: &str) -> String {
    name.trim_start_matches('/').to_string()
}

fn validate_name(name: &str) -> Result<()> {
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
    Ok(())
}

pub fn parse(data: &[u8]) -> Result<Vec<Entry>> {
    Ok(parse_from(data, 0)?.0)
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
        let name = std::str::from_utf8(&name_bytes[..name_bytes.len() - 1])?.to_string();
        let data_off = align4(name_end);
        ensure!(data_off <= data.len(), "cpio: truncated name padding");
        let file_end = data_off
            .checked_add(filesize)
            .ok_or_else(|| anyhow::anyhow!("cpio: file range overflow"))?;
        ensure!(file_end <= data.len(), "cpio: truncated file {name}");
        let file_data = data[data_off..file_end].to_vec();
        off = align4(file_end);
        ensure!(off <= data.len(), "cpio: truncated file padding for {name}");
        if name == TRAILER {
            return Ok((entries, off));
        }
        if name.is_empty() || name == "." {
            continue;
        }
        let name = normalize(&name);
        validate_name(&name)?;
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
    Ok(archives)
}

pub fn serialize_all(archives: &[Vec<Entry>]) -> Vec<u8> {
    let mut out = Vec::new();
    for a in archives {
        out.extend(serialize(a));
    }
    out
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
    let n = normalize(name);
    entries.iter_mut().find(|e| e.name == n)
}

fn find_any<'a>(archives: &'a [Vec<Entry>], name: &str) -> Option<&'a Entry> {
    let n = normalize(name);
    archives.iter().find_map(|a| a.iter().find(|e| e.name == n))
}

pub fn exists(data: &[u8], name: &str) -> Result<bool> {
    Ok(find_any(&parse_all(data)?, name).is_some())
}

pub fn extract_to(data: &[u8], name: &str, out: &Path) -> Result<()> {
    let archives = parse_all(data)?;
    let e = find_any(&archives, name).ok_or_else(|| anyhow::anyhow!("cpio: {name} not found"))?;
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
    let n = normalize(name);
    validate_name(&n)?;
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
    Ok(serialize_all(&archives))
}

pub fn mkdir(data: &[u8], mode: u32, name: &str) -> Result<Vec<u8>> {
    let mut archives = parse_all(data)?;
    if archives.is_empty() {
        archives.push(Vec::new());
    }
    let n = normalize(name);
    validate_name(&n)?;
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
    Ok(serialize_all(&archives))
}

pub fn rm(data: &[u8], name: &str, recursive: bool) -> Result<Vec<u8>> {
    let n = normalize(name);
    validate_name(&n)?;
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
    Ok(serialize_all(&archives))
}

pub fn mv(data: &[u8], from: &str, to: &str) -> Result<Vec<u8>> {
    let mut archives = parse_all(data)?;
    let src = normalize(from);
    let dst = normalize(to);
    validate_name(&src)?;
    validate_name(&dst)?;
    let mut found = false;
    for entries in &mut archives {
        if let Some(e) = find_mut(entries, &src) {
            e.name = dst.clone();
            found = true;
            break;
        }
    }
    ensure!(found, "cpio: {from} not found");
    Ok(serialize_all(&archives))
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
        other => bail!("unknown cpio command: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

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
    }

    #[test]
    fn add_replaces_all_duplicate_entries_with_one_final_regular_file() {
        let archive = serialize_all(&[
            vec![entry("ethereal.manager_token", b"first")],
            vec![
                entry("ethereal.manager_token", b"second"),
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
}
