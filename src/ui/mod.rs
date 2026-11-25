//! Widgets (feature `ui`): plain state structs, events in, responses out.
//!
//! There is no widget tree, no layout and no callbacks. A widget is a
//! value you own; you hand it the events for its window and get back a
//! [`Response`] saying what, if anything, happened; you draw it onto your
//! [`Canvas`] when you redraw. Event handling needs the `x11` feature (the
//! framebuffer has no input); the state and drawing do not.
//!
//! - [`TextEdit`] — a single-line edit buffer: cursor, selection, words.
//! - [`TextField`] — a `TextEdit` on screen: focus, keys, mouse, scrolling.
//! - [`Button`] and [`Toggle`] — push and on/off buttons.
//! - [`Slider`] — a value on a track, horizontal or vertical.
//! - [`List`] — scrolling rows of text, one selected.
//!
//! Every widget can draw a default look (a 1996-style bevel, colors from
//! [`Style`]); a skinned program instead reads the widget's state —
//! [`Button::is_down`], [`Slider::thumb_rect`], [`List::selected`] — and
//! blits its own sprites. A left click inside a widget focuses it and one
//! outside unfocuses it, so focus follows the mouse without help; moving
//! it with Tab is the program's job (`set_focused`).
//!
//! ```
//! use protolith::canvas::Canvas;
//! use protolith::ui::{Button, Orientation, Slider, Style};
//! use protolith::Rect;
//!
//! let play = Button::new(Rect::new(8, 8, 23, 18));
//! let volume = Slider::new(Rect::new(40, 12, 68, 13), Orientation::Horizontal, 100).with_thumb(14);
//! let mut canvas = Canvas::new(120, 34);
//! // Default looks...
//! volume.draw(&mut canvas, &Style::default());
//! // ...or a skin: here a sprite sheet with "up" at (0, 0) and "down" at (23, 0).
//! let sheet = Canvas::new(46, 18);
//! let sprite = if play.is_down() { Rect::new(23, 0, 23, 18) } else { Rect::new(0, 0, 23, 18) };
//! canvas.blit(&sheet.frame(), sprite, play.rect().x, play.rect().y);
//! ```

mod button;
mod edit;
mod field;
mod list;
mod slider;

pub use button::{Button, Toggle};
pub use edit::{Motion, TextEdit};
pub use field::{FieldStyle, TextField};
pub use list::List;
pub use slider::{Orientation, Slider};

use crate::canvas::Canvas;
use crate::pixel::Rect;

/// Two presses on the same list row within this many milliseconds (server
/// time) are a double click.
pub const DOUBLE_CLICK_MS: u32 = 400;

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
    /// The user committed: Enter in a text field, the end of a slider
    /// drag, a double click or Enter on a list row.
    Submit,
    /// Escape in a text field.
    Cancel,
    /// A button was clicked.
    Clicked,
}

/// Colors for the default widget looks (the text field has its own
/// [`FieldStyle`]). The default is gray 1996.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    /// Button faces, slider bodies and thumbs.
    pub face: u32,
    /// The lit edge of a bevel.
    pub light: u32,
    /// The shaded edge of a bevel; also disabled text and focus rings.
    pub shadow: u32,
    /// Text.
    pub text: u32,
    /// Behind list rows, inside slider grooves.
    pub background: u32,
    /// The selected list row, a lit toggle lamp.
    pub accent: u32,
    /// Text on the accent color.
    pub accent_text: u32,
}

impl Default for Style {
    fn default() -> Style {
        Style { face: 0xc0c0c0, light: 0xffffff, shadow: 0x808080, text: 0x000000, background: 0xffffff, accent: 0x000080, accent_text: 0xffffff }
    }
}

/// A one-pixel frame: top and left edges in one color, bottom and right in
/// another.
fn edge(canvas: &mut Canvas, r: Rect, top_left: u32, bottom_right: u32) {
    let last = |start: i32, len: u32| (i64::from(start) + i64::from(len) - 1).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    canvas.fill_rect(Rect::new(r.x, r.y, r.width, 1), top_left);
    canvas.fill_rect(Rect::new(r.x, r.y, 1, r.height), top_left);
    canvas.fill_rect(Rect::new(r.x, last(r.y, r.height), r.width, 1), bottom_right);
    canvas.fill_rect(Rect::new(last(r.x, r.width), r.y, 1, r.height), bottom_right);
}

/// A raised (light top-left) or sunken (shadow top-left) bevel.
fn bevel(canvas: &mut Canvas, r: Rect, style: &Style, raised: bool) {
    let (a, b) = if raised { (style.light, style.shadow) } else { (style.shadow, style.light) };
    edge(canvas, r, a, b);
}

/// The keyboard-focus mark: a frame two pixels inside the widget.
fn focus_ring(canvas: &mut Canvas, r: Rect, style: &Style) {
    if r.width > 4 && r.height > 4 {
        let inner = Rect::new(r.x.saturating_add(2), r.y.saturating_add(2), r.width - 4, r.height - 4);
        edge(canvas, inner, style.shadow, style.shadow);
    }
}

/// Focus follows a left press: in `rect` focuses, outside unfocuses.
/// Returns whether focus changed.
#[cfg(feature = "x11")]
fn click_focus(focused: &mut bool, rect: Rect, x: i32, y: i32) -> bool {
    let now = rect.contains(x, y);
    std::mem::replace(focused, now) != now
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::font::Font;

    /// A test font: `i` is 2 px wide, `m` 6, everything else 4; 6 px tall.
    /// Each glyph is a solid block in the color it is drawn with.
    pub(crate) struct Blocks;

    impl Font for Blocks {
        fn line_height(&self) -> u32 {
            6
        }
        fn advance(&self, ch: char) -> u32 {
            match ch {
                'i' => 2,
                'm' => 6,
                _ => 4,
            }
        }
        fn draw_char(&self, canvas: &mut Canvas, ch: char, x: i32, y: i32, color: u32) -> u32 {
            canvas.fill_rect(Rect::new(x, y, self.advance(ch), 6), color);
            self.advance(ch)
        }
    }

    /// Every color distinct, so tests can tell them apart.
    pub(crate) const STYLE: Style =
        Style { face: 0x111111, light: 0x222222, shadow: 0x333333, text: 0x444444, background: 0x555555, accent: 0x666666, accent_text: 0x888888 };

    #[cfg(feature = "x11")]
    mod events {
        use crate::x11::{Event, Key, Modifiers, MouseButton, MouseButtons, WindowId};

        const W: WindowId = WindowId(1);

        pub(crate) fn click(pressed: bool, x: i32, y: i32, time: u32) -> Event {
            Event::Button { window: W, button: MouseButton::Left, pressed, x, y, root_x: x, root_y: y, modifiers: Modifiers::default(), time }
        }
        pub(crate) fn motion(x: i32, y: i32, left: bool) -> Event {
            Event::Motion { window: W, x, y, root_x: x, root_y: y, buttons: MouseButtons { left, ..MouseButtons::default() }, modifiers: Modifiers::default() }
        }
        pub(crate) fn key(key: Key, repeat: bool) -> Event {
            Event::Key { window: W, key, keysym: 0, keycode: 0, pressed: true, repeat, modifiers: Modifiers::default() }
        }
        pub(crate) fn scroll(dx: i32, dy: i32, x: i32, y: i32) -> Event {
            Event::Scroll { window: W, dx, dy, x, y, modifiers: Modifiers::default() }
        }
    }
    #[cfg(feature = "x11")]
    pub(crate) use events::{click, key, motion, scroll};

    #[test]
    fn bevels_and_focus_rings() {
        let mut c = Canvas::new(8, 8);
        bevel(&mut c, Rect::new(0, 0, 8, 8), &STYLE, true);
        assert_eq!(
            (c.get(0, 0), c.get(7, 0), c.get(0, 7), c.get(7, 7), c.get(1, 1)),
            (Some(STYLE.light), Some(STYLE.shadow), Some(STYLE.shadow), Some(STYLE.shadow), Some(0))
        );
        focus_ring(&mut c, Rect::new(0, 0, 8, 8), &STYLE);
        assert_eq!((c.get(2, 2), c.get(5, 5), c.get(3, 3)), (Some(STYLE.shadow), Some(STYLE.shadow), Some(0)));
        for r in [Rect::new(0, 0, 0, 0), Rect::new(i32::MAX, i32::MIN, u32::MAX, u32::MAX), Rect::new(1, 1, 3, 3)] {
            bevel(&mut c, r, &STYLE, false);
            focus_ring(&mut c, r, &STYLE);
        }
    }
}
