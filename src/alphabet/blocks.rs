//! The two alphabets that fold a few bytes at a time into a few characters: Z85 and basE91.

use super::Problem;

/// ZeroMQ's Z85 (RFC 32/Z85): letters and digits, and punctuation that needs no quoting in most
/// source code.
pub(super) const Z85: &[u8; 85] =
    b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ.-:+=^!/*?&<>()[]{}@%$#";

/// Joachim Henke's basE91: printable ASCII but space, `-`, `\` and `'`.
pub(super) const BASE91: &[u8; 91] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!#$%&()*+,./:;<=>?@[]^_`{|}~\"";

const fn reverse(digits: &[u8]) -> [u8; 128] {
    let mut table = [u8::MAX; 128];
    let mut i = 0;
    while i < digits.len() {
        table[digits[i] as usize] = i as u8;
        i += 1;
    }
    table
}

const Z85_REVERSE: [u8; 128] = reverse(Z85);
const BASE91_REVERSE: [u8; 128] = reverse(BASE91);

fn digit(reverse: &[u8; 128], at: usize, c: char) -> Result<u32, (usize, Problem)> {
    match reverse.get(c as usize) {
        Some(&value) if value != u8::MAX => Ok(value as u32),
        _ => Err((at, Problem::Foreign(c))),
    }
}

/// Four bytes to five characters, most significant first. A short last group of `n` bytes is
/// written in its first `n + 1` characters, the rule Ascii85 has for it, since Z85 itself takes
/// only whole groups.
pub(super) fn z85_fold(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() / 4 * 5 + 5);
    for chunk in bytes.chunks(4) {
        let mut value = 0u32;
        for i in 0..4 {
            value = (value << 8) | *chunk.get(i).unwrap_or(&0) as u32;
        }
        let mut digits = [0u8; 5];
        for slot in digits.iter_mut().rev() {
            *slot = Z85[(value % 85) as usize];
            value /= 85;
        }
        out.extend(digits[..chunk.len() + 1].iter().map(|&d| d as char));
    }
    out
}

pub(super) fn z85_unfold(text: &str) -> Result<Vec<u8>, (usize, Problem)> {
    let mut out = Vec::with_capacity(text.len() / 5 * 4 + 4);
    let (mut group, mut held, mut start) = ([0u32; 5], 0, 0);
    for (at, c) in text.chars().enumerate() {
        if held == 0 {
            start = at;
        }
        group[held] = digit(&Z85_REVERSE, at, c)?;
        held += 1;
        if held == 5 {
            out.extend_from_slice(&group_bytes(&group, start)?);
            held = 0;
        }
    }
    match held {
        0 => {}
        1 => return Err((start, Problem::BadEnd)),
        _ => {
            // The characters left out stood for the low end, so the highest digit stands in.
            group[held..].fill(84);
            out.extend_from_slice(&group_bytes(&group, start)?[..held - 1]);
        }
    }
    Ok(out)
}

fn group_bytes(group: &[u32; 5], start: usize) -> Result<[u8; 4], (usize, Problem)> {
    let value = group.iter().fold(0u64, |value, &digit| value * 85 + digit as u64);
    u32::try_from(value).map(u32::to_be_bytes).map_err(|_| (start, Problem::TooBig))
}

/// basE91's own loop: 13 bits for two characters where that leaves the pair's value above 88,
/// otherwise 14.
pub(super) fn base91_fold(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 5 / 4 + 4);
    let (mut held, mut count) = (0u32, 0u32);
    for &byte in bytes {
        held |= (byte as u32) << count;
        count += 8;
        if count > 13 {
            let mut value = held & 8191;
            if value > 88 {
                held >>= 13;
                count -= 13;
            } else {
                value = held & 16383;
                held >>= 14;
                count -= 14;
            }
            out.push(BASE91[(value % 91) as usize] as char);
            out.push(BASE91[(value / 91) as usize] as char);
        }
    }
    if count > 0 {
        out.push(BASE91[(held % 91) as usize] as char);
        if count > 7 || held > 90 {
            out.push(BASE91[(held / 91) as usize] as char);
        }
    }
    out
}

pub(super) fn base91_unfold(text: &str) -> Result<Vec<u8>, (usize, Problem)> {
    let mut out = Vec::with_capacity(text.len() * 4 / 5 + 2);
    let (mut pending, mut held, mut count) = (None::<u32>, 0u64, 0u32);
    for (at, c) in text.chars().enumerate() {
        let value = digit(&BASE91_REVERSE, at, c)?;
        let Some(first) = pending.take() else {
            pending = Some(value);
            continue;
        };
        let pair = first + value * 91;
        held |= (pair as u64) << count;
        count += if pair & 8191 > 88 { 13 } else { 14 };
        while count > 7 {
            out.push(held as u8);
            held >>= 8;
            count -= 8;
        }
    }
    if let Some(last) = pending {
        out.push((held | (last as u64) << count) as u8);
    }
    Ok(out)
}
