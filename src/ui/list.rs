//! [`List`].

#[cfg(feature = "x11")]
use super::{click_focus, Response, DOUBLE_CLICK_MS};
use super::{focus_ring, Style};
use crate::canvas::Canvas;
use crate::font::Font;
use crate::pixel::Rect;
#[cfg(feature = "x11")]
use crate::x11::{Event, Key, MouseButton};

/// A scrolling list of text rows with one selected — a playlist, a file
/// picker.
///
/// A click selects a row ([`Response::Changed`]); a double click, or
/// Enter when focused, activates it ([`Response::Submit`]). When focused,
/// Up/Down, Page Up/Down, Home and End move the selection, scrolling it
/// into view. The wheel scrolls three rows a notch. For a scroll bar, pair
/// it with a vertical [`Slider`](super::Slider) over
/// [`max_top`](List::max_top), keeping the two in step with
/// [`top`](List::top) and [`set_top`](List::set_top).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    /// The rows. Edit them freely: the selection and scroll position are
    /// kept in range when used.
    pub items: Vec<String>,
    rect: Rect,
    row_height: u32,
    selected: Option<usize>,
    /// The first visible row.
    top: usize,
    /// The last press on a row, and its server time, for double clicks.
    last_click: Option<(usize, u32)>,
    focused: bool,
    enabled: bool,
}

impl List {
    /// An empty list at `rect` with rows `row_height` pixels tall.
    pub fn new(rect: Rect, row_height: u32) -> List {
        List { items: Vec::new(), rect, row_height: row_height.max(1), selected: None, top: 0, last_click: None, focused: false, enabled: true }
    }

    /// Where the list is.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Move or resize the list.
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// Row height in pixels.
    pub fn row_height(&self) -> u32 {
        self.row_height
    }

    /// The selected row, if any.
    pub fn selected(&self) -> Option<usize> {
        self.selected.filter(|&i| i < self.items.len())
    }

    /// Select a row (or none), scrolling it into view.
    pub fn select(&mut self, row: Option<usize>) {
        self.selected = row.filter(|&i| i < self.items.len());
        if let Some(i) = self.selected {
            let visible = self.visible_rows().max(1);
            self.top = self.top().min(i).max((i + 1).saturating_sub(visible));
        }
    }

    /// The first row shown.
    pub fn top(&self) -> usize {
        self.top.min(self.max_top())
    }

    /// Scroll so `row` is the first shown (clamped).
    pub fn set_top(&mut self, row: usize) {
        self.top = row.min(self.max_top());
    }

    /// How many whole rows fit.
    pub fn visible_rows(&self) -> usize {
        (self.rect.height / self.row_height) as usize
    }

    /// The largest useful [`top`](List::top): the last row at the bottom.
    pub fn max_top(&self) -> usize {
        self.items.len().saturating_sub(self.visible_rows())
    }

    /// The row under window point `(x, y)`, if any.
    pub fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.rect.contains(x, y) {
            return None;
        }
        let row = self.top() + ((i64::from(y) - i64::from(self.rect.y)) as u64 / u64::from(self.row_height)) as usize;
        (row < self.items.len()).then_some(row)
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

    /// Enable or disable it; a disabled list ignores every event.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[cfg(feature = "x11")]
    fn select_response(&mut self, row: usize) -> Response {
        let before = self.selected();
        self.select(Some(row.min(self.items.len().saturating_sub(1))));
        if self.selected() != before {
            Response::Changed
        } else {
            Response::Redraw
        }
    }

    /// React to an event for this list's window.
    #[cfg(feature = "x11")]
    pub fn handle(&mut self, event: &Event) -> Response {
        if !self.enabled {
            return Response::Ignored;
        }
        match *event {
            Event::Button { button: MouseButton::Left, pressed: true, x, y, time, .. } => {
                let focus = click_focus(&mut self.focused, self.rect, x, y);
                let Some(row) = self.row_at(x, y) else {
                    return if focus { Response::Redraw } else { Response::Ignored };
                };
                let double = matches!(self.last_click, Some((r, t)) if r == row && time.wrapping_sub(t) <= DOUBLE_CLICK_MS);
                // A third click starts over rather than counting as another double.
                self.last_click = if double { None } else { Some((row, time)) };
                let response = self.select_response(row);
                if double {
                    Response::Submit
                } else {
                    response
                }
            }
            Event::Scroll { dy, x, y, .. } if self.rect.contains(x, y) => {
                let before = self.top();
                let to = before as i64 + i64::from(dy) * 3;
                self.set_top(to.max(0) as usize);
                if self.top() != before {
                    Response::Redraw
                } else {
                    Response::Ignored
                }
            }
            Event::Key { key, pressed: true, .. } if self.focused && !self.items.is_empty() => {
                let page = self.visible_rows().saturating_sub(1).max(1);
                let last = self.items.len() - 1;
                let at = self.selected();
                match key {
                    Key::Down => self.select_response(at.map_or(0, |i| i + 1)),
                    Key::Up => self.select_response(at.map_or(last, |i| i.saturating_sub(1))),
                    Key::PageDown => self.select_response(at.map_or(0, |i| i + page)),
                    Key::PageUp => self.select_response(at.map_or(0, |i| i.saturating_sub(page))),
                    Key::Home => self.select_response(0),
                    Key::End => self.select_response(last),
                    Key::Enter if at.is_some() => Response::Submit,
                    _ => Response::Ignored,
                }
            }
            _ => Response::Ignored,
        }
    }

    /// Draw the default look: rows in `font` on the background color, the
    /// selected row in the accent color.
    pub fn draw(&self, canvas: &mut Canvas, font: &dyn Font, style: &Style) {
        let r = self.rect;
        canvas.fill_rect(r, style.background);
        let saved = canvas.clip();
        canvas.set_clip(saved.intersect(&r));
        let pad = (i64::from(self.row_height) - i64::from(font.line_height())) / 2;
        let clamp = |v: i64| v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        // Every row that is at least partly visible.
        let shown = (r.height as usize).div_ceil(self.row_height as usize);
        for (n, item) in self.items.iter().enumerate().skip(self.top()).take(shown) {
            let y = i64::from(r.y) + (n - self.top()) as i64 * i64::from(self.row_height);
            let mut color = style.text;
            if Some(n) == self.selected() {
                canvas.fill_rect(Rect::new(r.x, clamp(y), r.width, self.row_height), style.accent);
                color = style.accent_text;
            }
            if !self.enabled {
                color = style.shadow;
            }
            font.draw(canvas, item, r.x.saturating_add(2), clamp(y + pad), color);
        }
        canvas.set_clip(saved);
        if self.focused {
            focus_ring(canvas, r, style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tests::{Blocks, STYLE};

    /// Rows 10 px tall in a 35 px list: 3 whole rows, a fourth partly.
    fn list(n: usize) -> List {
        let mut l = List::new(Rect::new(0, 0, 40, 35), 10);
        l.items = (0..n).map(|i| format!("item {i}")).collect();
        l
    }

    #[test]
    fn geometry() {
        let mut l = list(10);
        assert_eq!((l.visible_rows(), l.max_top()), (3, 7));
        assert_eq!(
            (l.row_at(5, 0), l.row_at(5, 9), l.row_at(5, 10), l.row_at(5, 34), l.row_at(5, 35), l.row_at(40, 5)),
            (Some(0), Some(0), Some(1), Some(3), None, None)
        );
        l.set_top(99);
        assert_eq!(l.top(), 7);
        assert_eq!(l.row_at(5, 25), Some(9));
        assert_eq!(list(2).row_at(5, 25), None, "below the last row");
        assert_eq!(List::new(Rect::new(0, 0, 10, 10), 0).row_height(), 1);
    }

    #[test]
    fn selecting_scrolls_into_view_and_edits_stay_in_range() {
        let mut l = list(10);
        l.select(Some(5));
        assert_eq!((l.selected(), l.top()), (Some(5), 3), "scrolled down: 5 is the last whole row");
        l.select(Some(1));
        assert_eq!(l.top(), 1, "scrolled up");
        l.select(Some(2));
        assert_eq!(l.top(), 1, "already visible");
        l.select(Some(10));
        assert_eq!(l.selected(), None);
        l.select(Some(9));
        l.items.truncate(4);
        assert_eq!((l.selected(), l.top()), (None, 1), "shrinking the items clamps both");
    }

    #[test]
    fn draws_visible_rows_with_the_selection() {
        let mut l = list(10);
        l.set_top(1);
        l.select(Some(2));
        let mut c = Canvas::new(40, 40);
        c.fill(0x777777);
        l.draw(&mut c, &Blocks, &STYLE);
        // Row n's text is at x 2.., y 10 (n - top) + 2.. (6 px glyphs in 10 px rows).
        assert_eq!((c.get(2, 2), c.get(1, 2), c.get(2, 1)), (Some(STYLE.text), Some(STYLE.background), Some(STYLE.background)), "item 1");
        assert_eq!((c.get(2, 12), c.get(1, 10), c.get(39, 19)), (Some(STYLE.accent_text), Some(STYLE.accent), Some(STYLE.accent)), "item 2, selected");
        assert_eq!(c.get(2, 32), Some(STYLE.text), "item 4, partly shown");
        assert_eq!(c.get(2, 36), Some(0x777777), "nothing outside");
        assert_eq!(c.clip(), c.bounds());
    }

    #[cfg(feature = "x11")]
    mod input {
        use super::*;
        use crate::ui::tests::{click, key, scroll};

        #[test]
        fn click_selects_and_double_click_submits() {
            let mut l = list(10);
            assert_eq!(l.handle(&click(true, 5, 15, 1000)), Response::Changed);
            assert_eq!((l.selected(), l.is_focused()), (Some(1), true));
            assert_eq!(l.handle(&click(false, 5, 15, 1050)), Response::Ignored);
            assert_eq!(l.handle(&click(true, 5, 15, 1000 + DOUBLE_CLICK_MS)), Response::Submit);
            assert_eq!(l.handle(&click(true, 5, 15, 1001 + DOUBLE_CLICK_MS)), Response::Redraw, "a third click is a new first");
            assert_eq!(l.handle(&click(true, 5, 25, 1002 + DOUBLE_CLICK_MS)), Response::Changed, "another row");
            assert_eq!(l.handle(&click(true, 5, 15, 1003 + DOUBLE_CLICK_MS)), Response::Changed, "...is not a double on the first");
            assert_eq!(l.handle(&click(true, 5, 15, 2000 + 2 * DOUBLE_CLICK_MS)), Response::Redraw, "too slow");
            // Server time wraps around.
            l.handle(&click(true, 5, 5, u32::MAX - 10));
            assert_eq!(l.handle(&click(true, 5, 5, 20)), Response::Submit);
            // Below the last row and outside: no selection change.
            let mut short = list(2);
            assert_eq!(short.handle(&click(true, 5, 30, 0)), Response::Redraw, "focus only");
            assert_eq!(short.selected(), None);
            assert_eq!(short.handle(&click(true, 50, 5, 0)), Response::Redraw, "unfocused");
            assert_eq!(short.handle(&click(true, 50, 5, 0)), Response::Ignored);
        }

        #[test]
        fn keys_move_the_selection() {
            let mut l = list(10);
            assert_eq!(l.handle(&key(Key::Down, false)), Response::Ignored, "unfocused");
            l.set_focused(true);
            assert_eq!(l.handle(&key(Key::Enter, false)), Response::Ignored, "nothing selected");
            let mut seen = vec![];
            for k in [
                Key::Down,
                Key::Down,
                Key::PageDown,
                Key::PageDown,
                Key::PageDown,
                Key::PageDown,
                Key::Up,
                Key::PageUp,
                Key::Home,
                Key::Up,
                Key::End,
                Key::Down,
            ] {
                l.handle(&key(k, false));
                seen.push(l.selected().unwrap());
            }
            assert_eq!(seen, [0, 1, 3, 5, 7, 9, 8, 6, 0, 0, 9, 9]);
            assert_eq!(l.top(), 7, "the end is in view");
            assert_eq!(l.handle(&key(Key::Enter, false)), Response::Submit);
            let mut l = list(4);
            l.set_focused(true);
            l.handle(&key(Key::Up, false));
            assert_eq!(l.selected(), Some(3), "Up with no selection starts at the bottom");
            let mut empty = list(0);
            empty.set_focused(true);
            assert_eq!(empty.handle(&key(Key::Down, false)), Response::Ignored);
        }

        #[test]
        fn wheel_scrolls_three_rows() {
            let mut l = list(10);
            assert_eq!(l.handle(&scroll(0, 1, 5, 5)), Response::Redraw);
            assert_eq!(l.top(), 3);
            l.handle(&scroll(0, 5, 5, 5));
            assert_eq!(l.top(), 7);
            assert_eq!(l.handle(&scroll(0, 1, 5, 5)), Response::Ignored, "at the end");
            l.handle(&scroll(0, -9, 5, 5));
            assert_eq!(l.top(), 0);
            assert_eq!(l.handle(&scroll(0, -1, 50, 5)), Response::Ignored, "not over the list");
        }

        #[test]
        fn disabled_ignores_everything() {
            let mut l = list(10);
            l.set_focused(true);
            l.set_enabled(false);
            for e in [click(true, 5, 5, 0), scroll(0, 1, 5, 5), key(Key::Down, false)] {
                assert_eq!(l.handle(&e), Response::Ignored);
            }
            assert_eq!(l.selected(), None);
        }
    }
}
