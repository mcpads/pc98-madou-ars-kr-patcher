//! Decoder for the LZ-style packed A.R.S overlay/resource streams.
//!
//! `MAIN.COM` loads `GAME_*.OVL` into a work segment and expands it with the
//! routine around runtime PC `0x1BE17`. The stream is mostly literal runs plus
//! 8-bit-distance back-references. If a back-reference points before the
//! current output start, the game fills those bytes with zero.

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeReport {
    pub output: Vec<u8>,
    pub bytes_consumed: usize,
    pub commands: usize,
}

pub fn decode_overlay_lz(input: &[u8]) -> Result<DecodeReport> {
    let mut output = Vec::new();
    let mut offset = 0usize;
    let mut commands = 0usize;

    while offset < input.len() {
        let command = input[offset];
        offset += 1;
        commands += 1;

        if command == 0 {
            return Ok(DecodeReport {
                output,
                bytes_consumed: offset,
                commands,
            });
        }

        if command < 0x80 {
            let len = command as usize;
            let end = offset + len;
            if end > input.len() {
                bail!(
                    "literal run at 0x{:X} exceeds input: need {}, have {}",
                    offset - 1,
                    len,
                    input.len().saturating_sub(offset)
                );
            }
            output.extend_from_slice(&input[offset..end]);
            offset = end;
            continue;
        }

        if offset >= input.len() {
            bail!(
                "back-reference at 0x{:X} is missing distance byte",
                offset - 1
            );
        }

        let count = usize::from(command & 0x7F) + 3;
        let distance = usize::from(input[offset]) + 1;
        offset += 1;
        copy_backref_with_zero_fill(&mut output, count, distance);
    }

    bail!("overlay stream is missing the 0x00 terminator")
}

fn copy_backref_with_zero_fill(output: &mut Vec<u8>, count: usize, distance: usize) {
    let start = output.len() as isize - distance as isize;
    for i in 0..count {
        let src = start + i as isize;
        let byte = if src < 0 { 0 } else { output[src as usize] };
        output.push(byte);
    }
}

/// Minimum and maximum back-reference run length the format can encode.
const MIN_MATCH: usize = 3;
const MAX_MATCH: usize = 130; // (0x7F & command) + 3
/// Maximum back-reference distance (`input[offset] + 1`).
const MAX_DISTANCE: usize = 256;
/// Maximum literal bytes a single literal-run command can carry.
const MAX_LITERAL_RUN: usize = 0x7F;

/// Greedy LZ encoder that is the exact inverse of [`decode_overlay_lz`].
///
/// The output is a valid A.R.S overlay stream: literal-run commands
/// (`0x01..=0x7F`), 8-bit-distance back-references (`0x80..=0xFF`), and a
/// trailing `0x00` terminator. Matches are only emitted when the whole run
/// lies inside already-produced output (`distance <= pos`), so the decoder's
/// zero-fill path is never relied on; this guarantees
/// `decode(encode(x)) == x` for every input.
pub fn encode_overlay_lz(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut literals: Vec<u8> = Vec::new();
    let mut pos = 0usize;

    while pos < data.len() {
        let (best_len, best_dist) = longest_match(data, pos);
        if best_len >= MIN_MATCH {
            flush_literals(&mut out, &mut literals);
            out.push(0x80 | ((best_len - MIN_MATCH) as u8));
            out.push((best_dist - 1) as u8);
            pos += best_len;
        } else {
            literals.push(data[pos]);
            pos += 1;
            if literals.len() == MAX_LITERAL_RUN {
                flush_literals(&mut out, &mut literals);
            }
        }
    }

    flush_literals(&mut out, &mut literals);
    out.push(0x00);
    out
}

fn flush_literals(out: &mut Vec<u8>, literals: &mut Vec<u8>) {
    for chunk in literals.chunks(MAX_LITERAL_RUN) {
        out.push(chunk.len() as u8);
        out.extend_from_slice(chunk);
    }
    literals.clear();
}

/// Longest back-reference match at `pos`, considering only distances that stay
/// within already-output bytes. Returns `(length, distance)`, length 0 if none.
fn longest_match(data: &[u8], pos: usize) -> (usize, usize) {
    let max_len = MAX_MATCH.min(data.len() - pos);
    if max_len < MIN_MATCH {
        return (0, 0);
    }

    let mut best_len = 0usize;
    let mut best_dist = 0usize;
    let max_dist = MAX_DISTANCE.min(pos);
    for distance in 1..=max_dist {
        let mut len = 0usize;
        while len < max_len && data[pos + len] == data[pos - distance + len] {
            len += 1;
        }
        if len > best_len {
            best_len = len;
            best_dist = distance;
            if best_len == max_len {
                break;
            }
        }
    }
    (best_len, best_dist)
}

#[cfg(test)]
mod tests {
    use super::{decode_overlay_lz, encode_overlay_lz};

    fn roundtrip(data: &[u8]) {
        let encoded = encode_overlay_lz(data);
        assert_eq!(
            *encoded.last().unwrap(),
            0u8,
            "stream must end in terminator"
        );
        let decoded = decode_overlay_lz(&encoded).unwrap();
        assert_eq!(decoded.output, data, "decode(encode(x)) must equal x");
        assert_eq!(decoded.bytes_consumed, encoded.len());
    }

    #[test]
    fn roundtrips_empty() {
        roundtrip(&[]);
    }

    #[test]
    fn roundtrips_literals_only() {
        roundtrip(b"the quick brown fox");
    }

    #[test]
    fn roundtrips_repeats_as_backrefs() {
        // A run that the encoder should compress with overlapping back-refs.
        let data = vec![0xAB; 500];
        let encoded = encode_overlay_lz(&data);
        assert!(encoded.len() < data.len(), "repeats must compress");
        roundtrip(&data);
    }

    #[test]
    fn roundtrips_overlapping_pattern() {
        // distance < length: byte-by-byte repeat semantics.
        let mut data = vec![1u8, 2, 3];
        for _ in 0..200 {
            let n = data.len();
            data.push(data[n - 3]);
        }
        roundtrip(&data);
    }

    #[test]
    fn roundtrips_long_literal_runs() {
        // Incompressible data forces multiple >127-byte literal chunks.
        let data: Vec<u8> = (0..1000).map(|i| (i * 37 + 11) as u8).collect();
        roundtrip(&data);
    }

    #[test]
    fn roundtrips_leading_zeros_without_zero_fill() {
        // Leading zeros must round-trip via literals, not the decoder zero-fill.
        let mut data = vec![0u8; 64];
        data.extend_from_slice(b"payload");
        roundtrip(&data);
    }

    #[test]
    fn decodes_literal_runs() {
        let decoded = decode_overlay_lz(&[3, b'a', b'b', b'c', 0]).unwrap();
        assert_eq!(decoded.output, b"abc");
        assert_eq!(decoded.bytes_consumed, 5);
        assert_eq!(decoded.commands, 2);
    }

    #[test]
    fn decodes_back_references() {
        let decoded = decode_overlay_lz(&[3, b'a', b'b', b'c', 0x80, 2, 0]).unwrap();
        assert_eq!(decoded.output, b"abcabc");
    }

    #[test]
    fn zero_fills_back_references_before_output_start() {
        let decoded = decode_overlay_lz(&[0x80, 2, 0]).unwrap();
        assert_eq!(decoded.output, &[0, 0, 0]);
    }

    #[test]
    fn rejects_missing_terminator() {
        let err = decode_overlay_lz(&[1, b'a']).unwrap_err();
        assert!(err.to_string().contains("missing the 0x00 terminator"));
    }
}
