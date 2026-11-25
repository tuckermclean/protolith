//! Request encoders (client → server) and the reply decoders that go with
//! them. Every encoder returns one complete, padded request.

use super::{event::EVENT_LEN, Atom, ReadLe, Req, Xid};
use crate::pixel::Rect;

/// Major opcodes of the core requests this crate encodes.
pub mod opcode {
    #![allow(missing_docs)]
    pub const CREATE_WINDOW: u8 = 1;
    pub const CHANGE_WINDOW_ATTRIBUTES: u8 = 2;
    pub const GET_WINDOW_ATTRIBUTES: u8 = 3;
    pub const DESTROY_WINDOW: u8 = 4;
    pub const MAP_WINDOW: u8 = 8;
    pub const UNMAP_WINDOW: u8 = 10;
    pub const CONFIGURE_WINDOW: u8 = 12;
    pub const GET_GEOMETRY: u8 = 14;
    pub const INTERN_ATOM: u8 = 16;
    pub const CHANGE_PROPERTY: u8 = 18;
    pub const DELETE_PROPERTY: u8 = 19;
    pub const GET_PROPERTY: u8 = 20;
    pub const SEND_EVENT: u8 = 25;
    pub const UNGRAB_POINTER: u8 = 27;
    pub const TRANSLATE_COORDINATES: u8 = 40;
    pub const SET_INPUT_FOCUS: u8 = 42;
    pub const GET_INPUT_FOCUS: u8 = 43;
    pub const CREATE_PIXMAP: u8 = 53;
    pub const FREE_PIXMAP: u8 = 54;
    pub const CREATE_GC: u8 = 55;
    pub const FREE_GC: u8 = 60;
    pub const COPY_AREA: u8 = 62;
    pub const POLY_FILL_RECTANGLE: u8 = 70;
    pub const PUT_IMAGE: u8 = 72;
    pub const GET_IMAGE: u8 = 73;
    pub const CREATE_COLORMAP: u8 = 78;
    pub const FREE_COLORMAP: u8 = 79;
    pub const QUERY_EXTENSION: u8 = 98;
    pub const GET_KEYBOARD_MAPPING: u8 = 101;
    pub const BELL: u8 = 104;
    pub const GET_MODIFIER_MAPPING: u8 = 119;
    pub const NO_OPERATION: u8 = 127;
}

/// Human-readable name of a core major opcode, for error messages.
pub fn opcode_name(major: u8) -> &'static str {
    use opcode::*;
    match major {
        CREATE_WINDOW => "CreateWindow",
        CHANGE_WINDOW_ATTRIBUTES => "ChangeWindowAttributes",
        GET_WINDOW_ATTRIBUTES => "GetWindowAttributes",
        DESTROY_WINDOW => "DestroyWindow",
        MAP_WINDOW => "MapWindow",
        UNMAP_WINDOW => "UnmapWindow",
        CONFIGURE_WINDOW => "ConfigureWindow",
        GET_GEOMETRY => "GetGeometry",
        INTERN_ATOM => "InternAtom",
        CHANGE_PROPERTY => "ChangeProperty",
        DELETE_PROPERTY => "DeleteProperty",
        GET_PROPERTY => "GetProperty",
        SEND_EVENT => "SendEvent",
        UNGRAB_POINTER => "UngrabPointer",
        TRANSLATE_COORDINATES => "TranslateCoordinates",
        SET_INPUT_FOCUS => "SetInputFocus",
        GET_INPUT_FOCUS => "GetInputFocus",
        CREATE_PIXMAP => "CreatePixmap",
        FREE_PIXMAP => "FreePixmap",
        CREATE_GC => "CreateGC",
        FREE_GC => "FreeGC",
        COPY_AREA => "CopyArea",
        POLY_FILL_RECTANGLE => "PolyFillRectangle",
        PUT_IMAGE => "PutImage",
        GET_IMAGE => "GetImage",
        CREATE_COLORMAP => "CreateColormap",
        FREE_COLORMAP => "FreeColormap",
        QUERY_EXTENSION => "QueryExtension",
        GET_KEYBOARD_MAPPING => "GetKeyboardMapping",
        BELL => "Bell",
        GET_MODIFIER_MAPPING => "GetModifierMapping",
        NO_OPERATION => "NoOperation",
        128..=255 => "an extension request",
        _ => "a core request",
    }
}

// ============================================================== windows

/// Event-mask bits (`SETofEVENT`).
pub mod event_mask {
    #![allow(missing_docs)]
    pub const KEY_PRESS: u32 = 1 << 0;
    pub const KEY_RELEASE: u32 = 1 << 1;
    pub const BUTTON_PRESS: u32 = 1 << 2;
    pub const BUTTON_RELEASE: u32 = 1 << 3;
    pub const ENTER_WINDOW: u32 = 1 << 4;
    pub const LEAVE_WINDOW: u32 = 1 << 5;
    pub const POINTER_MOTION: u32 = 1 << 6;
    pub const EXPOSURE: u32 = 1 << 15;
    pub const STRUCTURE_NOTIFY: u32 = 1 << 17;
    pub const SUBSTRUCTURE_NOTIFY: u32 = 1 << 19;
    pub const SUBSTRUCTURE_REDIRECT: u32 = 1 << 20;
    pub const FOCUS_CHANGE: u32 = 1 << 21;
    pub const PROPERTY_CHANGE: u32 = 1 << 22;
}

/// Window class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowClass {
    /// Same as the parent.
    CopyFromParent = 0,
    /// A normal, drawable window.
    InputOutput = 1,
    /// An invisible window that only receives input.
    InputOnly = 2,
}

/// The optional attributes of `CreateWindow`/`ChangeWindowAttributes`. Only
/// the fields this crate needs; `None` leaves an attribute unset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindowAttributes {
    /// `background-pixmap` (`Some(0)` = None: the server never paints the
    /// background, which avoids a flash before the client draws).
    pub background_pixmap: Option<Xid>,
    /// `background-pixel`.
    pub background_pixel: Option<u32>,
    /// `border-pixel` (required when the visual differs from the parent's).
    pub border_pixel: Option<u32>,
    /// `bit-gravity` (`1` = NorthWest: keep contents on resize).
    pub bit_gravity: Option<u8>,
    /// `override-redirect`: bypass the window manager entirely.
    pub override_redirect: Option<bool>,
    /// `event-mask`.
    pub event_mask: Option<u32>,
    /// `colormap`.
    pub colormap: Option<Xid>,
}

impl WindowAttributes {
    /// The value-mask and value-list, low bit first as the protocol requires.
    fn values(&self) -> (u32, Vec<u32>) {
        let mut mask = 0;
        let mut vals = Vec::new();
        let mut put = |bit: u32, v: Option<u32>| {
            if let Some(v) = v {
                mask |= bit;
                vals.push(v);
            }
        };
        put(0x0001, self.background_pixmap);
        put(0x0002, self.background_pixel);
        put(0x0008, self.border_pixel);
        put(0x0010, self.bit_gravity.map(u32::from));
        put(0x0200, self.override_redirect.map(u32::from));
        put(0x0800, self.event_mask);
        put(0x2000, self.colormap);
        (mask, vals)
    }
}

/// `CreateWindow` parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateWindow {
    /// New window id.
    pub wid: Xid,
    /// Parent window (the root, for top-level windows).
    pub parent: Xid,
    /// Depth (0 = copy from parent).
    pub depth: u8,
    /// Position relative to the parent.
    pub x: i16,
    /// Position relative to the parent.
    pub y: i16,
    /// Size (must be non-zero).
    pub width: u16,
    /// Size (must be non-zero).
    pub height: u16,
    /// Border width.
    pub border_width: u16,
    /// Class.
    pub class: WindowClass,
    /// Visual id (0 = copy from parent).
    pub visual: u32,
    /// Optional attributes.
    pub attributes: WindowAttributes,
}

/// Encode `CreateWindow` (opcode 1).
pub fn create_window(p: &CreateWindow) -> Vec<u8> {
    let (mask, vals) = p.attributes.values();
    let mut r = Req::new(opcode::CREATE_WINDOW, p.depth)
        .u32(p.wid)
        .u32(p.parent)
        .i16(p.x)
        .i16(p.y)
        .u16(p.width)
        .u16(p.height)
        .u16(p.border_width)
        .u16(p.class as u16)
        .u32(p.visual)
        .u32(mask);
    for v in vals {
        r = r.u32(v);
    }
    r.finish()
}

/// Encode `ChangeWindowAttributes` (opcode 2).
pub fn change_window_attributes(window: Xid, attributes: &WindowAttributes) -> Vec<u8> {
    let (mask, vals) = attributes.values();
    let mut r = Req::new(opcode::CHANGE_WINDOW_ATTRIBUTES, 0).u32(window).u32(mask);
    for v in vals {
        r = r.u32(v);
    }
    r.finish()
}

/// Encode `GetWindowAttributes` (opcode 3).
pub fn get_window_attributes(window: Xid) -> Vec<u8> {
    Req::new(opcode::GET_WINDOW_ATTRIBUTES, 0).u32(window).finish()
}

/// `map-state` of a window, from a `GetWindowAttributes` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapState {
    /// Not mapped.
    Unmapped,
    /// Mapped, but an ancestor is not.
    Unviewable,
    /// Mapped and all ancestors mapped.
    Viewable,
}

/// The fields of a `GetWindowAttributes` reply this crate uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowAttributesReply {
    /// Visual id.
    pub visual: u32,
    /// Map state.
    pub map_state: MapState,
    /// `override-redirect`.
    pub override_redirect: bool,
    /// Union of every client's event mask on this window.
    pub all_event_masks: u32,
    /// This client's event mask on this window.
    pub your_event_mask: u32,
}

/// Decode a `GetWindowAttributes` reply.
pub fn parse_window_attributes(reply: &[u8]) -> Option<WindowAttributesReply> {
    Some(WindowAttributesReply {
        visual: reply.u32_at(8)?,
        map_state: match reply.u8_at(26)? {
            0 => MapState::Unmapped,
            1 => MapState::Unviewable,
            _ => MapState::Viewable,
        },
        override_redirect: reply.u8_at(27)? != 0,
        all_event_masks: reply.u32_at(32)?,
        your_event_mask: reply.u32_at(36)?,
    })
}

/// Encode `DestroyWindow` (opcode 4).
pub fn destroy_window(window: Xid) -> Vec<u8> {
    Req::new(opcode::DESTROY_WINDOW, 0).u32(window).finish()
}

/// Encode `MapWindow` (opcode 8).
pub fn map_window(window: Xid) -> Vec<u8> {
    Req::new(opcode::MAP_WINDOW, 0).u32(window).finish()
}

/// Encode `UnmapWindow` (opcode 10).
pub fn unmap_window(window: Xid) -> Vec<u8> {
    Req::new(opcode::UNMAP_WINDOW, 0).u32(window).finish()
}

/// Stacking order change for [`ConfigureWindow`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StackMode {
    /// Raise to the top.
    Above = 0,
    /// Lower to the bottom.
    Below = 1,
}

/// `ConfigureWindow` changes; `None` leaves a value unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConfigureWindow {
    /// New x.
    pub x: Option<i16>,
    /// New y.
    pub y: Option<i16>,
    /// New width.
    pub width: Option<u16>,
    /// New height.
    pub height: Option<u16>,
    /// New border width.
    pub border_width: Option<u16>,
    /// Restack.
    pub stack_mode: Option<StackMode>,
}

/// Encode `ConfigureWindow` (opcode 12). Each value occupies 4 bytes;
/// signed coordinates are sign-extended.
pub fn configure_window(window: Xid, c: &ConfigureWindow) -> Vec<u8> {
    let mut mask = 0u16;
    let mut vals = Vec::new();
    let mut put = |bit: u16, v: Option<u32>| {
        if let Some(v) = v {
            mask |= bit;
            vals.push(v);
        }
    };
    put(0x01, c.x.map(|v| i32::from(v) as u32));
    put(0x02, c.y.map(|v| i32::from(v) as u32));
    put(0x04, c.width.map(u32::from));
    put(0x08, c.height.map(u32::from));
    put(0x10, c.border_width.map(u32::from));
    put(0x40, c.stack_mode.map(|s| s as u32));
    let mut r = Req::new(opcode::CONFIGURE_WINDOW, 0).u32(window).u16(mask).skip(2);
    for v in vals {
        r = r.u32(v);
    }
    r.finish()
}

/// Encode `GetGeometry` (opcode 14).
pub fn get_geometry(drawable: Xid) -> Vec<u8> {
    Req::new(opcode::GET_GEOMETRY, 0).u32(drawable).finish()
}

/// A `GetGeometry` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    /// Depth.
    pub depth: u8,
    /// Root of the drawable's screen.
    pub root: Xid,
    /// Position relative to the parent (windows).
    pub x: i16,
    /// Position relative to the parent (windows).
    pub y: i16,
    /// Width.
    pub width: u16,
    /// Height.
    pub height: u16,
    /// Border width.
    pub border_width: u16,
}

/// Decode a `GetGeometry` reply.
pub fn parse_geometry(reply: &[u8]) -> Option<Geometry> {
    Some(Geometry {
        depth: reply.u8_at(1)?,
        root: reply.u32_at(8)?,
        x: reply.i16_at(12)?,
        y: reply.i16_at(14)?,
        width: reply.u16_at(16)?,
        height: reply.u16_at(18)?,
        border_width: reply.u16_at(20)?,
    })
}

/// Encode `TranslateCoordinates` (opcode 40).
pub fn translate_coordinates(src: Xid, dst: Xid, x: i16, y: i16) -> Vec<u8> {
    Req::new(opcode::TRANSLATE_COORDINATES, 0).u32(src).u32(dst).i16(x).i16(y).finish()
}

/// Decode a `TranslateCoordinates` reply into `(dst_x, dst_y)`.
pub fn parse_translate_coordinates(reply: &[u8]) -> Option<(i16, i16)> {
    Some((reply.i16_at(12)?, reply.i16_at(14)?))
}

// ============================================================ atoms & props

/// Encode `InternAtom` (opcode 16).
pub fn intern_atom(name: &str, only_if_exists: bool) -> Vec<u8> {
    let name = name.as_bytes();
    Req::new(opcode::INTERN_ATOM, u8::from(only_if_exists)).u16(name.len().min(0xffff) as u16).skip(2).padded(name).finish()
}

/// Decode an `InternAtom` reply (`0` = the atom does not exist).
pub fn parse_intern_atom(reply: &[u8]) -> Option<Atom> {
    reply.u32_at(8)
}

/// How `ChangeProperty` combines new data with the old.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropMode {
    /// Replace the value.
    Replace = 0,
    /// Insert before the value.
    Prepend = 1,
    /// Append after the value.
    Append = 2,
}

/// Property data in one of the three formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropData<'a> {
    /// Format 8: bytes (strings).
    U8(&'a [u8]),
    /// Format 16.
    U16(&'a [u16]),
    /// Format 32: atoms, windows, cardinals.
    U32(&'a [u32]),
}

/// Encode `ChangeProperty` (opcode 18).
pub fn change_property(mode: PropMode, window: Xid, property: Atom, type_: Atom, data: PropData<'_>) -> Vec<u8> {
    let (format, count, bytes): (u8, usize, Vec<u8>) = match data {
        PropData::U8(d) => (8, d.len(), d.to_vec()),
        PropData::U16(d) => (16, d.len(), d.iter().flat_map(|v| v.to_le_bytes()).collect()),
        PropData::U32(d) => (32, d.len(), d.iter().flat_map(|v| v.to_le_bytes()).collect()),
    };
    Req::with_capacity(opcode::CHANGE_PROPERTY, mode as u8, 24 + bytes.len() + 3)
        .u32(window)
        .u32(property)
        .u32(type_)
        .u8(format)
        .skip(3)
        .u32(count as u32)
        .padded(&bytes)
        .finish()
}

/// Encode `DeleteProperty` (opcode 19).
pub fn delete_property(window: Xid, property: Atom) -> Vec<u8> {
    Req::new(opcode::DELETE_PROPERTY, 0).u32(window).u32(property).finish()
}

/// Encode `GetProperty` (opcode 20). `type_ = 0` accepts any type;
/// `long_offset`/`long_length` are in 4-byte units.
pub fn get_property(delete: bool, window: Xid, property: Atom, type_: Atom, long_offset: u32, long_length: u32) -> Vec<u8> {
    Req::new(opcode::GET_PROPERTY, u8::from(delete)).u32(window).u32(property).u32(type_).u32(long_offset).u32(long_length).finish()
}

/// A decoded `GetProperty` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    /// 0 when the property does not exist, else 8, 16 or 32.
    pub format: u8,
    /// The property's type atom (0 when it does not exist).
    pub type_: Atom,
    /// Bytes not returned because of `long_length`.
    pub bytes_after: u32,
    /// The value's raw little-endian bytes.
    pub value: Vec<u8>,
}

impl Property {
    /// The value as 32-bit items (format 32), or `None`.
    pub fn as_u32s(&self) -> Option<Vec<u32>> {
        (self.format == 32).then(|| self.value.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
    }
}

/// Decode a `GetProperty` reply.
pub fn parse_property(reply: &[u8]) -> Option<Property> {
    let format = reply.u8_at(1)?;
    let items = reply.u32_at(16)? as usize;
    let unit = match format {
        0 => 0,
        8 => 1,
        16 => 2,
        32 => 4,
        _ => return None,
    };
    let len = items.checked_mul(unit)?;
    Some(Property { format, type_: reply.u32_at(8)?, bytes_after: reply.u32_at(12)?, value: reply.get(32..32usize.checked_add(len)?)?.to_vec() })
}

// ================================================================ events

/// Encode `SendEvent` (opcode 25): deliver a 32-byte `event` to
/// `destination` for clients selecting `event_mask` (or, with an empty
/// mask, to the client that created the window).
pub fn send_event(propagate: bool, destination: Xid, event_mask: u32, event: &[u8; EVENT_LEN]) -> Vec<u8> {
    Req::new(opcode::SEND_EVENT, u8::from(propagate)).u32(destination).u32(event_mask).bytes(event).finish()
}

/// Build a 32-byte `ClientMessage` event (format 32) for [`send_event`].
pub fn client_message(window: Xid, message_type: Atom, data: [u32; 5]) -> [u8; EVENT_LEN] {
    let mut e = [0u8; EVENT_LEN];
    e[0] = super::event::code::CLIENT_MESSAGE;
    e[1] = 32;
    e[4..8].copy_from_slice(&window.to_le_bytes());
    e[8..12].copy_from_slice(&message_type.to_le_bytes());
    for (i, d) in data.iter().enumerate() {
        e[12 + i * 4..16 + i * 4].copy_from_slice(&d.to_le_bytes());
    }
    e
}

/// Encode `UngrabPointer` (opcode 27) at `time` (0 = CurrentTime).
pub fn ungrab_pointer(time: u32) -> Vec<u8> {
    Req::new(opcode::UNGRAB_POINTER, 0).u32(time).finish()
}

/// `revert-to` for [`set_input_focus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevertTo {
    /// Revert to no focus.
    None = 0,
    /// Revert to the pointer root.
    PointerRoot = 1,
    /// Revert to the parent.
    Parent = 2,
}

/// Encode `SetInputFocus` (opcode 42). Only valid once `window` is
/// viewable; earlier, a server answers `BadMatch`.
pub fn set_input_focus(window: Xid, revert_to: RevertTo, time: u32) -> Vec<u8> {
    Req::new(opcode::SET_INPUT_FOCUS, revert_to as u8).u32(window).u32(time).finish()
}

/// Encode `GetInputFocus` (opcode 43). Its tiny reply makes it the classic
/// round-trip "sync" request.
pub fn get_input_focus() -> Vec<u8> {
    Req::new(opcode::GET_INPUT_FOCUS, 0).finish()
}

/// Encode `NoOperation` (opcode 127).
pub fn no_operation() -> Vec<u8> {
    Req::new(opcode::NO_OPERATION, 0).finish()
}

/// Encode `Bell` (opcode 104); `percent` in `-100..=100`.
pub fn bell(percent: i8) -> Vec<u8> {
    Req::new(opcode::BELL, percent as u8).finish()
}

// ============================================================ drawing

/// Encode `CreatePixmap` (opcode 53).
pub fn create_pixmap(pid: Xid, drawable: Xid, depth: u8, width: u16, height: u16) -> Vec<u8> {
    Req::new(opcode::CREATE_PIXMAP, depth).u32(pid).u32(drawable).u16(width).u16(height).finish()
}

/// Encode `FreePixmap` (opcode 54).
pub fn free_pixmap(pid: Xid) -> Vec<u8> {
    Req::new(opcode::FREE_PIXMAP, 0).u32(pid).finish()
}

/// Encode `CreateGC` (opcode 55) with `graphics-exposures` off: protolith
/// repaints from its own backing pixmap, so it never wants
/// `GraphicsExpose`/`NoExpose` after a `CopyArea`.
pub fn create_gc(cid: Xid, drawable: Xid) -> Vec<u8> {
    const GRAPHICS_EXPOSURES: u32 = 1 << 16;
    Req::new(opcode::CREATE_GC, 0).u32(cid).u32(drawable).u32(GRAPHICS_EXPOSURES).u32(0).finish()
}

/// Encode `FreeGC` (opcode 60).
pub fn free_gc(gc: Xid) -> Vec<u8> {
    Req::new(opcode::FREE_GC, 0).u32(gc).finish()
}

/// Encode `PolyFillRectangle` (opcode 70): fill `rects` with the GC's
/// foreground (pixel 0, black, for a GC created by [`create_gc`]).
pub fn poly_fill_rectangle(drawable: Xid, gc: Xid, rects: &[Rect]) -> Vec<u8> {
    let mut r = Req::with_capacity(opcode::POLY_FILL_RECTANGLE, 0, 12 + rects.len() * 8).u32(drawable).u32(gc);
    for rc in rects {
        r = r
            .i16(rc.x.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
            .i16(rc.y.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
            .u16(rc.width.min(u32::from(u16::MAX)) as u16)
            .u16(rc.height.min(u32::from(u16::MAX)) as u16);
    }
    r.finish()
}

/// Encode `CopyArea` (opcode 62).
#[allow(clippy::too_many_arguments)]
pub fn copy_area(src: Xid, dst: Xid, gc: Xid, src_x: i16, src_y: i16, dst_x: i16, dst_y: i16, width: u16, height: u16) -> Vec<u8> {
    Req::new(opcode::COPY_AREA, 0).u32(src).u32(dst).u32(gc).i16(src_x).i16(src_y).i16(dst_x).i16(dst_y).u16(width).u16(height).finish()
}

/// Bytes in a `PutImage` request before the image data.
pub const PUT_IMAGE_HEADER_LEN: usize = 24;
/// `ZPixmap` image format.
pub const FORMAT_ZPIXMAP: u8 = 2;

/// Encode one `PutImage` (opcode 72, `ZPixmap`). `data` is `height` rows
/// already in the drawable's wire layout, each padded to the format's
/// scanline pad.
#[allow(clippy::too_many_arguments)]
pub fn put_image(drawable: Xid, gc: Xid, width: u16, height: u16, dst_x: i16, dst_y: i16, depth: u8, data: &[u8]) -> Vec<u8> {
    Req::with_capacity(opcode::PUT_IMAGE, FORMAT_ZPIXMAP, PUT_IMAGE_HEADER_LEN + data.len() + 3)
        .u32(drawable)
        .u32(gc)
        .u16(width)
        .u16(height)
        .i16(dst_x)
        .i16(dst_y)
        .u8(0) // left-pad
        .u8(depth)
        .skip(2)
        .padded(data)
        .finish()
}

/// Split an image of `rows` rows of `row_stride` bytes into the fewest
/// `PutImage` bands that each fit `max_request_words` (the server's
/// maximum request length). Returns `(first_row, row_count)` per band.
///
/// Total: an empty image yields no bands; a limit too small for even one
/// row still yields one row per band (the server will refuse those with
/// `BadLength`, reported as an error, rather than the client looping).
pub fn put_image_bands(rows: usize, row_stride: usize, max_request_words: u16) -> Vec<(usize, usize)> {
    if rows == 0 || row_stride == 0 {
        return Vec::new();
    }
    let budget = (usize::from(max_request_words) * 4).saturating_sub(PUT_IMAGE_HEADER_LEN);
    let per_band = (budget / row_stride).max(1);
    let mut out = Vec::with_capacity(rows.div_ceil(per_band));
    let mut row = 0;
    while row < rows {
        let n = per_band.min(rows - row);
        out.push((row, n));
        row += n;
    }
    out
}

/// Encode `GetImage` (opcode 73), `ZPixmap`, all planes.
pub fn get_image(drawable: Xid, x: i16, y: i16, width: u16, height: u16) -> Vec<u8> {
    Req::new(opcode::GET_IMAGE, FORMAT_ZPIXMAP).u32(drawable).i16(x).i16(y).u16(width).u16(height).u32(u32::MAX).finish()
}

/// Decode a `GetImage` reply into `(depth, visual, data)`.
pub fn parse_image(reply: &[u8]) -> Option<(u8, u32, &[u8])> {
    let len = (reply.u32_at(4)? as usize).checked_mul(4)?;
    Some((reply.u8_at(1)?, reply.u32_at(8)?, reply.get(32..32usize.checked_add(len)?)?))
}

/// Encode `CreateColormap` (opcode 78), `alloc = None`.
pub fn create_colormap(mid: Xid, window: Xid, visual: u32) -> Vec<u8> {
    Req::new(opcode::CREATE_COLORMAP, 0).u32(mid).u32(window).u32(visual).finish()
}

/// Encode `FreeColormap` (opcode 79).
pub fn free_colormap(cmap: Xid) -> Vec<u8> {
    Req::new(opcode::FREE_COLORMAP, 0).u32(cmap).finish()
}

// ========================================================= extensions

/// Encode `QueryExtension` (opcode 98).
pub fn query_extension(name: &str) -> Vec<u8> {
    let name = name.as_bytes();
    Req::new(opcode::QUERY_EXTENSION, 0).u16(name.len().min(0xffff) as u16).skip(2).padded(name).finish()
}

/// A `QueryExtension` reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtensionInfo {
    /// The extension's major opcode.
    pub major_opcode: u8,
    /// Its first event code.
    pub first_event: u8,
    /// Its first error code.
    pub first_error: u8,
}

/// Decode a `QueryExtension` reply; `Ok(None)` when absent.
pub fn parse_query_extension(reply: &[u8]) -> Option<Option<ExtensionInfo>> {
    let present = reply.u8_at(8)? != 0;
    let info = ExtensionInfo { major_opcode: reply.u8_at(9)?, first_event: reply.u8_at(10)?, first_error: reply.u8_at(11)? };
    Some(present.then_some(info))
}

/// The SHAPE extension (non-rectangular windows).
pub mod shape {
    use super::{Rect, Req, Xid};
    use crate::x11::proto::ReadLe;

    /// Extension name for `QueryExtension`.
    pub const NAME: &str = "SHAPE";

    /// Which region of a window a shape request changes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        /// The window's outline (what is drawn and what takes input).
        Bounding = 0,
        /// The region the window's contents are clipped to.
        Clip = 1,
        /// The region that receives pointer input (SHAPE 1.1).
        Input = 2,
    }

    /// Encode `ShapeRectangles` (minor 1) with operation `Set` and
    /// `Unsorted` ordering. Rectangles are clamped to the 16-bit wire range.
    pub fn rectangles(major: u8, kind: Kind, window: Xid, x_off: i16, y_off: i16, rects: &[Rect]) -> Vec<u8> {
        let mut r = Req::with_capacity(major, 1, 16 + rects.len() * 8).u8(0).u8(kind as u8).u8(0).skip(1).u32(window).i16(x_off).i16(y_off);
        for rc in rects {
            r = r
                .i16(rc.x.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
                .i16(rc.y.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
                .u16(rc.width.min(u32::from(u16::MAX)) as u16)
                .u16(rc.height.min(u32::from(u16::MAX)) as u16);
        }
        r.finish()
    }

    /// Encode `ShapeMask` (minor 2) with source pixmap `None`, operation
    /// `Set`: removes the shape, making the window rectangular again.
    pub fn clear(major: u8, kind: Kind, window: Xid) -> Vec<u8> {
        Req::new(major, 2).u8(0).u8(kind as u8).skip(2).u32(window).i16(0).i16(0).u32(0).finish()
    }

    /// Encode `ShapeGetRectangles` (minor 8).
    pub fn get_rectangles(major: u8, window: Xid, kind: Kind) -> Vec<u8> {
        Req::new(major, 8).u32(window).u8(kind as u8).skip(3).finish()
    }

    /// Decode a `ShapeGetRectangles` reply.
    pub fn parse_rectangles(reply: &[u8]) -> Option<Vec<Rect>> {
        let n = reply.u32_at(8)? as usize;
        let mut out = Vec::with_capacity(n.min(4096));
        for i in 0..n {
            let o = 32usize.checked_add(i.checked_mul(8)?)?;
            out.push(Rect::new(i32::from(reply.i16_at(o)?), i32::from(reply.i16_at(o + 2)?), u32::from(reply.u16_at(o + 4)?), u32::from(reply.u16_at(o + 6)?)));
        }
        Some(out)
    }
}

/// The XKEYBOARD extension, for the one thing protolith wants from it:
/// detectable autorepeat.
pub mod xkb {
    use super::Req;
    use crate::x11::proto::ReadLe;

    /// Extension name for `QueryExtension`.
    pub const NAME: &str = "XKEYBOARD";
    /// `XkbUseCoreKbd` device spec.
    pub const USE_CORE_KBD: u16 = 0x0100;
    /// `XkbPCF_DetectableAutoRepeatMask`.
    pub const DETECTABLE_AUTO_REPEAT: u32 = 1;

    /// Encode `XkbUseExtension` (minor 0), which must precede any other
    /// XKB request; asks for XKB 1.0.
    pub fn use_extension(major: u8) -> Vec<u8> {
        Req::new(major, 0).u16(1).u16(0).finish()
    }

    /// Decode an `XkbUseExtension` reply: is the version supported?
    pub fn parse_use_extension(reply: &[u8]) -> Option<bool> {
        reply.u8_at(1).map(|s| s != 0)
    }

    /// Encode `XkbPerClientFlags` (minor 21) setting `flags` in `change`.
    pub fn per_client_flags(major: u8, change: u32, value: u32) -> Vec<u8> {
        Req::new(major, 21).u16(USE_CORE_KBD).skip(2).u32(change).u32(value).u32(0).u32(0).u32(0).finish()
    }

    /// Decode an `XkbPerClientFlags` reply into `(supported, value)`.
    pub fn parse_per_client_flags(reply: &[u8]) -> Option<(u32, u32)> {
        Some((reply.u32_at(8)?, reply.u32_at(12)?))
    }
}

// =========================================================== keyboard

/// Encode `GetKeyboardMapping` (opcode 101).
pub fn get_keyboard_mapping(first_keycode: u8, count: u8) -> Vec<u8> {
    Req::new(opcode::GET_KEYBOARD_MAPPING, 0).u8(first_keycode).u8(count).skip(2).finish()
}

/// Decode a `GetKeyboardMapping` reply into `(keysyms_per_keycode,
/// keysyms)`. The keysym count comes from the reply's own length field.
pub fn parse_keyboard_mapping(reply: &[u8]) -> Option<(u8, Vec<u32>)> {
    let per = reply.u8_at(1)?;
    let n = reply.u32_at(4)? as usize;
    let body = reply.get(32..32usize.checked_add(n.checked_mul(4)?)?)?;
    Some((per, body.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()))
}

/// Encode `GetModifierMapping` (opcode 119).
pub fn get_modifier_mapping() -> Vec<u8> {
    Req::new(opcode::GET_MODIFIER_MAPPING, 0).finish()
}

/// Decode a `GetModifierMapping` reply into eight lists of keycodes (Shift,
/// Lock, Control, Mod1..Mod5), zero keycodes dropped.
pub fn parse_modifier_mapping(reply: &[u8]) -> Option<[Vec<u8>; 8]> {
    let per = usize::from(reply.u8_at(1)?);
    let body = reply.get(32..32 + 8 * per)?;
    let mut out: [Vec<u8>; 8] = Default::default();
    for (m, slot) in out.iter_mut().enumerate() {
        *slot = body[m * per..(m + 1) * per].iter().copied().filter(|&k| k != 0).collect();
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(r: &[u8]) -> usize {
        usize::from(u16::from_le_bytes([r[2], r[3]]))
    }

    /// Every encoder's length field must match its byte length.
    fn check_len(r: &[u8]) {
        assert_eq!(r.len() % 4, 0, "padded");
        assert_eq!(words(r) * 4, r.len(), "length field");
    }

    #[test]
    fn create_window_value_list_is_in_bit_order() {
        let attrs = WindowAttributes {
            background_pixmap: Some(0),
            border_pixel: Some(0),
            bit_gravity: Some(1),
            event_mask: Some(0x0020_8001),
            colormap: Some(0x77),
            ..Default::default()
        };
        let r = create_window(&CreateWindow {
            wid: 0x0040_0001,
            parent: 0x42,
            depth: 24,
            x: -3,
            y: 4,
            width: 275,
            height: 116,
            border_width: 0,
            class: WindowClass::InputOutput,
            visual: 0x21,
            attributes: attrs,
        });
        check_len(&r);
        assert_eq!(r[0], 1);
        assert_eq!(r[1], 24);
        assert_eq!(&r[4..8], &0x0040_0001u32.to_le_bytes());
        assert_eq!(&r[8..12], &0x42u32.to_le_bytes());
        assert_eq!(&r[12..14], &(-3i16).to_le_bytes());
        assert_eq!(&r[16..20], &[19, 1, 116, 0]);
        assert_eq!(&r[22..24], &1u16.to_le_bytes(), "InputOutput");
        assert_eq!(&r[24..28], &0x21u32.to_le_bytes());
        let mask = u32::from_le_bytes(r[28..32].try_into().unwrap());
        assert_eq!(mask, 0x0001 | 0x0008 | 0x0010 | 0x0800 | 0x2000);
        let vals: Vec<u32> = r[32..].chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(vals, [0, 0, 1, 0x0020_8001, 0x77]);
    }

    #[test]
    fn simple_window_requests() {
        for (r, op) in [
            (map_window(9), 8),
            (unmap_window(9), 10),
            (destroy_window(9), 4),
            (get_geometry(9), 14),
            (free_pixmap(9), 54),
            (free_gc(9), 60),
            (free_colormap(9), 79),
            (get_window_attributes(9), 3),
        ] {
            check_len(&r);
            assert_eq!(r, [op, 0, 2, 0, 9, 0, 0, 0]);
        }
    }

    #[test]
    fn change_window_attributes_override_redirect() {
        let r = change_window_attributes(5, &WindowAttributes { override_redirect: Some(true), ..Default::default() });
        check_len(&r);
        assert_eq!(r, [2, 0, 4, 0, 5, 0, 0, 0, 0x00, 0x02, 0, 0, 1, 0, 0, 0]);
    }

    #[test]
    fn configure_window_sign_extends_coordinates() {
        let r = configure_window(7, &ConfigureWindow { x: Some(-10), height: Some(300), stack_mode: Some(StackMode::Above), ..Default::default() });
        check_len(&r);
        assert_eq!(&r[8..10], &(0x01u16 | 0x08 | 0x40).to_le_bytes());
        assert_eq!(&r[12..16], &(-10i32).to_le_bytes());
        assert_eq!(&r[16..20], &300u32.to_le_bytes());
        assert_eq!(&r[20..24], &0u32.to_le_bytes());
        assert_eq!(r.len(), 24);
    }

    #[test]
    fn intern_atom_and_reply() {
        let r = intern_atom("WM_PROTOCOLS", false);
        check_len(&r);
        assert_eq!(&r[..8], &[16, 0, 5, 0, 12, 0, 0, 0]);
        assert_eq!(&r[8..20], b"WM_PROTOCOLS");
        assert_eq!(intern_atom("X", true)[1], 1);
        let mut reply = [0u8; 32];
        reply[8..12].copy_from_slice(&321u32.to_le_bytes());
        assert_eq!(parse_intern_atom(&reply), Some(321));
        assert_eq!(parse_intern_atom(&reply[..11]), None);
    }

    #[test]
    fn change_property_formats() {
        let r = change_property(PropMode::Replace, 3, 39, 31, PropData::U8(b"hello"));
        check_len(&r);
        assert_eq!(&r[..4], &[18, 0, 8, 0]);
        assert_eq!(r[16], 8, "format");
        assert_eq!(&r[20..24], &5u32.to_le_bytes(), "item count");
        assert_eq!(&r[24..29], b"hello");
        let r = change_property(PropMode::Append, 3, 1, 4, PropData::U32(&[0xdead_beef, 1]));
        check_len(&r);
        assert_eq!(r[1], 2);
        assert_eq!(r[16], 32);
        assert_eq!(&r[20..24], &2u32.to_le_bytes());
        assert_eq!(&r[24..28], &0xdead_beefu32.to_le_bytes());
        let r = change_property(PropMode::Prepend, 3, 1, 4, PropData::U16(&[0x1234]));
        check_len(&r);
        assert_eq!(&r[24..26], &0x1234u16.to_le_bytes());
    }

    #[test]
    fn get_property_round_trip() {
        let r = get_property(false, 3, 39, 0, 0, 1024);
        check_len(&r);
        assert_eq!(r.len(), 24);
        let mut reply = vec![1u8, 32, 0, 0, 2, 0, 0, 0];
        reply.extend_from_slice(&4u32.to_le_bytes()); // type
        reply.extend_from_slice(&0u32.to_le_bytes()); // bytes after
        reply.extend_from_slice(&2u32.to_le_bytes()); // items
        reply.resize(32, 0);
        reply.extend_from_slice(&7u32.to_le_bytes());
        reply.extend_from_slice(&9u32.to_le_bytes());
        let p = parse_property(&reply).unwrap();
        assert_eq!((p.format, p.type_), (32, 4));
        assert_eq!(p.as_u32s(), Some(vec![7, 9]));
        assert_eq!(parse_property(&reply[..39]), None, "truncated value");
        reply[1] = 7;
        assert_eq!(parse_property(&reply), None, "bad format");
    }

    #[test]
    fn missing_property_parses_as_format_zero() {
        let reply = [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let p = parse_property(&reply).unwrap();
        assert_eq!(p.format, 0);
        assert!(p.value.is_empty());
        assert_eq!(p.as_u32s(), None);
    }

    #[test]
    fn send_event_wraps_a_client_message() {
        let ev = client_message(0x99, 0x123, [1, 2, 3, 4, 5]);
        assert_eq!(ev[0], 33);
        assert_eq!(ev[1], 32);
        assert_eq!(&ev[4..8], &0x99u32.to_le_bytes());
        assert_eq!(&ev[8..12], &0x123u32.to_le_bytes());
        assert_eq!(&ev[28..32], &5u32.to_le_bytes());
        let r = send_event(false, 0x1, 0x0018_0000, &ev);
        check_len(&r);
        assert_eq!(r.len(), 44);
        assert_eq!(&r[8..12], &0x0018_0000u32.to_le_bytes());
        assert_eq!(&r[12..], &ev);
    }

    #[test]
    fn focus_and_sync_requests() {
        assert_eq!(set_input_focus(0x0140_0001, RevertTo::Parent, 0), [42, 2, 3, 0, 0x01, 0x00, 0x40, 0x01, 0, 0, 0, 0]);
        assert_eq!(get_input_focus(), [43, 0, 1, 0]);
        assert_eq!(no_operation(), [127, 0, 1, 0]);
        assert_eq!(bell(-50), [104, 0xce, 1, 0]);
        assert_eq!(ungrab_pointer(0), [27, 0, 2, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn create_gc_turns_off_graphics_exposures() {
        let r = create_gc(2, 1);
        check_len(&r);
        assert_eq!(&r[12..16], &0x0001_0000u32.to_le_bytes());
        assert_eq!(&r[16..20], &0u32.to_le_bytes());
    }

    #[test]
    fn copy_area_and_pixmap_bytes() {
        let r = copy_area(1, 2, 3, -5, 60, 7, 0, 1024, 708);
        check_len(&r);
        assert_eq!(r.len(), 28);
        assert_eq!(&r[16..18], &(-5i16).to_le_bytes());
        assert_eq!(&r[24..28], &[0x00, 0x04, 0xc4, 0x02]);
        let r = create_pixmap(4, 1, 24, 275, 116);
        check_len(&r);
        assert_eq!(r[1], 24);
        assert_eq!(&r[12..16], &[19, 1, 116, 0]);
    }

    #[test]
    fn poly_fill_rectangle_bytes() {
        let r = poly_fill_rectangle(1, 2, &[Rect::new(0, 0, 275, 116)]);
        check_len(&r);
        assert_eq!(r, [70, 0, 5, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 19, 1, 116, 0]);
    }

    #[test]
    fn put_image_pads_data() {
        let r = put_image(1, 2, 2, 1, -1, 5, 24, &[1, 2, 3, 4, 5]);
        check_len(&r);
        assert_eq!(r[1], FORMAT_ZPIXMAP);
        assert_eq!(&r[16..18], &(-1i16).to_le_bytes());
        assert_eq!(&r[18..20], &5i16.to_le_bytes());
        assert_eq!((r[20], r[21]), (0, 24));
        assert_eq!(&r[24..29], &[1, 2, 3, 4, 5]);
        assert_eq!(r.len(), 32);
    }

    #[test]
    fn bands_respect_the_request_limit() {
        // 24-byte header + 2 rows of 16 = 56 bytes fits 15 words; 3 rows does not.
        let bands = put_image_bands(10, 16, 15);
        assert_eq!(bands, [(0, 2), (2, 2), (4, 2), (6, 2), (8, 2)]);
        for (_, n) in &bands {
            assert!(PUT_IMAGE_HEADER_LEN + n * 16 <= 15 * 4);
        }
        assert_eq!(put_image_bands(4, 8, 65535), [(0, 4)]);
        assert_eq!(put_image_bands(3, 100, 1), [(0, 1), (1, 1), (2, 1)], "always progresses");
        assert!(put_image_bands(0, 8, 100).is_empty());
        assert!(put_image_bands(5, 0, 100).is_empty());
    }

    #[test]
    fn get_image_and_reply() {
        let r = get_image(9, 1, 2, 3, 4);
        check_len(&r);
        assert_eq!(r[1], 2);
        assert_eq!(&r[16..20], &u32::MAX.to_le_bytes());
        let mut reply = vec![1u8, 24, 0, 0, 1, 0, 0, 0, 0x21, 0, 0, 0];
        reply.resize(32, 0);
        reply.extend_from_slice(&[9, 8, 7, 6]);
        assert_eq!(parse_image(&reply), Some((24, 0x21, &[9u8, 8, 7, 6][..])));
        assert_eq!(parse_image(&reply[..35]), None);
    }

    #[test]
    fn query_extension_round_trip() {
        let r = query_extension("SHAPE");
        check_len(&r);
        assert_eq!(&r[4..6], &5u16.to_le_bytes());
        assert_eq!(&r[8..13], b"SHAPE");
        let mut reply = [0u8; 32];
        reply[8..12].copy_from_slice(&[1, 129, 64, 128]);
        assert_eq!(parse_query_extension(&reply), Some(Some(ExtensionInfo { major_opcode: 129, first_event: 64, first_error: 128 })));
        reply[8] = 0;
        assert_eq!(parse_query_extension(&reply), Some(None));
    }

    #[test]
    fn shape_requests() {
        let r = shape::rectangles(129, shape::Kind::Bounding, 7, 0, 0, &[Rect::new(1, 2, 3, 4), Rect::new(-70_000, 0, 100_000, 1)]);
        check_len(&r);
        assert_eq!(&r[..8], &[129, 1, 8, 0, 0, 0, 0, 0], "op Set, kind Bounding, Unsorted");
        assert_eq!(&r[8..12], &7u32.to_le_bytes());
        assert_eq!(&r[16..24], &[1, 0, 2, 0, 3, 0, 4, 0]);
        assert_eq!(i16::from_le_bytes([r[24], r[25]]), i16::MIN, "clamped");
        assert_eq!(u16::from_le_bytes([r[28], r[29]]), u16::MAX, "clamped");
        let r = shape::clear(129, shape::Kind::Bounding, 7);
        check_len(&r);
        assert_eq!(r, [129, 2, 5, 0, 0, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        let r = shape::get_rectangles(129, 7, shape::Kind::Bounding);
        check_len(&r);
        let mut reply = vec![1u8, 0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0];
        reply.resize(32, 0);
        reply.extend_from_slice(&[5, 0, 6, 0, 7, 0, 8, 0]);
        assert_eq!(shape::parse_rectangles(&reply), Some(vec![Rect::new(5, 6, 7, 8)]));
        reply[8] = 2;
        assert_eq!(shape::parse_rectangles(&reply), None, "truncated");
    }

    #[test]
    fn xkb_requests() {
        let r = xkb::use_extension(135);
        check_len(&r);
        assert_eq!(r, [135, 0, 2, 0, 1, 0, 0, 0]);
        let r = xkb::per_client_flags(135, 1, 1);
        check_len(&r);
        assert_eq!(r.len(), 28);
        assert_eq!(&r[..12], &[135, 21, 7, 0, 0, 1, 0, 0, 1, 0, 0, 0]);
        assert_eq!(&r[12..16], &1u32.to_le_bytes());
        let mut reply = vec![1u8, 1, 0, 0, 0, 0, 0, 0];
        reply.extend_from_slice(&1u32.to_le_bytes());
        reply.extend_from_slice(&1u32.to_le_bytes());
        reply.resize(32, 0);
        assert_eq!(xkb::parse_per_client_flags(&reply), Some((1, 1)));
        assert_eq!(xkb::parse_use_extension(&reply), Some(true));
    }

    #[test]
    fn keyboard_mapping_round_trip() {
        assert_eq!(get_keyboard_mapping(8, 248), [101, 0, 2, 0, 8, 248, 0, 0]);
        let mut reply = vec![1u8, 2, 0, 0, 4, 0, 0, 0];
        reply.resize(32, 0);
        for k in [0x61u32, 0x41, 0x62, 0x42] {
            reply.extend_from_slice(&k.to_le_bytes());
        }
        assert_eq!(parse_keyboard_mapping(&reply), Some((2, vec![0x61, 0x41, 0x62, 0x42])));
        assert_eq!(parse_keyboard_mapping(&reply[..40]), None);
    }

    #[test]
    fn modifier_mapping_round_trip() {
        assert_eq!(get_modifier_mapping(), [119, 0, 1, 0]);
        let mut reply = vec![1u8, 2, 0, 0, 4, 0, 0, 0];
        reply.resize(32, 0);
        reply.extend_from_slice(&[50, 62, 66, 0, 37, 105, 64, 0, 77, 0, 0, 0, 0, 0, 133, 0]);
        let m = parse_modifier_mapping(&reply).unwrap();
        assert_eq!(m[0], [50, 62]);
        assert_eq!(m[1], [66]);
        assert_eq!(m[4], [77], "Mod2 = NumLock");
        assert_eq!(m[7], [133]);
        assert_eq!(parse_modifier_mapping(&reply[..40]), None);
    }

    #[test]
    fn geometry_translate_attributes_replies() {
        let mut g = vec![1u8, 24, 0, 0, 0, 0, 0, 0];
        g.extend_from_slice(&0x42u32.to_le_bytes());
        g.extend_from_slice(&(-5i16).to_le_bytes());
        g.extend_from_slice(&6i16.to_le_bytes());
        g.extend_from_slice(&275u16.to_le_bytes());
        g.extend_from_slice(&116u16.to_le_bytes());
        g.extend_from_slice(&0u16.to_le_bytes());
        g.resize(32, 0);
        assert_eq!(parse_geometry(&g), Some(Geometry { depth: 24, root: 0x42, x: -5, y: 6, width: 275, height: 116, border_width: 0 }));
        assert_eq!(parse_translate_coordinates(&g), Some((-5, 6)), "dst-x/dst-y share GetGeometry's x/y offsets");
        let mut a = vec![0u8; 44];
        a[26] = 2;
        a[27] = 1;
        a[32..36].copy_from_slice(&(1u32 << 20).to_le_bytes());
        let attrs = parse_window_attributes(&a).unwrap();
        assert_eq!(attrs.map_state, MapState::Viewable);
        assert!(attrs.override_redirect);
        assert_eq!(attrs.all_event_masks, 1 << 20);
        assert_eq!(parse_window_attributes(&a[..39]), None);
    }

    #[test]
    fn opcode_names() {
        assert_eq!(opcode_name(72), "PutImage");
        assert_eq!(opcode_name(200), "an extension request");
        assert_eq!(opcode_name(5), "a core request");
    }
}
