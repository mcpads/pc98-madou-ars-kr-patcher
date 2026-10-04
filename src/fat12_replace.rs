//! In-place replacement for a root-directory FAT12 file.
//!
//! This deliberately does not reallocate clusters yet. It is the smallest media
//! gate needed for length-preserving visible-text PoCs: write bytes into an
//! already allocated file chain and update the root directory size.

use anyhow::{Result, bail};

use crate::fat12::Bpb;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceReport {
    pub file_name: String,
    pub bytes: usize,
    pub capacity: usize,
    pub clusters: usize,
}

pub fn replace_file_in_place(
    disk: &mut [u8],
    file_name: &str,
    data: &[u8],
) -> Result<ReplaceReport> {
    let bpb = Bpb::parse(disk)?;
    let entry = find_root_entry(disk, &bpb, file_name)?;
    let chain = cluster_chain(disk, &bpb, entry.first_cluster)?;
    let capacity = chain.len() * bpb.cluster_size() as usize;
    if data.len() > capacity {
        bail!(
            "replacement data is {} bytes but {} only has {} allocated bytes ({} clusters)",
            data.len(),
            file_name,
            capacity,
            chain.len()
        );
    }

    write_chain_data(disk, &bpb, &chain, data)?;
    disk[entry.dir_offset + 0x1C..entry.dir_offset + 0x20]
        .copy_from_slice(&(data.len() as u32).to_le_bytes());

    Ok(ReplaceReport {
        file_name: file_name.to_string(),
        bytes: data.len(),
        capacity,
        clusters: chain.len(),
    })
}

#[derive(Debug)]
struct RootEntry {
    dir_offset: usize,
    first_cluster: u16,
}

fn find_root_entry(disk: &[u8], bpb: &Bpb, file_name: &str) -> Result<RootEntry> {
    let rd_off = bpb.root_dir_start_sector() as usize * bpb.bytes_per_sector as usize;
    let rd_len = bpb.root_dir_entries as usize * 32;
    if rd_off + rd_len > disk.len() {
        bail!("root directory outside disk image");
    }

    for i in 0..bpb.root_dir_entries as usize {
        let off = rd_off + i * 32;
        let entry = &disk[off..off + 32];
        match entry[0] {
            0x00 => break,
            0xE5 => continue,
            _ => {}
        }
        let attr = entry[0x0B];
        if attr & 0x08 != 0 {
            continue;
        }
        let name = fat_name(entry);
        if name.eq_ignore_ascii_case(file_name) {
            if attr & 0x10 != 0 {
                bail!("{file_name} is a directory, not a file");
            }
            let first_cluster = u16::from_le_bytes([entry[0x1A], entry[0x1B]]);
            let size = u32::from_le_bytes([entry[0x1C], entry[0x1D], entry[0x1E], entry[0x1F]]);
            if size > 0 && first_cluster < 2 {
                bail!("{file_name} has invalid first cluster {first_cluster:#X}");
            }
            return Ok(RootEntry {
                dir_offset: off,
                first_cluster,
            });
        }
    }
    bail!("file not found in root directory: {file_name}")
}

fn cluster_chain(disk: &[u8], bpb: &Bpb, first_cluster: u16) -> Result<Vec<u16>> {
    let mut out = Vec::new();
    let mut cluster = first_cluster;
    let mut visited = std::collections::HashSet::new();
    loop {
        if !(2..0xFF8).contains(&cluster) {
            bail!("FAT chain broken at cluster {cluster:#X}");
        }
        if !visited.insert(cluster) {
            bail!("FAT chain cycle at cluster {cluster:#X}");
        }
        out.push(cluster);
        let next = fat_get(disk, bpb, cluster)?;
        if next >= 0xFF8 {
            break;
        }
        cluster = next;
    }
    Ok(out)
}

fn fat_get(disk: &[u8], bpb: &Bpb, cluster: u16) -> Result<u16> {
    let pos = cluster as usize + cluster as usize / 2;
    let fat_off = bpb.fat_start_sector() as usize * bpb.bytes_per_sector as usize;
    if fat_off + pos + 2 > disk.len() {
        bail!("FAT entry for cluster {cluster:#X} outside disk image");
    }
    let raw = u16::from_le_bytes([disk[fat_off + pos], disk[fat_off + pos + 1]]);
    Ok(if cluster & 1 != 0 {
        raw >> 4
    } else {
        raw & 0x0FFF
    })
}

fn write_chain_data(disk: &mut [u8], bpb: &Bpb, chain: &[u16], data: &[u8]) -> Result<()> {
    let cluster_size = bpb.cluster_size() as usize;
    let mut remaining = data;
    for &cluster in chain {
        if remaining.is_empty() {
            break;
        }
        let sec = bpb.cluster_to_sector(cluster) as usize;
        let off = sec * bpb.bytes_per_sector as usize;
        let end = off + cluster_size;
        if end > disk.len() {
            bail!("cluster {cluster:#X} outside disk image");
        }
        let n = remaining.len().min(cluster_size);
        disk[off..off + n].copy_from_slice(&remaining[..n]);
        remaining = &remaining[n..];
    }
    if !remaining.is_empty() {
        bail!("internal error: replacement data not fully written");
    }
    Ok(())
}

fn fat_name(entry: &[u8]) -> String {
    let name8 = trim_ascii(&entry[0..8]);
    let ext3 = trim_ascii(&entry[8..11]);
    if ext3.is_empty() {
        name8
    } else {
        format!("{name8}.{ext3}")
    }
}

fn trim_ascii(bytes: &[u8]) -> String {
    let trimmed = bytes
        .iter()
        .rposition(|&b| b != 0x20 && b != 0x00)
        .map(|i| &bytes[..=i])
        .unwrap_or(&[]);
    String::from_utf8_lossy(trimmed).into_owned()
}
