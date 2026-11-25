//! Frames, rectangles, and pixel-format conversion — the part both
//! presenters ([`x11`](crate::x11) and [`fbdev`](crate::fbdev)) share.
//!
//! The application always hands protolith pixels in one of two simple
//! source layouts ([`Pixels`]): `0x00RRGGBB` words, or `[R, G, B, A]` bytes.
//! The destination layout is described by a [`PixelFormat`] — channel masks,
//! bytes per pixel, byte order — derived from the X server's visual or the
//! framebuffer's geometry. Conversion is a pure function of the two, so it is
//! tested exhaustively here, with no display involved.
//!
//! Channel reduction truncates (keeps the top bits), the same as every X
//! server and framebuffer console does it: `0xFF` stays all-ones at any
//! channel width, `0x00` stays zero, and nothing in between is dithered.

use std::fmt;

/// An integer pixel rectangle, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl Rect {
    /// A rectangle from its position and size.
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Rect {
        Rect { x, y, width, height }
    }

    /// The rectangle `(0, 0, width, height)`.
    pub const fn from_size(width: u32, height: u32) -> Rect {
        Rect { x: 0, y: 0, width, height }
    }

    /// `true` when the rectangle covers no pixels.
    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// The overlap of `self` and `other` (empty, at `self`'s origin, when
    /// they do not overlap). Computed in 64-bit so no edge can overflow.
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x0 = i64::from(self.x).max(i64::from(other.x));
        let y0 = i64::from(self.y).max(i64::from(other.y));
        let x1 = (i64::from(self.x) + i64::from(self.width)).min(i64::from(other.x) + i64::from(other.width));
        let y1 = (i64::from(self.y) + i64::from(self.height)).min(i64::from(other.y) + i64::from(other.height));
        if x1 <= x0 || y1 <= y0 {
            return Rect { x: self.x, y: self.y, width: 0, height: 0 };
        }
        // x0/y0 are each one of the two i32 inputs, and the extents are
        // bounded by the inputs' own u32 extents, so these casts are exact.
        Rect { x: x0 as i32, y: y0 as i32, width: (x1 - x0) as u32, height: (y1 - y0) as u32 }
    }

    /// The smallest rectangle covering both (an empty rectangle contributes
    /// nothing). Saturates rather than overflowing at the `i32` edges.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x0 = i64::from(self.x).min(i64::from(other.x));
        let y0 = i64::from(self.y).min(i64::from(other.y));
        let x1 = (i64::from(self.x) + i64::from(self.width)).max(i64::from(other.x) + i64::from(other.width));
        let y1 = (i64::from(self.y) + i64::from(self.height)).max(i64::from(other.y) + i64::from(other.height));
        Rect { x: x0 as i32, y: y0 as i32, width: (x1 - x0).min(i64::from(u32::MAX)) as u32, height: (y1 - y0).min(i64::from(u32::MAX)) as u32 }
    }

    /// Does the rectangle contain the point? Left/top edges inclusive,
    /// right/bottom exclusive.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        let (x, y) = (i64::from(x), i64::from(y));
        x >= i64::from(self.x) && y >= i64::from(self.y) && x < i64::from(self.x) + i64::from(self.width) && y < i64::from(self.y) + i64::from(self.height)
    }
}

/// The pixel data of a [`Frame`].
#[derive(Clone, Copy)]
pub enum Pixels<'a> {
    /// One `u32` per pixel, `0x00RRGGBB` (the top byte is ignored). The
    /// natural layout for software renderers and decoded BMP skins.
    Xrgb8888(&'a [u32]),
    /// Four bytes per pixel, `[R, G, B, A]`. Alpha is ignored: a window or
    /// framebuffer has nothing behind it to blend against.
    Rgba8(&'a [u8]),
}

impl fmt::Debug for Pixels<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pixels::Xrgb8888(p) => write!(f, "Xrgb8888({} px)", p.len()),
            Pixels::Rgba8(p) => write!(f, "Rgba8({} bytes)", p.len()),
        }
    }
}

/// A borrowed image to present: dimensions, row stride, and pixels.
///
/// Construction validates that the buffer is large enough, so every later
/// read is in bounds by construction.
#[derive(Debug, Clone, Copy)]
pub struct Frame<'a> {
    width: u32,
    height: u32,
    /// Row stride in *pixels*.
    stride: usize,
    pixels: Pixels<'a>,
}

/// Why a [`Frame`] could not be built from a buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameError {
    /// Pixels (or bytes, for [`Pixels::Rgba8`]) the dimensions require.
    pub needed: usize,
    /// Pixels (or bytes) the buffer actually holds.
    pub got: usize,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "frame buffer too small: need {}, got {}", self.needed, self.got)
    }
}

impl std::error::Error for FrameError {}

impl<'a> Frame<'a> {
    /// A tightly packed `0x00RRGGBB` frame (`stride == width`).
    pub fn xrgb(pixels: &'a [u32], width: u32, height: u32) -> Result<Frame<'a>, FrameError> {
        Frame::xrgb_with_stride(pixels, width, height, width as usize)
    }

    /// A `0x00RRGGBB` frame whose rows are `stride` pixels apart
    /// (`stride >= width`).
    pub fn xrgb_with_stride(pixels: &'a [u32], width: u32, height: u32, stride: usize) -> Result<Frame<'a>, FrameError> {
        let needed = required_len(width, height, stride).ok_or(FrameError { needed: usize::MAX, got: pixels.len() })?;
        if pixels.len() < needed {
            return Err(FrameError { needed, got: pixels.len() });
        }
        Ok(Frame { width, height, stride, pixels: Pixels::Xrgb8888(pixels) })
    }

    /// A tightly packed `[R, G, B, A]` byte frame.
    pub fn rgba8(bytes: &'a [u8], width: u32, height: u32) -> Result<Frame<'a>, FrameError> {
        let stride = width as usize;
        let needed = required_len(width, height, stride).and_then(|px| px.checked_mul(4)).ok_or(FrameError { needed: usize::MAX, got: bytes.len() })?;
        if bytes.len() < needed {
            return Err(FrameError { needed, got: bytes.len() });
        }
        Ok(Frame { width, height, stride, pixels: Pixels::Rgba8(bytes) })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The pixel data.
    pub fn pixels(&self) -> Pixels<'a> {
        self.pixels
    }

    /// Row stride in pixels.
    #[cfg(feature = "canvas")]
    pub(crate) fn stride(&self) -> usize {
        self.stride
    }

    /// `(0, 0, width, height)`.
    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.width, self.height)
    }

    /// The pixel at `(x, y)` as `0x00RRGGBB`, or `None` outside the frame.
    pub fn get(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = y as usize * self.stride + x as usize;
        Some(match self.pixels {
            Pixels::Xrgb8888(p) => p[i] & 0x00ff_ffff,
            Pixels::Rgba8(b) => {
                let o = i * 4;
                u32::from(b[o]) << 16 | u32::from(b[o + 1]) << 8 | u32::from(b[o + 2])
            }
        })
    }
}

/// `(height - 1) * stride + width` — the last row need not be padded out to
/// the full stride. `None` when it overflows or `stride < width`.
fn required_len(width: u32, height: u32, stride: usize) -> Option<usize> {
    if width == 0 || height == 0 {
        return Some(0);
    }
    if stride < width as usize {
        return None;
    }
    (height as usize - 1).checked_mul(stride)?.checked_add(width as usize)
}

/// Byte order of multi-byte pixels in the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ByteOrder {
    /// Least significant byte first (every x86 server and framebuffer).
    LsbFirst,
    /// Most significant byte first.
    MsbFirst,
}

/// One channel's position inside a packed pixel: `bits` wide, starting at
/// bit `shift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Channel {
    shift: u8,
    bits: u8,
}

impl Channel {
    /// Derive from a contiguous mask (e.g. `0xF800`). `None` for an empty
    /// or non-contiguous mask, which no real visual uses.
    fn from_mask(mask: u32) -> Option<Channel> {
        if mask == 0 {
            return None;
        }
        let shift = mask.trailing_zeros();
        let bits = (mask >> shift).trailing_ones();
        if (mask >> shift) >> bits != 0 {
            return None; // a hole in the mask
        }
        Some(Channel { shift: shift as u8, bits: bits as u8 })
    }

    /// Place an 8-bit channel value into this channel's bits. Narrower
    /// channels keep the top bits; wider ones replicate (so 0xFF stays
    /// all-ones in, say, a 10-bit channel).
    #[inline]
    fn place(self, v: u8) -> u32 {
        let v = u32::from(v);
        let bits = u32::from(self.bits);
        let scaled = if bits <= 8 {
            v >> (8 - bits)
        } else {
            let mut acc = 0u32;
            let mut filled = 0u32;
            while filled < bits {
                acc = (acc << 8) | v;
                filled += 8;
            }
            acc >> (filled - bits)
        };
        scaled << self.shift
    }
}

/// A destination pixel layout: which bits hold red, green and blue, how many
/// bytes each pixel occupies, and in which byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelFormat {
    red: Channel,
    green: Channel,
    blue: Channel,
    bytes_per_pixel: u8,
    byte_order: ByteOrder,
}

/// Why a [`PixelFormat`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PixelFormatError {
    /// Only 1, 2, 3 and 4 bytes per pixel exist.
    BytesPerPixel(u32),
    /// A channel mask is zero or has holes in it.
    Mask(u32),
    /// A channel mask does not fit in the pixel, or masks overlap.
    Layout,
}

impl fmt::Display for PixelFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PixelFormatError::BytesPerPixel(n) => write!(f, "unsupported pixel size: {n} bytes"),
            PixelFormatError::Mask(m) => write!(f, "unusable channel mask {m:#x}"),
            PixelFormatError::Layout => write!(f, "channel masks overlap or exceed the pixel size"),
        }
    }
}

impl std::error::Error for PixelFormatError {}

impl PixelFormat {
    /// 32-bit `0x00RRGGBB`, little-endian: the layout of every 24/32-bit
    /// x86 X server and framebuffer (`[B, G, R, X]` in memory).
    pub const XRGB8888: PixelFormat = PixelFormat {
        red: Channel { shift: 16, bits: 8 },
        green: Channel { shift: 8, bits: 8 },
        blue: Channel { shift: 0, bits: 8 },
        bytes_per_pixel: 4,
        byte_order: ByteOrder::LsbFirst,
    };

    /// 16-bit `RRRRRGGG GGGBBBBB`, little-endian.
    pub const RGB565: PixelFormat = PixelFormat {
        red: Channel { shift: 11, bits: 5 },
        green: Channel { shift: 5, bits: 6 },
        blue: Channel { shift: 0, bits: 5 },
        bytes_per_pixel: 2,
        byte_order: ByteOrder::LsbFirst,
    };

    /// Build a format from channel masks. Validated: every mask must be
    /// contiguous, fit in `bytes_per_pixel`, and not overlap the others.
    pub fn new(red_mask: u32, green_mask: u32, blue_mask: u32, bytes_per_pixel: u32, byte_order: ByteOrder) -> Result<PixelFormat, PixelFormatError> {
        if !(1..=4).contains(&bytes_per_pixel) {
            return Err(PixelFormatError::BytesPerPixel(bytes_per_pixel));
        }
        let red = Channel::from_mask(red_mask).ok_or(PixelFormatError::Mask(red_mask))?;
        let green = Channel::from_mask(green_mask).ok_or(PixelFormatError::Mask(green_mask))?;
        let blue = Channel::from_mask(blue_mask).ok_or(PixelFormatError::Mask(blue_mask))?;
        let pixel_bits = bytes_per_pixel * 8;
        let fits = |m: u32| pixel_bits >= 32 || m >> pixel_bits == 0;
        if !fits(red_mask) || !fits(green_mask) || !fits(blue_mask) {
            return Err(PixelFormatError::Layout);
        }
        if red_mask & green_mask != 0 || red_mask & blue_mask != 0 || green_mask & blue_mask != 0 {
            return Err(PixelFormatError::Layout);
        }
        Ok(PixelFormat { red, green, blue, bytes_per_pixel: bytes_per_pixel as u8, byte_order })
    }

    /// Bytes each destination pixel occupies.
    pub fn bytes_per_pixel(&self) -> usize {
        usize::from(self.bytes_per_pixel)
    }

    /// The destination byte order.
    pub fn byte_order(&self) -> ByteOrder {
        self.byte_order
    }

    /// Pack one `0x00RRGGBB` color into this format's pixel value.
    #[inline]
    pub fn pack(&self, xrgb: u32) -> u32 {
        self.red.place((xrgb >> 16) as u8) | self.green.place((xrgb >> 8) as u8) | self.blue.place(xrgb as u8)
    }

    /// Unpack a pixel value back to `0x00RRGGBB`, expanding narrow channels
    /// by bit replication (so `pack` then `unpack` of `0xFFFFFF` is
    /// `0xFFFFFF` in every format). Used to read pixels back for checks.
    pub fn unpack(&self, pixel: u32) -> u32 {
        fn expand(ch: Channel, pixel: u32) -> u32 {
            let bits = u32::from(ch.bits);
            let max = if bits >= 32 { u32::MAX } else { (1u32 << bits) - 1 };
            let v = (pixel >> ch.shift) & max;
            if bits >= 8 {
                v >> (bits - 8)
            } else {
                // Replicate the top bits down to fill 8 bits.
                let mut acc = 0u32;
                let mut filled = 0u32;
                while filled < 8 {
                    acc = (acc << bits) | v;
                    filled += bits;
                }
                acc >> (filled - 8)
            }
        }
        expand(self.red, pixel) << 16 | expand(self.green, pixel) << 8 | expand(self.blue, pixel)
    }

    /// The `0x00RRGGBB` color this format will actually display for `xrgb`
    /// (`unpack(pack(xrgb))`): what a readback of a presented pixel equals.
    pub fn quantize(&self, xrgb: u32) -> u32 {
        self.unpack(self.pack(xrgb))
    }

    /// Write one packed pixel value at the start of `out`.
    #[inline]
    fn store(&self, value: u32, out: &mut [u8]) {
        let n = usize::from(self.bytes_per_pixel);
        match self.byte_order {
            ByteOrder::LsbFirst => out[..n].copy_from_slice(&value.to_le_bytes()[..n]),
            ByteOrder::MsbFirst => out[..n].copy_from_slice(&value.to_be_bytes()[4 - n..]),
        }
    }

    /// Read one packed pixel value from the start of `bytes`.
    pub fn load(&self, bytes: &[u8]) -> Option<u32> {
        let n = usize::from(self.bytes_per_pixel);
        let b = bytes.get(..n)?;
        let mut v = 0u32;
        match self.byte_order {
            ByteOrder::LsbFirst => {
                for (i, byte) in b.iter().enumerate() {
                    v |= u32::from(*byte) << (8 * i);
                }
            }
            ByteOrder::MsbFirst => {
                for byte in b {
                    v = (v << 8) | u32::from(*byte);
                }
            }
        }
        Some(v)
    }

    /// Convert the `area` of `frame` (clipped to the frame) into this
    /// format, rows `dst_stride` bytes apart, into `out` (which is cleared
    /// first). Bytes between the end of a row's pixels and the stride are
    /// zero. Returns the clipped area actually converted.
    ///
    /// Total: an empty or fully out-of-frame `area` converts nothing.
    pub fn convert(&self, frame: &Frame<'_>, area: Rect, dst_stride: usize, out: &mut Vec<u8>) -> Rect {
        out.clear();
        let area = area.intersect(&frame.bounds());
        let bpp = self.bytes_per_pixel();
        let row_bytes = area.width as usize * bpp;
        if area.is_empty() || dst_stride < row_bytes {
            return Rect { width: 0, height: 0, ..area };
        }
        out.resize(dst_stride * area.height as usize, 0);
        let (x0, y0) = (area.x as usize, area.y as usize);
        let fast = *self == PixelFormat::XRGB8888;
        for row in 0..area.height as usize {
            let dst = &mut out[row * dst_stride..row * dst_stride + row_bytes];
            let src_start = (y0 + row) * frame.stride + x0;
            match frame.pixels {
                Pixels::Xrgb8888(p) => {
                    let src = &p[src_start..src_start + area.width as usize];
                    if fast {
                        for (d, s) in dst.chunks_exact_mut(4).zip(src) {
                            d.copy_from_slice(&(s & 0x00ff_ffff).to_le_bytes());
                        }
                    } else {
                        for (d, s) in dst.chunks_exact_mut(bpp).zip(src) {
                            self.store(self.pack(*s), d);
                        }
                    }
                }
                Pixels::Rgba8(b) => {
                    let src = &b[src_start * 4..(src_start + area.width as usize) * 4];
                    for (d, s) in dst.chunks_exact_mut(bpp).zip(src.chunks_exact(4)) {
                        let xrgb = u32::from(s[0]) << 16 | u32::from(s[1]) << 8 | u32::from(s[2]);
                        self.store(self.pack(xrgb), d);
                    }
                }
            }
        }
        area
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------ Rect

    #[test]
    fn intersect_overlapping_and_disjoint() {
        let a = Rect::new(0, 0, 10, 10);
        assert_eq!(a.intersect(&Rect::new(5, 5, 10, 10)), Rect::new(5, 5, 5, 5));
        assert!(a.intersect(&Rect::new(10, 0, 5, 5)).is_empty(), "touching edges do not overlap");
        assert!(a.intersect(&Rect::new(-20, -20, 5, 5)).is_empty());
    }

    #[test]
    fn intersect_survives_extreme_coordinates() {
        let a = Rect::new(i32::MAX - 1, i32::MAX - 1, u32::MAX, u32::MAX);
        let b = Rect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX);
        let _ = a.intersect(&b);
        let _ = a.union(&b);
        assert!(a.contains(i32::MAX - 1, i32::MAX - 1));
    }

    #[test]
    fn union_covers_both_and_ignores_empty() {
        let a = Rect::new(10, 10, 20, 20);
        assert_eq!(a.union(&Rect::new(50, 5, 10, 40)), Rect::new(10, 5, 50, 40));
        assert_eq!(a.union(&Rect::default()), a);
        assert_eq!(Rect::default().union(&a), a);
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(10, 30, 100, 20);
        assert!(r.contains(10, 30));
        assert!(r.contains(109, 49));
        assert!(!r.contains(110, 30));
        assert!(!r.contains(10, 50));
        assert!(!r.contains(9, 30));
    }

    // ----------------------------------------------------------------- Frame

    #[test]
    fn frame_rejects_short_buffers() {
        assert_eq!(Frame::xrgb(&[0; 5], 3, 2).unwrap_err(), FrameError { needed: 6, got: 5 });
        assert!(Frame::rgba8(&[0; 23], 3, 2).is_err());
        assert!(Frame::rgba8(&[0; 24], 3, 2).is_ok());
        assert!(Frame::xrgb_with_stride(&[0; 8], 3, 2, 2).is_err(), "stride < width");
    }

    #[test]
    fn frame_last_row_need_not_be_padded() {
        // 2 rows, width 3, stride 5: 5 + 3 = 8 pixels suffice.
        assert!(Frame::xrgb_with_stride(&[0; 8], 3, 2, 5).is_ok());
        assert!(Frame::xrgb_with_stride(&[0; 7], 3, 2, 5).is_err());
    }

    #[test]
    fn frame_zero_size_is_fine_and_empty() {
        let f = Frame::xrgb(&[], 0, 0).unwrap();
        assert!(f.bounds().is_empty());
        assert_eq!(f.get(0, 0), None);
    }

    #[test]
    fn frame_huge_dimensions_do_not_overflow() {
        assert!(Frame::xrgb(&[0; 4], u32::MAX, u32::MAX).is_err());
        assert!(Frame::rgba8(&[0; 4], u32::MAX, u32::MAX).is_err());
    }

    #[test]
    fn frame_get_reads_both_layouts() {
        let px = [0xAA11_2233u32, 0x0044_5566];
        let f = Frame::xrgb(&px, 2, 1).unwrap();
        assert_eq!(f.get(0, 0), Some(0x11_2233), "top byte is ignored");
        assert_eq!(f.get(1, 0), Some(0x44_5566));
        let bytes = [1, 2, 3, 255, 4, 5, 6, 0];
        let f = Frame::rgba8(&bytes, 2, 1).unwrap();
        assert_eq!(f.get(1, 0), Some(0x04_0506));
    }

    // ----------------------------------------------------------- PixelFormat

    #[test]
    fn rejects_bad_formats() {
        assert_eq!(PixelFormat::new(0xff0000, 0xff00, 0xff, 5, ByteOrder::LsbFirst), Err(PixelFormatError::BytesPerPixel(5)));
        assert_eq!(PixelFormat::new(0, 0xff00, 0xff, 4, ByteOrder::LsbFirst), Err(PixelFormatError::Mask(0)));
        assert_eq!(PixelFormat::new(0xf0f000, 0xff00, 0xff, 4, ByteOrder::LsbFirst), Err(PixelFormatError::Mask(0xf0f000)));
        assert_eq!(PixelFormat::new(0xff0000, 0xff00, 0xff, 2, ByteOrder::LsbFirst), Err(PixelFormatError::Layout));
        assert_eq!(PixelFormat::new(0xff00, 0xff00, 0xff, 4, ByteOrder::LsbFirst), Err(PixelFormatError::Layout));
    }

    #[test]
    fn constants_match_their_masks() {
        assert_eq!(PixelFormat::new(0xff0000, 0xff00, 0xff, 4, ByteOrder::LsbFirst), Ok(PixelFormat::XRGB8888));
        assert_eq!(PixelFormat::new(0xf800, 0x07e0, 0x1f, 2, ByteOrder::LsbFirst), Ok(PixelFormat::RGB565));
    }

    #[test]
    fn pack_rgb565_truncates() {
        let f = PixelFormat::RGB565;
        assert_eq!(f.pack(0xffffff), 0xffff);
        assert_eq!(f.pack(0x000000), 0);
        assert_eq!(f.pack(0xff0000), 0xf800);
        assert_eq!(f.pack(0x00ff00), 0x07e0);
        assert_eq!(f.pack(0x0000ff), 0x001f);
        assert_eq!(f.pack(0x070307), 0, "low bits below the channel width vanish");
    }

    #[test]
    fn pack_bgr233_eight_bit_truecolor() {
        // Xvfb's 8-bit TrueColor visual: red 0x07, green 0x38, blue 0xc0.
        let f = PixelFormat::new(0x07, 0x38, 0xc0, 1, ByteOrder::LsbFirst).unwrap();
        assert_eq!(f.pack(0xff0000), 0x07);
        assert_eq!(f.pack(0x00ff00), 0x38);
        assert_eq!(f.pack(0x0000ff), 0xc0);
        assert_eq!(f.pack(0xffffff), 0xff);
    }

    #[test]
    fn wide_channels_replicate_so_white_stays_white() {
        // 30-bit deep color: 10 bits per channel.
        let f = PixelFormat::new(0x3ff0_0000, 0x000f_fc00, 0x3ff, 4, ByteOrder::LsbFirst).unwrap();
        assert_eq!(f.pack(0xffffff), 0x3fff_ffff);
        assert_eq!(f.pack(0x800000) >> 20, 0x202, "0x80 -> 0b10_0000_0010");
        assert_eq!(f.quantize(0x123456), 0x123456, "8 -> 10 -> 8 bits is lossless");
    }

    #[test]
    fn unpack_inverts_pack_at_full_and_zero() {
        let formats = [
            PixelFormat::XRGB8888,
            PixelFormat::RGB565,
            PixelFormat::new(0x7c00, 0x03e0, 0x1f, 2, ByteOrder::LsbFirst).unwrap(), // RGB555
            PixelFormat::new(0x07, 0x38, 0xc0, 1, ByteOrder::LsbFirst).unwrap(),
            PixelFormat::new(0xff, 0xff00, 0xff0000, 3, ByteOrder::MsbFirst).unwrap(),
        ];
        for f in formats {
            for c in [0x000000, 0xffffff, 0xff0000, 0x00ff00, 0x0000ff] {
                assert_eq!(f.quantize(c), c, "{f:?} {c:06x}");
            }
        }
    }

    #[test]
    fn quantize_is_idempotent_for_every_channel_value() {
        let formats = [PixelFormat::RGB565, PixelFormat::new(0x07, 0x38, 0xc0, 1, ByteOrder::LsbFirst).unwrap()];
        for f in formats {
            for v in 0..=255u32 {
                let c = v << 16 | v << 8 | v;
                let q = f.quantize(c);
                assert_eq!(f.quantize(q), q, "{f:?} {v}");
            }
        }
    }

    #[test]
    fn store_and_load_respect_byte_order() {
        let le = PixelFormat::new(0xff0000, 0xff00, 0xff, 3, ByteOrder::LsbFirst).unwrap();
        let be = PixelFormat::new(0xff0000, 0xff00, 0xff, 3, ByteOrder::MsbFirst).unwrap();
        let mut buf = [0u8; 3];
        le.store(0x112233, &mut buf);
        assert_eq!(buf, [0x33, 0x22, 0x11]);
        assert_eq!(le.load(&buf), Some(0x112233));
        be.store(0x112233, &mut buf);
        assert_eq!(buf, [0x11, 0x22, 0x33]);
        assert_eq!(be.load(&buf), Some(0x112233));
        assert_eq!(be.load(&buf[..2]), None);
    }

    #[test]
    fn convert_xrgb_fast_path_is_bgrx() {
        let px = [0xff11_2233u32, 0x0044_5566];
        let frame = Frame::xrgb(&px, 2, 1).unwrap();
        let mut out = Vec::new();
        let done = PixelFormat::XRGB8888.convert(&frame, frame.bounds(), 8, &mut out);
        assert_eq!(done, Rect::new(0, 0, 2, 1));
        assert_eq!(out, [0x33, 0x22, 0x11, 0x00, 0x66, 0x55, 0x44, 0x00]);
    }

    #[test]
    fn convert_rgba8_to_rgb565_little_endian() {
        let bytes = [0xff, 0x00, 0x00, 0x80];
        let frame = Frame::rgba8(&bytes, 1, 1).unwrap();
        let mut out = Vec::new();
        PixelFormat::RGB565.convert(&frame, frame.bounds(), 2, &mut out);
        assert_eq!(out, [0x00, 0xf8]);
    }

    #[test]
    fn convert_pads_rows_to_stride_with_zeros() {
        let px = [0xffffff; 3];
        let frame = Frame::xrgb(&px, 3, 1).unwrap();
        let mut out = vec![0xAA; 99]; // stale contents must be cleared
        PixelFormat::RGB565.convert(&frame, frame.bounds(), 8, &mut out);
        assert_eq!(out, [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0, 0]);
    }

    #[test]
    fn convert_sub_rect_with_source_stride() {
        // 4x3 frame with stride 5; take the 2x2 block at (1, 1).
        let mut px = vec![0u32; 5 * 2 + 4];
        for y in 0..3 {
            for x in 0..4 {
                px[y * 5 + x] = (y * 16 + x) as u32;
            }
        }
        let frame = Frame::xrgb_with_stride(&px, 4, 3, 5).unwrap();
        let fmt = PixelFormat::new(0xff, 0xff00, 0xff0000, 4, ByteOrder::LsbFirst).unwrap();
        let mut out = Vec::new();
        let done = fmt.convert(&frame, Rect::new(1, 1, 2, 2), 8, &mut out);
        assert_eq!(done, Rect::new(1, 1, 2, 2));
        let vals: Vec<u32> = out.chunks(4).map(|c| fmt.unpack(fmt.load(c).unwrap())).collect();
        assert_eq!(vals, [0x11, 0x12, 0x21, 0x22]);
    }

    #[test]
    fn convert_clips_to_the_frame() {
        let px = [1u32; 4];
        let frame = Frame::xrgb(&px, 2, 2).unwrap();
        let mut out = Vec::new();
        let done = PixelFormat::XRGB8888.convert(&frame, Rect::new(1, -5, 100, 100), 4, &mut out);
        assert_eq!(done, Rect::new(1, 0, 1, 2));
        assert_eq!(out.len(), 8);
        let none = PixelFormat::XRGB8888.convert(&frame, Rect::new(50, 50, 1, 1), 4, &mut out);
        assert!(none.is_empty());
        assert!(out.is_empty());
    }

    #[test]
    fn convert_with_too_narrow_stride_converts_nothing() {
        let px = [1u32; 4];
        let frame = Frame::xrgb(&px, 4, 1).unwrap();
        let mut out = Vec::new();
        assert!(PixelFormat::XRGB8888.convert(&frame, frame.bounds(), 15, &mut out).is_empty());
        assert!(out.is_empty());
    }

    #[test]
    fn fast_and_generic_paths_agree() {
        let px: Vec<u32> = (0..64u32).map(|i| i.wrapping_mul(0x9E37_79B9)).collect();
        let frame = Frame::xrgb(&px, 8, 8).unwrap();
        let mut fast = Vec::new();
        PixelFormat::XRGB8888.convert(&frame, frame.bounds(), 32, &mut fast);
        let mut bytes = Vec::new();
        for p in &px {
            bytes.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 0]);
        }
        let rgba = Frame::rgba8(&bytes, 8, 8).unwrap();
        let mut generic = Vec::new();
        PixelFormat::XRGB8888.convert(&rgba, rgba.bounds(), 32, &mut generic);
        assert_eq!(fast, generic);
    }
}
