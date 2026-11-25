//! # protolith
//!
//! Pixels in a window with nothing underneath.
//!
//! protolith puts a buffer of pixels on screen — in an X11 window, or
//! straight onto the Linux console framebuffer — and hands back keyboard
//! and mouse input. It is written directly against the X11 wire protocol
//! and the framebuffer device: no Xlib, no xcb, no C, no `unsafe`, and no
//! dependencies beyond `std`.
//!
//! ```no_run
//! use protolith::x11::{Connection, Event, Key, WindowOptions};
//! use protolith::Frame;
//!
//! let mut conn = Connection::open()?;
//! let win = conn.create_window(WindowOptions::new(256, 256).title("gradient"))?;
//! let pixels: Vec<u32> = (0..256 * 256).map(|i| (i % 256) << 16 | (i / 256) << 8).collect();
//! conn.present(win, &Frame::xrgb(&pixels, 256, 256)?)?;
//! loop {
//!     match conn.wait_event()? {
//!         Event::Key { key: Key::Escape, .. } | Event::CloseRequested { .. } => break,
//!         _ => {}
//!     }
//! }
//! # Ok::<(), protolith::Error>(())
//! ```
//!
//! ## Pieces
//!
//! - [`Frame`] — a borrowed image (`0x00RRGGBB` words or RGBA bytes), and
//!   [`PixelFormat`], which converts it to whatever the display wants.
//! - [`x11`] — windows, events, keyboard, non-rectangular windows, window
//!   dragging, and the raw protocol underneath.
//! - [`fbdev`] — the console framebuffer, for machines with no X at all.
//! - [`Present`] — the one trait both implement, for code that should not
//!   care which it is drawing to.
//! - `canvas` — software drawing on an owned image: fills, blending,
//!   sprite blits, nearest-neighbor scaling; `canvas::bmp` decodes BMP
//!   sprite sheets.
//! - `font` — bitmap text on a canvas: sprite-sheet fonts, and a Terminus
//!   Font subset (OFL-1.1) with one feature per size and weight.
//! - `ui` — widgets as plain state structs: events in, a response out;
//!   so far the text field.
//!
//! ## Features
//!
//! `x11` and `fbdev` (both on by default) select the backends. `canvas`
//! adds software drawing, and `bmp` (which implies `canvas`) the BMP
//! decoder. `font` adds bitmap text, and `font-terminus-16` (likewise
//! `-12`, `-20`, `-24`, `-32`, and `-16-bold` … `-32-bold`) one size of
//! Terminus each. `ui` adds widgets. Each feature costs nothing when it is
//! off.

#![forbid(unsafe_code)]
#![doc(html_root_url = "https://docs.rs/protolith/0.1.0")]
#![deny(missing_docs)]
#![warn(rust_2018_idioms, unreachable_pub)]

mod error;
mod pixel;

#[cfg(all(doctest, feature = "x11"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

#[cfg(feature = "canvas")]
pub mod canvas;
#[cfg(feature = "fbdev")]
pub mod fbdev;
#[cfg(feature = "font")]
pub mod font;
#[cfg(feature = "ui")]
pub mod ui;
#[cfg(feature = "x11")]
pub mod x11;

pub use error::{Error, Result};
pub use pixel::{ByteOrder, Frame, FrameError, PixelFormat, PixelFormatError, Pixels, Rect};

/// Something a [`Frame`] can be shown on: an X11 window or a framebuffer.
pub trait Present {
    /// Size of the drawable area in pixels.
    fn size(&self) -> Result<(u32, u32)>;

    /// Show `frame` at the top-left, clipped to the drawable area.
    fn present(&mut self, frame: &Frame<'_>) -> Result<()>;

    /// Show only the `area` of `frame`.
    fn present_rect(&mut self, frame: &Frame<'_>, area: Rect) -> Result<()>;
}

/// A window viewed as a [`Present`] target: `conn.target(win)`.
#[cfg(feature = "x11")]
#[derive(Debug)]
pub struct WindowTarget<'a> {
    conn: &'a mut x11::Connection,
    window: x11::WindowId,
}

#[cfg(feature = "x11")]
impl x11::Connection {
    /// Borrow `window` as a [`Present`] target.
    pub fn target(&mut self, window: x11::WindowId) -> WindowTarget<'_> {
        WindowTarget { conn: self, window }
    }
}

#[cfg(feature = "x11")]
impl Present for WindowTarget<'_> {
    fn size(&self) -> Result<(u32, u32)> {
        self.conn.window_size(self.window)
    }

    fn present(&mut self, frame: &Frame<'_>) -> Result<()> {
        self.conn.present(self.window, frame)
    }

    fn present_rect(&mut self, frame: &Frame<'_>, area: Rect) -> Result<()> {
        self.conn.present_rect(self.window, frame, area)
    }
}
