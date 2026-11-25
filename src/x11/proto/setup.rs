//! Connection setup: the request a client opens with, and the server's
//! description of itself (screens, depths, visuals, pixmap formats) that
//! comes back.

use super::{pad, ReadLe, Xid};
use crate::pixel::{ByteOrder, PixelFormat, PixelFormatError};

/// The byte-order byte this client always sends: `'l'`, little-endian.
pub const BYTE_ORDER_LSB_FIRST: u8 = b'l';

/// Encode the connection-setup request: byte order, protocol 11.0, and the
/// authorization protocol name/data (each padded to 4 bytes). Both may be
/// empty for an unauthenticated connection.
pub fn encode_setup_request(auth_name: &[u8], auth_data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + auth_name.len() + auth_data.len() + 6);
    out.push(BYTE_ORDER_LSB_FIRST);
    out.push(0);
    out.extend_from_slice(&11u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(auth_name.len().min(0xffff) as u16).to_le_bytes());
    out.extend_from_slice(&(auth_data.len().min(0xffff) as u16).to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(auth_name);
    out.resize(out.len() + pad(auth_name.len()), 0);
    out.extend_from_slice(auth_data);
    out.resize(out.len() + pad(auth_data.len()), 0);
    out
}

/// How many bytes of setup reply follow its 8-byte header (the header's
/// `length` field, in 4-byte units). `None` if `header` is short.
pub fn setup_reply_body_len(header: &[u8]) -> Option<usize> {
    header.u16_at(6).map(|w| usize::from(w) * 4)
}

/// One `PIXMAP-FORMATS` entry: the in-memory layout of images at a depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Format {
    /// Drawable depth this entry describes.
    pub depth: u8,
    /// Bits each pixel occupies in an image (8, 16, 24 or 32 in practice).
    pub bits_per_pixel: u8,
    /// Each image scanline is padded to a multiple of this many bits.
    pub scanline_pad: u8,
}

impl Format {
    /// Bytes per scanline of a `width`-pixel `ZPixmap` image.
    pub fn row_stride(&self, width: u32) -> usize {
        let pad_bits = u64::from(self.scanline_pad.max(8));
        let bits = u64::from(width) * u64::from(self.bits_per_pixel);
        (bits.div_ceil(pad_bits) * pad_bits / 8) as usize
    }
}

/// A visual's class (how pixel values map to colors).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualClass {
    /// Fixed gray ramp.
    StaticGray,
    /// Writable gray ramp.
    GrayScale,
    /// Fixed color map.
    StaticColor,
    /// Writable color map (the classic 8-bit palette).
    PseudoColor,
    /// Direct RGB channels, fixed ramps: what protolith draws to.
    TrueColor,
    /// Direct RGB channels, writable ramps.
    DirectColor,
    /// A class value outside the protocol's range.
    Unknown(u8),
}

impl VisualClass {
    fn from_u8(v: u8) -> VisualClass {
        match v {
            0 => VisualClass::StaticGray,
            1 => VisualClass::GrayScale,
            2 => VisualClass::StaticColor,
            3 => VisualClass::PseudoColor,
            4 => VisualClass::TrueColor,
            5 => VisualClass::DirectColor,
            other => VisualClass::Unknown(other),
        }
    }
}

/// One `VISUALTYPE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visual {
    /// Visual id.
    pub id: u32,
    /// Class.
    pub class: VisualClass,
    /// Significant bits per channel.
    pub bits_per_rgb: u8,
    /// Colormap size.
    pub colormap_entries: u16,
    /// Red channel mask (TrueColor/DirectColor).
    pub red_mask: u32,
    /// Green channel mask.
    pub green_mask: u32,
    /// Blue channel mask.
    pub blue_mask: u32,
}

/// One `DEPTH`: a depth the screen supports and its visuals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Depth {
    /// Depth in bits.
    pub depth: u8,
    /// Visuals available at this depth.
    pub visuals: Vec<Visual>,
}

/// One `SCREEN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// Root window.
    pub root: Xid,
    /// Default colormap of the root.
    pub default_colormap: Xid,
    /// Pixel value for white in the default colormap.
    pub white_pixel: u32,
    /// Pixel value for black in the default colormap.
    pub black_pixel: u32,
    /// Event masks selected on the root by *all* clients at connect time.
    /// `SubstructureRedirect` here means a window manager is running.
    pub current_input_masks: u32,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
    /// Root visual id.
    pub root_visual: u32,
    /// Root depth.
    pub root_depth: u8,
    /// Every supported depth and its visuals.
    pub depths: Vec<Depth>,
}

/// `SubstructureRedirect` event mask bit: only one client — the window
/// manager — can select it on the root.
pub const SUBSTRUCTURE_REDIRECT_MASK: u32 = 1 << 20;

impl Screen {
    /// Is a window manager running (as of connection setup)?
    pub fn has_window_manager(&self) -> bool {
        self.current_input_masks & SUBSTRUCTURE_REDIRECT_MASK != 0
    }

    /// Find a visual by id, with its depth.
    pub fn visual(&self, id: u32) -> Option<(u8, &Visual)> {
        self.depths.iter().find_map(|d| d.visuals.iter().find(|v| v.id == id).map(|v| (d.depth, v)))
    }

    /// Choose the visual protolith draws with: the root visual if it is
    /// TrueColor (no colormap needed), else the deepest TrueColor visual of
    /// depth ≤ 24 (deeper ones carry an alpha channel we do not want), else
    /// any TrueColor visual. `None` when the screen has no TrueColor visual
    /// at all (a pure palette display, which protolith does not drive).
    pub fn choose_visual(&self) -> Option<(u8, Visual)> {
        if let Some((depth, v)) = self.visual(self.root_visual) {
            if v.class == VisualClass::TrueColor {
                return Some((depth, *v));
            }
        }
        let truecolor = || self.depths.iter().flat_map(|d| d.visuals.iter().map(move |v| (d.depth, *v))).filter(|(_, v)| v.class == VisualClass::TrueColor);
        truecolor().filter(|(d, _)| *d <= 24).max_by_key(|(d, _)| *d).or_else(|| truecolor().min_by_key(|(d, _)| *d))
    }
}

/// Everything a successful connection-setup reply says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    /// Server release number.
    pub release: u32,
    /// Base for client-allocated resource ids.
    pub resource_id_base: u32,
    /// Mask of the bits a client may vary in its resource ids.
    pub resource_id_mask: u32,
    /// The server's longest accepted request, in 4-byte units.
    pub maximum_request_length: u16,
    /// Byte order of pixel data in images.
    pub image_byte_order: ByteOrder,
    /// Smallest keycode the server sends.
    pub min_keycode: u8,
    /// Largest keycode the server sends.
    pub max_keycode: u8,
    /// Server vendor string.
    pub vendor: String,
    /// Image layout per depth.
    pub formats: Vec<Format>,
    /// Screens.
    pub screens: Vec<Screen>,
}

impl Setup {
    /// The pixmap format for `depth`.
    pub fn format(&self, depth: u8) -> Option<Format> {
        self.formats.iter().find(|f| f.depth == depth).copied()
    }

    /// The [`PixelFormat`] `PutImage`/`GetImage` data uses for `visual` at
    /// `depth`.
    pub fn pixel_format(&self, depth: u8, visual: &Visual) -> Result<PixelFormat, PixelFormatError> {
        let bpp = self.format(depth).map_or(0, |f| u32::from(f.bits_per_pixel));
        if bpp % 8 != 0 {
            return Err(PixelFormatError::BytesPerPixel(0));
        }
        PixelFormat::new(visual.red_mask, visual.green_mask, visual.blue_mask, bpp / 8, self.image_byte_order)
    }
}

/// Why a server refused or garbled connection setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    /// The server refused the connection (bad or missing authorization,
    /// usually); carries its stated reason.
    Refused(String),
    /// The server asked for further authentication, which no supported
    /// protocol needs; carries its stated reason.
    Authenticate(String),
    /// The reply was truncated or malformed.
    Malformed(&'static str),
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SetupError::Refused(r) => write!(f, "X server refused the connection: {}", r.trim_end()),
            SetupError::Authenticate(r) => write!(f, "X server requires further authentication: {}", r.trim_end()),
            SetupError::Malformed(what) => write!(f, "malformed connection-setup reply: {what}"),
        }
    }
}

impl std::error::Error for SetupError {}

/// Parse a full setup reply: the 8-byte header plus the body whose length
/// [`setup_reply_body_len`] gave. Total over any input.
pub fn parse_setup_reply(buf: &[u8]) -> Result<Setup, SetupError> {
    let t = SetupError::Malformed("truncated");
    let status = buf.u8_at(0).ok_or(SetupError::Malformed("empty"))?;
    match status {
        1 => {}
        0 => {
            let len = usize::from(buf.u8_at(1).unwrap_or(0));
            let reason = buf.get(8..8 + len).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
            return Err(SetupError::Refused(reason));
        }
        2 => {
            let len = setup_reply_body_len(buf).unwrap_or(0);
            let reason = buf.get(8..8 + len).map(|b| String::from_utf8_lossy(b).trim_end_matches('\0').to_string()).unwrap_or_default();
            return Err(SetupError::Authenticate(reason));
        }
        _ => return Err(SetupError::Malformed("unknown status byte")),
    }
    let declared = setup_reply_body_len(buf).ok_or(t.clone())?;
    if buf.len() < 8 + declared {
        return Err(t);
    }

    let vendor_len = usize::from(buf.u16_at(24).ok_or(t.clone())?);
    let num_screens = usize::from(buf.u8_at(28).ok_or(t.clone())?);
    let num_formats = usize::from(buf.u8_at(29).ok_or(t.clone())?);
    let image_byte_order = match buf.u8_at(30).ok_or(t.clone())? {
        0 => ByteOrder::LsbFirst,
        1 => ByteOrder::MsbFirst,
        _ => return Err(SetupError::Malformed("image byte order")),
    };
    let vendor = buf.get(40..40 + vendor_len).ok_or(t.clone())?;

    let mut off = 40 + vendor_len + pad(vendor_len);
    let mut formats = Vec::with_capacity(num_formats);
    for _ in 0..num_formats {
        formats.push(Format {
            depth: buf.u8_at(off).ok_or(t.clone())?,
            bits_per_pixel: buf.u8_at(off + 1).ok_or(t.clone())?,
            scanline_pad: buf.u8_at(off + 2).ok_or(t.clone())?,
        });
        off += 8;
    }

    let mut screens = Vec::with_capacity(num_screens);
    for _ in 0..num_screens {
        let s = off;
        let num_depths = usize::from(buf.u8_at(s + 39).ok_or(t.clone())?);
        let mut screen = Screen {
            root: buf.u32_at(s).ok_or(t.clone())?,
            default_colormap: buf.u32_at(s + 4).ok_or(t.clone())?,
            white_pixel: buf.u32_at(s + 8).ok_or(t.clone())?,
            black_pixel: buf.u32_at(s + 12).ok_or(t.clone())?,
            current_input_masks: buf.u32_at(s + 16).ok_or(t.clone())?,
            width: buf.u16_at(s + 20).ok_or(t.clone())?,
            height: buf.u16_at(s + 22).ok_or(t.clone())?,
            root_visual: buf.u32_at(s + 32).ok_or(t.clone())?,
            root_depth: buf.u8_at(s + 38).ok_or(t.clone())?,
            depths: Vec::with_capacity(num_depths),
        };
        off = s + 40;
        for _ in 0..num_depths {
            let depth = buf.u8_at(off).ok_or(t.clone())?;
            let num_visuals = usize::from(buf.u16_at(off + 2).ok_or(t.clone())?);
            off += 8;
            let mut visuals = Vec::with_capacity(num_visuals.min(256));
            for _ in 0..num_visuals {
                visuals.push(Visual {
                    id: buf.u32_at(off).ok_or(t.clone())?,
                    class: VisualClass::from_u8(buf.u8_at(off + 4).ok_or(t.clone())?),
                    bits_per_rgb: buf.u8_at(off + 5).ok_or(t.clone())?,
                    colormap_entries: buf.u16_at(off + 6).ok_or(t.clone())?,
                    red_mask: buf.u32_at(off + 8).ok_or(t.clone())?,
                    green_mask: buf.u32_at(off + 12).ok_or(t.clone())?,
                    blue_mask: buf.u32_at(off + 16).ok_or(t.clone())?,
                });
                off += 24;
            }
            screen.depths.push(Depth { depth, visuals });
        }
        screens.push(screen);
    }

    Ok(Setup {
        release: buf.u32_at(8).ok_or(t.clone())?,
        resource_id_base: buf.u32_at(12).ok_or(t.clone())?,
        resource_id_mask: buf.u32_at(16).ok_or(t.clone())?,
        maximum_request_length: buf.u16_at(26).ok_or(t.clone())?,
        image_byte_order,
        min_keycode: buf.u8_at(34).ok_or(t.clone())?,
        max_keycode: buf.u8_at(35).ok_or(t)?,
        vendor: String::from_utf8_lossy(vendor).into_owned(),
        formats,
        screens,
    })
}

/// Hands out client resource ids: `base | (n & mask)` for `n = 0, 1, ...`.
///
/// Returns `None` once the mask's id space is exhausted rather than
/// silently reusing a live id.
#[derive(Debug, Clone)]
pub struct IdAllocator {
    base: u32,
    mask: u32,
    next: u64,
    /// Ids returned with [`IdAllocator::release`], reused first.
    free: Vec<u32>,
}

impl IdAllocator {
    /// An allocator for the setup reply's base and mask.
    pub fn new(base: u32, mask: u32) -> IdAllocator {
        IdAllocator { base, mask, next: 0, free: Vec::new() }
    }

    /// The next unused id.
    pub fn allocate(&mut self) -> Option<u32> {
        if let Some(id) = self.free.pop() {
            return Some(id);
        }
        // The mask need not be contiguous; the n-th id spreads n's bits
        // over the mask's set bits, lowest first.
        let capacity = 1u64 << self.mask.count_ones();
        if self.next >= capacity {
            return None;
        }
        let mut n = self.next;
        self.next += 1;
        let mut id = self.base;
        let mut bit = 0u32;
        while n != 0 && bit < 32 {
            if self.mask & (1 << bit) != 0 {
                if n & 1 != 0 {
                    id |= 1 << bit;
                }
                n >>= 1;
            }
            bit += 1;
        }
        Some(id)
    }

    /// Return an id whose resource has been freed, for reuse.
    pub fn release(&mut self, id: u32) {
        self.free.push(id);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn setup_request_bytes() {
        let out = encode_setup_request(b"MIT-MAGIC-COOKIE-1", &[0xAA; 16]);
        assert_eq!(&out[..12], &[b'l', 0, 11, 0, 0, 0, 18, 0, 16, 0, 0, 0]);
        assert_eq!(&out[12..30], b"MIT-MAGIC-COOKIE-1");
        assert_eq!(&out[30..32], &[0, 0]);
        assert_eq!(&out[32..48], &[0xAA; 16]);
        assert_eq!(out.len(), 48);
        assert_eq!(encode_setup_request(b"", b"").len(), 12);
    }

    /// A synthetic setup reply. `screens` is a list of
    /// `(root, root_visual, root_depth, input_masks, depths)`.
    pub(crate) fn synth_setup(max_req: u16, image_byte_order: u8, formats: &[Format], screens: &[(u32, u32, u8, u32, Vec<Depth>)]) -> Vec<u8> {
        let vendor = b"protolith test";
        let mut b = Vec::new();
        b.extend_from_slice(&12_345u32.to_le_bytes()); // release
        b.extend_from_slice(&0x0020_0000u32.to_le_bytes()); // id base
        b.extend_from_slice(&0x001f_ffffu32.to_le_bytes()); // id mask
        b.extend_from_slice(&0u32.to_le_bytes()); // motion buffer
        b.extend_from_slice(&(vendor.len() as u16).to_le_bytes());
        b.extend_from_slice(&max_req.to_le_bytes());
        b.push(screens.len() as u8);
        b.push(formats.len() as u8);
        b.push(image_byte_order);
        b.extend_from_slice(&[0, 32, 32, 8, 255]); // bitmap order/unit/pad, keycodes
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(vendor);
        b.resize(b.len() + pad(vendor.len()), 0);
        for f in formats {
            b.extend_from_slice(&[f.depth, f.bits_per_pixel, f.scanline_pad, 0, 0, 0, 0, 0]);
        }
        for (root, visual, depth, masks, depths) in screens {
            b.extend_from_slice(&root.to_le_bytes());
            b.extend_from_slice(&0x20u32.to_le_bytes()); // colormap
            b.extend_from_slice(&0x00ff_ffffu32.to_le_bytes()); // white
            b.extend_from_slice(&0u32.to_le_bytes()); // black
            b.extend_from_slice(&masks.to_le_bytes());
            b.extend_from_slice(&1024u16.to_le_bytes());
            b.extend_from_slice(&768u16.to_le_bytes());
            b.extend_from_slice(&[0; 8]); // mm, installed maps
            b.extend_from_slice(&visual.to_le_bytes());
            b.extend_from_slice(&[0, 0, *depth, depths.len() as u8]);
            for d in depths {
                b.extend_from_slice(&[d.depth, 0]);
                b.extend_from_slice(&(d.visuals.len() as u16).to_le_bytes());
                b.extend_from_slice(&[0; 4]);
                for v in &d.visuals {
                    b.extend_from_slice(&v.id.to_le_bytes());
                    let class = match v.class {
                        VisualClass::StaticGray => 0,
                        VisualClass::GrayScale => 1,
                        VisualClass::StaticColor => 2,
                        VisualClass::PseudoColor => 3,
                        VisualClass::TrueColor => 4,
                        VisualClass::DirectColor => 5,
                        VisualClass::Unknown(c) => c,
                    };
                    b.push(class);
                    b.push(v.bits_per_rgb);
                    b.extend_from_slice(&v.colormap_entries.to_le_bytes());
                    b.extend_from_slice(&v.red_mask.to_le_bytes());
                    b.extend_from_slice(&v.green_mask.to_le_bytes());
                    b.extend_from_slice(&v.blue_mask.to_le_bytes());
                    b.extend_from_slice(&[0; 4]);
                }
            }
        }
        let mut out = vec![1, 0, 11, 0, 0, 0];
        out.extend_from_slice(&((b.len() / 4) as u16).to_le_bytes());
        out.extend_from_slice(&b);
        out
    }

    pub(crate) fn truecolor(id: u32, r: u32, g: u32, b: u32) -> Visual {
        Visual { id, class: VisualClass::TrueColor, bits_per_rgb: 8, colormap_entries: 256, red_mask: r, green_mask: g, blue_mask: b }
    }

    pub(crate) fn formats_24() -> Vec<Format> {
        vec![
            Format { depth: 1, bits_per_pixel: 1, scanline_pad: 32 },
            Format { depth: 8, bits_per_pixel: 8, scanline_pad: 32 },
            Format { depth: 16, bits_per_pixel: 16, scanline_pad: 32 },
            Format { depth: 24, bits_per_pixel: 32, scanline_pad: 32 },
        ]
    }

    #[test]
    fn parses_every_field() {
        let depths = vec![Depth { depth: 24, visuals: vec![truecolor(0x21, 0xff0000, 0xff00, 0xff)] }, Depth { depth: 1, visuals: vec![] }];
        let buf = synth_setup(65535, 0, &formats_24(), &[(0x1ab, 0x21, 24, SUBSTRUCTURE_REDIRECT_MASK, depths.clone())]);
        let s = parse_setup_reply(&buf).unwrap();
        assert_eq!(s.release, 12_345);
        assert_eq!((s.resource_id_base, s.resource_id_mask), (0x0020_0000, 0x001f_ffff));
        assert_eq!(s.maximum_request_length, 65535);
        assert_eq!(s.image_byte_order, ByteOrder::LsbFirst);
        assert_eq!((s.min_keycode, s.max_keycode), (8, 255));
        assert_eq!(s.vendor, "protolith test");
        assert_eq!(s.formats, formats_24());
        let scr = &s.screens[0];
        assert_eq!((scr.root, scr.root_visual, scr.root_depth), (0x1ab, 0x21, 24));
        assert_eq!((scr.width, scr.height, scr.white_pixel), (1024, 768, 0xffffff));
        assert!(scr.has_window_manager());
        assert_eq!(scr.depths, depths);
    }

    #[test]
    fn parses_multiple_screens() {
        let d = || vec![Depth { depth: 24, visuals: vec![truecolor(0x21, 0xff0000, 0xff00, 0xff)] }];
        let buf = synth_setup(4096, 1, &formats_24(), &[(1, 0x21, 24, 0, d()), (2, 0x21, 24, 0, d())]);
        let s = parse_setup_reply(&buf).unwrap();
        assert_eq!(s.screens.len(), 2);
        assert_eq!(s.screens[1].root, 2);
        assert_eq!(s.image_byte_order, ByteOrder::MsbFirst);
        assert!(!s.screens[0].has_window_manager());
    }

    #[test]
    fn refused_and_authenticate_carry_reasons() {
        let reason = b"No protocol specified\n";
        let mut buf = vec![0, reason.len() as u8, 11, 0, 0, 0, 6, 0];
        buf.extend_from_slice(reason);
        buf.resize(8 + 24, 0);
        assert_eq!(parse_setup_reply(&buf), Err(SetupError::Refused("No protocol specified\n".into())));
        let msg = parse_setup_reply(&buf).unwrap_err().to_string();
        assert!(msg.ends_with("No protocol specified"), "{msg}");

        let mut buf = vec![2, 0, 0, 0, 0, 0, 3, 0];
        buf.extend_from_slice(b"try harder\0\0");
        assert_eq!(parse_setup_reply(&buf), Err(SetupError::Authenticate("try harder".into())));
    }

    #[test]
    fn every_truncation_is_an_error_not_a_panic() {
        let depths = vec![Depth { depth: 24, visuals: vec![truecolor(0x21, 0xff0000, 0xff00, 0xff)] }];
        let buf = synth_setup(65535, 0, &formats_24(), &[(1, 0x21, 24, 0, depths)]);
        for cut in 0..buf.len() {
            assert!(parse_setup_reply(&buf[..cut]).is_err(), "cut at {cut}");
        }
        assert!(parse_setup_reply(&buf).is_ok());
    }

    #[test]
    fn garbage_never_panics() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..2000 {
            let len = (state % 300) as usize;
            let mut buf = Vec::with_capacity(len);
            for _ in 0..len {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                buf.push(state as u8);
            }
            if let Some(b) = buf.first_mut() {
                *b = 1;
            }
            let _ = parse_setup_reply(&buf);
        }
    }

    #[test]
    fn choose_visual_prefers_truecolor_root() {
        let s = Screen {
            root: 1,
            default_colormap: 0,
            white_pixel: 0,
            black_pixel: 0,
            current_input_masks: 0,
            width: 0,
            height: 0,
            root_visual: 0x21,
            root_depth: 24,
            depths: vec![
                Depth { depth: 24, visuals: vec![truecolor(0x21, 0xff0000, 0xff00, 0xff)] },
                Depth { depth: 32, visuals: vec![truecolor(0x99, 0xff0000, 0xff00, 0xff)] },
            ],
        };
        assert_eq!(s.choose_visual().map(|(d, v)| (d, v.id)), Some((24, 0x21)));
    }

    #[test]
    fn choose_visual_falls_back_from_pseudocolor_root() {
        // Xvfb at depth 8: PseudoColor root, an 8-bit TrueColor alongside.
        let pseudo = Visual { id: 0x21, class: VisualClass::PseudoColor, bits_per_rgb: 8, colormap_entries: 256, red_mask: 0, green_mask: 0, blue_mask: 0 };
        let s = Screen {
            root: 1,
            default_colormap: 0,
            white_pixel: 0,
            black_pixel: 0,
            current_input_masks: 0,
            width: 0,
            height: 0,
            root_visual: 0x21,
            root_depth: 8,
            depths: vec![
                Depth { depth: 8, visuals: vec![pseudo, truecolor(0x24, 0x07, 0x38, 0xc0)] },
                Depth { depth: 32, visuals: vec![truecolor(0x99, 0xff0000, 0xff00, 0xff)] },
            ],
        };
        assert_eq!(s.choose_visual().map(|(d, v)| (d, v.id)), Some((8, 0x24)), "prefers depth <= 24 over a 32-bit ARGB visual");

        let only_argb = Screen {
            depths: vec![Depth { depth: 8, visuals: vec![pseudo] }, Depth { depth: 32, visuals: vec![truecolor(0x99, 0xff0000, 0xff00, 0xff)] }],
            ..s.clone()
        };
        assert_eq!(only_argb.choose_visual().map(|(_, v)| v.id), Some(0x99));

        let palette_only = Screen { depths: vec![Depth { depth: 8, visuals: vec![pseudo] }], ..s };
        assert_eq!(palette_only.choose_visual(), None);
    }

    #[test]
    fn pixel_format_from_visual_and_format() {
        let buf = synth_setup(65535, 0, &formats_24(), &[(1, 0x21, 24, 0, vec![])]);
        let s = parse_setup_reply(&buf).unwrap();
        assert_eq!(s.pixel_format(24, &truecolor(1, 0xff0000, 0xff00, 0xff)), Ok(PixelFormat::XRGB8888));
        assert_eq!(s.pixel_format(16, &truecolor(1, 0xf800, 0x07e0, 0x1f)), Ok(PixelFormat::RGB565));
        assert!(s.pixel_format(1, &truecolor(1, 0, 0, 1)).is_err(), "1 bpp is not a byte multiple");
        assert!(s.pixel_format(12, &truecolor(1, 0xf00, 0xf0, 0xf)).is_err(), "no such format");
    }

    #[test]
    fn row_stride_rounds_to_scanline_pad() {
        let f = Format { depth: 24, bits_per_pixel: 32, scanline_pad: 32 };
        assert_eq!(f.row_stride(275), 1100);
        let f = Format { depth: 16, bits_per_pixel: 16, scanline_pad: 32 };
        assert_eq!(f.row_stride(3), 8);
        let f = Format { depth: 24, bits_per_pixel: 24, scanline_pad: 32 };
        assert_eq!(f.row_stride(5), 16);
        let f = Format { depth: 8, bits_per_pixel: 8, scanline_pad: 0 };
        assert_eq!(f.row_stride(3), 3, "a zero pad is treated as byte-aligned");
    }

    #[test]
    fn id_allocator_spreads_over_the_mask() {
        let mut ids = IdAllocator::new(0x0040_0000, 0x001f_ffff);
        assert_eq!(ids.allocate(), Some(0x0040_0000));
        assert_eq!(ids.allocate(), Some(0x0040_0001));
        let mut sparse = IdAllocator::new(0x1000_0000, 0b1010);
        let got: Vec<_> = std::iter::from_fn(|| sparse.allocate()).collect();
        assert_eq!(got, [0x1000_0000, 0x1000_0002, 0x1000_0008, 0x1000_000a], "then exhausted");
    }

    #[test]
    fn id_allocator_reuses_released_ids() {
        let mut ids = IdAllocator::new(0, 1);
        let a = ids.allocate().unwrap();
        let _b = ids.allocate().unwrap();
        assert_eq!(ids.allocate(), None);
        ids.release(a);
        assert_eq!(ids.allocate(), Some(a));
    }
}
