//! A decoder for uncompressed Windows BMP files (feature `bmp`) — the
//! format of classic skin sprite sheets.
//!
//! Reads the Windows (`BITMAPINFOHEADER` and its V4/V5 extensions) and
//! OS/2 1.x (`BITMAPCOREHEADER`) layouts at 1, 4, 8 (palette), 16 (5-5-5),
//! 24 and 32 bits per pixel, bottom-up or top-down. Not supported, and
//! reported as [`BmpError::Unsupported`]: RLE and bit-field compression,
//! embedded JPEG/PNG. Alpha in 32-bit files is ignored.
//!
//! The decoder is total over hostile input: any byte slice gives a
//! [`Canvas`] or an error, never a panic, and it allocates nothing until
//! the file is known to hold every pixel it claims.
//!
//! ```
//! # fn main() -> Result<(), protolith::canvas::bmp::BmpError> {
//! # let bytes = [66u8, 77, 58, 0, 0, 0, 0, 0, 0, 0, 54, 0, 0, 0, 40, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 24, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x33, 0x22, 0x11, 0];
//! // let bytes = std::fs::read("main.bmp")?;
//! let sheet = protolith::canvas::bmp::decode(&bytes)?;
//! assert_eq!(sheet.get(0, 0), Some(0x112233));
//! # Ok(()) }
//! ```

use std::fmt;

use super::Canvas;

/// Why a BMP could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BmpError {
    /// The data does not start with `BM`.
    NotBmp,
    /// The file ends before the header, palette or pixels do.
    Truncated,
    /// A header field is impossible (e.g. a negative width).
    Invalid(&'static str),
    /// A valid BMP this decoder does not read (compression, bit depth,
    /// header version).
    Unsupported(&'static str),
}

impl fmt::Display for BmpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BmpError::NotBmp => write!(f, "not a BMP file"),
            BmpError::Truncated => write!(f, "BMP file is truncated"),
            BmpError::Invalid(what) => write!(f, "invalid BMP: {what}"),
            BmpError::Unsupported(what) => write!(f, "unsupported BMP: {what}"),
        }
    }
}

impl std::error::Error for BmpError {}

fn le(bytes: &[u8], at: usize, n: usize) -> Result<u32, BmpError> {
    let b = bytes.get(at..at.checked_add(n).ok_or(BmpError::Truncated)?).ok_or(BmpError::Truncated)?;
    Ok(b.iter().rev().fold(0, |acc, &x| acc << 8 | u32::from(x)))
}

/// Decode a BMP file into a canvas of `0x00RRGGBB` pixels.
///
/// Palette indices past the end of the palette decode as black.
pub fn decode(bytes: &[u8]) -> Result<Canvas, BmpError> {
    if !bytes.starts_with(b"BM") {
        return Err(BmpError::NotBmp);
    }
    let data = le(bytes, 10, 4)? as usize;
    let header = le(bytes, 14, 4)? as usize;
    let (width, height, bits, entry) = match header {
        12 => (le(bytes, 18, 2)? as i32, le(bytes, 20, 2)? as i32, le(bytes, 24, 2)?, 3),
        40.. => {
            match le(bytes, 30, 4)? {
                0 => {}
                1 | 2 => return Err(BmpError::Unsupported("RLE compression")),
                3 | 6 => return Err(BmpError::Unsupported("bit-field compression")),
                4 | 5 => return Err(BmpError::Unsupported("embedded JPEG or PNG")),
                _ => return Err(BmpError::Invalid("unknown compression")),
            }
            (le(bytes, 18, 4)? as i32, le(bytes, 22, 4)? as i32, le(bytes, 28, 2)?, 4)
        }
        _ => return Err(BmpError::Unsupported("header version")),
    };
    if width < 0 {
        return Err(BmpError::Invalid("negative width"));
    }
    if height == i32::MIN {
        return Err(BmpError::Invalid("height out of range"));
    }
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) {
        return Err(BmpError::Unsupported("bit depth"));
    }
    let (w, h) = (width as u32, height.unsigned_abs());
    let top_down = height < 0;

    // Palette: up to 2^bits entries (fewer if the header says so), between
    // the header and the pixel data.
    let mut palette = Vec::new();
    if bits <= 8 {
        let used = if header == 12 { 0 } else { le(bytes, 46, 4)? as usize };
        let max = 1usize << bits;
        let count = if used == 0 || used > max { max } else { used };
        let start = header.saturating_add(14);
        let room = data.min(bytes.len()).saturating_sub(start) / entry;
        if room == 0 {
            return Err(BmpError::Truncated);
        }
        palette = (0..count.min(room)).map(|i| le(bytes, start + i * entry, 3)).collect::<Result<_, _>>()?;
    }

    if w == 0 || h == 0 {
        return Ok(Canvas::new(w, h));
    }
    // Rows are padded to 4 bytes; the last one need not be. Checked, in
    // u64, so a hostile header is rejected before anything is allocated.
    let line = (u64::from(w) * u64::from(bits)).div_ceil(8);
    let stride = line.div_ceil(4) * 4;
    let end = u64::from(h - 1).checked_mul(stride).and_then(|n| n.checked_add(line + data as u64));
    if end.map_or(true, |end| end > bytes.len() as u64) {
        return Err(BmpError::Truncated);
    }
    // The pixel data fits in memory, so these fit in usize.
    let (line, stride) = (line as usize, stride as usize);

    let mut canvas = Canvas::new(w, h);
    for (y, out) in canvas.pixels_mut().chunks_exact_mut(w as usize).enumerate() {
        let row = if top_down { y } else { h as usize - 1 - y };
        let src = &bytes[data + row * stride..][..line];
        for (x, px) in out.iter_mut().enumerate() {
            *px = match bits {
                24 | 32 => {
                    let o = x * (bits as usize / 8);
                    u32::from(src[o + 2]) << 16 | u32::from(src[o + 1]) << 8 | u32::from(src[o])
                }
                16 => {
                    let v = u32::from(src[x * 2]) | u32::from(src[x * 2 + 1]) << 8;
                    // 5 bits → 8 by replicating the top bits into the bottom.
                    let ch = |shift: u32| {
                        let c = (v >> shift) & 31;
                        (c << 3 | c >> 2) << (shift / 5 * 8)
                    };
                    ch(10) | ch(5) | ch(0)
                }
                _ => {
                    let bit = x * bits as usize;
                    let index = (src[bit / 8] >> (8 - bits as usize - bit % 8)) & ((1 << bits) - 1) as u8;
                    palette.get(usize::from(index)).copied().unwrap_or(0)
                }
            };
        }
    }
    Ok(canvas)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Windows BMP assembled field by field from the format's spec:
    /// 14-byte file header, a `header`-byte info header, the palette
    /// (`[B, G, R, 0]` entries), then `rows` as given (bottom row first
    /// unless `height` is negative). One argument per header field, so
    /// each fixture reads like the spec's table.
    #[allow(clippy::too_many_arguments)]
    fn bmp(header: u32, width: i32, height: i32, bits: u16, compression: u32, used: u32, palette: &[u32], rows: &[&[u8]]) -> Vec<u8> {
        let mut info = Vec::new();
        info.extend_from_slice(&header.to_le_bytes());
        info.extend_from_slice(&width.to_le_bytes());
        info.extend_from_slice(&height.to_le_bytes());
        info.extend_from_slice(&1u16.to_le_bytes()); // planes
        info.extend_from_slice(&bits.to_le_bytes());
        info.extend_from_slice(&compression.to_le_bytes());
        info.extend_from_slice(&0u32.to_le_bytes()); // image size (may be 0 for BI_RGB)
        info.extend_from_slice(&2835u32.to_le_bytes()); // 72 dpi
        info.extend_from_slice(&2835u32.to_le_bytes());
        info.extend_from_slice(&used.to_le_bytes());
        info.extend_from_slice(&0u32.to_le_bytes()); // important colors
        info.resize(header as usize, 0); // V4/V5 extension fields
        let mut pal = Vec::new();
        for c in palette {
            pal.extend_from_slice(&[*c as u8, (c >> 8) as u8, (c >> 16) as u8, 0]);
        }
        let data = 14 + info.len() + pal.len();
        let size = data + rows.iter().map(|r| r.len()).sum::<usize>();
        let mut out = b"BM".to_vec();
        out.extend_from_slice(&(size as u32).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&(data as u32).to_le_bytes());
        out.extend(info);
        out.extend(pal);
        for r in rows {
            out.extend_from_slice(r);
        }
        out
    }

    #[test]
    fn rgb24_bottom_up_with_row_padding() {
        // 2x2: two pixels of 3 bytes = 6, padded to 8. Bottom row first.
        let file = bmp(40, 2, 2, 24, 0, 0, &[], &[&[0, 0, 0xff, 0, 0xff, 0, 0, 0], &[0xff, 0, 0, 0x33, 0x22, 0x11, 0, 0]]);
        let c = decode(&file).unwrap();
        assert_eq!((c.width(), c.height()), (2, 2));
        assert_eq!(c.pixels(), &[0x0000ff, 0x112233, 0xff0000, 0x00ff00]);
    }

    #[test]
    fn negative_height_is_top_down() {
        let file = bmp(40, 1, -2, 24, 0, 0, &[], &[&[1, 2, 3, 0], &[4, 5, 6, 0]]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0x030201, 0x060504]);
    }

    #[test]
    fn last_row_padding_may_be_missing() {
        let file = bmp(40, 1, -2, 24, 0, 0, &[], &[&[1, 2, 3, 0], &[4, 5, 6]]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0x030201, 0x060504]);
        let file = bmp(40, 1, -2, 24, 0, 0, &[], &[&[1, 2, 3, 0], &[4, 5]]);
        assert_eq!(decode(&file), Err(BmpError::Truncated));
    }

    #[test]
    fn rgb32_ignores_the_fourth_byte() {
        let file = bmp(40, 2, 1, 32, 0, 0, &[], &[&[1, 2, 3, 0xff, 4, 5, 6, 0x00]]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0x030201, 0x060504]);
    }

    #[test]
    fn rgb16_is_555_with_bit_replication() {
        // 0x7fff white, 0x7c00 red, 0x0001 the lowest blue step, 0x8000 the unused top bit.
        let file = bmp(40, 4, 1, 16, 0, 0, &[], &[&[0xff, 0x7f, 0x00, 0x7c, 0x01, 0x00, 0x00, 0x80]]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0xffffff, 0xff0000, 0x000008, 0x000000]);
    }

    #[test]
    fn eight_bit_palette() {
        let pal = [0x000000, 0xff8000, 0x00ffff];
        let file = bmp(40, 3, 1, 8, 0, 3, &pal, &[&[2, 1, 0, 0]]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0x00ffff, 0xff8000, 0x000000]);
    }

    #[test]
    fn index_past_the_palette_is_black() {
        let file = bmp(40, 2, 1, 8, 0, 2, &[0x111111, 0x222222], &[&[1, 200, 0, 0]]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0x222222, 0]);
    }

    #[test]
    fn colors_used_zero_means_full_palette() {
        let pal: Vec<u32> = (0..16).map(|i| i * 0x10_1010).collect();
        // 4-bit, 3 pixels: high nibble first; 2 bytes, padded to 4.
        let file = bmp(40, 3, 1, 4, 0, 0, &pal, &[&[0xf1, 0x70, 0, 0]]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0xf0f0f0, 0x101010, 0x707070]);
    }

    #[test]
    fn one_bit_rows_cross_byte_boundaries() {
        // 10 pixels: 1011001110 → bytes 0b1011_0011, 0b10xx_xxxx.
        let file = bmp(40, 10, 1, 1, 0, 2, &[0x000000, 0xffffff], &[&[0b1011_0011, 0b1011_1111, 0, 0]]);
        let w = 0xffffff;
        assert_eq!(decode(&file).unwrap().pixels(), &[w, 0, w, w, 0, 0, w, w, w, 0]);
    }

    #[test]
    fn v5_header_is_read_like_v3() {
        let file = bmp(124, 1, 1, 24, 0, 0, &[], &[&[9, 8, 7, 0]]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0x070809]);
    }

    #[test]
    fn os2_core_header_with_three_byte_palette() {
        let mut file = b"BM".to_vec();
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&0u32.to_le_bytes());
        file.extend_from_slice(&(14u32 + 12 + 6).to_le_bytes());
        file.extend_from_slice(&12u32.to_le_bytes());
        file.extend_from_slice(&2u16.to_le_bytes()); // width
        file.extend_from_slice(&1u16.to_le_bytes()); // height
        file.extend_from_slice(&1u16.to_le_bytes()); // planes
        file.extend_from_slice(&1u16.to_le_bytes()); // bits
        file.extend_from_slice(&[0x30, 0x20, 0x10, 0x60, 0x50, 0x40]); // BGR, BGR
        file.extend_from_slice(&[0b0100_0000, 0, 0, 0]);
        assert_eq!(decode(&file).unwrap().pixels(), &[0x102030, 0x405060]);
    }

    #[test]
    fn zero_sized_images_decode_empty() {
        assert_eq!(decode(&bmp(40, 0, 5, 24, 0, 0, &[], &[])).unwrap().pixels(), &[] as &[u32]);
        assert_eq!(decode(&bmp(40, 5, 0, 24, 0, 0, &[], &[])).unwrap().height(), 0);
    }

    #[test]
    fn rejects_what_it_cannot_read() {
        assert_eq!(decode(b"PK\x03\x04"), Err(BmpError::NotBmp));
        assert_eq!(decode(b"BM"), Err(BmpError::Truncated));
        assert_eq!(decode(&bmp(40, 1, 1, 8, 1, 0, &[0], &[&[0, 0]])), Err(BmpError::Unsupported("RLE compression")));
        assert_eq!(decode(&bmp(40, 1, 1, 32, 3, 0, &[], &[&[0; 4]])), Err(BmpError::Unsupported("bit-field compression")));
        assert_eq!(decode(&bmp(40, 1, 1, 24, 4, 0, &[], &[&[0; 4]])), Err(BmpError::Unsupported("embedded JPEG or PNG")));
        assert_eq!(decode(&bmp(40, 1, 1, 2, 0, 0, &[0; 4], &[&[0; 4]])), Err(BmpError::Unsupported("bit depth")));
        assert_eq!(decode(&bmp(64, 1, 1, 24, 0, 0, &[], &[&[0; 4]])).map(|c| c.get(0, 0)), Ok(Some(0)), "OS/2 2.x 64-byte header reads as V3");
        assert_eq!(decode(&bmp(40, -1, 1, 24, 0, 0, &[], &[&[0; 4]])), Err(BmpError::Invalid("negative width")));
        assert_eq!(decode(&bmp(40, 1, i32::MIN, 24, 0, 0, &[], &[&[0; 4]])), Err(BmpError::Invalid("height out of range")));
        assert_eq!(decode(&bmp(40, 1, 1, 8, 0, 0, &[], &[])), Err(BmpError::Truncated), "a palette image with no palette");
        let mut odd = bmp(40, 1, 1, 24, 0, 0, &[], &[&[0; 4]]);
        odd[14] = 20;
        assert_eq!(decode(&odd), Err(BmpError::Unsupported("header version")));
    }

    #[test]
    fn huge_dimensions_are_truncated_not_allocated() {
        for (w, h, bits) in [(i32::MAX, 1, 1), (1, i32::MAX, 24), (i32::MAX, i32::MAX, 32), (65536, -65536, 24)] {
            assert_eq!(decode(&bmp(40, w, h, bits, 0, 2, &[0, 0], &[&[0; 64]])), Err(BmpError::Truncated), "{w}x{h}x{bits}");
        }
    }

    #[test]
    fn every_truncation_and_corruption_is_total() {
        let pal: Vec<u32> = (0..16).collect();
        let files = [
            bmp(40, 3, 2, 24, 0, 0, &[], &[&[1; 12], &[2; 12]]),
            bmp(40, 5, -3, 4, 0, 16, &pal, &[&[0x12; 4], &[0x34; 4], &[0x56; 4]]),
            bmp(124, 9, 2, 1, 0, 2, &[0, 0xffffff], &[&[0xaa; 4], &[0x55; 4]]),
            bmp(40, 2, 2, 16, 0, 0, &[], &[&[0xff; 4], &[0x0f; 4]]),
        ];
        let mut s = 0x2545_F491_4F6C_DD1Du64;
        for file in &files {
            for n in 0..=file.len() {
                let _ = decode(&file[..n]);
            }
            for _ in 0..20_000 {
                let mut f = file.clone();
                for _ in 0..1 + (s % 4) {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    let i = (s >> 8) as usize % f.len();
                    f[i] = (s >> 40) as u8;
                }
                if let Ok(c) = decode(&f) {
                    assert_eq!(c.pixels().len(), c.width() as usize * c.height() as usize);
                }
            }
        }
    }
}
