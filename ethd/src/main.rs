mod assets;
mod bundle;
mod cli;
mod defs;
mod ethd;
mod event;
mod lua;
mod metamodule;
mod module;
mod module_config;
mod package;
#[cfg(any(target_os = "linux", target_os = "android"))]
mod pty;
mod ramdisk_patch;
mod resetprop;
mod restorecon;
mod sepolicy;
mod supercall;
mod utils;
fn main() -> anyhow::Result<()> {
    cli::run()
}
