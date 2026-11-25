//! A 191-glyph subset of Terminus Font, by Dimitar Toshkov Zhekov, under
//! the SIL Open Font License 1.1 — see `third_party/terminus-font/`.
//!
//! The glyph tables (`terminus/ter-u*.bin`) are generated from the
//! upstream 4.49.1 BDF sources by `tools/gen-terminus.py`, which documents
//! their layout: 191 glyphs (ASCII `0x20..=0x7E`, then Latin-1
//! `0xA0..=0xFF`), each `height` rows of `ceil(width / 8)` bytes, most
//! significant bit leftmost — the layout [`Canvas::fill_mask`] draws.

use super::Font;
use crate::canvas::Canvas;
use crate::pixel::Rect;

/// One size and weight of the embedded Terminus subset: ASCII and
/// Latin-1, drawn in any color. Characters outside the subset draw as a
/// hollow box.
///
/// Each constant exists only with its cargo feature (`font-terminus-16`
/// for [`Terminus::REGULAR_16`], `font-terminus-16-bold` for
/// [`Terminus::BOLD_16`], ...), so a program pays only for the bitmaps it
/// names. There is no 12 px bold: upstream Terminus has none (its
/// `ter-u12b` is a copy of the regular 6×12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terminus {
    width: u8,
    height: u8,
    ascent: u8,
    glyphs: &'static [u8],
}

impl Terminus {
    /// 6×12 regular (feature `font-terminus-12`).
    #[cfg(feature = "font-terminus-12")]
    pub const REGULAR_12: Terminus = Terminus { width: 6, height: 12, ascent: 10, glyphs: include_bytes!("terminus/ter-u12n.bin") };
    /// 8×16 regular (feature `font-terminus-16`).
    #[cfg(feature = "font-terminus-16")]
    pub const REGULAR_16: Terminus = Terminus { width: 8, height: 16, ascent: 12, glyphs: include_bytes!("terminus/ter-u16n.bin") };
    /// 8×16 bold (feature `font-terminus-16-bold`).
    #[cfg(feature = "font-terminus-16-bold")]
    pub const BOLD_16: Terminus = Terminus { width: 8, height: 16, ascent: 12, glyphs: include_bytes!("terminus/ter-u16b.bin") };
    /// 10×20 regular (feature `font-terminus-20`).
    #[cfg(feature = "font-terminus-20")]
    pub const REGULAR_20: Terminus = Terminus { width: 10, height: 20, ascent: 16, glyphs: include_bytes!("terminus/ter-u20n.bin") };
    /// 10×20 bold (feature `font-terminus-20-bold`).
    #[cfg(feature = "font-terminus-20-bold")]
    pub const BOLD_20: Terminus = Terminus { width: 10, height: 20, ascent: 16, glyphs: include_bytes!("terminus/ter-u20b.bin") };
    /// 12×24 regular (feature `font-terminus-24`).
    #[cfg(feature = "font-terminus-24")]
    pub const REGULAR_24: Terminus = Terminus { width: 12, height: 24, ascent: 19, glyphs: include_bytes!("terminus/ter-u24n.bin") };
    /// 12×24 bold (feature `font-terminus-24-bold`).
    #[cfg(feature = "font-terminus-24-bold")]
    pub const BOLD_24: Terminus = Terminus { width: 12, height: 24, ascent: 19, glyphs: include_bytes!("terminus/ter-u24b.bin") };
    /// 16×32 regular (feature `font-terminus-32`).
    #[cfg(feature = "font-terminus-32")]
    pub const REGULAR_32: Terminus = Terminus { width: 16, height: 32, ascent: 26, glyphs: include_bytes!("terminus/ter-u32n.bin") };
    /// 16×32 bold (feature `font-terminus-32-bold`).
    #[cfg(feature = "font-terminus-32-bold")]
    pub const BOLD_32: Terminus = Terminus { width: 16, height: 32, ascent: 26, glyphs: include_bytes!("terminus/ter-u32b.bin") };

    /// Cell width (every glyph's advance), in pixels.
    pub fn cell_width(&self) -> u32 {
        u32::from(self.width)
    }

    /// Rows from the top of a cell down to the baseline.
    pub fn ascent(&self) -> u32 {
        u32::from(self.ascent)
    }

    /// The bitmap of `ch` — `line_height()` rows of `ceil(cell_width / 8)`
    /// bytes, most significant bit leftmost — or `None` outside ASCII and
    /// Latin-1 (control characters included).
    pub fn glyph(&self, ch: char) -> Option<&'static [u8]> {
        let index = match u32::from(ch) {
            c @ 0x20..=0x7e => c - 0x20,
            c @ 0xa0..=0xff => c - 0xa0 + 95,
            _ => return None,
        } as usize;
        let size = usize::from(self.height) * usize::from(self.width).div_ceil(8);
        let glyphs: &'static [u8] = self.glyphs;
        glyphs.get(index * size..(index + 1) * size)
    }
}

impl Font for Terminus {
    fn line_height(&self) -> u32 {
        u32::from(self.height)
    }

    fn advance(&self, _ch: char) -> u32 {
        self.cell_width()
    }

    fn draw_char(&self, canvas: &mut Canvas, ch: char, x: i32, y: i32, color: u32) -> u32 {
        let (w, h) = (self.cell_width(), self.line_height());
        match self.glyph(ch) {
            Some(bits) => canvas.fill_mask(bits, w, h, x, y, color),
            None => {
                // A hollow box one pixel inside the cell.
                let (x1, y1) = (x.saturating_add(1), y.saturating_add(1));
                canvas.fill_rect(Rect::new(x1, y1, w - 2, 1), color);
                canvas.fill_rect(Rect::new(x1, y.saturating_add(h as i32 - 2), w - 2, 1), color);
                canvas.fill_rect(Rect::new(x1, y1, 1, h - 2), color);
                canvas.fill_rect(Rect::new(x.saturating_add(w as i32 - 2), y1, 1, h - 2), color);
            }
        }
        w
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Glyphs per table: printable ASCII (95) and Latin-1 (96).
    const GLYPHS: usize = 191;

    /// Every table compiled in, with its file name for messages.
    fn all() -> Vec<(&'static str, Terminus)> {
        vec![
            #[cfg(feature = "font-terminus-12")]
            ("ter-u12n", Terminus::REGULAR_12),
            #[cfg(feature = "font-terminus-16")]
            ("ter-u16n", Terminus::REGULAR_16),
            #[cfg(feature = "font-terminus-16-bold")]
            ("ter-u16b", Terminus::BOLD_16),
            #[cfg(feature = "font-terminus-20")]
            ("ter-u20n", Terminus::REGULAR_20),
            #[cfg(feature = "font-terminus-20-bold")]
            ("ter-u20b", Terminus::BOLD_20),
            #[cfg(feature = "font-terminus-24")]
            ("ter-u24n", Terminus::REGULAR_24),
            #[cfg(feature = "font-terminus-24-bold")]
            ("ter-u24b", Terminus::BOLD_24),
            #[cfg(feature = "font-terminus-32")]
            ("ter-u32n", Terminus::REGULAR_32),
            #[cfg(feature = "font-terminus-32-bold")]
            ("ter-u32b", Terminus::BOLD_32),
        ]
    }

    /// Render `ch` and return it as text art: `#` set, `.` clear.
    fn art(font: Terminus, ch: char) -> Vec<String> {
        let mut c = Canvas::new(font.cell_width(), font.line_height());
        assert_eq!(font.draw_char(&mut c, ch, 0, 0, 0xffffff), font.cell_width());
        c.pixels().chunks(font.cell_width() as usize).map(|row| row.iter().map(|&p| if p != 0 { '#' } else { '.' }).collect()).collect()
    }

    /// The upstream BDF `BITMAP` lines of a glyph, as text art.
    fn bdf(width: u32, rows: &[&str]) -> Vec<String> {
        rows.iter()
            .map(|r| (0..width).map(|x| if u32::from_str_radix(r, 16).unwrap() >> (r.len() as u32 * 4 - 1 - x) & 1 != 0 { '#' } else { '.' }).collect())
            .collect()
    }

    #[test]
    fn every_table_has_191_whole_glyphs_and_sane_metrics() {
        for (name, f) in all() {
            let glyph = f.line_height() as usize * f.cell_width().div_ceil(8) as usize;
            assert_eq!(f.glyphs.len(), GLYPHS * glyph, "{name}");
            assert!(f.ascent() < f.line_height() && f.cell_width() * 2 == f.line_height(), "{name}");
            assert!(f.glyph(' ').unwrap().iter().all(|&b| b == 0), "{name}: space is blank");
            assert!(f.glyph('\u{ff}').is_some() && f.glyph('\u{100}').is_none() && f.glyph('\u{7f}').is_none(), "{name}");
            // No glyph sets the padding bits past the cell width.
            let pad = (8 - f.cell_width() % 8) % 8;
            for (i, row) in f.glyphs.chunks(f.cell_width().div_ceil(8) as usize).enumerate() {
                assert_eq!(row[row.len() - 1] & ((1u8 << pad) - 1), 0, "{name}: row {i} sets padding bits");
            }
        }
    }

    /// One glyph per table, its `BITMAP` lines copied from the upstream
    /// 4.49.1 BDF file named, rendered through the whole draw path.
    #[test]
    fn glyphs_match_upstream_bdf() {
        #[rustfmt::skip]
        let cases: Vec<(Terminus, char, &[&str])> = vec![
            #[cfg(feature = "font-terminus-12")] // ter-u12n.bdf, W
            (Terminus::REGULAR_12, 'W', &["00", "00", "88", "88", "88", "88", "A8", "A8", "D8", "88", "00", "00"]),
            #[cfg(feature = "font-terminus-16")] // ter-u16n.bdf, A
            (Terminus::REGULAR_16, 'A', &["00", "00", "3C", "42", "42", "42", "42", "7E", "42", "42", "42", "42", "00", "00", "00", "00"]),
            #[cfg(feature = "font-terminus-16")] // ter-u16n.bdf, eacute: Latin-1 indexing
            (Terminus::REGULAR_16, 'é', &["00", "00", "08", "10", "00", "3C", "42", "42", "7E", "40", "40", "3C", "00", "00", "00", "00"]),
            #[cfg(feature = "font-terminus-16-bold")] // ter-u16b.bdf, R
            (Terminus::BOLD_16, 'R', &["00", "00", "FC", "C6", "C6", "C6", "C6", "FC", "F0", "D8", "CC", "C6", "00", "00", "00", "00"]),
            #[cfg(feature = "font-terminus-20")] // ter-u20n.bdf, at
            (Terminus::REGULAR_20, '@', &["0000", "0000", "0000", "3F00", "4080", "4080", "4780", "4880", "4880", "4880", "4880", "4980", "4680", "4000", "4000", "3F80", "0000", "0000", "0000", "0000"]),
            #[cfg(feature = "font-terminus-20-bold")] // ter-u20b.bdf, ampersand
            (Terminus::BOLD_20, '&', &["0000", "0000", "0000", "3C00", "6600", "6600", "6600", "3C00", "1800", "3980", "6D80", "C700", "C300", "C300", "6780", "3D80", "0000", "0000", "0000", "0000"]),
            #[cfg(feature = "font-terminus-24")] // ter-u24n.bdf, germandbls
            (Terminus::REGULAR_24, 'ß', &["0000", "0000", "0000", "0000", "3E00", "4100", "4080", "4080", "4080", "4100", "7F00", "4080", "4040", "4040", "4040", "4040", "6040", "5080", "4F00", "0000", "0000", "0000", "0000", "0000"]),
            #[cfg(feature = "font-terminus-24-bold")] // ter-u24b.bdf, percent
            (Terminus::BOLD_24, '%', &["0000", "0000", "0000", "0000", "0000", "38C0", "6CC0", "6D80", "3980", "0300", "0300", "0600", "0600", "0C00", "0C00", "19C0", "1B60", "3360", "31C0", "0000", "0000", "0000", "0000", "0000"]),
            #[cfg(feature = "font-terminus-32")] // ter-u32n.bdf, Q
            (Terminus::REGULAR_32, 'Q', &["0000", "0000", "0000", "0000", "0000", "0000", "0FF0", "1FF8", "381C", "300C", "300C", "300C", "300C", "300C", "300C", "300C", "300C", "300C", "300C", "300C", "300C", "300C", "31CC", "38FC", "1FF8", "0FF8", "001C", "000E", "0000", "0000", "0000", "0000"]),
            #[cfg(feature = "font-terminus-32-bold")] // ter-u32b.bdf, g: a descender
            (Terminus::BOLD_32, 'g', &["0000", "0000", "0000", "0000", "0000", "0000", "0000", "0000", "0000", "0000", "0000", "0000", "1FFC", "3FFC", "781C", "701C", "701C", "701C", "701C", "701C", "701C", "701C", "701C", "781C", "3FFC", "1FFC", "001C", "001C", "003C", "3FF8", "3FF0", "0000"]),
        ];
        assert!(!cases.is_empty());
        for (font, ch, rows) in cases {
            assert_eq!(art(font, ch), bdf(font.cell_width(), rows), "{ch:?} at {}px", font.line_height());
        }
    }

    #[test]
    fn outside_the_subset_is_a_hollow_box() {
        for (name, f) in all() {
            let (w, h) = (f.cell_width() as usize, f.line_height() as usize);
            let want: Vec<String> = (0..h)
                .map(|y| {
                    (0..w)
                        .map(|x| {
                            let inside = (1..w - 1).contains(&x) && (1..h - 1).contains(&y);
                            let edge = x == 1 || x == w - 2 || y == 1 || y == h - 2;
                            if inside && edge {
                                '#'
                            } else {
                                '.'
                            }
                        })
                        .collect()
                })
                .collect();
            for ch in ['日', '\n', '\u{7f}', '\u{10ffff}'] {
                assert_eq!(art(f, ch), want, "{name} {ch:?}");
            }
        }
    }

    #[test]
    fn draws_a_line_and_clips_at_any_position() {
        for (name, f) in all() {
            let mut c = Canvas::new(100, 40);
            let end = f.draw(&mut c, "Hi!", 3, 2, 0xff0000);
            assert_eq!(end, 3 + 3 * f.cell_width() as i32, "{name}");
            assert_eq!(f.width("Hi!"), 3 * f.cell_width());
            for p in c.pixels() {
                assert!(*p == 0 || *p == 0xff0000);
            }
            for (x, y) in [(i32::MIN, i32::MIN), (i32::MAX, i32::MAX), (-5, -5), (98, 38)] {
                f.draw(&mut c, "Hé日", x, y, 0x00ff00);
            }
        }
    }
}
