//! The X11 core protocol as pure functions: bytes in, values out, and back.
//!
//! Nothing in this module touches a socket. Every encoder returns the exact
//! request bytes; every decoder takes a byte slice and is *total* — a
//! truncated, malformed or hostile buffer yields `None`/`Err`, never a panic
//! or an out-of-bounds read. That is what makes the protocol testable
//! byte-for-byte without an X server, and it is the layer to reach for when
//! the high-level [`Connection`](super::Connection) does not cover a request
//! you need (send the bytes with [`Client::send`](super::Client::send)).
//!
//! Byte order: this client always declares little-endian (`'l'`) at
//! connection setup, so every multi-byte field it sends and receives is
//! little-endian. Pixel data inside `PutImage`/`GetImage` follows the
//! server's *image* byte order instead; see
//! [`Setup::image_byte_order`](setup::Setup::image_byte_order).
//!
//! Reference: *X Window System Protocol*, X Consortium Standard, X11R7.7,
//! and the *X Nonrectangular Window Shape Extension Protocol* v1.1.

pub mod event;
pub mod request;
pub mod setup;

/// An X resource id (window, pixmap, GC, colormap, ...).
pub type Xid = u32;
/// An interned atom.
pub type Atom = u32;
/// A keysym (see `keysymdef.h`).
pub type Keysym = u32;

/// The `None` resource / `CurrentTime` / `AnyPropertyType`, all zero.
pub const NONE: u32 = 0;

/// Atoms predefined by the core protocol (no `InternAtom` round trip).
pub mod atom {
    use super::Atom;
    /// `ATOM`
    pub const ATOM: Atom = 4;
    /// `CARDINAL`
    pub const CARDINAL: Atom = 6;
    /// `STRING` (Latin-1)
    pub const STRING: Atom = 31;
    /// `WINDOW`
    pub const WINDOW: Atom = 33;
    /// `WM_ICON_NAME`
    pub const WM_ICON_NAME: Atom = 37;
    /// `WM_NAME`
    pub const WM_NAME: Atom = 39;
    /// `WM_NORMAL_HINTS`
    pub const WM_NORMAL_HINTS: Atom = 40;
    /// `WM_SIZE_HINTS`
    pub const WM_SIZE_HINTS: Atom = 41;
    /// `WM_CLASS`
    pub const WM_CLASS: Atom = 67;
    /// `WM_TRANSIENT_FOR`
    pub const WM_TRANSIENT_FOR: Atom = 68;
}

/// Bytes needed to pad `n` up to a multiple of 4. Every variable-length
/// field in the protocol is padded this way.
pub const fn pad(n: usize) -> usize {
    (4 - (n % 4)) % 4
}

// ---------------------------------------------------------------- reading

/// Bounds-checked little-endian reads. Every accessor returns `None` past
/// the end of the buffer instead of panicking.
pub(crate) trait ReadLe {
    fn u8_at(&self, off: usize) -> Option<u8>;
    fn u16_at(&self, off: usize) -> Option<u16>;
    fn i16_at(&self, off: usize) -> Option<i16> {
        self.u16_at(off).map(|v| v as i16)
    }
    fn u32_at(&self, off: usize) -> Option<u32>;
}

impl ReadLe for [u8] {
    fn u8_at(&self, off: usize) -> Option<u8> {
        self.get(off).copied()
    }

    fn u16_at(&self, off: usize) -> Option<u16> {
        let b = self.get(off..off.checked_add(2)?)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32_at(&self, off: usize) -> Option<u32> {
        let b = self.get(off..off.checked_add(4)?)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

// ---------------------------------------------------------------- writing

/// Builds one request. The length field (bytes 2..4, in 4-byte units) is
/// computed and the body padded by [`Req::finish`], so no encoder ever
/// hand-counts a length.
#[derive(Debug)]
pub(crate) struct Req {
    buf: Vec<u8>,
}

impl Req {
    /// Start a request with its major opcode and the "data" byte that
    /// follows it (an opcode-specific small field, or a minor opcode for
    /// extensions).
    pub(crate) fn new(opcode: u8, data: u8) -> Req {
        Req::with_capacity(opcode, data, 32)
    }

    pub(crate) fn with_capacity(opcode: u8, data: u8, cap: usize) -> Req {
        let mut buf = Vec::with_capacity(cap.max(4));
        buf.extend_from_slice(&[opcode, data, 0, 0]);
        Req { buf }
    }

    pub(crate) fn u8(mut self, v: u8) -> Req {
        self.buf.push(v);
        self
    }

    pub(crate) fn u16(mut self, v: u16) -> Req {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn i16(self, v: i16) -> Req {
        self.u16(v as u16)
    }

    pub(crate) fn u32(mut self, v: u32) -> Req {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    pub(crate) fn bytes(mut self, b: &[u8]) -> Req {
        self.buf.extend_from_slice(b);
        self
    }

    /// `n` zero bytes (an "unused" field).
    pub(crate) fn skip(mut self, n: usize) -> Req {
        self.buf.resize(self.buf.len() + n, 0);
        self
    }

    /// `b`, then zero padding to a 4-byte boundary.
    pub(crate) fn padded(self, b: &[u8]) -> Req {
        let p = pad(b.len());
        self.bytes(b).skip(p)
    }

    /// Pad to a multiple of 4 and write the length field. A request too
    /// large for the 16-bit length is written with length 0, which no
    /// server accepts without BIG-REQUESTS; [`Client::send`](super::Client::send)
    /// rejects it before it reaches the wire.
    pub(crate) fn finish(mut self) -> Vec<u8> {
        let p = pad(self.buf.len());
        self.buf.resize(self.buf.len() + p, 0);
        let words = self.buf.len() / 4;
        let len = u16::try_from(words).unwrap_or(0);
        self.buf[2..4].copy_from_slice(&len.to_le_bytes());
        self.buf
    }
}

/// A request's length in 4-byte units, as written in its header. For the
/// core protocol (no BIG-REQUESTS) this is always bytes 2..4.
pub fn request_len_words(req: &[u8]) -> Option<u16> {
    req.u16_at(2)
}

// ------------------------------------------------------------------ errors

/// An X protocol error: the server rejected a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct XError {
    /// Error code (`BadWindow` = 3, `BadMatch` = 8, ...).
    pub code: u8,
    /// Full (widened) sequence number of the failed request; `0` until a
    /// [`Client`](super::Client) widens the 16-bit wire value.
    pub sequence: u64,
    /// The offending resource id or value, for errors that carry one.
    pub bad_value: u32,
    /// Minor opcode of the failed request (extensions).
    pub minor_opcode: u16,
    /// Major opcode of the failed request.
    pub major_opcode: u8,
}

impl XError {
    /// Decode a 32-byte error packet (first byte `0`). `None` if short or
    /// not an error packet. The sequence is the raw 16-bit wire value.
    pub fn parse(buf: &[u8]) -> Option<XError> {
        if buf.len() < 32 || buf[0] != 0 {
            return None;
        }
        Some(XError { code: buf[1], sequence: u64::from(buf.u16_at(2)?), bad_value: buf.u32_at(4)?, minor_opcode: buf.u16_at(8)?, major_opcode: buf[10] })
    }

    /// The protocol name of the error code (`"BadMatch"`), or `"extension
    /// error"` for codes past the core range.
    pub fn code_name(&self) -> &'static str {
        match self.code {
            1 => "BadRequest",
            2 => "BadValue",
            3 => "BadWindow",
            4 => "BadPixmap",
            5 => "BadAtom",
            6 => "BadCursor",
            7 => "BadFont",
            8 => "BadMatch",
            9 => "BadDrawable",
            10 => "BadAccess",
            11 => "BadAlloc",
            12 => "BadColormap",
            13 => "BadGContext",
            14 => "BadIDChoice",
            15 => "BadName",
            16 => "BadLength",
            17 => "BadImplementation",
            _ => "extension error",
        }
    }
}

impl std::fmt::Display for XError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (code {}) from {} (major {}, minor {}), sequence {}, value {:#x}",
            self.code_name(),
            self.code,
            request::opcode_name(self.major_opcode),
            self.major_opcode,
            self.minor_opcode,
            self.sequence,
            self.bad_value
        )
    }
}

impl std::error::Error for XError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_rounds_up_to_four() {
        assert_eq!([0, 1, 2, 3, 4, 5].map(pad), [0, 3, 2, 1, 0, 3]);
    }

    #[test]
    fn req_finish_pads_and_sets_length() {
        let r = Req::new(99, 7).u8(1).finish();
        assert_eq!(r, [99, 7, 2, 0, 1, 0, 0, 0]);
        assert_eq!(request_len_words(&r), Some(2));
    }

    #[test]
    fn req_padded_field() {
        let r = Req::new(1, 0).padded(b"abcde").finish();
        assert_eq!(r.len(), 12);
        assert_eq!(&r[4..9], b"abcde");
        assert_eq!(&r[9..], &[0, 0, 0]);
    }

    #[test]
    fn oversized_request_gets_zero_length() {
        let r = Req::new(1, 0).skip(4 * 70_000).finish();
        assert_eq!(request_len_words(&r), Some(0));
    }

    #[test]
    fn read_le_is_bounds_checked() {
        let b: &[u8] = &[1, 2, 3];
        assert_eq!(b.u16_at(1), Some(0x0302));
        assert_eq!(b.u16_at(2), None);
        assert_eq!(b.u32_at(0), None);
        assert_eq!(b.u32_at(usize::MAX), None);
        assert_eq!(b.i16_at(0), Some(0x0201));
    }

    #[test]
    fn xerror_parse_and_display() {
        let mut e = [0u8; 32];
        e[1] = 8;
        e[2..4].copy_from_slice(&7u16.to_le_bytes());
        e[4..8].copy_from_slice(&0x0040_0001u32.to_le_bytes());
        e[10] = 1;
        let err = XError::parse(&e).unwrap();
        assert_eq!(err, XError { code: 8, sequence: 7, bad_value: 0x0040_0001, minor_opcode: 0, major_opcode: 1 });
        let s = err.to_string();
        assert!(s.contains("BadMatch") && s.contains("CreateWindow"), "{s}");
        assert_eq!(XError::parse(&e[..31]), None);
        e[0] = 1;
        assert_eq!(XError::parse(&e), None, "a reply is not an error");
    }
}
