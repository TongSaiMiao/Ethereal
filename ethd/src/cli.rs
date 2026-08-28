use crate::{defs, event, lua, module, module_config, ramdisk_patch, supercall, utils};
#[cfg(target_os = "android")]
use android_logger::Config;
use anyhow::{Context, Result};
use clap::Parser;
#[cfg(target_os = "android")]
use log::LevelFilter;
use std::path::PathBuf;

/// Ethereal daemon (ethd)
#[derive(Parser, Debug)]
#[command(author, version = defs::VERSION_CODE, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Manage Ethereal modules
    Module {
        #[command(subcommand)]
        command: Module,
    },

    /// Trigger `post-fs-data` event
    PostFsData,

    /// Trigger `service` event
    Services,

    /// Trigger `boot-complete` event
    BootCompleted,

    /// Start uid listener for synchronizing root list
    UidListener,

    /// Resetprop - Magisk-compatible system property tool
    #[command(disable_help_flag = true)]
    Resetprop {
        /// Arguments passed to resetprop
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, num_args = 0..)]
        args: Vec<String>,
    },

    /// MagiskPolicy - SELinux Policy Patch Tool
    Sepolicy(crate::sepolicy::Args),

    /// Patch one GKI 1.0 boot or GKI 2.0 init_boot image
    BootPatch {
        /// Input image; kernel-only boot and vendor_boot images are rejected
        #[arg(long)]
        image: PathBuf,
        /// Output image path
        #[arg(long)]
        out: PathBuf,
        /// ethinit stub ELF (default: bundled)
        #[arg(long)]
        ethinit: Option<PathBuf>,
        /// Explicit custom ethereal.ko; bundled modules remain KMI-qualified
        #[arg(long)]
        ko: Option<PathBuf>,
        /// Android UID of the Ethereal manager allowed to use the kernel interface
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        manager_uid: u32,
        /// Internal manager authentication; supplied by the Ethereal app
        #[arg(long, hide = true)]
        manager_token_file: PathBuf,
    },

    /// Patch a GKI 2.0 init_boot and kernel-only boot as one transaction
    BootPatchPair {
        /// Input init_boot image containing the first-stage ramdisk
        #[arg(long)]
        init_boot: PathBuf,
        /// Input kernel-only boot image whose cmdline is authoritative
        #[arg(long)]
        boot: PathBuf,
        /// Output patched init_boot image
        #[arg(long)]
        out_init_boot: PathBuf,
        /// Output patched boot image
        #[arg(long)]
        out_boot: PathBuf,
        /// ethinit stub ELF (default: bundled)
        #[arg(long)]
        ethinit: Option<PathBuf>,
        /// Explicit custom ethereal.ko; bundled modules remain KMI-qualified
        #[arg(long)]
        ko: Option<PathBuf>,
        /// Android UID of the Ethereal manager allowed to use the kernel interface
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        manager_uid: u32,
        /// Internal manager authentication; supplied by the Ethereal app
        #[arg(long, hide = true)]
        manager_token_file: PathBuf,
    },

    /// Restore stock init from a ramtool-patched image
    BootUnpatch {
        #[arg(long)]
        image: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(clap::Subcommand, Debug)]
enum Module {
    /// Install module <ZIP>
    Install {
        /// module zip file path
        zip: String,
    },

    /// Uninstall module <id>
    Uninstall {
        /// module id
        id: String,
    },

    /// UudoUninstall module <id>
    UndoUninstall {
        /// module id
        id: String,
    },

    /// enable module <id>
    Enable {
        /// module id
        id: String,
    },

    /// disable module <id>
    Disable {
        // module id
        id: String,
    },

    /// run action for module <id>
    Action {
        // module id
        id: String,
    },
    /// module lua runner
    Lua {
        // module id
        id: String,
        // lua function
        function: String,
    },
    /// list all modules
    List,

    /// manage module configuration
    Config {
        /// target internal module name (resolved as internal.<name>)
        #[arg(long)]
        internal: Option<String>,
        #[command(subcommand)]
        command: ModuleConfigCmd,
    },
}

#[derive(clap::Subcommand, Debug)]
enum ModuleConfigCmd {
    /// Get a config value
    Get {
        /// config key
        key: String,
    },

    /// Set a config value
    Set {
        /// config key
        key: String,
        /// config value (omit to read from stdin)
        value: Option<String>,
        /// read value from stdin (default if value not provided)
        #[arg(long)]
        stdin: bool,
        /// use temporary config (cleared on reboot)
        #[arg(short, long)]
        temp: bool,
    },

    /// List all config entries
    List,

    /// Delete a config entry
    Delete {
        /// config key
        key: String,
        /// delete from temporary config
        #[arg(short, long)]
        temp: bool,
    },

    /// Clear all config entries
    Clear {
        /// clear temporary config
        #[arg(short, long)]
        temp: bool,
    },
}

pub fn run() -> Result<()> {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        Config::default()
            .with_max_level(LevelFilter::Trace) // limit log level
            .with_tag("EtherealD")
            .with_filter(
                android_logger::FilterBuilder::new()
                    .filter_level(LevelFilter::Trace)
                    .filter_module("notify", LevelFilter::Warn)
                    .build(),
            ),
    );

    #[cfg(not(target_os = "android"))]
    env_logger::init();

    // The kernel redirects the su entry point to this process.
    let arg0 = std::env::args().next().unwrap_or_default();
    if arg0.ends_with("su") {
        return crate::ethd::root_shell();
    }
    if arg0.ends_with("resetprop") {
        let all_args: Vec<String> = std::env::args().collect();
        crate::resetprop::resetprop_main(&all_args)
    }
    if arg0.ends_with("magiskpolicy") {
        let all_args: Vec<String> = std::env::args().collect();
        crate::sepolicy::policy_main(&all_args)
    }

    let cli = Args::parse();

    log::info!("command: {:?}", cli.command);

    // SuperCall is not available until the LKM is loaded. Image patch commands
    // run in userspace on an unpatched kernel and must not depend on it.
    match &cli.command {
        Commands::BootPatch { .. }
        | Commands::BootPatchPair { .. }
        | Commands::BootUnpatch { .. } => {}
        _ => supercall::privilege_ethd_profile(),
    }

    let result = match cli.command {
        Commands::PostFsData => event::on_post_data_fs(),

        Commands::BootCompleted => event::on_boot_completed(),

        Commands::UidListener => event::start_uid_listener(),

        Commands::Module { command } => {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                utils::switch_mnt_ns(1)?;
            }
            match command {
                Module::Install { zip } => module::install_module(&zip),
                Module::Uninstall { id } => module::uninstall_module(&id),
                Module::UndoUninstall { id } => module::undo_uninstall_module(&id),
                Module::Action { id } => module::run_action(&id),
                Module::Lua { id, function } => {
                    module_config::validate_module_id(&id)?;
                    lua::run_lua(&id, &function, false, true).map_err(|e| anyhow::anyhow!("{}", e))
                }
                Module::Enable { id } => module::enable_module(&id),
                Module::Disable { id } => module::disable_module(&id),
                Module::List => module::list_modules(),
                Module::Config { internal, command } => {
                    let module_id = match internal {
                        Some(internal_name) => format!("internal.{internal_name}"),
                        None => std::env::var("ETHEREAL_MODULE").map_err(|_| {
                            anyhow::anyhow!(
                                "This command must be run in the context of a module or passed --internal <name>"
                            )
                        })?,
                    };
                    module_config::validate_module_id(&module_id)?;

                    match command {
                        ModuleConfigCmd::Get { key } => {
                            // Use merge_configs to respect priority (temp overrides persist)
                            let config = module_config::merge_configs(&module_id)?;
                            match config.get(&key) {
                                Some(value) => {
                                    println!("{value}");
                                    Ok(())
                                }
                                None => anyhow::bail!("Key '{key}' not found"),
                            }
                        }
                        ModuleConfigCmd::Set {
                            key,
                            value,
                            stdin,
                            temp,
                        } => {
                            // Validate key at CLI layer for better user experience
                            module_config::validate_config_key(&key)?;

                            // Read value from stdin or argument
                            let value_str = match value {
                                Some(v) if !stdin => v,
                                _ => {
                                    // Read from stdin
                                    use std::io::Read;
                                    let mut buffer = String::new();
                                    std::io::stdin()
                                        .read_to_string(&mut buffer)
                                        .context("Failed to read from stdin")?;
                                    buffer
                                }
                            };

                            // Validate value
                            module_config::validate_config_value(&value_str)?;

                            let config_type = if temp {
                                module_config::ConfigType::Temp
                            } else {
                                module_config::ConfigType::Persist
                            };
                            module_config::set_config_value(
                                &module_id,
                                &key,
                                &value_str,
                                config_type,
                            )
                        }
                        ModuleConfigCmd::List => {
                            let config = module_config::merge_configs(&module_id)?;
                            if config.is_empty() {
                                println!("No config entries found");
                            } else {
                                for (key, value) in config {
                                    println!("{key}={value}");
                                }
                            }
                            Ok(())
                        }
                        ModuleConfigCmd::Delete { key, temp } => {
                            let config_type = if temp {
                                module_config::ConfigType::Temp
                            } else {
                                module_config::ConfigType::Persist
                            };
                            module_config::delete_config_value(&module_id, &key, config_type)
                        }
                        ModuleConfigCmd::Clear { temp } => {
                            let config_type = if temp {
                                module_config::ConfigType::Temp
                            } else {
                                module_config::ConfigType::Persist
                            };
                            module_config::clear_config(&module_id, config_type)
                        }
                    }
                }
            }
        }

        Commands::Services => event::on_services(),

        Commands::Resetprop { args } => {
            let mut full_args = vec!["resetprop".to_string()];
            full_args.extend(args);
            crate::resetprop::resetprop_main(&full_args)
        }

        Commands::Sepolicy(sepolicy_args) => crate::sepolicy::execute(&sepolicy_args),

        Commands::BootPatch {
            image,
            out,
            ethinit,
            ko,
            manager_uid,
            manager_token_file,
        } => ramdisk_patch::boot_patch(ramdisk_patch::PatchArgs {
            image,
            out,
            ethinit,
            ko,
            manager_uid,
            manager_token_file,
        }),

        Commands::BootPatchPair {
            init_boot,
            boot,
            out_init_boot,
            out_boot,
            ethinit,
            ko,
            manager_uid,
            manager_token_file,
        } => ramdisk_patch::boot_patch_pair(ramdisk_patch::PairPatchArgs {
            init_boot,
            boot,
            out_init_boot,
            out_boot,
            ethinit,
            ko,
            manager_uid,
            manager_token_file,
        }),

        Commands::BootUnpatch { image, out } => ramdisk_patch::boot_unpatch(image, out),
    };

    if let Err(e) = &result {
        log::error!("Error: {:?}", e);
    }
    result
}
