//! Read-only FAT12 support for the sector-linear A.R.S HDM images.

use std::collections::HashSet;

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bpb {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub root_dir_entries: u16,
    pub total_sectors: u16,
    pub media_descriptor: u8,
    pub sectors_per_fat: u16,
}

impl Bpb {
    pub fn parse(boot_sector: &[u8]) -> Result<Self> {
        if boot_sector.len() < 0x18 {
            bail!(
                "boot sector is too short for a FAT12 BPB: {} bytes",
                boot_sector.len()
            );
        }
        if !matches!(boot_sector[0], 0xE9 | 0xEB) {
            bail!(
                "not a FAT12 boot sector: jump byte 0x{:02X}",
                boot_sector[0]
            );
        }

        let bpb = Self {
            bytes_per_sector: u16::from_le_bytes([boot_sector[0x0B], boot_sector[0x0C]]),
            sectors_per_cluster: boot_sector[0x0D],
            reserved_sectors: u16::from_le_bytes([boot_sector[0x0E], boot_sector[0x0F]]),
            num_fats: boot_sector[0x10],
            root_dir_entries: u16::from_le_bytes([boot_sector[0x11], boot_sector[0x12]]),
            total_sectors: u16::from_le_bytes([boot_sector[0x13], boot_sector[0x14]]),
            media_descriptor: boot_sector[0x15],
            sectors_per_fat: u16::from_le_bytes([boot_sector[0x16], boot_sector[0x17]]),
        };

        if bpb.bytes_per_sector == 0 || !bpb.bytes_per_sector.is_power_of_two() {
            bail!("invalid FAT12 bytes_per_sector={}", bpb.bytes_per_sector);
        }
        if bpb.sectors_per_cluster == 0 || !bpb.sectors_per_cluster.is_power_of_two() {
            bail!(
                "invalid FAT12 sectors_per_cluster={}",
                bpb.sectors_per_cluster
            );
        }
        if !(1..=2).contains(&bpb.num_fats) {
            bail!("invalid FAT12 num_fats={}", bpb.num_fats);
        }
        if bpb.reserved_sectors == 0 {
            bail!("invalid FAT12 reserved_sectors=0");
        }
        if bpb.root_dir_entries == 0 {
            bail!("invalid FAT12 root_dir_entries=0");
        }
        if bpb.total_sectors == 0 {
            bail!("invalid FAT12 total_sectors=0");
        }
        if bpb.sectors_per_fat == 0 {
            bail!("invalid FAT12 sectors_per_fat=0");
        }
        if bpb.data_start_sector() >= u32::from(bpb.total_sectors) {
            bail!("FAT12 metadata consumes the declared volume");
        }

        let data_sectors = u32::from(bpb.total_sectors) - bpb.data_start_sector();
        let data_clusters = data_sectors / u32::from(bpb.sectors_per_cluster);
        if data_clusters >= 4085 {
            bail!("declared volume has {data_clusters} clusters and is not FAT12");
        }

        Ok(bpb)
    }

    pub const fn fat_start_sector(self) -> u32 {
        self.reserved_sectors as u32
    }

    pub const fn root_dir_start_sector(self) -> u32 {
        self.reserved_sectors as u32 + self.num_fats as u32 * self.sectors_per_fat as u32
    }

    pub const fn root_dir_sectors(self) -> u32 {
        (self.root_dir_entries as u32 * 32).div_ceil(self.bytes_per_sector as u32)
    }

    pub const fn data_start_sector(self) -> u32 {
        self.root_dir_start_sector() + self.root_dir_sectors()
    }

    pub const fn cluster_to_sector(self, cluster: u16) -> u32 {
        self.data_start_sector() + (cluster as u32 - 2) * self.sectors_per_cluster as u32
    }

    pub const fn cluster_size(self) -> u32 {
        self.bytes_per_sector as u32 * self.sectors_per_cluster as u32
    }

    fn max_data_cluster(self) -> u16 {
        let data_sectors = u32::from(self.total_sectors) - self.data_start_sector();
        let data_clusters = data_sectors / u32::from(self.sectors_per_cluster);
        data_clusters as u16 + 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub size: u32,
    pub first_cluster: u16,
    pub attr: u8,
    pub date: u16,
    pub time: u16,
}

#[derive(Debug)]
pub struct Fat12Volume<'a> {
    pub bpb: Bpb,
    disk: &'a [u8],
}

impl<'a> Fat12Volume<'a> {
    pub fn open(disk: &'a [u8]) -> Result<Self> {
        let bpb = Bpb::parse(disk)?;
        let declared_len = usize::from(bpb.total_sectors)
            .checked_mul(usize::from(bpb.bytes_per_sector))
            .ok_or_else(|| anyhow::anyhow!("FAT12 volume size overflows usize"))?;
        if disk.len() < declared_len {
            bail!(
                "disk is too small for FAT12 layout: need {declared_len} bytes, have {}",
                disk.len()
            );
        }

        let root_end = bpb.root_dir_start_sector() as usize * usize::from(bpb.bytes_per_sector)
            + usize::from(bpb.root_dir_entries) * 32;
        if root_end > declared_len {
            bail!("FAT12 root directory is outside the declared volume");
        }

        Ok(Self { bpb, disk })
    }

    pub fn list_files(&self) -> Vec<FileEntry> {
        let root_offset =
            self.bpb.root_dir_start_sector() as usize * usize::from(self.bpb.bytes_per_sector);
        let mut files = Vec::new();
        for index in 0..usize::from(self.bpb.root_dir_entries) {
            let offset = root_offset + index * 32;
            let entry = &self.disk[offset..offset + 32];
            match entry[0] {
                0x00 => break,
                0xE5 => continue,
                _ => {}
            }
            let attr = entry[0x0B];
            if attr & 0x08 != 0 {
                continue;
            }
            let base = trim_ascii(&entry[..8]);
            let extension = trim_ascii(&entry[8..11]);
            let name = if extension.is_empty() {
                base
            } else {
                format!("{base}.{extension}")
            };
            files.push(FileEntry {
                name,
                size: u32::from_le_bytes([entry[0x1C], entry[0x1D], entry[0x1E], entry[0x1F]]),
                first_cluster: u16::from_le_bytes([entry[0x1A], entry[0x1B]]),
                attr,
                date: u16::from_le_bytes([entry[0x18], entry[0x19]]),
                time: u16::from_le_bytes([entry[0x16], entry[0x17]]),
            });
        }
        files
    }

    pub fn find(&self, name: &str) -> Option<FileEntry> {
        self.list_files()
            .into_iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    pub fn read_file(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .find(name)
            .ok_or_else(|| anyhow::anyhow!("file not found in FAT12 root directory: {name}"))?;
        self.read_entry(&entry)
    }

    pub fn read_entry(&self, entry: &FileEntry) -> Result<Vec<u8>> {
        if entry.size == 0 {
            return Ok(Vec::new());
        }

        let max_cluster = self.bpb.max_data_cluster();
        let mut cluster = entry.first_cluster;
        let mut seen = HashSet::new();
        let mut data = Vec::with_capacity(entry.size as usize);
        while data.len() < entry.size as usize {
            if !(2..=max_cluster).contains(&cluster) || !seen.insert(cluster) {
                bail!(
                    "{} has a broken FAT12 chain at cluster 0x{cluster:03X}",
                    entry.name
                );
            }

            let offset = self.bpb.cluster_to_sector(cluster) as usize
                * usize::from(self.bpb.bytes_per_sector);
            let end = offset + self.bpb.cluster_size() as usize;
            if end > self.disk.len() {
                bail!(
                    "{} cluster 0x{cluster:03X} is outside the disk image",
                    entry.name
                );
            }
            data.extend_from_slice(&self.disk[offset..end]);
            if data.len() >= entry.size as usize {
                break;
            }

            let next = self.fat_get(cluster)?;
            if next >= 0xFF8 {
                bail!("{} FAT12 chain ends before its declared size", entry.name);
            }
            if next == 0xFF7 || next < 2 {
                bail!(
                    "{} has a broken FAT12 chain at cluster 0x{next:03X}",
                    entry.name
                );
            }
            cluster = next;
        }

        data.truncate(entry.size as usize);
        Ok(data)
    }

    fn fat_get(&self, cluster: u16) -> Result<u16> {
        let position = cluster as usize + cluster as usize / 2;
        let offset = self.bpb.fat_start_sector() as usize * usize::from(self.bpb.bytes_per_sector)
            + position;
        if offset + 2 > self.disk.len() {
            bail!("FAT12 entry for cluster 0x{cluster:03X} is outside the disk image");
        }
        let raw = u16::from_le_bytes([self.disk[offset], self.disk[offset + 1]]);
        Ok(if cluster & 1 == 0 {
            raw & 0x0FFF
        } else {
            raw >> 4
        })
    }
}

fn trim_ascii(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .rposition(|byte| !matches!(byte, 0x00 | 0x20))
        .map_or(0, |index| index + 1);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECTOR_SIZE: usize = 1024;

    fn fixture() -> Vec<u8> {
        let mut disk = vec![0u8; 20 * SECTOR_SIZE];
        disk[0] = 0xEB;
        disk[2] = 0x90;
        disk[0x0B..0x0D].copy_from_slice(&(SECTOR_SIZE as u16).to_le_bytes());
        disk[0x0D] = 1;
        disk[0x0E..0x10].copy_from_slice(&1u16.to_le_bytes());
        disk[0x10] = 2;
        disk[0x11..0x13].copy_from_slice(&16u16.to_le_bytes());
        disk[0x13..0x15].copy_from_slice(&20u16.to_le_bytes());
        disk[0x15] = 0xFE;
        disk[0x16..0x18].copy_from_slice(&1u16.to_le_bytes());

        for fat_sector in [1usize, 2] {
            let fat = fat_sector * SECTOR_SIZE;
            disk[fat..fat + 3].copy_from_slice(&[0xFE, 0xFF, 0xFF]);
            disk[fat + 3] = 0xFF;
            disk[fat + 4] = 0x0F;
        }

        let root = 3 * SECTOR_SIZE;
        disk[root..root + 11].copy_from_slice(b"HELLO   TXT");
        disk[root + 0x1A..root + 0x1C].copy_from_slice(&2u16.to_le_bytes());
        disk[root + 0x1C..root + 0x20].copy_from_slice(&5u32.to_le_bytes());
        disk[4 * SECTOR_SIZE..4 * SECTOR_SIZE + 5].copy_from_slice(b"hello");
        disk
    }

    #[test]
    fn parses_and_reads_a_root_file() {
        let disk = fixture();
        let volume = Fat12Volume::open(&disk).unwrap();
        assert_eq!(volume.bpb.bytes_per_sector, 1024);
        assert_eq!(volume.list_files()[0].name, "HELLO.TXT");
        assert_eq!(volume.read_file("hello.txt").unwrap(), b"hello");
    }

    #[test]
    fn rejects_a_truncated_declared_volume() {
        let mut disk = fixture();
        disk.truncate(10 * SECTOR_SIZE);
        let err = Fat12Volume::open(&disk).unwrap_err();
        assert!(err.to_string().contains("disk is too small"));
    }
}
