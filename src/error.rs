//! The crate's error type.

use std::fmt;
use std::io;

/// Everything that can go wrong in protolith.
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// An I/O error talking to the server or device.
    Io(io::Error),
    /// The server closed the connection.
    Disconnected,
    /// `$DISPLAY` (or the given display name) did not parse.
    #[cfg(feature = "x11")]
    Display(crate::x11::display::DisplayError),
    /// The server refused connection setup or sent a malformed reply.
    #[cfg(feature = "x11")]
    Setup(crate::x11::proto::setup::SetupError),
    /// The server rejected a request.
    #[cfg(feature = "x11")]
    X(crate::x11::proto::XError),
    /// The server (or device) sent data that does not follow the protocol.
    Protocol(String),
    /// A request would exceed the server's maximum request length.
    RequestTooLarge {
        /// Size of the request in bytes.
        bytes: usize,
        /// The server's limit in bytes.
        max: usize,
    },
    /// The display or device cannot be driven (e.g. no TrueColor visual,
    /// or an unsupported framebuffer depth).
    Unsupported(String),
    /// An argument was out of range (zero-sized window, unknown window id,
    /// a frame too small for its dimensions, ...).
    InvalidArgument(String),
    /// The server's resource-id space is exhausted.
    OutOfIds,
}

/// `Result<T, protolith::Error>`.
pub type Result<T, E = Error> = std::result::Result<T, E>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::Disconnected => write!(f, "the server closed the connection"),
            #[cfg(feature = "x11")]
            Error::Display(e) => e.fmt(f),
            #[cfg(feature = "x11")]
            Error::Setup(e) => e.fmt(f),
            #[cfg(feature = "x11")]
            Error::X(e) => write!(f, "X protocol error: {e}"),
            Error::Protocol(s) => write!(f, "protocol violation: {s}"),
            Error::RequestTooLarge { bytes, max } => write!(f, "request of {bytes} bytes exceeds the server's {max}-byte limit"),
            Error::Unsupported(s) => write!(f, "unsupported: {s}"),
            Error::InvalidArgument(s) => write!(f, "invalid argument: {s}"),
            Error::OutOfIds => write!(f, "out of X resource ids"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            #[cfg(feature = "x11")]
            Error::Display(e) => Some(e),
            #[cfg(feature = "x11")]
            Error::Setup(e) => Some(e),
            #[cfg(feature = "x11")]
            Error::X(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

impl From<crate::pixel::FrameError> for Error {
    fn from(e: crate::pixel::FrameError) -> Error {
        Error::InvalidArgument(e.to_string())
    }
}

#[cfg(feature = "x11")]
impl From<crate::x11::display::DisplayError> for Error {
    fn from(e: crate::x11::display::DisplayError) -> Error {
        Error::Display(e)
    }
}

#[cfg(feature = "x11")]
impl From<crate::x11::proto::setup::SetupError> for Error {
    fn from(e: crate::x11::proto::setup::SetupError) -> Error {
        Error::Setup(e)
    }
}

#[cfg(feature = "x11")]
impl From<crate::x11::proto::XError> for Error {
    fn from(e: crate::x11::proto::XError) -> Error {
        Error::X(e)
    }
}
