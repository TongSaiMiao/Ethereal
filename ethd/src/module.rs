use crate::sepolicy::get_policy_main;
use crate::{lua, module_config};
use anyhow::{Context, Result, anyhow, bail, ensure};
use const_format::concatcp;
use is_executable::is_executable;
use java_properties::PropertiesIter;
use log::{debug, info, warn};
#[cfg(unix)]
use std::os::unix::{prelude::PermissionsExt, process::CommandExt};
use std::{
    collections::{HashMap, HashSet},
    env::var as env_var,
    fs::{self, OpenOptions, remove_dir_all},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::{Component, Path, PathBuf},
    process::{Command, ExitStatus},
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};

#[allow(clippy::wildcard_imports)]
use crate::utils::*;
use crate::{
    assets,
    defs::{self, MODULE_DIR, MODULE_UPDATE_DIR},
    metamodule, restorecon,
};

const INSTALLER_CONTENT: &str = include_str!("../assets/installer.sh");
const INSTALL_MODULE_SCRIPT: &str = concatcp!(
    INSTALLER_CONTENT,
    "\n",
    "install_module",
    "\n",
    "exit 0",
    "\n"
);

const UPDATE_READY_FILE: &str = ".ethereal-update-ready";
const STAGING_PREFIX: &str = ".ethereal-stage-";
const ACTIVE_BACKUP_PREFIX: &str = ".ethereal-active-backup-";
const PENDING_BACKUP_PREFIX: &str = ".ethereal-pending-backup-";
const MAX_MODULE_ARCHIVE_SIZE: u64 = 512 * 1024 * 1024;
const MAX_MODULE_PROP_SIZE: u64 = 64 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 8_192;
const MAX_ARCHIVE_ENTRY_SIZE: u64 = 256 * 1024 * 1024;
const MAX_ARCHIVE_TOTAL_SIZE: u64 = 512 * 1024 * 1024;
const MAX_ARCHIVE_COMPRESSION_RATIO: u64 = 200;
const MAX_ARCHIVE_PATH_SIZE: usize = 4_096;
const COMPRESSION_RATIO_SLACK: u64 = 4_096;
const MIN_FREE_SPACE_RESERVE: u64 = 512 * 1024 * 1024;

static INTERNAL_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct ArchiveLimits {
    archive_size: u64,
    module_prop_size: u64,
    entries: usize,
    entry_size: u64,
    total_size: u64,
    compression_ratio: u64,
    path_size: usize,
    free_space_reserve: u64,
}

const MODULE_ARCHIVE_LIMITS: ArchiveLimits = ArchiveLimits {
    archive_size: MAX_MODULE_ARCHIVE_SIZE,
    module_prop_size: MAX_MODULE_PROP_SIZE,
    entries: MAX_ARCHIVE_ENTRIES,
    entry_size: MAX_ARCHIVE_ENTRY_SIZE,
    total_size: MAX_ARCHIVE_TOTAL_SIZE,
    compression_ratio: MAX_ARCHIVE_COMPRESSION_RATIO,
    path_size: MAX_ARCHIVE_PATH_SIZE,
    free_space_reserve: MIN_FREE_SPACE_RESERVE,
};

struct ModuleArchiveInfo {
    properties: HashMap<String, String>,
    module_id: String,
    needs_mount: bool,
    expanded_size: u64,
}

struct StagingGuard {
    path: PathBuf,
    armed: bool,
}

impl StagingGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.armed
            && let Err(err) = remove_path_no_follow(&self.path)
        {
            warn!(
                "Failed to clean staging path {}: {err}",
                self.path.display()
            );
        }
    }
}

#[derive(PartialEq, Eq)]
pub enum ModuleType {
    All,
    Active,
    Updated,
}

fn remove_path_no_follow(path: &Path) -> std::io::Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

fn create_private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn parse_module_properties(content: Vec<u8>) -> Result<HashMap<String, String>> {
    let mut properties = HashMap::new();
    PropertiesIter::new_with_encoding(Cursor::new(content), encoding_rs::UTF_8).read_into(
        |key, value| {
            properties.insert(key, value);
        },
    )?;
    Ok(properties)
}

fn normalize_archive_path(name: &str, limit: usize) -> Result<PathBuf> {
    ensure!(!name.is_empty(), "Module archive contains an empty path");
    ensure!(
        name.len() <= limit,
        "Module archive path is too long: {} bytes (max: {limit})",
        name.len()
    );
    ensure!(
        !name.contains('\\'),
        "Module archive path contains a backslash: {name:?}"
    );

    let path = Path::new(name);
    ensure!(
        !path.is_absolute(),
        "Module archive contains an absolute path: {name:?}"
    );

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                ensure!(
                    part.as_encoded_bytes().len() <= 255,
                    "Module archive path component is too long: {name:?}"
                );
                normalized.push(part);
            }
            _ => bail!("Module archive contains an unsafe path: {name:?}"),
        }
    }
    ensure!(
        !normalized.as_os_str().is_empty(),
        "Module archive contains an empty normalized path"
    );
    Ok(normalized)
}

fn validate_archive_entry_type<R: Read>(entry: &zip::read::ZipFile<'_, R>) -> Result<()> {
    ensure!(
        !entry.is_symlink(),
        "Module archive contains a symbolic link: {:?}",
        entry.name()
    );
    if let Some(mode) = entry.unix_mode() {
        let file_type = mode & 0o170000;
        ensure!(
            file_type == 0 || file_type == 0o100000 || file_type == 0o040000,
            "Module archive contains a special file: {:?}",
            entry.name()
        );
    }
    Ok(())
}

fn classic_zip_entry_count(zip_path: &Path) -> Result<usize> {
    const EOCD_MIN_SIZE: u64 = 22;
    const MAX_COMMENT_SIZE: u64 = u16::MAX as u64;
    let mut file = fs::File::open(zip_path)?;
    let length = file.metadata()?.len();
    ensure!(length >= EOCD_MIN_SIZE, "Module archive is truncated");
    let tail_start = length.saturating_sub(EOCD_MIN_SIZE + MAX_COMMENT_SIZE);
    file.seek(SeekFrom::Start(tail_start))?;
    let mut tail = Vec::with_capacity((length - tail_start) as usize);
    file.read_to_end(&mut tail)?;

    let signature = b"PK\x05\x06";
    let offset = tail
        .windows(signature.len())
        .enumerate()
        .rev()
        .find_map(|(offset, bytes)| {
            if bytes != signature || offset + EOCD_MIN_SIZE as usize > tail.len() {
                return None;
            }
            let comment_length =
                u16::from_le_bytes([tail[offset + 20], tail[offset + 21]]) as usize;
            (offset + EOCD_MIN_SIZE as usize + comment_length == tail.len()).then_some(offset)
        })
        .ok_or_else(|| anyhow!("Module archive has no valid end-of-central-directory record"))?;

    let disk = u16::from_le_bytes([tail[offset + 4], tail[offset + 5]]);
    let central_disk = u16::from_le_bytes([tail[offset + 6], tail[offset + 7]]);
    let entries_on_disk = u16::from_le_bytes([tail[offset + 8], tail[offset + 9]]);
    let total_entries = u16::from_le_bytes([tail[offset + 10], tail[offset + 11]]);
    ensure!(
        disk == 0 && central_disk == 0 && entries_on_disk == total_entries,
        "Multi-disk module archives are not supported"
    );
    ensure!(
        total_entries != u16::MAX,
        "ZIP64 module archives are not supported"
    );
    Ok(total_entries as usize)
}

fn validate_central_entry_count<R: Read + Seek>(
    zip_path: &Path,
    archive: &zip::ZipArchive<R>,
) -> Result<()> {
    let declared = classic_zip_entry_count(zip_path)?;
    ensure!(
        declared == archive.len(),
        "Module archive contains duplicate filenames or malformed central-directory entries"
    );
    Ok(())
}

fn inspect_module_archive_with_limits(
    zip_path: &Path,
    limits: ArchiveLimits,
) -> Result<ModuleArchiveInfo> {
    let metadata = fs::metadata(zip_path)?;
    ensure!(metadata.is_file(), "Module archive is not a regular file");
    ensure!(
        metadata.len() <= limits.archive_size,
        "Module archive is too large: {} bytes (max: {})",
        metadata.len(),
        limits.archive_size
    );

    let mut archive = zip::ZipArchive::new(fs::File::open(zip_path)?)?;
    validate_central_entry_count(zip_path, &archive)?;
    ensure!(
        archive.len() <= limits.entries,
        "Module archive contains too many entries: {} (max: {})",
        archive.len(),
        limits.entries
    );

    let mut paths = HashSet::with_capacity(archive.len());
    let mut total_size = 0u64;
    let mut module_prop = None;
    let mut has_system = false;
    let mut has_skip_mount = false;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        ensure!(
            !entry.encrypted(),
            "Encrypted module archive entries are not supported: {:?}",
            entry.name()
        );
        validate_archive_entry_type(&entry)?;
        let path = normalize_archive_path(entry.name(), limits.path_size)?;
        ensure!(
            paths.insert(path.clone()),
            "Module archive contains a duplicate path: {}",
            path.display()
        );
        ensure!(
            entry.size() <= limits.entry_size,
            "Module archive entry is too large: {:?} ({} bytes, max: {})",
            entry.name(),
            entry.size(),
            limits.entry_size
        );
        total_size = total_size
            .checked_add(entry.size())
            .ok_or_else(|| anyhow!("Module archive expanded size overflow"))?;
        ensure!(
            total_size <= limits.total_size,
            "Module archive expands to too much data: {total_size} bytes (max: {})",
            limits.total_size
        );

        if !entry.is_dir() && entry.size() > COMPRESSION_RATIO_SLACK {
            let compressed = entry.compressed_size().max(COMPRESSION_RATIO_SLACK);
            let allowed = compressed.saturating_mul(limits.compression_ratio);
            ensure!(
                entry.size() <= allowed,
                "Module archive entry compression ratio is too high: {:?}",
                entry.name()
            );
        }

        if path == Path::new("module.prop") {
            ensure!(!entry.is_dir(), "module.prop must be a regular file");
            ensure!(
                entry.size() <= limits.module_prop_size,
                "module.prop is too large: {} bytes (max: {})",
                entry.size(),
                limits.module_prop_size
            );
            let mut content = Vec::with_capacity(entry.size() as usize);
            entry
                .by_ref()
                .take(limits.module_prop_size + 1)
                .read_to_end(&mut content)?;
            ensure!(
                content.len() as u64 <= limits.module_prop_size,
                "module.prop exceeds the read limit"
            );
            module_prop = Some(content);
        }

        has_system |= path.starts_with("system");
        has_skip_mount |= path == Path::new("skip_mount");
    }

    let properties = parse_module_properties(
        module_prop.ok_or_else(|| anyhow!("module.prop not found in module archive"))?,
    )?;
    let module_id = properties
        .get("id")
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| anyhow!("module id not found in module.prop"))?;
    module_config::validate_module_id(&module_id)?;

    Ok(ModuleArchiveInfo {
        properties,
        module_id,
        needs_mount: has_system && !has_skip_mount,
        expanded_size: total_size,
    })
}

fn inspect_module_archive(zip_path: &Path) -> Result<ModuleArchiveInfo> {
    inspect_module_archive_with_limits(zip_path, MODULE_ARCHIVE_LIMITS)
}

fn validate_available_space(available: u64, expanded_size: u64, reserve: u64) -> Result<()> {
    let required = expanded_size
        .checked_add(reserve)
        .ok_or_else(|| anyhow!("Required extraction space overflow"))?;
    ensure!(
        available >= required,
        "Not enough free space for module extraction: {available} bytes available, {required} bytes required"
    );
    Ok(())
}

fn ensure_extraction_space(path: &Path, expanded_size: u64, reserve: u64) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        let stat = rustix::fs::statvfs(path)?;
        let fragment_size = if stat.f_frsize == 0 {
            stat.f_bsize
        } else {
            stat.f_frsize
        };
        let available = stat.f_bavail.saturating_mul(fragment_size);
        validate_available_space(available, expanded_size, reserve)
    }
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        let _ = path;
        validate_available_space(u64::MAX, expanded_size, reserve)
    }
}

fn extract_module_archive_with_limits(
    zip_path: &Path,
    destination: &Path,
    limits: ArchiveLimits,
) -> Result<()> {
    let archive_info = inspect_module_archive_with_limits(zip_path, limits)?;
    ensure_extraction_space(
        destination,
        archive_info.expanded_size,
        limits.free_space_reserve,
    )?;
    let archive_size = fs::metadata(zip_path)?.len();
    ensure!(
        archive_size <= limits.archive_size,
        "Module archive is too large"
    );
    let mut archive = zip::ZipArchive::new(fs::File::open(zip_path)?)?;
    validate_central_entry_count(zip_path, &archive)?;
    ensure!(archive.len() <= limits.entries, "Too many archive entries");
    let mut paths = HashSet::with_capacity(archive.len());
    let mut actual_total = 0u64;
    let mut declared_total = 0u64;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        ensure!(!entry.encrypted(), "Encrypted archive entry");
        validate_archive_entry_type(&entry)?;
        let relative = normalize_archive_path(entry.name(), limits.path_size)?;
        ensure!(paths.insert(relative.clone()), "Duplicate archive path");
        ensure!(
            entry.size() <= limits.entry_size,
            "Archive entry is too large"
        );
        declared_total = declared_total
            .checked_add(entry.size())
            .ok_or_else(|| anyhow!("Module archive expanded size overflow"))?;
        ensure!(
            declared_total <= limits.total_size,
            "Module archive exceeded the declared expanded size limit"
        );
        if !entry.is_dir() && entry.size() > COMPRESSION_RATIO_SLACK {
            let compressed = entry.compressed_size().max(COMPRESSION_RATIO_SLACK);
            ensure!(
                entry.size() <= compressed.saturating_mul(limits.compression_ratio),
                "Archive entry compression ratio is too high"
            );
        }
        if relative == Path::new("module.prop") {
            ensure!(
                entry.size() <= limits.module_prop_size,
                "module.prop is too large"
            );
        }

        if relative.starts_with("META-INF") {
            continue;
        }

        let output = destination.join(&relative);
        ensure!(
            output.starts_with(destination),
            "Archive entry escaped the staging directory"
        );
        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)?;
        let copied = std::io::copy(&mut entry.by_ref().take(limits.entry_size + 1), &mut file)?;
        ensure!(
            copied == entry.size(),
            "Archive entry size changed while extracting"
        );
        actual_total = actual_total
            .checked_add(copied)
            .ok_or_else(|| anyhow!("Module archive expanded size overflow"))?;
        ensure!(
            actual_total <= limits.total_size,
            "Module archive exceeded the expanded size limit"
        );
        file.flush()?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o777))?;
        }
    }
    Ok(())
}

fn extract_module_archive(zip_path: &Path, destination: &Path) -> Result<()> {
    extract_module_archive_with_limits(zip_path, destination, MODULE_ARCHIVE_LIMITS)
}

fn exec_install_script(
    module_file: &str,
    is_metamodule: bool,
    module_id: &str,
    staging_dir: &Path,
) -> Result<()> {
    let realpath = std::fs::canonicalize(module_file)
        .with_context(|| format!("realpath: {module_file} failed"))?;

    // Get install script from metamodule module
    let install_script =
        metamodule::get_install_script(is_metamodule, INSTALLER_CONTENT, INSTALL_MODULE_SCRIPT)?;

    let result = Command::new(assets::BUSYBOX_PATH)
        .args(["sh", "-c", &install_script])
        .envs(get_common_script_envs(Some(module_id)))
        .env("ETHEREAL_MODULE_STAGING", staging_dir)
        .env("OUTFD", "1")
        .env("ZIPFILE", realpath)
        .status()?;
    ensure!(result.success(), "Failed to install module script");
    Ok(())
}

fn path_exists_no_follow(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(any(target_os = "android", target_os = "linux"))]
    {
        use rustix::fs::{Mode, OFlags, fsync, open};
        let directory = open(
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        fsync(directory)?;
        Ok(())
    }
    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    {
        fs::File::open(path)?.sync_all()?;
        Ok(())
    }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn sync_tree_no_follow(path: &Path) -> Result<()> {
    use rustix::fs::{FileType, Mode, OFlags, fstat, open};

    fn sync_directory_fd(directory_fd: rustix::fd::OwnedFd) -> Result<()> {
        use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, fstat, fsync, openat, statat};

        let mut directory = Dir::new(directory_fd)?;
        while let Some(entry) = directory.read() {
            let entry = entry?;
            let name = entry.file_name();
            if name.to_bytes() == b"." || name.to_bytes() == b".." {
                continue;
            }
            let stat = statat(directory.fd()?, name, AtFlags::SYMLINK_NOFOLLOW)?;
            match FileType::from_raw_mode(stat.st_mode) {
                FileType::RegularFile => {
                    let file = openat(
                        directory.fd()?,
                        name,
                        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
                        Mode::empty(),
                    )?;
                    ensure!(
                        FileType::from_raw_mode(fstat(&file)?.st_mode) == FileType::RegularFile,
                        "Module file type changed while syncing"
                    );
                    fsync(file)?;
                }
                FileType::Directory => {
                    let child = openat(
                        directory.fd()?,
                        name,
                        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
                        Mode::empty(),
                    )?;
                    sync_directory_fd(child)?;
                }
                // Symlinks and installer-created whiteout nodes are persisted
                // by syncing their containing directory; never open/follow them.
                _ => {}
            }
        }
        fsync(directory.fd()?)?;
        Ok(())
    }

    let root = open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    ensure!(
        FileType::from_raw_mode(fstat(&root)?.st_mode) == FileType::Directory,
        "Module staging root changed while syncing"
    );
    sync_directory_fd(root)
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
fn sync_tree_no_follow(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if metadata.is_file() {
        fs::File::open(path)?.sync_all()?;
        return Ok(());
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            sync_tree_no_follow(&entry?.path())?;
        }
        sync_directory(path)?;
    }
    Ok(())
}

fn validate_module_directory(
    path: &Path,
    expected_id: &str,
    require_ready: bool,
) -> Result<HashMap<String, String>> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Failed to inspect {}", path.display()))?;
    ensure!(
        metadata.file_type().is_dir() && !metadata.file_type().is_symlink(),
        "Module directory is not a real directory: {}",
        path.display()
    );

    if require_ready {
        let ready = path.join(UPDATE_READY_FILE);
        let ready_metadata = fs::symlink_metadata(&ready)
            .with_context(|| format!("Update completion marker missing: {}", ready.display()))?;
        ensure!(
            ready_metadata.file_type().is_file() && !ready_metadata.file_type().is_symlink(),
            "Update completion marker is not a regular file"
        );
    }

    let properties = read_module_prop(path)?;
    let actual_id = properties
        .get("id")
        .map(|id| id.trim())
        .ok_or_else(|| anyhow!("module.prop has no id"))?;
    ensure!(
        actual_id == expected_id,
        "Module id mismatch: directory is {expected_id:?}, module.prop is {actual_id:?}"
    );
    module_config::validate_module_id(actual_id)?;
    Ok(properties)
}

fn staging_path(update_root: &Path, module_id: &str) -> PathBuf {
    let counter = INTERNAL_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    update_root.join(format!(
        "{STAGING_PREFIX}{module_id}-{}-{counter}",
        std::process::id()
    ))
}

fn recover_backup(
    backup: &Path,
    target: &Path,
    rename: &mut impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    if path_exists_no_follow(target) {
        remove_path_no_follow(backup)?;
    } else {
        rename(backup, target).with_context(|| {
            format!(
                "Failed to recover interrupted module update {} -> {}",
                backup.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

fn recover_and_clean_internal_paths(
    modules_root: &Path,
    update_root: &Path,
    rename: &mut impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<()> {
    let mut paths = fs::read_dir(update_root)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with('.'))
        })
        .collect::<Vec<_>>();
    paths.sort();

    // Restore backups before deleting abandoned staging directories. This also
    // makes a process death between the two activation renames recoverable.
    for path in &paths {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if let Some(module_id) = name.strip_prefix(ACTIVE_BACKUP_PREFIX) {
            if module_config::validate_module_id(module_id).is_ok() {
                let active = modules_root.join(module_id);
                let pending = update_root.join(module_id);
                if path_exists_no_follow(&active.join(UPDATE_READY_FILE)) {
                    // The new directory reached modules/, but activation did
                    // not finish. Put it back in the pending queue before
                    // restoring the old active module.
                    if path_exists_no_follow(&pending) {
                        remove_path_no_follow(&active)?;
                    } else {
                        rename(&active, &pending)?;
                    }
                    rename(path, &active)?;
                    sync_directory(modules_root)?;
                    sync_directory(update_root)?;
                } else {
                    recover_backup(path, &active, rename)?;
                }
            }
        } else if let Some(module_id) = name.strip_prefix(PENDING_BACKUP_PREFIX)
            && module_config::validate_module_id(module_id).is_ok()
        {
            recover_backup(path, &update_root.join(module_id), rename)?;
        }
    }

    for path in paths {
        if path_exists_no_follow(&path) {
            warn!("Removing abandoned module update path: {}", path.display());
            remove_path_no_follow(&path)?;
        }
    }

    // A first-time install has no old-module backup. A ready marker in the
    // active tree therefore also means activation was interrupted.
    let mut active_paths = fs::read_dir(modules_root)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    active_paths.sort();
    for active in active_paths {
        if !path_exists_no_follow(&active.join(UPDATE_READY_FILE)) {
            continue;
        }
        let Some(module_id) = active.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if module_config::validate_module_id(module_id).is_err() {
            continue;
        }
        let pending = update_root.join(module_id);
        warn!(
            "Rolling back interrupted first-time activation: {}",
            active.display()
        );
        if path_exists_no_follow(&pending) {
            remove_path_no_follow(&active)?;
        } else {
            rename(&active, &pending)?;
        }
        sync_directory(modules_root)?;
        sync_directory(update_root)?;
    }
    Ok(())
}

fn commit_staged_update_with_sync(
    modules_root: &Path,
    update_root: &Path,
    module_id: &str,
    staging: &Path,
    sync_staging: &mut impl FnMut(&Path) -> Result<()>,
) -> Result<PathBuf> {
    validate_module_directory(staging, module_id, false)?;
    sync_staging(staging)?;
    validate_module_directory(staging, module_id, false)?;
    let ready_path = staging.join(UPDATE_READY_FILE);
    let mut ready = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&ready_path)?;
    ready.write_all(module_id.as_bytes())?;
    ready.sync_all()?;
    sync_directory(staging)?;
    validate_module_directory(staging, module_id, true)?;

    let target = update_root.join(module_id);
    let backup = update_root.join(format!("{PENDING_BACKUP_PREFIX}{module_id}"));
    if path_exists_no_follow(&backup) {
        if path_exists_no_follow(&target) {
            remove_path_no_follow(&backup)?;
        } else {
            fs::rename(&backup, &target)?;
        }
    }

    let had_previous = path_exists_no_follow(&target);
    if had_previous {
        fs::rename(&target, &backup)
            .with_context(|| format!("Failed to preserve pending update {}", target.display()))?;
    }

    if let Err(commit_error) = fs::rename(staging, &target) {
        if had_previous && let Err(restore_error) = fs::rename(&backup, &target) {
            return Err(anyhow!(
                "Failed to commit update ({commit_error}); failed to restore previous pending update ({restore_error})"
            ));
        }
        return Err(commit_error).with_context(|| {
            format!(
                "Failed to atomically commit module update {}",
                target.display()
            )
        });
    }
    sync_directory(update_root)?;

    if had_previous && let Err(err) = remove_path_no_follow(&backup) {
        warn!(
            "Failed to remove old pending update {}: {err}",
            backup.display()
        );
    }

    let active = modules_root.join(module_id);
    if path_exists_no_follow(&active)
        && let Err(err) = ensure_file_exists(active.join(defs::UPDATE_FILE_NAME))
    {
        warn!("Failed to mark module {module_id} as pending update: {err}");
    }
    Ok(target)
}

fn commit_staged_update(
    modules_root: &Path,
    update_root: &Path,
    module_id: &str,
    staging: &Path,
) -> Result<PathBuf> {
    let mut sync_staging = sync_tree_no_follow;
    commit_staged_update_with_sync(
        modules_root,
        update_root,
        module_id,
        staging,
        &mut sync_staging,
    )
}

fn prepare_staged_update(
    modules_root: &Path,
    update_root: &Path,
    module_id: &str,
    install: impl FnOnce(&Path) -> Result<()>,
) -> Result<PathBuf> {
    module_config::validate_module_id(module_id)?;
    create_private_dir(modules_root)?;
    create_private_dir(update_root)?;
    let staging = staging_path(update_root, module_id);
    fs::create_dir(&staging)?;
    #[cfg(unix)]
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;
    let mut guard = StagingGuard::new(staging.clone());

    install(&staging)?;
    let target = commit_staged_update(modules_root, update_root, module_id, &staging)?;
    guard.disarm();
    Ok(target)
}

fn handle_updated_modules_with(
    modules_root: &Path,
    update_root: &Path,
    rename: &mut impl FnMut(&Path, &Path) -> std::io::Result<()>,
    mut after_activate: impl FnMut(&Path, &HashMap<String, String>) -> Result<()>,
) -> Result<()> {
    if !update_root.exists() {
        return Ok(());
    }
    create_private_dir(modules_root)?;
    recover_and_clean_internal_paths(modules_root, update_root, rename)?;

    let mut updates = fs::read_dir(update_root)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    updates.sort();

    for updated_module in updates {
        let Some(module_id) = updated_module.file_name().and_then(|name| name.to_str()) else {
            remove_path_no_follow(&updated_module)?;
            continue;
        };
        if module_id.starts_with('.') || module_config::validate_module_id(module_id).is_err() {
            warn!(
                "Removing invalid pending update: {}",
                updated_module.display()
            );
            remove_path_no_follow(&updated_module)?;
            continue;
        }

        let properties = match validate_module_directory(&updated_module, module_id, true) {
            Ok(properties) => properties,
            Err(err) => {
                warn!(
                    "Removing incomplete pending update {}: {err}",
                    updated_module.display()
                );
                remove_path_no_follow(&updated_module)?;
                continue;
            }
        };

        let active = modules_root.join(module_id);
        let backup = update_root.join(format!("{ACTIVE_BACKUP_PREFIX}{module_id}"));
        let had_active = path_exists_no_follow(&active);
        let disabled = had_active && active.join(defs::DISABLE_FILE_NAME).exists();
        let removed = had_active && active.join(defs::REMOVE_FILE_NAME).exists();

        if path_exists_no_follow(&backup) {
            recover_backup(&backup, &active, rename)?;
        }
        if had_active {
            rename(&active, &backup).with_context(|| {
                format!("Failed to preserve active module {}", active.display())
            })?;
            sync_directory(modules_root)?;
            sync_directory(update_root)?;
        }

        if let Err(activate_error) = rename(&updated_module, &active) {
            if had_active && let Err(restore_error) = rename(&backup, &active) {
                return Err(anyhow!(
                    "Failed to activate update ({activate_error}); failed to restore old module ({restore_error})"
                ));
            }
            return Err(activate_error)
                .with_context(|| format!("Failed to activate module update {module_id}"));
        }
        sync_directory(modules_root)?;
        sync_directory(update_root)?;

        let finish_activation = (|| -> Result<()> {
            if removed {
                ensure_file_exists(active.join(defs::REMOVE_FILE_NAME))?;
            } else if disabled {
                ensure_file_exists(active.join(defs::DISABLE_FILE_NAME))?;
            }
            after_activate(&active, &properties)?;
            fs::remove_file(active.join(UPDATE_READY_FILE))?;
            sync_directory(&active)?;
            sync_directory(modules_root)?;
            Ok(())
        })();

        if let Err(finish_error) = finish_activation {
            let rollback_update = rename(&active, &updated_module);
            let rollback_old = if had_active {
                rename(&backup, &active)
            } else {
                Ok(())
            };
            if let Err(rollback_error) = rollback_update.and(rollback_old) {
                return Err(anyhow!(
                    "Failed to finish update ({finish_error}); rollback also failed ({rollback_error})"
                ));
            }
            return Err(finish_error).context("Failed to finish module update activation");
        }

        if had_active && let Err(err) = remove_path_no_follow(&backup) {
            warn!("Failed to remove module backup {}: {err}", backup.display());
        }
        sync_directory(update_root)?;
    }

    match fs::remove_dir(update_root) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(())
}

pub fn handle_updated_modules() -> Result<()> {
    let mut rename = |from: &Path, to: &Path| fs::rename(from, to);
    handle_updated_modules_with(
        Path::new(MODULE_DIR),
        Path::new(MODULE_UPDATE_DIR),
        &mut rename,
        |active, properties| {
            if metamodule::is_metamodule(properties)
                && let Err(err) = metamodule::ensure_symlink(active)
            {
                warn!("Failed to create metamodule symlink: {err}");
            }
            Ok(())
        },
    )
}

/// Get common environment variables for script execution
pub fn get_common_script_envs(module_id: Option<&str>) -> Vec<(&'static str, String)> {
    let mut envs = vec![
        ("ASH_STANDALONE", "1".to_string()),
        ("ETHEREAL", "true".to_string()),
        ("ETHEREAL_VER", defs::VERSION_NAME.to_string()),
        ("ETHEREAL_VER_CODE", defs::VERSION_CODE.to_string()),
        ("ETHD", "true".to_string()),
        (
            "PATH",
            format!(
                "{}:{}",
                env_var("PATH").unwrap_or_default(),
                defs::BINARY_DIR.trim_end_matches('/')
            ),
        ),
    ];

    if let Some(id) = module_id {
        envs.push(("ETHEREAL_MODULE", id.to_string()));
    }

    envs
}

// because we use something like A-B update
// we need to update the module state after the boot_completed
// if someone(such as the module) install a module before the boot_completed
// then it may cause some problems, just forbid it
fn ensure_boot_completed() -> Result<()> {
    // ensure getprop sys.boot_completed == 1
    if getprop("sys.boot_completed").as_deref() != Some("1") {
        bail!("Android is Booting!");
    }
    Ok(())
}

fn mark_update() -> Result<()> {
    ensure_file_exists(concatcp!(defs::WORKING_DIR, defs::UPDATE_FILE_NAME))
}

fn mark_module_state(module: &str, flag_file: &str, create_or_delete: bool) -> Result<()> {
    module_config::validate_module_id(module)?;
    let module_state_file = Path::new(defs::MODULE_DIR).join(module).join(flag_file);
    if create_or_delete {
        ensure_file_exists(module_state_file)
    } else {
        if module_state_file.exists() {
            fs::remove_file(module_state_file)?;
        }
        Ok(())
    }
}
pub fn foreach_module(
    module_type: ModuleType,
    mut f: impl FnMut(&Path) -> Result<()>,
) -> Result<()> {
    let modules_dir = Path::new(match module_type {
        ModuleType::Updated => MODULE_UPDATE_DIR,
        _ => defs::MODULE_DIR,
    });
    let dir = std::fs::read_dir(modules_dir)?;
    for entry in dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            warn!("{} is not a directory, skip", path.display());
            continue;
        }

        if module_type == ModuleType::Active && path.join(defs::DISABLE_FILE_NAME).exists() {
            info!("{} is disabled, skip", path.display());
            continue;
        }
        if module_type == ModuleType::Active && path.join(defs::REMOVE_FILE_NAME).exists() {
            warn!("{} is removed, skip", path.display());
            continue;
        }

        f(&path)?;
    }

    Ok(())
}

fn foreach_active_module(f: impl FnMut(&Path) -> Result<()>) -> Result<()> {
    foreach_module(ModuleType::Active, f)
}

pub fn load_sepolicy_rule() -> Result<()> {
    foreach_active_module(|path| {
        let rule_file = path.join("sepolicy.rule");
        if !rule_file.exists() {
            return Ok(());
        }

        info!("load policy: {}", rule_file.display());
        let mut _sepol = get_policy_main(&[
            "magiskpolicy".to_string(),
            "--live".to_string(),
            "--apply".to_string(),
            rule_file.display().to_string(),
        ])?;

        Ok(())
    })?;

    Ok(())
}

pub fn exec_script<T: AsRef<Path>>(path: T, wait: bool) -> Result<()> {
    info!("exec {}", path.as_ref().display());

    let is_module_script = path.as_ref().starts_with(defs::MODULE_DIR);
    // Extract module_id from path if it matches /data/adb/modules/{id}/...
    let module_id = if is_module_script {
        path.as_ref()
            .strip_prefix(defs::MODULE_DIR)
            .ok()
            .and_then(|p| p.components().next())
            .and_then(|c| c.as_os_str().to_str())
            .map(ToString::to_string)
    } else {
        None
    };

    if is_module_script && module_id.is_none() {
        debug!(
            "Failed to extract module_id from script path '{}'. Script will run without ETHEREAL_MODULE environment variable.",
            path.as_ref().display()
        );
    }

    let mut command = &mut Command::new(assets::BUSYBOX_PATH);
    #[cfg(unix)]
    {
        command = command.process_group(0);
        command = unsafe {
            command.pre_exec(|| {
                // ignore the error?
                switch_cgroups();
                Ok(())
            })
        };
    }
    command = command
        .current_dir(path.as_ref().parent().unwrap())
        .arg("sh")
        .arg(path.as_ref())
        .envs(get_common_script_envs(module_id.as_deref()));

    let result = if wait {
        command
            .status()
            .map_err(anyhow::Error::from)
            .and_then(|status| ensure_script_success(path.as_ref(), status))
    } else {
        command.spawn().map(|_| ()).map_err(anyhow::Error::from)
    };
    result.map_err(|err| anyhow!("Failed to exec {}: {}", path.as_ref().display(), err))
}

fn ensure_script_success(path: &Path, status: ExitStatus) -> Result<()> {
    ensure!(
        status.success(),
        "Script {} exited unsuccessfully: {status}",
        path.display()
    );
    Ok(())
}

pub fn exec_stage_script(stage: &str, block: bool) -> Result<()> {
    let mut failures = Vec::new();
    foreach_active_module(|module| {
        let script_path = module.join(format!("{stage}.sh"));
        if !script_path.exists() {
            return Ok(());
        }

        if let Err(err) = exec_script(&script_path, block) {
            failures.push(format!("{}: {err}", script_path.display()));
        }
        Ok(())
    })?;
    ensure!(
        failures.is_empty(),
        "One or more {stage} scripts failed: {}",
        failures.join("; ")
    );
    Ok(())
}

pub fn exec_common_scripts(dir: &str, wait: bool) -> Result<()> {
    let script_dir = Path::new(defs::ADB_DIR).join(dir);
    if !script_dir.exists() {
        info!("{} not exists, skip", script_dir.display());
        return Ok(());
    }

    let dir = fs::read_dir(&script_dir)?;
    let mut failures = Vec::new();
    for entry in dir.flatten() {
        let path = entry.path();

        if !is_executable(&path) {
            warn!("{} is not executable, skip", path.display());
            continue;
        }

        if let Err(err) = exec_script(&path, wait) {
            failures.push(format!("{}: {err}", path.display()));
        }
    }

    ensure!(
        failures.is_empty(),
        "One or more scripts in {} failed: {}",
        script_dir.display(),
        failures.join("; ")
    );
    Ok(())
}

pub fn load_system_prop() -> Result<()> {
    foreach_active_module(|module| {
        let system_prop = module.join("system.prop");
        if !system_prop.exists() {
            return Ok(());
        }
        info!("load {} system.prop", module.display());

        crate::resetprop::load_system_prop_file(&system_prop)?;

        Ok(())
    })?;

    Ok(())
}

pub fn prune_modules() -> Result<()> {
    foreach_module(ModuleType::All, |module| {
        fs::remove_file(module.join(defs::UPDATE_FILE_NAME)).ok();
        if !module.join(defs::REMOVE_FILE_NAME).exists() {
            return Ok(());
        }

        info!("remove module: {}", module.display());

        // Execute metamodule's metauninstall.sh first
        let module_id = module.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Check if this is a metamodule
        let is_metamodule = read_module_prop(module)
            .map(|props| metamodule::is_metamodule(&props))
            .unwrap_or(false);

        if is_metamodule {
            info!("Removing metamodule symlink");
            if let Err(e) = metamodule::remove_symlink() {
                warn!("Failed to remove metamodule symlink: {e}");
            }
        } else if let Err(e) = metamodule::exec_metauninstall_script(module_id) {
            warn!("Failed to exec metamodule uninstall for {module_id}: {e}",);
        }

        // Then execute module's own uninstall.sh
        let uninstaller = module.join("uninstall.sh");
        if uninstaller.exists()
            && let Err(e) = exec_script(uninstaller, true)
        {
            warn!("Failed to exec uninstaller: {e}");
        }

        // Clear module configs before removing module directory
        if let Err(e) = module_config::clear_module_configs(module_id) {
            warn!("Failed to clear configs for {module_id}: {e}");
        }

        // Finally remove the module directory
        if let Err(e) = remove_dir_all(module) {
            warn!("Failed to remove {}: {e}", module.display());
        }

        Ok(())
    })?;

    // clean up metamodule record if none remain
    let has_remaining = std::fs::read_dir(defs::MODULE_DIR)?
        .filter_map(std::result::Result::ok)
        .any(|entry| entry.path().join("module.prop").exists());

    if !has_remaining {
        info!("no remaining modules.");
    }

    Ok(())
}

fn _install_module(zip: &str) -> Result<()> {
    ensure_boot_completed()?;

    // print banner
    println!(include_str!("./../../banner"));

    assets::ensure_binaries().with_context(|| "binary missing")?;

    // first check if workding dir is usable
    ensure_dir_exists(defs::WORKING_DIR).with_context(|| "Failed to create working dir")?;
    ensure_dir_exists(defs::BINARY_DIR).with_context(|| "Failed to create bin dir")?;

    let zip_path = PathBuf::from_str(zip)?;
    let zip_path = zip_path.canonicalize()?;
    let archive_info = inspect_module_archive(&zip_path)?;
    let module_id = archive_info.module_id.as_str();
    let module_prop = &archive_info.properties;
    info!("module prop: {:?}", module_prop);

    // Check if this module is a metamodule
    let is_metamodule = metamodule::is_metamodule(module_prop);

    // Check if it's safe to install regular module
    if !is_metamodule
        && archive_info.needs_mount
        && let Err(is_disabled) = metamodule::check_install_safety()
    {
        println!("\n❌ Installation Blocked");
        println!("┌────────────────────────────────");
        println!("│ A metamodule with custom installer is active");
        println!("│");
        if is_disabled {
            println!("│ Current state: Disabled");
            println!("│ Action required: Re-enable or uninstall it, then reboot");
        } else {
            println!("│ Current state: Pending changes");
            println!("│ Action required: Reboot to apply changes first");
        }
        println!("└─────────────────────────────────\n");
        bail!("Metamodule installation blocked");
    }

    let modules_dir = Path::new(defs::MODULE_DIR);
    let modules_update_dir = Path::new(defs::MODULE_UPDATE_DIR);
    create_private_dir(modules_dir)?;
    create_private_dir(modules_update_dir)?;

    if is_metamodule {
        info!("Installing metamodule: {module_id}");

        // Check if there's already a metamodule installed
        if metamodule::has_metamodule()
            && let Some(existing_path) = metamodule::get_metamodule_path()
        {
            let existing_id = read_module_prop(&existing_path)
                .ok()
                .and_then(|m| m.get("id").cloned())
                .unwrap_or_else(|| "unknown".to_string());

            if existing_id != module_id {
                println!("\n❌ Installation Failed");
                println!("┌────────────────────────────────");
                println!("│ A metamodule is already installed");
                println!("│   Current metamodule: {existing_id}");
                println!("│");
                println!("│ Only one metamodule can be active at a time.");
                println!("│");
                println!("│ To install this metamodule:");
                println!("│   1. Uninstall the current metamodule");
                println!("│   2. Reboot your device");
                println!("│   3. Install the new metamodule");
                println!("└─────────────────────────────────\n");
                bail!("Cannot install multiple metamodules");
            }
        }
    }

    let zip_for_installer = zip_path
        .to_str()
        .ok_or_else(|| anyhow!("Module archive path is not valid UTF-8"))?;
    let committed = prepare_staged_update(modules_dir, modules_update_dir, module_id, |staging| {
        extract_module_archive(&zip_path, staging)?;
        println!("- Running module installer");
        exec_install_script(zip_for_installer, is_metamodule, module_id, staging)?;

        let module_system_dir = staging.join("system");
        if module_system_dir.exists() {
            #[cfg(unix)]
            fs::set_permissions(&module_system_dir, fs::Permissions::from_mode(0o755))?;
            restorecon::restore_syscon(&module_system_dir)?;
        }
        Ok(())
    })?;
    info!("Committed module update: {}", committed.display());

    mark_update()?;
    Ok(())
}

pub fn install_module(zip: &str) -> Result<()> {
    _install_module(zip)
}

pub fn _uninstall_module(id: &str, update_dir: &str) -> Result<()> {
    module_config::validate_module_id(id)?;
    let dir = Path::new(update_dir);
    ensure!(dir.exists(), "No module installed");

    // iterate the modules_update dir, find the module to be removed
    let dir = fs::read_dir(dir)?;
    for entry in dir.flatten() {
        let path = entry.path();
        let module_prop = path.join("module.prop");
        if !module_prop.exists() {
            continue;
        }
        let content = fs::read(module_prop)?;
        let mut module_id: String = String::new();
        PropertiesIter::new_with_encoding(Cursor::new(content), encoding_rs::UTF_8).read_into(
            |k, v| {
                if k.eq("id") {
                    module_id = v;
                }
            },
        )?;
        if module_id.eq(id) {
            let remove_file = path.join(defs::REMOVE_FILE_NAME);
            fs::File::create(remove_file).with_context(|| "Failed to create remove file.")?;
            break;
        }
    }

    // santity check
    let target_module_path = format!("{update_dir}/{id}");
    let target_module = Path::new(&target_module_path);
    if target_module.exists() {
        let remove_file = target_module.join(defs::REMOVE_FILE_NAME);
        if !remove_file.exists() {
            fs::File::create(remove_file).with_context(|| "Failed to create remove file.")?;
        }
    }

    let _ = mark_module_state(id, defs::REMOVE_FILE_NAME, true);
    Ok(())
}
pub fn uninstall_module(id: &str) -> Result<()> {
    _uninstall_module(id, defs::MODULE_DIR)?;
    mark_update()?;
    Ok(())
}

pub fn _undo_uninstall_module(id: &str, update_dir: &str) -> Result<()> {
    module_config::validate_module_id(id)?;
    let dir = Path::new(update_dir);
    ensure!(dir.exists(), "No module installed");

    let mut found = false;
    for entry in fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let module_prop = path.join("module.prop");
        if !module_prop.exists() {
            continue;
        }

        let content = fs::read(&module_prop)?;
        let mut module_id = String::new();

        PropertiesIter::new_with_encoding(Cursor::new(content), encoding_rs::UTF_8).read_into(
            |k, v| {
                if k == "id" {
                    module_id = v;
                }
            },
        )?;
        if module_id == id {
            let remove_file = path.join(defs::REMOVE_FILE_NAME);
            fs::remove_file(remove_file).with_context(|| "Failed to remove removefile.")?;
            found = true;
            break;
        }
    }

    ensure!(found, "Module not found");

    let _ = mark_module_state(id, defs::REMOVE_FILE_NAME, false);
    Ok(())
}
pub fn undo_uninstall_module(id: &str) -> Result<()> {
    _undo_uninstall_module(id, defs::MODULE_DIR)?;
    mark_update()?;
    Ok(())
}

/// Read module.prop from the given module path and return as a HashMap
pub fn read_module_prop(module_path: &Path) -> Result<HashMap<String, String>> {
    let module_prop = module_path.join("module.prop");
    let metadata = fs::symlink_metadata(&module_prop)
        .with_context(|| format!("module.prop not found in {}", module_path.display()))?;
    ensure!(
        metadata.file_type().is_file() && !metadata.file_type().is_symlink(),
        "module.prop is not a regular file: {}",
        module_prop.display()
    );
    ensure!(
        metadata.len() <= MAX_MODULE_PROP_SIZE,
        "module.prop is too large: {} bytes (max: {MAX_MODULE_PROP_SIZE})",
        metadata.len()
    );

    let mut content = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(&module_prop)?
        .take(MAX_MODULE_PROP_SIZE + 1)
        .read_to_end(&mut content)
        .with_context(|| format!("Failed to read module.prop: {}", module_prop.display()))?;
    ensure!(
        content.len() as u64 <= MAX_MODULE_PROP_SIZE,
        "module.prop grew beyond the size limit while reading"
    );
    parse_module_properties(content)
        .with_context(|| format!("Failed to parse module.prop: {}", module_prop.display()))
}

pub fn run_action(id: &str) -> Result<()> {
    module_config::validate_module_id(id)?;
    let action_script_path = format!("/data/adb/modules/{}/action.sh", id);
    if Path::new(&action_script_path).exists() {
        let _ = exec_script(&action_script_path, true);
    } else {
        //if no action.sh, try to run lua action
        lua::run_lua(id, "action", false, true).map_err(|e| anyhow::anyhow!("{}", e))?;
    }
    Ok(())
}

fn _change_module_state(module_dir: &str, mid: &str, enable: bool) -> Result<()> {
    module_config::validate_module_id(mid)?;
    let src_module_path = format!("{module_dir}/{mid}");
    let src_module = Path::new(&src_module_path);
    ensure!(src_module.exists(), "module: {} not found!", mid);

    let disable_path = src_module.join(defs::DISABLE_FILE_NAME);
    if enable {
        if disable_path.exists() {
            fs::remove_file(&disable_path).with_context(|| {
                format!("Failed to remove disable file: {}", disable_path.display())
            })?;
        }
    } else {
        ensure_file_exists(disable_path)?;
    }

    let _ = mark_module_state(mid, defs::DISABLE_FILE_NAME, !enable);

    Ok(())
}

pub fn _enable_module(id: &str, update_dir: &Path) -> Result<()> {
    if let Some(module_dir_str) = update_dir.to_str() {
        _change_module_state(module_dir_str, id, true)
    } else {
        info!("Enable module failed: Invalid path");
        Err(anyhow::anyhow!("Invalid module directory"))
    }
}

pub fn enable_module(id: &str) -> Result<()> {
    let update_dir = Path::new(defs::MODULE_DIR);
    _enable_module(id, update_dir)?;
    Ok(())
}

pub fn _disable_module(id: &str, update_dir: &Path) -> Result<()> {
    if let Some(module_dir_str) = update_dir.to_str() {
        _change_module_state(module_dir_str, id, false)
    } else {
        info!("Disable module failed: Invalid path");
        Err(anyhow::anyhow!("Invalid module directory"))
    }
}

pub fn disable_module(id: &str) -> Result<()> {
    let module_dir = Path::new(defs::MODULE_DIR);
    _disable_module(id, module_dir)?;

    Ok(())
}

pub fn _disable_all_modules(dir: &str) -> Result<()> {
    let dir = fs::read_dir(dir)?;
    for entry in dir.flatten() {
        let path = entry.path();
        let disable_flag = path.join(defs::DISABLE_FILE_NAME);
        if let Err(e) = ensure_file_exists(disable_flag) {
            warn!("Failed to disable module: {}: {}", path.display(), e);
        }
    }
    Ok(())
}

pub fn disable_all_modules() -> Result<()> {
    // Skip disabling modules since boot completed
    if getprop("sys.boot_completed").as_deref() == Some("1") {
        info!("System boot completed, no need to disable all modules");
        return Ok(());
    }
    mark_update()?;
    _disable_all_modules(defs::MODULE_DIR)?;
    Ok(())
}

fn _list_modules(path: &str) -> Vec<HashMap<String, String>> {
    // Load all module configs once to minimize I/O overhead
    let all_configs = match module_config::get_all_module_configs() {
        Ok(configs) => configs,
        Err(e) => {
            warn!("Failed to load module configs: {e}");
            HashMap::new()
        }
    };

    // first check enabled modules
    let dir = fs::read_dir(path);
    let Ok(dir) = dir else {
        return Vec::new();
    };

    let mut modules: Vec<HashMap<String, String>> = Vec::new();

    for entry in dir.flatten() {
        let path = entry.path();
        info!("path: {}", path.display());
        let module_prop = path.join("module.prop");
        if !module_prop.exists() {
            continue;
        }
        let content = fs::read(&module_prop);
        let Ok(content) = content else {
            warn!("Failed to read file: {}", module_prop.display());
            continue;
        };
        let mut module_prop_map: HashMap<String, String> = HashMap::new();
        let encoding = encoding_rs::UTF_8;

        if PropertiesIter::new_with_encoding(Cursor::new(content), encoding)
            .read_into(|k, v| {
                module_prop_map.insert(k, v);
            })
            .is_err()
        {
            warn!("Failed to parse module.prop: {}", module_prop.display());
            continue;
        }

        if !module_prop_map.contains_key("id") || module_prop_map["id"].is_empty() {
            match entry.file_name().to_str() {
                Some(id) => {
                    info!("Use dir name as module id: {}", id);
                    module_prop_map.insert("id".to_owned(), id.to_owned());
                }
                _ => {
                    info!("Failed to get module id: {:?}", module_prop);
                    continue;
                }
            }
        }

        // Add enabled, update, remove flags
        let enabled = !path.join(defs::DISABLE_FILE_NAME).exists();
        let update = path.join(defs::UPDATE_FILE_NAME).exists();
        let remove = path.join(defs::REMOVE_FILE_NAME).exists();
        let web = path.join(defs::MODULE_WEB_DIR).exists();
        let id = module_prop_map.get("id").map(|s| s.as_str()).unwrap_or("");
        let id_lua_file = format!("{}.lua", id);
        let action = path.join(defs::MODULE_ACTION_SH).exists() || path.join(&id_lua_file).exists();

        module_prop_map.insert("enabled".to_owned(), enabled.to_string());
        module_prop_map.insert("update".to_owned(), update.to_string());
        module_prop_map.insert("remove".to_owned(), remove.to_string());
        module_prop_map.insert("web".to_owned(), web.to_string());
        module_prop_map.insert("action".to_owned(), action.to_string());

        // Apply module config overrides and extract managed features
        if let Some(module_id) = module_prop_map.get("id")
            && let Some(config) = all_configs.get(module_id.as_str())
        {
            // Apply override.description
            if let Some(desc) = config.get("override.description") {
                module_prop_map.insert("description".to_owned(), desc.clone());
            }
        }

        modules.push(module_prop_map);
    }

    modules
}

pub fn list_modules() -> Result<()> {
    let modules = _list_modules(defs::MODULE_DIR);
    println!("{}", serde_json::to_string_pretty(&modules)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;
    use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

    struct TestTree {
        root: PathBuf,
    }

    impl TestTree {
        fn new() -> Result<Self> {
            let counter = INTERNAL_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("ethd-module-test-{}-{counter}", std::process::id()));
            remove_path_no_follow(&root)?;
            fs::create_dir(&root)?;
            Ok(Self { root })
        }

        fn modules(&self) -> PathBuf {
            self.root.join("modules")
        }

        fn updates(&self) -> PathBuf {
            self.root.join("modules_update")
        }
    }

    impl Drop for TestTree {
        fn drop(&mut self) {
            let _ = remove_path_no_follow(&self.root);
        }
    }

    fn write_module(path: &Path, module_id: &str, payload: &str) -> Result<()> {
        fs::create_dir_all(path)?;
        fs::write(
            path.join("module.prop"),
            format!("id={module_id}\nname=Test module\nversion=1\n"),
        )?;
        fs::write(path.join("payload"), payload)?;
        Ok(())
    }

    fn handle_test_updates(modules: &Path, updates: &Path) -> Result<()> {
        let mut rename = |from: &Path, to: &Path| fs::rename(from, to);
        handle_updated_modules_with(modules, updates, &mut rename, |_, _| Ok(()))
    }

    fn valid_limits() -> ArchiveLimits {
        ArchiveLimits {
            archive_size: 1024 * 1024,
            module_prop_size: 1024,
            entries: 16,
            entry_size: 64 * 1024,
            total_size: 128 * 1024,
            compression_ratio: 1_000,
            path_size: 256,
            free_space_reserve: 0,
        }
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) -> Result<()> {
        let file = fs::File::create(path)?;
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        for (name, content) in entries {
            writer.start_file(*name, options)?;
            writer.write_all(content)?;
        }
        writer.finish()?;
        Ok(())
    }

    #[test]
    fn partial_extraction_failure_never_publishes_update() -> Result<()> {
        let tree = TestTree::new()?;
        let module_id = "test.module";
        write_module(&tree.modules().join(module_id), module_id, "old")?;

        let result =
            prepare_staged_update(&tree.modules(), &tree.updates(), module_id, |staging| {
                write_module(staging, module_id, "partial")?;
                bail!("injected extraction failure")
            });
        assert!(result.is_err());
        handle_test_updates(&tree.modules(), &tree.updates())?;
        assert_eq!(
            fs::read_to_string(tree.modules().join(module_id).join("payload"))?,
            "old"
        );
        assert!(!tree.updates().join(module_id).exists());
        Ok(())
    }

    #[test]
    fn installer_failure_never_publishes_update() -> Result<()> {
        let tree = TestTree::new()?;
        let module_id = "test.module";
        write_module(&tree.modules().join(module_id), module_id, "old")?;

        let result =
            prepare_staged_update(&tree.modules(), &tree.updates(), module_id, |staging| {
                write_module(staging, module_id, "installed-but-failed")?;
                bail!("injected installer failure")
            });
        assert!(result.is_err());
        handle_test_updates(&tree.modules(), &tree.updates())?;
        assert_eq!(
            fs::read_to_string(tree.modules().join(module_id).join("payload"))?,
            "old"
        );
        Ok(())
    }

    #[test]
    fn unmarked_pending_directory_is_removed_without_activation() -> Result<()> {
        let tree = TestTree::new()?;
        let module_id = "test.module";
        write_module(&tree.modules().join(module_id), module_id, "old")?;
        write_module(&tree.updates().join(module_id), module_id, "incomplete")?;

        handle_test_updates(&tree.modules(), &tree.updates())?;
        assert_eq!(
            fs::read_to_string(tree.modules().join(module_id).join("payload"))?,
            "old"
        );
        assert!(!tree.updates().join(module_id).exists());
        Ok(())
    }

    #[test]
    fn successful_update_is_published_then_activated_atomically() -> Result<()> {
        let tree = TestTree::new()?;
        let module_id = "test.module";
        let active = tree.modules().join(module_id);
        write_module(&active, module_id, "old")?;
        ensure_file_exists(active.join(defs::DISABLE_FILE_NAME))?;

        let pending =
            prepare_staged_update(&tree.modules(), &tree.updates(), module_id, |staging| {
                write_module(staging, module_id, "new")
            })?;
        assert!(pending.join(UPDATE_READY_FILE).is_file());
        assert_eq!(fs::read_to_string(active.join("payload"))?, "old");

        handle_test_updates(&tree.modules(), &tree.updates())?;
        assert_eq!(fs::read_to_string(active.join("payload"))?, "new");
        assert!(active.join(defs::DISABLE_FILE_NAME).is_file());
        assert!(!active.join(UPDATE_READY_FILE).exists());
        Ok(())
    }

    #[test]
    fn sync_failure_happens_before_ready_is_published() -> Result<()> {
        let tree = TestTree::new()?;
        let module_id = "test.module";
        let staging = tree.updates().join(".injected-stage");
        fs::create_dir_all(tree.modules())?;
        fs::create_dir_all(tree.updates())?;
        write_module(&staging, module_id, "new")?;

        let mut fail_sync = |_path: &Path| bail!("injected sync failure");
        let result = commit_staged_update_with_sync(
            &tree.modules(),
            &tree.updates(),
            module_id,
            &staging,
            &mut fail_sync,
        );
        assert!(result.is_err());
        assert!(staging.is_dir());
        assert!(!staging.join(UPDATE_READY_FILE).exists());
        assert!(!tree.updates().join(module_id).exists());
        Ok(())
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn recursive_sync_handles_nested_files_without_following_symlinks() -> Result<()> {
        use std::os::unix::fs::symlink;

        let tree = TestTree::new()?;
        let staging = tree.root.join("staging");
        let nested = staging.join("one/two");
        fs::create_dir_all(&nested)?;
        fs::write(nested.join("payload"), b"payload")?;
        let outside = tree.root.join("outside");
        fs::write(&outside, b"outside")?;
        symlink(&outside, staging.join("link"))?;

        sync_tree_no_follow(&staging)?;
        assert_eq!(fs::read(&outside)?, b"outside");

        let root_link = tree.root.join("staging-link");
        symlink(&staging, &root_link)?;
        assert!(sync_tree_no_follow(&root_link).is_err());
        Ok(())
    }

    #[test]
    fn extraction_space_reserve_and_production_limits_are_enforced() -> Result<()> {
        assert!(validate_available_space(1_024, 512, 512).is_ok());
        assert!(validate_available_space(1_023, 512, 512).is_err());
        assert_eq!(MAX_MODULE_ARCHIVE_SIZE, 512 * 1024 * 1024);
        assert_eq!(MAX_ARCHIVE_ENTRY_SIZE, 256 * 1024 * 1024);
        assert_eq!(MAX_ARCHIVE_TOTAL_SIZE, 512 * 1024 * 1024);
        assert_eq!(MAX_ARCHIVE_ENTRIES, 8_192);
        assert_eq!(MAX_ARCHIVE_COMPRESSION_RATIO, 200);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn waited_script_exit_status_is_not_ignored() -> Result<()> {
        let success = Command::new("sh").args(["-c", "exit 0"]).status()?;
        assert!(ensure_script_success(Path::new("success.sh"), success).is_ok());

        let failure = Command::new("sh").args(["-c", "exit 23"]).status()?;
        let error = ensure_script_success(Path::new("failure.sh"), failure)
            .expect_err("non-zero script exit must be reported");
        assert!(error.to_string().contains("failure.sh"));
        Ok(())
    }

    #[test]
    fn activation_rename_failure_restores_old_module() -> Result<()> {
        let tree = TestTree::new()?;
        let module_id = "test.module";
        let active = tree.modules().join(module_id);
        let pending = tree.updates().join(module_id);
        write_module(&active, module_id, "old")?;
        prepare_staged_update(&tree.modules(), &tree.updates(), module_id, |staging| {
            write_module(staging, module_id, "new")
        })?;

        let mut fail_once = true;
        let mut rename = |from: &Path, to: &Path| {
            if fail_once && from == pending && to == active {
                fail_once = false;
                Err(std::io::Error::new(
                    ErrorKind::PermissionDenied,
                    "injected activation failure",
                ))
            } else {
                fs::rename(from, to)
            }
        };
        let result =
            handle_updated_modules_with(&tree.modules(), &tree.updates(), &mut rename, |_, _| {
                Ok(())
            });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(active.join("payload"))?, "old");
        assert_eq!(fs::read_to_string(pending.join("payload"))?, "new");
        assert!(pending.join(UPDATE_READY_FILE).is_file());
        assert!(
            !tree
                .updates()
                .join(format!("{ACTIVE_BACKUP_PREFIX}{module_id}"))
                .exists()
        );
        Ok(())
    }

    #[test]
    fn interrupted_activation_with_ready_marker_restores_old_before_retry() -> Result<()> {
        let tree = TestTree::new()?;
        let module_id = "test.module";
        let active = tree.modules().join(module_id);
        let pending = tree.updates().join(module_id);
        let backup = tree
            .updates()
            .join(format!("{ACTIVE_BACKUP_PREFIX}{module_id}"));
        fs::create_dir_all(tree.updates())?;
        write_module(&backup, module_id, "old")?;
        write_module(&active, module_id, "new")?;
        fs::write(active.join(UPDATE_READY_FILE), module_id)?;

        let mut fail_retry = true;
        let mut rename = |from: &Path, to: &Path| {
            if fail_retry && from == pending && to == active {
                fail_retry = false;
                Err(std::io::Error::new(
                    ErrorKind::PermissionDenied,
                    "stop after recovery",
                ))
            } else {
                fs::rename(from, to)
            }
        };
        let result =
            handle_updated_modules_with(&tree.modules(), &tree.updates(), &mut rename, |_, _| {
                Ok(())
            });
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(active.join("payload"))?, "old");
        assert_eq!(fs::read_to_string(pending.join("payload"))?, "new");
        assert!(pending.join(UPDATE_READY_FILE).is_file());
        Ok(())
    }

    #[test]
    fn interrupted_first_install_returns_to_pending_and_can_retry() -> Result<()> {
        let tree = TestTree::new()?;
        let module_id = "test.module";
        let active = tree.modules().join(module_id);
        let pending = tree.updates().join(module_id);
        fs::create_dir_all(tree.updates())?;
        write_module(&active, module_id, "new")?;
        fs::write(active.join(UPDATE_READY_FILE), module_id)?;

        let mut fail_retry = true;
        let mut rename = |from: &Path, to: &Path| {
            if fail_retry && from == pending && to == active {
                fail_retry = false;
                Err(std::io::Error::new(
                    ErrorKind::PermissionDenied,
                    "stop after first-install recovery",
                ))
            } else {
                fs::rename(from, to)
            }
        };
        let result =
            handle_updated_modules_with(&tree.modules(), &tree.updates(), &mut rename, |_, _| {
                Ok(())
            });
        assert!(result.is_err());
        assert!(!active.exists());
        assert_eq!(fs::read_to_string(pending.join("payload"))?, "new");
        assert!(pending.join(UPDATE_READY_FILE).is_file());

        handle_test_updates(&tree.modules(), &tree.updates())?;
        assert_eq!(fs::read_to_string(active.join("payload"))?, "new");
        assert!(!active.join(UPDATE_READY_FILE).exists());
        Ok(())
    }

    #[test]
    fn archive_validation_and_extraction_accept_valid_module() -> Result<()> {
        let tree = TestTree::new()?;
        let zip = tree.root.join("module.zip");
        write_zip(
            &zip,
            &[
                ("module.prop", b"id=test.module\nname=Test\n"),
                ("system/bin/tool", b"payload"),
            ],
        )?;
        let info = inspect_module_archive_with_limits(&zip, valid_limits())?;
        assert_eq!(info.module_id, "test.module");
        assert!(info.needs_mount);

        let output = tree.root.join("output");
        fs::create_dir(&output)?;
        extract_module_archive_with_limits(&zip, &output, valid_limits())?;
        assert_eq!(
            fs::read_to_string(output.join("system/bin/tool"))?,
            "payload"
        );
        Ok(())
    }

    #[test]
    fn archive_rejects_unsafe_and_duplicate_paths() -> Result<()> {
        assert!(normalize_archive_path("../escape", 256).is_err());
        assert!(normalize_archive_path("system\\escape", 256).is_err());

        let tree = TestTree::new()?;
        let zip = tree.root.join("duplicate.zip");
        write_zip(
            &zip,
            &[
                ("module.prop", b"id=test.module\n"),
                ("payload1", b"one"),
                ("payload2", b"two"),
            ],
        )?;
        let mut bytes = fs::read(&zip)?;
        let mut replacements = 0;
        for offset in 0..=bytes.len() - b"payload2".len() {
            if &bytes[offset..offset + b"payload2".len()] == b"payload2" {
                bytes[offset..offset + b"payload2".len()].copy_from_slice(b"payload1");
                replacements += 1;
            }
        }
        assert_eq!(
            replacements, 2,
            "local and central ZIP names must be patched"
        );
        fs::write(&zip, bytes)?;
        assert!(inspect_module_archive_with_limits(&zip, valid_limits()).is_err());
        Ok(())
    }

    #[test]
    fn archive_rejects_symlinks() -> Result<()> {
        let tree = TestTree::new()?;
        let zip = tree.root.join("symlink.zip");
        let file = fs::File::create(&zip)?;
        let mut writer = ZipWriter::new(file);
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(0o644);
        writer.start_file("module.prop", options)?;
        writer.write_all(b"id=test.module\n")?;
        writer.add_symlink("system/bin/link", "/outside", options)?;
        writer.finish()?;
        assert!(inspect_module_archive_with_limits(&zip, valid_limits()).is_err());
        Ok(())
    }

    #[test]
    fn archive_enforces_entry_count_and_size_limits() -> Result<()> {
        let tree = TestTree::new()?;
        let zip = tree.root.join("limits.zip");
        write_zip(
            &zip,
            &[
                ("module.prop", b"id=test.module\n"),
                ("one", b"12345678901234567890"),
                ("two", b"12345678901234567890"),
            ],
        )?;

        let mut limits = valid_limits();
        limits.entries = 2;
        assert!(inspect_module_archive_with_limits(&zip, limits).is_err());

        limits = valid_limits();
        limits.entry_size = 15;
        assert!(inspect_module_archive_with_limits(&zip, limits).is_err());

        limits = valid_limits();
        limits.total_size = 50;
        assert!(inspect_module_archive_with_limits(&zip, limits).is_err());
        Ok(())
    }

    #[test]
    fn archive_enforces_module_prop_and_compression_ratio_limits() -> Result<()> {
        let tree = TestTree::new()?;
        let zip = tree.root.join("ratio.zip");
        let zeros = vec![0u8; 8 * 1024];
        write_zip(
            &zip,
            &[
                ("module.prop", b"id=test.module\n"),
                ("zeros", zeros.as_slice()),
            ],
        )?;

        let mut limits = valid_limits();
        limits.module_prop_size = 8;
        assert!(inspect_module_archive_with_limits(&zip, limits).is_err());

        limits = valid_limits();
        limits.compression_ratio = 1;
        assert!(inspect_module_archive_with_limits(&zip, limits).is_err());
        Ok(())
    }
}
