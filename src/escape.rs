//! Text written into a folded file as itself: table entries, and the lines a table squeeze keeps
//! as they are. It stays readable, but nothing in it can break the file's own lines or vanish:
//!
//! - `\\`, `\t`, `\n`, `\r` and `\e` (ESC) for themselves;
//! - `\u{…}` for any other control, and for the invisible characters (zero widths, bidi controls,
//!   the byte order mark), so they are seen;
//! - `\u{20}` for a space at the very end, which editors and hooks trim;
//! - `\<` for a `<` at the very start, since a table entry spelled `<end>` is a marker, and `\@`
//!   for an `@` there, since `@3` is the third value of an index;
//! - `\0` for an empty text, so an entry is never blank.

/// `text` as a folded file writes it.
pub(crate) fn escape(text: &str) -> String {
    if text.is_empty() {
        return "\\0".to_string();
    }
    let mut out = String::with_capacity(text.len());
    let last = text.chars().count() - 1;
    for (index, c) in text.chars().enumerate() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{1b}' => out.push_str("\\e"),
            '<' if index == 0 => out.push_str("\\<"),
            '@' if index == 0 => out.push_str("\\@"),
            ' ' if index == last => out.push_str("\\u{20}"),
            c if c.is_control() || invisible(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// The text an [`escape`]d one stands for.
pub(crate) fn unescape(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('e') => out.push('\u{1b}'),
            Some('<') => out.push('<'),
            Some('@') => out.push('@'),
            Some('0') => {}
            Some('u') => {
                let rest = chars.as_str();
                let inner = rest.strip_prefix('{').and_then(|r| r.split_once('}'));
                let Some((hex, after)) = inner else {
                    return Err(format!("`\\u` without `{{…}}` in {text:?}"));
                };
                let decoded = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                out.push(decoded.ok_or_else(|| format!("`\\u{{{hex}}}` is no character"))?);
                chars = after.chars();
            }
            other => return Err(format!("`\\{}` means nothing in {text:?}", other.unwrap_or(' '))),
        }
    }
    Ok(out)
}

/// Characters that take no room and show nothing: soft hyphen, zero widths, joiners, bidi
/// controls and marks, the byte order mark, interlinear annotation, and the tag characters.
fn invisible(c: char) -> bool {
    matches!(c as u32,
        0x00AD | 0x034F | 0x061C | 0x115F | 0x1160 | 0x17B4 | 0x17B5 | 0x180B..=0x180F
        | 0x200B..=0x200F | 0x2028..=0x202E | 0x2060..=0x206F | 0x3164 | 0xFE00..=0xFE0F
        | 0xFEFF | 0xFFA0 | 0xFFF0..=0xFFFB | 0xE0000..=0xE0FFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_could_break_a_line_or_vanish_is_spelled_out_and_read_back() {
        for text in [
            "",
            "plain",
            "tab\there",
            "back\\slash",
            "\u{1b}[38;2;1;2;3m",
            "trailing ",
            " leading",
            "<end>",
            "@12",
            "zero\u{200b}width",
            "bell\u{7}",
            "\r\n",
            "ascii art: ⣿⡇ 白猫",
        ] {
            let escaped = escape(text);
            assert!(!escaped.is_empty() && !escaped.contains(['\n', '\t', '\r']), "{escaped:?}");
            assert!(!escaped.ends_with(' ') && !escaped.starts_with(['<', '@']), "{escaped:?}");
            assert_eq!(unescape(&escaped).as_deref(), Ok(text));
        }
    }

    #[test]
    fn readable_text_stays_as_it_is() {
        assert_eq!(escape("Half-Life 2"), "Half-Life 2");
        assert_eq!(escape("a <b>"), "a <b>");
    }

    #[test]
    fn a_broken_escape_is_refused() {
        assert!(unescape("\\q").is_err());
        assert!(unescape("\\u{zz}").is_err());
        assert!(unescape("\\u12").is_err());
        assert!(unescape("dangling\\").is_err());
    }
}
