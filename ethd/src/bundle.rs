//! Embedded ramtool, ethinit, and per-KMI ethereal.ko (filled by build.rs).

use anyhow::{Result, bail};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub static RAMTOOL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded/ramtool"));
pub static ETHINIT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/embedded/ethinit"));

include!(concat!(env!("OUT_DIR"), "/embedded_kos.rs"));

fn looks_elf(data: &[u8]) -> bool {
    data.len() > 4 && data.starts_with(b"\x7fELF")
}

fn write_exec(path: &Path, data: &[u8]) -> Result<()> {
    if data.is_empty() || !looks_elf(data) {
        bail!(
            "{}: bundled blob missing (rebuild ramtool/ethinit/ethereal.ko first)",
            path.display()
        );
    }
    fs::write(path, data)?;
    let mut perm = fs::metadata(path)?.permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm)?;
    Ok(())
}

fn maybe_write_exec(path: &Path, data: &[u8]) -> Result<()> {
    // Do not clobber a staged executable (symlink into the APK lib dir).
    if path.exists() {
        return Ok(());
    }
    write_exec(path, data)
}

/// Drop bundled binaries into `dir` (usually the patch working directory).
pub fn extract_into(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    maybe_write_exec(&dir.join("ramtool"), RAMTOOL)?;
    maybe_write_exec(&dir.join("ethinit"), ETHINIT)?;
    for (kmi, blob) in KOS {
        if !looks_elf(blob) {
            continue;
        }
        let path = dir.join(format!("ethereal.{kmi}.ko"));
        if !path.exists() {
            fs::write(path, blob)?;
        }
    }
    Ok(())
}

pub fn bundled_ko_names() -> Vec<String> {
    KOS.iter()
        .filter(|(_, blob)| looks_elf(blob))
        .map(|(kmi, _)| format!("ethereal.{kmi}.ko"))
        .collect()
}
