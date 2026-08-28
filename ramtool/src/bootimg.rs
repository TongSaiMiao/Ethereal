use crate::compress::{self, Format};
use anyhow::{bail, ensure, Result};
use std::fs;
use std::path::{Path, PathBuf};

const BOOT_MAGIC: &[u8; 8] = b"ANDROID!";
const VENDOR_MAGIC: &[u8; 8] = b"VNDRBOOT";
const BOOT_V4_SIGNATURE_SIZE_OFF: usize = 0x62c;
const ETHEREAL_RDINIT: &str = "rdinit=/ethereal-init";
const AVB_FOOTER_MAGIC: &[u8; 4] = b"AVBf";
const AVB_FOOTER_SIZE: usize = 64;
const MAX_BOOT_IMAGE_SIZE: u64 = 512 * 1024 * 1024;

fn read_image(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    ensure!(metadata.is_file(), "boot image is not a regular file");
    ensure!(
        metadata.len() <= MAX_BOOT_IMAGE_SIZE,
        "boot image exceeds {} MiB",
        MAX_BOOT_IMAGE_SIZE / (1024 * 1024)
    );
    let data = fs::read(path)?;
    ensure!(
        data.len() as u64 <= MAX_BOOT_IMAGE_SIZE,
        "boot image grew beyond {} MiB while reading",
        MAX_BOOT_IMAGE_SIZE / (1024 * 1024)
    );
    Ok(data)
}

fn r32(data: &[u8], off: usize) -> Result<u32> {
    ensure!(off + 4 <= data.len(), "boot image truncated at {off}");
    Ok(u32::from_le_bytes(data[off..off + 4].try_into().unwrap()))
}

fn r64be(data: &[u8], off: usize) -> Result<u64> {
    ensure!(off + 8 <= data.len(), "image truncated at {off}");
    Ok(u64::from_be_bytes(data[off..off + 8].try_into().unwrap()))
}

/// vendor_boot v4 ramdisk fragment types (AOSP bootimg.h).
const VENDOR_TYPE_RECOVERY: u32 = 2;

fn w32(data: &mut [u8], off: usize, v: u32) -> Result<()> {
    ensure!(off + 4 <= data.len(), "boot header truncated at {off}");
    data[off..off + 4].copy_from_slice(&v.to_le_bytes());
    Ok(())
}

fn align(n: usize, page: usize) -> usize {
    if page == 0 {
        n
    } else {
        (n + page - 1) & !(page - 1)
    }
}

fn cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn valid_page(p: u32) -> bool {
    matches!(p, 2048 | 4096 | 8192 | 16384 | 32768 | 65536)
}

fn find_magic(data: &[u8]) -> Result<(usize, bool)> {
    ensure!(data.len() >= 8, "ramtool: boot image is too short");
    let last = data.len().saturating_sub(8);
    let search_to = last.min(64 * 1024);
    for i in 0..=search_to {
        if &data[i..i + 8] == BOOT_MAGIC {
            return Ok((i, false));
        }
        if &data[i..i + 8] == VENDOR_MAGIC {
            return Ok((i, true));
        }
    }
    bail!("ramtool: ANDROID! / VNDRBOOT magic not found");
}

struct Layout {
    magic_off: usize,
    vendor: bool,
    header_version: u32,
    page_size: usize,
    kernel_off: usize,
    kernel_size: usize,
    ramdisk_off: usize,
    ramdisk_size: usize,
    cmdline: String,
    cmdline_off: usize,
    cmdline_cap: usize,
}

struct VendorEntry {
    size: u32,
    offset: u32,
    kind: u32,
    name: String,
}

struct VendorV4 {
    header_size: usize,
    dtb_size: usize,
    table_size: usize,
    table_n: usize,
    table_entsz: usize,
    bootconfig_size: usize,
    ramdisk_off: usize,
    dtb_off: usize,
    table_off: usize,
    bootconfig_off: usize,
    entries: Vec<VendorEntry>,
}

fn parse_vendor_v4(data: &[u8], magic_off: usize, page: usize) -> Result<Option<VendorV4>> {
    let h = &data[magic_off..];
    let ver = r32(h, 8)?;
    if ver < 4 {
        return Ok(None);
    }
    let vendor_ramdisk_size = r32(h, 0x18)? as usize;
    let header_size = r32(h, 0x830)? as usize;
    let dtb_size = r32(h, 0x834)? as usize;
    let table_size = r32(h, 0x840).unwrap_or(0) as usize;
    let table_n = r32(h, 0x844).unwrap_or(0) as usize;
    let table_entsz = r32(h, 0x848).unwrap_or(0) as usize;
    let bootconfig_size = r32(h, 0x84c).unwrap_or(0) as usize;
    if table_n == 0 || table_entsz < 12 {
        return Ok(None);
    }
    let hdr_pages = align(header_size.max(1), page);
    let ramdisk_off = magic_off + hdr_pages;
    let dtb_off = ramdisk_off + align(vendor_ramdisk_size, page);
    let table_off = dtb_off + align(dtb_size, page);
    let bootconfig_off = table_off + align(table_size, page);
    let table_entries_size = table_n
        .checked_mul(table_entsz)
        .ok_or_else(|| anyhow::anyhow!("vendor ramdisk table size overflow"))?;
    let table_entries_end = table_off
        .checked_add(table_entries_size)
        .ok_or_else(|| anyhow::anyhow!("vendor ramdisk table range overflow"))?;
    ensure!(
        table_entries_end <= data.len(),
        "vendor ramdisk table truncated"
    );
    let mut entries = Vec::new();
    for i in 0..table_n {
        let eoff = table_off + i * table_entsz;
        let size = r32(data, eoff)?;
        let offset = r32(data, eoff + 4)?;
        let kind = r32(data, eoff + 8)?;
        let name = if table_entsz >= 44 {
            cstr(&data[eoff + 12..eoff + 44.min(table_entsz)])
        } else {
            String::new()
        };
        entries.push(VendorEntry {
            size,
            offset,
            kind,
            name,
        });
    }
    Ok(Some(VendorV4 {
        header_size,
        dtb_size,
        table_size,
        table_n,
        table_entsz,
        bootconfig_size,
        ramdisk_off,
        dtb_off,
        table_off,
        bootconfig_off,
        entries,
    }))
}

fn kind_name(kind: u32) -> &'static str {
    match kind {
        0 => "none",
        1 => "platform",
        2 => "recovery",
        3 => "dlkm",
        _ => "other",
    }
}

fn parse_layout(data: &[u8]) -> Result<Layout> {
    let (magic_off, vendor) = find_magic(data)?;
    let h = &data[magic_off..];
    if vendor {
        let header_version = r32(h, 8)?;
        let page_size = r32(h, 12)? as usize;
        ensure!(
            valid_page(page_size as u32),
            "vendor_boot: bad page_size {page_size}"
        );
        let vendor_ramdisk_size = r32(h, 0x18)? as usize;
        let header_size = r32(h, 0x830).unwrap_or(page_size as u32) as usize;
        let hdr_pages = align(header_size.max(1), page_size);
        let kernel_off = magic_off + hdr_pages;
        // vendor_boot: ramdisk is first payload (no kernel)
        return Ok(Layout {
            magic_off,
            vendor: true,
            header_version,
            page_size,
            kernel_off,
            kernel_size: 0,
            ramdisk_off: kernel_off,
            ramdisk_size: vendor_ramdisk_size,
            cmdline: cstr(&h[28..28 + 2048.min(h.len().saturating_sub(28))]),
            cmdline_off: 28,
            cmdline_cap: 2048,
        });
    }

    let ver_at_28 = r32(h, 0x28)?;
    let page_at_24 = r32(h, 0x24)?;
    if valid_page(page_at_24) && ver_at_28 <= 2 {
        ensure!(h.len() >= 0x40, "legacy boot header is truncated");
        let page_size = page_at_24 as usize;
        let kernel_size = r32(h, 0x08)? as usize;
        let ramdisk_size = r32(h, 0x10)? as usize;
        let kernel_off = magic_off + page_size;
        let ramdisk_off = kernel_off + align(kernel_size, page_size);
        return Ok(Layout {
            magic_off,
            vendor: false,
            header_version: ver_at_28,
            page_size,
            kernel_off,
            kernel_size,
            ramdisk_off,
            ramdisk_size,
            cmdline: cstr(&h[0x40..0x40 + 512.min(h.len().saturating_sub(0x40))]),
            cmdline_off: 0x40,
            cmdline_cap: 512,
        });
    }

    let header_version = ver_at_28;
    ensure!(
        header_version == 3 || header_version == 4,
        "unsupported ANDROID boot header version {header_version}"
    );
    let page_size = 4096usize;
    let kernel_size = r32(h, 0x08)? as usize;
    let ramdisk_size = r32(h, 0x0c)? as usize;
    let kernel_off = magic_off + page_size;
    let ramdisk_off = kernel_off + align(kernel_size, page_size);
    Ok(Layout {
        magic_off,
        vendor: false,
        header_version,
        page_size,
        kernel_off,
        kernel_size,
        ramdisk_off,
        ramdisk_size,
        cmdline: cstr(&h[0x2c..0x2c + 1536.min(h.len().saturating_sub(0x2c))]),
        cmdline_off: 0x2c,
        cmdline_cap: 1536,
    })
}

fn boot_v4_signature_size(data: &[u8], layout: &Layout) -> Result<usize> {
    if layout.vendor || layout.header_version != 4 {
        return Ok(0);
    }
    Ok(r32(&data[layout.magic_off..], BOOT_V4_SIGNATURE_SIZE_OFF)? as usize)
}

fn avb_tail_start(data: &[u8]) -> Result<Option<usize>> {
    if data.len() < AVB_FOOTER_SIZE {
        return Ok(None);
    }
    let footer_off = data.len() - AVB_FOOTER_SIZE;
    if &data[footer_off..footer_off + AVB_FOOTER_MAGIC.len()] != AVB_FOOTER_MAGIC {
        return Ok(None);
    }

    // AvbFooter is network byte order: magic, major, minor,
    // original_image_size, vbmeta_offset, vbmeta_size, reserved.
    let vbmeta_offset = usize::try_from(r64be(data, footer_off + 20)?)
        .map_err(|_| anyhow::anyhow!("AVB vbmeta offset does not fit usize"))?;
    let vbmeta_size = usize::try_from(r64be(data, footer_off + 28)?)
        .map_err(|_| anyhow::anyhow!("AVB vbmeta size does not fit usize"))?;
    let vbmeta_end = vbmeta_offset
        .checked_add(vbmeta_size)
        .ok_or_else(|| anyhow::anyhow!("AVB vbmeta range overflow"))?;
    ensure!(
        vbmeta_offset < footer_off && vbmeta_end <= footer_off,
        "invalid AVB footer vbmeta range {vbmeta_offset}..{vbmeta_end}"
    );
    Ok(Some(vbmeta_offset))
}

fn original_payload_end(data: &[u8], layout: &Layout, signature_size: usize) -> Result<usize> {
    let ramdisk_end = layout
        .ramdisk_off
        .checked_add(align(layout.ramdisk_size, layout.page_size))
        .ok_or_else(|| anyhow::anyhow!("boot ramdisk range overflow"))?;
    let end = ramdisk_end
        .checked_add(align(signature_size, layout.page_size))
        .ok_or_else(|| anyhow::anyhow!("boot signature range overflow"))?;
    ensure!(end <= data.len(), "boot image payload is truncated");
    Ok(end)
}

fn fixed_tail_start(data: &[u8], layout: &Layout, signature_size: usize) -> Result<usize> {
    let payload_end = original_payload_end(data, layout, signature_size)?;
    let avb_start = avb_tail_start(data)?;
    // Header v3/v4 has no payload after the boot signature. A full partition
    // dump commonly has zero padding but no AVB footer; that padding is safe
    // capacity for a larger compressed ramdisk. Preserve any non-zero unknown
    // tail instead of guessing what an OEM put there.
    let tail_start = avb_start.unwrap_or_else(|| {
        if layout.header_version >= 3 && data[payload_end..].iter().all(|byte| *byte == 0) {
            data.len()
        } else {
            payload_end
        }
    });
    ensure!(
        tail_start >= payload_end,
        "AVB metadata overlaps the boot payload ({tail_start} < {payload_end})"
    );
    Ok(tail_start)
}

pub fn append_rdinit(cmdline: &str) -> String {
    let parts: Vec<&str> = cmdline.split_whitespace().collect();
    if parts
        .iter()
        .any(|p| *p == ETHEREAL_RDINIT || p.starts_with("rdinit="))
    {
        parts
            .iter()
            .map(|p| {
                if p.starts_with("rdinit=") {
                    ETHEREAL_RDINIT
                } else {
                    *p
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else if cmdline.trim().is_empty() {
        ETHEREAL_RDINIT.to_string()
    } else {
        format!("{cmdline} {ETHEREAL_RDINIT}")
    }
}

fn gki2_cmdline_with_rdinit(cmdline: &str, cap: usize) -> Result<String> {
    let mut has_ethereal_rdinit = false;
    let mut rdinit_count = 0usize;
    for arg in cmdline.split_whitespace() {
        if arg.starts_with("rdinit=") {
            rdinit_count += 1;
            ensure!(
                arg == ETHEREAL_RDINIT,
                "boot cmdline already contains conflicting {arg}; refusing to replace it"
            );
            has_ethereal_rdinit = true;
        }
    }
    ensure!(
        rdinit_count <= 1,
        "boot cmdline contains multiple rdinit= parameters; use an unmodified stock image"
    );

    let next = if has_ethereal_rdinit {
        cmdline.to_string()
    } else if cmdline.trim().is_empty() {
        ETHEREAL_RDINIT.to_string()
    } else {
        format!("{cmdline} {ETHEREAL_RDINIT}")
    };
    ensure!(
        next.len() < cap,
        "boot cmdline with {ETHEREAL_RDINIT} is {} bytes, but the header allows at most {} bytes",
        next.len(),
        cap.saturating_sub(1)
    );
    Ok(next)
}

fn apply_cmdline_exact(
    image: &mut [u8],
    magic_off: usize,
    off: usize,
    cap: usize,
    cmdline: &str,
) -> Result<()> {
    let start = magic_off
        .checked_add(off)
        .ok_or_else(|| anyhow::anyhow!("boot cmdline offset overflow"))?;
    let end = start
        .checked_add(cap)
        .ok_or_else(|| anyhow::anyhow!("boot cmdline size overflow"))?;
    ensure!(end <= image.len(), "boot header cmdline field is truncated");
    ensure!(
        cmdline.len() < cap,
        "boot cmdline is {} bytes, but the header allows at most {} bytes",
        cmdline.len(),
        cap.saturating_sub(1)
    );

    image[start..end].fill(0);
    image[start..start + cmdline.len()].copy_from_slice(cmdline.as_bytes());
    Ok(())
}

/// Add Ethereal's rdinit to a GKI 2.0 kernel-only boot image without
/// reconstructing the image. Bytes outside the fixed cmdline field are kept
/// verbatim, including partition padding and AVB data.
pub fn patch_gki2_boot_cmdline_bytes(data: &[u8]) -> Result<Vec<u8>> {
    let layout = parse_layout(data)?;
    ensure!(!layout.vendor, "expected boot.img, got vendor_boot");
    ensure!(
        matches!(layout.header_version, 3 | 4),
        "GKI 2.0 boot cmdline patch requires boot header v3 or v4"
    );
    ensure!(
        layout.kernel_size > 0 && layout.ramdisk_size == 0,
        "GKI 2.0 boot cmdline patch requires a kernel-only boot image"
    );
    slice(data, layout.kernel_off, layout.kernel_size)?;

    if layout.header_version == 4 {
        let signature_size = boot_v4_signature_size(data, &layout)?;
        ensure!(
            signature_size == 0,
            "boot v4 has a {signature_size}-byte boot signature; changing cmdline requires re-signing"
        );
    }

    let cmdline = gki2_cmdline_with_rdinit(&layout.cmdline, layout.cmdline_cap)?;
    let mut patched = data.to_vec();
    apply_cmdline_exact(
        &mut patched,
        layout.magic_off,
        layout.cmdline_off,
        layout.cmdline_cap,
        &cmdline,
    )?;
    debug_assert_eq!(patched.len(), data.len());
    Ok(patched)
}

pub fn patch_gki2_boot_cmdline(orig: &Path, out: &Path) -> Result<()> {
    let original = read_image(orig)?;
    let patched = patch_gki2_boot_cmdline_bytes(&original)?;
    ensure!(
        patched.len() == original.len(),
        "internal error: cmdline patch changed boot image length"
    );
    fs::write(out, patched)?;
    println!("Patched GKI 2.0 boot cmdline to {}", out.display());
    Ok(())
}

fn apply_cmdline(header: &mut [u8], off: usize, cap: usize, cmdline: &str) {
    if off + cap > header.len() {
        return;
    }
    for b in header[off..off + cap].iter_mut() {
        *b = 0;
    }
    let bytes = cmdline.as_bytes();
    let n = bytes.len().min(cap.saturating_sub(1));
    header[off..off + n].copy_from_slice(&bytes[..n]);
}

fn slice(data: &[u8], off: usize, size: usize) -> Result<&[u8]> {
    if size == 0 {
        return Ok(&[]);
    }
    let end = off
        .checked_add(size)
        .ok_or_else(|| anyhow::anyhow!("payload range overflow"))?;
    ensure!(
        end <= data.len(),
        "payload truncated ({off}+{size} > {})",
        data.len()
    );
    Ok(&data[off..end])
}

pub fn unpack(img: &Path, dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    let data = read_image(img)?;
    let layout = parse_layout(&data)?;
    println!("Parsing boot image: [{}]", img.display());
    println!("HEADER_VER      [{}]", layout.header_version);
    println!(
        "IMAGE           [{}]",
        if layout.vendor {
            "vendor_boot"
        } else if layout.kernel_size == 0 {
            "init_boot (GKI 2.0 ramdisk)"
        } else if layout.ramdisk_size > 0 {
            "boot (GKI 1.0 kernel+ramdisk)"
        } else {
            "boot (GKI 2.0 kernel-only — patch init_boot)"
        }
    );
    println!("PAGE_SIZE       [{}]", layout.page_size);
    println!("KERNEL_SZ       [{}]", layout.kernel_size);
    println!("RAMDISK_SZ      [{}]", layout.ramdisk_size);
    if !layout.cmdline.is_empty() {
        println!("CMDLINE         [{}]", layout.cmdline);
    }

    if layout.kernel_size > 0 {
        fs::write(
            dir.join("kernel"),
            slice(&data, layout.kernel_off, layout.kernel_size)?,
        )?;
    }
    fs::write(dir.join("cmdline.txt"), layout.cmdline.as_bytes())?;
    fs::write(
        dir.join("cmdline.cap"),
        format!("{}", layout.cmdline_cap).as_bytes(),
    )?;

    if layout.vendor {
        // Keep an explicit type marker for every vendor_boot version. The v4
        // table parser replaces this with richer metadata below; v3 still
        // needs the marker so callers cannot mistake it for init_boot.
        fs::write(
            dir.join("vendor_meta.txt"),
            format!("header_version={}\n", layout.header_version),
        )?;
        if let Some(v) = parse_vendor_v4(&data, layout.magic_off, layout.page_size)? {
            return unpack_vendor_v4(&data, dir, &v);
        }
    }

    if layout.ramdisk_size == 0 {
        println!("RAMDISK         [empty]");
        println!(
            "NOTE: kernel-only boot.img (GKI 2.0). Cmdline can still take rdinit=/ethereal-init."
        );
        return Ok(());
    }

    let ramdisk_blob = slice(&data, layout.ramdisk_off, layout.ramdisk_size)?;
    let (plain, fmt) = compress::decompress(ramdisk_blob)?;
    println!("RAMDISK_FMT     [{}]", fmt.as_str());
    fs::write(dir.join("ramdisk.cpio"), &plain)?;
    fs::write(dir.join("ramdisk.fmt"), fmt.as_str().as_bytes())?;
    Ok(())
}

fn unpack_vendor_v4(data: &[u8], dir: &Path, v: &VendorV4) -> Result<()> {
    if v.dtb_size > 0 {
        fs::write(dir.join("vendor.dtb"), slice(data, v.dtb_off, v.dtb_size)?)?;
        println!("DTB_SZ          [{}]", v.dtb_size);
    }
    fs::write(
        dir.join("vendor_table.bin"),
        slice(data, v.table_off, v.table_size)?,
    )?;
    if v.bootconfig_size > 0 {
        fs::write(
            dir.join("vendor_bootconfig.bin"),
            slice(data, v.bootconfig_off, v.bootconfig_size)?,
        )?;
    }

    let mut primary_idx: Option<usize> = None;
    for (i, ent) in v.entries.iter().enumerate() {
        let blob = slice(data, v.ramdisk_off + ent.offset as usize, ent.size as usize)?;
        fs::write(dir.join(format!("vendor_frag.{i}.bin")), blob)?;
        let recovery = ent.kind == VENDOR_TYPE_RECOVERY;
        let tag = if recovery { "kept" } else { "patchable" };
        print!(
            "VENDOR_FRAG     [{i}] type={} ({}) name={} size={} [{tag}]",
            ent.kind,
            kind_name(ent.kind),
            if ent.name.is_empty() {
                "(unnamed)"
            } else {
                ent.name.as_str()
            },
            ent.size
        );
        if recovery {
            println!();
            continue;
        }
        match compress::decompress(blob) {
            Ok((plain, fmt)) => {
                println!(" fmt={}", fmt.as_str());
                if primary_idx.is_none() {
                    fs::write(dir.join("ramdisk.cpio"), &plain)?;
                    fs::write(dir.join("ramdisk.fmt"), fmt.as_str().as_bytes())?;
                    primary_idx = Some(i);
                    println!("RAMDISK_FMT     [{}] (vendor fragment {i})", fmt.as_str());
                }
            }
            Err(e) => println!(" decompress failed: {e}"),
        }
    }
    let meta = format!(
        "header_version=4\nprimary={}\nn={}\nentsz={}\n",
        primary_idx
            .map(|i| i.to_string())
            .unwrap_or_else(|| "-1".into()),
        v.table_n,
        v.table_entsz
    );
    fs::write(dir.join("vendor_meta.txt"), meta)?;
    if primary_idx.is_none() {
        println!("RAMDISK         [no platform fragment]");
    }
    Ok(())
}

fn load_optional(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn pad_to(buf: &mut Vec<u8>, page: usize) {
    let n = align(buf.len(), page);
    buf.resize(n, 0);
}

pub fn ramdisk_path(dir: &Path) -> PathBuf {
    dir.join("ramdisk.cpio")
}

pub fn load_ramdisk_fmt(dir: &Path) -> Result<Format> {
    match fs::read_to_string(dir.join("ramdisk.fmt")) {
        Ok(s) => Format::parse(&s),
        Err(_) => Ok(Format::Gzip),
    }
}

pub fn repack(orig: &Path, dir: &Path, out: &Path) -> Result<()> {
    let orig_data = read_image(orig)?;
    let layout = parse_layout(&orig_data)?;
    let page = layout.page_size;

    let kernel = match load_optional(&dir.join("kernel"))? {
        Some(k) => k,
        None => slice(&orig_data, layout.kernel_off, layout.kernel_size)?.to_vec(),
    };

    let ramdisk_plain = match load_optional(&dir.join("ramdisk.cpio"))? {
        Some(p) => p,
        None => {
            if layout.ramdisk_size == 0 {
                Vec::new()
            } else {
                let blob = slice(&orig_data, layout.ramdisk_off, layout.ramdisk_size)?;
                compress::decompress(blob)?.0
            }
        }
    };
    let fmt = load_ramdisk_fmt(dir)?;
    let ramdisk = if ramdisk_plain.is_empty() {
        Vec::new()
    } else {
        compress::compress(&ramdisk_plain, fmt)?
    };

    let cmdline = match load_optional(&dir.join("cmdline.txt"))? {
        Some(v) => String::from_utf8_lossy(&v)
            .trim_end_matches('\0')
            .to_string(),
        None => layout.cmdline.clone(),
    };

    let mut image = Vec::new();
    if layout.magic_off > 0 {
        image.extend_from_slice(&orig_data[..layout.magic_off]);
    }

    if layout.vendor {
        if let Some(v) = parse_vendor_v4(&orig_data, layout.magic_off, page)? {
            return repack_vendor_v4(&orig_data, dir, out, &layout, &v, &cmdline);
        }
        let header_len = align(
            r32(&orig_data[layout.magic_off..], 2096).unwrap_or(page as u32) as usize,
            page,
        );
        let mut header = orig_data[layout.magic_off..layout.magic_off + header_len].to_vec();
        w32(&mut header, 24, ramdisk.len() as u32)?;
        apply_cmdline(
            &mut header,
            layout.cmdline_off,
            layout.cmdline_cap,
            &cmdline,
        );
        image.extend_from_slice(&header);
        pad_to(&mut image, page);
        image.extend_from_slice(&ramdisk);
        pad_to(&mut image, page);
        let dtb_off = layout.ramdisk_off + align(layout.ramdisk_size, page);
        if dtb_off < orig_data.len() {
            image.extend_from_slice(&orig_data[dtb_off..]);
        }
        fs::write(out, &image)?;
        println!("Repack to {}", out.display());
        return Ok(());
    }

    let signature_size = boot_v4_signature_size(&orig_data, &layout)?;
    ensure!(
        signature_size == 0,
        "boot v4 has a {signature_size}-byte boot signature; repacking requires re-signing"
    );
    let tail_start = fixed_tail_start(&orig_data, &layout, signature_size)?;

    let header_len = if layout.header_version <= 2 {
        page
    } else {
        4096
    };
    let mut header = orig_data[layout.magic_off..layout.magic_off + header_len].to_vec();
    if layout.header_version <= 2 {
        w32(&mut header, 0x08, kernel.len() as u32)?;
        w32(&mut header, 0x10, ramdisk.len() as u32)?;
    } else {
        w32(&mut header, 0x08, kernel.len() as u32)?;
        w32(&mut header, 0x0c, ramdisk.len() as u32)?;
    }
    apply_cmdline(
        &mut header,
        layout.cmdline_off,
        layout.cmdline_cap,
        &cmdline,
    );
    image.extend_from_slice(&header);
    pad_to(&mut image, page);
    image.extend_from_slice(&kernel);
    pad_to(&mut image, page);
    image.extend_from_slice(&ramdisk);
    pad_to(&mut image, page);
    ensure!(
        image.len() <= tail_start,
        "repacked boot body is {} bytes, but only {tail_start} bytes are available before the fixed tail",
        image.len()
    );
    let mut fixed = orig_data;
    // Do not retain a previous patched ramdisk (and its old authentication
    // token) in unused body slack. The AVB/vbmeta tail starts at tail_start and
    // remains byte-for-byte untouched.
    fixed[..tail_start].fill(0);
    fixed[..image.len()].copy_from_slice(&image);
    fs::write(out, &fixed)?;
    println!("Repack to {}", out.display());
    Ok(())
}

fn primary_index(dir: &Path, v: &VendorV4) -> usize {
    if let Ok(s) = fs::read_to_string(dir.join("vendor_meta.txt")) {
        for line in s.lines() {
            if let Some(p) = line.strip_prefix("primary=") {
                if let Ok(i) = p.trim().parse::<usize>() {
                    if i < v.entries.len() {
                        return i;
                    }
                }
            }
        }
    }
    v.entries
        .iter()
        .position(|e| e.kind != VENDOR_TYPE_RECOVERY)
        .unwrap_or(0)
}

fn repack_vendor_v4(
    orig_data: &[u8],
    dir: &Path,
    out: &Path,
    layout: &Layout,
    v: &VendorV4,
    cmdline: &str,
) -> Result<()> {
    let page = layout.page_size;
    let primary = primary_index(dir, v);
    let ramdisk_plain = load_optional(&dir.join("ramdisk.cpio"))?;
    let fmt = load_ramdisk_fmt(dir)?;

    let mut fragments: Vec<Vec<u8>> = Vec::new();
    let mut replaced = false;
    for (i, ent) in v.entries.iter().enumerate() {
        let use_patched = ramdisk_plain.is_some() && i == primary && !replaced;
        let blob = if use_patched {
            replaced = true;
            compress::compress(ramdisk_plain.as_ref().unwrap(), fmt)?
        } else if let Some(saved) = load_optional(&dir.join(format!("vendor_frag.{i}.bin")))? {
            saved
        } else {
            slice(
                orig_data,
                v.ramdisk_off + ent.offset as usize,
                ent.size as usize,
            )?
            .to_vec()
        };
        println!(
            "VENDOR_FRAG     [{i}] type={} ({}) size={} [{}]",
            ent.kind,
            kind_name(ent.kind),
            blob.len(),
            if use_patched {
                "recompressed"
            } else {
                "original"
            }
        );
        fragments.push(blob);
    }

    let mut ramdisk_section = Vec::new();
    let mut table = slice(orig_data, v.table_off, v.table_size)?.to_vec();
    let mut off = 0u32;
    for (i, blob) in fragments.iter().enumerate() {
        let eoff = i * v.table_entsz;
        w32(&mut table, eoff, blob.len() as u32)?;
        w32(&mut table, eoff + 4, off)?;
        ramdisk_section.extend_from_slice(blob);
        off += blob.len() as u32;
    }

    let header_len = align(v.header_size.max(1), page);
    let mut header = orig_data[layout.magic_off..layout.magic_off + header_len].to_vec();
    w32(&mut header, 0x18, ramdisk_section.len() as u32)?;
    apply_cmdline(&mut header, layout.cmdline_off, layout.cmdline_cap, cmdline);

    let dtb = match load_optional(&dir.join("vendor.dtb"))? {
        Some(d) => d,
        None => slice(orig_data, v.dtb_off, v.dtb_size)?.to_vec(),
    };
    w32(&mut header, 0x834, dtb.len() as u32)?;

    let bootconfig = match load_optional(&dir.join("vendor_bootconfig.bin"))? {
        Some(b) => b,
        None if v.bootconfig_size > 0 => {
            slice(orig_data, v.bootconfig_off, v.bootconfig_size)?.to_vec()
        }
        None => Vec::new(),
    };
    w32(&mut header, 0x84c, bootconfig.len() as u32)?;
    w32(&mut header, 0x840, table.len() as u32)?;
    w32(&mut header, 0x844, v.table_n as u32)?;
    w32(&mut header, 0x848, v.table_entsz as u32)?;

    let mut image = Vec::new();
    if layout.magic_off > 0 {
        image.extend_from_slice(&orig_data[..layout.magic_off]);
    }
    image.extend_from_slice(&header);
    pad_to(&mut image, page);
    image.extend_from_slice(&ramdisk_section);
    pad_to(&mut image, page);
    image.extend_from_slice(&dtb);
    pad_to(&mut image, page);
    image.extend_from_slice(&table);
    pad_to(&mut image, page);
    if !bootconfig.is_empty() {
        image.extend_from_slice(&bootconfig);
        pad_to(&mut image, page);
    }
    if image.len() < orig_data.len() {
        image.resize(orig_data.len(), 0);
    }
    fs::write(out, &image)?;
    println!("Repack to {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn kernel_only_boot(version: u32, cmdline: &str) -> Vec<u8> {
        assert!(matches!(version, 3 | 4));
        assert!(cmdline.len() < 1536);
        let mut image = vec![0u8; 3 * 4096];
        image[..8].copy_from_slice(BOOT_MAGIC);
        image[0x08..0x0c].copy_from_slice(&64u32.to_le_bytes());
        image[0x0c..0x10].copy_from_slice(&0u32.to_le_bytes());
        image[0x14..0x18]
            .copy_from_slice(&(if version == 4 { 1584u32 } else { 1580u32 }).to_le_bytes());
        image[0x28..0x2c].copy_from_slice(&version.to_le_bytes());
        image[0x2c..0x2c + cmdline.len()].copy_from_slice(cmdline.as_bytes());
        image
    }

    fn unique_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ethereal-ramtool-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x6d2b_79f5u32;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    fn boot_with_ramdisk(version: u32, kernel: &[u8], ramdisk_plain: &[u8]) -> Vec<u8> {
        let ramdisk = compress::compress(ramdisk_plain, Format::Gzip).unwrap();
        let mut header = vec![0u8; 4096];
        header[..8].copy_from_slice(BOOT_MAGIC);
        header[0x08..0x0c].copy_from_slice(&(kernel.len() as u32).to_le_bytes());
        header[0x0c..0x10].copy_from_slice(&(ramdisk.len() as u32).to_le_bytes());
        header[0x14..0x18]
            .copy_from_slice(&(if version == 4 { 1584u32 } else { 1580u32 }).to_le_bytes());
        header[0x28..0x2c].copy_from_slice(&version.to_le_bytes());
        let mut image = header;
        image.extend_from_slice(kernel);
        pad_to(&mut image, 4096);
        image.extend_from_slice(&ramdisk);
        pad_to(&mut image, 4096);
        image
    }

    fn add_avb_tail(body: &[u8], slack: usize) -> (Vec<u8>, usize) {
        let vbmeta_offset = align(body.len() + slack, 4096);
        let mut image = vec![0xa5; vbmeta_offset + 4096];
        image[..body.len()].copy_from_slice(body);
        image[vbmeta_offset..vbmeta_offset + 4].copy_from_slice(b"AVB0");
        let footer = image.len() - AVB_FOOTER_SIZE;
        image[footer..footer + 4].copy_from_slice(AVB_FOOTER_MAGIC);
        image[footer + 4..footer + 8].copy_from_slice(&1u32.to_be_bytes());
        image[footer + 8..footer + 12].copy_from_slice(&0u32.to_be_bytes());
        image[footer + 12..footer + 20].copy_from_slice(&(body.len() as u64).to_be_bytes());
        image[footer + 20..footer + 28].copy_from_slice(&(vbmeta_offset as u64).to_be_bytes());
        image[footer + 28..footer + 36].copy_from_slice(&256u64.to_be_bytes());
        (image, vbmeta_offset)
    }

    fn assert_ramdisk_repack_preserves_fixed_tail(version: u32, kernel: &[u8], label: &str) {
        let dir = unique_dir(label);
        let unpacked = dir.join("unpacked");
        fs::create_dir_all(&unpacked).unwrap();
        let input = dir.join("input.img");
        let output = dir.join("output.img");
        let body = boot_with_ramdisk(version, kernel, b"stock ramdisk");
        let (original, tail_start) = add_avb_tail(&body, 256 * 1024);
        let replacement = noise(64 * 1024);
        fs::write(&input, &original).unwrap();
        fs::write(unpacked.join("ramdisk.cpio"), &replacement).unwrap();
        fs::write(unpacked.join("ramdisk.fmt"), b"gzip").unwrap();

        repack(&input, &unpacked, &output).unwrap();
        let patched = fs::read(&output).unwrap();
        assert_eq!(patched.len(), original.len());
        assert_eq!(&patched[tail_start..], &original[tail_start..]);
        let layout = parse_layout(&patched).unwrap();
        assert_eq!(layout.kernel_size, kernel.len());
        assert_eq!(
            slice(&patched, layout.kernel_off, layout.kernel_size).unwrap(),
            kernel
        );
        let blob = slice(&patched, layout.ramdisk_off, layout.ramdisk_size).unwrap();
        assert_eq!(compress::decompress(blob).unwrap().0, replacement);
        let patched_payload_end = original_payload_end(&patched, &layout, 0).unwrap();
        assert!(patched[patched_payload_end..tail_start]
            .iter()
            .all(|byte| *byte == 0));

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repack_init_boot_preserves_length_and_avb_tail() {
        assert_ramdisk_repack_preserves_fixed_tail(4, &[], "init-boot-fixed-tail");
    }

    #[test]
    fn repack_gki1_boot_preserves_length_and_avb_tail() {
        assert_ramdisk_repack_preserves_fixed_tail(3, &noise(8192), "gki1-fixed-tail");
    }

    #[test]
    fn repack_uses_verified_zero_partition_padding_without_avb() {
        let dir = unique_dir("zero-partition-padding");
        let unpacked = dir.join("unpacked");
        fs::create_dir_all(&unpacked).unwrap();
        let input = dir.join("input.img");
        let output = dir.join("output.img");
        let mut original = boot_with_ramdisk(3, &noise(8192), b"tiny");
        original.resize(original.len() + 256 * 1024, 0);
        let replacement = noise(64 * 1024);
        fs::write(&input, &original).unwrap();
        fs::write(unpacked.join("ramdisk.cpio"), &replacement).unwrap();
        fs::write(unpacked.join("ramdisk.fmt"), b"gzip").unwrap();

        repack(&input, &unpacked, &output).unwrap();
        let patched = fs::read(&output).unwrap();
        assert_eq!(patched.len(), original.len());
        let layout = parse_layout(&patched).unwrap();
        let blob = slice(&patched, layout.ramdisk_off, layout.ramdisk_size).unwrap();
        assert_eq!(compress::decompress(blob).unwrap().0, replacement);

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repack_rejects_ramdisk_that_exceeds_fixed_body_capacity() {
        let dir = unique_dir("fixed-tail-overflow");
        let unpacked = dir.join("unpacked");
        fs::create_dir_all(&unpacked).unwrap();
        let input = dir.join("input.img");
        let output = dir.join("output.img");
        let original = boot_with_ramdisk(4, &[], b"tiny");
        fs::write(&input, &original).unwrap();
        fs::write(unpacked.join("ramdisk.cpio"), noise(64 * 1024)).unwrap();
        fs::write(unpacked.join("ramdisk.fmt"), b"gzip").unwrap();

        let error = repack(&input, &unpacked, &output).unwrap_err();
        assert!(error
            .to_string()
            .contains("available before the fixed tail"));
        assert!(!output.exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn repack_rejects_v4_boot_signature() {
        let dir = unique_dir("signed-v4-repack");
        let unpacked = dir.join("unpacked");
        fs::create_dir_all(&unpacked).unwrap();
        let input = dir.join("input.img");
        let output = dir.join("output.img");
        let mut body = boot_with_ramdisk(4, &[], b"stock");
        body[BOOT_V4_SIGNATURE_SIZE_OFF..BOOT_V4_SIGNATURE_SIZE_OFF + 4]
            .copy_from_slice(&4096u32.to_le_bytes());
        body.extend_from_slice(&vec![0x5a; 4096]);
        let (original, _) = add_avb_tail(&body, 4096);
        fs::write(&input, original).unwrap();
        fs::write(unpacked.join("ramdisk.cpio"), b"replacement").unwrap();
        fs::write(unpacked.join("ramdisk.fmt"), b"gzip").unwrap();

        let error = repack(&input, &unpacked, &output).unwrap_err();
        assert!(error.to_string().contains("repacking requires re-signing"));
        assert!(!output.exists());

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gki2_cmdline_patch_preserves_tail_and_all_non_cmdline_bytes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ethereal-ramtool-gki2-cmdline-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let input = dir.join("boot.img");
        let output = dir.join("patched-boot.img");

        let mut original = kernel_only_boot(4, "console=ttyS0");
        original[4096..4160].fill(0x5a);
        let footer = original.len() - 64;
        original[footer..footer + 4].copy_from_slice(b"AVBf");
        original[footer + 4..].fill(0xa5);
        fs::write(&input, &original).unwrap();

        patch_gki2_boot_cmdline(&input, &output).unwrap();
        let patched = fs::read(&output).unwrap();
        assert_eq!(patched.len(), original.len());
        assert_eq!(
            cstr(&patched[0x2c..0x2c + 1536]),
            "console=ttyS0 rdinit=/ethereal-init"
        );
        for i in 0..original.len() {
            if !(0x2c..0x2c + 1536).contains(&i) {
                assert_eq!(patched[i], original[i], "byte changed at offset {i:#x}");
            }
        }
        assert_eq!(&patched[footer..footer + 4], b"AVBf");

        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn gki2_cmdline_patch_accepts_v3_and_is_idempotent() {
        let image = kernel_only_boot(3, "quiet");
        let patched = patch_gki2_boot_cmdline_bytes(&image).unwrap();
        assert_eq!(
            cstr(&patched[0x2c..0x2c + 1536]),
            "quiet rdinit=/ethereal-init"
        );

        let patched_again = patch_gki2_boot_cmdline_bytes(&patched).unwrap();
        assert_eq!(patched_again, patched);
    }

    #[test]
    fn gki2_cmdline_patch_rejects_conflicting_rdinit() {
        let image = kernel_only_boot(3, "console=ttyS0 rdinit=/init");
        let error = patch_gki2_boot_cmdline_bytes(&image).unwrap_err();
        assert!(error.to_string().contains("conflicting rdinit=/init"));
    }

    #[test]
    fn gki2_cmdline_patch_rejects_duplicate_ethereal_rdinit() {
        let image = kernel_only_boot(
            3,
            "rdinit=/ethereal-init console=ttyS0 rdinit=/ethereal-init",
        );
        let error = patch_gki2_boot_cmdline_bytes(&image).unwrap_err();
        assert!(error.to_string().contains("multiple rdinit="));
    }

    #[test]
    fn gki2_cmdline_patch_rejects_overlong_cmdline() {
        let image = kernel_only_boot(3, &"a".repeat(1520));
        let error = patch_gki2_boot_cmdline_bytes(&image).unwrap_err();
        assert!(error
            .to_string()
            .contains("header allows at most 1535 bytes"));
    }

    #[test]
    fn gki2_cmdline_patch_rejects_v4_boot_signature() {
        let mut image = kernel_only_boot(4, "console=ttyS0");
        image[BOOT_V4_SIGNATURE_SIZE_OFF..BOOT_V4_SIGNATURE_SIZE_OFF + 4]
            .copy_from_slice(&4096u32.to_le_bytes());
        let error = patch_gki2_boot_cmdline_bytes(&image).unwrap_err();
        assert!(error
            .to_string()
            .contains("changing cmdline requires re-signing"));
    }

    #[test]
    fn gki2_cmdline_patch_rejects_non_kernel_only_images() {
        let mut init_boot = kernel_only_boot(4, "");
        init_boot[0x08..0x0c].copy_from_slice(&0u32.to_le_bytes());
        init_boot[0x0c..0x10].copy_from_slice(&64u32.to_le_bytes());
        let error = patch_gki2_boot_cmdline_bytes(&init_boot).unwrap_err();
        assert!(error.to_string().contains("kernel-only boot image"));

        let mut boot_with_ramdisk = kernel_only_boot(3, "");
        boot_with_ramdisk[0x0c..0x10].copy_from_slice(&64u32.to_le_bytes());
        let error = patch_gki2_boot_cmdline_bytes(&boot_with_ramdisk).unwrap_err();
        assert!(error.to_string().contains("kernel-only boot image"));
    }

    #[test]
    fn vendor_v3_unpack_writes_type_marker() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "ethereal-ramtool-vendor-v3-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).unwrap();
        let image = dir.join("vendor_boot.img");
        let unpacked = dir.join("unpacked");

        let mut header = vec![0u8; 4096];
        header[..8].copy_from_slice(VENDOR_MAGIC);
        header[8..12].copy_from_slice(&3u32.to_le_bytes());
        header[12..16].copy_from_slice(&4096u32.to_le_bytes());
        header[0x830..0x834].copy_from_slice(&2112u32.to_le_bytes());
        fs::write(&image, header).unwrap();

        unpack(&image, &unpacked).unwrap();
        assert_eq!(
            fs::read_to_string(unpacked.join("vendor_meta.txt")).unwrap(),
            "header_version=3\n"
        );

        fs::remove_dir_all(dir).unwrap();
    }
}
