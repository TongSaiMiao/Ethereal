use anyhow::{bail, ensure, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Raw,
    Gzip,
    Lz4,
    Lz4Legacy,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Raw => "raw",
            Format::Gzip => "gzip",
            Format::Lz4 => "lz4",
            Format::Lz4Legacy => "lz4_legacy",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s.trim() {
            "raw" => Format::Raw,
            "gzip" => Format::Gzip,
            "lz4" => Format::Lz4,
            "lz4_legacy" | "lz4legacy" => Format::Lz4Legacy,
            other => bail!("unknown ramdisk format: {other}"),
        })
    }
}

const LZ4_LEGACY_MAGIC: u32 = 0x184C2102;
const LZ4_FRAME_MAGIC: [u8; 4] = [0x04, 0x22, 0x4D, 0x18];
const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];
const CPIO_NEWC: &[u8] = b"070701";
const CPIO_CRC: &[u8] = b"070702";
const LZ4_LEGACY_BLOCK: usize = 8 * 1024 * 1024;
pub const MAX_DECOMPRESSED_SIZE: usize = 256 * 1024 * 1024;

fn read_decompressed_limited(reader: impl Read) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    reader
        .take((MAX_DECOMPRESSED_SIZE as u64) + 1)
        .read_to_end(&mut out)?;
    ensure!(
        out.len() <= MAX_DECOMPRESSED_SIZE,
        "ramdisk expands beyond {} MiB",
        MAX_DECOMPRESSED_SIZE / (1024 * 1024)
    );
    Ok(out)
}

pub fn detect(data: &[u8]) -> Format {
    if data.len() >= 2 && data[..2] == GZIP_MAGIC {
        return Format::Gzip;
    }
    if data.len() >= 4 && data[..4] == LZ4_FRAME_MAGIC {
        return Format::Lz4;
    }
    if data.len() >= 4 && u32::from_le_bytes(data[..4].try_into().unwrap()) == LZ4_LEGACY_MAGIC {
        return Format::Lz4Legacy;
    }
    if data.starts_with(CPIO_NEWC) || data.starts_with(CPIO_CRC) {
        return Format::Raw;
    }
    Format::Gzip
}

pub fn decompress(data: &[u8]) -> Result<(Vec<u8>, Format)> {
    let fmt = detect(data);
    let out = match fmt {
        Format::Raw => {
            ensure!(
                data.len() <= MAX_DECOMPRESSED_SIZE,
                "raw ramdisk exceeds {} MiB",
                MAX_DECOMPRESSED_SIZE / (1024 * 1024)
            );
            data.to_vec()
        }
        Format::Gzip => read_decompressed_limited(GzDecoder::new(data))?,
        Format::Lz4 => read_decompressed_limited(lz4_flex::frame::FrameDecoder::new(data))?,
        Format::Lz4Legacy => decompress_lz4_legacy(data)?,
    };
    Ok((out, fmt))
}

pub fn compress(data: &[u8], fmt: Format) -> Result<Vec<u8>> {
    match fmt {
        Format::Raw => Ok(data.to_vec()),
        Format::Gzip => {
            let mut e = GzEncoder::new(Vec::new(), Compression::default());
            e.write_all(data)?;
            Ok(e.finish()?)
        }
        Format::Lz4 => {
            let mut e = lz4_flex::frame::FrameEncoder::new(Vec::new());
            e.write_all(data)?;
            Ok(e.finish()?)
        }
        Format::Lz4Legacy => compress_lz4_legacy(data),
    }
}

fn decompress_lz4_legacy(input: &[u8]) -> Result<Vec<u8>> {
    if input.len() < 4 {
        bail!("lz4_legacy: truncated magic");
    }
    let magic = u32::from_le_bytes(input[..4].try_into().unwrap());
    if magic != LZ4_LEGACY_MAGIC {
        bail!("lz4_legacy: bad magic");
    }
    let mut off = 4;
    let mut out = Vec::new();
    while off + 4 <= input.len() {
        let sz = u32::from_le_bytes(input[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if sz == 0 {
            break;
        }
        if off + sz > input.len() {
            bail!("lz4_legacy: truncated block");
        }
        let block = &input[off..off + sz];
        off += sz;
        let mut dest = vec![0u8; LZ4_LEGACY_BLOCK];
        match lz4_flex::block::decompress_into(block, &mut dest) {
            Ok(n) => {
                ensure!(
                    out.len().saturating_add(n) <= MAX_DECOMPRESSED_SIZE,
                    "ramdisk expands beyond {} MiB",
                    MAX_DECOMPRESSED_SIZE / (1024 * 1024)
                );
                out.extend_from_slice(&dest[..n]);
            }
            Err(_) => {
                // some writers store an uncompressed block when compression expands
                ensure!(
                    out.len().saturating_add(block.len()) <= MAX_DECOMPRESSED_SIZE,
                    "ramdisk expands beyond {} MiB",
                    MAX_DECOMPRESSED_SIZE / (1024 * 1024)
                );
                out.extend_from_slice(block);
            }
        }
    }
    Ok(out)
}

fn compress_lz4_legacy(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::from(LZ4_LEGACY_MAGIC.to_le_bytes());
    for chunk in data.chunks(LZ4_LEGACY_BLOCK) {
        let compressed = lz4_flex::block::compress(chunk);
        out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        out.extend_from_slice(&compressed);
    }
    Ok(out)
}
