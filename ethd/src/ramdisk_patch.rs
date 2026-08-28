//! Ramdisk patch using an `rdinit=/ethereal-init` trampoline only.
//!
//! GKI 1.0 stores the payload and cmdline in `boot`. GKI 2.0 stores the
//! payload in `init_boot` and the cmdline in `boot`. Neither flow ELF-hooks or
//! replaces the OEM `/init`, rewrites the kernel Image, or patches `vendor_boot`.

use anyhow::{Context, Result, bail, ensure};
use std::fs::{self, OpenOptions};
use std::io::Write;
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

#[derive(Clone, Debug)]
pub struct PatchArgs {
    pub image: PathBuf,
    pub out: PathBuf,
    pub ethinit: Option<PathBuf>,
    pub ko: Option<PathBuf>,
    pub manager_uid: u32,
    pub manager_token_file: PathBuf,
    pub skip_symbol_check: bool,
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
    pub skip_symbol_check: bool,
}

fn looks_stock_boot_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if !(n.ends_with(".img") || n.ends_with(".bin")) {
        return false;
    }
    if n.contains("ethereal")
        || n.contains("magisk")
        || n.contains("patched")
        || n.starts_with("new-")
    {
        return false;
    }
    n.contains("vendor_boot")
        || n.contains("init_boot")
        || n == "boot.img"
        || n == "boot.bin"
        || n.starts_with("boot_a.")
        || n.starts_with("boot_b.")
}

/// Only images next to the primary (the APK patch dir). Never scan Download —
/// those were getting ELF-hooked and flashed onto live partitions.
fn find_sibling_images(primary: &Path) -> Vec<PathBuf> {
    let Some(dir) = primary.parent() else {
        return Vec::new();
    };
    let primary_canon = primary
        .canonicalize()
        .unwrap_or_else(|_| primary.to_path_buf());
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(dir) else {
        return out;
    };
    for ent in rd.flatten() {
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !looks_stock_boot_name(name) {
            continue;
        }
        let canon = path.canonicalize().unwrap_or(path.clone());
        if canon == primary_canon {
            continue;
        }
        if out.iter().any(|p: &PathBuf| *p == canon) {
            continue;
        }
        out.push(canon);
    }
    out
}

fn sibling_out(primary_out: &Path, sibling: &Path) -> PathBuf {
    let stem = sibling
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("vendor_boot");
    let name = format!("new-{stem}.img");
    primary_out
        .parent()
        .map(|p| p.join(&name))
        .unwrap_or_else(|| PathBuf::from(name))
}

fn ensure_rdinit() -> Result<()> {
    const TOK: &str = "rdinit=/ethereal-init";
    let p = Path::new("cmdline.txt");
    let cur = fs::read_to_string(p).unwrap_or_default();
    let existing = cur.split_whitespace().find(|t| t.starts_with("rdinit="));
    ensure!(
        existing.is_none() || existing == Some(TOK),
        "image already defines {}; refusing to overwrite it",
        existing.unwrap_or("rdinit")
    );
    let next = if existing.is_some() {
        cur.split_whitespace().collect::<Vec<_>>().join(" ")
    } else if cur.trim().is_empty() {
        TOK.to_string()
    } else {
        format!("{} {TOK}", cur.trim())
    };
    let cap = fs::read_to_string("cmdline.cap")
        .context("read cmdline.cap")?
        .trim()
        .parse::<usize>()
        .context("parse cmdline.cap")?;
    ensure!(
        next.len() < cap,
        "boot cmdline with {TOK} is {} bytes, but the header allows at most {} bytes",
        next.len(),
        cap.saturating_sub(1)
    );
    fs::write(p, next).context("write cmdline.txt")?;
    Ok(())
}

fn strip_rdinit() {
    let p = Path::new("cmdline.txt");
    let cur = fs::read_to_string(p).unwrap_or_default();
    let next = cur
        .split_whitespace()
        .filter(|t| *t != "rdinit=/ethereal-init")
        .collect::<Vec<_>>()
        .join(" ");
    let _ = fs::write(p, next);
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

fn pack_kos(ramtool: &Path, ko: &Option<PathBuf>) -> Result<usize> {
    let mut packed = 0usize;
    // Remove any generic module inherited from an older patched image. The
    // generic path is reserved for this invocation's explicit --ko only.
    if cpio_ok(ramtool, "ramdisk.cpio", "exists ethereal.ko") {
        cpio(ramtool, &["rm ethereal.ko"])?;
    }
    if let Some(ko) = ko {
        cpio(
            ramtool,
            &[&format!("add 0755 ethereal.ko {}", ko.display())],
        )?;
        packed += 1;
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
        packed += 1;
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

fn pack_su(ramtool: &Path) -> Result<()> {
    let Some(su) = find_su() else {
        persist_log("- WARNING: su binary not staged; ramdisk will have no /eth/su");
        return Ok(());
    };
    let path = su.display().to_string();
    let _ = cpio(ramtool, &["mkdir 0755 eth"]);
    let _ = cpio(ramtool, &["mkdir 0755 debug_ramdisk"]);
    cpio(ramtool, &[&format!("add 0755 eth/su {path}")])?;
    let _ = cpio(ramtool, &[&format!("add 0755 debug_ramdisk/su {path}")]);
    let _ = cpio(ramtool, &[&format!("add 0755 su {path}")]);
    persist_log(&format!(
        "- packed {} as /eth/su /debug_ramdisk/su /su",
        path
    ));
    Ok(())
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

fn image_kind(kernel: bool, ramdisk: bool) -> &'static str {
    if is_vendor() {
        "vendor_boot"
    } else if ramdisk && !kernel {
        "init_boot"
    } else if ramdisk && kernel {
        "boot"
    } else {
        "boot-kernel"
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
    Gki1Boot,
    Gki2InitBoot,
}

fn patch_ramdisk_image(
    ramtool: &Path,
    ethinit: &Path,
    image: &Path,
    out: &Path,
    ko: &Option<PathBuf>,
    manager_uid: u32,
    manager_token: &[u8],
    _skip_symbol_check: bool,
    target: RamdiskTarget,
) -> Result<()> {
    clean_unpack_stale();
    persist_log(&format!("- image:   {}", image.display()));
    let unpack = run(ramtool, &["unpack", &image.to_string_lossy()])?;
    print_output(&unpack);
    let vendor = is_vendor();
    let has_rd = Path::new("ramdisk.cpio").exists();
    let has_kernel = Path::new("kernel").exists();
    let kind = image_kind(has_kernel, has_rd);
    persist_log(&format!("- kind:    {kind}"));

    if vendor {
        bail!(
            "vendor_boot is not a standalone Ethereal patch target; select init_boot (GKI 2.0) or boot (GKI 1.0)"
        );
    }

    match target {
        RamdiskTarget::Gki1Boot => ensure!(
            has_kernel && has_rd,
            "boot-patch only accepts a GKI 1.0 boot image containing both kernel and ramdisk; use boot-patch-pair for GKI 2.0"
        ),
        RamdiskTarget::Gki2InitBoot => ensure!(
            !has_kernel && has_rd,
            "--init-boot must be a GKI 2.0 init_boot image containing ramdisk and no kernel"
        ),
    }

    match target {
        RamdiskTarget::Gki1Boot => {
            ensure_rdinit()?;
            persist_log("- mode:    GKI 1.0 ramdisk payload + rdinit=");
        }
        RamdiskTarget::Gki2InitBoot => {
            // Remove the obsolete marker from images produced by the old,
            // invalid single-init_boot patch flow. GKI 2.0 takes cmdline from boot.
            strip_rdinit();
            persist_log("- mode:    GKI 2.0 init_boot payload only (cmdline stays in boot)");
        }
    }
    pack_loader(ramtool, ethinit)?;
    pack_manager_credentials(ramtool, manager_uid, manager_token)?;
    let packed = pack_kos(ramtool, ko)?;
    if packed == 0 {
        persist_log("- WARNING: ethereal.ko not bundled; LKM will not load");
    }
    pack_su(ramtool)?;
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
    let manager_token = read_manager_token(&args.manager_token_file)?;
    let (ramtool, ethinit) = prepare_patch_tools(args.ethinit)?;
    // A generic ethereal.ko is accepted only through an explicit --ko.
    // Silently taking one from cwd previously made a stale 6.1 module look
    // like a valid fallback for unknown or ambiguous kernels.
    let ko = args.ko;
    persist_log("- mode:    GKI 1.0 single boot; rdinit=/ethereal-init; never ELF-hook /init");

    patch_ramdisk_image(
        &ramtool,
        &ethinit,
        &args.image,
        &args.out,
        &ko,
        args.manager_uid,
        &manager_token,
        args.skip_symbol_check,
        RamdiskTarget::Gki1Boot,
    )?;

    persist_log(&format!("- wrote {}", args.out.display()));
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

fn staged_output(out: &Path, label: &str) -> Result<PathBuf> {
    let name = out
        .file_name()
        .and_then(|name| name.to_str())
        .context("paired output path has no file name")?;
    let parent = out.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(format!(
        ".{name}.ethereal-{}-{label}.tmp",
        std::process::id()
    )))
}

fn remove_if_present(path: &Path) {
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

pub fn boot_patch_pair(args: PairPatchArgs) -> Result<()> {
    let init_input = path_identity(&args.init_boot);
    let boot_input = path_identity(&args.boot);
    let init_output = path_identity(&args.out_init_boot);
    let boot_output = path_identity(&args.out_boot);

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
        !args.out_init_boot.exists() && !args.out_boot.exists(),
        "paired output paths must not already exist"
    );

    let staged_init = staged_output(&args.out_init_boot, "init-boot")?;
    let staged_boot = staged_output(&args.out_boot, "boot")?;
    remove_if_present(&staged_init);
    remove_if_present(&staged_boot);

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
            args.skip_symbol_check,
            RamdiskTarget::Gki2InitBoot,
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

        let staged_init_len = fs::metadata(&staged_init)?.len();
        let staged_boot_len = fs::metadata(&staged_boot)?.len();
        let original_boot_len = fs::metadata(&args.boot)?.len();
        ensure!(staged_init_len > 0, "patched init_boot output is empty");
        ensure!(staged_boot_len > 0, "patched boot output is empty");
        ensure!(
            staged_boot_len == original_boot_len,
            "patched boot length changed from {original_boot_len} to {staged_boot_len}"
        );

        fs::rename(&staged_init, &args.out_init_boot).with_context(|| {
            format!(
                "publish paired init_boot output {}",
                args.out_init_boot.display()
            )
        })?;
        if let Err(error) = fs::rename(&staged_boot, &args.out_boot) {
            remove_if_present(&args.out_init_boot);
            return Err(error).with_context(|| {
                format!("publish paired boot output {}", args.out_boot.display())
            });
        }
        Ok(())
    })();

    if let Err(error) = result {
        remove_if_present(&staged_init);
        remove_if_present(&staged_boot);
        remove_if_present(&args.out_init_boot);
        remove_if_present(&args.out_boot);
        return Err(error);
    }

    persist_log(&format!("- wrote {}", args.out_init_boot.display()));
    persist_log(&format!("- wrote {}", args.out_boot.display()));
    persist_log("- paired patch complete; both images are required");
    Ok(())
}

fn rm_if_present(ramtool: &Path, name: &str) {
    if cpio_ok(ramtool, "ramdisk.cpio", &format!("exists {name}")) {
        let _ = cpio(ramtool, &[&format!("rm {name}")]);
    }
}

pub fn boot_unpatch(image: PathBuf, out: PathBuf) -> Result<()> {
    let ramtool = tool("ramtool");
    clean_unpack_stale();
    let unpack = run(&ramtool, &["unpack", &image.to_string_lossy()])?;
    print_output(&unpack);
    strip_rdinit();
    let vendor = is_vendor();
    if Path::new("ramdisk.cpio").exists() {
        let mut restored_elf_hook = false;
        if cpio_ok(&ramtool, "ramdisk.cpio", "exists init.ethereal.bak") {
            let restore = run(
                &ramtool,
                &[
                    "cpio",
                    "ramdisk.cpio",
                    "rm init",
                    "mv init.ethereal.bak init",
                ],
            )?;
            print_output(&restore);
            restored_elf_hook = true;
        }
        rm_if_present(&ramtool, "ethereal-init");
        rm_if_present(&ramtool, "ethereal.manager_uid");
        rm_if_present(&ramtool, "ethereal.manager_token");
        rm_if_present(&ramtool, "ethereal.ko");
        rm_if_present(&ramtool, "su");
        rm_if_present(&ramtool, "eth/su");
        rm_if_present(&ramtool, "debug_ramdisk/su");
        for name in crate::bundle::bundled_ko_names() {
            rm_if_present(&ramtool, &name);
        }
        if vendor {
            // After stripping extras, drop decompressed cpio so original fragments are kept
            // unless we restored an ELF-hook backup that must be written back.
            if !restored_elf_hook {
                let _ = fs::remove_file("ramdisk.cpio");
            }
        }
    }
    let out_s = out.to_string_lossy().into_owned();
    let repack = run(&ramtool, &["repack", &image.to_string_lossy(), &out_s])?;
    print_output(&repack);
    persist_log(&format!("- restored ramdisk/cmdline to {out_s}"));
    Ok(())
}
