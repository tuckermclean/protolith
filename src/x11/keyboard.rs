//! Keyboard: keycode → keysym → [`Key`], following the core protocol's
//! rules for Shift, Lock (Caps Lock) and Num Lock (*X Window System
//! Protocol*, §5 "Keyboards").
//!
//! Only keysym group 1 is consulted (no `Mode_switch`/ISO group shifting),
//! and characters are produced for Latin-1, explicit-Unicode keysyms and
//! the keypad. That covers US/European layouts; anything else arrives as
//! [`Key::Unknown`] with its keysym, so nothing is silently lost.

use super::proto::event::state;
use super::proto::Keysym;

/// A decoded key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Key {
    /// A key that produces text, with Shift/Caps Lock already applied
    /// (`'a'` vs `'A'`, `'1'` vs `'!'`). Space is `Character(' ')`.
    Character(char),
    /// Return / keypad Enter.
    Enter,
    /// Tab (also Shift+Tab's `ISO_Left_Tab`).
    Tab,
    /// Backspace.
    Backspace,
    /// Escape.
    Escape,
    /// Delete.
    Delete,
    /// Insert.
    Insert,
    /// Home.
    Home,
    /// End.
    End,
    /// Page Up.
    PageUp,
    /// Page Down.
    PageDown,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Function key `F1` = `F(1)` through `F(35)`.
    F(u8),
    /// Either Shift.
    Shift,
    /// Either Control.
    Control,
    /// Either Alt (or Meta).
    Alt,
    /// Either Super ("Windows" key).
    Super,
    /// Caps Lock.
    CapsLock,
    /// Num Lock.
    NumLock,
    /// Scroll Lock.
    ScrollLock,
    /// Pause / Break.
    Pause,
    /// Print Screen.
    PrintScreen,
    /// The context-menu key.
    Menu,
    /// Media: volume up.
    VolumeUp,
    /// Media: volume down.
    VolumeDown,
    /// Media: mute.
    Mute,
    /// Media: play (or play/pause on most keyboards).
    MediaPlay,
    /// Media: pause.
    MediaPause,
    /// Media: stop.
    MediaStop,
    /// Media: next track.
    MediaNext,
    /// Media: previous track.
    MediaPrevious,
    /// Any other keysym.
    Unknown(Keysym),
}

/// Keysym values used below (`keysymdef.h`, `XF86keysym.h`).
pub mod keysym {
    #![allow(missing_docs)]
    use super::Keysym;
    pub const NO_SYMBOL: Keysym = 0;
    pub const BACKSPACE: Keysym = 0xff08;
    pub const TAB: Keysym = 0xff09;
    pub const RETURN: Keysym = 0xff0d;
    pub const PAUSE: Keysym = 0xff13;
    pub const SCROLL_LOCK: Keysym = 0xff14;
    pub const ESCAPE: Keysym = 0xff1b;
    pub const HOME: Keysym = 0xff50;
    pub const LEFT: Keysym = 0xff51;
    pub const UP: Keysym = 0xff52;
    pub const RIGHT: Keysym = 0xff53;
    pub const DOWN: Keysym = 0xff54;
    pub const PAGE_UP: Keysym = 0xff55;
    pub const PAGE_DOWN: Keysym = 0xff56;
    pub const END: Keysym = 0xff57;
    pub const PRINT: Keysym = 0xff61;
    pub const INSERT: Keysym = 0xff63;
    pub const MENU: Keysym = 0xff67;
    pub const NUM_LOCK: Keysym = 0xff7f;
    pub const KP_SPACE: Keysym = 0xff80;
    pub const KP_ENTER: Keysym = 0xff8d;
    pub const KP_HOME: Keysym = 0xff95;
    pub const KP_LEFT: Keysym = 0xff96;
    pub const KP_UP: Keysym = 0xff97;
    pub const KP_RIGHT: Keysym = 0xff98;
    pub const KP_DOWN: Keysym = 0xff99;
    pub const KP_PAGE_UP: Keysym = 0xff9a;
    pub const KP_PAGE_DOWN: Keysym = 0xff9b;
    pub const KP_END: Keysym = 0xff9c;
    pub const KP_INSERT: Keysym = 0xff9e;
    pub const KP_DELETE: Keysym = 0xff9f;
    pub const KP_MULTIPLY: Keysym = 0xffaa;
    pub const KP_DIVIDE: Keysym = 0xffaf;
    pub const KP_0: Keysym = 0xffb0;
    pub const KP_9: Keysym = 0xffb9;
    pub const KP_EQUAL: Keysym = 0xffbd;
    pub const F1: Keysym = 0xffbe;
    pub const F35: Keysym = 0xffe0;
    pub const SHIFT_L: Keysym = 0xffe1;
    pub const SHIFT_R: Keysym = 0xffe2;
    pub const CONTROL_L: Keysym = 0xffe3;
    pub const CONTROL_R: Keysym = 0xffe4;
    pub const CAPS_LOCK: Keysym = 0xffe5;
    pub const SHIFT_LOCK: Keysym = 0xffe6;
    pub const META_L: Keysym = 0xffe7;
    pub const META_R: Keysym = 0xffe8;
    pub const ALT_L: Keysym = 0xffe9;
    pub const ALT_R: Keysym = 0xffea;
    pub const SUPER_L: Keysym = 0xffeb;
    pub const SUPER_R: Keysym = 0xffec;
    pub const DELETE: Keysym = 0xffff;
    pub const ISO_LEFT_TAB: Keysym = 0xfe20;
    pub const XF86_AUDIO_LOWER_VOLUME: Keysym = 0x1008_ff11;
    pub const XF86_AUDIO_MUTE: Keysym = 0x1008_ff12;
    pub const XF86_AUDIO_RAISE_VOLUME: Keysym = 0x1008_ff13;
    pub const XF86_AUDIO_PLAY: Keysym = 0x1008_ff14;
    pub const XF86_AUDIO_STOP: Keysym = 0x1008_ff15;
    pub const XF86_AUDIO_PREV: Keysym = 0x1008_ff16;
    pub const XF86_AUDIO_NEXT: Keysym = 0x1008_ff17;
    pub const XF86_AUDIO_PAUSE: Keysym = 0x1008_ff31;
}

/// Is this a keypad keysym (affected by Num Lock)?
pub fn is_keypad(sym: Keysym) -> bool {
    (keysym::KP_SPACE..=keysym::KP_EQUAL).contains(&sym) || (0x1100_0000..=0x1100_ffff).contains(&sym)
}

/// The character a keysym types, if any: Latin-1, explicit Unicode
/// (`0x0100_0000 + code point`), and the keypad's digits and operators.
pub fn keysym_to_char(sym: Keysym) -> Option<char> {
    match sym {
        0x20..=0x7e | 0xa0..=0xff => char::from_u32(sym),
        0x0100_0100..=0x0110_ffff => char::from_u32(sym - 0x0100_0000),
        keysym::KP_SPACE => Some(' '),
        keysym::KP_0..=keysym::KP_9 => char::from_u32(u32::from(b'0') + (sym - keysym::KP_0)),
        keysym::KP_MULTIPLY..=keysym::KP_DIVIDE => Some(['*', '+', ',', '-', '.', '/'][(sym - keysym::KP_MULTIPLY) as usize]),
        keysym::KP_EQUAL => Some('='),
        _ => None,
    }
}

/// Map a keysym to a [`Key`].
pub fn keysym_to_key(sym: Keysym) -> Key {
    use keysym::*;
    if let Some(c) = keysym_to_char(sym) {
        return Key::Character(c);
    }
    match sym {
        RETURN | KP_ENTER => Key::Enter,
        TAB | ISO_LEFT_TAB => Key::Tab,
        BACKSPACE => Key::Backspace,
        ESCAPE => Key::Escape,
        DELETE | KP_DELETE => Key::Delete,
        INSERT | KP_INSERT => Key::Insert,
        HOME | KP_HOME => Key::Home,
        END | KP_END => Key::End,
        PAGE_UP | KP_PAGE_UP => Key::PageUp,
        PAGE_DOWN | KP_PAGE_DOWN => Key::PageDown,
        LEFT | KP_LEFT => Key::Left,
        RIGHT | KP_RIGHT => Key::Right,
        UP | KP_UP => Key::Up,
        DOWN | KP_DOWN => Key::Down,
        F1..=F35 => Key::F((sym - F1 + 1) as u8),
        SHIFT_L | SHIFT_R => Key::Shift,
        CONTROL_L | CONTROL_R => Key::Control,
        ALT_L | ALT_R | META_L | META_R => Key::Alt,
        SUPER_L | SUPER_R => Key::Super,
        CAPS_LOCK => Key::CapsLock,
        NUM_LOCK => Key::NumLock,
        SCROLL_LOCK => Key::ScrollLock,
        PAUSE => Key::Pause,
        PRINT => Key::PrintScreen,
        MENU => Key::Menu,
        XF86_AUDIO_RAISE_VOLUME => Key::VolumeUp,
        XF86_AUDIO_LOWER_VOLUME => Key::VolumeDown,
        XF86_AUDIO_MUTE => Key::Mute,
        XF86_AUDIO_PLAY => Key::MediaPlay,
        XF86_AUDIO_PAUSE => Key::MediaPause,
        XF86_AUDIO_STOP => Key::MediaStop,
        XF86_AUDIO_NEXT => Key::MediaNext,
        XF86_AUDIO_PREV => Key::MediaPrevious,
        other => Key::Unknown(other),
    }
}

/// `(lowercase, uppercase)` of a keysym; both the same when caseless.
fn convert_case(sym: Keysym) -> (Keysym, Keysym) {
    match sym {
        0x41..=0x5a => (sym + 0x20, sym),
        0x61..=0x7a => (sym, sym - 0x20),
        0xc0..=0xde if sym != 0xd7 => (sym + 0x20, sym),
        0xe0..=0xfe if sym != 0xf7 => (sym, sym - 0x20),
        0x0100_0100..=0x0110_ffff => {
            let Some(c) = char::from_u32(sym - 0x0100_0000) else { return (sym, sym) };
            // Only one-to-one mappings count ('ß' uppercases to "SS").
            let single = |it: &mut dyn Iterator<Item = char>| match (it.next(), it.next()) {
                (Some(x), None) => x,
                _ => c,
            };
            // A result inside Latin-1 is named by its Latin-1 keysym.
            let keysym_of = |ch: char| if u32::from(ch) < 0x100 { u32::from(ch) } else { u32::from(ch) + 0x0100_0000 };
            (keysym_of(single(&mut c.to_lowercase())), keysym_of(single(&mut c.to_uppercase())))
        }
        _ => (sym, sym),
    }
}

/// Modifier keys held during an event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Shift.
    pub shift: bool,
    /// Control.
    pub ctrl: bool,
    /// Alt (the modifier `Alt_L`/`Meta_L` is bound to, normally Mod1).
    pub alt: bool,
    /// Super (the modifier `Super_L` is bound to, normally Mod4).
    pub logo: bool,
    /// Caps Lock is on.
    pub caps_lock: bool,
    /// Num Lock is on.
    pub num_lock: bool,
}

/// The server's keyboard layout: keycode → keysyms, plus which modifier
/// bits mean Num Lock, Alt and Super.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    min_keycode: u8,
    per_keycode: u8,
    keysyms: Vec<Keysym>,
    num_lock: u16,
    alt: u16,
    logo: u16,
    lock: LockMode,
}

/// What the Lock modifier means, from the keysyms bound to it (§5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum LockMode {
    /// Nothing recognizable is bound to Lock: ignore it.
    #[default]
    None,
    /// `Caps_Lock`: uppercase letters only.
    Caps,
    /// `Shift_Lock`: like holding Shift.
    Shift,
}

impl Keymap {
    /// Build from `GetKeyboardMapping` (starting at `min_keycode`) and
    /// `GetModifierMapping` replies.
    pub fn new(min_keycode: u8, keysyms_per_keycode: u8, keysyms: Vec<Keysym>, modifiers: &[Vec<u8>; 8]) -> Keymap {
        let mut map = Keymap { min_keycode, per_keycode: keysyms_per_keycode, keysyms, num_lock: 0, alt: 0, logo: 0, lock: LockMode::None };
        // Mod1..Mod5 are modifier indices 3..8; find which one each
        // special key is bound to, falling back to the usual Mod2/Mod1/Mod4.
        let find = |map: &Keymap, syms: &[Keysym], fallback: u16| {
            (3..8)
                .find(|&m| modifiers[m].iter().any(|&kc| (0..usize::from(map.per_keycode)).any(|col| syms.contains(&map.raw(kc, col)))))
                .map_or(fallback, |m| 1 << m)
        };
        map.num_lock = find(&map, &[keysym::NUM_LOCK], 0);
        map.alt = find(&map, &[keysym::ALT_L, keysym::ALT_R, keysym::META_L, keysym::META_R], state::MOD1);
        map.logo = find(&map, &[keysym::SUPER_L, keysym::SUPER_R], state::MOD4);
        // Lock is Caps Lock if Caps_Lock is bound to it, Shift Lock if
        // Shift_Lock is, and otherwise ignored (protocol §5).
        let lock_syms: Vec<Keysym> =
            modifiers[1].iter().flat_map(|&kc| (0..usize::from(map.per_keycode)).map(move |col| (kc, col))).map(|(kc, col)| map.raw(kc, col)).collect();
        map.lock = if lock_syms.contains(&keysym::CAPS_LOCK) {
            LockMode::Caps
        } else if lock_syms.contains(&keysym::SHIFT_LOCK) {
            LockMode::Shift
        } else {
            LockMode::None
        };
        map
    }

    /// The keysym in `column` of `keycode`'s row, or `NoSymbol`.
    fn raw(&self, keycode: u8, column: usize) -> Keysym {
        if keycode < self.min_keycode || column >= usize::from(self.per_keycode) {
            return keysym::NO_SYMBOL;
        }
        let i = usize::from(keycode - self.min_keycode) * usize::from(self.per_keycode) + column;
        self.keysyms.get(i).copied().unwrap_or(keysym::NO_SYMBOL)
    }

    /// The keysym `keycode` produces with modifier `state` (an event's
    /// `state` field). `NoSymbol` (0) for an unmapped key.
    pub fn keysym(&self, keycode: u8, state_bits: u16) -> Keysym {
        let (mut k0, mut k1) = (self.raw(keycode, 0), self.raw(keycode, 1));
        if k1 == keysym::NO_SYMBOL {
            // A lone keysym: a letter means (lower, upper); else itself twice.
            (k0, k1) = convert_case(k0);
        }
        let lock_on = state_bits & state::LOCK != 0;
        let shift = state_bits & state::SHIFT != 0 || (lock_on && self.lock == LockMode::Shift);
        let caps = lock_on && self.lock == LockMode::Caps;
        if self.num_lock != 0 && state_bits & self.num_lock != 0 && is_keypad(k1) {
            return if shift { k0 } else { k1 };
        }
        match (shift, caps) {
            (false, false) => k0,
            (false, true) => convert_case(k0).1,
            (true, true) => convert_case(k1).1,
            (true, false) => k1,
        }
    }

    /// The [`Key`] and keysym for a key event.
    pub fn lookup(&self, keycode: u8, state_bits: u16) -> (Key, Keysym) {
        let sym = self.keysym(keycode, state_bits);
        (keysym_to_key(sym), sym)
    }

    /// Decode the modifier bits of an event's `state`.
    pub fn modifiers(&self, state_bits: u16) -> Modifiers {
        Modifiers {
            shift: state_bits & state::SHIFT != 0,
            ctrl: state_bits & state::CONTROL != 0,
            alt: state_bits & self.alt != 0,
            logo: state_bits & self.logo != 0,
            caps_lock: state_bits & state::LOCK != 0 && self.lock == LockMode::Caps,
            num_lock: self.num_lock != 0 && state_bits & self.num_lock != 0,
        }
    }

    /// A keycode (and whether Shift is needed) that types `sym`; the
    /// inverse of [`Keymap::keysym`], for synthesizing input in tests.
    pub fn keycode_for(&self, sym: Keysym) -> Option<(u8, bool)> {
        let per = usize::from(self.per_keycode).max(1);
        let rows = self.keysyms.len() / per;
        (0..rows).find_map(|row| {
            let kc = u8::try_from(row + usize::from(self.min_keycode)).ok()?;
            if self.keysym(kc, 0) == sym {
                Some((kc, false))
            } else if self.keysym(kc, state::SHIFT) == sym {
                Some((kc, true))
            } else {
                None
            }
        })
    }

    /// The modifier mask Num Lock is bound to (0 if none).
    pub fn num_lock_mask(&self) -> u16 {
        self.num_lock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny 2-column keymap starting at keycode 8:
    /// 8 a/A, 9 1/!, 10 KP_Home/KP_7, 11 Num_Lock, 12 Alt_L, 13 Super_L,
    /// 14 lone 'b', 15 lone 'é' (0xe9), 16 Return, 17 lone Unicode 'ж',
    /// 18 Caps_Lock (bound to Lock), 19 Shift_Lock.
    fn map() -> Keymap {
        let rows: [[Keysym; 2]; 12] = [
            [0x61, 0x41],
            [0x31, 0x21],
            [keysym::KP_HOME, 0xffb7],
            [keysym::NUM_LOCK, 0],
            [keysym::ALT_L, keysym::META_L],
            [keysym::SUPER_L, 0],
            [0x62, 0],
            [0xe9, 0],
            [keysym::RETURN, 0],
            [0x0100_0436, 0],
            [keysym::CAPS_LOCK, 0],
            [keysym::SHIFT_LOCK, 0],
        ];
        let syms = rows.concat();
        let mut mods: [Vec<u8>; 8] = Default::default();
        mods[3] = vec![12]; // Mod1 = Alt
        mods[4] = vec![11]; // Mod2 = NumLock
        mods[6] = vec![13]; // Mod4 = Super
        mods[1] = vec![18]; // Lock = Caps_Lock
        Keymap::new(8, 2, syms, &mods)
    }

    #[test]
    fn shift_selects_the_second_column() {
        let m = map();
        assert_eq!(m.lookup(8, 0).0, Key::Character('a'));
        assert_eq!(m.lookup(8, state::SHIFT).0, Key::Character('A'));
        assert_eq!(m.lookup(9, 0).0, Key::Character('1'));
        assert_eq!(m.lookup(9, state::SHIFT).0, Key::Character('!'));
    }

    #[test]
    fn lock_meaning_follows_its_binding() {
        let base = map();
        let with_lock = |kcs: Vec<u8>| {
            let mut mods: [Vec<u8>; 8] = Default::default();
            mods[1] = kcs;
            Keymap::new(8, 2, base.keysyms.clone(), &mods)
        };
        let shift_lock = with_lock(vec![19]);
        assert_eq!(shift_lock.lookup(9, state::LOCK).0, Key::Character('!'), "Shift_Lock shifts digits too");
        assert_eq!(shift_lock.lookup(8, state::LOCK).0, Key::Character('A'));
        assert!(!shift_lock.modifiers(state::LOCK).caps_lock);
        let unbound = with_lock(vec![]);
        assert_eq!(unbound.lookup(8, state::LOCK).0, Key::Character('a'), "an unbound Lock is ignored");
        assert!(!unbound.modifiers(state::LOCK).caps_lock);
        assert!(base.modifiers(state::LOCK).caps_lock);
    }

    #[test]
    fn caps_lock_uppercases_letters_only() {
        let m = map();
        assert_eq!(m.lookup(8, state::LOCK).0, Key::Character('A'));
        assert_eq!(m.lookup(9, state::LOCK).0, Key::Character('1'), "Lock does not shift digits");
        assert_eq!(m.lookup(8, state::LOCK | state::SHIFT).0, Key::Character('A'));
    }

    #[test]
    fn lone_letters_get_both_cases() {
        let m = map();
        assert_eq!(m.lookup(14, 0).0, Key::Character('b'));
        assert_eq!(m.lookup(14, state::SHIFT).0, Key::Character('B'));
        assert_eq!(m.lookup(15, state::SHIFT).0, Key::Character('É'));
        assert_eq!(m.lookup(17, state::SHIFT).0, Key::Character('Ж'));
        assert_eq!(m.lookup(16, state::SHIFT).0, Key::Enter, "caseless lone keysym repeats");
    }

    #[test]
    fn num_lock_selects_keypad_digits() {
        let m = map();
        let nl = m.num_lock_mask();
        assert_eq!(nl, state::MOD2);
        assert_eq!(m.lookup(10, 0).0, Key::Home);
        assert_eq!(m.lookup(10, nl).0, Key::Character('7'));
        assert_eq!(m.lookup(10, nl | state::SHIFT).0, Key::Home, "Shift inverts Num Lock");
        assert!(m.modifiers(nl).num_lock);
    }

    #[test]
    fn modifiers_follow_the_modifier_map() {
        let m = map();
        let mods = m.modifiers(state::SHIFT | state::CONTROL | state::MOD1 | state::MOD4 | state::LOCK);
        assert_eq!(mods, Modifiers { shift: true, ctrl: true, alt: true, logo: true, caps_lock: true, num_lock: false });

        // Alt bound to Mod3 instead of Mod1.
        let mut mods8: [Vec<u8>; 8] = Default::default();
        mods8[5] = vec![12];
        let m = Keymap::new(8, 2, map().keysyms, &mods8);
        assert!(m.modifiers(state::MOD3).alt);
        assert!(!m.modifiers(state::MOD1).alt);
        assert_eq!(m.num_lock_mask(), 0, "no Num Lock bound");
        assert!(!m.modifiers(state::MOD2).num_lock);
    }

    #[test]
    fn out_of_range_keycodes_are_no_symbol() {
        let m = map();
        assert_eq!(m.keysym(7, 0), 0);
        assert_eq!(m.keysym(200, 0), 0);
        assert_eq!(m.lookup(200, 0).0, Key::Unknown(0));
        let empty = Keymap::default();
        assert_eq!(empty.keysym(8, state::SHIFT), 0);
    }

    #[test]
    fn keycode_for_inverts_lookup() {
        let m = map();
        assert_eq!(m.keycode_for(0x61), Some((8, false)));
        assert_eq!(m.keycode_for(0x21), Some((9, true)));
        assert_eq!(m.keycode_for(0x42), Some((14, true)));
        assert_eq!(m.keycode_for(0x1234), None);
    }

    #[test]
    fn named_and_media_keys() {
        assert_eq!(keysym_to_key(keysym::RETURN), Key::Enter);
        assert_eq!(keysym_to_key(keysym::KP_ENTER), Key::Enter);
        assert_eq!(keysym_to_key(keysym::ISO_LEFT_TAB), Key::Tab);
        assert_eq!(keysym_to_key(keysym::F1), Key::F(1));
        assert_eq!(keysym_to_key(0xffc2), Key::F(5));
        assert_eq!(keysym_to_key(keysym::F35), Key::F(35));
        assert_eq!(keysym_to_key(keysym::XF86_AUDIO_PLAY), Key::MediaPlay);
        assert_eq!(keysym_to_key(keysym::XF86_AUDIO_NEXT), Key::MediaNext);
        assert_eq!(keysym_to_key(keysym::XF86_AUDIO_PREV), Key::MediaPrevious);
        assert_eq!(keysym_to_key(keysym::XF86_AUDIO_STOP), Key::MediaStop);
        assert_eq!(keysym_to_key(keysym::XF86_AUDIO_PAUSE), Key::MediaPause);
        assert_eq!(keysym_to_key(keysym::XF86_AUDIO_MUTE), Key::Mute);
        assert_eq!(keysym_to_key(keysym::XF86_AUDIO_RAISE_VOLUME), Key::VolumeUp);
        assert_eq!(keysym_to_key(keysym::XF86_AUDIO_LOWER_VOLUME), Key::VolumeDown);
        assert_eq!(keysym_to_key(0xdead), Key::Unknown(0xdead));
    }

    #[test]
    fn characters_from_keysyms() {
        assert_eq!(keysym_to_char(0x20), Some(' '));
        assert_eq!(keysym_to_char(0x7e), Some('~'));
        assert_eq!(keysym_to_char(0x7f), None);
        assert_eq!(keysym_to_char(0xa9), Some('©'));
        assert_eq!(keysym_to_char(0x0100_20ac), Some('€'));
        assert_eq!(keysym_to_char(0x0100_d800), None, "a surrogate is not a char");
        assert_eq!(keysym_to_char(keysym::KP_0), Some('0'));
        assert_eq!(keysym_to_char(keysym::KP_9), Some('9'));
        assert_eq!(keysym_to_char(0xffab), Some('+'));
        assert_eq!(keysym_to_char(keysym::KP_DIVIDE), Some('/'));
        assert_eq!(keysym_to_char(keysym::KP_EQUAL), Some('='));
    }

    #[test]
    fn latin1_case_pairs_skip_multiply_and_divide() {
        assert_eq!(convert_case(0xd7), (0xd7, 0xd7), "× is not a letter");
        assert_eq!(convert_case(0xf7), (0xf7, 0xf7), "÷ is not a letter");
        assert_eq!(convert_case(0xff), (0xff, 0xff), "ÿ's capital is outside Latin-1");
        assert_eq!(convert_case(0xc0), (0xe0, 0xc0));
        assert_eq!(convert_case(0x0100_212a), (0x6b, 0x0100_212a), "Kelvin sign lowercases into Latin-1");
        assert_eq!(convert_case(0x0100_00df), (0x0100_00df, 0x0100_00df), "non-canonical keysym below U+0100 is left alone");
    }
}
