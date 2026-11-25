//! An X11 client written against the wire protocol.
//!
//! Three layers, use whichever fits:
//!
//! - [`Connection`] — windows, frames, events. What most programs want.
//! - [`Client`] — one protocol connection: send requests, collect replies,
//!   read events and errors, with sequence numbers tracked for you.
//! - [`proto`] — the protocol itself as pure encode/decode functions, no
//!   I/O at all.
//!
//! The core protocol plus the SHAPE extension is all protolith speaks. It
//! connects over the local Unix socket or TCP (including SSH X11
//! forwarding), authenticates with `MIT-MAGIC-COOKIE-1`, and draws to any
//! TrueColor visual of 8 to 32 bits, in either image byte order.

mod client;
mod connection;
pub mod display;
#[cfg(test)]
pub(crate) mod fake;
pub mod keyboard;
pub mod proto;
pub mod shape;

pub use client::{Client, Cookie, Message};
pub use connection::{Connection, Event, MouseButton, MouseButtons, Waker, WindowId, WindowOptions};
pub use display::{Auth, DisplayName};
pub use keyboard::{Key, Modifiers};
