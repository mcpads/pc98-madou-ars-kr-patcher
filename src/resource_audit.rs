//! Static profiles for classifying packed A.R.S resources.

use crate::{media_identity::sha256_hex, overlay_lz::decode_overlay_lz, sjis_sweep};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteProfile {
    pub size: usize,
    pub sha256: String,
    pub zero_bytes: usize,
    pub ff_bytes: usize,
    pub byte_55: usize,
    pub byte_aa: usize,
    pub distinct_bytes: usize,
    pub sjis_regions: usize,
    pub blocks_0x4000: Option<usize>,
    pub screen_planes_32000: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LzStreamProfile {
    pub input_offset: usize,
    pub bytes_consumed: usize,
    pub commands: usize,
    pub decoded_size: usize,
    pub decoded_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LzProfile {
    pub exact: bool,
    pub bytes_consumed: usize,
    pub trailing_bytes: usize,
    pub commands: usize,
    pub streams: Vec<LzStreamProfile>,
    pub decoded: ByteProfile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceAudit {
    pub packed: ByteProfile,
    pub lz: Option<LzProfile>,
}

pub fn audit_resource(bytes: &[u8], min_run: usize, min_japanese: usize) -> ResourceAudit {
    let packed = profile_bytes(bytes, min_run, min_japanese);
    let mut input_offset = 0usize;
    let mut decoded = Vec::new();
    let mut streams = Vec::new();
    while input_offset < bytes.len() {
        let Ok(report) = decode_overlay_lz(&bytes[input_offset..]) else {
            break;
        };
        // A zero-only tail can look like arbitrarily many empty streams. Empty
        // output carries no resource payload, so leave it as trailing data.
        if report.output.is_empty() {
            break;
        }
        streams.push(LzStreamProfile {
            input_offset,
            bytes_consumed: report.bytes_consumed,
            commands: report.commands,
            decoded_size: report.output.len(),
            decoded_sha256: sha256_hex(&report.output),
        });
        input_offset += report.bytes_consumed;
        decoded.extend_from_slice(&report.output);
    }
    let lz = (!streams.is_empty()).then(|| {
        let trailing_bytes = bytes.len().saturating_sub(input_offset);
        LzProfile {
            exact: trailing_bytes == 0,
            bytes_consumed: input_offset,
            trailing_bytes,
            commands: streams.iter().map(|stream| stream.commands).sum(),
            streams,
            decoded: profile_bytes(&decoded, min_run, min_japanese),
        }
    });
    ResourceAudit { packed, lz }
}

fn profile_bytes(bytes: &[u8], min_run: usize, min_japanese: usize) -> ByteProfile {
    let mut seen = [false; 256];
    let mut zero_bytes = 0usize;
    let mut ff_bytes = 0usize;
    let mut byte_55 = 0usize;
    let mut byte_aa = 0usize;
    for &byte in bytes {
        seen[usize::from(byte)] = true;
        match byte {
            0x00 => zero_bytes += 1,
            0xFF => ff_bytes += 1,
            0x55 => byte_55 += 1,
            0xAA => byte_aa += 1,
            _ => {}
        }
    }
    ByteProfile {
        size: bytes.len(),
        sha256: sha256_hex(bytes),
        zero_bytes,
        ff_bytes,
        byte_55,
        byte_aa,
        distinct_bytes: seen.into_iter().filter(|present| *present).count(),
        sjis_regions: sjis_sweep::scan_string_regions(bytes, min_run, min_japanese).len(),
        blocks_0x4000: (!bytes.is_empty() && bytes.len().is_multiple_of(0x4000))
            .then_some(bytes.len() / 0x4000),
        screen_planes_32000: (!bytes.is_empty() && bytes.len().is_multiple_of(32_000))
            .then_some(bytes.len() / 32_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay_lz::encode_overlay_lz;

    #[test]
    fn profiles_an_exact_three_plane_graphics_shaped_stream() {
        let mut decoded = vec![0u8; 0xC000];
        decoded[0x4000] = 0x55;
        decoded[0x8000] = 0xAA;
        decoded[0xBFFF] = 0xFF;
        let packed = encode_overlay_lz(&decoded);
        let audit = audit_resource(&packed, 8, 5);
        let lz = audit.lz.unwrap();
        assert!(lz.exact);
        assert_eq!(lz.trailing_bytes, 0);
        assert_eq!(lz.streams.len(), 1);
        assert_eq!(lz.decoded.size, 0xC000);
        assert_eq!(lz.decoded.blocks_0x4000, Some(3));
        assert_eq!(lz.decoded.screen_planes_32000, None);
        assert_eq!(lz.decoded.zero_bytes, 0xC000 - 3);
        assert_eq!(lz.decoded.byte_55, 1);
        assert_eq!(lz.decoded.byte_aa, 1);
        assert_eq!(lz.decoded.ff_bytes, 1);
        assert_eq!(lz.decoded.sjis_regions, 0);
    }

    #[test]
    fn leaves_a_non_stream_unclassified() {
        let audit = audit_resource(b"not an A.R.S stream", 8, 5);
        assert!(audit.lz.is_none());
    }

    #[test]
    fn joins_concatenated_lz_streams_without_hiding_the_boundaries() {
        let first = vec![0x55; 32_000];
        let second = vec![0xAA; 32_000];
        let mut packed = encode_overlay_lz(&first);
        packed.extend_from_slice(&encode_overlay_lz(&second));
        let audit = audit_resource(&packed, 8, 5);
        let lz = audit.lz.unwrap();
        assert!(lz.exact);
        assert_eq!(lz.streams.len(), 2);
        assert_eq!(lz.streams[0].input_offset, 0);
        assert_eq!(lz.streams[1].input_offset, lz.streams[0].bytes_consumed);
        assert_eq!(lz.decoded.size, 64_000);
        assert_eq!(lz.decoded.screen_planes_32000, Some(2));
    }
}
