//! Sprite-sheet looks for widgets: where each state's picture is in a
//! sheet, and `draw_sprite` methods that pick the right one.

use super::{Button, Slider, Toggle};
use crate::canvas::{Blit, Canvas};
use crate::pixel::{Frame, Rect};

/// A run of same-sized pictures in a sheet — a slider's track at each
/// level, say — laid out in a column, a row or a grid.
///
/// Frame `i` is `first` moved by `step.0` per column and `step.1` per row,
/// filling `columns` frames per row: one column of 28 frames, 15 pixels
/// apart, is `columns: 1, step: (0, 15)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Frames {
    /// Frame 0.
    pub first: Rect,
    /// How many frames there are (at least one is assumed).
    pub count: u32,
    /// Frames per row before wrapping to the next.
    pub columns: u32,
    /// Offset from one frame to the next in a row, and from row to row.
    pub step: (i32, i32),
}

impl Frames {
    /// A single picture.
    pub const fn single(rect: Rect) -> Frames {
        Frames { first: rect, count: 1, columns: 1, step: (0, 0) }
    }

    /// `count` frames in one column, `dy` pixels apart.
    pub const fn column(first: Rect, count: u32, dy: i32) -> Frames {
        Frames { first, count, columns: 1, step: (0, dy) }
    }

    /// Where frame `i` is (clamped to the last frame).
    pub fn rect(&self, i: u32) -> Rect {
        let i = i.min(self.count.saturating_sub(1));
        let columns = self.columns.max(1);
        let at = |start: i32, n: u32, step: i32| (i64::from(start) + i64::from(n) * i64::from(step)).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        Rect { x: at(self.first.x, i % columns, self.step.0), y: at(self.first.y, i / columns, self.step.1), ..self.first }
    }
}

/// A [`Button`]'s pictures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ButtonSprites {
    /// At rest.
    pub up: Rect,
    /// Held down with the pointer over it.
    pub down: Rect,
}

/// A [`Toggle`]'s pictures: off and on, each at rest and held down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToggleSprites {
    /// Off, at rest.
    pub off: Rect,
    /// Off, held down.
    pub off_down: Rect,
    /// On, at rest.
    pub on: Rect,
    /// On, held down.
    pub on_down: Rect,
}

/// A [`Slider`]'s pictures: a track that may change with the value, and a
/// thumb. An empty rectangle draws nothing (a slider with no thumb, or a
/// track painted elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SliderSprites {
    /// The track, drawn at the slider's top-left: frame 0 at the minimum,
    /// the last at the maximum, evenly between.
    pub track: Frames,
    /// Count track frames outward from the middle of the range instead —
    /// frame 0 centered, the last at either end (a balance control).
    pub centered: bool,
    /// The thumb, centered on [`Slider::thumb_rect`].
    pub thumb: Rect,
    /// The thumb while dragged.
    pub thumb_down: Rect,
}

/// Draw `from` of `sheet` with its top-left at `(x, y)`. Alpha-blended, so
/// an RGBA sheet's transparency works and an XRGB one is copied.
fn sprite(canvas: &mut Canvas, sheet: &Frame<'_>, from: Rect, x: i32, y: i32) {
    canvas.blit_with(sheet, from, Rect::new(x, y, from.width, from.height), Blit::Alpha);
}

impl Button {
    /// Draw the button from a sprite sheet, at its rectangle's top-left.
    pub fn draw_sprite(&self, canvas: &mut Canvas, sheet: &Frame<'_>, sprites: &ButtonSprites) {
        let r = self.rect();
        sprite(canvas, sheet, if self.is_down() { sprites.down } else { sprites.up }, r.x, r.y);
    }
}

impl Toggle {
    /// Draw the toggle from a sprite sheet, at its rectangle's top-left.
    pub fn draw_sprite(&self, canvas: &mut Canvas, sheet: &Frame<'_>, sprites: &ToggleSprites) {
        let from = match (self.is_on(), self.is_down()) {
            (false, false) => sprites.off,
            (false, true) => sprites.off_down,
            (true, false) => sprites.on,
            (true, true) => sprites.on_down,
        };
        let r = self.rect();
        sprite(canvas, sheet, from, r.x, r.y);
    }
}

impl Slider {
    /// The track frame for the current value.
    pub fn track_frame(&self, sprites: &SliderSprites) -> u32 {
        let last = u64::from(sprites.track.count.saturating_sub(1));
        let (v, max) = (u64::from(self.value()), u64::from(self.max()));
        if max == 0 {
            return 0;
        }
        // Distance from the low end, or from the middle (both out of `max`).
        let d = if sprites.centered { (2 * v).abs_diff(max) } else { v };
        ((d * last + max / 2) / max) as u32
    }

    /// Draw the slider from a sprite sheet: the track frame for the value
    /// at the slider's top-left, then the thumb (pressed while dragged)
    /// centered on [`thumb_rect`](Slider::thumb_rect). Give the slider a
    /// [`with_thumb`](Slider::with_thumb) length that matches the sprite.
    pub fn draw_sprite(&self, canvas: &mut Canvas, sheet: &Frame<'_>, sprites: &SliderSprites) {
        let r = self.rect();
        sprite(canvas, sheet, sprites.track.rect(self.track_frame(sprites)), r.x, r.y);
        let t = self.thumb_rect();
        let thumb = if self.is_dragging() { sprites.thumb_down } else { sprites.thumb };
        let center = |start: i32, room: u32, len: u32| {
            (i64::from(start) + (i64::from(room) - i64::from(len)) / 2).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
        };
        sprite(canvas, sheet, thumb, center(t.x, t.width, thumb.width), center(t.y, t.height, thumb.height));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Orientation;

    /// A sheet whose every pixel is distinct: `y << 8 | x`.
    fn sheet() -> Canvas {
        Canvas::from_pixels((0..64u32).flat_map(|y| (0..64u32).map(move |x| y << 8 | x)).collect(), 64, 64).unwrap()
    }

    #[test]
    fn frames_walk_columns_rows_and_grids() {
        let col = Frames::column(Rect::new(2, 1, 10, 4), 5, 6);
        assert_eq!((col.rect(0), col.rect(3), col.rect(99)), (Rect::new(2, 1, 10, 4), Rect::new(2, 19, 10, 4), Rect::new(2, 25, 10, 4)));
        let grid = Frames { first: Rect::new(0, 0, 3, 3), count: 7, columns: 3, step: (4, 5) };
        let at: Vec<(i32, i32)> = (0..7).map(|i| (grid.rect(i).x, grid.rect(i).y)).collect();
        assert_eq!(at, [(0, 0), (4, 0), (8, 0), (0, 5), (4, 5), (8, 5), (0, 10)]);
        assert_eq!(Frames::single(Rect::new(1, 1, 1, 1)).rect(5), Rect::new(1, 1, 1, 1));
        let odd = Frames { first: Rect::new(i32::MAX, 0, 1, 1), count: 0, columns: 0, step: (i32::MAX, i32::MIN) };
        assert_eq!(odd.rect(9), Rect::new(i32::MAX, 0, 1, 1), "no frames, no columns: still total");
    }

    #[test]
    fn buttons_and_toggles_pick_their_state() {
        let s = sheet();
        let b = Button::new(Rect::new(1, 1, 4, 4));
        let mut c = Canvas::new(8, 8);
        b.draw_sprite(&mut c, &s.frame(), &ButtonSprites { up: Rect::new(10, 20, 2, 2), down: Rect::new(30, 40, 2, 2) });
        assert_eq!((c.get(1, 1), c.get(2, 2), c.get(3, 3)), (Some(20 << 8 | 10), Some(21 << 8 | 11), Some(0)), "up, at the top-left, its own size");
        let sprites = ToggleSprites { off: Rect::new(0, 0, 1, 1), off_down: Rect::new(1, 0, 1, 1), on: Rect::new(2, 0, 1, 1), on_down: Rect::new(3, 0, 1, 1) };
        let mut t = Toggle::new(Rect::new(0, 0, 1, 1), false);
        t.draw_sprite(&mut c, &s.frame(), &sprites);
        assert_eq!(c.get(0, 0), Some(0));
        t.set_on(true);
        t.draw_sprite(&mut c, &s.frame(), &sprites);
        assert_eq!(c.get(0, 0), Some(2));
    }

    #[test]
    fn track_frames_follow_the_value_linearly_or_from_the_center() {
        let mut s = Slider::new(Rect::new(0, 0, 50, 5), Orientation::Horizontal, 100);
        let linear =
            SliderSprites { track: Frames::column(Rect::new(0, 0, 50, 5), 28, 6), centered: false, thumb: Rect::default(), thumb_down: Rect::default() };
        let centered = SliderSprites { centered: true, ..linear };
        let frames =
            |s: &mut Slider, sp: &SliderSprites| -> Vec<u32> { [0, 1, 50, 51, 99, 100].iter().map(|&v| (s.set_value(v), s.track_frame(sp)).1).collect() };
        assert_eq!(frames(&mut s, &linear), [0, 0, 14, 14, 27, 27]);
        assert_eq!(frames(&mut s, &centered), [27, 26, 0, 1, 26, 27]);
        s.set_max(0);
        assert_eq!(s.track_frame(&linear), 0);
        let none = SliderSprites { track: Frames { count: 0, ..linear.track }, ..linear };
        assert_eq!(Slider::new(Rect::new(0, 0, 9, 9), Orientation::Vertical, 10).track_frame(&none), 0);
    }

    #[test]
    fn slider_draws_track_frame_then_centered_thumb() {
        let s = sheet();
        // 20x6 slider, thumb length 4; the thumb sprite is 4x2, so it sits 2 rows down.
        let mut sl = Slider::new(Rect::new(0, 0, 20, 6), Orientation::Horizontal, 2).with_thumb(4);
        sl.set_value(2);
        let sprites = SliderSprites {
            track: Frames::column(Rect::new(0, 10, 20, 6), 3, 6),
            centered: false,
            thumb: Rect::new(40, 0, 4, 2),
            thumb_down: Rect::new(50, 0, 4, 2),
        };
        let mut c = Canvas::new(20, 6);
        sl.draw_sprite(&mut c, &s.frame(), &sprites);
        assert_eq!(c.get(0, 0), Some(22 << 8), "track frame 2 (y = 10 + 2 * 6)");
        assert_eq!((c.get(16, 2), c.get(19, 3), c.get(16, 1), c.get(16, 4)), (Some(40), Some(1 << 8 | 43), Some(23 << 8 | 16), Some(26 << 8 | 16)));
        // A vertical slider centers across its width the same way.
        let mut v = Slider::new(Rect::new(0, 0, 6, 20), Orientation::Vertical, 2).with_thumb(4);
        v.set_value(2);
        let mut c = Canvas::new(6, 20);
        v.draw_sprite(
            &mut c,
            &s.frame(),
            &SliderSprites { track: Frames::single(Rect::default()), centered: false, thumb: Rect::new(40, 0, 2, 4), thumb_down: Rect::default() },
        );
        assert_eq!((c.get(2, 0), c.get(3, 3), c.get(1, 0), c.get(4, 0)), (Some(40), Some(3 << 8 | 41), Some(0), Some(0)));
    }

    #[test]
    fn rgba_sheets_blend() {
        let px = [255, 0, 0, 255, 0, 0, 255, 0];
        let sheet = Frame::rgba8(&px, 2, 1).unwrap();
        let b = Button::new(Rect::new(0, 0, 2, 1));
        let mut c = Canvas::new(2, 1);
        c.fill(0x00ff00);
        b.draw_sprite(&mut c, &sheet, &ButtonSprites { up: Rect::new(0, 0, 2, 1), down: Rect::default() });
        assert_eq!(c.pixels(), &[0xff0000, 0x00ff00], "opaque copied, transparent skipped");
    }

    #[cfg(feature = "x11")]
    #[test]
    fn pressed_states_use_their_sprites() {
        use crate::ui::tests::click;
        let s = sheet();
        let mut b = Button::new(Rect::new(0, 0, 2, 2));
        b.handle(&click(true, 1, 1, 0));
        let mut c = Canvas::new(2, 2);
        b.draw_sprite(&mut c, &s.frame(), &ButtonSprites { up: Rect::new(0, 0, 2, 2), down: Rect::new(30, 40, 2, 2) });
        assert_eq!(c.get(0, 0), Some(40 << 8 | 30));
        let mut sl = Slider::new(Rect::new(0, 0, 10, 2), Orientation::Horizontal, 1).with_thumb(2);
        sl.handle(&click(true, 1, 1, 0));
        assert!(sl.is_dragging());
        let mut c = Canvas::new(10, 2);
        sl.draw_sprite(
            &mut c,
            &s.frame(),
            &SliderSprites { track: Frames::single(Rect::default()), centered: false, thumb: Rect::new(0, 0, 2, 2), thumb_down: Rect::new(50, 50, 2, 2) },
        );
        assert_eq!(c.get(0, 0), Some(50 << 8 | 50));
    }
}
