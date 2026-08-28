#[allow(dead_code)]
#[path = "../build.rs"]
mod build_script;

use build_script::{validate_modversion_records, validate_modversions};
use std::path::PathBuf;

const RELEASE_KMIS: &[&str] = &[
    "android12-5.4",
    "android12-5.10",
    "android13-5.10",
    "android13-5.15",
    "android14-5.15",
    "android14-6.1",
    "android15-6.6",
    "android16-6.12",
];

#[test]
fn accepts_basic_modversions() {
    assert!(validate_modversion_records(Some(64), None, None).is_ok());
}

#[test]
fn accepts_extended_only_modversions() {
    let crcs = [0u8; 8];
    let names = b"first_symbol\0second_symbol\0";
    assert!(validate_modversion_records(None, Some(&crcs), Some(names)).is_ok());
}

#[test]
fn accepts_all_release_module_sections() {
    let manifest_dir = option_env!("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let source = PathBuf::from(file!());
            let source = if source.is_absolute() {
                source
            } else {
                std::env::current_dir()
                    .expect("current directory")
                    .join(source)
            };
            source
                .parent()
                .and_then(|tests| tests.parent())
                .expect("ethd manifest directory")
                .to_path_buf()
        });
    let root = manifest_dir.join("..");
    for kmi in RELEASE_KMIS {
        let path = root.join("kmod/prebuilt").join(kmi).join("ethereal.ko");
        let data = std::fs::read(&path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", path.display());
        });
        validate_modversions(&data).unwrap_or_else(|error| {
            panic!("{} failed modversion validation: {error}", path.display());
        });
    }
}

#[test]
fn rejects_incomplete_or_misaligned_extended_modversions() {
    assert!(validate_modversion_records(None, Some(&[0; 4]), None).is_err());
    assert!(validate_modversion_records(None, Some(&[0; 3]), Some(b"symbol\0")).is_err());
}

#[test]
fn rejects_extended_name_count_mismatches() {
    assert!(validate_modversion_records(None, Some(&[0; 8]), Some(b"only_one\0")).is_err());
    assert!(validate_modversion_records(None, Some(&[0; 4]), Some(b"one\0extra\0")).is_err());
    assert!(validate_modversion_records(None, Some(&[0; 4]), Some(b"\0")).is_err());
}
