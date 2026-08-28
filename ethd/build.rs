use std::env;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::process::Command;

// version.properties at the repo root is the single source of the manager
// version code epoch; build.gradle.kts and CI derive theirs from it as well.
fn get_version_epoch() -> u32 {
    std::fs::read_to_string("../version.properties")
        .expect("Failed to read ../version.properties")
        .lines()
        .find_map(|line| {
            line.strip_prefix("managerVersionEpoch=")
                .and_then(|v| v.trim().parse().ok())
        })
        .expect("managerVersionEpoch not found in version.properties")
}

fn get_git_version() -> Result<(u32, String), std::io::Error> {
    let output = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .output()?;

    let output = output.stdout;
    let version_code = String::from_utf8(output).expect("Failed to read git count stdout");
    let version_code: u32 = version_code
        .trim()
        .parse()
        .map_err(|_| std::io::Error::other("Failed to parse git count"))?;
    let version_code = get_version_epoch() + version_code;

    let version_name = String::from_utf8(
        Command::new("git")
            .args(["describe", "--tags", "--always"])
            .output()?
            .stdout,
    )
    .map_err(|_| std::io::Error::other("Failed to read git describe stdout"))?;
    let version_name = version_name.trim_start_matches('v').to_string();
    Ok((version_code, version_name))
}

fn main() {
    // update VersionCode when git repository change
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/refs/");
    println!("cargo:rerun-if-changed=../version.properties");

    let (code, name) = match get_git_version() {
        Ok((code, name)) => (code, name),
        Err(_) => {
            // show warning if git is not installed
            println!("cargo:warning=Failed to get git version, using 0.0.0");
            (0, "0.0.0".to_string())
        }
    };
    let out_dir = env::var("OUT_DIR").expect("Failed to get $OUT_DIR");
    println!("out_dir: ${out_dir}");
    println!("code: ${code}");
    let out_dir = Path::new(&out_dir);
    File::create(Path::new(out_dir).join("VERSION_CODE"))
        .expect("Failed to create VERSION_CODE")
        .write_all(code.to_string().as_bytes())
        .expect("Failed to write VERSION_CODE");

    File::create(Path::new(out_dir).join("VERSION_NAME"))
        .expect("Failed to create VERSION_NAME")
        .write_all(name.trim().as_bytes())
        .expect("Failed to write VERSION_NAME");

    embed_bins(out_dir);
}

fn copy_first(dst: &Path, srcs: &[&str]) {
    for s in srcs {
        let p = Path::new(s);
        println!("cargo:rerun-if-changed={s}");
        if p.is_file() && p.metadata().map(|m| m.len() > 64).unwrap_or(false) {
            std::fs::copy(p, dst).expect("copy embedded blob");
            println!(
                "cargo:warning=embedded {} from {s}",
                dst.file_name().unwrap().to_string_lossy()
            );
            return;
        }
    }
    std::fs::write(dst, b"MISSING").expect("write placeholder");
    println!(
        "cargo:warning={} missing; boot-patch will fail until ramtool/ethinit/ethereal.ko are built",
        dst.file_name().unwrap().to_string_lossy()
    );
}

fn embed_bins(out_dir: &Path) {
    let dir = out_dir.join("embedded");
    std::fs::create_dir_all(&dir).unwrap();
    copy_first(
        &dir.join("ramtool"),
        &[
            "embedded/ramtool",
            "../ramtool/target/aarch64-linux-android/release/ramtool",
        ],
    );
    copy_first(
        &dir.join("ethinit"),
        &[
            "embedded/ethinit",
            "../ethinit/target/aarch64-linux-android/release/ethinit",
        ],
    );
    embed_kos(out_dir, &dir);
}

fn looks_elf_file(p: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(p) else {
        return false;
    };
    let mut magic = [0u8; 4];
    use std::io::Read;
    f.read_exact(&mut magic).is_ok()
        && magic == *b"\x7fELF"
        && p.metadata().map(|m| m.len() > 64).unwrap_or(false)
}

fn elf_u16(data: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        data.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn elf_u32(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn elf_u64(data: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        data.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

#[derive(Clone, Copy)]
struct ElfSection {
    offset: usize,
    size: usize,
}

fn elf_section(data: &[u8], wanted: &[u8]) -> Option<ElfSection> {
    if data.len() < 64 || data.get(4) != Some(&2) || data.get(5) != Some(&1) {
        return None;
    }
    let section_offset = usize::try_from(elf_u64(data, 0x28)?).ok()?;
    let entry_size = usize::from(elf_u16(data, 0x3a)?);
    let section_count = usize::from(elf_u16(data, 0x3c)?);
    let names_index = usize::from(elf_u16(data, 0x3e)?);
    if entry_size < 64 || section_count == 0 || names_index >= section_count {
        return None;
    }
    let table_size = entry_size.checked_mul(section_count)?;
    section_offset
        .checked_add(table_size)
        .filter(|end| *end <= data.len())?;
    let header = |index: usize| section_offset.checked_add(index.checked_mul(entry_size)?);
    let names_header = header(names_index)?;
    let names_offset = usize::try_from(elf_u64(data, names_header + 24)?).ok()?;
    let names_size = usize::try_from(elf_u64(data, names_header + 32)?).ok()?;
    let names_end = names_offset.checked_add(names_size)?;
    if names_end > data.len() {
        return None;
    }
    for index in 0..section_count {
        let section_header = header(index)?;
        let relative_name = usize::try_from(elf_u32(data, section_header)?).ok()?;
        let name_start = match names_offset.checked_add(relative_name) {
            Some(start) if start < names_end => start,
            _ => continue,
        };
        let name_end = data[name_start..names_end]
            .iter()
            .position(|byte| *byte == 0)
            .map(|len| name_start + len)?;
        if &data[name_start..name_end] == wanted {
            let offset = usize::try_from(elf_u64(data, section_header + 24)?).ok()?;
            let size = usize::try_from(elf_u64(data, section_header + 32)?).ok()?;
            offset.checked_add(size).filter(|end| *end <= data.len())?;
            return Some(ElfSection { offset, size });
        }
    }
    None
}

pub(crate) fn validate_modversion_records(
    basic_size: Option<usize>,
    extended_crcs: Option<&[u8]>,
    extended_names: Option<&[u8]>,
) -> Result<(), String> {
    let has_basic = basic_size.unwrap_or(0) > 0;
    let has_extended = extended_crcs.is_some_and(|section| !section.is_empty())
        && extended_names.is_some_and(|section| !section.is_empty());
    if !has_basic && !has_extended {
        return Err("no non-empty basic or extended modversion records".into());
    }

    let has_any_extended = extended_crcs.is_some_and(|section| !section.is_empty())
        || extended_names.is_some_and(|section| !section.is_empty());
    if !has_any_extended {
        return Ok(());
    }
    if !has_extended {
        return Err("incomplete extended modversion sections".into());
    }

    let crcs = extended_crcs.expect("checked above");
    let names = extended_names.expect("checked above");
    if crcs.len() % 4 != 0 {
        return Err("misaligned __version_ext_crcs section".into());
    }

    let mut position = 0usize;
    for index in 0..(crcs.len() / 4) {
        let relative_end = names[position..]
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| "fewer extended modversion names than CRCs".to_string())?;
        if relative_end == 0 {
            return Err(format!("empty extended modversion name at index {index}"));
        }
        position += relative_end + 1;
    }
    if names[position..].iter().any(|byte| *byte != 0) {
        return Err("more extended modversion names than CRCs or non-zero name padding".into());
    }
    Ok(())
}

pub(crate) fn validate_modversions(data: &[u8]) -> Result<(), String> {
    let basic = elf_section(data, b"__versions");
    let extended_crcs = elf_section(data, b"__version_ext_crcs");
    let extended_names = elf_section(data, b"__version_ext_names");
    let bytes = |section: ElfSection| &data[section.offset..section.offset + section.size];
    validate_modversion_records(
        basic.map(|section| section.size),
        extended_crcs.map(bytes),
        extended_names.map(bytes),
    )
}

const REQUIRED_KMIS: &[&str] = &[
    "android12-5.4",
    "android12-5.10",
    "android13-5.10",
    "android13-5.15",
    "android14-5.15",
    "android14-6.1",
    "android15-6.6",
    "android16-6.12",
];

fn validate_ko(path: &Path, kmi: &str, marker: &[u8]) {
    assert!(
        looks_elf_file(path),
        "missing or invalid {}",
        path.display()
    );
    let data = std::fs::read(path).expect("read kernel module");
    assert!(
        data.windows(marker.len()).any(|window| window == marker),
        "{} is stale: missing Ethereal feature marker {}",
        path.display(),
        String::from_utf8_lossy(marker)
    );
    assert!(
        data.windows(b"name=ethereal".len())
            .any(|window| window == b"name=ethereal"),
        "{} has the wrong module identity",
        path.display()
    );
    let vermagic = format!("vermagic={}", kmi.rsplit('-').next().unwrap_or(kmi));
    assert!(
        data.windows(vermagic.len())
            .any(|window| window == vermagic.as_bytes()),
        "{} does not match {}",
        path.display(),
        kmi
    );
    if let Err(error) = validate_modversions(&data) {
        panic!(
            "{} has invalid module-version metadata: {error}; refusing a pseudo-compatible KO",
            path.display()
        );
    }
}

fn embed_kos(out_dir: &Path, embedded: &Path) {
    println!("cargo:rerun-if-changed=../kmod/prebuilt");
    println!("cargo:rerun-if-changed=embedded");

    let kos_dir = embedded.join("kos");
    std::fs::create_dir_all(&kos_dir).unwrap();

    let marker_path = Path::new("../kmod/feature-marker.txt");
    println!("cargo:rerun-if-changed={}", marker_path.display());
    let marker_text = std::fs::read_to_string(marker_path).expect("read kernel feature marker");
    let marker = marker_text.trim().as_bytes();
    let mut entries: Vec<(String, String)> = Vec::new();
    let prebuilt = Path::new("../kmod/prebuilt");
    for kmi in REQUIRED_KMIS {
        let ko = prebuilt.join(kmi).join("ethereal.ko");
        println!("cargo:rerun-if-changed={}", ko.display());
        validate_ko(&ko, kmi, marker);
        let dst = kos_dir.join(format!("{kmi}.ko"));
        std::fs::copy(&ko, &dst).expect("copy kernel module");
        entries.push(((*kmi).into(), dst.to_string_lossy().into_owned()));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut rs = String::from("pub static KOS: &[(&str, &[u8])] = &[\n");
    if entries.is_empty() {
        println!("cargo:warning=no ethereal.ko built yet; boot-patch will pack without LKM");
    }
    for (kmi, _path) in &entries {
        println!("cargo:warning=embedded ethereal.{kmi}.ko");
        rs.push_str(&format!(
            "    (\"{kmi}\", include_bytes!(concat!(env!(\"OUT_DIR\"), \"/embedded/kos/{kmi}.ko\"))),\n"
        ));
    }
    rs.push_str("];\n");
    std::fs::write(out_dir.join("embedded_kos.rs"), rs).expect("write embedded_kos.rs");
}
