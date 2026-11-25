//! Software drawing on an owned `0x00RRGGBB` image (feature `canvas`).
//!
//! A [`Canvas`] is the thing you draw a window's contents into before
//! presenting it: solid fills, alpha-blended fills, and blits — copied,
//! color-keyed, alpha-blended, or nearest-neighbor scaled — of any
//! [`Frame`], including another canvas's or a BMP sprite sheet decoded
//! by `canvas::bmp` (feature `bmp`). Every operation is clipped to
//! the canvas and to its clip rectangle, and is total: any rectangle, at
//! any coordinates, draws only what lands inside and never panics.
//!
//! ```
//! use protolith::canvas::{Blit, Canvas};
//! use protolith::Rect;
//!
//! let mut sheet = Canvas::new(16, 8);
//! sheet.fill_rect(Rect::new(0, 0, 8, 8), 0xff0000); // sprite 0: red
//! sheet.fill_rect(Rect::new(8, 0, 8, 8), 0x0000ff); // sprite 1: blue
//!
//! let mut screen = Canvas::new(64, 64);
//! screen.blit(&sheet.frame(), Rect::new(8, 0, 8, 8), 10, 10);
//! screen.blit_with(&sheet.frame(), Rect::new(0, 0, 8, 8), Rect::new(20, 20, 16, 16), Blit::Copy);
//! assert_eq!(screen.get(10, 10), Some(0x0000ff));
//! assert_eq!(screen.get(35, 35), Some(0xff0000)); // doubled
//! ```
//!
//! Blending is straight-alpha source-over with exact rounding, so alpha
//! 255 is the source and alpha 0 the destination, bit for bit.

use std::fmt;

use crate::pixel::{Frame, FrameError, Pixels, Rect};

#[cfg(feature = "bmp")]
pub mod bmp;

/// An owned `width × height` image of `0x00RRGGBB` pixels, with a clip
/// rectangle that every drawing operation respects.
#[derive(Clone, PartialEq, Eq)]
pub struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    /// Always inside `bounds()`.
    clip: Rect,
}

impl fmt::Debug for Canvas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Canvas").field("width", &self.width).field("height", &self.height).field("clip", &self.clip).finish_non_exhaustive()
    }
}

/// How [`Canvas::blit_with`] combines a source pixel with the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Blit {
    /// Replace the canvas pixel (source alpha, if any, is ignored).
    Copy,
    /// Like `Copy`, but source pixels of this `0x00RRGGBB` color are
    /// skipped — the classic sprite transparency.
    Key(u32),
    /// Blend by the source's alpha ([`Pixels::Rgba8`]); an
    /// [`Pixels::Xrgb8888`] source is opaque, so this is `Copy` for it.
    Alpha,
}

impl Canvas {
    /// A black canvas.
    ///
    /// # Panics
    ///
    /// If `width * height` pixels cannot be addressed (only possible on a
    /// 32-bit target), like any oversized allocation.
    pub fn new(width: u32, height: u32) -> Canvas {
        let len = (width as usize).checked_mul(height as usize).expect("canvas size overflows usize");
        Canvas { width, height, pixels: vec![0; len], clip: Rect::from_size(width, height) }
    }

    /// A canvas over existing `0x00RRGGBB` pixels, rows packed
    /// (`stride == width`). Extra pixels past `width * height` are dropped.
    pub fn from_pixels(mut pixels: Vec<u32>, width: u32, height: u32) -> Result<Canvas, FrameError> {
        Frame::xrgb(&pixels, width, height)?;
        pixels.truncate(width as usize * height as usize);
        Ok(Canvas { width, height, pixels, clip: Rect::from_size(width, height) })
    }

    /// A copy of `frame` (alpha, if any, dropped).
    pub fn from_frame(frame: &Frame<'_>) -> Canvas {
        let mut canvas = Canvas::new(frame.width(), frame.height());
        canvas.blit(frame, frame.bounds(), 0, 0);
        canvas
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// `(0, 0, width, height)`.
    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.width, self.height)
    }

    /// The pixels, row-major, `width` per row.
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }

    /// The pixels, mutably — for drawing of your own. Not clipped.
    pub fn pixels_mut(&mut self) -> &mut [u32] {
        &mut self.pixels
    }

    /// The pixel buffer, giving up the canvas.
    pub fn into_pixels(self) -> Vec<u32> {
        self.pixels
    }

    /// The canvas as a [`Frame`], to present or to blit from.
    pub fn frame(&self) -> Frame<'_> {
        Frame::xrgb(&self.pixels, self.width, self.height).expect("a canvas always holds width * height pixels")
    }

    /// The pixel at `(x, y)`, or `None` outside the canvas.
    pub fn get(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(self.pixels[y as usize * self.width as usize + x as usize] & 0x00ff_ffff)
    }

    /// The clip rectangle: drawing outside it has no effect.
    pub fn clip(&self) -> Rect {
        self.clip
    }

    /// Restrict drawing to `rect` (intersected with the canvas).
    pub fn set_clip(&mut self, rect: Rect) {
        self.clip = self.bounds().intersect(&rect);
    }

    /// Allow drawing anywhere on the canvas again.
    pub fn reset_clip(&mut self) {
        self.clip = self.bounds();
    }

    /// Set one pixel (clipped).
    pub fn put(&mut self, x: i32, y: i32, color: u32) {
        self.fill_rect(Rect::new(x, y, 1, 1), color);
    }

    /// Fill the whole clip rectangle.
    pub fn fill(&mut self, color: u32) {
        self.fill_rect(self.clip, color);
    }

    /// Fill `rect` (clipped) with `color`.
    pub fn fill_rect(&mut self, rect: Rect, color: u32) {
        let r = self.clip.intersect(&rect);
        let w = self.width as usize;
        for y in r.y..r.y + r.height as i32 {
            let start = y as usize * w + r.x as usize;
            self.pixels[start..start + r.width as usize].fill(color & 0x00ff_ffff);
        }
    }

    /// Blend `color` over `rect` (clipped) at opacity `alpha` (255 = opaque).
    pub fn blend_rect(&mut self, rect: Rect, color: u32, alpha: u8) {
        let r = self.clip.intersect(&rect);
        let w = self.width as usize;
        for y in r.y..r.y + r.height as i32 {
            let start = y as usize * w + r.x as usize;
            for d in &mut self.pixels[start..start + r.width as usize] {
                *d = blend(color, *d, alpha);
            }
        }
    }

    /// Paint `color` wherever a 1-bit `mask` is set — a bitmap glyph, an
    /// icon, a cursor. The mask is `width × height`, top row first, each row
    /// `ceil(width / 8)` bytes with the most significant bit leftmost (the
    /// BDF and PSF font layout). Drawn with its top-left at `(x, y)`,
    /// clipped; rows missing from a short `mask` are not drawn.
    pub fn fill_mask(&mut self, mask: &[u8], width: u32, height: u32, x: i32, y: i32, color: u32) {
        let row_bytes = width.div_ceil(8) as usize;
        let r = self.clip.intersect(&Rect::new(x, y, width, height));
        if r.is_empty() {
            return;
        }
        let w = self.width as usize;
        for dy in r.y..r.y + r.height as i32 {
            // Offsets into the mask are non-negative: r lies inside (x, y, width, height).
            let my = (i64::from(dy) - i64::from(y)) as usize;
            let Some(bits) = my.checked_mul(row_bytes).and_then(|o| mask.get(o..)?.get(..row_bytes)) else { break };
            for dx in r.x..r.x + r.width as i32 {
                let mx = (i64::from(dx) - i64::from(x)) as usize;
                if bits[mx / 8] & (0x80 >> (mx % 8)) != 0 {
                    self.pixels[dy as usize * w + dx as usize] = color & 0x00ff_ffff;
                }
            }
        }
    }

    /// Copy the `from` area of `src` with its top-left at `(x, y)`.
    /// Parts of `from` outside `src` are not drawn.
    pub fn blit(&mut self, src: &Frame<'_>, from: Rect, x: i32, y: i32) {
        self.blit_with(src, from, Rect::new(x, y, from.width, from.height), Blit::Copy);
    }

    /// Draw the `from` area of `src` into `to`, scaled nearest-neighbor
    /// when the two differ in size, combining pixels per `mode`.
    ///
    /// Each canvas pixel in `to` takes the source pixel its position maps
    /// to (`from.x + (dx - to.x) * from.width / to.width`, likewise for
    /// y); positions that map outside `src` are left alone.
    pub fn blit_with(&mut self, src: &Frame<'_>, from: Rect, to: Rect, mode: Blit) {
        let vis = self.clip.intersect(&to);
        if from.is_empty() || vis.is_empty() {
            return;
        }
        // Canvas offset (non-negative, < to's extent) → source coordinate,
        // in 64 bits: offset and size are each < 2^32, so the product fits.
        let map = |d: i32, to0: i32, to_len: u32, from0: i32, from_len: u32| -> i64 {
            let r = (i64::from(d) - i64::from(to0)) as u64;
            let scaled = if from_len == to_len { r } else { r * u64::from(from_len) / u64::from(to_len) };
            i64::from(from0) + scaled as i64
        };
        let (sw, sh) = (i64::from(src.width()), i64::from(src.height()));
        let (stride, pixels, w) = (src.stride(), src.pixels(), self.width as usize);
        for dy in vis.y..vis.y + vis.height as i32 {
            let sy = map(dy, to.y, to.height, from.y, from.height);
            if !(0..sh).contains(&sy) {
                continue;
            }
            let row = dy as usize * w;
            for dx in vis.x..vis.x + vis.width as i32 {
                let sx = map(dx, to.x, to.width, from.x, from.width);
                if !(0..sw).contains(&sx) {
                    continue;
                }
                // In bounds: the Frame was validated for its width, height
                // and stride, and (sx, sy) is inside them.
                let i = sy as usize * stride + sx as usize;
                let (c, a) = match pixels {
                    Pixels::Xrgb8888(p) => (p[i] & 0x00ff_ffff, 255),
                    Pixels::Rgba8(b) => (u32::from(b[i * 4]) << 16 | u32::from(b[i * 4 + 1]) << 8 | u32::from(b[i * 4 + 2]), b[i * 4 + 3]),
                };
                let d = &mut self.pixels[row + dx as usize];
                match mode {
                    Blit::Copy => *d = c,
                    Blit::Key(k) if c == k & 0x00ff_ffff => {}
                    Blit::Key(_) => *d = c,
                    Blit::Alpha => *d = blend(c, *d, a),
                }
            }
        }
    }
}

/// `src` over `dst` at opacity `alpha`, per channel, rounded to nearest.
#[inline]
fn blend(src: u32, dst: u32, alpha: u8) -> u32 {
    match alpha {
        255 => src & 0x00ff_ffff,
        0 => dst & 0x00ff_ffff,
        _ => {
            let a = u32::from(alpha);
            let mix = |shift: u32| div255(((src >> shift) & 0xff) * a + ((dst >> shift) & 0xff) * (255 - a)) << shift;
            mix(16) | mix(8) | mix(0)
        }
    }
}

/// `round(x / 255)` for `x <= 255 * 255`, without a division.
#[inline]
fn div255(x: u32) -> u32 {
    let x = x + 128;
    (x + (x >> 8)) >> 8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rng(seed: u64) -> impl FnMut() -> u32 {
        let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 32) as u32
        }
    }

    /// A canvas whose every pixel is distinct: `y << 8 | x`.
    fn numbered(w: u32, h: u32) -> Canvas {
        Canvas::from_pixels((0..h).flat_map(|y| (0..w).map(move |x| y << 8 | x)).collect(), w, h).unwrap()
    }

    #[test]
    fn new_is_black_and_sized() {
        let c = Canvas::new(3, 2);
        assert_eq!(c.pixels(), &[0; 6]);
        assert_eq!((c.width(), c.height(), c.clip()), (3, 2, Rect::new(0, 0, 3, 2)));
        assert_eq!(c.get(2, 1), Some(0));
        assert_eq!(c.get(3, 0), None);
        assert_eq!(c.frame().bounds(), c.bounds());
        assert!(Canvas::new(0, 5).pixels().is_empty());
    }

    #[test]
    fn from_pixels_checks_length() {
        assert_eq!(Canvas::from_pixels(vec![0; 5], 3, 2).unwrap_err(), FrameError { needed: 6, got: 5 });
        assert_eq!(Canvas::from_pixels(vec![7; 9], 3, 2).unwrap().into_pixels(), vec![7; 6]);
    }

    #[test]
    fn from_frame_copies_and_drops_alpha() {
        let bytes = [1, 2, 3, 0, 4, 5, 6, 255];
        let c = Canvas::from_frame(&Frame::rgba8(&bytes, 2, 1).unwrap());
        assert_eq!(c.pixels(), &[0x010203, 0x040506]);
        let px = [0xff11_2233u32, 0, 0, 0x44_5566];
        let c = Canvas::from_frame(&Frame::xrgb_with_stride(&px, 1, 2, 3).unwrap());
        assert_eq!(c.pixels(), &[0x11_2233, 0x44_5566], "stride honored, top byte cleared");
    }

    #[test]
    fn fill_rect_clips_to_canvas() {
        let mut c = Canvas::new(4, 3);
        c.fill_rect(Rect::new(-2, 1, 4, 10), 0xffab_cdef);
        #[rustfmt::skip]
        assert_eq!(c.pixels(), &[
            0, 0, 0, 0,
            0xabcdef, 0xabcdef, 0, 0,
            0xabcdef, 0xabcdef, 0, 0,
        ]);
    }

    #[test]
    fn extreme_rects_do_not_panic() {
        let mut c = Canvas::new(4, 4);
        let src = numbered(4, 4);
        for r in [
            Rect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX),
            Rect::new(i32::MAX, i32::MAX, u32::MAX, u32::MAX),
            Rect::new(-1, -1, 0, 0),
            Rect::new(3, 3, u32::MAX, 1),
        ] {
            c.fill_rect(r, 1);
            c.blend_rect(r, 1, 128);
            c.set_clip(r);
            c.reset_clip();
            for mode in [Blit::Copy, Blit::Key(0), Blit::Alpha] {
                c.blit_with(&src.frame(), r, Rect::new(0, 0, 4, 4), mode);
                c.blit_with(&src.frame(), Rect::new(0, 0, 4, 4), r, mode);
                c.blit_with(&src.frame(), r, r, mode);
            }
            c.blit(&src.frame(), r, r.x, r.y);
        }
    }

    #[test]
    fn clip_restricts_every_operation() {
        let src = numbered(4, 4);
        type Op = fn(&mut Canvas, &Canvas);
        let ops: [Op; 6] = [
            |c, _| c.fill(0xffffff),
            |c, _| c.fill_rect(Rect::new(0, 0, 9, 9), 0xffffff),
            |c, _| c.blend_rect(Rect::new(0, 0, 9, 9), 0xffffff, 99),
            |c, _| c.put(1, 1, 0xffffff),
            |c, s| c.blit(&s.frame(), s.bounds(), 0, 0),
            |c, s| c.blit_with(&s.frame(), s.bounds(), Rect::new(0, 0, 8, 8), Blit::Alpha),
        ];
        for op in ops {
            let mut c = Canvas::new(4, 4);
            c.fill(0x123456);
            c.set_clip(Rect::new(1, 1, 2, 9)); // clamped to (1, 1, 2, 3)
            assert_eq!(c.clip(), Rect::new(1, 1, 2, 3));
            op(&mut c, &src);
            for y in 0..4 {
                for x in 0..4 {
                    if !c.clip().contains(x as i32, y as i32) {
                        assert_eq!(c.get(x, y), Some(0x123456), "({x}, {y}) is outside the clip");
                    }
                }
            }
            c.reset_clip();
            assert_eq!(c.clip(), c.bounds());
        }
    }

    #[test]
    fn div255_rounds_exactly() {
        for x in 0..=255 * 255u32 {
            assert_eq!(div255(x), (f64::from(x) / 255.0).round() as u32, "{x}");
        }
    }

    #[test]
    fn blend_endpoints_and_midpoint() {
        assert_eq!(blend(0xff12_3456, 0xabcdef, 255), 0x123456);
        assert_eq!(blend(0x123456, 0xffab_cdef, 0), 0xabcdef);
        assert_eq!(blend(0xffffff, 0x000000, 128), 0x808080);
        assert_eq!(blend(0x000000, 0xffffff, 128), 0x7f7f7f);
        // Channels do not bleed into each other.
        assert_eq!(blend(0xff0000, 0x0000ff, 51), 0x3300cc);
    }

    #[test]
    fn blend_rect_blends() {
        let mut c = Canvas::new(2, 1);
        c.fill(0x0000ff);
        c.blend_rect(Rect::new(1, 0, 5, 5), 0xff0000, 51);
        assert_eq!(c.pixels(), &[0x0000ff, 0x3300cc]);
    }

    #[test]
    fn blit_copies_a_sub_rect_from_a_strided_source() {
        // 3x2 source with stride 4; the 4th column is padding that must never be read.
        let px = [1, 2, 3, 99, 4, 5, 6];
        let src = Frame::xrgb_with_stride(&px, 3, 2, 4).unwrap();
        let mut c = Canvas::new(3, 3);
        c.blit(&src, Rect::new(1, 0, 2, 2), 1, 1);
        assert_eq!(c.pixels(), &[0, 0, 0, 0, 2, 3, 0, 5, 6]);
    }

    #[test]
    fn blit_partly_outside_source_and_canvas() {
        let src = numbered(4, 4);
        let mut c = Canvas::new(4, 4);
        c.fill(0xeeeeee);
        // from (-1,-1)..(3,3): its first row and column are outside the
        // source; drawn at (-1, 2), so the canvas loses the left column too.
        c.blit(&src.frame(), Rect::new(-1, -1, 4, 4), -1, 2);
        const E: u32 = 0xeeeeee;
        #[rustfmt::skip]
        assert_eq!(c.pixels(), &[
            E, E, E, E,
            E, E, E, E,
            E, E, E, E,
            0x000, 0x001, 0x002, E,
        ]);
    }

    /// Unscaled `blit_with` against a direct per-pixel definition, over
    /// random rectangles, clips and modes.
    #[test]
    fn unscaled_blit_matches_reference() {
        let mut next = rng(7);
        let mut coord = || (next() % 15) as i32 - 5;
        for _ in 0..3000 {
            let (sw, sh, cw, ch) = (6, 5, 7, 6);
            let src = numbered(sw, sh);
            let mut c = Canvas::new(cw, ch);
            c.fill(0xc0ffee);
            let clip = Rect::new(coord(), coord(), coord().unsigned_abs(), coord().unsigned_abs());
            let from = Rect::new(coord(), coord(), coord().unsigned_abs(), coord().unsigned_abs());
            let (x, y) = (coord(), coord());
            let key = if coord() > 0 { Blit::Key(0x102) } else { Blit::Copy };
            let mut want = c.clone();
            c.set_clip(clip);
            c.blit_with(&src.frame(), from, Rect::new(x, y, from.width, from.height), key);
            let clip = want.bounds().intersect(&clip);
            for dy in 0..ch as i32 {
                for dx in 0..cw as i32 {
                    let (sx, sy) = (from.x + dx - x, from.y + dy - y);
                    let inside = from.contains(sx, sy) && src.bounds().contains(sx, sy) && clip.contains(dx, dy);
                    if inside {
                        let s = src.get(sx as u32, sy as u32).unwrap();
                        if key != Blit::Key(s) {
                            want.pixels_mut()[(dy * cw as i32 + dx) as usize] = s;
                        }
                    }
                }
            }
            assert_eq!(c.pixels(), want.pixels(), "clip {clip:?} from {from:?} at ({x}, {y}) {key:?}");
        }
    }

    #[test]
    fn fill_mask_is_msb_first_and_multi_byte() {
        // 10 wide → 2 bytes per row. Row 0: pixels 0, 7, 8, 9; row 1: pixel 1.
        let mask = [0b1000_0001, 0b1100_0000, 0b0100_0000, 0b0011_1111];
        let mut c = Canvas::new(12, 2);
        c.fill_mask(&mask, 10, 2, 1, 0, 0xffab_cdef);
        let lit: Vec<(u32, u32)> = (0..2).flat_map(|y| (0..12).map(move |x| (x, y))).filter(|&(x, y)| c.get(x, y) == Some(0xabcdef)).collect();
        assert_eq!(lit, [(1, 0), (8, 0), (9, 0), (10, 0), (2, 1)], "the unused low bits of a row's last byte are ignored");
    }

    #[test]
    fn fill_mask_clips_and_tolerates_short_masks() {
        let mask = [0xff; 3]; // 8x3, all set
        let mut c = Canvas::new(4, 4);
        c.set_clip(Rect::new(1, 0, 9, 9));
        c.fill_mask(&mask, 8, 3, -4, 2, 1);
        assert_eq!(c.pixels(), &[0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1]);
        let mut c = Canvas::new(2, 4);
        c.fill_mask(&mask[..2], 8, 3, 0, 0, 1);
        assert_eq!(c.pixels(), &[1, 1, 1, 1, 0, 0, 0, 0], "the missing third row is not drawn");
        c.fill_mask(&mask, u32::MAX, u32::MAX, i32::MIN, i32::MIN, 1);
        c.fill_mask(&[], 8, 8, 0, 0, 2);
        c.fill_mask(&mask, 0, 3, 0, 0, 2);
        assert!(!c.pixels().contains(&2));
    }

    #[test]
    fn color_key_skips_only_the_key() {
        let src = Canvas::from_pixels(vec![0xff00ff, 0x123456, 0xff00fe], 3, 1).unwrap();
        let mut c = Canvas::new(3, 1);
        c.fill(0x777777);
        c.blit_with(&src.frame(), src.bounds(), c.bounds(), Blit::Key(0xaaff_00ff));
        assert_eq!(c.pixels(), &[0x777777, 0x123456, 0xff00fe]);
    }

    #[test]
    fn alpha_blit_uses_rgba_alpha_and_treats_xrgb_as_opaque() {
        let bytes = [255, 0, 0, 0, 255, 0, 0, 51, 255, 0, 0, 255];
        let src = Frame::rgba8(&bytes, 3, 1).unwrap();
        let mut c = Canvas::new(3, 1);
        c.fill(0x0000ff);
        c.blit_with(&src, src.bounds(), c.bounds(), Blit::Alpha);
        assert_eq!(c.pixels(), &[0x0000ff, 0x3300cc, 0xff0000]);

        let opaque = Canvas::from_pixels(vec![0xff0000; 3], 3, 1).unwrap();
        c.fill(0x0000ff);
        c.blit_with(&opaque.frame(), opaque.bounds(), c.bounds(), Blit::Alpha);
        assert_eq!(c.pixels(), &[0xff0000; 3]);

        // Copy ignores alpha entirely.
        c.fill(0x0000ff);
        c.blit_with(&src, src.bounds(), c.bounds(), Blit::Copy);
        assert_eq!(c.pixels(), &[0xff0000; 3]);
    }

    #[test]
    fn scaled_up_by_two_repeats_each_pixel() {
        let src = numbered(2, 2);
        let mut c = Canvas::new(4, 4);
        c.blit_with(&src.frame(), src.bounds(), c.bounds(), Blit::Copy);
        #[rustfmt::skip]
        assert_eq!(c.pixels(), &[
            0x000, 0x000, 0x001, 0x001,
            0x000, 0x000, 0x001, 0x001,
            0x100, 0x100, 0x101, 0x101,
            0x100, 0x100, 0x101, 0x101,
        ]);
    }

    #[test]
    fn scaled_down_by_two_takes_every_other_pixel() {
        let src = numbered(4, 4);
        let mut c = Canvas::new(2, 2);
        c.blit_with(&src.frame(), src.bounds(), c.bounds(), Blit::Copy);
        assert_eq!(c.pixels(), &[0x000, 0x002, 0x200, 0x202]);
    }

    #[test]
    fn scaled_blit_clipped_on_the_left_keeps_its_mapping() {
        // A 3x upscale of a 2-pixel row drawn from x = -2: canvas column 0
        // is the third destination column, which still maps to source 0.
        let src = numbered(2, 1);
        let mut c = Canvas::new(4, 1);
        c.fill(0xeeeeee);
        c.blit_with(&src.frame(), src.bounds(), Rect::new(-2, 0, 6, 1), Blit::Copy);
        assert_eq!(c.pixels(), &[0x000, 0x001, 0x001, 0x001]);
    }

    #[test]
    fn huge_scaled_destination_is_bounded_by_the_canvas() {
        let src = numbered(2, 2);
        let mut c = Canvas::new(3, 3);
        c.blit_with(&src.frame(), src.bounds(), Rect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX), Blit::Copy);
        // Canvas (0..3) is at offset 2^31 into a 2^32-1 destination: the second half.
        assert_eq!(c.pixels(), &[0x101; 9]);
    }
}
