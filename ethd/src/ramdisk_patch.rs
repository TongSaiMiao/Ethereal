//! Single-image ramdisk patching for GKI 1.0 boot and GKI 2.0 init_boot.
//!
//! GKI 1.0 uses an `rdinit=/ethereal-init` trampoline. GKI 2.0 keeps the OEM
//! `/init` file and injects the same loader as an extra PT_LOAD before its
//! original entry. Neither flow replaces `/init`, rewrites the kernel Image,
//! or patches `vendor_boot`.

use anyhow::{Context, Result, bail, ensure};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn persist_log(line: &str) {
    println!("{line}");
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open("ethereal-patch.log")
    {
        let _ = writeln!(f, "{line}");
    }

    #[cfg(target_os = "android")]
    {
        let _ = fs::create_dir_all("/data/adb/eth/log");
        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open("/data/adb/eth/log/patch.log")
        {
            let _ = writeln!(f, "{line}");
        }
    }
}

fn tool(name: &str) -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    for candidate in [
        exe_dir.join(format!("lib{name}.so")),
        exe_dir.join(name),
        cwd.join(name),
        cwd.join(format!("lib{name}.so")),
    ] {
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from(name)
}

fn run(bin: &Path, args: &[&str]) -> Result<std::process::Output> {
    let output = Command::new(bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("spawn {} {args:?}", bin.display()))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let out = String::from_utf8_lossy(&output.stdout);
        bail!(
            "{} {args:?} failed (status {:?})\n{out}{err}",
            bin.display(),
            output.status.code()
        );
    }
    Ok(output)
}

fn print_output(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        persist_log(stdout.trim_end());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        persist_log(stderr.trim_end());
    }
}

fn cpio_ok(ramtool: &Path, archive: &str, cmd: &str) -> bool {
    Command::new(ramtool)
        .args(["cpio", archive, cmd])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

const PATCH_STATE_ENTRY: &str = "ethereal.patch_state";
const PATCH_STATE_LOCAL: &str = "ethereal.patch_state.input";
const MAX_PATCH_STATE_BYTES: usize = 16 * 1024;
const MAX_PATCH_STATE_ENTRIES: usize = 128;
const ETHEREAL_RDINIT: &str = "rdinit=/ethereal-init";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PatchMode {
    Gki1Single,
    Gki2Single,
    Gki2Pair,
}

impl PatchMode {
    fn name(self) -> &'static str {
        match self {
            Self::Gki1Single => "gki1-single",
            Self::Gki2Single => "gki2-single",
            Self::Gki2Pair => "gki2-pair",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "gki1-single" => Ok(Self::Gki1Single),
            "gki2-single" => Ok(Self::Gki2Single),
            "gki2-pair" => Ok(Self::Gki2Pair),
            _ => bail!("unsupported Ethereal patch mode {value:?}"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PatchState {
    mode: PatchMode,
    entries: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RdinitState {
    None,
    Ethereal,
}

#[derive(Clone, Debug)]
pub struct PatchArgs {
    pub image: PathBuf,
    pub out: PathBuf,
    pub ethinit: Option<PathBuf>,
    pub ko: Option<PathBuf>,
    pub manager_uid: u32,
    pub manager_token_file: PathBuf,
}

#[derive(Clone, Debug)]
pub struct PairPatchArgs {
    pub init_boot: PathBuf,
    pub boot: PathBuf,
    pub out_init_boot: PathBuf,
    pub out_boot: PathBuf,
    pub ethinit: Option<PathBuf>,
    pub ko: Option<PathBuf>,
    pub manager_uid: u32,
    pub manager_token_file: PathBuf,
}

fn parse_rdinit_state(cmdline: &str) -> Result<RdinitState> {
    let values: Vec<_> = cmdline
        .split_whitespace()
        .filter(|token| token.starts_with("rdinit="))
        .collect();
    ensure!(
        values.len() <= 1,
        "image defines multiple rdinit= parameters; use an unmodified stock image"
    );
    match values.first().copied() {
        None => Ok(RdinitState::None),
        Some(ETHEREAL_RDINIT) => Ok(RdinitState::Ethereal),
        Some(value) => bail!("image already defines {value}; use an unmodified stock image"),
    }
}

fn read_rdinit_state() -> Result<RdinitState> {
    parse_rdinit_state(&fs::read_to_string("cmdline.txt").unwrap_or_default())
}

fn ensure_rdinit() -> Result<()> {
    let p = Path::new("cmdline.txt");
    let cur = fs::read_to_string(p).unwrap_or_default();
    if read_rdinit_state()? == RdinitState::Ethereal {
        return Ok(());
    }
    let next = if cur.trim().is_empty() {
        ETHEREAL_RDINIT.to_string()
    } else {
        format!("{} {ETHEREAL_RDINIT}", cur.trim())
    };
    let cap = fs::read_to_string("cmdline.cap")
        .context("read cmdline.cap")?
        .trim()
        .parse::<usize>()
        .context("parse cmdline.cap")?;
    ensure!(
        next.len() < cap,
        "boot cmdline with {ETHEREAL_RDINIT} is {} bytes, but the header allows at most {} bytes",
        next.len(),
        cap.saturating_sub(1)
    );
    fs::write(p, next).context("write cmdline.txt")?;
    Ok(())
}

fn ensure_no_rdinit() -> Result<()> {
    ensure!(
        read_rdinit_state()? == RdinitState::None,
        "image already defines {ETHEREAL_RDINIT}; use an unmodified stock image"
    );
    Ok(())
}

fn strip_rdinit() -> Result<()> {
    let p = Path::new("cmdline.txt");
    let cur = fs::read_to_string(p).unwrap_or_default();
    if parse_rdinit_state(&cur)? == RdinitState::None {
        return Ok(());
    }
    let next = cur
        .split_whitespace()
        .filter(|t| *t != ETHEREAL_RDINIT)
        .collect::<Vec<_>>()
        .join(" ");
    fs::write(p, next).context("write cmdline.txt while removing Ethereal rdinit")?;
    Ok(())
}

fn cpio(ramtool: &Path, args: &[&str]) -> Result<()> {
    let mut all = vec!["cpio", "ramdisk.cpio"];
    all.extend_from_slice(args);
    let out = run(ramtool, &all)?;
    print_output(&out);
    Ok(())
}

fn pack_loader(ramtool: &Path, ethinit: &Path) -> Result<()> {
    let dest = PathBuf::from("ethereal-init");
    fs::copy(ethinit, &dest)
        .with_context(|| format!("copy {} -> ethereal-init", ethinit.display()))?;
    cpio(ramtool, &["add 0755 ethereal-init ethereal-init"])?;
    persist_log("- packed /ethereal-init (rdinit trampoline; OEM /init not replaced)");
    Ok(())
}

fn pack_kos(ramtool: &Path, ko: &Option<PathBuf>) -> Result<Vec<String>> {
    let mut packed = Vec::new();
    if let Some(ko) = ko {
        cpio(
            ramtool,
            &[&format!("add 0755 ethereal.ko {}", ko.display())],
        )?;
        packed.push("ethereal.ko".to_string());
        persist_log(&format!(
            "- packed {} into ramdisk as ethereal.ko",
            ko.display()
        ));
    }
    for name in crate::bundle::bundled_ko_names() {
        let src = PathBuf::from(&name);
        if !src.exists() {
            continue;
        }
        cpio(ramtool, &[&format!("add 0755 {name} {}", src.display())])?;
        packed.push(name.clone());
        persist_log(&format!("- packed {name} into ramdisk"));
    }
    Ok(packed)
}

fn find_su() -> Option<PathBuf> {
    for n in ["su", "eth/su", "libsu.so"] {
        let p = PathBuf::from(n);
        if p.is_file() && p.metadata().map(|m| m.len() > 64).unwrap_or(false) {
            return Some(p);
        }
    }
    None
}

fn pack_su(ramtool: &Path) -> Result<bool> {
    let Some(su) = find_su() else {
        persist_log("- WARNING: su binary not staged; ramdisk will have no /eth/su");
        return Ok(false);
    };
    let path = su.display().to_string();
    // `/su` belongs to whoever got there first. Keep the image edit under an
    // Ethereal-owned name and let early userspace copy it into disposable RAM.
    cpio(ramtool, &[&format!("add 0755 ethereal-su {path}")])?;
    persist_log(&format!(
        "- packed {} as /ethereal-su (staged as /eth/su at boot)",
        path
    ));
    Ok(true)
}

fn state_entry_allowed(name: &str) -> bool {
    matches!(
        name,
        "ethereal-init"
            | "ethereal.manager_uid"
            | "ethereal.manager_token"
            | "ethereal.ko"
            | "ethereal-su"
            | "init.ethereal.bak"
    ) || (name.starts_with("ethereal.android")
        && name.ends_with(".ko")
        && name.len() <= 128
        && !name.contains('/')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')))
}

fn parse_patch_state(data: &[u8]) -> Result<PatchState> {
    ensure!(
        data.len() <= MAX_PATCH_STATE_BYTES,
        "Ethereal patch state exceeds {MAX_PATCH_STATE_BYTES} bytes"
    );
    let text = std::str::from_utf8(data).context("Ethereal patch state is not UTF-8")?;
    let mut mode = None;
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if index == 0 {
            ensure!(
                line == "format=2",
                "unsupported Ethereal patch state format"
            );
        } else if let Some(value) = line.strip_prefix("mode=") {
            ensure!(mode.is_none(), "duplicate mode in Ethereal patch state");
            mode = Some(PatchMode::parse(value)?);
        } else if let Some(name) = line.strip_prefix("entry=") {
            ensure!(
                entries.len() < MAX_PATCH_STATE_ENTRIES,
                "Ethereal patch state has too many entries"
            );
            ensure!(
                state_entry_allowed(name),
                "unsafe entry in Ethereal patch state: {name:?}"
            );
            ensure!(
                !entries.iter().any(|entry| entry == name),
                "duplicate entry in Ethereal patch state: {name}"
            );
            entries.push(name.to_string());
        } else {
            bail!("unknown line in Ethereal patch state: {line:?}");
        }
    }
    let mode = mode.context("Ethereal patch state has no mode")?;
    for required in [
        "ethereal-init",
        "ethereal.manager_uid",
        "ethereal.manager_token",
    ] {
        ensure!(
            entries.iter().any(|entry| entry == required),
            "Ethereal patch state is missing {required}"
        );
    }
    ensure!(
        (mode == PatchMode::Gki2Single) == entries.iter().any(|entry| entry == "init.ethereal.bak"),
        "init.ethereal.bak does not match the recorded patch mode"
    );
    Ok(PatchState { mode, entries })
}

fn encode_patch_state(state: &PatchState) -> Vec<u8> {
    let mut text = format!("format=2\nmode={}\n", state.mode.name());
    for entry in &state.entries {
        text.push_str("entry=");
        text.push_str(entry);
        text.push('\n');
    }
    text.into_bytes()
}

fn read_patch_state(ramtool: &Path) -> Result<Option<PatchState>> {
    if !cpio_ok(
        ramtool,
        "ramdisk.cpio",
        &format!("exists {PATCH_STATE_ENTRY}"),
    ) {
        return Ok(None);
    }

    let local = Path::new(PATCH_STATE_LOCAL);
    let _ = fs::remove_file(local);
    let result = (|| -> Result<PatchState> {
        cpio(
            ramtool,
            &[&format!("extract {PATCH_STATE_ENTRY} {PATCH_STATE_LOCAL}")],
        )?;
        let state_len = fs::metadata(local)
            .context("stat Ethereal patch state")?
            .len();
        ensure!(
            state_len <= MAX_PATCH_STATE_BYTES as u64,
            "Ethereal patch state exceeds {MAX_PATCH_STATE_BYTES} bytes"
        );
        let data = fs::read(local).context("read Ethereal patch state")?;
        parse_patch_state(&data)
    })();
    let _ = fs::remove_file(local);
    result.map(Some)
}

fn owned_ramdisk_entries() -> Vec<String> {
    let mut entries = vec![
        "ethereal-init".to_string(),
        "ethereal.manager_uid".to_string(),
        "ethereal.manager_token".to_string(),
        "ethereal.ko".to_string(),
        "ethereal-su".to_string(),
        "init.ethereal.bak".to_string(),
    ];
    entries.extend(crate::bundle::bundled_ko_names());
    entries
}

fn legacy_patch_state_from_entries<F>(rdinit: RdinitState, mut exists: F) -> Option<PatchState>
where
    F: FnMut(&str) -> bool,
{
    // v0.1.1 left no guest list, so every piece of its old outfit has to show up.
    let core = [
        "ethereal-init",
        "ethereal.manager_uid",
        "ethereal.manager_token",
    ];
    if !core.iter().all(|name| exists(name)) {
        return None;
    }

    // v0.1.1 predates both the ownership record and these two paths. Their
    // presence without a state is a partial/newer patch, not a legacy image.
    if exists("ethereal-su") || exists("init.ethereal.bak") {
        return None;
    }

    let mut modules = Vec::new();
    if exists("ethereal.ko") {
        modules.push("ethereal.ko".to_string());
    }
    for name in crate::bundle::bundled_ko_names() {
        if exists(&name) {
            modules.push(name);
        }
    }
    if modules.is_empty() {
        return None;
    }

    let mode = match rdinit {
        RdinitState::Ethereal => PatchMode::Gki1Single,
        RdinitState::None => PatchMode::Gki2Pair,
    };
    let mut entries = core
        .iter()
        .map(|name| (*name).to_string())
        .collect::<Vec<_>>();
    entries.extend(modules);
    Some(PatchState { mode, entries })
}

fn detect_legacy_patch_state(ramtool: &Path, rdinit: RdinitState) -> Option<PatchState> {
    legacy_patch_state_from_entries(rdinit, |name| {
        cpio_ok(ramtool, "ramdisk.cpio", &format!("exists {name}"))
    })
}

fn verify_patch_ownership(ramtool: &Path, mode: PatchMode) -> Result<Option<PatchState>> {
    let rdinit = read_rdinit_state()?;
    if let Some(state) = read_patch_state(ramtool)? {
        ensure!(
            state.mode == mode,
            "Ethereal patch mode does not match this operation"
        );
        for name in &state.entries {
            ensure!(
                cpio_ok(ramtool, "ramdisk.cpio", &format!("exists {name}")),
                "Ethereal-owned ramdisk entry is missing: {name}"
            );
        }
        ensure!(
            (mode == PatchMode::Gki1Single) == (rdinit == RdinitState::Ethereal),
            "Ethereal rdinit does not match the recorded patch mode"
        );
        return Ok(Some(state));
    }

    if let Some(state) = detect_legacy_patch_state(ramtool, rdinit) {
        ensure!(
            state.mode == mode,
            "legacy Ethereal patch mode does not match this operation"
        );
        persist_log("- migrating complete v0.1.1 ramdisk layout to ownership state format 2");
        return Ok(Some(state));
    }

    ensure!(
        rdinit == RdinitState::None,
        "image has Ethereal rdinit without an ownership state; use a stock image"
    );
    for name in owned_ramdisk_entries() {
        ensure!(
            !cpio_ok(ramtool, "ramdisk.cpio", &format!("exists {name}")),
            "ramdisk already contains {name} without an Ethereal ownership state; use a stock image"
        );
    }
    Ok(None)
}

fn pack_patch_state(ramtool: &Path, state: &PatchState) -> Result<()> {
    let local = Path::new(PATCH_STATE_LOCAL);
    fs::write(local, encode_patch_state(state)).context("write Ethereal patch state")?;
    let result = cpio(
        ramtool,
        &[&format!("add 0400 {PATCH_STATE_ENTRY} {PATCH_STATE_LOCAL}")],
    );
    let _ = fs::remove_file(local);
    result
}

fn clean_unpack_stale() {
    for stale in [
        "ramdisk.cpio",
        "ramdisk.fmt",
        "kernel",
        "init",
        "cmdline.txt",
        "cmdline.cap",
        "ethereal-init",
        "ethereal.manager_uid",
        "ethereal.manager_token",
        PATCH_STATE_LOCAL,
        "vendor.dtb",
        "vendor_meta.txt",
        "vendor_table.bin",
        "vendor_bootconfig.bin",
    ] {
        let _ = fs::remove_file(stale);
    }
    if let Ok(rd) = fs::read_dir(".") {
        for ent in rd.flatten() {
            let s = ent.file_name().to_string_lossy().into_owned();
            if s.starts_with("vendor_frag.") {
                let _ = fs::remove_file(ent.path());
            }
        }
    }
}

fn is_vendor() -> bool {
    Path::new("vendor_meta.txt").exists()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ImageLayout {
    VendorBoot,
    Gki1Boot,
    Gki2InitBoot,
    KernelOnlyBoot,
    EmptyBoot,
}

impl ImageLayout {
    fn label(self) -> &'static str {
        match self {
            Self::VendorBoot => "vendor_boot",
            Self::Gki1Boot => "boot (kernel + ramdisk)",
            Self::Gki2InitBoot => "init_boot (ramdisk only)",
            Self::KernelOnlyBoot => "boot (kernel only)",
            Self::EmptyBoot => "boot (no kernel or ramdisk)",
        }
    }
}

fn classify_image_layout(vendor: bool, kernel: bool, ramdisk: bool) -> ImageLayout {
    // Filenames lie surprisingly often. The unpacked structure gets the final word.
    if vendor {
        ImageLayout::VendorBoot
    } else {
        match (kernel, ramdisk) {
            (true, true) => ImageLayout::Gki1Boot,
            (false, true) => ImageLayout::Gki2InitBoot,
            (true, false) => ImageLayout::KernelOnlyBoot,
            (false, false) => ImageLayout::EmptyBoot,
        }
    }
}

fn pack_manager_credentials(ramtool: &Path, manager_uid: u32, token: &[u8]) -> Result<()> {
    ensure!(manager_uid > 0, "manager UID must be greater than zero");
    ensure!(
        token.len() == 32,
        "manager token file must contain exactly 32 bytes"
    );
    ensure!(
        token.iter().any(|byte| *byte != 0),
        "manager token must not be all zero"
    );

    let uid_source = Path::new("ethereal.manager_uid");
    let token_source = Path::new("ethereal.manager_token");
    fs::write(uid_source, format!("{manager_uid}\n"))?;
    cpio(
        ramtool,
        &["add 0400 ethereal.manager_uid ethereal.manager_uid"],
    )?;
    let token_pack = (|| -> Result<()> {
        let mut token_options = OpenOptions::new();
        token_options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            token_options.mode(0o600);
        }
        let mut token_output = token_options.open(token_source)?;
        token_output.write_all(token)?;
        token_output.sync_all()?;
        drop(token_output);
        cpio(
            ramtool,
            &["add 0400 ethereal.manager_token ethereal.manager_token"],
        )?;
        Ok(())
    })();
    let _ = fs::remove_file(token_source);
    token_pack?;
    persist_log("- manager credentials embedded");
    Ok(())
}

#[derive(Clone, Copy)]
enum RamdiskTarget {
    AutoSingle,
    Gki2PairInitBoot,
}

fn hook_first_stage_init(ramtool: &Path, ethinit: &Path) -> Result<()> {
    cpio(ramtool, &[&format!("hook-inits {}", ethinit.display())])?;
    persist_log("- hooked effective root /init entry (OEM bytes retained in backup)");
    Ok(())
}

fn restore_first_stage_init_if_present(ramtool: &Path) -> Result<bool> {
    if !cpio_ok(ramtool, "ramdisk.cpio", "exists init.ethereal.bak") {
        return Ok(false);
    }
    cpio(ramtool, &["restore-init-hook"])?;
    persist_log("- restored effective root /init from Ethereal backup");
    Ok(true)
}

fn patch_ramdisk_image(
    ramtool: &Path,
    ethinit: &Path,
    image: &Path,
    out: &Path,
    ko: &Option<PathBuf>,
    manager_uid: u32,
    manager_token: &[u8],
    target: RamdiskTarget,
) -> Result<()> {
    clean_unpack_stale();
    persist_log(&format!("- image:   {}", image.display()));
    let unpack = run(ramtool, &["unpack", &image.to_string_lossy()])?;
    print_output(&unpack);
    let vendor = is_vendor();
    let has_rd = Path::new("ramdisk.cpio").exists();
    let has_kernel = Path::new("kernel").exists();
    let layout = classify_image_layout(vendor, has_kernel, has_rd);
    persist_log(&format!("- kind:    {}", layout.label()));
    let mode = match (target, layout) {
        (_, ImageLayout::VendorBoot) => bail!(
            "vendor_boot is not a standalone Ethereal patch target; select init_boot (GKI 2.0) or boot (GKI 1.0)"
        ),
        (RamdiskTarget::AutoSingle, ImageLayout::KernelOnlyBoot) => {
            bail!("selected boot image is kernel-only; patch its matching init_boot image instead")
        }
        (RamdiskTarget::AutoSingle, ImageLayout::EmptyBoot) => {
            bail!("boot-patch requires a GKI 1.0 boot or GKI 2.0 init_boot image")
        }
        (RamdiskTarget::AutoSingle, ImageLayout::Gki1Boot) => PatchMode::Gki1Single,
        (RamdiskTarget::AutoSingle, ImageLayout::Gki2InitBoot) => PatchMode::Gki2Single,
        (RamdiskTarget::Gki2PairInitBoot, ImageLayout::Gki2InitBoot) => PatchMode::Gki2Pair,
        (RamdiskTarget::Gki2PairInitBoot, _) => {
            bail!("--init-boot must be a GKI 2.0 init_boot image containing ramdisk and no kernel")
        }
    };
    let previous_state = verify_patch_ownership(ramtool, mode)?;

    match mode {
        PatchMode::Gki1Single => {
            restore_first_stage_init_if_present(ramtool)?;
            ensure_rdinit()?;
            persist_log("- mode:    GKI 1.0 single boot; ramdisk payload + rdinit=");
        }
        PatchMode::Gki2Single => {
            ensure_no_rdinit()?;
            strip_rdinit()?;
            hook_first_stage_init(ramtool, ethinit)?;
            persist_log("- mode:    GKI 2.0 single init_boot; root /init entry hook");
        }
        PatchMode::Gki2Pair => {
            // Remove the obsolete marker from images produced by the old,
            // invalid single-init_boot patch flow. GKI 2.0 takes cmdline from boot.
            ensure_no_rdinit()?;
            strip_rdinit()?;
            restore_first_stage_init_if_present(ramtool)?;
            persist_log("- mode:    GKI 2.0 init_boot payload only (cmdline stays in boot)");
        }
    }
    if let Some(state) = previous_state {
        for name in state.entries {
            if name != "init.ethereal.bak" {
                rm_if_present(ramtool, &name)?;
            }
        }
    }
    pack_loader(ramtool, ethinit)?;
    pack_manager_credentials(ramtool, manager_uid, manager_token)?;
    let packed_kos = pack_kos(ramtool, ko)?;
    if packed_kos.is_empty() {
        persist_log("- WARNING: ethereal.ko not bundled; LKM will not load");
    }
    let packed_su = pack_su(ramtool)?;
    let mut owned_entries = vec![
        "ethereal-init".to_string(),
        "ethereal.manager_uid".to_string(),
        "ethereal.manager_token".to_string(),
    ];
    owned_entries.extend(packed_kos);
    if packed_su {
        owned_entries.push("ethereal-su".to_string());
    }
    if mode == PatchMode::Gki2Single {
        owned_entries.push("init.ethereal.bak".to_string());
    }
    pack_patch_state(
        ramtool,
        &PatchState {
            mode,
            entries: owned_entries,
        },
    )?;
    let repack = run(
        ramtool,
        &["repack", &image.to_string_lossy(), &out.to_string_lossy()],
    )?;
    print_output(&repack);
    Ok(())
}

fn read_manager_token(path: &Path) -> Result<Vec<u8>> {
    let manager_token =
        fs::read(path).with_context(|| format!("read manager token file {}", path.display()))?;
    ensure!(
        manager_token.len() == 32,
        "manager token file must contain exactly 32 bytes"
    );
    ensure!(
        manager_token.iter().any(|byte| *byte != 0),
        "manager token must not be all zero"
    );
    Ok(manager_token)
}

fn prepare_patch_tools(ethinit: Option<PathBuf>) -> Result<(PathBuf, PathBuf)> {
    crate::bundle::extract_into(Path::new("."))?;
    let ramtool = tool("ramtool");
    ensure!(
        ramtool.exists(),
        "ramtool not found at {}",
        ramtool.display()
    );
    let ethinit = ethinit.unwrap_or_else(|| tool("ethinit"));
    ensure!(
        ethinit.exists(),
        "ethinit stub not found at {}",
        ethinit.display()
    );
    persist_log(&format!("- ramtool: {}", ramtool.display()));
    persist_log(&format!("- ethinit: {}", ethinit.display()));
    Ok((ramtool, ethinit))
}

pub fn boot_patch(args: PatchArgs) -> Result<()> {
    let PatchArgs {
        image,
        out,
        ethinit,
        ko,
        manager_uid,
        manager_token_file,
    } = args;

    stage_single_output(&image, &out, "patch", |staged| {
        let manager_token = read_manager_token(&manager_token_file)?;
        let (ramtool, ethinit) = prepare_patch_tools(ethinit)?;
        // A generic ethereal.ko is accepted only through an explicit --ko.
        // Silently taking one from cwd previously made a stale 6.1 module look
        // like a valid fallback for unknown or ambiguous kernels.
        persist_log("- mode:    auto-detect single boot/init_boot image");
        patch_ramdisk_image(
            &ramtool,
            &ethinit,
            &image,
            staged,
            &ko,
            manager_uid,
            &manager_token,
            RamdiskTarget::AutoSingle,
        )
    })?;

    persist_log(&format!("- wrote {}", out.display()));
    persist_log("- kernel image untouched; OEM /init file kept");
    Ok(())
}

fn path_identity(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let Some(name) = absolute.file_name() else {
        return absolute;
    };
    absolute
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .map(|parent| parent.join(name))
        .unwrap_or(absolute)
}

fn output_parent(out: &Path) -> &Path {
    out.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn normalized_output_path(out: &Path) -> Result<PathBuf> {
    let name = out.file_name().context("output path has no file name")?;
    let parent = output_parent(out)
        .canonicalize()
        .with_context(|| format!("resolve output directory {}", output_parent(out).display()))?;
    Ok(parent.join(name))
}

fn staged_output(out: &Path, label: &str, nonce: u64) -> Result<PathBuf> {
    let name = out
        .file_name()
        .and_then(|name| name.to_str())
        .context("output path has no file name")?;
    let parent = output_parent(out);
    Ok(parent.join(format!(
        ".{name}.ethereal-{}-{label}-{nonce:016x}.tmp",
        std::process::id(),
    )))
}

fn root_staging_parent_is_safe(mode: u32, owner_uid: u32, effective_uid: u32) -> bool {
    effective_uid != 0 || (owner_uid == 0 && mode & 0o022 == 0)
}

fn cleanup_stale_staged_outputs(parent: &Path, output_name: &str) -> Result<()> {
    let prefix = format!(".{output_name}.ethereal-");
    let current_pid = std::process::id();
    for entry in fs::read_dir(parent)
        .with_context(|| format!("scan output directory {}", parent.display()))?
    {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        if !rest.ends_with(".tmp") {
            continue;
        }
        let Some(pid) = rest
            .split_once('-')
            .and_then(|(pid, _)| pid.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == current_pid || Path::new("/proc").join(pid.to_string()).exists() {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_file() || file_type.is_symlink() {
            fs::remove_file(entry.path()).with_context(|| {
                format!("remove stale staged output {}", entry.path().display())
            })?;
        }
    }
    Ok(())
}

fn reserve_staged_output(out: &Path, label: &str) -> Result<PathBuf> {
    let parent = output_parent(out);
    let parent_metadata = fs::metadata(parent)
        .with_context(|| format!("stat output directory {}", parent.display()))?;
    ensure!(
        parent_metadata.is_dir(),
        "output parent is not a directory: {}",
        parent.display()
    );
    let effective_uid = unsafe { libc::geteuid() };
    ensure!(
        root_staging_parent_is_safe(
            parent_metadata.permissions().mode(),
            parent_metadata.uid(),
            effective_uid,
        ),
        "refusing root image staging outside a root-owned private directory: {}",
        parent.display()
    );
    let output_name = out
        .file_name()
        .and_then(|name| name.to_str())
        .context("output path has no file name")?;
    cleanup_stale_staged_outputs(parent, output_name)?;

    for _ in 0..32 {
        let mut nonce = [0u8; 8];
        OpenOptions::new()
            .read(true)
            .open("/dev/urandom")
            .context("open /dev/urandom for staging nonce")?
            .read_exact(&mut nonce)
            .context("read staging nonce")?;
        let staged = staged_output(out, label, u64::from_ne_bytes(nonce))?;
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&staged)
        {
            Ok(_) => return Ok(staged),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("reserve staging file {}", staged.display()));
            }
        }
    }
    bail!(
        "could not reserve a unique staging file beside {}",
        out.display()
    )
}

fn staged_regular_file_len(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("stat staged output {}", path.display()))?;
    ensure!(
        metadata.file_type().is_file(),
        "staged output is no longer a regular file: {}",
        path.display()
    );
    Ok(metadata.len())
}

fn remove_if_present(path: &Path) {
    if fs::symlink_metadata(path).is_ok() {
        let _ = fs::remove_file(path);
    }
}

fn ensure_path_absent(path: &Path, message: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => bail!("{message}"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("inspect output path {}", path.display())),
    }
}

fn ensure_pair_paths_safe(
    init_input: &Path,
    boot_input: &Path,
    init_output: &Path,
    boot_output: &Path,
    staged_init: &Path,
    staged_boot: &Path,
) -> Result<()> {
    let init_input = path_identity(init_input);
    let boot_input = path_identity(boot_input);
    let init_output = path_identity(init_output);
    let boot_output = path_identity(boot_output);
    let staged_init = path_identity(staged_init);
    let staged_boot = path_identity(staged_boot);

    ensure!(
        init_input != boot_input,
        "init_boot and boot inputs must be different files"
    );
    ensure!(
        init_output != boot_output,
        "paired output paths must be different files"
    );
    ensure!(
        init_output != init_input
            && init_output != boot_input
            && boot_output != init_input
            && boot_output != boot_input,
        "paired outputs must not overwrite either input image"
    );
    ensure!(
        staged_init != staged_boot,
        "paired staging paths must be different files"
    );
    for staged in [&staged_init, &staged_boot] {
        ensure!(
            staged != &init_input
                && staged != &boot_input
                && staged != &init_output
                && staged != &boot_output,
            "paired staging path aliases an input or output image"
        );
    }
    Ok(())
}

fn stage_single_output<F>(input: &Path, out: &Path, label: &str, build: F) -> Result<()>
where
    F: FnOnce(&Path) -> Result<()>,
{
    // A patched image is either complete or absent. Half-written boot files are bad souvenirs.
    let input_identity = path_identity(input);
    let resolved_out = normalized_output_path(out)?;
    let output_identity = path_identity(&resolved_out);
    ensure!(
        input_identity != output_identity,
        "output must not overwrite the input image"
    );
    ensure_path_absent(&resolved_out, "output path must not already exist")?;

    let staged = reserve_staged_output(&resolved_out, label)?;
    if path_identity(&staged) == input_identity {
        remove_if_present(&staged);
        bail!("staging path aliases the input image");
    }

    let result = (|| -> Result<()> {
        build(&staged)?;
        let staged_len = staged_regular_file_len(&staged)?;
        ensure!(staged_len > 0, "patched output is empty");
        ensure_path_absent(&resolved_out, "output path appeared while patching")?;
        fs::rename(&staged, &resolved_out)
            .with_context(|| format!("publish patched output {}", resolved_out.display()))?;
        Ok(())
    })();

    if result.is_err() {
        remove_if_present(&staged);
    }
    result
}

pub fn boot_patch_pair(args: PairPatchArgs) -> Result<()> {
    let out_init_boot = normalized_output_path(&args.out_init_boot)?;
    let out_boot = normalized_output_path(&args.out_boot)?;
    ensure_path_absent(&out_init_boot, "paired output paths must not already exist")?;
    ensure_path_absent(&out_boot, "paired output paths must not already exist")?;
    let staged_init = reserve_staged_output(&out_init_boot, "init-boot")?;
    let staged_boot = match reserve_staged_output(&out_boot, "boot") {
        Ok(path) => path,
        Err(error) => {
            remove_if_present(&staged_init);
            return Err(error);
        }
    };
    if let Err(error) = ensure_pair_paths_safe(
        &args.init_boot,
        &args.boot,
        &out_init_boot,
        &out_boot,
        &staged_init,
        &staged_boot,
    ) {
        remove_if_present(&staged_init);
        remove_if_present(&staged_boot);
        return Err(error);
    }
    let mut published_init = false;
    let mut published_boot = false;
    let result = (|| -> Result<()> {
        let manager_token = read_manager_token(&args.manager_token_file)?;
        let (ramtool, ethinit) = prepare_patch_tools(args.ethinit)?;
        let ko = args.ko;

        persist_log("- mode:    GKI 2.0 paired init_boot payload + boot cmdline");
        patch_ramdisk_image(
            &ramtool,
            &ethinit,
            &args.init_boot,
            &staged_init,
            &ko,
            args.manager_uid,
            &manager_token,
            RamdiskTarget::Gki2PairInitBoot,
        )?;

        let boot_patch = run(
            &ramtool,
            &[
                "patch-gki2-boot-cmdline",
                &args.boot.to_string_lossy(),
                &staged_boot.to_string_lossy(),
            ],
        )?;
        print_output(&boot_patch);

        let staged_init_len = staged_regular_file_len(&staged_init)?;
        let staged_boot_len = staged_regular_file_len(&staged_boot)?;
        let original_boot_len = fs::metadata(&args.boot)?.len();
        ensure!(staged_init_len > 0, "patched init_boot output is empty");
        ensure!(staged_boot_len > 0, "patched boot output is empty");
        ensure!(
            staged_boot_len == original_boot_len,
            "patched boot length changed from {original_boot_len} to {staged_boot_len}"
        );

        ensure_path_absent(
            &out_init_boot,
            "paired init_boot output appeared while patching",
        )?;
        ensure_path_absent(&out_boot, "paired boot output appeared while patching")?;
        fs::rename(&staged_init, &out_init_boot).with_context(|| {
            format!(
                "publish paired init_boot output {}",
                out_init_boot.display()
            )
        })?;
        published_init = true;
        fs::rename(&staged_boot, &out_boot)
            .with_context(|| format!("publish paired boot output {}", out_boot.display()))?;
        published_boot = true;
        Ok(())
    })();

    if let Err(error) = result {
        remove_if_present(&staged_init);
        remove_if_present(&staged_boot);
        if published_init {
            remove_if_present(&out_init_boot);
        }
        if published_boot {
            remove_if_present(&out_boot);
        }
        return Err(error);
    }

    persist_log(&format!("- wrote {}", args.out_init_boot.display()));
    persist_log(&format!("- wrote {}", args.out_boot.display()));
    persist_log("- paired patch complete; both images are required");
    Ok(())
}

fn rm_if_present(ramtool: &Path, name: &str) -> Result<()> {
    if cpio_ok(ramtool, "ramdisk.cpio", &format!("exists {name}")) {
        cpio(ramtool, &[&format!("rm {name}")])?;
    }
    Ok(())
}

fn ensure_single_unpatch_mode(mode: PatchMode) -> Result<()> {
    // The matching boot still points at our rdinit; peeling off only init_boot leaves half a patch.
    ensure!(
        mode != PatchMode::Gki2Pair,
        "a GKI 2.0 paired init_boot cannot be unpatched alone; restore its matching boot and init_boot together"
    );
    Ok(())
}

pub fn boot_unpatch(image: PathBuf, out: PathBuf) -> Result<()> {
    stage_single_output(&image, &out, "unpatch", |staged| {
        let ramtool = tool("ramtool");
        clean_unpack_stale();
        let unpack = run(&ramtool, &["unpack", &image.to_string_lossy()])?;
        print_output(&unpack);
        let vendor = is_vendor();
        let has_kernel = Path::new("kernel").exists();
        let rdinit = read_rdinit_state()?;
        if Path::new("ramdisk.cpio").exists() {
            let state = match read_patch_state(&ramtool)? {
                Some(state) => state,
                None => detect_legacy_patch_state(&ramtool, rdinit).context(
                    "image has no supported Ethereal ownership state; refusing to remove ramdisk files",
                )?,
            };
            ensure_single_unpatch_mode(state.mode)?;
            ensure!(
                !vendor
                    && ((state.mode == PatchMode::Gki1Single && has_kernel)
                        || (state.mode != PatchMode::Gki1Single && !has_kernel)),
                "ramdisk layout does not match the Ethereal patch state"
            );
            ensure!(
                (state.mode == PatchMode::Gki1Single) == (rdinit == RdinitState::Ethereal),
                "Ethereal rdinit does not match the recorded patch mode"
            );
            for name in &state.entries {
                ensure!(
                    cpio_ok(&ramtool, "ramdisk.cpio", &format!("exists {name}")),
                    "Ethereal-owned ramdisk entry is missing: {name}"
                );
            }
            strip_rdinit()?;
            let restored_elf_hook = restore_first_stage_init_if_present(&ramtool)?;
            for name in state.entries {
                rm_if_present(&ramtool, &name)?;
            }
            rm_if_present(&ramtool, PATCH_STATE_ENTRY)?;
            if vendor {
                // Keep original vendor fragments unless an init hook was restored.
                if !restored_elf_hook {
                    let _ = fs::remove_file("ramdisk.cpio");
                }
            }
        } else {
            ensure!(
                !vendor && has_kernel && rdinit == RdinitState::Ethereal,
                "image has neither a complete Ethereal ramdisk patch nor an Ethereal rdinit"
            );
            bail!(
                "a GKI 2.0 paired boot cannot be unpatched alone; restore its matching boot and init_boot together"
            );
        }
        let staged_s = staged.to_string_lossy().into_owned();
        let repack = run(&ramtool, &["repack", &image.to_string_lossy(), &staged_s])?;
        print_output(&repack);
        Ok(())
    })?;
    persist_log(&format!("- restored ramdisk/cmdline to {}", out.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ethereal-ramdisk-patch-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn single_image_layout_classification_is_structural() {
        assert_eq!(
            classify_image_layout(false, true, true),
            ImageLayout::Gki1Boot
        );
        assert_eq!(
            classify_image_layout(false, false, true),
            ImageLayout::Gki2InitBoot
        );
        assert_eq!(
            classify_image_layout(false, true, false),
            ImageLayout::KernelOnlyBoot
        );
        assert_eq!(
            classify_image_layout(false, false, false),
            ImageLayout::EmptyBoot
        );
        for kernel in [false, true] {
            for ramdisk in [false, true] {
                assert_eq!(
                    classify_image_layout(true, kernel, ramdisk),
                    ImageLayout::VendorBoot
                );
            }
        }
    }

    #[test]
    fn patch_state_round_trip_keeps_the_recorded_module_set() {
        let state = PatchState {
            mode: PatchMode::Gki2Single,
            entries: vec![
                "ethereal-init".to_string(),
                "ethereal.manager_uid".to_string(),
                "ethereal.manager_token".to_string(),
                "ethereal.android17-6.18.ko".to_string(),
                "ethereal-su".to_string(),
                "init.ethereal.bak".to_string(),
            ],
        };

        assert_eq!(
            parse_patch_state(&encode_patch_state(&state)).unwrap(),
            state
        );
    }

    #[test]
    fn patch_state_rejects_unsafe_or_inconsistent_entries() {
        for state in [
            "format=2\nmode=gki1-single\nentry=ethereal-init\nentry=ethereal.manager_uid\nentry=ethereal.manager_token\nentry=../ethereal.ko\n",
            "format=2\nmode=gki2-single\nentry=ethereal-init\nentry=ethereal.manager_uid\nentry=ethereal.manager_token\n",
            "format=2\nmode=gki2-pair\nentry=ethereal-init\nentry=ethereal.manager_uid\nentry=ethereal.manager_token\nentry=init.ethereal.bak\n",
            "format=2\nmode=gki1-single\nentry=ethereal-init\nentry=ethereal.manager_uid\nentry=ethereal.manager_uid\nentry=ethereal.manager_token\n",
        ] {
            assert!(parse_patch_state(state.as_bytes()).is_err(), "{state}");
        }

        assert!(parse_patch_state(&vec![b'x'; MAX_PATCH_STATE_BYTES + 1]).is_err());

        let mut too_many = String::from(
            "format=2\nmode=gki1-single\nentry=ethereal-init\nentry=ethereal.manager_uid\nentry=ethereal.manager_token\n",
        );
        for index in 0..MAX_PATCH_STATE_ENTRIES {
            too_many.push_str(&format!("entry=ethereal.android{index}-6.1.ko\n"));
        }
        assert!(parse_patch_state(too_many.as_bytes()).is_err());
    }

    #[test]
    fn rdinit_parser_rejects_foreign_and_duplicate_values() {
        assert_eq!(parse_rdinit_state("quiet").unwrap(), RdinitState::None);
        assert_eq!(
            parse_rdinit_state("quiet rdinit=/ethereal-init").unwrap(),
            RdinitState::Ethereal
        );
        assert!(parse_rdinit_state("rdinit=/foreign-init").is_err());
        assert!(parse_rdinit_state("rdinit=/ethereal-init rdinit=/foreign-init").is_err());
        assert!(parse_rdinit_state("rdinit=/ethereal-init rdinit=/ethereal-init").is_err());
    }

    #[test]
    fn complete_v011_layout_is_migrated_without_claiming_shared_su_paths() {
        use std::collections::HashSet;

        let names = HashSet::from([
            "ethereal-init",
            "ethereal.manager_uid",
            "ethereal.manager_token",
            "ethereal.ko",
            "su",
            "eth/su",
            "debug_ramdisk/su",
        ]);
        let gki1 =
            legacy_patch_state_from_entries(RdinitState::Ethereal, |name| names.contains(name))
                .unwrap();
        assert_eq!(gki1.mode, PatchMode::Gki1Single);
        assert_eq!(
            gki1.entries,
            vec![
                "ethereal-init",
                "ethereal.manager_uid",
                "ethereal.manager_token",
                "ethereal.ko",
            ]
        );
        assert!(
            !gki1
                .entries
                .iter()
                .any(|name| name == "su" || name.contains('/'))
        );

        let pair = legacy_patch_state_from_entries(RdinitState::None, |name| names.contains(name))
            .unwrap();
        assert_eq!(pair.mode, PatchMode::Gki2Pair);

        let stock = HashSet::from(["init", "su"]);
        assert!(
            legacy_patch_state_from_entries(RdinitState::None, |name| stock.contains(name))
                .is_none()
        );
        let partial = HashSet::from(["ethereal-init", "ethereal.ko"]);
        assert!(
            legacy_patch_state_from_entries(RdinitState::Ethereal, |name| {
                partial.contains(name)
            })
            .is_none()
        );
        let core_without_module = HashSet::from([
            "ethereal-init",
            "ethereal.manager_uid",
            "ethereal.manager_token",
        ]);
        assert!(
            legacy_patch_state_from_entries(RdinitState::Ethereal, |name| {
                core_without_module.contains(name)
            })
            .is_none()
        );
        let mut state_less_newer = names.clone();
        state_less_newer.insert("ethereal-su");
        assert!(
            legacy_patch_state_from_entries(RdinitState::Ethereal, |name| {
                state_less_newer.contains(name)
            })
            .is_none()
        );
    }

    #[test]
    fn paired_init_boot_requires_paired_unpatch() {
        assert!(ensure_single_unpatch_mode(PatchMode::Gki2Pair).is_err());
        ensure_single_unpatch_mode(PatchMode::Gki1Single).unwrap();
        ensure_single_unpatch_mode(PatchMode::Gki2Single).unwrap();
    }

    #[test]
    fn paired_staging_paths_cannot_alias_inputs_or_outputs() {
        let dir = unique_dir("pair-paths");
        fs::create_dir_all(&dir).unwrap();
        let init_input = dir.join("init_boot.img");
        let boot_input = dir.join("boot.img");
        let init_output = dir.join("out-init_boot.img");
        let boot_output = dir.join("out-boot.img");
        fs::write(&init_input, b"init").unwrap();
        fs::write(&boot_input, b"boot").unwrap();
        let staged_init = staged_output(&init_output, "init-boot", 1).unwrap();
        let staged_boot = staged_output(&boot_output, "boot", 2).unwrap();

        ensure_pair_paths_safe(
            &init_input,
            &boot_input,
            &init_output,
            &boot_output,
            &staged_init,
            &staged_boot,
        )
        .unwrap();
        assert!(
            ensure_pair_paths_safe(
                &init_input,
                &boot_input,
                &init_output,
                &boot_output,
                &boot_input,
                &staged_boot,
            )
            .is_err()
        );
        assert!(
            ensure_pair_paths_safe(
                &init_input,
                &boot_input,
                &init_output,
                &boot_output,
                &staged_init,
                &staged_init,
            )
            .is_err()
        );

        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn dangling_output_symlink_is_not_treated_as_absent() {
        use std::os::unix::fs::symlink;

        let dir = unique_dir("dangling-output");
        fs::create_dir_all(&dir).unwrap();
        let output = dir.join("output.img");
        symlink(dir.join("missing-target"), &output).unwrap();
        assert!(ensure_path_absent(&output, "output exists").is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn staged_single_output_rejects_alias_and_cleans_failures() {
        let dir = unique_dir("staging");
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.img");
        let output = dir.join("output.img");
        fs::write(&input, b"stock").unwrap();

        let alias_error = stage_single_output(&input, &input, "test", |_| Ok(())).unwrap_err();
        assert!(alias_error.to_string().contains("must not overwrite"));
        assert_eq!(fs::read(&input).unwrap(), b"stock");

        let build_error = stage_single_output(&input, &output, "test", |pending| {
            fs::write(pending, b"partial")?;
            Err(anyhow::anyhow!("forced build failure"))
        })
        .unwrap_err();
        assert!(build_error.to_string().contains("forced build failure"));
        assert!(!output.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

        stage_single_output(&input, &output, "test", |pending| {
            fs::write(pending, b"patched")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"patched");

        let existing_error = stage_single_output(&input, &output, "test", |_| Ok(())).unwrap_err();
        assert!(
            existing_error
                .to_string()
                .contains("must not already exist")
        );
        assert_eq!(fs::read(&output).unwrap(), b"patched");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn root_staging_rejects_shared_writable_directories() {
        assert!(root_staging_parent_is_safe(0o700, 0, 0));
        assert!(root_staging_parent_is_safe(0o755, 0, 0));
        assert!(!root_staging_parent_is_safe(0o700, 10000, 0));
        assert!(!root_staging_parent_is_safe(0o770, 0, 0));
        assert!(!root_staging_parent_is_safe(0o777, 0, 0));
        assert!(root_staging_parent_is_safe(0o777, 10000, 10000));
    }

    #[test]
    fn stale_staging_files_are_cleaned_without_touching_live_or_unrelated_files() {
        let dir = unique_dir("stale-staging");
        fs::create_dir_all(&dir).unwrap();
        let output_name = "output.img";
        let dead = dir.join(format!(
            ".{output_name}.ethereal-4294967295-patch-deadbeef.tmp"
        ));
        let live = dir.join(format!(
            ".{output_name}.ethereal-{}-patch-livebeef.tmp",
            std::process::id()
        ));
        let unrelated = dir.join("keep.tmp");
        fs::write(&dead, b"dead").unwrap();
        fs::write(&live, b"live").unwrap();
        fs::write(&unrelated, b"keep").unwrap();

        cleanup_stale_staged_outputs(&dir, output_name).unwrap();
        assert!(!dead.exists());
        assert!(live.exists());
        assert!(unrelated.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
