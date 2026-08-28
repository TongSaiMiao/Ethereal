//! Surgical ELF patch of first-stage init.
//!
//! KernelSU LKM **replaces** ramdisk `/init` with a different binary (ksuinit).
//! We never do that. GKI 1.0 (`boot` ramdisk) and GKI 2.0 (`init_boot` ramdisk)
//! both ship AOSP first-stage init, so we hook landmarks they share:
//!
//!   - string `"init first stage started!"`
//!   - symbol `FirstStageMain` / `_ZN7android4init15FirstStageMainEiPPc`
//!
//! Method: keep the OEM `/init` file, add a PT_LOAD stub, retarget `e_entry`.
//! The stub loads `ethereal.ko` (ramdisk still mounted) then branches to the
//! original entry. We do **not** hook `selinux_setup` — that is after
//! switch_root, when the ramdisk has already been freed.

use crate::scan::{self, InitScan};
use anyhow::{bail, ensure, Result};
use goblin::elf::header::EM_AARCH64;
use goblin::elf::program_header::{PF_R, PF_X, PT_LOAD, PT_NOTE};
use goblin::elf::Elf;

pub const MAGIC_ORIG_ENTRY: u64 = 0xD10E_7E00_E7E0_0001;
pub const MAGIC_STUB_VADDR: u64 = 0xD10E_7E00_E7E0_0002;
pub const PATCH_MARKER: &[u8; 8] = b"ETHRL01\0";
/// 64K so p_offset ≡ p_vaddr holds on 4K / 16K / 64K Android page sizes.
/// A 4K-only align boots on 16K kernels with a SIGKILL of pid 1 (OEM splash).
const PAGE: u64 = 0x10000;
const PHDR64: usize = 56;

pub fn is_patched(data: &[u8]) -> bool {
    data.windows(PATCH_MARKER.len()).any(|w| w == PATCH_MARKER)
}

fn r16(data: &[u8], off: usize) -> Result<u16> {
    ensure!(off + 2 <= data.len(), "elf truncated at {off}");
    Ok(u16::from_le_bytes(data[off..off + 2].try_into().unwrap()))
}

fn r64(data: &[u8], off: usize) -> Result<u64> {
    ensure!(off + 8 <= data.len(), "elf truncated at {off}");
    Ok(u64::from_le_bytes(data[off..off + 8].try_into().unwrap()))
}

fn w16(data: &mut [u8], off: usize, v: u16) -> Result<()> {
    ensure!(off + 2 <= data.len(), "elf truncated at {off}");
    data[off..off + 2].copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn w32(data: &mut [u8], off: usize, v: u32) -> Result<()> {
    ensure!(off + 4 <= data.len(), "elf truncated at {off}");
    data[off..off + 4].copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn w64(data: &mut [u8], off: usize, v: u64) -> Result<()> {
    ensure!(off + 8 <= data.len(), "elf truncated at {off}");
    data[off..off + 8].copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn align(n: u64, a: u64) -> u64 {
    (n + a - 1) & !(a - 1)
}

fn stub_payload(stub_elf: &[u8]) -> Result<(usize, Vec<u8>)> {
    let elf = Elf::parse(stub_elf)?;
    ensure!(elf.is_64, "stub must be ELF64");
    let mut loads: Vec<_> = elf
        .program_headers
        .iter()
        .filter(|p| p.p_type == PT_LOAD)
        .collect();
    ensure!(!loads.is_empty(), "stub has no PT_LOAD");
    loads.sort_by_key(|p| p.p_vaddr);
    let min_v = loads[0].p_vaddr;
    let max_end = loads
        .iter()
        .map(|p| p.p_vaddr + p.p_memsz.max(p.p_filesz))
        .max()
        .unwrap();
    let mut image = vec![0u8; (max_end - min_v) as usize];
    for p in &loads {
        let dest = (p.p_vaddr - min_v) as usize;
        let src_off = p.p_offset as usize;
        let n = p.p_filesz as usize;
        ensure!(src_off + n <= stub_elf.len(), "stub PT_LOAD truncated");
        image[dest..dest + n].copy_from_slice(&stub_elf[src_off..src_off + n]);
    }
    let entry_off = (elf.entry - min_v) as usize;
    ensure!(entry_off < image.len(), "stub e_entry outside PT_LOAD");
    Ok((entry_off, image))
}

fn patch_magics(buf: &mut [u8], orig_entry: u64, stub_vaddr: u64) -> Result<()> {
    let orig_b = orig_entry.to_le_bytes();
    let stub_b = stub_vaddr.to_le_bytes();
    let m_orig = MAGIC_ORIG_ENTRY.to_le_bytes();
    let m_stub = MAGIC_STUB_VADDR.to_le_bytes();
    let mut orig_hit = 0;
    let mut stub_hit = 0;
    let mut i = 0;
    while i + 8 <= buf.len() {
        if buf[i..i + 8] == m_orig {
            buf[i..i + 8].copy_from_slice(&orig_b);
            orig_hit += 1;
            i += 8;
            continue;
        }
        if buf[i..i + 8] == m_stub {
            buf[i..i + 8].copy_from_slice(&stub_b);
            stub_hit += 1;
            i += 8;
            continue;
        }
        i += 1;
    }
    ensure!(orig_hit >= 1, "stub is missing ORIG_ENTRY magic");
    ensure!(stub_hit >= 1, "stub is missing STUB_VADDR magic");
    Ok(())
}

fn write_phdr64(buf: &mut [u8], p_offset: u64, p_vaddr: u64, p_filesz: u64) -> Result<()> {
    w32(buf, 0, PT_LOAD)?;
    w32(buf, 4, PF_R | PF_X)?;
    w64(buf, 8, p_offset)?;
    w64(buf, 16, p_vaddr)?;
    w64(buf, 24, p_vaddr)?;
    w64(buf, 32, p_filesz)?;
    w64(buf, 40, p_filesz)?;
    w64(buf, 48, PAGE)?;
    Ok(())
}

fn install_phdr(init: &mut Vec<u8>, phdr_bytes: [u8; PHDR64]) -> Result<()> {
    let phoff = r64(init, 32)? as usize;
    let phentsize = r16(init, 54)? as usize;
    let phnum = r16(init, 56)? as usize;
    ensure!(phentsize == PHDR64, "unexpected phentsize {phentsize}");
    let table_end = phoff + phnum * phentsize;

    let elf = Elf::parse(init)?;
    let mut next_off = init.len();
    for p in &elf.program_headers {
        if p.p_offset as usize > table_end {
            next_off = next_off.min(p.p_offset as usize);
        }
    }
    if table_end + PHDR64 <= next_off && init[table_end..table_end + PHDR64].iter().all(|&b| b == 0)
    {
        init[table_end..table_end + PHDR64].copy_from_slice(&phdr_bytes);
        w16(init, 56, (phnum + 1) as u16)?;
        println!("HOOK_PHDR       [appended]");
        return Ok(());
    }

    for (i, p) in elf.program_headers.iter().enumerate() {
        if p.p_type == PT_NOTE {
            let off = phoff + i * phentsize;
            init[off..off + PHDR64].copy_from_slice(&phdr_bytes);
            println!("HOOK_PHDR       [replaced PT_NOTE #{i}]");
            return Ok(());
        }
    }
    bail!("no room to add a PT_LOAD (no PHDR padding, no PT_NOTE to reuse)");
}

pub struct PatchReport {
    pub orig_entry: u64,
    pub stub_vaddr: u64,
    pub landmarks: InitScan,
}

pub fn patch_init(init: &mut Vec<u8>, stub_elf: &[u8]) -> Result<PatchReport> {
    ensure!(
        init.len() >= 64 && init.starts_with(b"\x7fELF"),
        "not an ELF init"
    );
    ensure!(r16(init, 18)? == EM_AARCH64, "init is not aarch64");

    let scan = scan::scan_init(init);
    ensure!(
        scan.is_hookable(),
        "refusing to patch: need 'init first stage started!' or FirstStageMain (GKI 1.0/2.0 shared, before switch_root)"
    );
    if is_patched(init) {
        bail!("init is already Ethereal-hooked (ETHRL01)");
    }

    let orig_entry = r64(init, 24)?;
    let (entry_off, mut payload) = stub_payload(stub_elf)?;

    let elf = Elf::parse(init)?;
    let max_vend = elf
        .program_headers
        .iter()
        .filter(|p| p.p_type == PT_LOAD)
        .map(|p| p.p_vaddr + p.p_memsz)
        .max()
        .unwrap_or(0);

    // Both file offset and vaddr must share the same residue modulo PAGE.
    let new_off = align(init.len() as u64, PAGE);
    let new_vaddr = align(max_vend, PAGE);
    debug_assert_eq!(new_off % PAGE, 0);
    debug_assert_eq!(new_vaddr % PAGE, 0);

    // Pad so the payload (and _start) sit at a 16-byte boundary after the marker.
    const PREFIX: usize = 16;
    let mut prefix = [0u8; PREFIX];
    prefix[..PATCH_MARKER.len()].copy_from_slice(PATCH_MARKER);
    let payload_vaddr = new_vaddr + PREFIX as u64;
    let stub_entry_vaddr = payload_vaddr + entry_off as u64;
    patch_magics(&mut payload, orig_entry, stub_entry_vaddr)?;

    init.resize(new_off as usize, 0);
    init.extend_from_slice(&prefix);
    init.extend_from_slice(&payload);
    let filesz = init.len() as u64 - new_off;
    ensure!(
        new_off % PAGE == new_vaddr % PAGE,
        "PT_LOAD alignment broken: off={new_off:#x} va={new_vaddr:#x} align={PAGE:#x}"
    );

    let mut phdr = [0u8; PHDR64];
    write_phdr64(&mut phdr, new_off, new_vaddr, filesz)?;
    install_phdr(init, phdr)?;
    w64(init, 24, stub_entry_vaddr)?;

    println!("LANDMARK        [init first stage started! / FirstStageMain]");
    println!("HOOK            [ELF e_entry — OEM /init kept, not replaced]");
    println!("ORIG_ENTRY      [{orig_entry:#x}]");
    println!("STUB_ENTRY      [{stub_entry_vaddr:#x}]");
    Ok(PatchReport {
        orig_entry,
        stub_vaddr: stub_entry_vaddr,
        landmarks: scan,
    })
}

pub fn patch_init_file(init_path: &std::path::Path, stub_elf: &[u8]) -> Result<PatchReport> {
    let mut init = std::fs::read(init_path)?;
    let report = patch_init(&mut init, stub_elf)?;
    std::fs::write(init_path, init)?;
    Ok(report)
}
