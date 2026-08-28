use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use ramtool::{bootimg, cpio, elfpatch, scan};
use std::fs;
use std::path::PathBuf;

/// ramtool — unpack/repack Android boot and init_boot ramdisk images.
/// Interface is magiskboot-like; this binary is not magiskboot.
#[derive(Parser, Debug)]
#[command(name = "ramtool", version)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Unpack boot/init_boot/vendor_boot into the current directory
    Unpack { image: PathBuf },
    /// Repack using files in the current directory
    Repack { orig: PathBuf, out: Option<PathBuf> },
    /// Add rdinit to a GKI 2.0 kernel-only boot without rebuilding the image
    PatchGki2BootCmdline { boot: PathBuf, out: PathBuf },
    /// newc cpio operations on a ramdisk (exists/extract/add/mkdir/rm/mv)
    Cpio {
        archive: PathBuf,
        /// Each command is a single token, e.g. "mv init init.real"
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        commands: Vec<String>,
    },
    /// Scan an extracted init binary for first-stage landmarks
    ScanInit { init: PathBuf },
    /// Hook OEM first-stage init (does NOT replace the /init file like KernelSU)
    PatchInit {
        init: PathBuf,
        /// ethinit stub ELF (injected as extra PT_LOAD; e_entry retargeted)
        stub: PathBuf,
    },
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Commands::Unpack { image } => {
            bootimg::unpack(&image, std::env::current_dir()?.as_path())?;
        }
        Commands::Repack { orig, out } => {
            let out = out.unwrap_or_else(|| PathBuf::from("new-boot.img"));
            bootimg::repack(&orig, std::env::current_dir()?.as_path(), &out)?;
        }
        Commands::PatchGki2BootCmdline { boot, out } => {
            bootimg::patch_gki2_boot_cmdline(&boot, &out)?;
        }
        Commands::Cpio { archive, commands } => {
            if commands.is_empty() {
                bail!("ramtool cpio <archive> <cmd>...");
            }
            let mut data = fs::read(&archive)?;
            let readonly = commands
                .iter()
                .all(|c| matches!(c.split_whitespace().next(), Some("exists" | "extract")));
            for cmd in commands {
                data = cpio::apply_command(data, &cmd)?;
            }
            if !readonly {
                fs::write(&archive, data)?;
            }
        }
        Commands::ScanInit { init } => {
            let result = scan::scan_file(&init)?;
            scan::print_scan(&result);
            if !result.is_first_stage() {
                std::process::exit(2);
            }
        }
        Commands::PatchInit { init, stub } => {
            let stub_elf = fs::read(&stub)?;
            elfpatch::patch_init_file(&init, &stub_elf)?;
        }
    }
    Ok(())
}
