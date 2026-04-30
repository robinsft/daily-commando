// SPDX-License-Identifier: EUPL-1.2
//! Big ASCII digit fonts.
//!
//! Two fonts:
//!  * `Ok` — clean, full-block (`█`/`░`). Used while time is healthy.
//!  * `Ko` — corrupted, zero-based (`0`/`░`). Used in warning/critical phases
//!    and alternated with `Ok` during overtime to give a "broken clock" feel.
//!
//! Each digit is 7 columns wide × 7 rows tall. The colon `:` is 3 columns.
//! Composition: characters are concatenated horizontally with a 1-col gap.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Font { Ok, Ko }

const ROWS: usize = 7;

// Each digit is 7 chars wide; rows separated by '\n'.
const DIGITS_OK: [&str; 10] = [
    " █████ \n██   ██\n██   ██\n██   ██\n██   ██\n██   ██\n █████ ",
    "  ██   \n ███   \n  ██   \n  ██   \n  ██   \n  ██   \n██████ ",
    " █████ \n██   ██\n     ██\n  ████ \n ██    \n██     \n███████",
    " █████ \n██   ██\n     ██\n  ████ \n     ██\n██   ██\n █████ ",
    "██   ██\n██   ██\n██   ██\n███████\n     ██\n     ██\n     ██",
    "███████\n██     \n██     \n██████ \n     ██\n██   ██\n █████ ",
    " █████ \n██     \n██     \n██████ \n██   ██\n██   ██\n █████ ",
    "███████\n     ██\n    ██ \n   ██  \n  ██   \n ██    \n██     ",
    " █████ \n██   ██\n██   ██\n █████ \n██   ██\n██   ██\n █████ ",
    " █████ \n██   ██\n██   ██\n ██████\n     ██\n     ██\n █████ ",
];

const DIGITS_KO: [&str; 10] = [
    " 00000 \n00░░░00\n00░ ░00\n00░ ░00\n00░ ░00\n00░░░00\n 00000 ",
    "  00   \n 000   \n░░00░  \n  00░  \n  00░  \n  00░  \n000000 ",
    " 00000 \n00░░░00\n░░░ ░00\n  ░000 \n 00░░░ \n00░░░░░\n0000000",
    " 00000 \n00░░░00\n░░░ ░00\n  ░00░ \n░░░ ░00\n00░ ░00\n 00000 ",
    "00░ ░00\n00░ ░00\n00░ ░00\n0000000\n░░░ ░00\n░░░ ░00\n░░░ ░00",
    "0000000\n00░░░░░\n00░░░░░\n000000░\n░░░ ░00\n00░ ░00\n 00000 ",
    " 00000 \n00░░░░░\n00░░░░░\n000000░\n00░ ░00\n00░ ░00\n 00000 ",
    "0000000\n░░░ ░00\n░░ ░00░\n░ ░00░░\n  00░░░\n 00░░░░\n00░░░░░",
    " 00000 \n00░ ░00\n00░ ░00\n 00000 \n00░ ░00\n00░ ░00\n 00000 ",
    " 00000 \n00░ ░00\n00░ ░00\n 000000\n░░░ ░00\n░░░ ░00\n 00000 ",
];

const COLON_OK: &str = "   \n██ \n██ \n   \n██ \n██ \n   ";
const COLON_KO: &str = "   \n00 \n00 \n   \n00 \n00 \n   ";

const MINUS_OK: &str = "   \n   \n   \n███\n   \n   \n   ";
const MINUS_KO: &str = "   \n   \n   \n000\n   \n   \n   ";

fn glyph(font: Font, ch: char) -> &'static str {
    match (font, ch) {
        (Font::Ok, '0'..='9') => DIGITS_OK[(ch as u8 - b'0') as usize],
        (Font::Ko, '0'..='9') => DIGITS_KO[(ch as u8 - b'0') as usize],
        (Font::Ok, ':') => COLON_OK,
        (Font::Ko, ':') => COLON_KO,
        (Font::Ok, '-') => MINUS_OK,
        (Font::Ko, '-') => MINUS_KO,
        _ => COLON_OK, // fallback
    }
}

/// Render a string (digits, ':', '-') as a 7-line ASCII-art block.
pub fn render_big(s: &str, font: Font) -> Vec<String> {
    let glyphs: Vec<Vec<&str>> = s
        .chars()
        .map(|ch| glyph(font, ch).split('\n').collect())
        .collect();
    (0..ROWS)
        .map(|row| {
            let mut line = String::new();
            for (i, g) in glyphs.iter().enumerate() {
                if i > 0 { line.push(' '); }
                line.push_str(g.get(row).copied().unwrap_or(""));
            }
            line
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_basic_digits() {
        let lines = render_big("123", Font::Ok);
        assert_eq!(lines.len(), ROWS);
        assert!(lines.iter().any(|l| l.contains('█')));
    }

    #[test]
    fn render_with_colon() {
        let lines = render_big("1:23", Font::Ko);
        assert_eq!(lines.len(), ROWS);
        assert!(lines.iter().any(|l| l.contains('0')));
    }
}
