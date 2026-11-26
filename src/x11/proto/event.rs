//! Event decoding (server → client): the 32-byte core events, decoded to
//! [`RawEvent`] with every field the protocol carries for the events this
//! crate selects, and a catch-all for the rest.

use super::{Atom, ReadLe, Xid};

/// Every core event is exactly this many bytes (`GenericEvent` adds a
/// trailing body, see [`packet_len`]).
pub const EVENT_LEN: usize = 32;

/// Core event codes.
pub mod code {
    #![allow(missing_docs)]
    pub const KEY_PRESS: u8 = 2;
    pub const KEY_RELEASE: u8 = 3;
    pub const BUTTON_PRESS: u8 = 4;
    pub const BUTTON_RELEASE: u8 = 5;
    pub const MOTION_NOTIFY: u8 = 6;
    pub const ENTER_NOTIFY: u8 = 7;
    pub const LEAVE_NOTIFY: u8 = 8;
    pub const FOCUS_IN: u8 = 9;
    pub const FOCUS_OUT: u8 = 10;
    pub const KEYMAP_NOTIFY: u8 = 11;
    pub const EXPOSE: u8 = 12;
    pub const DESTROY_NOTIFY: u8 = 17;
    pub const UNMAP_NOTIFY: u8 = 18;
    pub const MAP_NOTIFY: u8 = 19;
    pub const MAP_REQUEST: u8 = 20;
    pub const REPARENT_NOTIFY: u8 = 21;
    pub const CONFIGURE_NOTIFY: u8 = 22;
    pub const CONFIGURE_REQUEST: u8 = 23;
    pub const PROPERTY_NOTIFY: u8 = 28;
    pub const CLIENT_MESSAGE: u8 = 33;
    pub const MAPPING_NOTIFY: u8 = 34;
    pub const GENERIC_EVENT: u8 = 35;
}

/// The total length of the packet whose first 32 bytes are `head`: 32 for
/// errors and core events, `32 + 4 * length` for replies and
/// `GenericEvent`s. `None` if `head` is shorter than 8 bytes.
pub fn packet_len(head: &[u8]) -> Option<usize> {
    let kind = head.u8_at(0)?;
    if kind == 1 || kind & 0x7f == code::GENERIC_EVENT {
        let words = head.u32_at(4)? as usize;
        return EVENT_LEN.checked_add(words.checked_mul(4)?);
    }
    head.get(..8)?;
    Some(EVENT_LEN)
}

/// The fields shared by key, button and motion events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputEvent {
    /// Keycode (key events), button number (button events), or `0`/`1`
    /// "is hint" (motion).
    pub detail: u8,
    /// Server timestamp in milliseconds.
    pub time: u32,
    /// Root window.
    pub root: Xid,
    /// The window the event is reported relative to.
    pub event: Xid,
    /// Child of `event` containing the pointer, or 0.
    pub child: Xid,
    /// Pointer position relative to the root.
    pub root_x: i16,
    /// Pointer position relative to the root.
    pub root_y: i16,
    /// Pointer position relative to `event`.
    pub x: i16,
    /// Pointer position relative to `event`.
    pub y: i16,
    /// Modifier and button state *before* the event (`SETofKEYBUTMASK`).
    pub state: u16,
}

/// A decoded core event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawEvent {
    /// `KeyPress`.
    KeyPress(InputEvent),
    /// `KeyRelease`.
    KeyRelease(InputEvent),
    /// `ButtonPress`.
    ButtonPress(InputEvent),
    /// `ButtonRelease`.
    ButtonRelease(InputEvent),
    /// `MotionNotify`.
    MotionNotify(InputEvent),
    /// `EnterNotify`.
    EnterNotify {
        /// Position, state and windows.
        input: InputEvent,
        /// 0 Normal, 1 Grab, 2 Ungrab.
        mode: u8,
    },
    /// `LeaveNotify`.
    LeaveNotify {
        /// Position, state and windows.
        input: InputEvent,
        /// 0 Normal, 1 Grab, 2 Ungrab.
        mode: u8,
    },
    /// `FocusIn`.
    FocusIn {
        /// Window.
        window: Xid,
        /// Focus mode (0 Normal, 1 Grab, 2 Ungrab, 3 WhileGrabbed).
        mode: u8,
    },
    /// `FocusOut`.
    FocusOut {
        /// Window.
        window: Xid,
        /// Focus mode.
        mode: u8,
    },
    /// `Expose`: a region of `window` needs repainting.
    Expose {
        /// Window.
        window: Xid,
        /// Damaged region.
        x: u16,
        /// Damaged region.
        y: u16,
        /// Damaged region.
        width: u16,
        /// Damaged region.
        height: u16,
        /// Number of further `Expose` events that follow for this window.
        count: u16,
    },
    /// `DestroyNotify`.
    DestroyNotify {
        /// The window that was destroyed.
        window: Xid,
    },
    /// `UnmapNotify`.
    UnmapNotify {
        /// The window that was unmapped.
        window: Xid,
    },
    /// `MapNotify`.
    MapNotify {
        /// The window that was mapped.
        window: Xid,
    },
    /// `ReparentNotify` (a window manager adopting the window).
    ReparentNotify {
        /// The window.
        window: Xid,
        /// Its new parent.
        parent: Xid,
    },
    /// `ConfigureNotify`: size, position or stacking changed.
    ConfigureNotify {
        /// The window.
        window: Xid,
        /// Position relative to the parent (or the root, when synthetic).
        x: i16,
        /// Position relative to the parent (or the root, when synthetic).
        y: i16,
        /// New width.
        width: u16,
        /// New height.
        height: u16,
    },
    /// `PropertyNotify`.
    PropertyNotify {
        /// The window.
        window: Xid,
        /// The property.
        atom: Atom,
        /// 0 = new value, 1 = deleted.
        state: u8,
    },
    /// `ClientMessage`.
    ClientMessage {
        /// The window.
        window: Xid,
        /// 8, 16 or 32.
        format: u8,
        /// The message type atom.
        message_type: Atom,
        /// The 20 data bytes as five 32-bit words (meaningful for format 32).
        data: [u32; 5],
    },
    /// `MappingNotify`: the keyboard or modifier mapping changed.
    MappingNotify {
        /// 0 Modifier, 1 Keyboard, 2 Pointer.
        request: u8,
        /// First changed keycode.
        first_keycode: u8,
        /// Number of changed keycodes.
        count: u8,
    },
    /// `MapRequest` (to a window manager): a child of a window it
    /// redirects wants to be shown.
    MapRequest {
        /// The redirecting parent.
        parent: Xid,
        /// The window to be shown.
        window: Xid,
    },
    /// `ConfigureRequest` (to a window manager): a child of a window it
    /// redirects asks to move, resize or restack. Only the fields named in
    /// `value_mask` were asked for.
    ConfigureRequest {
        /// The redirecting parent.
        parent: Xid,
        /// The window.
        window: Xid,
        /// Sibling for restacking (with `stack_mode`), or 0.
        sibling: Xid,
        /// Asked-for position.
        x: i16,
        /// Asked-for position.
        y: i16,
        /// Asked-for width.
        width: u16,
        /// Asked-for height.
        height: u16,
        /// Asked-for border width.
        border_width: u16,
        /// 0 Above, 1 Below, 2 TopIf, 3 BottomIf, 4 Opposite.
        stack_mode: u8,
        /// Which fields: x 1, y 2, width 4, height 8, border 16, sibling
        /// 32, stack mode 64.
        value_mask: u16,
    },
    /// Any event this crate does not decode, by code.
    Other(u8),
}

impl RawEvent {
    /// The window the event is about: the one acted on, for events a
    /// parent hears of through `SubstructureNotify` or redirection (not
    /// the parent it was reported to). `None` for keyboard-wide events.
    pub fn window(&self) -> Option<Xid> {
        match *self {
            RawEvent::KeyPress(i)
            | RawEvent::KeyRelease(i)
            | RawEvent::ButtonPress(i)
            | RawEvent::ButtonRelease(i)
            | RawEvent::MotionNotify(i)
            | RawEvent::EnterNotify { input: i, .. }
            | RawEvent::LeaveNotify { input: i, .. } => Some(i.event),
            RawEvent::FocusIn { window, .. }
            | RawEvent::FocusOut { window, .. }
            | RawEvent::Expose { window, .. }
            | RawEvent::DestroyNotify { window }
            | RawEvent::UnmapNotify { window }
            | RawEvent::MapNotify { window }
            | RawEvent::ReparentNotify { window, .. }
            | RawEvent::ConfigureNotify { window, .. }
            | RawEvent::PropertyNotify { window, .. }
            | RawEvent::ClientMessage { window, .. }
            | RawEvent::MapRequest { window, .. }
            | RawEvent::ConfigureRequest { window, .. } => Some(window),
            RawEvent::MappingNotify { .. } | RawEvent::Other(_) => None,
        }
    }
}

/// A decoded event and whether it was synthesized with `SendEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// The event.
    pub event: RawEvent,
    /// `true` if another client sent it via `SendEvent`.
    pub send_event: bool,
    /// The 16-bit sequence number it carries, or `None` for `KeymapNotify`,
    /// the one event without one.
    pub sequence: Option<u16>,
}

fn input(buf: &[u8]) -> Option<InputEvent> {
    Some(InputEvent {
        detail: buf.u8_at(1)?,
        time: buf.u32_at(4)?,
        root: buf.u32_at(8)?,
        event: buf.u32_at(12)?,
        child: buf.u32_at(16)?,
        root_x: buf.i16_at(20)?,
        root_y: buf.i16_at(22)?,
        x: buf.i16_at(24)?,
        y: buf.i16_at(26)?,
        state: buf.u16_at(28)?,
    })
}

/// Decode one event. `None` only for a buffer shorter than 32 bytes or one
/// that is not an event (first byte 0 = error, 1 = reply); unknown event
/// codes decode as [`RawEvent::Other`].
pub fn parse_event(buf: &[u8]) -> Option<Event> {
    if buf.len() < EVENT_LEN || buf[0] < 2 {
        return None;
    }
    let send_event = buf[0] & 0x80 != 0;
    let c = buf[0] & 0x7f;
    use code::*;
    let event = match c {
        KEY_PRESS => RawEvent::KeyPress(input(buf)?),
        KEY_RELEASE => RawEvent::KeyRelease(input(buf)?),
        BUTTON_PRESS => RawEvent::ButtonPress(input(buf)?),
        BUTTON_RELEASE => RawEvent::ButtonRelease(input(buf)?),
        MOTION_NOTIFY => RawEvent::MotionNotify(input(buf)?),
        ENTER_NOTIFY => RawEvent::EnterNotify { input: input(buf)?, mode: buf.u8_at(30)? },
        LEAVE_NOTIFY => RawEvent::LeaveNotify { input: input(buf)?, mode: buf.u8_at(30)? },
        FOCUS_IN => RawEvent::FocusIn { window: buf.u32_at(4)?, mode: buf.u8_at(8)? },
        FOCUS_OUT => RawEvent::FocusOut { window: buf.u32_at(4)?, mode: buf.u8_at(8)? },
        EXPOSE => RawEvent::Expose {
            window: buf.u32_at(4)?,
            x: buf.u16_at(8)?,
            y: buf.u16_at(10)?,
            width: buf.u16_at(12)?,
            height: buf.u16_at(14)?,
            count: buf.u16_at(16)?,
        },
        DESTROY_NOTIFY => RawEvent::DestroyNotify { window: buf.u32_at(8)? },
        UNMAP_NOTIFY => RawEvent::UnmapNotify { window: buf.u32_at(8)? },
        MAP_NOTIFY => RawEvent::MapNotify { window: buf.u32_at(8)? },
        REPARENT_NOTIFY => RawEvent::ReparentNotify { window: buf.u32_at(8)?, parent: buf.u32_at(12)? },
        CONFIGURE_NOTIFY => {
            RawEvent::ConfigureNotify { window: buf.u32_at(8)?, x: buf.i16_at(16)?, y: buf.i16_at(18)?, width: buf.u16_at(20)?, height: buf.u16_at(22)? }
        }
        PROPERTY_NOTIFY => RawEvent::PropertyNotify { window: buf.u32_at(4)?, atom: buf.u32_at(8)?, state: buf.u8_at(16)? },
        CLIENT_MESSAGE => {
            let mut data = [0u32; 5];
            for (i, d) in data.iter_mut().enumerate() {
                *d = buf.u32_at(12 + i * 4)?;
            }
            RawEvent::ClientMessage { window: buf.u32_at(4)?, format: buf.u8_at(1)?, message_type: buf.u32_at(8)?, data }
        }
        MAP_REQUEST => RawEvent::MapRequest { parent: buf.u32_at(4)?, window: buf.u32_at(8)? },
        CONFIGURE_REQUEST => RawEvent::ConfigureRequest {
            stack_mode: buf.u8_at(1)?,
            parent: buf.u32_at(4)?,
            window: buf.u32_at(8)?,
            sibling: buf.u32_at(12)?,
            x: buf.i16_at(16)?,
            y: buf.i16_at(18)?,
            width: buf.u16_at(20)?,
            height: buf.u16_at(22)?,
            border_width: buf.u16_at(24)?,
            value_mask: buf.u16_at(26)?,
        },
        MAPPING_NOTIFY => RawEvent::MappingNotify { request: buf.u8_at(4)?, first_keycode: buf.u8_at(5)?, count: buf.u8_at(6)? },
        other => RawEvent::Other(other),
    };
    let sequence = if c == KEYMAP_NOTIFY { None } else { buf.u16_at(2) };
    Some(Event { event, send_event, sequence })
}

/// `SETofKEYBUTMASK` bits found in [`InputEvent::state`].
pub mod state {
    #![allow(missing_docs)]
    pub const SHIFT: u16 = 1 << 0;
    pub const LOCK: u16 = 1 << 1;
    pub const CONTROL: u16 = 1 << 2;
    pub const MOD1: u16 = 1 << 3;
    pub const MOD2: u16 = 1 << 4;
    pub const MOD3: u16 = 1 << 5;
    pub const MOD4: u16 = 1 << 6;
    pub const MOD5: u16 = 1 << 7;
    pub const BUTTON1: u16 = 1 << 8;
    pub const BUTTON2: u16 = 1 << 9;
    pub const BUTTON3: u16 = 1 << 10;
    pub const BUTTON4: u16 = 1 << 11;
    pub const BUTTON5: u16 = 1 << 12;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A 32-byte event with `code`, `detail`, and the given fields.
    pub(crate) fn synth(code: u8, detail: u8, fields: &[(usize, &[u8])]) -> [u8; 32] {
        let mut b = [0u8; 32];
        b[0] = code;
        b[1] = detail;
        for (off, bytes) in fields {
            b[*off..*off + bytes.len()].copy_from_slice(bytes);
        }
        b
    }

    #[test]
    fn key_press_carries_every_field() {
        let b = synth(
            2,
            38,
            &[
                (2, &7u16.to_le_bytes()),
                (4, &1234u32.to_le_bytes()),
                (8, &0x42u32.to_le_bytes()),
                (12, &0x0040_0001u32.to_le_bytes()),
                (20, &(-1i16).to_le_bytes()),
                (22, &500i16.to_le_bytes()),
                (24, &10i16.to_le_bytes()),
                (26, &20i16.to_le_bytes()),
                (28, &(state::SHIFT | state::BUTTON1).to_le_bytes()),
            ],
        );
        let e = parse_event(&b).unwrap();
        assert_eq!(e.sequence, Some(7));
        assert!(!e.send_event);
        let RawEvent::KeyPress(i) = e.event else { panic!("{e:?}") };
        assert_eq!(i, InputEvent { detail: 38, time: 1234, root: 0x42, event: 0x0040_0001, child: 0, root_x: -1, root_y: 500, x: 10, y: 20, state: 0x101 });
    }

    #[test]
    fn input_event_codes() {
        for (c, check) in [
            (3u8, (|e| matches!(e, RawEvent::KeyRelease(_))) as fn(RawEvent) -> bool),
            (4, |e| matches!(e, RawEvent::ButtonPress(_))),
            (5, |e| matches!(e, RawEvent::ButtonRelease(_))),
            (6, |e| matches!(e, RawEvent::MotionNotify(_))),
            (7, |e| matches!(e, RawEvent::EnterNotify { mode: 0, .. })),
            (8, |e| matches!(e, RawEvent::LeaveNotify { mode: 0, .. })),
        ] {
            assert!(check(parse_event(&synth(c, 0, &[])).unwrap().event), "code {c}");
        }
    }

    #[test]
    fn crossing_mode_is_decoded() {
        let e = parse_event(&synth(8, 0, &[(30, &[1])])).unwrap().event;
        assert!(matches!(e, RawEvent::LeaveNotify { mode: 1, .. }));
    }

    #[test]
    fn structure_events() {
        let e = parse_event(&synth(12, 0, &[(4, &9u32.to_le_bytes()), (8, &[40, 0, 50, 0, 44, 1, 200, 0, 3, 0])])).unwrap().event;
        assert_eq!(e, RawEvent::Expose { window: 9, x: 40, y: 50, width: 300, height: 200, count: 3 });
        let e = parse_event(&synth(22, 0, &[(8, &9u32.to_le_bytes()), (16, &(-4i16).to_le_bytes()), (18, &8i16.to_le_bytes()), (20, &[128, 2, 224, 1])]))
            .unwrap()
            .event;
        assert_eq!(e, RawEvent::ConfigureNotify { window: 9, x: -4, y: 8, width: 640, height: 480 });
        assert_eq!(parse_event(&synth(17, 0, &[(8, &9u32.to_le_bytes())])).unwrap().event, RawEvent::DestroyNotify { window: 9 });
        assert_eq!(parse_event(&synth(18, 0, &[(8, &9u32.to_le_bytes())])).unwrap().event, RawEvent::UnmapNotify { window: 9 });
        assert_eq!(parse_event(&synth(19, 0, &[(8, &9u32.to_le_bytes())])).unwrap().event, RawEvent::MapNotify { window: 9 });
        assert_eq!(
            parse_event(&synth(21, 0, &[(8, &9u32.to_le_bytes()), (12, &5u32.to_le_bytes())])).unwrap().event,
            RawEvent::ReparentNotify { window: 9, parent: 5 }
        );
        assert_eq!(parse_event(&synth(9, 0, &[(4, &9u32.to_le_bytes()), (8, &[2])])).unwrap().event, RawEvent::FocusIn { window: 9, mode: 2 });
        assert_eq!(parse_event(&synth(10, 0, &[(4, &9u32.to_le_bytes())])).unwrap().event, RawEvent::FocusOut { window: 9, mode: 0 });
        assert_eq!(
            parse_event(&synth(28, 0, &[(4, &9u32.to_le_bytes()), (8, &39u32.to_le_bytes()), (16, &[1])])).unwrap().event,
            RawEvent::PropertyNotify { window: 9, atom: 39, state: 1 }
        );
        assert_eq!(parse_event(&synth(34, 0, &[(4, &[1, 8, 248])])).unwrap().event, RawEvent::MappingNotify { request: 1, first_keycode: 8, count: 248 });
    }

    #[test]
    fn window_manager_events() {
        let e = parse_event(&synth(20, 0, &[(4, &1u32.to_le_bytes()), (8, &9u32.to_le_bytes())])).unwrap().event;
        assert_eq!(e, RawEvent::MapRequest { parent: 1, window: 9 });
        assert_eq!(e.window(), Some(9), "the window, not the parent it was reported to");
        let fields: &[(usize, &[u8])] = &[
            (1, &[1]),
            (4, &1u32.to_le_bytes()),
            (8, &9u32.to_le_bytes()),
            (12, &7u32.to_le_bytes()),
            (16, &(-3i16).to_le_bytes()),
            (18, &5i16.to_le_bytes()),
            (20, &[128, 2, 224, 1, 2, 0, 0x4c, 0]),
        ];
        let e = parse_event(&synth(23, 0, fields)).unwrap().event;
        let want = RawEvent::ConfigureRequest {
            parent: 1,
            window: 9,
            sibling: 7,
            x: -3,
            y: 5,
            width: 640,
            height: 480,
            border_width: 2,
            stack_mode: 1,
            value_mask: 0x4c,
        };
        assert_eq!(e, want);
        let synthetic = super::super::request::configure_notify(9, 9, 10, -20, 300, 200);
        assert_eq!(parse_event(&synthetic).unwrap().event, RawEvent::ConfigureNotify { window: 9, x: 10, y: -20, width: 300, height: 200 });
    }

    #[test]
    fn client_message_matches_the_encoder() {
        let bytes = crate::x11::proto::request::client_message(0x99, 0x123, [1, 2, 3, 4, 5]);
        let e = parse_event(&bytes).unwrap().event;
        assert_eq!(e, RawEvent::ClientMessage { window: 0x99, format: 32, message_type: 0x123, data: [1, 2, 3, 4, 5] });
    }

    #[test]
    fn send_event_bit_is_reported_and_masked() {
        let e = parse_event(&synth(12 | 0x80, 0, &[])).unwrap();
        assert!(e.send_event);
        assert!(matches!(e.event, RawEvent::Expose { .. }));
    }

    #[test]
    fn keymap_notify_has_no_sequence() {
        let e = parse_event(&synth(11, 0xff, &[])).unwrap();
        assert_eq!(e.sequence, None);
        assert_eq!(e.event, RawEvent::Other(11));
    }

    #[test]
    fn non_events_and_short_buffers() {
        assert_eq!(parse_event(&synth(0, 3, &[])), None, "error packet");
        assert_eq!(parse_event(&synth(1, 0, &[])), None, "reply packet");
        assert_eq!(parse_event(&[2; 31]), None);
        assert_eq!(parse_event(&synth(127, 0, &[])).unwrap().event, RawEvent::Other(127));
    }

    #[test]
    fn packet_lengths() {
        assert_eq!(packet_len(&synth(0, 0, &[])), Some(32), "error");
        assert_eq!(packet_len(&synth(12, 0, &[])), Some(32), "core event");
        assert_eq!(packet_len(&synth(1, 0, &[(4, &3u32.to_le_bytes())])), Some(44), "reply");
        assert_eq!(packet_len(&synth(35, 0, &[(4, &2u32.to_le_bytes())])), Some(40), "generic event");
        assert_eq!(packet_len(&synth(35 | 0x80, 0, &[(4, &2u32.to_le_bytes())])), Some(40));
        assert_eq!(packet_len(&[1, 0, 0]), None);
        assert_eq!(packet_len(&synth(1, 0, &[(4, &u32::MAX.to_le_bytes())])), Some(32 + u32::MAX as usize * 4));
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut s = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..5000 {
            let mut b = [0u8; 32];
            for x in b.iter_mut() {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                *x = s as u8;
            }
            let _ = parse_event(&b);
            let _ = packet_len(&b);
            let _ = super::super::XError::parse(&b);
        }
    }
}
