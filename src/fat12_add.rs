//! Root-directory FAT12 file insertion for boot smoke images.
//!
//! This is intentionally narrow: flat HDM FAT12, root directory only, and no
//! deletion or compaction. It exists to build multi-disk boot probes without
//! changing the source disk images.

use anyhow::{Result, bail};

use crate::fat12::Bpb;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootFileMeta {
    pub attr: u8,
    pub date: u16,
    pub time: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddReport {
    pub file_name: String,
    pub bytes: usize,
    pub clusters: usize,
    pub first_cluster: u16,
    pub root_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureReport {
    Added(AddReport),
    Reused { file_name: String, bytes: usize },
}

pub fn root_file_meta(disk: &[u8], file_name: &str) -> Result<RootFileMeta> {
    let bpb = Bpb::parse(disk)?;
    let entry = find_root_entry(disk, &bpb, file_name)?
        .ok_or_else(|| anyhow::anyhow!("file not found in root directory: {file_name}"))?;
    Ok(RootFileMeta {
        attr: entry.attr,
        date: entry.date,
        time: entry.time,
    })
}

pub fn root_file_exists(disk: &[u8], file_name: &str) -> Result<bool> {
    let bpb = Bpb::parse(disk)?;
    validate_layout(disk, &bpb)?;
    Ok(find_root_entry(disk, &bpb, file_name)?.is_some())
}

pub fn add_root_file(
    disk: &mut [u8],
    file_name: &str,
    data: &[u8],
    meta: RootFileMeta,
) -> Result<AddReport> {
    let bpb = Bpb::parse(disk)?;
    validate_layout(disk, &bpb)?;
    if find_root_entry(disk, &bpb, file_name)?.is_some() {
        bail!("file already exists in root directory: {file_name}");
    }

    let root_slot = find_free_root_slot(disk, &bpb)?;
    let clusters_needed = data.len().div_ceil(bpb.cluster_size() as usize);
    let clusters = allocate_clusters(disk, &bpb, clusters_needed)?;
    write_cluster_data(disk, &bpb, &clusters, data)?;
    write_root_entry(
        disk,
        &bpb,
        root_slot,
        file_name,
        data.len(),
        meta,
        &clusters,
    )?;

    Ok(AddReport {
        file_name: file_name.to_string(),
        bytes: data.len(),
        clusters: clusters.len(),
        first_cluster: clusters.first().copied().unwrap_or(0),
        root_index: root_slot,
    })
}

/// Add a root file, or reuse an existing byte-identical copy.
///
/// Multi-stage builds may independently require the same boot dependency. A
/// duplicate is safe only when its bytes already match the authoritative source;
/// a differing file remains a hard error so a stale intermediate cannot leak
/// into the final image.
pub fn ensure_root_file(
    disk: &mut [u8],
    file_name: &str,
    data: &[u8],
    meta: RootFileMeta,
) -> Result<EnsureReport> {
    let bpb = Bpb::parse(disk)?;
    validate_layout(disk, &bpb)?;
    if find_root_entry(disk, &bpb, file_name)?.is_some() {
        let existing = crate::read_fat12_file_from_hdm(disk, file_name)?;
        if existing != data {
            bail!(
                "existing root file differs from staged source: {file_name} ({} != {} bytes or content mismatch)",
                existing.len(),
                data.len()
            );
        }
        return Ok(EnsureReport::Reused {
            file_name: file_name.to_string(),
            bytes: data.len(),
        });
    }

    add_root_file(disk, file_name, data, meta).map(EnsureReport::Added)
}

/// Replace an existing root file with `data` of ANY size: free its current
/// cluster chain, allocate a fresh chain from free space (reusing the just-freed
/// clusters), write the data, and update the same directory entry (first cluster
/// + size). Unlike `fat12_replace::replace_file_in_place`, this can GROW the file
///
/// past its old allocation, which the Korean overlays need once relocation makes
/// them larger than the original.
pub fn replace_file_grow(disk: &mut [u8], file_name: &str, data: &[u8]) -> Result<AddReport> {
    let bpb = Bpb::parse(disk)?;
    validate_layout(disk, &bpb)?;

    let want = encode_83_name(file_name)?;
    let rd_off = root_dir_offset(&bpb);
    let mut found: Option<(usize, u16, RootFileMeta)> = None;
    for i in 0..bpb.root_dir_entries as usize {
        let off = rd_off + i * 32;
        let e = &disk[off..off + 32];
        match e[0] {
            0x00 => break,
            0xE5 => continue,
            _ => {}
        }
        // Skip volume-label entries (attr bit 0x08) -- their 11 raw name bytes could
        // otherwise collide with the encoded 8.3 name.
        if e[0x0B] & 0x08 != 0 {
            continue;
        }
        if e[0..11] == want {
            if e[0x0B] & 0x10 != 0 {
                bail!("{file_name} is a directory, not a file");
            }
            let first = u16::from_le_bytes([e[0x1A], e[0x1B]]);
            let size = u32::from_le_bytes([e[0x1C], e[0x1D], e[0x1E], e[0x1F]]);
            // A non-empty file must start at a real data cluster; a size-0 entry
            // with a stale first_cluster must NOT trigger a free-walk of whatever
            // junk it points at (that could zero another file's chain).
            if size > 0 && first < 2 {
                bail!("{file_name} has size {size} but first cluster {first} < 2 (corrupt entry)");
            }
            let meta = RootFileMeta {
                attr: e[0x0B],
                time: u16::from_le_bytes([e[0x16], e[0x17]]),
                date: u16::from_le_bytes([e[0x18], e[0x19]]),
            };
            found = Some((i, if size == 0 { 0 } else { first }, meta));
            break;
        }
    }
    let (root_index, first_cluster, meta) =
        found.ok_or_else(|| anyhow::anyhow!("file not found in root directory: {file_name}"))?;

    // Validate the existing chain (read-only) BEFORE destroying it: a cycle or an
    // out-of-range link means the entry is already corrupt, and freeing blindly
    // could cross-corrupt another file. Bail loudly instead.
    let max = max_data_cluster(&bpb);
    {
        let mut seen = std::collections::HashSet::new();
        let mut c = first_cluster;
        while c >= 2 && c <= max {
            if !seen.insert(c) {
                bail!("{file_name}'s cluster chain has a cycle at {c}; refusing to free");
            }
            let next = fat_get(disk, &bpb, c)?;
            if !(2..0xFF8).contains(&next) {
                break;
            }
            if next > max {
                bail!("{file_name}'s chain links to out-of-range cluster {next}; refusing to free");
            }
            c = next;
        }
    }
    // Free the whole existing chain (each cluster -> 0), so allocate_clusters can
    // reuse them for the new (possibly larger) chain.
    let mut c = first_cluster;
    while c >= 2 && c <= max {
        let next = fat_get(disk, &bpb, c)?;
        fat_set_all(disk, &bpb, c, 0)?;
        if !(2..0xFF8).contains(&next) {
            break;
        }
        c = next;
    }

    let clusters_needed = data.len().div_ceil(bpb.cluster_size() as usize);
    let clusters = allocate_clusters(disk, &bpb, clusters_needed)?;
    write_cluster_data(disk, &bpb, &clusters, data)?;
    write_root_entry(
        disk,
        &bpb,
        root_index,
        file_name,
        data.len(),
        meta,
        &clusters,
    )?;

    Ok(AddReport {
        file_name: file_name.to_string(),
        bytes: data.len(),
        clusters: clusters.len(),
        first_cluster: clusters.first().copied().unwrap_or(0),
        root_index,
    })
}

#[derive(Debug)]
struct RootEntry {
    attr: u8,
    date: u16,
    time: u16,
}

fn validate_layout(disk: &[u8], bpb: &Bpb) -> Result<()> {
    let need = bpb.total_sectors as usize * bpb.bytes_per_sector as usize;
    if need > disk.len() {
        bail!(
            "disk too small for BPB layout: need {} bytes, have {} bytes",
            need,
            disk.len()
        );
    }
    Ok(())
}

fn find_root_entry(disk: &[u8], bpb: &Bpb, file_name: &str) -> Result<Option<RootEntry>> {
    let want = encode_83_name(file_name)?;
    let rd_off = root_dir_offset(bpb);
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
        if entry[0..11] == want {
            let attr = entry[0x0B];
            if attr & 0x10 != 0 {
                bail!("{file_name} is a directory, not a file");
            }
            return Ok(Some(RootEntry {
                attr,
                time: u16::from_le_bytes([entry[0x16], entry[0x17]]),
                date: u16::from_le_bytes([entry[0x18], entry[0x19]]),
            }));
        }
    }
    Ok(None)
}

fn find_free_root_slot(disk: &[u8], bpb: &Bpb) -> Result<usize> {
    let rd_off = root_dir_offset(bpb);
    for i in 0..bpb.root_dir_entries as usize {
        let first = disk[rd_off + i * 32];
        if first == 0x00 || first == 0xE5 {
            return Ok(i);
        }
    }
    bail!("root directory has no free entries")
}

fn allocate_clusters(disk: &mut [u8], bpb: &Bpb, count: usize) -> Result<Vec<u16>> {
    if count == 0 {
        return Ok(Vec::new());
    }

    let max_cluster = max_data_cluster(bpb);
    let mut clusters = Vec::with_capacity(count);
    for cluster in 2..=max_cluster {
        if fat_get(disk, bpb, cluster)? == 0 {
            clusters.push(cluster);
            if clusters.len() == count {
                break;
            }
        }
    }
    if clusters.len() != count {
        bail!(
            "not enough free clusters: need {}, found {}",
            count,
            clusters.len()
        );
    }

    for (i, &cluster) in clusters.iter().enumerate() {
        let next = clusters.get(i + 1).copied().unwrap_or(0xFFF);
        fat_set_all(disk, bpb, cluster, next)?;
    }
    Ok(clusters)
}

fn write_cluster_data(disk: &mut [u8], bpb: &Bpb, clusters: &[u16], data: &[u8]) -> Result<()> {
    let cluster_size = bpb.cluster_size() as usize;
    let mut remaining = data;
    for &cluster in clusters {
        let off = bpb.cluster_to_sector(cluster) as usize * bpb.bytes_per_sector as usize;
        let end = off + cluster_size;
        if end > disk.len() {
            bail!("cluster {cluster:#X} outside disk image");
        }
        disk[off..end].fill(0);
        let n = remaining.len().min(cluster_size);
        disk[off..off + n].copy_from_slice(&remaining[..n]);
        remaining = &remaining[n..];
    }
    if !remaining.is_empty() {
        bail!("internal error: data not fully written");
    }
    Ok(())
}

fn write_root_entry(
    disk: &mut [u8],
    bpb: &Bpb,
    root_index: usize,
    file_name: &str,
    size: usize,
    meta: RootFileMeta,
    clusters: &[u16],
) -> Result<()> {
    let rd_off = root_dir_offset(bpb);
    let off = rd_off + root_index * 32;
    if off + 32 > disk.len() {
        bail!("root directory entry outside disk image");
    }

    let name = encode_83_name(file_name)?;
    let entry = &mut disk[off..off + 32];
    entry.fill(0);
    entry[0..11].copy_from_slice(&name);
    entry[0x0B] = meta.attr & !0x10;
    entry[0x16..0x18].copy_from_slice(&meta.time.to_le_bytes());
    entry[0x18..0x1A].copy_from_slice(&meta.date.to_le_bytes());
    entry[0x1A..0x1C].copy_from_slice(&clusters.first().copied().unwrap_or(0).to_le_bytes());
    entry[0x1C..0x20].copy_from_slice(&(size as u32).to_le_bytes());
    Ok(())
}

fn fat_get(disk: &[u8], bpb: &Bpb, cluster: u16) -> Result<u16> {
    let off = fat_entry_offset(bpb, 0, cluster);
    if off + 2 > disk.len() {
        bail!("FAT entry for cluster {cluster:#X} outside disk image");
    }
    let raw = u16::from_le_bytes([disk[off], disk[off + 1]]);
    Ok(if cluster & 1 != 0 {
        raw >> 4
    } else {
        raw & 0x0FFF
    })
}

fn fat_set_all(disk: &mut [u8], bpb: &Bpb, cluster: u16, value: u16) -> Result<()> {
    for fat_index in 0..bpb.num_fats as usize {
        let off = fat_entry_offset(bpb, fat_index, cluster);
        if off + 2 > disk.len() {
            bail!("FAT entry for cluster {cluster:#X} outside disk image");
        }
        let value = value & 0x0FFF;
        if cluster & 1 != 0 {
            disk[off] = (disk[off] & 0x0F) | ((value << 4) as u8 & 0xF0);
            disk[off + 1] = (value >> 4) as u8;
        } else {
            disk[off] = value as u8;
            disk[off + 1] = (disk[off + 1] & 0xF0) | ((value >> 8) as u8 & 0x0F);
        }
    }
    Ok(())
}

fn encode_83_name(file_name: &str) -> Result<[u8; 11]> {
    let (base, ext) = match file_name.rsplit_once('.') {
        Some((base, ext)) if !base.is_empty() && !ext.is_empty() => (base, ext),
        _ => (file_name, ""),
    };
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        bail!("not an 8.3 FAT name: {file_name}");
    }

    let mut out = [b' '; 11];
    copy_ascii_upper(base, &mut out[0..8], file_name)?;
    copy_ascii_upper(ext, &mut out[8..11], file_name)?;
    Ok(out)
}

fn copy_ascii_upper(src: &str, dst: &mut [u8], original: &str) -> Result<()> {
    for (i, byte) in src.bytes().enumerate() {
        if !byte.is_ascii() || byte == b' ' {
            bail!("unsupported FAT name byte in {original}");
        }
        dst[i] = byte.to_ascii_uppercase();
    }
    Ok(())
}

fn fat_entry_offset(bpb: &Bpb, fat_index: usize, cluster: u16) -> usize {
    let fat_sector = bpb.fat_start_sector() as usize + fat_index * bpb.sectors_per_fat as usize;
    fat_sector * bpb.bytes_per_sector as usize + cluster as usize + cluster as usize / 2
}

fn root_dir_offset(bpb: &Bpb) -> usize {
    bpb.root_dir_start_sector() as usize * bpb.bytes_per_sector as usize
}

fn max_data_cluster(bpb: &Bpb) -> u16 {
    let data_sectors = bpb.total_sectors as u32 - bpb.data_start_sector();
    let data_clusters = data_sectors / bpb.sectors_per_cluster as u32;
    data_clusters as u16 + 1
}
