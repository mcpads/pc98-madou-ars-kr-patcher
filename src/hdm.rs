//! Sector-linear PC-98 HDM images used by the A.R.S floppy set.

use anyhow::{Result, bail};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HdmGeometry {
    pub cylinders: u8,
    pub heads: u8,
    pub sectors_per_track: u8,
    pub sector_size: u16,
}

impl HdmGeometry {
    pub const fn total_sectors(self) -> usize {
        self.cylinders as usize * self.heads as usize * self.sectors_per_track as usize
    }

    pub const fn total_size(self) -> usize {
        self.total_sectors() * self.sector_size as usize
    }
}

const KNOWN_GEOMETRIES: [HdmGeometry; 2] = [
    HdmGeometry {
        cylinders: 77,
        heads: 2,
        sectors_per_track: 8,
        sector_size: 1024,
    },
    HdmGeometry {
        cylinders: 80,
        heads: 2,
        sectors_per_track: 8,
        sector_size: 1024,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HdmFile {
    pub geometry: HdmGeometry,
    data: Vec<u8>,
}

impl HdmFile {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let Some(geometry) = KNOWN_GEOMETRIES
            .into_iter()
            .find(|geometry| geometry.total_size() == bytes.len())
        else {
            let expected = KNOWN_GEOMETRIES
                .iter()
                .map(|geometry| geometry.total_size().to_string())
                .collect::<Vec<_>>()
                .join(" or ");
            bail!(
                "HDM size {} is unsupported; expected {expected} bytes",
                bytes.len()
            );
        };

        Ok(Self {
            geometry,
            data: bytes.to_vec(),
        })
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn serialize(&self) -> Vec<u8> {
        self.data.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_both_supported_geometries() {
        for geometry in KNOWN_GEOMETRIES {
            let bytes = vec![0xA5; geometry.total_size()];
            let parsed = HdmFile::parse(&bytes).unwrap();
            assert_eq!(parsed.geometry, geometry);
            assert_eq!(parsed.serialize(), bytes);
        }
    }

    #[test]
    fn rejects_an_unknown_size() {
        let err = HdmFile::parse(&[0; 1024]).unwrap_err();
        assert!(err.to_string().contains("HDM size 1024 is unsupported"));
    }
}
