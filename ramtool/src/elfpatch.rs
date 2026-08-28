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
use goblin::elf::program_header::{PF_R, PF_X, PT_DYNAMIC, PT_INTERP, PT_LOAD, PT_NOTE};
use goblin::elf::Elf;

pub const MAGIC_ORIG_ENTRY: u64 = 0xD10E_7E00_E7E0_0001;
pub const MAGIC_STUB_VADDR: u64 = 0xD10E_7E00_E7E0_0002;
pub const PATCH_MARKER: &[u8; 8] = b"ETHRL01\0";
/// 64K so p_offset ≡ p_vaddr holds on 4K / 16K / 64K Android page sizes.
/// A 4K-only align boots on 16K kernels with a SIGKILL of pid 1 (OEM splash).
const PAGE: u64 = 0x10000;
const PHDR64: usize = 56;
const MAX_STUB_IMAGE: u64 = 64 * 1024 * 1024;

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

fn align(n: u64, a: u64) -> Result<u64> {
    ensure!(a.is_power_of_two(), "invalid alignment {a}");
    Ok(n.checked_add(a - 1)
        .ok_or_else(|| anyhow::anyhow!("alignment overflow"))?
        & !(a - 1))
}

fn stub_payload(stub_elf: &[u8]) -> Result<(usize, Vec<u8>)> {
    let elf = Elf::parse(stub_elf)?;
    ensure!(elf.is_64, "stub must be ELF64");
    ensure!(
        elf.header.e_machine == EM_AARCH64,
        "stub must target aarch64"
    );
    ensure!(
        !elf.program_headers.iter().any(|p| p.p_type == PT_INTERP),
        "stub must be statically linked (PT_INTERP is not supported)"
    );
    // We copy PT_LOAD bytes, not the stub's ELF loader state. Dynamic relocations
    // would be stranded in the injected segment and usually fail as pid 1.
    ensure!(
        !elf.program_headers.iter().any(|p| p.p_type == PT_DYNAMIC),
        "stub must be self-contained (PT_DYNAMIC is not supported)"
    );
    let mut loads: Vec<_> = elf
        .program_headers
        .iter()
        .filter(|p| p.p_type == PT_LOAD)
        .collect();
    ensure!(!loads.is_empty(), "stub has no PT_LOAD");
    loads.sort_by_key(|p| p.p_vaddr);
    let min_v = loads[0].p_vaddr;
    ensure!(
        min_v & 0xfff == 0,
        "stub PT_LOAD base must be 4K-aligned for AArch64 page-relative addressing"
    );
    let mut max_end = min_v;
    let mut entry_is_executable = false;
    for p in &loads {
        ensure!(p.p_filesz <= p.p_memsz, "stub PT_LOAD filesz exceeds memsz");
        let mem_end = p
            .p_vaddr
            .checked_add(p.p_memsz)
            .ok_or_else(|| anyhow::anyhow!("stub PT_LOAD virtual range overflows"))?;
        max_end = max_end.max(mem_end);
        let file_vend = p
            .p_vaddr
            .checked_add(p.p_filesz)
            .ok_or_else(|| anyhow::anyhow!("stub PT_LOAD file-backed range overflows"))?;
        if p.p_flags & PF_X != 0 && elf.entry >= p.p_vaddr && elf.entry < file_vend {
            entry_is_executable = true;
        }
    }
    ensure!(
        entry_is_executable,
        "stub e_entry is not file-backed by an executable PT_LOAD"
    );

    let image_len = max_end
        .checked_sub(min_v)
        .ok_or_else(|| anyhow::anyhow!("stub PT_LOAD range underflows"))?;
    ensure!(
        image_len <= MAX_STUB_IMAGE,
        "stub PT_LOAD image exceeds {MAX_STUB_IMAGE} bytes"
    );
    let mut image = vec![
        0u8;
        usize::try_from(image_len)
            .map_err(|_| anyhow::anyhow!("stub PT_LOAD image is too large"))?
    ];
    for p in &loads {
        let dest = usize::try_from(
            p.p_vaddr
                .checked_sub(min_v)
                .ok_or_else(|| anyhow::anyhow!("stub PT_LOAD precedes image base"))?,
        )
        .map_err(|_| anyhow::anyhow!("stub PT_LOAD destination is too large"))?;
        let src_off = usize::try_from(p.p_offset)
            .map_err(|_| anyhow::anyhow!("stub PT_LOAD offset is too large"))?;
        let n = usize::try_from(p.p_filesz)
            .map_err(|_| anyhow::anyhow!("stub PT_LOAD filesz is too large"))?;
        let src_end = src_off
            .checked_add(n)
            .ok_or_else(|| anyhow::anyhow!("stub PT_LOAD file range overflows"))?;
        let dest_end = dest
            .checked_add(n)
            .ok_or_else(|| anyhow::anyhow!("stub PT_LOAD destination range overflows"))?;
        ensure!(src_end <= stub_elf.len(), "stub PT_LOAD truncated");
        ensure!(dest_end <= image.len(), "stub PT_LOAD exceeds image range");
        image[dest..dest_end].copy_from_slice(&stub_elf[src_off..src_end]);
    }
    let entry_off = usize::try_from(
        elf.entry
            .checked_sub(min_v)
            .ok_or_else(|| anyhow::anyhow!("stub e_entry precedes its PT_LOAD image"))?,
    )
    .map_err(|_| anyhow::anyhow!("stub e_entry offset is too large"))?;
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

    let elf = Elf::parse(&*init)?;
    ensure!(elf.is_64, "init must be ELF64");
    ensure!(
        !elf.program_headers.iter().any(|p| p.p_type == PT_INTERP),
        "refusing to patch a dynamically linked /init"
    );

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

    let mut max_vend = None;
    for phdr in elf.program_headers.iter().filter(|p| p.p_type == PT_LOAD) {
        let end = phdr
            .p_vaddr
            .checked_add(phdr.p_memsz)
            .ok_or_else(|| anyhow::anyhow!("init PT_LOAD virtual range overflows"))?;
        max_vend = Some(max_vend.map_or(end, |current: u64| current.max(end)));
    }
    let max_vend = max_vend.ok_or_else(|| anyhow::anyhow!("init has no PT_LOAD"))?;

    // Both file offset and vaddr must share the same residue modulo PAGE.
    let new_off = align(
        u64::try_from(init.len()).map_err(|_| anyhow::anyhow!("init is too large"))?,
        PAGE,
    )?;
    let new_vaddr = align(max_vend, PAGE)?;
    debug_assert_eq!(new_off % PAGE, 0);
    debug_assert_eq!(new_vaddr % PAGE, 0);

    // A 16-byte prefix looked harmless once. ADRP disagreed and shifted every
    // string address, so the linked image must start at a page-preserving load bias.
    let stub_entry_vaddr = new_vaddr + entry_off as u64;
    patch_magics(&mut payload, orig_entry, stub_entry_vaddr)?;

    init.resize(new_off as usize, 0);
    init.extend_from_slice(&payload);
    init.extend_from_slice(PATCH_MARKER);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn elf64(entry: u64, phnum: u16, len: usize) -> Vec<u8> {
        let mut data = vec![0u8; len];
        data[..4].copy_from_slice(b"\x7fELF");
        data[4] = 2; // ELFCLASS64
        data[5] = 1; // ELFDATA2LSB
        data[6] = 1; // EV_CURRENT
        w16(&mut data, 16, 2).unwrap(); // ET_EXEC
        w16(&mut data, 18, EM_AARCH64).unwrap();
        w32(&mut data, 20, 1).unwrap();
        w64(&mut data, 24, entry).unwrap();
        w64(&mut data, 32, 64).unwrap();
        w16(&mut data, 52, 64).unwrap();
        w16(&mut data, 54, PHDR64 as u16).unwrap();
        w16(&mut data, 56, phnum).unwrap();
        data
    }

    fn set_load(data: &mut [u8], index: usize, offset: u64, vaddr: u64, filesz: u64, memsz: u64) {
        let phdr = 64 + index * PHDR64;
        w32(&mut data[phdr..phdr + PHDR64], 0, PT_LOAD).unwrap();
        w32(&mut data[phdr..phdr + PHDR64], 4, PF_R | PF_X).unwrap();
        w64(&mut data[phdr..phdr + PHDR64], 8, offset).unwrap();
        w64(&mut data[phdr..phdr + PHDR64], 16, vaddr).unwrap();
        w64(&mut data[phdr..phdr + PHDR64], 24, vaddr).unwrap();
        w64(&mut data[phdr..phdr + PHDR64], 32, filesz).unwrap();
        w64(&mut data[phdr..phdr + PHDR64], 40, memsz).unwrap();
        w64(&mut data[phdr..phdr + PHDR64], 48, 0x4000).unwrap();
    }

    #[test]
    fn injected_stub_keeps_its_linked_page_offsets() {
        let mut init = elf64(0x100, 1, 512);
        set_load(&mut init, 0, 0, 0, 512, 512);
        let landmark = b"init first stage started!";
        init[320..320 + landmark.len()].copy_from_slice(landmark);

        // Model the real ethinit shape: headers/rodata and code live in
        // separate PT_LOADs with a virtual-address gap between them.
        let mut stub = elf64(0x204320, 2, 0x400);
        set_load(&mut stub, 0, 0, 0x200000, 0x200, 0x200);
        set_load(&mut stub, 1, 0x300, 0x204300, 0x100, 0x120);
        w64(&mut stub, 0x340, MAGIC_ORIG_ENTRY).unwrap();
        w64(&mut stub, 0x348, MAGIC_STUB_VADDR).unwrap();

        let (entry_off, mut expected_payload) = stub_payload(&stub).unwrap();
        let original_entry = r64(&init, 24).unwrap();
        let report = patch_init(&mut init, &stub).unwrap();
        let patched = Elf::parse(&init).unwrap();
        let injected = patched
            .program_headers
            .iter()
            .find(|phdr| phdr.p_type == PT_LOAD && phdr.p_offset >= PAGE)
            .unwrap();

        assert_eq!(report.stub_vaddr, injected.p_vaddr + entry_off as u64);
        patch_magics(&mut expected_payload, original_entry, report.stub_vaddr).unwrap();
        let start = injected.p_offset as usize;
        let end = start + expected_payload.len();
        assert_eq!(&init[start..end], expected_payload);
        assert_eq!(&init[end..end + PATCH_MARKER.len()], PATCH_MARKER);
        assert_eq!(
            injected.p_filesz as usize,
            expected_payload.len() + PATCH_MARKER.len()
        );
    }

    #[test]
    fn rejects_dynamic_stub_before_building_a_payload() {
        let mut stub = elf64(0x200100, 2, 512);
        set_load(&mut stub, 0, 0, 0x200000, 512, 512);
        let interp = 64 + PHDR64;
        w32(&mut stub[interp..interp + PHDR64], 0, PT_INTERP).unwrap();

        let error = stub_payload(&stub).unwrap_err();
        assert!(error.to_string().contains("PT_INTERP"));
    }

    #[test]
    fn rejects_static_pie_stub_with_dynamic_relocations() {
        let mut stub = elf64(0x200100, 2, 512);
        set_load(&mut stub, 0, 0, 0x200000, 512, 512);
        let dynamic = 64 + PHDR64;
        w32(&mut stub[dynamic..dynamic + PHDR64], 0, PT_DYNAMIC).unwrap();

        let error = stub_payload(&stub).unwrap_err();
        assert!(error.to_string().contains("PT_DYNAMIC"));
    }

    #[test]
    fn rejects_stub_entry_outside_executable_file_bytes() {
        let mut stub = elf64(0x200200, 1, 512);
        set_load(&mut stub, 0, 0, 0x200000, 0x180, 0x300);

        let error = stub_payload(&stub).unwrap_err();
        assert!(error.to_string().contains("executable PT_LOAD"));
    }
}
