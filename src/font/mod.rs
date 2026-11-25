//! Bitmap text on a [`Canvas`] (feature `font`).
//!
//! Two kinds of font, behind one trait, [`Font`]:
//!
//! - [`SheetFont`] — glyphs cut from an image: a skin's `text.bmp`-style
//!   sprite sheet of fixed-size cells, in whatever order the sheet uses.
//! - `Terminus` — a 191-glyph subset (ASCII and Latin-1) of
//!   [Terminus Font](https://terminus-font.sourceforge.net/), drawn in any
//!   color. Each size and weight is its own cargo feature, so a program
//!   carries only the bitmaps it asked for: `font-terminus-16` (3 KB of
//!   glyph data), `font-terminus-16-bold`, and likewise for 20, 24 and 32
//!   pixels, and `font-terminus-12` (regular only; upstream has no 12 px
//!   bold). Terminus Font is © Dimitar Toshkov Zhekov, under the SIL
//!   Open Font License 1.1 (`third_party/terminus-font/` in the source).
//!
//! Text is drawn one line at a time with its top-left at the given point;
//! there is no layout, wrapping or shaping. Every glyph of a font is one
//! cell wide.
//!
//! ```
//! use protolith::canvas::Canvas;
//! use protolith::font::{Font, SheetFont};
//! use protolith::Rect;
//!
//! // A 3x5-pixel-cell sheet: "01" on the first row, "2" on the second.
//! let mut sheet = Canvas::new(6, 10);
//! sheet.fill_rect(Rect::new(0, 0, 3, 5), 0xff0000);
//! let digits = SheetFont::new(sheet, 3, 5, "01\n2");
//!
//! let mut screen = Canvas::new(32, 8);
//! let end = digits.draw(&mut screen, "1020", 2, 1, 0);
//! assert_eq!(end, 2 + 4 * 3);
//! assert_eq!(screen.get(5, 1), Some(0xff0000)); // the second glyph, '0'
//! ```

use crate::canvas::{Blit, Canvas};
use crate::pixel::Rect;

#[cfg(any(
    feature = "font-terminus-12",
    feature = "font-terminus-16",
    feature = "font-terminus-16-bold",
    feature = "font-terminus-20",
    feature = "font-terminus-20-bold",
    feature = "font-terminus-24",
    feature = "font-terminus-24-bold",
    feature = "font-terminus-32",
    feature = "font-terminus-32-bold",
))]
mod terminus;
#[cfg(any(
    feature = "font-terminus-12",
    feature = "font-terminus-16",
    feature = "font-terminus-16-bold",
    feature = "font-terminus-20",
    feature = "font-terminus-20-bold",
    feature = "font-terminus-24",
    feature = "font-terminus-24-bold",
    feature = "font-terminus-32",
    feature = "font-terminus-32-bold",
))]
pub use terminus::Terminus;

/// A bitmap font that draws onto a [`Canvas`].
pub trait Font {
    /// Height of a line of text, in pixels.
    fn line_height(&self) -> u32;

    /// How far `ch` moves the pen, in pixels.
    fn advance(&self, ch: char) -> u32;

    /// Draw `ch` with its cell's top-left at `(x, y)`, in `color` where the
    /// font has no colors of its own. Returns the advance.
    fn draw_char(&self, canvas: &mut Canvas, ch: char, x: i32, y: i32, color: u32) -> u32;

    /// Draw `text` on one line from `(x, y)`; returns the pen position
    /// after the last character.
    fn draw(&self, canvas: &mut Canvas, text: &str, x: i32, y: i32, color: u32) -> i32 {
        text.chars().fold(x, |x, ch| x.saturating_add(i32::try_from(self.draw_char(canvas, ch, x, y, color)).unwrap_or(i32::MAX)))
    }

    /// The width `text` would take, in pixels.
    fn width(&self, text: &str) -> u32 {
        text.chars().fold(0, |w, ch| w.saturating_add(self.advance(ch)))
    }
}

/// A font cut from an image of fixed-size cells — the classic skin
/// `text.bmp` arrangement.
///
/// The `layout` string names the cells in sheet order: each line of it is
/// one row of cells, left to right. A character that appears more than
/// once uses its first cell; characters not in the layout draw the
/// [fallback](SheetFont::with_fallback) cell, or nothing.
#[derive(Debug, Clone)]
pub struct SheetFont {
    sheet: Canvas,
    cell_width: u32,
    cell_height: u32,
    cells: Vec<(char, Rect)>,
    fallback: Option<Rect>,
    mode: Blit,
}

impl SheetFont {
    /// A font over `sheet`, whose cells are `cell_width × cell_height`
    /// and named, row by row, by the lines of `layout`. Glyphs are copied
    /// as they are ([`Blit::Copy`]); see [`SheetFont::with_blit`].
    pub fn new(sheet: Canvas, cell_width: u32, cell_height: u32, layout: &str) -> SheetFont {
        // Lookups take the first match, so a repeated character keeps its first cell.
        let at = |i: usize, size: u32| (i as i64).saturating_mul(i64::from(size)).min(i64::from(i32::MAX)) as i32;
        let cells = layout
            .split('\n')
            .enumerate()
            .flat_map(|(row, line)| {
                line.chars().enumerate().map(move |(col, ch)| (ch, Rect::new(at(col, cell_width), at(row, cell_height), cell_width, cell_height)))
            })
            .collect();
        SheetFont { sheet, cell_width, cell_height, cells, fallback: None, mode: Blit::Copy }
    }

    /// Draw characters missing from the layout as `ch`'s cell (if `ch` is
    /// in the layout) instead of leaving their cell blank.
    pub fn with_fallback(mut self, ch: char) -> SheetFont {
        self.fallback = self.cell(ch);
        self
    }

    /// How glyph cells are combined with the canvas — e.g.
    /// [`Blit::Key`] to let a background color show through.
    pub fn with_blit(mut self, mode: Blit) -> SheetFont {
        self.mode = mode;
        self
    }

    /// The sheet the glyphs are cut from.
    pub fn sheet(&self) -> &Canvas {
        &self.sheet
    }

    fn cell(&self, ch: char) -> Option<Rect> {
        self.cells.iter().find(|(c, _)| *c == ch).map(|(_, r)| *r)
    }
}

impl Font for SheetFont {
    fn line_height(&self) -> u32 {
        self.cell_height
    }

    fn advance(&self, _ch: char) -> u32 {
        self.cell_width
    }

    /// `color` is unused: a sheet's glyphs carry their own.
    fn draw_char(&self, canvas: &mut Canvas, ch: char, x: i32, y: i32, _color: u32) -> u32 {
        if let Some(from) = self.cell(ch).or(self.fallback) {
            canvas.blit_with(&self.sheet.frame(), from, Rect::new(x, y, self.cell_width, self.cell_height), self.mode);
        }
        self.cell_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3-cell-wide, 2-row sheet of 2x2 cells; cell (col, row) is filled
    /// with the color `row << 4 | col`, plus 1 so no cell is black.
    fn sheet() -> Canvas {
        let mut s = Canvas::new(6, 4);
        for row in 0..2 {
            for col in 0..3 {
                s.fill_rect(Rect::new(col * 2, row * 2, 2, 2), (row << 4 | col) as u32 + 1);
            }
        }
        s
    }

    /// The color of each 2x2 cell along a 1-cell-high strip of `c`.
    fn strip(c: &Canvas) -> Vec<u32> {
        (0..c.width() / 2).map(|i| c.get(i * 2, 0).unwrap()).collect()
    }

    #[test]
    fn layout_lines_are_rows() {
        let font = SheetFont::new(sheet(), 2, 2, "abc\nde");
        let mut c = Canvas::new(12, 2);
        assert_eq!(font.draw(&mut c, "edcba", 0, 0, 0), 10);
        assert_eq!(strip(&c), [0x12, 0x11, 0x03, 0x02, 0x01, 0]);
    }

    #[test]
    fn first_occurrence_wins_and_unknowns_leave_a_gap() {
        let font = SheetFont::new(sheet(), 2, 2, "aba\nbX");
        let mut c = Canvas::new(8, 2);
        c.fill(0x777777);
        font.draw(&mut c, "b?Xa", 0, 0, 0);
        assert_eq!(strip(&c), [0x02, 0x777777, 0x12, 0x01]);
    }

    #[test]
    fn fallback_draws_for_unknowns() {
        let font = SheetFont::new(sheet(), 2, 2, "abc\nd?").with_fallback('?');
        let mut c = Canvas::new(6, 2);
        font.draw(&mut c, "a日\n", 0, 0, 0);
        assert_eq!(strip(&c), [0x01, 0x12, 0x12]);
        // A fallback that is not in the layout means "draw nothing".
        let font = SheetFont::new(sheet(), 2, 2, "a").with_fallback('#');
        let mut c = Canvas::new(2, 2);
        font.draw(&mut c, "z", 0, 0, 0);
        assert_eq!(strip(&c), [0]);
    }

    #[test]
    fn blit_mode_applies_to_glyphs() {
        let font = SheetFont::new(sheet(), 2, 2, "abc").with_blit(Blit::Key(0x02));
        let mut c = Canvas::new(4, 2);
        c.fill(0x777777);
        font.draw(&mut c, "ba", 0, 0, 0);
        assert_eq!(strip(&c), [0x777777, 0x01]);
    }

    #[test]
    fn measures_and_returns_the_pen() {
        let font = SheetFont::new(sheet(), 2, 2, "abc");
        assert_eq!((font.line_height(), font.advance('a'), font.width("héllo")), (2, 2, 10));
        let mut c = Canvas::new(1, 1);
        assert_eq!(font.draw(&mut c, "aa", -7, 0, 0), -3);
        assert_eq!(font.draw(&mut c, "aaa", i32::MAX - 3, 0, 0), i32::MAX, "the pen saturates");
        assert_eq!(font.width(""), 0);
    }

    #[test]
    fn huge_cells_and_layouts_do_not_overflow() {
        let font = SheetFont::new(sheet(), u32::MAX, u32::MAX, "ab\ncd");
        let mut c = Canvas::new(4, 4);
        font.draw(&mut c, "dcba", i32::MIN, i32::MIN, 0);
        assert_eq!(font.width("ab"), u32::MAX);
    }
}
