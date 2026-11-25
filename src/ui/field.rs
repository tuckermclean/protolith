//! [`TextField`]: a [`TextEdit`] on screen — input, scrolling, drawing.

#[cfg(feature = "x11")]
use super::edit::Motion;
use super::edit::TextEdit;
#[cfg(feature = "x11")]
use super::Response;
use crate::canvas::Canvas;
use crate::font::Font;
use crate::pixel::Rect;
#[cfg(feature = "x11")]
use crate::x11::{Event, Key, MouseButton};

/// Colors for [`TextField::draw`]. The default is a sunken white box with
/// black text and a navy selection; a skin that paints its own field sets
/// `background` and `bevel` to `None` and gets only the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldStyle {
    /// Fill behind the text, or `None` to leave the canvas alone.
    pub background: Option<u32>,
    /// A one-pixel sunken border, `(top and left, bottom and right)`.
    pub bevel: Option<(u32, u32)>,
    /// Text color (fonts with colors of their own ignore it).
    pub text: u32,
    /// Behind selected text.
    pub selection: u32,
    /// Selected text.
    pub selected_text: u32,
    /// The cursor bar, shown while focused; `None` hides it (to blink it,
    /// alternate).
    pub cursor: Option<u32>,
}

impl Default for FieldStyle {
    fn default() -> FieldStyle {
        FieldStyle {
            background: Some(0xffffff),
            bevel: Some((0x808080, 0xffffff)),
            text: 0x000000,
            selection: 0x000080,
            selected_text: 0xffffff,
            cursor: Some(0x000000),
        }
    }
}

/// A single-line text input: a [`TextEdit`] placed at a rectangle, with
/// keyboard focus, mouse placement and drag-selection, and horizontal
/// scrolling that keeps the cursor in view.
///
/// Feed it events with [`handle`](TextField::handle) (only those for its
/// window) and draw it with [`draw`](TextField::draw), passing the same
/// font to both. Keys while focused: typing, Backspace/Delete (with Ctrl,
/// a word), Left/Right (with Ctrl, a word), Home/End, Shift with any move
/// to select, Ctrl+A to select all, Enter → [`Response::Submit`], Escape
/// → [`Response::Cancel`]. A left click inside focuses it and places the
/// cursor (Shift extends the selection); a left click outside unfocuses it.
#[derive(Debug, Clone)]
pub struct TextField {
    /// The text, cursor and selection — edit them directly as you like.
    pub edit: TextEdit,
    rect: Rect,
    padding: u32,
    focused: bool,
    dragging: bool,
    /// Pixels of text scrolled off the left edge.
    scroll: u32,
}

impl TextField {
    /// An empty, unfocused field at `rect`, with 3 pixels of padding.
    pub fn new(rect: Rect) -> TextField {
        TextField { edit: TextEdit::new(), rect, padding: 3, focused: false, dragging: false, scroll: 0 }
    }

    /// Space between the field's edge and its text.
    pub fn with_padding(mut self, padding: u32) -> TextField {
        self.padding = padding;
        self
    }

    /// Where the field is.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Move or resize the field.
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// The area text is drawn in: the rectangle less its padding.
    pub fn text_area(&self) -> Rect {
        let r = self.rect;
        let p = self.padding.min(r.width / 2).min(r.height / 2);
        Rect::new(r.x.saturating_add(p as i32), r.y.saturating_add(p as i32), r.width - 2 * p, r.height - 2 * p)
    }

    /// Does the field have keyboard focus?
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Give or take keyboard focus (e.g. on Tab).
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
        self.dragging &= focused;
    }

    /// React to an event for this field's window. See the type's docs
    /// for the bindings; anything else is [`Response::Ignored`], so the
    /// caller can use it.
    #[cfg(feature = "x11")]
    pub fn handle(&mut self, event: &Event, font: &dyn Font) -> Response {
        let response = match *event {
            Event::Button { button: MouseButton::Left, pressed: true, x, y, modifiers, .. } => {
                self.dragging = self.rect.contains(x, y);
                if !self.dragging {
                    let was = std::mem::replace(&mut self.focused, false);
                    return if was { Response::Redraw } else { Response::Ignored };
                }
                self.focused = true;
                self.edit.set_cursor(self.hit(x, font), modifiers.shift);
                Response::Redraw
            }
            Event::Button { button: MouseButton::Left, pressed: false, .. } => {
                self.dragging = false;
                Response::Ignored
            }
            Event::Motion { x, buttons, .. } if self.dragging => {
                self.dragging = buttons.left;
                let at = self.hit(x, font);
                if !self.dragging || at == self.edit.cursor() {
                    return Response::Ignored;
                }
                self.edit.set_cursor(at, true);
                Response::Redraw
            }
            Event::Key { key, pressed: true, modifiers: m, .. } if self.focused => {
                let edited = |changed: bool| if changed { Response::Changed } else { Response::Redraw };
                let word = |plain, word| if m.ctrl { word } else { plain };
                match key {
                    Key::Character(c) if m.ctrl && !m.alt && c.eq_ignore_ascii_case(&'a') => {
                        self.edit.select_all();
                        Response::Redraw
                    }
                    Key::Character(c) if !m.ctrl && !m.alt => edited(self.edit.insert(c.encode_utf8(&mut [0; 4]))),
                    Key::Backspace => edited(self.edit.delete(word(Motion::Left, Motion::WordLeft))),
                    Key::Delete => edited(self.edit.delete(word(Motion::Right, Motion::WordRight))),
                    Key::Left | Key::Right | Key::Home | Key::End => {
                        let motion = match key {
                            Key::Left => word(Motion::Left, Motion::WordLeft),
                            Key::Right => word(Motion::Right, Motion::WordRight),
                            Key::Home => Motion::Home,
                            _ => Motion::End,
                        };
                        self.edit.move_cursor(motion, m.shift);
                        Response::Redraw
                    }
                    Key::Enter => Response::Submit,
                    Key::Escape => Response::Cancel,
                    _ => Response::Ignored,
                }
            }
            _ => Response::Ignored,
        };
        self.reveal(font);
        response
    }

    /// Draw the field onto `canvas` in `style`. Takes `&mut self` because
    /// drawing scrolls the text to keep the cursor in view (after edits
    /// made through [`edit`](TextField::edit) directly).
    pub fn draw(&mut self, canvas: &mut Canvas, font: &dyn Font, style: &FieldStyle) {
        self.reveal(font);
        let r = self.rect;
        if let Some(bg) = style.background {
            canvas.fill_rect(r, bg);
        }
        if let Some((dark, light)) = style.bevel {
            super::edge(canvas, r, dark, light);
        }

        let area = self.text_area();
        let saved = canvas.clip();
        let clip = saved.intersect(&area);
        canvas.set_clip(clip);
        let lh = font.line_height();
        let y = area.y.saturating_add((area.height.saturating_sub(lh) / 2) as i32);
        let x0 = i64::from(area.x) - i64::from(self.scroll);
        // Pen positions are clamped into i32; anything that far out is clipped anyway.
        let pen = |chars: usize| -> i32 {
            let w: u32 = self.edit.text().chars().take(chars).fold(0, |w, c| w.saturating_add(font.advance(c)));
            (x0 + i64::from(w)).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
        };
        let text = self.edit.text();
        font.draw(canvas, text, pen(0), y, style.text);
        if let (true, Some(sel)) = (self.focused, self.edit.selection()) {
            let (a, b) = (pen(sel.start), pen(sel.end));
            let band = Rect::new(a, y, b.abs_diff(a), lh);
            canvas.fill_rect(band, style.selection);
            canvas.set_clip(clip.intersect(&band));
            font.draw(canvas, text, pen(0), y, style.selected_text);
            canvas.set_clip(clip);
        }
        if let (true, Some(color)) = (self.focused, style.cursor) {
            canvas.fill_rect(Rect::new(pen(self.edit.cursor()), y, 1, lh), color);
        }
        canvas.set_clip(saved);
    }

    /// The character position nearest window x-coordinate `x`.
    #[cfg(feature = "x11")]
    fn hit(&self, x: i32, font: &dyn Font) -> usize {
        let target = i64::from(x) - i64::from(self.text_area().x) + i64::from(self.scroll);
        let mut pen = 0i64;
        for (i, c) in self.edit.text().chars().enumerate() {
            let advance = i64::from(font.advance(c));
            if target < pen + advance / 2 {
                return i;
            }
            pen += advance;
        }
        self.edit.text().chars().count()
    }

    /// Scroll so the cursor (one pixel wide) is inside the text area, and
    /// no further than the text needs.
    fn reveal(&mut self, font: &dyn Font) {
        let width = self.text_area().width;
        let text = self.edit.text();
        let cursor_x = text.chars().take(self.edit.cursor()).fold(0u32, |w, c| w.saturating_add(font.advance(c)));
        let end = font.width(text).saturating_add(1);
        if cursor_x < self.scroll {
            self.scroll = cursor_x;
        }
        if cursor_x.saturating_add(1) > self.scroll.saturating_add(width) {
            self.scroll = cursor_x.saturating_add(1) - width;
        }
        self.scroll = self.scroll.min(end.saturating_sub(width));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::Motion;

    /// A test font: `i` is 2 px wide, `m` 6, everything else 4; 6 px tall.
    /// Each glyph is a solid block in the color it is drawn with.
    struct Blocks;

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

    /// Field at (10, 5), 30x12: text area (13, 8, 24, 6).
    fn field(text: &str) -> TextField {
        let mut f = TextField::new(Rect::new(10, 5, 30, 12));
        f.edit.set_text(text);
        f
    }

    const STYLE: FieldStyle = FieldStyle {
        background: Some(0x111111),
        bevel: Some((0x222222, 0x333333)),
        text: 0xaaaaaa,
        selection: 0xbbbbbb,
        selected_text: 0xcccccc,
        cursor: Some(0xdddddd),
    };

    /// The colors along the text row (y = 8), x = 10..40.
    fn row(c: &Canvas) -> Vec<u32> {
        (10..40).map(|x| c.get(x, 8).unwrap()).collect()
    }

    #[test]
    fn draws_box_text_and_nothing_outside() {
        let mut f = field("ab");
        let mut c = Canvas::new(50, 25);
        c.fill(0x777777);
        f.draw(&mut c, &Blocks, &STYLE);
        let (bg, tx) = (0x111111, 0xaaaaaa);
        // bevel, padding, "ab" (8 px), the rest of the area, padding, bevel.
        let mut want = vec![0x222222, bg, bg];
        want.extend([tx; 8]);
        want.extend([bg; 16]);
        want.extend([bg, bg, 0x333333]);
        assert_eq!(row(&c), want);
        for y in 0..25 {
            for x in 0..50 {
                if !f.rect().contains(x, y) {
                    assert_eq!(c.get(x as u32, y as u32), Some(0x777777), "({x}, {y})");
                }
            }
        }
        assert_eq!(c.clip(), c.bounds(), "the clip is restored");
    }

    #[test]
    fn focus_shows_cursor_and_selection() {
        let mut f = field("abc");
        f.set_focused(true);
        f.edit.set_cursor(1, false);
        f.edit.set_cursor(2, true); // "b" selected, cursor after it
        let mut c = Canvas::new(50, 25);
        f.draw(&mut c, &Blocks, &STYLE);
        let r = row(&c);
        assert_eq!(&r[3..7], &[0xaaaaaa; 4], "a");
        assert_eq!(&r[7..11], &[0xcccccc; 4], "b, selected");
        assert_eq!(r[11], 0xdddddd, "the cursor bar, after b");
        assert_eq!(&r[12..15], &[0xaaaaaa; 3], "the rest of c");
        f.set_focused(false);
        f.draw(&mut c, &Blocks, &STYLE);
        assert_eq!(&row(&c)[3..15], &[0xaaaaaa; 12], "unfocused: no cursor or selection");
    }

    #[test]
    fn skins_can_skip_the_box() {
        let mut f = field("a");
        let mut c = Canvas::new(50, 25);
        c.fill(0x777777);
        f.draw(&mut c, &Blocks, &FieldStyle { background: None, bevel: None, ..STYLE });
        assert_eq!(&row(&c)[..8], &[0x777777, 0x777777, 0x777777, 0xaaaaaa, 0xaaaaaa, 0xaaaaaa, 0xaaaaaa, 0x777777]);
    }

    #[test]
    fn long_text_scrolls_to_keep_the_cursor_visible() {
        // 10 chars x 4 px = 40 px in a 24 px area.
        let mut f = field("0123456789");
        f.set_focused(true);
        let mut c = Canvas::new(50, 25);
        f.draw(&mut c, &Blocks, &STYLE);
        assert_eq!(f.scroll, 17, "cursor at 40 shown at the area's last column");
        assert_eq!(row(&c)[3 + 23], 0xdddddd);
        f.edit.move_cursor(Motion::Home, false);
        f.draw(&mut c, &Blocks, &STYLE);
        assert_eq!(f.scroll, 0);
        // Moving right inside the view does not scroll.
        f.edit.set_cursor(3, false);
        f.draw(&mut c, &Blocks, &STYLE);
        assert_eq!(f.scroll, 0);
        // Deleting text pulls the scroll back so no space is wasted.
        f.edit.move_cursor(Motion::End, false);
        f.draw(&mut c, &Blocks, &STYLE);
        f.edit.set_text("0123");
        f.draw(&mut c, &Blocks, &STYLE);
        assert_eq!(f.scroll, 0);
    }

    #[test]
    fn degenerate_rects_and_fonts_do_not_panic() {
        for rect in [Rect::new(0, 0, 0, 0), Rect::new(0, 0, 5, 1), Rect::new(i32::MAX, i32::MAX, u32::MAX, u32::MAX), Rect::new(i32::MIN, i32::MIN, 7, 7)] {
            let mut f = TextField::new(rect).with_padding(u32::MAX);
            f.edit.set_text("mimi mimi");
            f.set_focused(true);
            f.edit.select_all();
            let mut c = Canvas::new(8, 8);
            f.draw(&mut c, &Blocks, &STYLE);
            f.edit.set_cursor(2, false);
            f.draw(&mut c, &Blocks, &STYLE);
        }
    }

    #[cfg(feature = "x11")]
    mod input {
        use super::*;
        use crate::x11::{Modifiers, MouseButtons, WindowId};

        const W: WindowId = WindowId(1);

        fn key(key: Key, mods: Modifiers) -> Event {
            Event::Key { window: W, key, keysym: 0, keycode: 0, pressed: true, repeat: false, modifiers: mods }
        }
        fn plain(k: Key) -> Event {
            key(k, Modifiers::default())
        }
        fn shift(k: Key) -> Event {
            key(k, Modifiers { shift: true, ..Modifiers::default() })
        }
        fn ctrl(k: Key) -> Event {
            key(k, Modifiers { ctrl: true, ..Modifiers::default() })
        }
        fn button(pressed: bool, x: i32, y: i32, mods: Modifiers) -> Event {
            Event::Button { window: W, button: MouseButton::Left, pressed, x, y, root_x: x, root_y: y, modifiers: mods, time: 0 }
        }
        fn drag(x: i32, left: bool) -> Event {
            Event::Motion {
                window: W,
                x,
                y: 10,
                root_x: x,
                root_y: 10,
                buttons: MouseButtons { left, ..MouseButtons::default() },
                modifiers: Modifiers::default(),
            }
        }
        fn typed(f: &mut TextField, s: &str) {
            for c in s.chars() {
                assert_eq!(f.handle(&plain(Key::Character(c)), &Blocks), Response::Changed);
            }
        }

        #[test]
        fn keys_are_ignored_until_focused() {
            let mut f = field("");
            assert_eq!(f.handle(&plain(Key::Character('a')), &Blocks), Response::Ignored);
            f.set_focused(true);
            typed(&mut f, "hi there");
            assert_eq!(f.edit.text(), "hi there");
        }

        #[test]
        fn editing_keys() {
            let mut f = field("hello world");
            f.set_focused(true);
            assert_eq!(f.handle(&ctrl(Key::Backspace), &Blocks), Response::Changed);
            assert_eq!(f.edit.text(), "hello ");
            f.handle(&ctrl(Key::Left), &Blocks);
            assert_eq!(f.edit.cursor(), 0);
            assert_eq!(f.handle(&plain(Key::Backspace), &Blocks), Response::Redraw, "nothing to delete, but the key is used");
            f.handle(&shift(Key::End), &Blocks);
            assert_eq!(f.edit.selected_text(), "hello ");
            typed(&mut f, "bye");
            f.handle(&plain(Key::Home), &Blocks);
            f.handle(&plain(Key::Delete), &Blocks);
            f.handle(&ctrl(Key::Delete), &Blocks);
            assert_eq!(f.edit.text(), "");
            typed(&mut f, "x y");
            assert_eq!(f.handle(&ctrl(Key::Character('a')), &Blocks), Response::Redraw);
            assert_eq!(f.edit.selected_text(), "x y");
            assert_eq!(f.handle(&ctrl(Key::Character('A')), &Blocks), Response::Redraw, "with Shift held too");
            assert_eq!(f.handle(&ctrl(Key::Character('z')), &Blocks), Response::Ignored, "other shortcuts are the app's");
            let alt = Modifiers { alt: true, ..Modifiers::default() };
            assert_eq!(f.handle(&key(Key::Character('f'), alt), &Blocks), Response::Ignored);
            assert_eq!(f.handle(&shift(Key::Character('Q')), &Blocks), Response::Changed);
            assert_eq!(f.edit.text(), "Q");
        }

        #[test]
        fn enter_escape_and_unknown_keys() {
            let mut f = field("x");
            f.set_focused(true);
            assert_eq!(f.handle(&plain(Key::Enter), &Blocks), Response::Submit);
            assert_eq!(f.handle(&plain(Key::Escape), &Blocks), Response::Cancel);
            assert_eq!(f.handle(&plain(Key::Tab), &Blocks), Response::Ignored);
            assert_eq!(f.handle(&plain(Key::F(1)), &Blocks), Response::Ignored);
            let release =
                Event::Key { window: W, key: Key::Character('a'), keysym: 0, keycode: 0, pressed: false, repeat: false, modifiers: Modifiers::default() };
            assert_eq!(f.handle(&release, &Blocks), Response::Ignored);
            assert!(f.is_focused(), "Enter and Escape leave focus to the caller");
        }

        #[test]
        fn clicks_focus_place_the_cursor_and_unfocus() {
            // Text area starts at x = 13. "mix": m 13..19, i 19..21, x 21..25.
            let mut f = field("mix");
            let none = Modifiers::default();
            assert_eq!(f.handle(&button(true, 15, 10, none), &Blocks), Response::Redraw);
            assert!(f.is_focused());
            assert_eq!(f.edit.cursor(), 0, "left half of m");
            for (x, want) in [(16, 1), (19, 1), (20, 2), (22, 2), (23, 3), (39, 3), (11, 0)] {
                f.handle(&button(true, x, 10, none), &Blocks);
                assert_eq!(f.edit.cursor(), want, "click at x = {x}");
            }
            f.handle(&button(true, 16, 10, none), &Blocks);
            f.handle(&button(true, 23, 10, Modifiers { shift: true, ..none }), &Blocks);
            assert_eq!(f.edit.selected_text(), "ix", "shift-click extends");
            assert_eq!(f.handle(&button(true, 45, 10, none), &Blocks), Response::Redraw, "click outside unfocuses");
            assert!(!f.is_focused());
            assert_eq!(f.handle(&button(true, 45, 10, none), &Blocks), Response::Ignored);
        }

        #[test]
        fn drag_selects_until_release() {
            let mut f = field("mix");
            f.handle(&button(true, 14, 10, Modifiers::default()), &Blocks);
            assert_eq!(f.handle(&drag(22, true), &Blocks), Response::Redraw);
            assert_eq!(f.edit.selected_text(), "mi");
            assert_eq!(f.handle(&drag(22, true), &Blocks), Response::Ignored, "no change, no redraw");
            f.handle(&button(false, 22, 10, Modifiers::default()), &Blocks);
            assert_eq!(f.handle(&drag(30, true), &Blocks), Response::Ignored, "released: motion is not a drag");
            assert_eq!(f.edit.selected_text(), "mi");
            // A release we never saw: motion without the button ends the drag.
            f.handle(&button(true, 14, 10, Modifiers::default()), &Blocks);
            assert_eq!(f.handle(&drag(30, false), &Blocks), Response::Ignored);
            assert_eq!(f.handle(&drag(30, true), &Blocks), Response::Ignored);
            // A press outside never starts a drag.
            f.handle(&button(true, 45, 10, Modifiers::default()), &Blocks);
            assert_eq!(f.handle(&drag(20, true), &Blocks), Response::Ignored);
        }

        #[test]
        fn clicks_account_for_scrolling() {
            let mut f = field("0123456789"); // 40 px wide, scrolled by 17 once focused at the end
            f.set_focused(true);
            f.handle(&plain(Key::End), &Blocks);
            assert_eq!(f.scroll, 17);
            // Window x 13 is text x 17: inside '4' (16..20), left half.
            f.handle(&button(true, 13, 10, Modifiers::default()), &Blocks);
            assert_eq!(f.edit.cursor(), 4);
        }
    }
}
