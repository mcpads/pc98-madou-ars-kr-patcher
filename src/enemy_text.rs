//! Text probes for per-enemy A.R.S data files.

use anyhow::{Result, bail};
use encoding_rs::SHIFT_JIS;

const ATTACK_SUFFIX: &[u8] = &[
    0x82, 0xCC, // の
    0x81, 0x40, // full-width space
    0x8D, 0x55, 0x8C, 0x82, // 攻撃
    0x81, 0x49, // !
    0x0A, 0x0A, 0x00,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyAttackName {
    pub name_offset: usize,
    pub name_end_offset: usize,
    pub attack_text_offset: usize,
    pub raw: Vec<u8>,
    pub trailing: Vec<u8>,
    pub name: String,
}

pub fn find_enemy_attack_names(data: &[u8]) -> Vec<EnemyAttackName> {
    let mut names = Vec::new();
    let mut offset = 0usize;

    while let Some(relative) = find_bytes(&data[offset..], ATTACK_SUFFIX) {
        let attack_text_offset = offset + relative;
        if attack_text_offset > 0
            && data[attack_text_offset - 1] == 0
            && let Some((name_offset, name_end_offset)) =
                find_name_span(data, attack_text_offset - 1, 32)
        {
            let raw = data[name_offset..name_end_offset].to_vec();
            let (name, _, _) = SHIFT_JIS.decode(&raw);
            let name = name.into_owned();
            names.push(EnemyAttackName {
                name_offset,
                name_end_offset,
                attack_text_offset,
                trailing: data[name_end_offset..attack_text_offset - 1].to_vec(),
                raw,
                name,
            });
        }
        offset = attack_text_offset + ATTACK_SUFFIX.len();
    }

    names
}

/// Report of a length-preserving gaiji overwrite of one enemy name field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnemyNameGaijiPatch {
    pub name_offset: usize,
    pub name_end_offset: usize,
    pub original_name: String,
    pub glyph_count: usize,
}

/// Overwrite the first anchored enemy attack name in `data` with a repeated
/// two-byte gaiji code, preserving the field byte length exactly.
///
/// The name field is full-width SJIS (an even byte length), so a two-byte gaiji
/// pair tiles it without remainder. This is the renderer-path Hangul PoC input:
/// the same per-enemy buffer feeds the graphics text renderer at runtime.
pub fn overwrite_first_enemy_name_with_gaiji(
    data: &mut [u8],
    gaiji_sjis: [u8; 2],
) -> Result<EnemyNameGaijiPatch> {
    let names = find_enemy_attack_names(data);
    let Some(first) = names.first() else {
        bail!("no anchored enemy attack name found");
    };
    let span = first.name_end_offset - first.name_offset;
    if !span.is_multiple_of(2) {
        bail!("enemy name field is not an even byte length: {span}");
    }
    let original_name = first.name.clone();
    let name_offset = first.name_offset;
    let name_end_offset = first.name_end_offset;
    for pair in data[name_offset..name_end_offset].as_chunks_mut::<2>().0 {
        pair.copy_from_slice(&gaiji_sjis);
    }
    Ok(EnemyNameGaijiPatch {
        name_offset,
        name_end_offset,
        original_name,
        glyph_count: span / 2,
    })
}

/// Overwrite the first anchored enemy name with `replacement`, which must match
/// the original name field byte length exactly (length-preserving injection).
pub fn overwrite_first_enemy_name(
    data: &mut [u8],
    replacement: &[u8],
) -> Result<EnemyNameGaijiPatch> {
    let names = find_enemy_attack_names(data);
    let Some(first) = names.first() else {
        bail!("no anchored enemy attack name found");
    };
    let span = first.name_end_offset - first.name_offset;
    if replacement.len() != span {
        bail!(
            "replacement is {} bytes but the name field is {span} bytes (must preserve length)",
            replacement.len()
        );
    }
    let original_name = first.name.clone();
    let name_offset = first.name_offset;
    let name_end_offset = first.name_end_offset;
    data[name_offset..name_end_offset].copy_from_slice(replacement);
    Ok(EnemyNameGaijiPatch {
        name_offset,
        name_end_offset,
        original_name,
        glyph_count: span / 2,
    })
}

fn find_name_span(data: &[u8], field_end: usize, max_len: usize) -> Option<(usize, usize)> {
    let min_start = field_end.saturating_sub(max_len);
    let after_ret = data[min_start..field_end]
        .iter()
        .rposition(|&byte| byte == 0xC3)
        .map(|position| min_start + position + 1)?;
    if after_ret < field_end
        && let Some(name_len) = plausible_name_prefix_len(&data[after_ret..field_end])
    {
        Some((after_ret, after_ret + name_len))
    } else {
        None
    }
}

fn plausible_name_prefix_len(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }

    let mut offset = 0usize;
    let mut has_sjis_pair = false;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if offset + 1 < bytes.len()
            && ((0x81..=0x9F).contains(&byte) || (0xE0..=0xFC).contains(&byte))
            && ((0x40..=0x7E).contains(&bytes[offset + 1])
                || (0x80..=0xFC).contains(&bytes[offset + 1]))
        {
            has_sjis_pair = true;
            offset += 2;
            continue;
        }

        break;
    }

    if !has_sjis_pair {
        return None;
    }

    let (decoded, _, had_errors) = SHIFT_JIS.decode(&bytes[..offset]);
    if !had_errors
        && decoded.chars().count() >= 1
        && decoded
            .chars()
            .all(|ch| !ch.is_control() && !('\u{E000}'..='\u{F8FF}').contains(&ch))
    {
        Some(offset)
    } else {
        None
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::{find_enemy_attack_names, overwrite_first_enemy_name_with_gaiji};

    fn sample_enemy() -> Vec<u8> {
        let mut data = vec![0xCC, 0x90, 0xC3];
        data.extend_from_slice(&[
            0x83, 0x69, 0x83, 0x58, 0x83, 0x4F, 0x83, 0x8C, 0x83, 0x43, 0x83, 0x75, 0x00, 0x82,
            0xCC, 0x81, 0x40, 0x8D, 0x55, 0x8C, 0x82, 0x81, 0x49, 0x0A, 0x0A, 0x00,
        ]);
        data
    }

    #[test]
    fn finds_name_before_attack_suffix() {
        let data = sample_enemy();
        let names = find_enemy_attack_names(&data);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name_offset, 3);
        assert_eq!(names[0].name_end_offset, 15);
        assert_eq!(names[0].attack_text_offset, 16);
        assert_eq!(names[0].name, "ナスグレイブ");
    }

    #[test]
    fn finds_single_kanji_name_before_attack_suffix() {
        let mut data = vec![0xC3];
        data.extend_from_slice(&[
            0x8E, 0x98, 0x00, // 侍 + NUL
            0x82, 0xCC, 0x81, 0x40, 0x8D, 0x55, 0x8C, 0x82, 0x81, 0x49, 0x0A, 0x0A, 0x00,
        ]);

        let names = find_enemy_attack_names(&data);
        assert_eq!(names.len(), 1);
        assert_eq!(names[0].name_offset, 1);
        assert_eq!(names[0].name_end_offset, 3);
        assert_eq!(names[0].attack_text_offset, 4);
        assert_eq!(names[0].name, "侍");
    }

    #[test]
    fn gaiji_overwrite_preserves_length_and_terminator() {
        let mut data = sample_enemy();
        let original = data.clone();
        let patch = overwrite_first_enemy_name_with_gaiji(&mut data, [0xEB, 0x9F]).unwrap();

        assert_eq!(patch.name_offset, 3);
        assert_eq!(patch.name_end_offset, 15);
        assert_eq!(patch.glyph_count, 6);
        assert_eq!(patch.original_name, "ナスグレイブ");
        assert_eq!(data.len(), original.len(), "byte length preserved");
        // Name span is now repeated gaiji; the null terminator and attack suffix
        // are untouched.
        assert!(
            data[patch.name_offset..patch.name_end_offset]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|pair| *pair == [0xEB, 0x9F])
        );
        assert_eq!(
            &data[patch.name_end_offset..],
            &original[patch.name_end_offset..]
        );
    }
}
