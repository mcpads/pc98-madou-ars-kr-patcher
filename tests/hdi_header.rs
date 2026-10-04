#[path = "common/mod.rs"]
mod common;

const HDI_PATH: &str = "roms/Madou Monogatari A.R.S.hdi";

fn le_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("u32 field"))
}

#[test]

#[ignore = "requires the original A.R.S HDI in roms/"]
fn hdi_header_matches_observed_geometry() {
    let Some(bytes) = common::try_read(HDI_PATH) else {
        return;
    };
    assert_eq!(bytes.len(), 10_479_616);

    let header_size = le_u32(&bytes, 0x08);
    let raw_size = le_u32(&bytes, 0x0C);
    let bytes_per_sector = le_u32(&bytes, 0x10);
    let sectors_per_track = le_u32(&bytes, 0x14);
    let heads = le_u32(&bytes, 0x18);
    let cylinders = le_u32(&bytes, 0x1C);

    assert_eq!(header_size, 0x1000);
    assert_eq!(raw_size, 0x009F_D800);
    assert_eq!(bytes_per_sector, 256);
    assert_eq!(sectors_per_track, 33);
    assert_eq!(heads, 4);
    assert_eq!(cylinders, 310);
    assert_eq!(
        raw_size,
        bytes_per_sector * sectors_per_track * heads * cylinders
    );
    assert_eq!(bytes.len() as u32, header_size + raw_size);
}
