//! [`Button`] and [`Toggle`].

use super::{bevel, focus_ring, Style};
#[cfg(feature = "x11")]
use super::{click_focus, Response};
use crate::canvas::Canvas;
use crate::font::Font;
use crate::pixel::Rect;
#[cfg(feature = "x11")]
use crate::x11::{Event, Key, MouseButton};

/// A push button. It fires on release, and only if the pointer is still
/// over it — press, drag off and release to back out — and, when focused,
/// on Space or Enter.
///
/// For a skin, draw the sprite for [`is_down`](Button::is_down) at
/// [`rect`](Button::rect) yourself instead of calling
/// [`draw`](Button::draw).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    rect: Rect,
    /// The left button went down on us and has not come up.
    armed: bool,
    /// ...and the pointer is over us now.
    inside: bool,
    focused: bool,
    enabled: bool,
}

impl Button {
    /// An enabled, unfocused button at `rect`.
    pub fn new(rect: Rect) -> Button {
        Button { rect, armed: false, inside: false, focused: false, enabled: true }
    }

    /// Where the button is.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Move or resize the button.
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// Should it look pressed? (Held down with the pointer over it.)
    pub fn is_down(&self) -> bool {
        self.armed && self.inside
    }

    /// Does it have keyboard focus?
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Give or take keyboard focus.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Does it respond to input?
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable or disable it; a disabled button ignores every event.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.armed &= enabled;
    }

    /// React to an event for this button's window: [`Response::Clicked`]
    /// when it fires.
    #[cfg(feature = "x11")]
    pub fn handle(&mut self, event: &Event) -> Response {
        if !self.enabled {
            return Response::Ignored;
        }
        match *event {
            Event::Button { button: MouseButton::Left, pressed: true, x, y, .. } => {
                let focus = click_focus(&mut self.focused, self.rect, x, y);
                self.armed = self.rect.contains(x, y);
                self.inside = self.armed;
                if self.armed || focus {
                    Response::Redraw
                } else {
                    Response::Ignored
                }
            }
            Event::Button { button: MouseButton::Left, pressed: false, x, y, .. } if self.armed => {
                self.armed = false;
                if self.rect.contains(x, y) {
                    Response::Clicked
                } else {
                    Response::Redraw
                }
            }
            Event::Motion { x, y, buttons, .. } if self.armed => {
                let was = self.is_down();
                // No button held: we missed the release (another window took it).
                self.armed = buttons.left;
                self.inside = self.rect.contains(x, y);
                if self.is_down() != was {
                    Response::Redraw
                } else {
                    Response::Ignored
                }
            }
            Event::Key { key: Key::Character(' ') | Key::Enter, pressed: true, repeat: false, .. } if self.focused => Response::Clicked,
            _ => Response::Ignored,
        }
    }

    /// Draw the default look: a raised bevel (sunken while down) with
    /// `label` centered in `font`.
    pub fn draw(&self, canvas: &mut Canvas, font: &dyn Font, label: &str, style: &Style) {
        draw_button(canvas, self, self.is_down(), font, label, style, 0);
    }
}

/// The shared button look; `indent` reserves room on the left (for the
/// toggle's lamp).
fn draw_button(canvas: &mut Canvas, b: &Button, sunken: bool, font: &dyn Font, label: &str, style: &Style, indent: u32) {
    let r = b.rect;
    canvas.fill_rect(r, style.face);
    bevel(canvas, r, style, !sunken);
    let saved = canvas.clip();
    canvas.set_clip(saved.intersect(&r));
    let push = i64::from(sunken);
    let x = i64::from(r.x) + i64::from(indent) + (i64::from(r.width.saturating_sub(indent)) - i64::from(font.width(label))) / 2 + push;
    let y = i64::from(r.y) + (i64::from(r.height) - i64::from(font.line_height())) / 2 + push;
    let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    font.draw(canvas, label, clamp(x), clamp(y), if b.enabled { style.text } else { style.shadow });
    canvas.set_clip(saved);
    if b.focused {
        focus_ring(canvas, r, style);
    }
}

/// A button that stays in or out: on, off, flip on click. The classic
/// shuffle and repeat buttons.
///
/// For a skin, pick the sprite from [`is_on`](Toggle::is_on) and
/// [`is_down`](Toggle::is_down).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toggle {
    button: Button,
    on: bool,
}

impl Toggle {
    /// A toggle at `rect`, initially `on` or off.
    pub fn new(rect: Rect, on: bool) -> Toggle {
        Toggle { button: Button::new(rect), on }
    }

    /// Is it on?
    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Turn it on or off (no response is generated).
    pub fn set_on(&mut self, on: bool) {
        self.on = on;
    }

    /// Where it is.
    pub fn rect(&self) -> Rect {
        self.button.rect
    }

    /// Move or resize it.
    pub fn set_rect(&mut self, rect: Rect) {
        self.button.rect = rect;
    }

    /// Held down with the pointer over it?
    pub fn is_down(&self) -> bool {
        self.button.is_down()
    }

    /// Does it have keyboard focus?
    pub fn is_focused(&self) -> bool {
        self.button.focused
    }

    /// Give or take keyboard focus.
    pub fn set_focused(&mut self, focused: bool) {
        self.button.focused = focused;
    }

    /// Does it respond to input?
    pub fn is_enabled(&self) -> bool {
        self.button.enabled
    }

    /// Enable or disable it.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.button.set_enabled(enabled);
    }

    /// React to an event: flips and returns [`Response::Changed`] when
    /// clicked (by the same rules as [`Button`]).
    #[cfg(feature = "x11")]
    pub fn handle(&mut self, event: &Event) -> Response {
        match self.button.handle(event) {
            Response::Clicked => {
                self.on = !self.on;
                Response::Changed
            }
            other => other,
        }
    }

    /// Draw the default look: a button that stays sunken while on, with a
    /// lamp on its left lit in the accent color.
    pub fn draw(&self, canvas: &mut Canvas, font: &dyn Font, label: &str, style: &Style) {
        let lamp = 6u32.min(self.button.rect.height / 2);
        let indent = lamp + 6;
        draw_button(canvas, &self.button, self.on || self.is_down(), font, label, style, indent);
        let r = self.button.rect;
        let at = Rect::new(r.x.saturating_add(4), r.y.saturating_add(((r.height - lamp) / 2) as i32), lamp, lamp);
        canvas.fill_rect(at.intersect(&r), if self.on { style.accent } else { style.shadow });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{Blocks, STYLE};

    #[test]
    fn draws_raised_sunken_and_disabled() {
        let mut b = Button::new(Rect::new(1, 1, 20, 10));
        let mut c = Canvas::new(22, 12);
        b.draw(&mut c, &Blocks, "ab", &STYLE);
        // Raised: light top-left, shadow bottom-right; label "ab" (8x6) centered at (7, 3).
        assert_eq!((c.get(1, 1), c.get(20, 10), c.get(2, 2)), (Some(STYLE.light), Some(STYLE.shadow), Some(STYLE.face)));
        assert_eq!((c.get(7, 3), c.get(14, 8), c.get(6, 3), c.get(15, 3)), (Some(STYLE.text), Some(STYLE.text), Some(STYLE.face), Some(STYLE.face)));
        b.armed = true;
        b.inside = true;
        b.draw(&mut c, &Blocks, "ab", &STYLE);
        assert_eq!((c.get(1, 1), c.get(20, 10)), (Some(STYLE.shadow), Some(STYLE.light)), "sunken");
        assert_eq!((c.get(7, 3), c.get(8, 4)), (Some(STYLE.face), Some(STYLE.text)), "the label shifts by one");
        b.set_enabled(false);
        assert!(!b.is_down(), "disabling releases it");
        b.draw(&mut c, &Blocks, "ab", &STYLE);
        assert_eq!(c.get(7, 3), Some(STYLE.shadow), "grayed label");
    }

    #[test]
    fn label_is_clipped_to_the_button() {
        let b = Button::new(Rect::new(5, 0, 6, 6));
        let mut c = Canvas::new(20, 6);
        c.fill(0x777777);
        b.draw(&mut c, &Blocks, "a long label", &STYLE);
        for x in (0..5).chain(11..20) {
            assert_eq!(c.get(x, 3), Some(0x777777), "x = {x}");
        }
        assert_eq!(c.clip(), c.bounds());
    }

    #[test]
    fn toggle_lamp_and_look_follow_the_state() {
        let mut t = Toggle::new(Rect::new(0, 0, 40, 12), false);
        let mut c = Canvas::new(40, 12);
        t.draw(&mut c, &Blocks, "", &STYLE);
        assert_eq!((c.get(4, 3), c.get(0, 0)), (Some(STYLE.shadow), Some(STYLE.light)), "lamp off, raised");
        t.set_on(true);
        t.draw(&mut c, &Blocks, "", &STYLE);
        assert_eq!((c.get(4, 3), c.get(9, 8), c.get(0, 0)), (Some(STYLE.accent), Some(STYLE.accent), Some(STYLE.shadow)), "lamp lit, sunken");
    }

    #[cfg(feature = "x11")]
    mod input {
        use super::*;
        use crate::ui::tests::{click, key, motion};

        #[test]
        fn fires_on_release_inside_only() {
            let mut b = Button::new(Rect::new(10, 10, 20, 10));
            assert_eq!(b.handle(&click(true, 15, 15, 100)), Response::Redraw);
            assert!(b.is_down() && b.is_focused());
            assert_eq!(b.handle(&click(false, 16, 16, 200)), Response::Clicked);
            assert!(!b.is_down());
            // Press, drag off (looks up), drag back (looks down), release outside: nothing.
            b.handle(&click(true, 15, 15, 300));
            assert_eq!(b.handle(&motion(50, 50, true)), Response::Redraw);
            assert!(!b.is_down());
            assert_eq!(b.handle(&motion(51, 50, true)), Response::Ignored);
            assert_eq!(b.handle(&motion(12, 12, true)), Response::Redraw);
            assert!(b.is_down());
            b.handle(&motion(50, 50, true));
            assert_eq!(b.handle(&click(false, 50, 50, 400)), Response::Redraw);
            // A press that started elsewhere never fires us.
            assert_eq!(b.handle(&click(true, 50, 50, 500)), Response::Redraw, "focus lost");
            assert!(!b.is_focused());
            assert_eq!(b.handle(&motion(15, 15, true)), Response::Ignored);
            assert_eq!(b.handle(&click(false, 15, 15, 600)), Response::Ignored);
            assert_eq!(b.handle(&click(true, 50, 50, 700)), Response::Ignored, "already unfocused");
        }

        #[test]
        fn a_missed_release_disarms() {
            let mut b = Button::new(Rect::new(0, 0, 10, 10));
            b.handle(&click(true, 5, 5, 0));
            assert_eq!(b.handle(&motion(6, 6, false)), Response::Redraw);
            assert!(!b.is_down());
            assert_eq!(b.handle(&click(false, 6, 6, 1)), Response::Ignored);
        }

        #[test]
        fn space_and_enter_fire_when_focused() {
            let mut b = Button::new(Rect::new(0, 0, 10, 10));
            assert_eq!(b.handle(&key(Key::Enter, false)), Response::Ignored);
            b.set_focused(true);
            assert_eq!(b.handle(&key(Key::Enter, false)), Response::Clicked);
            assert_eq!(b.handle(&key(Key::Character(' '), false)), Response::Clicked);
            assert_eq!(b.handle(&key(Key::Character(' '), true)), Response::Ignored, "not on autorepeat");
            assert_eq!(b.handle(&key(Key::Character('x'), false)), Response::Ignored);
        }

        #[test]
        fn disabled_ignores_everything() {
            let mut b = Button::new(Rect::new(0, 0, 10, 10));
            b.set_focused(true);
            b.set_enabled(false);
            for e in [click(true, 5, 5, 0), click(false, 5, 5, 1), key(Key::Enter, false), motion(5, 5, true)] {
                assert_eq!(b.handle(&e), Response::Ignored);
            }
        }

        #[test]
        fn toggle_flips_on_click() {
            let mut t = Toggle::new(Rect::new(0, 0, 10, 10), false);
            t.handle(&click(true, 5, 5, 0));
            assert!(t.is_down() && !t.is_on());
            assert_eq!(t.handle(&click(false, 5, 5, 1)), Response::Changed);
            assert!(t.is_on());
            assert_eq!(t.handle(&key(Key::Character(' '), false)), Response::Changed);
            assert!(!t.is_on());
            t.handle(&click(true, 5, 5, 2));
            assert_eq!(t.handle(&click(false, 50, 5, 3)), Response::Redraw, "released outside");
            assert!(!t.is_on());
        }
    }
}
