//! [`TextEdit`]: a single-line edit buffer with a cursor and a selection.

use std::ops::Range;

/// Where a cursor movement (or a deletion) goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Motion {
    /// One character left.
    Left,
    /// One character right.
    Right,
    /// To the start of the word on the left (Ctrl+Left).
    WordLeft,
    /// To the end of the word on the right (Ctrl+Right).
    WordRight,
    /// To the start of the line.
    Home,
    /// To the end of the line.
    End,
}

/// A single line of editable text: the buffer, a cursor, and a selection
/// running from an anchor to the cursor. No drawing and no input
/// decoding — [`TextField`](super::TextField) adds those — so it works
/// with any front end.
///
/// Positions are counted in `char`s. Control characters (newlines, tabs,
/// ...) are never stored, and an optional limit caps the length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    /// Byte offsets, always on char boundaries.
    cursor: usize,
    anchor: usize,
    max_chars: usize,
}

impl Default for TextEdit {
    fn default() -> TextEdit {
        TextEdit::new()
    }
}

/// Letters, digits and `_` — what word motions skip over. Exactly
/// Unicode's alphanumerics through Latin-1 (everything Terminus draws);
/// past it, everything but the General and CJK punctuation blocks counts,
/// which avoids carrying Unicode's ~3 KB of letter tables.
fn is_word(c: char) -> bool {
    match c {
        '0'..='9' | 'A'..='Z' | 'a'..='z' | '_' => true,
        '\u{aa}' | '\u{b2}' | '\u{b3}' | '\u{b5}' | '\u{b9}' | '\u{ba}' | '\u{bc}'..='\u{be}' => true,
        '\u{d7}' | '\u{f7}' => false,
        '\u{c0}'..='\u{ff}' => true,
        '\u{2000}'..='\u{206f}' | '\u{3000}'..='\u{303f}' => false,
        _ => u32::from(c) > 0xff,
    }
}

impl TextEdit {
    /// An empty buffer with no length limit.
    pub fn new() -> TextEdit {
        TextEdit { text: String::new(), cursor: 0, anchor: 0, max_chars: usize::MAX }
    }

    /// Limit the buffer to `max` characters (truncating what is there).
    /// Insertions past the limit are cut short.
    pub fn with_max_chars(mut self, max: usize) -> TextEdit {
        self.max_chars = max;
        let text = std::mem::take(&mut self.text);
        self.set_text(&text);
        self
    }

    /// The length limit, in characters.
    pub fn max_chars(&self) -> usize {
        self.max_chars
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replace the text (control characters dropped, cut to the limit);
    /// the cursor goes to the end, with nothing selected.
    pub fn set_text(&mut self, text: &str) {
        self.text = text.chars().filter(|c| !c.is_control()).take(self.max_chars).collect();
        self.cursor = self.text.len();
        self.anchor = self.cursor;
    }

    /// The cursor position, in characters from the start.
    pub fn cursor(&self) -> usize {
        self.chars_before(self.cursor)
    }

    /// The selected characters, if any, as a range of character positions.
    pub fn selection(&self) -> Option<Range<usize>> {
        let r = self.selected_bytes();
        (!r.is_empty()).then(|| self.chars_before(r.start)..self.chars_before(r.end))
    }

    /// The selected text (empty when nothing is selected).
    pub fn selected_text(&self) -> &str {
        &self.text[self.selected_bytes()]
    }

    /// Put the cursor at character `position` (clamped to the end). With
    /// `extend`, the selection grows or shrinks to it; without, the
    /// selection is cleared.
    pub fn set_cursor(&mut self, position: usize, extend: bool) {
        self.cursor = self.text.char_indices().nth(position).map_or(self.text.len(), |(i, _)| i);
        if !extend {
            self.anchor = self.cursor;
        }
    }

    /// Select everything, cursor at the end.
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
    }

    /// Move the cursor. With `extend`, the selection follows; without, it
    /// is cleared — and `Left`/`Right` from a selection land on its
    /// near edge rather than moving. Returns whether anything changed.
    pub fn move_cursor(&mut self, motion: Motion, extend: bool) -> bool {
        let before = (self.cursor, self.anchor);
        let sel = self.selected_bytes();
        self.cursor = match motion {
            Motion::Left if !extend && !sel.is_empty() => sel.start,
            Motion::Right if !extend && !sel.is_empty() => sel.end,
            _ => self.target(motion),
        };
        if !extend {
            self.anchor = self.cursor;
        }
        before != (self.cursor, self.anchor)
    }

    /// Type `s` at the cursor, replacing the selection. Control characters
    /// are dropped and the result is cut to the length limit. Returns
    /// whether the text changed.
    pub fn insert(&mut self, s: &str) -> bool {
        let sel = self.selected_bytes();
        let room = self.max_chars.saturating_sub(self.text.chars().count() - self.text[sel.clone()].chars().count());
        let s: String = s.chars().filter(|c| !c.is_control()).take(room).collect();
        if s.is_empty() {
            return false;
        }
        self.text.replace_range(sel.clone(), &s);
        self.cursor = sel.start + s.len();
        self.anchor = self.cursor;
        true
    }

    /// Delete the selection, or else from the cursor to where `motion`
    /// would take it (Backspace is `Left`, Delete is `Right`,
    /// Ctrl+Backspace `WordLeft`). Returns whether the text changed.
    pub fn delete(&mut self, motion: Motion) -> bool {
        let mut range = self.selected_bytes();
        if range.is_empty() {
            let to = self.target(motion);
            range = self.cursor.min(to)..self.cursor.max(to);
        }
        if range.is_empty() {
            return false;
        }
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = range.start;
        true
    }

    fn selected_bytes(&self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    fn chars_before(&self, byte: usize) -> usize {
        self.text[..byte].chars().count()
    }

    /// The byte offset `motion` leads to from the cursor.
    fn target(&self, motion: Motion) -> usize {
        let (before, after) = self.text.split_at(self.cursor);
        // Byte length of the run at the end of `before` (or start of
        // `after`) whose chars satisfy `f`.
        let back = |s: &str, f: fn(char) -> bool| s.chars().rev().take_while(|&c| f(c)).map(char::len_utf8).sum::<usize>();
        let fwd = |s: &str, f: fn(char) -> bool| s.chars().take_while(|&c| f(c)).map(char::len_utf8).sum::<usize>();
        match motion {
            Motion::Left => self.cursor - before.chars().next_back().map_or(0, char::len_utf8),
            Motion::Right => self.cursor + after.chars().next().map_or(0, char::len_utf8),
            Motion::Home => 0,
            Motion::End => self.text.len(),
            Motion::WordLeft => {
                let gap = back(before, |c| !is_word(c));
                self.cursor - gap - back(&before[..before.len() - gap], is_word)
            }
            Motion::WordRight => {
                let gap = fwd(after, |c| !is_word(c));
                self.cursor + gap + fwd(&after[gap..], is_word)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str, cursor: usize) -> TextEdit {
        let mut e = TextEdit::new();
        e.set_text(text);
        e.set_cursor(cursor, false);
        e
    }

    #[test]
    fn insert_at_start_middle_and_end() {
        let mut e = TextEdit::new();
        assert!(e.insert("abc"));
        e.set_cursor(0, false);
        e.insert("X");
        e.set_cursor(2, false);
        e.insert("Y");
        e.move_cursor(Motion::End, false);
        e.insert("Z");
        assert_eq!((e.text(), e.cursor()), ("XaYbcZ", 6));
    }

    #[test]
    fn control_characters_are_never_stored() {
        let mut e = TextEdit::new();
        assert!(!e.insert("\n\t\u{7}"));
        assert!(e.insert("a\nb"));
        e.set_text("x\ty\r\n");
        assert_eq!(e.text(), "xy");
    }

    #[test]
    fn backspace_and_delete_at_the_edges() {
        let mut e = at("abc", 0);
        assert!(!e.delete(Motion::Left), "backspace at the start");
        assert!(e.delete(Motion::Right));
        assert_eq!((e.text(), e.cursor()), ("bc", 0));
        e.move_cursor(Motion::End, false);
        assert!(!e.delete(Motion::Right), "delete at the end");
        assert!(e.delete(Motion::Left));
        assert_eq!((e.text(), e.cursor()), ("b", 1));
        let mut e = TextEdit::new();
        for _ in 0..5 {
            assert!(!e.delete(Motion::Left));
        }
    }

    #[test]
    fn moves_clamp() {
        let mut e = at("ab", 1);
        assert!(e.move_cursor(Motion::Left, false));
        assert!(!e.move_cursor(Motion::Left, false));
        assert!(!e.move_cursor(Motion::Home, false));
        e.move_cursor(Motion::End, false);
        assert!(!e.move_cursor(Motion::Right, false));
        assert_eq!(e.cursor(), 2);
        e.set_cursor(1000, false);
        assert_eq!(e.cursor(), 2);
    }

    #[test]
    fn shift_moves_extend_and_plain_moves_collapse() {
        let mut e = at("hello world", 2);
        e.move_cursor(Motion::Right, true);
        e.move_cursor(Motion::Right, true);
        assert_eq!((e.selection(), e.selected_text()), (Some(2..4), "ll"));
        // Back across the anchor: the selection flips sides.
        for _ in 0..3 {
            e.move_cursor(Motion::Left, true);
        }
        assert_eq!((e.selection(), e.cursor()), (Some(1..2), 1));
        // Right without shift lands on the selection's right edge.
        assert!(e.move_cursor(Motion::Right, false));
        assert_eq!((e.selection(), e.cursor()), (None, 2));
        e.set_cursor(6, true);
        assert!(e.move_cursor(Motion::Left, false));
        assert_eq!((e.selection(), e.cursor()), (None, 2), "Left lands on the left edge");
        e.select_all();
        assert_eq!((e.selected_text(), e.cursor()), ("hello world", 11));
        e.move_cursor(Motion::Home, false);
        assert_eq!((e.selection(), e.cursor()), (None, 0));
    }

    #[test]
    fn typing_and_deleting_replace_the_selection() {
        let mut e = at("hello world", 0);
        e.set_cursor(5, true);
        assert!(e.insert("bye"));
        assert_eq!((e.text(), e.cursor(), e.selection()), ("bye world", 3, None));
        e.set_cursor(9, true);
        assert!(e.delete(Motion::WordLeft), "a selection is deleted whatever the motion");
        assert_eq!((e.text(), e.cursor()), ("bye", 3));
    }

    #[test]
    fn word_motions() {
        let s = "  foo_bar, baz--qux  ";
        let mut e = at(s, 0);
        let mut stops = vec![];
        while e.move_cursor(Motion::WordRight, false) {
            stops.push(e.cursor());
        }
        assert_eq!(stops, [9, 14, 19, 21]);
        let mut stops = vec![];
        while e.move_cursor(Motion::WordLeft, false) {
            stops.push(e.cursor());
        }
        assert_eq!(stops, [16, 11, 2, 0]);
    }

    #[test]
    fn word_chars_match_unicode_through_latin1() {
        for c in (0..=0xffu8).map(char::from) {
            assert_eq!(is_word(c), c.is_alphanumeric() || c == '_', "{c:?}");
        }
        for (c, word) in [('世', true), ('ж', true), ('—', false), ('…', false), ('、', false), ('€', true)] {
            assert_eq!(is_word(c), word, "{c:?}");
        }
    }

    #[test]
    fn word_deletes() {
        let mut e = at("open the pod bay", 12);
        assert!(e.delete(Motion::WordLeft));
        assert_eq!((e.text(), e.cursor()), ("open the  bay", 9));
        assert!(e.delete(Motion::WordRight));
        assert_eq!((e.text(), e.cursor()), ("open the ", 9));
    }

    #[test]
    fn max_chars_cuts_insertions_but_lets_a_selection_be_replaced() {
        let mut e = TextEdit::new().with_max_chars(5);
        assert!(e.insert("abcdefgh"));
        assert_eq!(e.text(), "abcde");
        assert!(!e.insert("x"), "full");
        e.set_cursor(1, false);
        e.set_cursor(3, true);
        assert!(e.insert("XYZW"));
        assert_eq!((e.text(), e.cursor()), ("aXYde", 3));
        assert!(!TextEdit::new().with_max_chars(0).insert("a"));
        let mut e = TextEdit::new();
        e.set_text("abcdef");
        assert_eq!(e.with_max_chars(2).text(), "ab");
    }

    #[test]
    fn multibyte_text_never_splits_a_char() {
        let mut e = TextEdit::new();
        e.insert("héllo€世界 x");
        e.move_cursor(Motion::Home, false);
        e.delete(Motion::Right);
        e.move_cursor(Motion::WordRight, false);
        assert_eq!(e.cursor(), 7, "'éllo€世界' is one word");
        e.move_cursor(Motion::Left, true);
        assert_eq!(e.selected_text(), "界");
        e.delete(Motion::Left);
        assert_eq!(e.text(), "éllo€世 x");
    }

    /// The same buffer, kept as a `Vec<char>` with char indices — written
    /// separately and naively, as an oracle for random edit sequences.
    #[derive(Clone)]
    struct Model {
        text: Vec<char>,
        cursor: usize,
        anchor: usize,
        max: usize,
    }

    impl Model {
        fn sel(&self) -> (usize, usize) {
            (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
        }

        fn target(&self, m: Motion) -> usize {
            let t = &self.text;
            let mut i = self.cursor;
            match m {
                Motion::Left => i.saturating_sub(1),
                Motion::Right => (i + 1).min(t.len()),
                Motion::Home => 0,
                Motion::End => t.len(),
                Motion::WordLeft => {
                    while i > 0 && !is_word(t[i - 1]) {
                        i -= 1;
                    }
                    while i > 0 && is_word(t[i - 1]) {
                        i -= 1;
                    }
                    i
                }
                Motion::WordRight => {
                    while i < t.len() && !is_word(t[i]) {
                        i += 1;
                    }
                    while i < t.len() && is_word(t[i]) {
                        i += 1;
                    }
                    i
                }
            }
        }

        fn mv(&mut self, m: Motion, extend: bool) {
            let (s, e) = self.sel();
            self.cursor = match m {
                Motion::Left if !extend && s != e => s,
                Motion::Right if !extend && s != e => e,
                _ => self.target(m),
            };
            if !extend {
                self.anchor = self.cursor;
            }
        }

        fn insert(&mut self, s: &str) {
            let (a, b) = self.sel();
            let room = self.max.saturating_sub(self.text.len() - (b - a));
            let new: Vec<char> = s.chars().filter(|c| !c.is_control()).take(room).collect();
            if new.is_empty() {
                return;
            }
            self.text.splice(a..b, new.iter().copied());
            self.cursor = a + new.len();
            self.anchor = self.cursor;
        }

        fn delete(&mut self, m: Motion) {
            let (mut a, mut b) = self.sel();
            if a == b {
                let t = self.target(m);
                (a, b) = (self.cursor.min(t), self.cursor.max(t));
            }
            self.text.drain(a..b);
            self.cursor = a;
            self.anchor = a;
        }
    }

    #[test]
    fn random_edits_match_the_model() {
        let motions = [Motion::Left, Motion::Right, Motion::WordLeft, Motion::WordRight, Motion::Home, Motion::End];
        let inserts = ["a", "é", "世界", " ", "--", "x_y", "\n", "", "ab cd"];
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        for max in [usize::MAX, 7] {
            let mut e = TextEdit::new().with_max_chars(max);
            let mut m = Model { text: vec![], cursor: 0, anchor: 0, max };
            for step in 0..20_000 {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                let pick = (s >> 33) as usize;
                let motion = motions[pick / 7 % motions.len()];
                match pick % 7 {
                    0 | 1 => {
                        let ins = inserts[pick / 11 % inserts.len()];
                        e.insert(ins);
                        m.insert(ins);
                    }
                    2 => {
                        e.delete(motion);
                        m.delete(motion);
                    }
                    3 | 4 => {
                        let extend = pick / 13 % 2 == 0;
                        e.move_cursor(motion, extend);
                        m.mv(motion, extend);
                    }
                    5 => {
                        let (p, extend) = (pick / 17 % 12, pick / 19 % 2 == 0);
                        e.set_cursor(p, extend);
                        m.cursor = p.min(m.text.len());
                        if !extend {
                            m.anchor = m.cursor;
                        }
                    }
                    _ => {
                        if pick / 23 % 8 == 0 {
                            e.select_all();
                            (m.anchor, m.cursor) = (0, m.text.len());
                        }
                    }
                }
                let text: String = m.text.iter().collect();
                let (a, b) = m.sel();
                assert_eq!((e.text(), e.cursor()), (text.as_str(), m.cursor), "step {step}");
                assert_eq!(e.selection(), (a != b).then_some(a..b), "step {step}");
            }
        }
    }
}
