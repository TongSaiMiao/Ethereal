//! ELF-hook every first-stage init in a ramdisk (all concatenated cpio archives).
//! OEM /init is kept; only e_entry is retargeted. Not a KernelSU-style replace.

use crate::cpio::{self, Entry};
use crate::elfpatch;
use crate::scan;
use anyhow::Result;

fn has_pt_interp(data: &[u8]) -> bool {
    goblin::elf::Elf::parse(data)
        .map(|e| {
            e.program_headers
                .iter()
                .any(|p| p.p_type == goblin::elf::program_header::PT_INTERP)
        })
        .unwrap_or(false)
}

fn is_hookable_elf(data: &[u8]) -> bool {
    data.len() >= 64
        && data.starts_with(b"\x7fELF")
        && scan::scan_init(data).is_hookable()
        && !has_pt_interp(data)
}

fn restore_baks(entries: &mut [Entry]) {
    let baks: Vec<(String, Vec<u8>)> = entries
        .iter()
        .filter(|e| e.name.ends_with(".ethereal.bak"))
        .map(|e| {
            (
                e.name.trim_end_matches(".ethereal.bak").to_string(),
                e.data.clone(),
            )
        })
        .collect();
    for (orig, data) in baks {
        if let Some(e) = entries.iter_mut().find(|e| e.name == orig) {
            e.data = data;
        }
    }
}

fn hook_entries(entries: &mut Vec<Entry>, stub: &[u8]) -> Result<usize> {
    restore_baks(entries);
    let mut backups: Vec<Entry> = Vec::new();
    let mut n = 0usize;
    let existing: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
    for e in entries.iter_mut() {
        if e.is_dir() || e.name.ends_with(".ethereal.bak") {
            continue;
        }
        if !is_hookable_elf(&e.data) {
            continue;
        }
        if elfpatch::is_patched(&e.data) {
            continue;
        }
        let bak = format!("{}.ethereal.bak", e.name);
        if !existing.iter().any(|n| n == &bak) && !backups.iter().any(|b| b.name == bak) {
            let mut b = e.clone();
            b.name = bak;
            backups.push(b);
        }
        elfpatch::patch_init(&mut e.data, stub)?;
        println!("HOOKED          [{}]", e.name);
        n += 1;
    }
    entries.extend(backups);
    Ok(n)
}

pub fn hook_cpio(data: &[u8], stub: &[u8]) -> Result<(Vec<u8>, usize)> {
    let mut archives = cpio::parse_all(data)?;
    let mut n = 0usize;
    for arch in &mut archives {
        n += hook_entries(arch, stub)?;
    }
    Ok((cpio::serialize_all(&archives), n))
}
