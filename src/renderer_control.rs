//! Renderer control-code boundaries shared by extraction and validation.

use anyhow::{Result, bail};

/// Parameter bytes consumed after one renderer control byte.
///
/// The dispatcher handles every byte below 0x20 by its low nibble, so the
/// 0x10..=0x1F aliases have the same arity as 0x00..=0x0F.
pub const fn control_arity(byte: u8) -> Option<usize> {
    if byte >= 0x20 {
        return None;
    }
    Some(match byte & 0x0F {
        0x01 => 2,
        0x02 | 0x04 | 0x0C => 1,
        _ => 0,
    })
}

pub const fn is_terminator(byte: u8) -> bool {
    byte < 0x20 && byte & 0x0F == 0
}

/// Find the terminator consumed by the renderer, skipping control parameters.
pub fn find_message_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        let Some(arity) = control_arity(byte) else {
            cursor += 1;
            continue;
        };
        if is_terminator(byte) {
            return Some(cursor);
        }
        cursor = cursor.checked_add(1 + arity)?;
        if cursor > bytes.len() {
            return None;
        }
    }
    None
}

pub fn message_bytes(bytes: &[u8], start: usize) -> Option<&[u8]> {
    let end = find_message_end(bytes, start)?;
    Some(&bytes[start..end])
}

/// Whether `index` is locally explained as a parameter of a preceding control.
/// Used only to reject false NUL-delimited starts during the completeness scan.
pub fn is_control_parameter_at(bytes: &[u8], index: usize) -> bool {
    (1..=2).any(|distance| {
        let Some(control_index) = index.checked_sub(distance) else {
            return false;
        };
        control_arity(bytes[control_index]).is_some_and(|arity| distance <= arity)
    })
}

/// Parse control instructions, retaining their parameter bytes as one unit.
/// Newlines may move during translation and can be excluded from the result.
pub fn control_sequences(bytes: &[u8], include_newlines: bool) -> Result<Vec<Vec<u8>>> {
    let mut sequences = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        let Some(arity) = control_arity(byte) else {
            cursor += 1;
            continue;
        };
        if is_terminator(byte) {
            bail!("unexpected renderer terminator at byte {cursor}");
        }
        let end = cursor + 1 + arity;
        if end > bytes.len() {
            bail!(
                "truncated renderer control 0x{byte:02X} at byte {cursor}: needs {arity} parameter byte(s)"
            );
        }
        if include_newlines || byte & 0x0F != 0x0A {
            sequences.push(bytes[cursor..end].to_vec());
        }
        cursor = end;
    }
    Ok(sequences)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nul_inside_cursor_parameters_is_not_a_terminator() {
        let message = [0x01, 0x0E, 0x00, 0x04, 0x07, b'A', 0x04, 0x02, 0x00];
        assert_eq!(find_message_end(&message, 0), Some(8));
        assert_eq!(message_bytes(&message, 0), Some(&message[..8]));
        assert!(is_control_parameter_at(&message, 2));
    }

    #[test]
    fn sequences_keep_parameters_attached_to_their_opcode() {
        let message = [0x01, 0x0E, 0x00, 0x04, 0x07, b'A', 0x0A, 0x04, 0x02];
        assert_eq!(
            control_sequences(&message, false).unwrap(),
            vec![vec![0x01, 0x0E, 0x00], vec![0x04, 0x07], vec![0x04, 0x02]]
        );
    }

    #[test]
    fn rejects_a_truncated_parameter_sequence() {
        let err = control_sequences(&[b'A', 0x01, 0x0E], false).unwrap_err();
        assert!(err.to_string().contains("truncated renderer control"));
    }
}
