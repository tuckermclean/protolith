//! Widgets (feature `ui`): plain state structs, events in, responses out.
//!
//! There is no widget tree, no layout and no callbacks. A widget is a
//! value you own; you hand it the events for its window and get back a
//! [`Response`] saying what, if anything, happened; you draw it onto your
//! [`Canvas`](crate::canvas::Canvas) when you redraw. Event handling needs
//! the `x11` feature (the framebuffer has no input); the state and
//! drawing do not.
//!
//! - [`TextEdit`] — a single-line edit buffer: cursor, selection, words.
//! - [`TextField`] — a `TextEdit` on screen: focus, keys, mouse, scrolling.

mod edit;
mod field;

pub use edit::{Motion, TextEdit};
pub use field::{FieldStyle, TextField};

/// What an event did to a widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Response {
    /// Not for this widget: the caller may use the event itself.
    Ignored,
    /// The widget used the event; its appearance may have changed.
    Redraw,
    /// The widget's value changed (and it needs a redraw).
    Changed,
    /// Enter in a text field.
    Submit,
    /// Escape in a text field.
    Cancel,
}
