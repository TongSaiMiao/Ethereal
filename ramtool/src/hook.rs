//! ELF-hook the effective root `/init` in a concatenated ramdisk.
//!
//! Later cpio archives and later entries within an archive win during initramfs
//! extraction. Only that final root `init` is patched; similarly named binaries
//! are never scanned or modified.

use crate::cpio::{self, Entry};
use crate::elfpatch;
use anyhow::{ensure, Result};

const ROOT_INIT: &str = "init";
const ROOT_INIT_BACKUP: &str = "init.ethereal.bak";

fn last_entry_position(archives: &[Vec<Entry>], name: &str) -> Option<(usize, usize)> {
    archives
        .iter()
        .enumerate()
        .rev()
        .find_map(|(archive_index, entries)| {
            entries
                .iter()
                .rposition(|entry| entry.name == name)
                .map(|entry_index| (archive_index, entry_index))
        })
}

fn backup_indices(entries: &[Entry]) -> Vec<usize> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| (entry.name == ROOT_INIT_BACKUP).then_some(index))
        .collect()
}

fn ensure_no_hardlinked_files(archives: &[Vec<Entry>]) -> Result<()> {
    for entry in archives.iter().flat_map(|entries| entries.iter()) {
        ensure!(
            entry.is_dir() || entry.nlink == 1,
            "ramdisk contains hard-linked non-directory entry {:?}; refusing an ambiguous init hook",
            entry.name
        );
    }
    Ok(())
}

pub fn hook_cpio(data: &[u8], stub: &[u8]) -> Result<(Vec<u8>, usize)> {
    let mut archives = cpio::parse_all(data)?;
    ensure_no_hardlinked_files(&archives)?;
    // "FirstStageMain" can show up in other binaries too. Touch only the root init
    // that actually wins extraction, and leave every lookalike alone.
    let (archive_index, init_index) = last_entry_position(&archives, ROOT_INIT)
        .ok_or_else(|| anyhow::anyhow!("ramdisk root /init not found"))?;
    let backup_indices = backup_indices(&archives[archive_index]);
    ensure!(
        backup_indices.len() <= 1,
        "multiple {ROOT_INIT_BACKUP} entries accompany the effective root /init"
    );

    let current = archives[archive_index][init_index].clone();
    ensure!(
        current.is_regular(),
        "ramdisk root /init is not a regular file"
    );
    ensure!(
        current.nlink == 1,
        "ramdisk root /init is hard-linked; refusing an ambiguous entry hook"
    );
    let (original, add_backup) = if let Some(&backup_index) = backup_indices.first() {
        ensure!(
            elfpatch::is_patched(&current.data),
            "{ROOT_INIT_BACKUP} exists but the effective root /init is not Ethereal-hooked"
        );
        let mut original = archives[archive_index][backup_index].clone();
        ensure!(
            original.is_regular(),
            "{ROOT_INIT_BACKUP} is not a regular file"
        );
        ensure!(
            !elfpatch::is_patched(&original.data),
            "{ROOT_INIT_BACKUP} is already Ethereal-hooked"
        );
        ensure!(
            original.nlink == 1,
            "{ROOT_INIT_BACKUP} must not be hard-linked"
        );
        original.name = current.name.clone();
        original.ino = current.ino;
        original.nlink = current.nlink;
        (original, false)
    } else {
        ensure!(
            !elfpatch::is_patched(&current.data),
            "effective root /init is Ethereal-hooked but its backup is missing"
        );
        (current, true)
    };

    let mut patched = original.clone();
    elfpatch::patch_init(&mut patched.data, stub)?;
    archives[archive_index][init_index] = patched;
    if add_backup {
        let mut backup = original;
        backup.name = ROOT_INIT_BACKUP.to_string();
        // A copied inode number plus nlink > 1 turns this safety copy into a real
        // hardlink during initramfs extraction. Give the backup its own inode.
        backup.ino = 0;
        backup.nlink = 1;
        archives[archive_index].push(backup);
    }

    println!("HOOKED          [/init]");
    Ok((cpio::serialize_all_checked(&archives)?, 1))
}

pub fn restore_cpio(data: &[u8]) -> Result<Vec<u8>> {
    let mut archives = cpio::parse_all(data)?;
    let (archive_index, init_index) = last_entry_position(&archives, ROOT_INIT)
        .ok_or_else(|| anyhow::anyhow!("ramdisk root /init not found"))?;
    let backup_indices = backup_indices(&archives[archive_index]);
    ensure!(
        backup_indices.len() == 1,
        "expected exactly one {ROOT_INIT_BACKUP} beside the effective root /init"
    );
    let current = archives[archive_index][init_index].clone();
    ensure!(
        elfpatch::is_patched(&current.data),
        "effective root /init is not Ethereal-hooked"
    );
    ensure!(current.nlink == 1, "effective root /init is hard-linked");

    let backup_index = backup_indices[0];
    let mut restored = archives[archive_index][backup_index].clone();
    ensure!(
        restored.is_regular(),
        "{ROOT_INIT_BACKUP} is not a regular file"
    );
    ensure!(
        !elfpatch::is_patched(&restored.data),
        "{ROOT_INIT_BACKUP} is already Ethereal-hooked"
    );
    ensure!(
        restored.nlink == 1,
        "{ROOT_INIT_BACKUP} must not be hard-linked"
    );
    restored.name = current.name;
    restored.ino = current.ino;
    restored.nlink = current.nlink;

    let entries = std::mem::take(&mut archives[archive_index]);
    let mut next = Vec::with_capacity(entries.len().saturating_sub(1));
    for (index, entry) in entries.into_iter().enumerate() {
        if index == init_index {
            next.push(restored.clone());
        } else if index != backup_index {
            next.push(entry);
        }
    }
    archives[archive_index] = next;
    println!("RESTORED        [/init]");
    cpio::serialize_all_checked(&archives)
}
