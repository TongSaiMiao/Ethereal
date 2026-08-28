use anyhow::Result;
use goblin::elf::Elf;

/// Landmarks that exist in AOSP first-stage init on **both** GKI 1.0
/// (`boot` ramdisk) and GKI 2.0 (`init_boot` ramdisk).
///
/// Hookable (before switch_root, ramdisk still mounted):
///   `init first stage started!`, `FirstStageMain`
/// Too late to load a ramdisk .ko (after switch_root — do not hook):
///   `selinux_setup`, `/system/bin/init`
pub const INIT_STRINGS: &[&str] = &[
    "init first stage started!",
    "init second stage started!",
    "selinux_setup",
    "second_stage",
    "/system/bin/init",
    "FirstStageMount",
    "FirstStageMain",
    "Switching root",
    "Couldn't load SELinux policy",
    "Loading SELinux policy",
    "ro.boot.init_fatal",
    "INIT_FIRST_STAGE",
];

pub const INIT_SYMBOLS: &[&str] = &[
    "_ZN7android4init15FirstStageMainEiPPc",
    "_ZN7android4init16SecondStageMainEiPPc",
    "_ZN7android4init13selinux_setupEv",
    "FirstStageMain",
    "SecondStageMain",
    "selinux_setup",
];

#[derive(Debug, Default)]
pub struct InitScan {
    pub strings: Vec<String>,
    pub symbols: Vec<String>,
}

impl InitScan {
    fn has(&self, n: &str) -> bool {
        self.strings.iter().any(|s| s == n) || self.symbols.iter().any(|s| s.contains(n))
    }

    /// First-stage init fingerprint (GKI 1.0 boot ramdisk and GKI 2.0 init_boot).
    pub fn is_first_stage(&self) -> bool {
        self.has("init first stage started!")
            || self.has("FirstStageMain")
            || (self.has("selinux_setup") && self.has("/system/bin/init"))
    }

    /// Landmarks we actually hook — must run before switch_root.
    pub fn is_hookable(&self) -> bool {
        self.has("init first stage started!") || self.has("FirstStageMain")
    }
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

pub fn scan_init(data: &[u8]) -> InitScan {
    let mut scan = InitScan::default();
    for s in INIT_STRINGS {
        if find_bytes(data, s.as_bytes()) {
            scan.strings.push((*s).to_string());
        }
    }
    if let Ok(elf) = Elf::parse(data) {
        for sym in INIT_SYMBOLS {
            let found = elf.syms.iter().any(|s| {
                elf.strtab
                    .get_at(s.st_name)
                    .map(|n| n == *sym || n.ends_with(sym))
                    .unwrap_or(false)
            });
            if found {
                scan.symbols.push((*sym).to_string());
            }
        }
    }
    scan
}

pub fn print_scan(scan: &InitScan) {
    println!("INIT_STRINGS    [{}]", scan.strings.len());
    for s in &scan.strings {
        println!("  string        {s}");
    }
    println!("INIT_SYMBOLS    [{}]", scan.symbols.len());
    for s in &scan.symbols {
        println!("  symbol        {s}");
    }
    if scan.is_first_stage() {
        println!("FIRST_STAGE     [yes]");
    } else {
        println!("FIRST_STAGE     [no]");
    }
    if scan.is_hookable() {
        println!("HOOKABLE        [yes — FirstStageMain / init first stage started!]");
    } else {
        println!("HOOKABLE        [no]");
    }
}

pub fn scan_file(path: &std::path::Path) -> Result<InitScan> {
    let data = std::fs::read(path)?;
    Ok(scan_init(&data))
}
