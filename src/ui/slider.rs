//! [`Slider`].

use super::{bevel, focus_ring, Style};
#[cfg(feature = "x11")]
use super::{click_focus, Response};
use crate::canvas::Canvas;
use crate::pixel::Rect;
#[cfg(feature = "x11")]
use crate::x11::{Event, Key, MouseButton};

/// Which way a [`Slider`] runs. Values grow rightward, or upward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Minimum at the left.
    Horizontal,
    /// Minimum at the bottom.
    Vertical,
}

/// A value from `0` to `max`, set by dragging a thumb along a track:
/// volume, balance, seek position, an equalizer band.
///
/// Pressing on the thumb drags it from where it was grabbed; pressing
/// elsewhere on the track jumps the thumb's center there, then drags. The
/// drag follows the pointer anywhere until the button is released.
/// [`Response::Changed`] reports each new value and
/// [`Response::Submit`] the end of a drag — a seek bar seeks on that. The
/// wheel over it, and (when focused) the arrow keys, change it by
/// [`step`](Slider::with_step); Page Up/Down by ten steps; Home and End go
/// to the ends.
///
/// For a skin, blit the thumb sprite at [`thumb_rect`](Slider::thumb_rect)
/// (pressed-looking while [`is_dragging`](Slider::is_dragging)) and pick
/// the background from [`value`](Slider::value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slider {
    rect: Rect,
    orientation: Orientation,
    value: u32,
    max: u32,
    thumb: u32,
    step: u32,
    /// While dragging: where along the thumb it was grabbed.
    grab: Option<u32>,
    focused: bool,
    enabled: bool,
}

impl Slider {
    /// A slider at `rect` over `0..=max`, at 0. The thumb is as long as the
    /// track is wide (square), and the step is a twentieth of `max`.
    pub fn new(rect: Rect, orientation: Orientation, max: u32) -> Slider {
        let thumb = match orientation {
            Orientation::Horizontal => rect.height,
            Orientation::Vertical => rect.width,
        };
        Slider { rect, orientation, value: 0, max, thumb, step: (max / 20).max(1), grab: None, focused: false, enabled: true }
    }

    /// Set the thumb's length along the track, in pixels.
    pub fn with_thumb(mut self, length: u32) -> Slider {
        self.thumb = length;
        self
    }

    /// Set how far the wheel and arrow keys move the value.
    pub fn with_step(mut self, step: u32) -> Slider {
        self.step = step;
        self
    }

    /// The value, `0..=max`.
    pub fn value(&self) -> u32 {
        self.value
    }

    /// Set the value (clamped to `max`; no response is generated).
    pub fn set_value(&mut self, value: u32) {
        self.value = value.min(self.max);
    }

    /// The largest value.
    pub fn max(&self) -> u32 {
        self.max
    }

    /// Change the range (the value is clamped into it).
    pub fn set_max(&mut self, max: u32) {
        self.max = max;
        self.value = self.value.min(max);
    }

    /// Where the slider is.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Move or resize the slider.
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// Is the thumb being dragged?
    pub fn is_dragging(&self) -> bool {
        self.grab.is_some()
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

    /// Enable or disable it; a disabled slider ignores every event.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.grab = None;
        }
    }

    /// `(length along the track, thumb length)`, the thumb no longer than the track.
    fn lengths(&self) -> (u32, u32) {
        let along = match self.orientation {
            Orientation::Horizontal => self.rect.width,
            Orientation::Vertical => self.rect.height,
        };
        (along, self.thumb.min(along))
    }

    /// Thumb offset from the track's minimum end for `value`, rounded.
    fn offset(&self, value: u32) -> u32 {
        let (along, thumb) = self.lengths();
        let travel = u64::from(along - thumb);
        if self.max == 0 {
            return 0;
        }
        ((u64::from(value) * travel + u64::from(self.max) / 2) / u64::from(self.max)) as u32
    }

    /// The value whose thumb offset is nearest `offset`.
    #[cfg(feature = "x11")]
    fn value_at(&self, offset: i64) -> u32 {
        let (along, thumb) = self.lengths();
        let travel = i64::from(along - thumb);
        if travel == 0 {
            return 0;
        }
        let o = offset.clamp(0, travel) as u64;
        ((o * u64::from(self.max) + travel as u64 / 2) / travel as u64) as u32
    }

    /// The pixel index along the track, from its minimum end, of a point.
    #[cfg(feature = "x11")]
    fn along(&self, x: i32, y: i32) -> i64 {
        match self.orientation {
            Orientation::Horizontal => i64::from(x) - i64::from(self.rect.x),
            Orientation::Vertical => i64::from(self.rect.y) + i64::from(self.rect.height) - 1 - i64::from(y),
        }
    }

    /// Where the thumb is drawn.
    pub fn thumb_rect(&self) -> Rect {
        let (along, thumb) = self.lengths();
        let r = self.rect;
        let o = self.offset(self.value);
        let at = |start: i32, by: u32| (i64::from(start) + i64::from(by)).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        match self.orientation {
            Orientation::Horizontal => Rect::new(at(r.x, o), r.y, thumb, r.height),
            Orientation::Vertical => Rect::new(r.x, at(r.y, along - thumb - o), r.width, thumb),
        }
    }

    #[cfg(feature = "x11")]
    fn set(&mut self, value: i64) -> Response {
        let value = value.clamp(0, i64::from(self.max)) as u32;
        let changed = value != self.value;
        self.value = value;
        if changed {
            Response::Changed
        } else {
            Response::Redraw
        }
    }

    /// React to an event for this slider's window.
    #[cfg(feature = "x11")]
    pub fn handle(&mut self, event: &Event) -> Response {
        if !self.enabled {
            return Response::Ignored;
        }
        let (value, step) = (i64::from(self.value), i64::from(self.step));
        match *event {
            Event::Button { button: MouseButton::Left, pressed: true, x, y, .. } => {
                let focus = click_focus(&mut self.focused, self.rect, x, y);
                if !self.rect.contains(x, y) {
                    return if focus { Response::Redraw } else { Response::Ignored };
                }
                let (_, thumb) = self.lengths();
                let (a, o) = (self.along(x, y), i64::from(self.offset(self.value)));
                let grab = if (o..o + i64::from(thumb)).contains(&a) { a - o } else { i64::from(thumb / 2) };
                self.grab = Some(grab as u32);
                self.set(i64::from(self.value_at(a - grab)))
            }
            Event::Motion { x, y, buttons, .. } if self.grab.is_some() => {
                if !buttons.left {
                    self.grab = None;
                    return Response::Submit;
                }
                let v = self.value_at(self.along(x, y) - i64::from(self.grab.unwrap_or(0)));
                match self.set(i64::from(v)) {
                    Response::Changed => Response::Changed,
                    _ => Response::Ignored,
                }
            }
            Event::Button { button: MouseButton::Left, pressed: false, .. } if self.grab.is_some() => {
                self.grab = None;
                Response::Submit
            }
            Event::Scroll { dx, dy, x, y, .. } if self.rect.contains(x, y) => self.set(value + (i64::from(dx) - i64::from(dy)) * step),
            Event::Key { key, pressed: true, .. } if self.focused => match key {
                Key::Left | Key::Down => self.set(value - step),
                Key::Right | Key::Up => self.set(value + step),
                Key::PageDown => self.set(value - 10 * step),
                Key::PageUp => self.set(value + 10 * step),
                Key::Home => self.set(0),
                Key::End => self.set(i64::from(self.max)),
                _ => Response::Ignored,
            },
            _ => Response::Ignored,
        }
    }

    /// Draw the default look: a sunken groove along the middle of the
    /// track and a raised thumb (sunken while dragged).
    pub fn draw(&self, canvas: &mut Canvas, style: &Style) {
        let r = self.rect;
        canvas.fill_rect(r, style.face);
        let groove = match self.orientation {
            Orientation::Horizontal => Rect::new(r.x, r.y.saturating_add((r.height / 2) as i32 - 2), r.width, 4.min(r.height)),
            Orientation::Vertical => Rect::new(r.x.saturating_add((r.width / 2) as i32 - 2), r.y, 4.min(r.width), r.height),
        };
        canvas.fill_rect(groove, style.background);
        bevel(canvas, groove, style, false);
        let t = self.thumb_rect();
        canvas.fill_rect(t, if self.enabled { style.face } else { style.shadow });
        bevel(canvas, t, style, !self.is_dragging());
        if self.focused {
            focus_ring(canvas, r, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::STYLE;

    #[test]
    fn thumb_position_maps_the_whole_range() {
        // 100 px track, 10 px thumb: 90 px of travel.
        let mut s = Slider::new(Rect::new(5, 0, 100, 10), Orientation::Horizontal, 9);
        let xs: Vec<i32> = (0..=9).map(|v| (s.set_value(v), s.thumb_rect().x).1).collect();
        assert_eq!(xs, [5, 15, 25, 35, 45, 55, 65, 75, 85, 95]);
        assert_eq!(s.thumb_rect(), Rect::new(95, 0, 10, 10));
        let mut v = Slider::new(Rect::new(0, 5, 10, 100), Orientation::Vertical, 9);
        v.set_value(0);
        assert_eq!(v.thumb_rect(), Rect::new(0, 95, 10, 10), "vertical: minimum at the bottom");
        v.set_value(9);
        assert_eq!(v.thumb_rect(), Rect::new(0, 5, 10, 10));
        // Positions round to the nearest pixel: 10 px of travel over 0..=3.
        let mut r = Slider::new(Rect::new(0, 0, 20, 10), Orientation::Horizontal, 3);
        let xs: Vec<i32> = (0..=3).map(|v| (r.set_value(v), r.thumb_rect().x).1).collect();
        assert_eq!(xs, [0, 3, 7, 10]);
    }

    #[test]
    fn values_clamp_and_degenerate_geometry_is_total() {
        let mut s = Slider::new(Rect::new(0, 0, 50, 10), Orientation::Horizontal, 10);
        s.set_value(99);
        assert_eq!(s.value(), 10);
        s.set_max(4);
        assert_eq!(s.value(), 4);
        for (rect, max, thumb) in
            [(Rect::new(0, 0, 0, 0), 0, 0), (Rect::new(0, 0, 5, 5), 100, 50), (Rect::new(i32::MAX, i32::MAX, u32::MAX, u32::MAX), u32::MAX, 3)]
        {
            for o in [Orientation::Horizontal, Orientation::Vertical] {
                let mut s = Slider::new(rect, o, max).with_thumb(thumb);
                s.set_value(max);
                let mut c = Canvas::new(8, 8);
                s.draw(&mut c, &STYLE);
                let _ = s.thumb_rect();
            }
        }
    }

    #[test]
    fn draws_groove_and_thumb() {
        let mut s = Slider::new(Rect::new(0, 0, 40, 10), Orientation::Horizontal, 3);
        s.set_value(3);
        let mut c = Canvas::new(40, 10);
        s.draw(&mut c, &STYLE);
        // Groove at rows 3..7 (sunken: shadow on top), thumb at x 30..40 (raised).
        assert_eq!((c.get(5, 3), c.get(5, 6), c.get(5, 4)), (Some(STYLE.shadow), Some(STYLE.light), Some(STYLE.background)));
        assert_eq!((c.get(30, 0), c.get(39, 9), c.get(35, 5)), (Some(STYLE.light), Some(STYLE.shadow), Some(STYLE.face)));
    }

    #[cfg(feature = "x11")]
    mod input {
        use super::*;
        use crate::ui::tests::{click, key, motion, scroll};

        fn slider() -> Slider {
            // Track x 0..100, thumb 10: value v (0..=9) puts the thumb at 10 v.
            Slider::new(Rect::new(0, 0, 100, 10), Orientation::Horizontal, 9)
        }

        #[test]
        fn press_on_the_track_jumps_then_drags_until_release() {
            let mut s = slider();
            assert_eq!(s.handle(&click(true, 54, 5, 0)), Response::Changed, "thumb centered on x 54: offset 49 → 5");
            assert_eq!((s.value(), s.is_dragging(), s.is_focused()), (5, true, true));
            assert_eq!(s.handle(&motion(76, 5, true)), Response::Changed);
            assert_eq!(s.value(), 7);
            assert_eq!(s.handle(&motion(77, 5, true)), Response::Ignored, "same value");
            assert_eq!(s.handle(&motion(500, 300, true)), Response::Changed, "the drag follows the pointer anywhere");
            assert_eq!(s.value(), 9);
            assert_eq!(s.handle(&motion(-500, 300, true)), Response::Changed);
            assert_eq!(s.value(), 0);
            assert_eq!(s.handle(&click(false, -500, 300, 1)), Response::Submit);
            assert_eq!(s.handle(&motion(50, 5, true)), Response::Ignored);
            assert_eq!(s.value(), 0);
        }

        #[test]
        fn press_on_the_thumb_keeps_the_grab_point() {
            let mut s = slider();
            s.set_value(3); // thumb at 30..40
            assert_eq!(s.handle(&click(true, 38, 5, 0)), Response::Redraw, "grabbed 8 px in: no jump");
            assert_eq!(s.value(), 3);
            assert_eq!(s.handle(&motion(48, 5, true)), Response::Changed);
            assert_eq!(s.value(), 4, "moved 10 px");
        }

        #[test]
        fn vertical_runs_upward() {
            let mut s = Slider::new(Rect::new(0, 0, 10, 100), Orientation::Vertical, 9);
            s.handle(&click(true, 5, 4, 0));
            assert_eq!(s.value(), 9, "near the top is the maximum");
            s.handle(&motion(5, 95, true));
            assert_eq!(s.value(), 0);
            s.handle(&click(false, 5, 95, 1));
            // The thumb is at rows 90..100: its top row grabs it, the row above jumps.
            assert_eq!(s.handle(&click(true, 5, 90, 2)), Response::Redraw);
            s.handle(&click(false, 5, 90, 3));
            assert_eq!(s.handle(&click(true, 5, 89, 4)), Response::Changed);
        }

        #[test]
        fn a_missed_release_ends_the_drag() {
            let mut s = slider();
            s.handle(&click(true, 50, 5, 0));
            assert_eq!(s.handle(&motion(80, 5, false)), Response::Submit);
            assert!(!s.is_dragging());
        }

        #[test]
        fn wheel_and_keys_step() {
            let mut s = Slider::new(Rect::new(0, 0, 100, 10), Orientation::Horizontal, 100).with_step(3);
            assert_eq!(s.step, 3);
            assert_eq!(s.handle(&scroll(0, -1, 50, 5)), Response::Changed, "wheel up raises");
            assert_eq!(s.value(), 3);
            assert_eq!(s.handle(&scroll(0, 2, 50, 5)), Response::Changed);
            assert_eq!(s.value(), 0);
            assert_eq!(s.handle(&scroll(0, 1, 50, 5)), Response::Redraw, "already at the bottom");
            assert_eq!(s.handle(&scroll(0, -1, 500, 5)), Response::Ignored, "not over the slider");
            assert_eq!(s.handle(&key(Key::Right, false)), Response::Ignored, "unfocused");
            s.set_focused(true);
            for (k, want) in
                [(Key::Right, 3), (Key::Up, 6), (Key::PageUp, 36), (Key::Down, 33), (Key::End, 100), (Key::PageUp, 100), (Key::Home, 0), (Key::Left, 0)]
            {
                s.handle(&key(k, false));
                assert_eq!(s.value(), want, "{k:?}");
            }
            assert_eq!(Slider::new(Rect::new(0, 0, 1, 1), Orientation::Horizontal, 7).step, 1, "default step is at least 1");
        }

        #[test]
        fn disabled_ignores_everything() {
            let mut s = slider();
            s.set_focused(true);
            s.handle(&click(true, 50, 5, 0));
            s.set_enabled(false);
            assert!(!s.is_dragging());
            for e in [click(true, 50, 5, 0), motion(60, 5, true), scroll(0, 1, 50, 5), key(Key::Right, false)] {
                assert_eq!(s.handle(&e), Response::Ignored);
            }
        }
    }
}
