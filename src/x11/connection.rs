//! [`Connection`]: windows you present pixels to, and the events they get.

use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::client::{Client, Message};
use super::display::{hostname, Auth, DisplayName};
use super::keyboard::{Key, Keymap, Modifiers};
use super::proto::event::{self as wire, state, InputEvent, RawEvent};
use super::proto::request::{self as req, event_mask, shape as shape_req, PropData, PropMode, WindowAttributes, WindowClass};
use super::proto::setup::{Format, Visual};
use super::proto::{atom, Atom, Keysym, XError, Xid};
use crate::error::{Error, Result};
use crate::pixel::{Frame, PixelFormat, Rect};

/// A window created by a [`Connection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(pub(crate) Xid);

impl WindowId {
    /// The X window id (for use with raw requests).
    pub fn xid(&self) -> Xid {
        self.0
    }

    /// The id of an X window, for building [`Event`]s in tests of code
    /// that handles them. Only ids from [`Connection::create_window`]
    /// name windows a connection can use; for any other, its methods
    /// return an error.
    pub const fn from_xid(xid: Xid) -> WindowId {
        WindowId(xid)
    }
}

/// How a new window should look and behave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowOptions {
    width: u32,
    height: u32,
    title: String,
    class: (String, String),
    position: Option<(i32, i32)>,
    decorations: bool,
    resizable: bool,
    always_on_top: bool,
    visible: bool,
    override_redirect: bool,
    transient_for: Option<WindowId>,
    frame: bool,
    parent: Option<Xid>,
}

impl WindowOptions {
    /// A visible, decorated, resizable `width × height` window titled
    /// "protolith".
    pub fn new(width: u32, height: u32) -> WindowOptions {
        WindowOptions {
            width,
            height,
            title: "protolith".into(),
            class: ("protolith".into(), "Protolith".into()),
            position: None,
            decorations: true,
            resizable: true,
            always_on_top: false,
            visible: true,
            override_redirect: false,
            transient_for: None,
            frame: false,
            parent: None,
        }
    }

    /// Window title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// `WM_CLASS` (instance, class), used by window managers for rules.
    pub fn class(mut self, instance: impl Into<String>, class: impl Into<String>) -> Self {
        self.class = (instance.into(), class.into());
        self
    }

    /// Initial position (otherwise the window manager places it): where
    /// the window itself goes, its frame (if the window manager adds one)
    /// around it.
    pub fn position(mut self, x: i32, y: i32) -> Self {
        self.position = Some((x, y));
        self
    }

    /// Title bar and borders from the window manager (`_MOTIF_WM_HINTS`).
    /// Turn off to draw your own, as skinned players do; move the window
    /// with [`Connection::begin_drag_move`].
    pub fn decorations(mut self, on: bool) -> Self {
        self.decorations = on;
        self
    }

    /// Whether the user may resize the window (min size = max size when
    /// off).
    pub fn resizable(mut self, on: bool) -> Self {
        self.resizable = on;
        self
    }

    /// Ask to be kept above other windows (`_NET_WM_STATE_ABOVE`).
    pub fn always_on_top(mut self, on: bool) -> Self {
        self.always_on_top = on;
        self
    }

    /// Map the window immediately (default). Create it hidden to set a
    /// shape or icon first, then [`Connection::show`] it.
    pub fn visible(mut self, on: bool) -> Self {
        self.visible = on;
        self
    }

    /// Bypass the window manager entirely (popup menus, tooltips). Such a
    /// window gets no decorations, placement, or focus from a WM.
    pub fn override_redirect(mut self, on: bool) -> Self {
        self.override_redirect = on;
        self
    }

    /// Create the window inside `parent` (any window, this client's or
    /// not), at [`position`](Self::position) relative to it: a part of a
    /// larger window, such as a window manager's title bar inside its
    /// frame. Such a window is no top-level, so it gets none of the window
    /// manager properties (title, class, hints).
    pub fn parent(mut self, parent: Xid) -> Self {
        self.parent = Some(parent);
        self
    }

    /// A window manager's frame: requests by windows reparented into it
    /// (to map, move or resize themselves) are redirected to this
    /// connection, and their unmapping and destruction reported to it, as
    /// [`Event::Foreign`] (after [`Connection::become_window_manager`]).
    pub fn frame(mut self, on: bool) -> Self {
        self.frame = on;
        self
    }

    /// Mark the window as belonging to `owner` (`WM_TRANSIENT_FOR`): a
    /// program's secondary windows, which window managers keep off the
    /// taskbar, above their owner, and minimized along with it.
    pub fn transient_for(mut self, owner: WindowId) -> Self {
        self.transient_for = Some(owner);
        self
    }
}

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Button 1.
    Left,
    /// Button 2.
    Middle,
    /// Button 3.
    Right,
    /// Button 8 ("back" side button).
    Back,
    /// Button 9 ("forward" side button).
    Forward,
    /// Any other button number.
    Other(u8),
}

/// Mouse buttons held during an event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MouseButtons {
    /// Button 1.
    pub left: bool,
    /// Button 2.
    pub middle: bool,
    /// Button 3.
    pub right: bool,
}

/// Something that happened to one of the connection's windows.
///
/// Coordinates are window-relative unless named `root_*` (relative to the
/// screen). Exposure is handled internally — each window keeps its last
/// presented frame on the server — so there is no redraw event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// A key was pressed or released.
    Key {
        /// The window with keyboard focus.
        window: WindowId,
        /// The decoded key, Shift/Caps Lock applied.
        key: Key,
        /// The raw keysym.
        keysym: Keysym,
        /// The physical keycode.
        keycode: u8,
        /// Press (`true`) or release.
        pressed: bool,
        /// A press generated by key autorepeat (detected with XKB's
        /// detectable autorepeat where the server has it, otherwise from
        /// the Release+Press pair the core protocol sends).
        repeat: bool,
        /// Modifiers held.
        modifiers: Modifiers,
    },
    /// A mouse button was pressed or released.
    Button {
        /// The window.
        window: WindowId,
        /// Which button.
        button: MouseButton,
        /// Press (`true`) or release.
        pressed: bool,
        /// Pointer position.
        x: i32,
        /// Pointer position.
        y: i32,
        /// Pointer position on the screen.
        root_x: i32,
        /// Pointer position on the screen.
        root_y: i32,
        /// Modifiers held.
        modifiers: Modifiers,
        /// Server time in milliseconds (wraps every ~49 days) — compare two
        /// presses with `wrapping_sub` to detect a double click.
        time: u32,
    },
    /// The wheel turned (buttons 4–7). Positive `dy` is down / toward the
    /// user, positive `dx` is right; one notch is 1.
    Scroll {
        /// The window.
        window: WindowId,
        /// Horizontal notches.
        dx: i32,
        /// Vertical notches.
        dy: i32,
        /// Pointer position.
        x: i32,
        /// Pointer position.
        y: i32,
        /// Modifiers held.
        modifiers: Modifiers,
    },
    /// The pointer moved. Bursts are coalesced to the latest position.
    Motion {
        /// The window.
        window: WindowId,
        /// Pointer position.
        x: i32,
        /// Pointer position.
        y: i32,
        /// Pointer position on the screen.
        root_x: i32,
        /// Pointer position on the screen.
        root_y: i32,
        /// Buttons held.
        buttons: MouseButtons,
        /// Modifiers held.
        modifiers: Modifiers,
    },
    /// The pointer entered the window.
    PointerEntered {
        /// The window.
        window: WindowId,
        /// Entry position.
        x: i32,
        /// Entry position.
        y: i32,
    },
    /// The pointer left the window.
    PointerLeft {
        /// The window.
        window: WindowId,
    },
    /// The window gained or lost keyboard focus.
    Focused {
        /// The window.
        window: WindowId,
        /// Gained (`true`) or lost.
        focused: bool,
    },
    /// The window's size changed. Present a frame of the new size.
    Resized {
        /// The window.
        window: WindowId,
        /// New width.
        width: u32,
        /// New height.
        height: u32,
    },
    /// The user asked to close the window (the title bar's ×). Nothing is
    /// closed until you call [`Connection::destroy_window`] or exit.
    CloseRequested {
        /// The window.
        window: WindowId,
    },
    /// The window was destroyed by someone else (another client, or the
    /// server). Its [`WindowId`] is no longer valid.
    Destroyed {
        /// The window.
        window: WindowId,
    },
    /// A [`Waker`] woke the event loop.
    Wake,
    /// The server rejected a request that had no reply. Usually a bug in
    /// the caller (a bad window id) or the server running out of memory.
    Error(XError),
    /// An event about a window this connection did not create, as the
    /// wire carried it: what a window manager hears of other programs'
    /// windows (map and configure requests, unmapping, destruction,
    /// property changes, client messages, grabbed keys and buttons). Only
    /// after [`Connection::become_window_manager`].
    Foreign(wire::Event),
}

impl Event {
    /// The window the event concerns, if any.
    pub fn window(&self) -> Option<WindowId> {
        match *self {
            Event::Key { window, .. }
            | Event::Button { window, .. }
            | Event::Scroll { window, .. }
            | Event::Motion { window, .. }
            | Event::PointerEntered { window, .. }
            | Event::PointerLeft { window }
            | Event::Focused { window, .. }
            | Event::Resized { window, .. }
            | Event::CloseRequested { window }
            | Event::Destroyed { window } => Some(window),
            Event::Wake | Event::Error(_) | Event::Foreign(_) => None,
        }
    }
}

/// Atoms interned once at startup.
#[derive(Debug, Clone, Copy, Default)]
struct Atoms {
    wm_protocols: Atom,
    wm_delete_window: Atom,
    wm_client_machine: Atom,
    utf8_string: Atom,
    net_wm_name: Atom,
    net_wm_icon_name: Atom,
    net_wm_icon: Atom,
    net_wm_pid: Atom,
    net_wm_state: Atom,
    net_wm_state_above: Atom,
    net_wm_moveresize: Atom,
    net_supported: Atom,
    motif_wm_hints: Atom,
    wake: Atom,
    wm_change_state: Atom,
}

const ATOM_NAMES: [&str; 15] = [
    "WM_PROTOCOLS",
    "WM_DELETE_WINDOW",
    "WM_CLIENT_MACHINE",
    "UTF8_STRING",
    "_NET_WM_NAME",
    "_NET_WM_ICON_NAME",
    "_NET_WM_ICON",
    "_NET_WM_PID",
    "_NET_WM_STATE",
    "_NET_WM_STATE_ABOVE",
    "_NET_WM_MOVERESIZE",
    "_NET_SUPPORTED",
    "_MOTIF_WM_HINTS",
    "_PROTOLITH_WAKE",
    "WM_CHANGE_STATE",
];

impl Atoms {
    fn from_list(a: &[Atom; 15]) -> Atoms {
        Atoms {
            wm_protocols: a[0],
            wm_delete_window: a[1],
            wm_client_machine: a[2],
            utf8_string: a[3],
            net_wm_name: a[4],
            net_wm_icon_name: a[5],
            net_wm_icon: a[6],
            net_wm_pid: a[7],
            net_wm_state: a[8],
            net_wm_state_above: a[9],
            net_wm_moveresize: a[10],
            net_supported: a[11],
            motif_wm_hints: a[12],
            wake: a[13],
            wm_change_state: a[14],
        }
    }
}

/// Every input and structure event a window selects.
const WINDOW_EVENTS: u32 = event_mask::KEY_PRESS
    | event_mask::KEY_RELEASE
    | event_mask::BUTTON_PRESS
    | event_mask::BUTTON_RELEASE
    | event_mask::ENTER_WINDOW
    | event_mask::LEAVE_WINDOW
    | event_mask::POINTER_MOTION
    | event_mask::EXPOSURE
    | event_mask::STRUCTURE_NOTIFY
    | event_mask::FOCUS_CHANGE;

/// `_NET_WM_MOVERESIZE` direction: move.
const MOVERESIZE_MOVE: u32 = 8;

#[derive(Debug)]
struct WindowState {
    width: u16,
    height: u16,
    /// Server-side copy of the last presented frame; exposures repaint
    /// from it.
    pixmap: Xid,
    gc: Xid,
    mapped: bool,
    focus_requested: bool,
    resizable: bool,
    always_on_top: bool,
    /// The position last asked for: rewriting the size hints must keep
    /// it, or a window manager reading them afterwards takes the window as
    /// unplaced (twm then waits for the user to place it by hand, holding
    /// a server grab).
    position: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    window: Xid,
    start_root: (i32, i32),
    start_pos: (i32, i32),
}

/// A connection to an X server, owning a set of windows.
///
/// ```no_run
/// use protolith::x11::{Connection, Event, WindowOptions};
/// use protolith::Frame;
///
/// let mut conn = Connection::open()?;
/// let win = conn.create_window(WindowOptions::new(320, 200).title("hello"))?;
/// let pixels = vec![0x00_33_66_99u32; 320 * 200];
/// conn.present(win, &Frame::xrgb(&pixels, 320, 200)?)?;
/// loop {
///     match conn.wait_event()? {
///         Event::CloseRequested { .. } => break,
///         _ => {}
///     }
/// }
/// # Ok::<(), protolith::Error>(())
/// ```
#[derive(Debug)]
pub struct Connection {
    client: Client,
    root: Xid,
    depth: u8,
    visual: Visual,
    image_format: Format,
    pixel_format: PixelFormat,
    colormap: Option<Xid>,
    atoms: Atoms,
    shape_opcode: Option<u8>,
    keymap: Keymap,
    has_wm: bool,
    wm_moveresize: Option<bool>,
    windows: HashMap<Xid, WindowState>,
    events: VecDeque<Event>,
    scratch: Vec<u8>,
    drag: Option<Drag>,
    wake_window: Option<Xid>,
    /// Pass events about other clients' windows on ([`Event::Foreign`]).
    foreign: bool,
    /// Windows we destroyed whose DestroyNotify has not arrived yet.
    dying: std::collections::HashSet<Xid>,
    /// XKB detectable autorepeat is on: repeats are bare presses.
    detectable_repeat: bool,
    /// Keycodes currently held down (pressed while we had focus).
    held: std::collections::HashSet<u8>,
}

impl Connection {
    /// Connect to `$DISPLAY`.
    pub fn open() -> Result<Connection> {
        Connection::with_client(Client::connect_env()?)
    }

    /// Connect to a named display (`":1"`, `"localhost:10.0"`), with
    /// explicit credentials or (`None`) those from `$XAUTHORITY`.
    pub fn open_display(display: &str, auth: Option<Auth>) -> Result<Connection> {
        Connection::with_client(Client::connect(&DisplayName::parse(display)?, auth)?)
    }

    /// Wrap an already-connected [`Client`].
    pub fn with_client(mut client: Client) -> Result<Connection> {
        let screen = client.screen().clone();
        let (depth, visual) = screen.choose_visual().ok_or_else(|| Error::Unsupported("the screen has no TrueColor visual".into()))?;
        let image_format = client.setup().format(depth).ok_or_else(|| Error::Unsupported(format!("no pixmap format for depth {depth}")))?;
        let pixel_format =
            client.setup().pixel_format(depth, &visual).map_err(|e| Error::Unsupported(format!("depth {depth} visual {:#x}: {e}", visual.id)))?;

        // One round trip for everything: atoms, SHAPE, keyboard, WM check.
        let atom_cookies = ATOM_NAMES.iter().map(|n| client.send_with_reply(&req::intern_atom(n, false))).collect::<Result<Vec<_>>>()?;
        let shape_cookie = client.send_with_reply(&req::query_extension(shape_req::NAME))?;
        let (min_kc, max_kc) = (client.setup().min_keycode, client.setup().max_keycode);
        let kbd_cookie = client.send_with_reply(&req::get_keyboard_mapping(min_kc, max_kc.saturating_sub(min_kc).saturating_add(1)))?;
        let mods_cookie = client.send_with_reply(&req::get_modifier_mapping())?;
        let root_cookie = client.send_with_reply(&req::get_window_attributes(screen.root))?;
        let xkb_cookie = client.send_with_reply(&req::query_extension(req::xkb::NAME))?;

        let mut atoms = [0; 15];
        for (slot, c) in atoms.iter_mut().zip(atom_cookies) {
            *slot = req::parse_intern_atom(&client.wait_reply(c)?).ok_or_else(|| Error::Protocol("short InternAtom reply".into()))?;
        }
        let shape_opcode = req::parse_query_extension(&client.wait_reply(shape_cookie)?).flatten().map(|e| e.major_opcode);
        let (per, syms) =
            req::parse_keyboard_mapping(&client.wait_reply(kbd_cookie)?).ok_or_else(|| Error::Protocol("short GetKeyboardMapping reply".into()))?;
        let mods = req::parse_modifier_mapping(&client.wait_reply(mods_cookie)?).ok_or_else(|| Error::Protocol("short GetModifierMapping reply".into()))?;
        let root_attrs =
            req::parse_window_attributes(&client.wait_reply(root_cookie)?).ok_or_else(|| Error::Protocol("short GetWindowAttributes reply".into()))?;
        let xkb = req::parse_query_extension(&client.wait_reply(xkb_cookie)?).flatten();
        let detectable_repeat = match xkb {
            Some(info) => enable_detectable_autorepeat(&mut client, info.major_opcode)?,
            None => false,
        };

        let colormap = if visual.id != screen.root_visual {
            let cmap = client.generate_id()?;
            client.send(&req::create_colormap(cmap, screen.root, visual.id))?;
            Some(cmap)
        } else {
            None
        };

        Ok(Connection {
            root: screen.root,
            depth,
            visual,
            image_format,
            pixel_format,
            colormap,
            atoms: Atoms::from_list(&atoms),
            shape_opcode,
            keymap: Keymap::new(min_kc, per, syms, &mods),
            has_wm: root_attrs.all_event_masks & event_mask::SUBSTRUCTURE_REDIRECT != 0,
            wm_moveresize: None,
            windows: HashMap::new(),
            events: VecDeque::new(),
            scratch: Vec::new(),
            drag: None,
            wake_window: None,
            foreign: false,
            dying: Default::default(),
            detectable_repeat,
            held: Default::default(),
            client,
        })
    }

    // ---------------------------------------------------------- queries

    /// The underlying protocol client, for requests this API does not
    /// cover. Do not use it to read events (that would bypass the window
    /// bookkeeping); use it to send requests and collect their replies.
    pub fn client(&mut self) -> &mut Client {
        &mut self.client
    }

    /// The destination pixel layout frames are converted to.
    pub fn pixel_format(&self) -> PixelFormat {
        self.pixel_format
    }

    /// Depth of the windows this connection creates.
    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// Screen size in pixels.
    pub fn screen_size(&self) -> (u32, u32) {
        let s = self.client.screen();
        (u32::from(s.width), u32::from(s.height))
    }

    /// Was a window manager running when the connection opened (or is
    /// this connection one)?
    pub fn has_window_manager(&self) -> bool {
        self.has_wm
    }

    /// The root window of the screen.
    pub fn root(&self) -> Xid {
        self.root
    }

    /// Become the window manager: redirect the root's children's map and
    /// configure requests to this connection, and hear of their
    /// unmapping, destruction and property changes — every such event
    /// about a window this connection did not create comes as
    /// [`Event::Foreign`]. Only one client may hold the redirection: with
    /// another window manager running this fails with the server's
    /// `Access` error ([`Error::X`], code 10).
    pub fn become_window_manager(&mut self) -> Result<()> {
        let mask = event_mask::SUBSTRUCTURE_REDIRECT | event_mask::SUBSTRUCTURE_NOTIFY | event_mask::STRUCTURE_NOTIFY | event_mask::PROPERTY_CHANGE;
        let first = self.client.send(&req::change_window_attributes(self.root, &WindowAttributes { event_mask: Some(mask), ..Default::default() }))?;
        self.client.check_since(first)?;
        self.has_wm = true; // our own windows must not grab the focus on mapping
        self.foreign = true;
        Ok(())
    }

    /// Does the server support non-rectangular windows?
    pub fn supports_shape(&self) -> bool {
        self.shape_opcode.is_some()
    }

    /// The window's current size.
    pub fn window_size(&self, window: WindowId) -> Result<(u32, u32)> {
        let w = self.state(window)?;
        Ok((u32::from(w.width), u32::from(w.height)))
    }

    /// The keyboard map in use.
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    fn state(&self, window: WindowId) -> Result<&WindowState> {
        self.windows.get(&window.0).ok_or_else(|| Error::InvalidArgument(format!("unknown window {:#x}", window.0)))
    }

    fn state_mut(&mut self, window: WindowId) -> Result<&mut WindowState> {
        self.windows.get_mut(&window.0).ok_or_else(|| Error::InvalidArgument(format!("unknown window {:#x}", window.0)))
    }

    // --------------------------------------------------------- windows

    /// Create a window. Errors the server reports for any part of the
    /// setup (bad size, out of memory) are returned here, not later.
    pub fn create_window(&mut self, opts: WindowOptions) -> Result<WindowId> {
        let (w, h) = (dim(opts.width)?, dim(opts.height)?);
        let (x, y) = opts.position.unwrap_or((0, 0));
        let (x, y) = (coord(x), coord(y));
        let first = self.client.last_sequence() + 1;
        let wid = self.client.generate_id()?;
        let pixmap = self.client.generate_id()?;
        let gc = self.client.generate_id()?;
        let c = &mut self.client;
        c.send(&req::create_window(&req::CreateWindow {
            wid,
            parent: opts.parent.unwrap_or(self.root),
            depth: self.depth,
            x,
            y,
            width: w,
            height: h,
            border_width: 0,
            class: WindowClass::InputOutput,
            visual: self.visual.id,
            attributes: WindowAttributes {
                background_pixmap: Some(0),
                border_pixel: Some(0),
                bit_gravity: Some(1),
                override_redirect: opts.override_redirect.then_some(true),
                event_mask: Some(if opts.frame { WINDOW_EVENTS | event_mask::SUBSTRUCTURE_REDIRECT | event_mask::SUBSTRUCTURE_NOTIFY } else { WINDOW_EVENTS }),
                colormap: self.colormap,
                ..Default::default()
            },
        }))?;
        c.send(&req::create_pixmap(pixmap, wid, self.depth, w, h))?;
        c.send(&req::create_gc(gc, wid))?;
        c.send(&req::poly_fill_rectangle(pixmap, gc, &[Rect::from_size(opts.width, opts.height)]))?;

        let a = self.atoms;
        if opts.parent.is_none() {
            top_level_properties(c, a, wid, &opts, w, h)?;
        }
        if opts.visible {
            c.send(&req::map_window(wid))?;
        }
        if let Err(e) = self.client.check_since(first) {
            // Best effort: free whatever did get created.
            let _ = self.client.send(&req::free_gc(gc));
            let _ = self.client.send(&req::free_pixmap(pixmap));
            let _ = self.client.send(&req::destroy_window(wid));
            // Drop only the errors this attempt caused (cleanup included);
            // events and other windows' errors stay queued.
            let _ = self.client.sync();
            self.client.discard_errors_since(first);
            self.client.release_id(gc);
            self.client.release_id(pixmap);
            self.dying.insert(wid);
            return Err(e);
        }
        self.windows.insert(
            wid,
            WindowState {
                width: w,
                height: h,
                pixmap,
                gc,
                mapped: false,
                focus_requested: false,
                resizable: opts.resizable,
                always_on_top: opts.always_on_top,
                position: opts.position,
            },
        );
        Ok(WindowId(wid))
    }

    /// Destroy a window and free its server resources.
    pub fn destroy_window(&mut self, window: WindowId) -> Result<()> {
        let st = self.windows.remove(&window.0).ok_or_else(|| Error::InvalidArgument(format!("unknown window {:#x}", window.0)))?;
        self.client.send(&req::free_gc(st.gc))?;
        self.client.send(&req::free_pixmap(st.pixmap))?;
        self.client.send(&req::destroy_window(window.0))?;
        // The GC and pixmap ids are free for reuse at once: the server
        // processes requests in order and sends no events about them. The
        // window id is not: events for it (Unmap, FocusOut, ...) are still
        // in flight, and a new window wearing the same id would receive
        // them. It is released when its DestroyNotify arrives.
        self.client.release_id(st.gc);
        self.client.release_id(st.pixmap);
        self.dying.insert(window.0);
        if self.drag.is_some_and(|d| d.window == window.0) {
            self.drag = None;
        }
        self.events.retain(|e| e.window() != Some(window));
        self.client.flush()
    }

    /// Map (show) the window.
    pub fn show(&mut self, window: WindowId) -> Result<()> {
        self.state(window)?;
        self.client.send(&req::map_window(window.0))?;
        self.client.flush()
    }

    /// Unmap (hide) the window.
    pub fn hide(&mut self, window: WindowId) -> Result<()> {
        self.state(window)?;
        self.client.send(&req::unmap_window(window.0))?;
        self.client.flush()
    }

    /// Set the title.
    pub fn set_title(&mut self, window: WindowId, title: &str) -> Result<()> {
        self.state(window)?;
        let a = self.atoms;
        set_text(&mut self.client, window.0, atom::WM_NAME, a.net_wm_name, a.utf8_string, title)?;
        set_text(&mut self.client, window.0, atom::WM_ICON_NAME, a.net_wm_icon_name, a.utf8_string, title)?;
        self.client.flush()
    }

    /// Move the window (top-left corner, screen coordinates): the window
    /// itself, as [`Connection::window_position`] reports it, whatever
    /// frame the window manager puts around it.
    pub fn set_position(&mut self, window: WindowId, x: i32, y: i32) -> Result<()> {
        self.state_mut(window)?.position = Some((x, y));
        self.client.send(&req::configure_window(window.0, &req::ConfigureWindow { x: Some(coord(x)), y: Some(coord(y)), ..Default::default() }))?;
        self.client.flush()
    }

    /// Resize the window. A [`Event::Resized`] follows once the server
    /// (or window manager) applies it.
    pub fn set_size(&mut self, window: WindowId, width: u32, height: u32) -> Result<()> {
        let (w, h) = (dim(width)?, dim(height)?);
        let st = self.state(window)?;
        if !st.resizable {
            // Fixed-size hints would make the WM refuse the new size.
            self.client.send(&size_hints(window.0, st.position, w, h, false))?;
        }
        self.client.send(&req::configure_window(window.0, &req::ConfigureWindow { width: Some(w), height: Some(h), ..Default::default() }))?;
        self.client.flush()
    }

    /// Raise the window above its siblings.
    pub fn raise(&mut self, window: WindowId) -> Result<()> {
        self.state(window)?;
        self.client.send(&req::configure_window(window.0, &req::ConfigureWindow { stack_mode: Some(req::StackMode::Above), ..Default::default() }))?;
        self.client.flush()
    }

    /// Give the window keyboard focus. Only works once it is mapped. With
    /// no window manager, protolith does this automatically each time a
    /// window is mapped — so with several windows, the last one shown has
    /// the keyboard until you call this.
    pub fn focus(&mut self, window: WindowId) -> Result<()> {
        self.state(window)?;
        self.client.send(&req::set_input_focus(window.0, req::RevertTo::Parent, 0))?;
        self.client.flush()
    }

    /// Keep the window above others, or stop doing so.
    pub fn set_always_on_top(&mut self, window: WindowId, on: bool) -> Result<()> {
        let st = self.state_mut(window)?;
        st.always_on_top = on;
        let mapped = st.mapped;
        let a = self.atoms;
        if mapped && self.has_wm {
            // A mapped window's state is changed by asking the WM.
            let ev = req::client_message(window.0, a.net_wm_state, [u32::from(on), a.net_wm_state_above, 0, 1, 0]);
            self.client.send(&req::send_event(false, self.root, event_mask::SUBSTRUCTURE_REDIRECT | event_mask::SUBSTRUCTURE_NOTIFY, &ev))?;
        } else {
            let above = [a.net_wm_state_above];
            let list: &[u32] = if on { &above } else { &[] };
            self.client.send(&req::change_property(PropMode::Replace, window.0, a.net_wm_state, atom::ATOM, PropData::U32(list)))?;
            if mapped && on {
                self.client.send(&req::configure_window(window.0, &req::ConfigureWindow { stack_mode: Some(req::StackMode::Above), ..Default::default() }))?;
            }
        }
        self.client.flush()
    }

    /// Minimize (iconify) the window: ask the window manager to hide it
    /// until the user brings it back from the taskbar or dock. The way
    /// back is the window manager's, so without one this does nothing.
    pub fn minimize(&mut self, window: WindowId) -> Result<()> {
        let mapped = self.state(window)?.mapped;
        if !mapped || !self.has_wm {
            return Ok(());
        }
        // ICCCM 4.1.4: WM_CHANGE_STATE with IconicState (3), sent to the root.
        let ev = req::client_message(window.0, self.atoms.wm_change_state, [3, 0, 0, 0, 0]);
        self.client.send(&req::send_event(false, self.root, event_mask::SUBSTRUCTURE_REDIRECT | event_mask::SUBSTRUCTURE_NOTIFY, &ev))?;
        self.client.flush()
    }

    /// Turn window-manager decorations on or off.
    pub fn set_decorations(&mut self, window: WindowId, on: bool) -> Result<()> {
        self.state(window)?;
        let a = self.atoms.motif_wm_hints;
        self.client.send(&req::change_property(PropMode::Replace, window.0, a, a, PropData::U32(&motif_hints(on))))?;
        self.client.flush()
    }

    /// Set the window icon (`_NET_WM_ICON`) from a frame. Large icons may
    /// exceed the server's request limit; 16×16 to 64×64 is typical.
    pub fn set_icon(&mut self, window: WindowId, icon: &Frame<'_>) -> Result<()> {
        self.state(window)?;
        let (w, h) = (icon.width(), icon.height());
        let mut data = Vec::with_capacity(2 + (w * h) as usize);
        data.extend([w, h]);
        for y in 0..h {
            for x in 0..w {
                data.push(0xff00_0000 | icon.get(x, y).unwrap_or(0));
            }
        }
        let a = self.atoms.net_wm_icon;
        self.client.send(&req::change_property(PropMode::Replace, window.0, a, atom::CARDINAL, PropData::U32(&data)))?;
        self.client.flush()
    }

    /// Make the window non-rectangular: only the union of `rects` is drawn
    /// and receives input. See [`shape::rects_from_mask`](super::shape::rects_from_mask).
    pub fn set_shape(&mut self, window: WindowId, rects: &[Rect]) -> Result<()> {
        self.state(window)?;
        let major = self.shape_opcode.ok_or_else(|| Error::Unsupported("the server lacks the SHAPE extension".into()))?;
        self.client.send(&shape_req::rectangles(major, shape_req::Kind::Bounding, window.0, 0, 0, rects))?;
        self.client.flush()
    }

    /// Make the window rectangular again.
    pub fn clear_shape(&mut self, window: WindowId) -> Result<()> {
        self.state(window)?;
        let major = self.shape_opcode.ok_or_else(|| Error::Unsupported("the server lacks the SHAPE extension".into()))?;
        self.client.send(&shape_req::clear(major, shape_req::Kind::Bounding, window.0))?;
        self.client.flush()
    }

    /// Ring the bell, `percent` of the base volume in `-100..=100`.
    pub fn bell(&mut self, percent: i8) -> Result<()> {
        self.client.send(&req::bell(percent.clamp(-100, 100)))?;
        self.client.flush()
    }

    /// Where the window's top-left corner is on the screen. Under a window
    /// manager this is the window itself, not its frame.
    pub fn window_position(&mut self, window: WindowId) -> Result<(i32, i32)> {
        self.state(window)?;
        let reply = self.client.round_trip(&req::translate_coordinates(window.0, self.root, 0, 0))?;
        let (x, y) = req::parse_translate_coordinates(&reply).ok_or_else(|| Error::Protocol("short TranslateCoordinates reply".into()))?;
        Ok((i32::from(x), i32::from(y)))
    }

    /// Start moving the window with the pointer, typically on a button
    /// press over your own title bar (pass the press's root coordinates).
    ///
    /// With a window manager that supports it, the WM takes over the drag
    /// (`_NET_WM_MOVERESIZE`) and handles snapping and edge resistance.
    /// Otherwise protolith follows the pointer itself until the button is
    /// released. Either way your app just keeps handling events.
    pub fn begin_drag_move(&mut self, window: WindowId, root_x: i32, root_y: i32, button: MouseButton) -> Result<()> {
        self.state(window)?;
        if self.has_wm && self.wm_supports_moveresize()? {
            let b = match button {
                MouseButton::Left => 1,
                MouseButton::Middle => 2,
                MouseButton::Right => 3,
                _ => 0,
            };
            let a = self.atoms;
            self.client.send(&req::ungrab_pointer(0))?;
            let ev = req::client_message(window.0, a.net_wm_moveresize, [root_x as u32, root_y as u32, MOVERESIZE_MOVE, b, 1]);
            self.client.send(&req::send_event(false, self.root, event_mask::SUBSTRUCTURE_REDIRECT | event_mask::SUBSTRUCTURE_NOTIFY, &ev))?;
            return self.client.flush();
        }
        let start_pos = self.window_position(window)?;
        self.drag = Some(Drag { window: window.0, start_root: (root_x, root_y), start_pos });
        Ok(())
    }

    fn wm_supports_moveresize(&mut self) -> Result<bool> {
        if let Some(s) = self.wm_moveresize {
            return Ok(s);
        }
        let reply = self.client.round_trip(&req::get_property(false, self.root, self.atoms.net_supported, atom::ATOM, 0, 4096))?;
        let supported = req::parse_property(&reply).and_then(|p| p.as_u32s()).is_some_and(|l| l.contains(&self.atoms.net_wm_moveresize));
        self.wm_moveresize = Some(supported);
        Ok(supported)
    }

    // ------------------------------------------------------------ pixels

    /// Show `frame` in the window. The frame is drawn at the window's
    /// top-left; a frame smaller than the window leaves the rest as it
    /// was, a larger one is clipped.
    pub fn present(&mut self, window: WindowId, frame: &Frame<'_>) -> Result<()> {
        self.present_rect(window, frame, frame.bounds())
    }

    /// Show only the `area` of `frame` (frame and window coordinates
    /// coincide) — for animating a small part of a window, such as a
    /// spectrum analyzer, without resending the rest.
    pub fn present_rect(&mut self, window: WindowId, frame: &Frame<'_>, area: Rect) -> Result<()> {
        let st = self.state(window)?;
        let (pixmap, gc) = (st.pixmap, st.gc);
        // Clip first: the PutImage width and the row stride must describe
        // the same, final rectangle.
        let area = area.intersect(&Rect::from_size(u32::from(st.width), u32::from(st.height))).intersect(&frame.bounds());
        let stride = self.image_format.row_stride(area.width);
        let mut buf = std::mem::take(&mut self.scratch);
        let done = self.pixel_format.convert(frame, area, stride, &mut buf);
        let r = self.upload(pixmap, gc, done, &buf, stride).and_then(|_| self.copy_to_window(window.0, done)).and_then(|_| self.client.flush());
        self.scratch = buf;
        r
    }

    fn upload(&mut self, pixmap: Xid, gc: Xid, area: Rect, data: &[u8], stride: usize) -> Result<()> {
        if area.is_empty() {
            return Ok(());
        }
        let max_words = self.client.setup().maximum_request_length;
        for (row, rows) in req::put_image_bands(area.height as usize, stride, max_words) {
            let band = &data[row * stride..(row + rows) * stride];
            let y = area.y + row as i32;
            self.client.send(&req::put_image(pixmap, gc, area.width as u16, rows as u16, area.x as i16, y as i16, self.depth, band))?;
        }
        Ok(())
    }

    fn copy_to_window(&mut self, window: Xid, area: Rect) -> Result<()> {
        if area.is_empty() {
            return Ok(());
        }
        let st = &self.windows[&window];
        let (x, y, w, h) = (area.x as i16, area.y as i16, area.width as u16, area.height as u16);
        let r = req::copy_area(st.pixmap, window, st.gc, x, y, x, y, w, h);
        self.client.send(&r).map(drop)
    }

    /// Shift the pixels of `area` by `(dx, dy)` on the server, without
    /// resending them — the classic scroll. The strip it uncovers keeps
    /// stale pixels until you [`present_rect`](Connection::present_rect)
    /// it.
    pub fn scroll(&mut self, window: WindowId, area: Rect, dx: i32, dy: i32) -> Result<()> {
        let st = self.state(window)?;
        let bounds = Rect::from_size(u32::from(st.width), u32::from(st.height));
        let (pixmap, gc) = (st.pixmap, st.gc);
        let area = area.intersect(&bounds);
        if area.is_empty() {
            return Ok(());
        }
        // Only pixels that land inside `area` move; nothing outside it is
        // touched (the backing pixmap repaints exposures, so it must stay
        // exactly what was presented).
        let dst = Rect::new(area.x.saturating_add(dx), area.y.saturating_add(dy), area.width, area.height).intersect(&area);
        if !dst.is_empty() {
            let (sx, sy) = (dst.x - dx, dst.y - dy);
            let r = req::copy_area(pixmap, pixmap, gc, sx as i16, sy as i16, dst.x as i16, dst.y as i16, dst.width as u16, dst.height as u16);
            self.client.send(&r)?;
        }
        self.copy_to_window(window.0, area)?;
        self.client.flush()
    }

    /// Read back the window's current contents (from its backing store,
    /// so it works even while the window is hidden or covered) as
    /// `0x00RRGGBB` pixels, row-major. Channels narrower than 8 bits are
    /// expanded by bit replication.
    pub fn read_pixels(&mut self, window: WindowId, area: Rect) -> Result<Vec<u32>> {
        let st = self.state(window)?;
        let area = area.intersect(&Rect::from_size(u32::from(st.width), u32::from(st.height)));
        if area.is_empty() {
            return Ok(Vec::new());
        }
        let pixmap = st.pixmap;
        let reply = self.client.round_trip(&req::get_image(pixmap, area.x as i16, area.y as i16, area.width as u16, area.height as u16))?;
        let (_, _, data) = req::parse_image(&reply).ok_or_else(|| Error::Protocol("short GetImage reply".into()))?;
        let stride = self.image_format.row_stride(area.width);
        let bpp = self.pixel_format.bytes_per_pixel();
        let mut out = Vec::with_capacity((area.width * area.height) as usize);
        for row in 0..area.height as usize {
            for col in 0..area.width as usize {
                let off = row * stride + col * bpp;
                let px = data.get(off..).and_then(|b| self.pixel_format.load(b)).ok_or_else(|| Error::Protocol("GetImage reply too short".into()))?;
                out.push(self.pixel_format.unpack(px));
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ events

    /// The next event, without blocking.
    pub fn poll_event(&mut self) -> Result<Option<Event>> {
        loop {
            if let Some(e) = self.events.pop_front() {
                return Ok(Some(e));
            }
            match self.client.poll_message()? {
                Some(m) => self.handle(m)?,
                None => return Ok(None),
            }
        }
    }

    /// Block until the next event.
    pub fn wait_event(&mut self) -> Result<Event> {
        loop {
            if let Some(e) = self.events.pop_front() {
                return Ok(e);
            }
            if let Some(m) = self.client.wait_message(None)? {
                self.handle(m)?;
            }
        }
    }

    /// Block until the next event or until `timeout` passes (`Ok(None)`).
    /// The building block of an animation loop.
    pub fn wait_event_timeout(&mut self, timeout: Duration) -> Result<Option<Event>> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(e) = self.events.pop_front() {
                return Ok(Some(e));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.client.wait_message(Some(left))? {
                Some(m) => self.handle(m)?,
                None => return Ok(None),
            }
        }
    }

    /// A handle that can wake [`wait_event`](Connection::wait_event) from
    /// another thread (with an [`Event::Wake`]), e.g. when a decoder thread
    /// has new data. Opens a second connection to the same display.
    pub fn waker(&mut self) -> Result<Waker> {
        let display = self.client.display_name().cloned().ok_or_else(|| Error::Unsupported("a Waker needs a connection opened by display name".into()))?;
        let window = match self.wake_window {
            Some(w) => w,
            None => {
                let w = self.client.generate_id()?;
                let first = self.client.last_sequence() + 1;
                self.client.send(&req::create_window(&req::CreateWindow {
                    wid: w,
                    parent: self.root,
                    depth: 0,
                    x: -1,
                    y: -1,
                    width: 1,
                    height: 1,
                    border_width: 0,
                    class: WindowClass::InputOnly,
                    visual: 0,
                    attributes: WindowAttributes::default(),
                }))?;
                self.client.check_since(first)?;
                self.wake_window = Some(w);
                w
            }
        };
        let other = Client::connect(&display, Some(self.client.auth().clone()))?;
        Ok(Waker { inner: Arc::new(Mutex::new(other)), window, atom: self.atoms.wake })
    }

    /// Turn one protocol message into zero or more [`Event`]s.
    fn handle(&mut self, m: Message) -> Result<()> {
        let ev = match m {
            Message::Error(e) => {
                self.events.push_back(Event::Error(e));
                return Ok(());
            }
            Message::Event(e) => e,
        };
        if self.foreign {
            if let Some(w) = ev.event.window() {
                if !self.windows.contains_key(&w) && !self.dying.contains(&w) && Some(w) != self.wake_window {
                    self.events.push_back(Event::Foreign(ev));
                    return Ok(());
                }
            }
        }
        match ev.event {
            RawEvent::KeyPress(i) | RawEvent::KeyRelease(i) => {
                let pressed = matches!(ev.event, RawEvent::KeyPress(_));
                if !self.windows.contains_key(&i.event) {
                    return Ok(());
                }
                let mut repeat = false;
                if pressed {
                    // With detectable autorepeat, a repeat is a press of a
                    // key that is already down.
                    repeat = !self.held.insert(i.detail);
                } else if self.detectable_repeat {
                    self.held.remove(&i.detail);
                } else {
                    // Without XKB, autorepeat arrives as Release+Press with
                    // one timestamp. Only a pair already received can be
                    // recognized; a pair split across reads shows up as a
                    // release and a fresh press.
                    self.held.remove(&i.detail);
                    match self.client.poll_message()? {
                        Some(Message::Event(next)) if matches!(next.event, RawEvent::KeyPress(n) if n.detail == i.detail && n.time == i.time && n.event == i.event) =>
                        {
                            repeat = true;
                            self.held.insert(i.detail);
                        }
                        Some(other) => self.client.unread(other),
                        None => {}
                    }
                }
                let (key, keysym) = self.keymap.lookup(i.detail, i.state);
                self.events.push_back(Event::Key {
                    window: WindowId(i.event),
                    key,
                    keysym,
                    keycode: i.detail,
                    pressed: pressed || repeat,
                    repeat,
                    modifiers: self.keymap.modifiers(i.state),
                });
            }
            RawEvent::ButtonPress(i) | RawEvent::ButtonRelease(i) => {
                let pressed = matches!(ev.event, RawEvent::ButtonPress(_));
                if !pressed && self.drag.is_some_and(|d| d.window == i.event) {
                    self.drag = None;
                }
                if !self.windows.contains_key(&i.event) {
                    return Ok(());
                }
                let window = WindowId(i.event);
                let modifiers = self.keymap.modifiers(i.state);
                let (x, y) = (i32::from(i.x), i32::from(i.y));
                let scroll = match i.detail {
                    4 => Some((0, -1)),
                    5 => Some((0, 1)),
                    6 => Some((-1, 0)),
                    7 => Some((1, 0)),
                    _ => None,
                };
                if let Some((dx, dy)) = scroll {
                    if pressed {
                        self.events.push_back(Event::Scroll { window, dx, dy, x, y, modifiers });
                    }
                    return Ok(());
                }
                let button = match i.detail {
                    1 => MouseButton::Left,
                    2 => MouseButton::Middle,
                    3 => MouseButton::Right,
                    8 => MouseButton::Back,
                    9 => MouseButton::Forward,
                    n => MouseButton::Other(n),
                };
                self.events.push_back(Event::Button {
                    window,
                    button,
                    pressed,
                    x,
                    y,
                    root_x: i32::from(i.root_x),
                    root_y: i32::from(i.root_y),
                    modifiers,
                    time: i.time,
                });
            }
            RawEvent::MotionNotify(first) => {
                let mut i = first;
                // Coalesce a burst: skip to the newest queued motion.
                loop {
                    match self.client.poll_message()? {
                        Some(Message::Event(next)) => match next.event {
                            RawEvent::MotionNotify(n) if n.event == i.event => i = n,
                            _ => {
                                self.client.unread(Message::Event(next));
                                break;
                            }
                        },
                        Some(other) => {
                            self.client.unread(other);
                            break;
                        }
                        None => break,
                    }
                }
                self.follow_drag(&i)?;
                if self.windows.contains_key(&i.event) {
                    self.events.push_back(Event::Motion {
                        window: WindowId(i.event),
                        x: i32::from(i.x),
                        y: i32::from(i.y),
                        root_x: i32::from(i.root_x),
                        root_y: i32::from(i.root_y),
                        buttons: MouseButtons {
                            left: i.state & state::BUTTON1 != 0,
                            middle: i.state & state::BUTTON2 != 0,
                            right: i.state & state::BUTTON3 != 0,
                        },
                        modifiers: self.keymap.modifiers(i.state),
                    });
                }
            }
            // Mode 0 = Normal: grab/ungrab crossings (a WM starting a move,
            // say) are not the pointer really leaving.
            RawEvent::EnterNotify { input: i, mode: 0 } if self.windows.contains_key(&i.event) => {
                self.events.push_back(Event::PointerEntered { window: WindowId(i.event), x: i32::from(i.x), y: i32::from(i.y) });
            }
            RawEvent::LeaveNotify { input: i, mode: 0 } if self.windows.contains_key(&i.event) => {
                self.events.push_back(Event::PointerLeft { window: WindowId(i.event) });
            }
            // Mode 0 = Normal; grab/ungrab focus shuffles are not real changes.
            RawEvent::FocusIn { window, mode: 0 | 3 } if self.windows.contains_key(&window) => {
                self.events.push_back(Event::Focused { window: WindowId(window), focused: true });
            }
            RawEvent::FocusOut { window, mode: 0 | 3 } if self.windows.contains_key(&window) => {
                // Keys released while we lack focus are never reported.
                self.held.clear();
                self.events.push_back(Event::Focused { window: WindowId(window), focused: false });
            }
            RawEvent::Expose { window, x, y, width, height, .. } => {
                if let Some(st) = self.windows.get(&window) {
                    let r = req::copy_area(st.pixmap, window, st.gc, x as i16, y as i16, x as i16, y as i16, width, height);
                    self.client.send(&r)?;
                    self.client.flush()?;
                }
            }
            RawEvent::MapNotify { window } => {
                let has_wm = self.has_wm;
                if let Some(st) = self.windows.get_mut(&window) {
                    st.mapped = true;
                    if !has_wm && !st.focus_requested {
                        // Without a WM nobody else will give us the keyboard.
                        st.focus_requested = true;
                        self.client.send(&req::set_input_focus(window, req::RevertTo::Parent, 0))?;
                        self.client.flush()?;
                    }
                }
            }
            RawEvent::UnmapNotify { window } => {
                if let Some(st) = self.windows.get_mut(&window) {
                    st.mapped = false;
                    st.focus_requested = false; // take it again when re-shown
                }
            }
            RawEvent::ConfigureNotify { window, mut width, mut height, .. } => {
                // Coalesce an interactive-resize burst to the final size.
                loop {
                    match self.client.poll_message()? {
                        Some(Message::Event(next)) => match next.event {
                            RawEvent::ConfigureNotify { window: w, width: nw, height: nh, .. } if w == window => (width, height) = (nw, nh),
                            _ => {
                                self.client.unread(Message::Event(next));
                                break;
                            }
                        },
                        Some(other) => {
                            self.client.unread(other);
                            break;
                        }
                        None => break,
                    }
                }
                self.resized(window, width, height)?
            }
            RawEvent::DestroyNotify { window } => {
                if self.dying.remove(&window) {
                    self.client.release_id(window);
                } else if let Some(st) = self.windows.remove(&window) {
                    // Destroyed by someone else (another client, xkill -id).
                    self.client.send(&req::free_gc(st.gc))?;
                    self.client.send(&req::free_pixmap(st.pixmap))?;
                    self.client.release_id(st.gc);
                    self.client.release_id(st.pixmap);
                    self.client.release_id(window);
                    if self.drag.is_some_and(|d| d.window == window) {
                        self.drag = None;
                    }
                    self.events.retain(|e| e.window() != Some(WindowId(window)));
                    self.events.push_back(Event::Destroyed { window: WindowId(window) });
                }
            }
            RawEvent::ClientMessage { window, message_type, data, .. } => {
                let a = self.atoms;
                if message_type == a.wm_protocols && data[0] == a.wm_delete_window && self.windows.contains_key(&window) {
                    self.events.push_back(Event::CloseRequested { window: WindowId(window) });
                } else if message_type == a.wake && Some(window) == self.wake_window && !self.events.contains(&Event::Wake) {
                    self.events.push_back(Event::Wake);
                }
            }
            RawEvent::MappingNotify { request, .. } if request != 2 => self.reload_keymap()?,
            _ => {}
        }
        Ok(())
    }

    /// Follow the pointer during a protolith-driven window drag.
    fn follow_drag(&mut self, i: &InputEvent) -> Result<()> {
        let Some(d) = self.drag else { return Ok(()) };
        if d.window != i.event {
            return Ok(());
        }
        if i.state & (state::BUTTON1 | state::BUTTON2 | state::BUTTON3) == 0 {
            self.drag = None; // the release was missed; stop following
            return Ok(());
        }
        let x = d.start_pos.0 + i32::from(i.root_x) - d.start_root.0;
        let y = d.start_pos.1 + i32::from(i.root_y) - d.start_root.1;
        self.client.send(&req::configure_window(d.window, &req::ConfigureWindow { x: Some(coord(x)), y: Some(coord(y)), ..Default::default() }))?;
        self.client.flush()
    }

    /// The server resized a window: grow or shrink its backing pixmap,
    /// keeping the overlapping contents, and report it.
    fn resized(&mut self, window: Xid, width: u16, height: u16) -> Result<()> {
        let Some(st) = self.windows.get(&window) else { return Ok(()) };
        if (st.width, st.height) == (width, height) || width == 0 || height == 0 {
            return Ok(());
        }
        let (old_pixmap, gc, ow, oh) = (st.pixmap, st.gc, st.width, st.height);
        let pixmap = self.client.generate_id()?;
        self.client.send(&req::create_pixmap(pixmap, window, self.depth, width, height))?;
        self.client.send(&req::poly_fill_rectangle(pixmap, gc, &[Rect::from_size(u32::from(width), u32::from(height))]))?;
        self.client.send(&req::copy_area(old_pixmap, pixmap, gc, 0, 0, 0, 0, ow.min(width), oh.min(height)))?;
        self.client.send(&req::free_pixmap(old_pixmap))?;
        self.client.release_id(old_pixmap);
        if let Some(st) = self.windows.get_mut(&window) {
            st.pixmap = pixmap;
            st.width = width;
            st.height = height;
        }
        self.client.flush()?;
        // Collapse a burst of resizes into the latest.
        self.events.retain(|e| !matches!(e, Event::Resized { window: w, .. } if w.0 == window));
        self.events.push_back(Event::Resized { window: WindowId(window), width: u32::from(width), height: u32::from(height) });
        Ok(())
    }

    fn reload_keymap(&mut self) -> Result<()> {
        let (min_kc, max_kc) = (self.client.setup().min_keycode, self.client.setup().max_keycode);
        let kbd = self.client.send_with_reply(&req::get_keyboard_mapping(min_kc, max_kc.saturating_sub(min_kc).saturating_add(1)))?;
        let mods = self.client.send_with_reply(&req::get_modifier_mapping())?;
        let kbd = self.client.wait_reply(kbd)?;
        let mods = self.client.wait_reply(mods)?;
        if let (Some((per, syms)), Some(mods)) = (req::parse_keyboard_mapping(&kbd), req::parse_modifier_mapping(&mods)) {
            self.keymap = Keymap::new(min_kc, per, syms, &mods);
        }
        Ok(())
    }
}

impl AsFd for Connection {
    /// The X socket, for `poll(2)` alongside other descriptors. Before
    /// blocking on it, drain [`poll_event`](Connection::poll_event): events
    /// may already be buffered without the socket being readable.
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.client.as_fd()
    }
}

impl AsRawFd for Connection {
    fn as_raw_fd(&self) -> RawFd {
        self.client.as_raw_fd()
    }
}

/// Wakes a [`Connection`]'s event loop from any thread.
///
/// Cheap to clone; every clone shares one connection to the server.
#[derive(Debug, Clone)]
pub struct Waker {
    inner: Arc<Mutex<Client>>,
    window: Xid,
    atom: Atom,
}

impl Waker {
    /// Make the connection's pending (or next) wait return
    /// [`Event::Wake`]. Wakes that arrive before the loop gets to them
    /// collapse into one.
    pub fn wake(&self) -> Result<()> {
        let mut c = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let ev = req::client_message(self.window, self.atom, [0; 5]);
        // Mask 0: delivered to the client that created the window.
        c.send(&req::send_event(false, self.window, 0, &ev))?;
        c.flush()?;
        // Discard anything the server said to this connection (errors if
        // the window is gone) so its buffer never fills.
        while let Ok(Some(_)) = c.poll_message() {}
        Ok(())
    }
}

// ---------------------------------------------------------------- helpers

/// Ask XKB to report autorepeat as repeated presses without releases.
/// `Ok(false)` when the server declines; protolith then falls back to
/// recognizing Release+Press pairs.
fn enable_detectable_autorepeat(client: &mut Client, major: u8) -> Result<bool> {
    use req::xkb;
    let use_ext = client.send_with_reply(&xkb::use_extension(major))?;
    let flags = client.send_with_reply(&xkb::per_client_flags(major, xkb::DETECTABLE_AUTO_REPEAT, xkb::DETECTABLE_AUTO_REPEAT))?;
    let ok = match client.wait_reply(use_ext) {
        Ok(r) => xkb::parse_use_extension(&r) == Some(true),
        Err(Error::X(_)) => false,
        Err(e) => return Err(e),
    };
    match client.wait_reply(flags) {
        Ok(r) => Ok(ok && xkb::parse_per_client_flags(&r).is_some_and(|(_, value)| value & xkb::DETECTABLE_AUTO_REPEAT != 0)),
        Err(Error::X(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

fn dim(v: u32) -> Result<u16> {
    match u16::try_from(v) {
        Ok(d) if d > 0 && d <= i16::MAX as u16 => Ok(d),
        _ => Err(Error::InvalidArgument(format!("window dimension {v} is outside 1..=32767"))),
    }
}

fn coord(v: i32) -> i16 {
    v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
}

/// Latin-1 bytes for a `STRING` property (`?` for what Latin-1 lacks).
fn latin1(s: &str) -> Vec<u8> {
    s.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect()
}

/// Set a legacy `STRING` property and its `UTF8_STRING` EWMH twin.
fn set_text(c: &mut Client, window: Xid, legacy: Atom, net: Atom, utf8: Atom, text: &str) -> Result<()> {
    c.send(&req::change_property(PropMode::Replace, window, legacy, atom::STRING, PropData::U8(&latin1(text))))?;
    c.send(&req::change_property(PropMode::Replace, window, net, utf8, PropData::U8(text.as_bytes())))?;
    Ok(())
}

/// `_MOTIF_WM_HINTS`: flags = decorations field valid.
fn motif_hints(decorations: bool) -> [u32; 5] {
    [2, 0, u32::from(decorations), 0, 0]
}

/// The ICCCM and EWMH properties of a top-level window.
fn top_level_properties(c: &mut Client, a: Atoms, wid: Xid, opts: &WindowOptions, w: u16, h: u16) -> Result<()> {
    set_text(c, wid, atom::WM_NAME, a.net_wm_name, a.utf8_string, &opts.title)?;
    set_text(c, wid, atom::WM_ICON_NAME, a.net_wm_icon_name, a.utf8_string, &opts.title)?;
    let mut class = Vec::new();
    for part in [&opts.class.0, &opts.class.1] {
        class.extend(latin1(part));
        class.push(0);
    }
    c.send(&req::change_property(PropMode::Replace, wid, atom::WM_CLASS, atom::STRING, PropData::U8(&class)))?;
    c.send(&req::change_property(PropMode::Replace, wid, a.wm_protocols, atom::ATOM, PropData::U32(&[a.wm_delete_window])))?;
    c.send(&req::change_property(PropMode::Replace, wid, a.net_wm_pid, atom::CARDINAL, PropData::U32(&[std::process::id()])))?;
    c.send(&req::change_property(PropMode::Replace, wid, a.wm_client_machine, atom::STRING, PropData::U8(&latin1(&hostname()))))?;
    c.send(&size_hints(wid, opts.position, w, h, opts.resizable))?;
    if let Some(owner) = opts.transient_for {
        c.send(&req::change_property(PropMode::Replace, wid, atom::WM_TRANSIENT_FOR, atom::WINDOW, PropData::U32(&[owner.0])))?;
    }
    if !opts.decorations {
        c.send(&req::change_property(PropMode::Replace, wid, a.motif_wm_hints, a.motif_wm_hints, PropData::U32(&motif_hints(false))))?;
    }
    if opts.always_on_top {
        c.send(&req::change_property(PropMode::Replace, wid, a.net_wm_state, atom::ATOM, PropData::U32(&[a.net_wm_state_above])))?;
    }
    Ok(())
}

/// `WM_NORMAL_HINTS` (ICCCM §4.1.2.3). Always with static gravity: a
/// position (here, or later in `set_position`) is then where the window
/// itself goes, not its window manager frame — the same corner that
/// `window_position` reports, so moving a window to where it is leaves it
/// there instead of creeping by the frame's border and title.
fn size_hints(window: Xid, position: Option<(i32, i32)>, w: u16, h: u16, resizable: bool) -> Vec<u8> {
    const US_POSITION: u32 = 1;
    const P_POSITION: u32 = 4;
    const P_MIN_SIZE: u32 = 16;
    const P_MAX_SIZE: u32 = 32;
    const P_WIN_GRAVITY: u32 = 512;
    const STATIC_GRAVITY: u32 = 10;
    let mut hints = [0u32; 18];
    hints[0] = P_WIN_GRAVITY;
    hints[17] = STATIC_GRAVITY;
    if let Some((x, y)) = position {
        hints[0] |= US_POSITION | P_POSITION;
        hints[1] = x as u32;
        hints[2] = y as u32;
    }
    hints[3] = u32::from(w);
    hints[4] = u32::from(h);
    if !resizable {
        hints[0] |= P_MIN_SIZE | P_MAX_SIZE;
        hints[5] = u32::from(w);
        hints[6] = u32::from(h);
        hints[7] = u32::from(w);
        hints[8] = u32::from(h);
    }
    req::change_property(PropMode::Replace, window, atom::WM_NORMAL_HINTS, atom::WM_SIZE_HINTS, PropData::U32(&hints))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x11::fake::{FakeScreen, FakeServer, Script, FOCUS_WINDOW, ROOT, SHAPE_MAJOR};
    use crate::x11::proto::event::tests::synth;
    use crate::x11::proto::request::opcode;

    fn open(script: Script) -> (Connection, FakeServer) {
        let (client, server) = FakeServer::start(script);
        (Connection::with_client(client.unwrap()).unwrap(), server)
    }

    fn u32_at(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
    }

    fn input(code: u8, detail: u8, window: u32, x: i16, y: i16, state_bits: u16, time: u32) -> [u8; 32] {
        synth(
            code,
            detail,
            &[
                (4, &time.to_le_bytes()),
                (8, &ROOT.to_le_bytes()),
                (12, &window.to_le_bytes()),
                (20, &(x + 100).to_le_bytes()),
                (22, &(y + 50).to_le_bytes()),
                (24, &x.to_le_bytes()),
                (26, &y.to_le_bytes()),
                (28, &state_bits.to_le_bytes()),
            ],
        )
    }

    fn next(conn: &mut Connection) -> Event {
        conn.wait_event_timeout(Duration::from_secs(5)).unwrap().expect("an event")
    }

    #[test]
    fn opening_reads_everything_in_one_round_trip() {
        let (conn, server) = open(Script::default());
        assert_eq!(conn.depth(), 24);
        assert_eq!(conn.pixel_format(), PixelFormat::XRGB8888);
        assert!(conn.supports_shape());
        assert!(!conn.has_window_manager());
        assert_eq!(conn.atoms.wm_delete_window, server.atom("WM_DELETE_WINDOW"));
        assert_eq!(conn.keymap().num_lock_mask(), state::MOD2);
        let log = server.finish();
        assert!(log.with_opcode(opcode::CREATE_COLORMAP).is_empty(), "TrueColor root needs no colormap");
    }

    #[test]
    fn pseudocolor_root_gets_a_truecolor_visual_and_colormap() {
        let (mut conn, server) = open(Script { screen: FakeScreen::Pseudo8, ..Default::default() });
        assert_eq!(conn.depth(), 8);
        assert_eq!(conn.visual.id, 0x24);
        let win = conn.create_window(WindowOptions::new(10, 10)).unwrap();
        let log = server.finish();
        let cmap = log.with_opcode(opcode::CREATE_COLORMAP);
        assert_eq!(cmap.len(), 1);
        assert_eq!(u32_at(cmap[0], 12), 0x24, "colormap for the chosen visual");
        let cw = log.with_opcode(opcode::CREATE_WINDOW)[0];
        assert_eq!(u32_at(cw, 24), 0x24, "window uses the chosen visual");
        assert_eq!(u32_at(cw, 28) & 0x2008, 0x2008, "with colormap and border pixel");
        assert_eq!(win.xid(), u32_at(cw, 4));
    }

    #[test]
    fn create_window_sets_wm_properties() {
        let (mut conn, server) = open(Script::default());
        let win = conn
            .create_window(
                WindowOptions::new(275, 116).title("Wïnamp ♪").class("amp", "Amp").position(10, 20).decorations(false).resizable(false).always_on_top(true),
            )
            .unwrap();
        let w = win.xid();
        assert_eq!(server.property(w, "WM_NAME").unwrap().2, b"W\xefnamp ?");
        assert_eq!(server.property(w, "_NET_WM_NAME").unwrap().2, "Wïnamp ♪".as_bytes());
        assert_eq!(server.property(w, "WM_CLASS").unwrap().2, b"amp\0Amp\0");
        let protocols = server.property(w, "WM_PROTOCOLS").unwrap().2;
        assert_eq!(u32_at(&protocols, 0), server.atom("WM_DELETE_WINDOW"));
        let motif = server.property(w, "_MOTIF_WM_HINTS").unwrap().2;
        assert_eq!((u32_at(&motif, 0), u32_at(&motif, 8)), (2, 0), "decorations off");
        let hints = server.property(w, "WM_NORMAL_HINTS").unwrap().2;
        let hint = |i: usize| u32_at(&hints, i * 4);
        assert_eq!(hint(0), 1 | 4 | 16 | 32 | 512);
        assert_eq!((hint(1), hint(2)), (10, 20));
        assert_eq!(hint(17), 10, "static gravity: the position is the window's, not its frame's");
        assert_eq!((hint(5), hint(6), hint(7), hint(8)), (275, 116, 275, 116), "fixed size");
        let st = server.property(w, "_NET_WM_STATE").unwrap().2;
        assert_eq!(u32_at(&st, 0), server.atom("_NET_WM_STATE_ABOVE"));
        assert_eq!(u32_at(&server.property(w, "_NET_WM_PID").unwrap().2, 0), std::process::id());
        let log = server.finish();
        let cw = log.with_opcode(opcode::CREATE_WINDOW)[0];
        assert_eq!((i16::from_le_bytes([cw[12], cw[13]]), u16::from_le_bytes([cw[16], cw[17]])), (10, 275));
        assert_eq!(log.with_opcode(opcode::MAP_WINDOW).len(), 1);
        assert_eq!(log.with_opcode(opcode::POLY_FILL_RECTANGLE).len(), 1, "backing pixmap starts black");
    }

    #[test]
    fn hidden_windows_are_not_mapped_until_shown() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5).visible(false)).unwrap();
        assert!(server.log().with_opcode(opcode::MAP_WINDOW).is_empty());
        conn.show(win).unwrap();
        conn.client().sync().unwrap();
        assert_eq!(server.finish().with_opcode(opcode::MAP_WINDOW).len(), 1);
    }

    #[test]
    fn invalid_sizes_are_rejected_before_the_server() {
        let (mut conn, server) = open(Script::default());
        for (w, h) in [(0, 5), (5, 0), (40_000, 5), (u32::MAX, 1)] {
            assert!(matches!(conn.create_window(WindowOptions::new(w, h)), Err(Error::InvalidArgument(_))), "{w}x{h}");
        }
        assert!(server.finish().with_opcode(opcode::CREATE_WINDOW).is_empty());
    }

    #[test]
    fn create_window_reports_server_errors_synchronously() {
        let (mut conn, server) = open(Script { fail_opcodes: vec![opcode::CREATE_PIXMAP], ..Default::default() });
        match conn.create_window(WindowOptions::new(5, 5)) {
            Err(Error::X(e)) => assert_eq!(e.major_opcode, opcode::CREATE_PIXMAP),
            other => panic!("{other:?}"),
        }
        assert_eq!(conn.poll_event().unwrap(), None, "the error is not also delivered as an event");
        let log = server.finish();
        assert_eq!(log.with_opcode(opcode::DESTROY_WINDOW).len(), 1, "cleaned up");
    }

    #[test]
    fn present_uploads_to_the_pixmap_and_copies_to_the_window() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(3, 2)).unwrap();
        let px = [0x112233, 0x445566, 0x778899, 0xaabbcc, 0xddeeff, 0x000000];
        conn.present(win, &Frame::xrgb(&px, 3, 2).unwrap()).unwrap();
        conn.client().sync().unwrap();
        let log = server.finish();
        let pixmap = u32_at(log.with_opcode(opcode::CREATE_PIXMAP)[0], 4);
        let put = log.with_opcode(opcode::PUT_IMAGE);
        assert_eq!(put.len(), 1);
        assert_eq!(u32_at(put[0], 4), pixmap, "drawn into the backing pixmap");
        assert_eq!(&put[0][24..36], &[0x33, 0x22, 0x11, 0, 0x66, 0x55, 0x44, 0, 0x99, 0x88, 0x77, 0]);
        let copy = log.with_opcode(opcode::COPY_AREA);
        let copy = copy.last().unwrap();
        assert_eq!((u32_at(copy, 4), u32_at(copy, 8)), (pixmap, win.xid()));
        assert_eq!(&copy[24..28], &[3, 0, 2, 0]);
    }

    #[test]
    fn present_rect_sends_only_the_area() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(100, 100)).unwrap();
        let px = vec![0xffffffu32; 100 * 100];
        conn.present_rect(win, &Frame::xrgb(&px, 100, 100).unwrap(), Rect::new(10, 20, 5, 4)).unwrap();
        conn.client().sync().unwrap();
        let log = server.finish();
        let put = log.with_opcode(opcode::PUT_IMAGE)[0];
        assert_eq!(&put[12..20], &[5, 0, 4, 0, 10, 0, 20, 0]);
        assert_eq!(put.len(), 24 + 5 * 4 * 4);
    }

    #[test]
    fn present_bands_under_a_small_request_limit() {
        let (mut conn, server) = open(Script { max_request_words: 4096, ..Default::default() });
        let win = conn.create_window(WindowOptions::new(275, 116)).unwrap();
        let px = vec![0x123456u32; 275 * 116];
        conn.present(win, &Frame::xrgb(&px, 275, 116).unwrap()).unwrap();
        conn.client().sync().unwrap();
        let log = server.finish();
        let puts = log.with_opcode(opcode::PUT_IMAGE);
        assert!(puts.len() > 1);
        assert!(puts.iter().all(|p| p.len() <= 4096 * 4));
        let rows: u32 = puts.iter().map(|p| u32::from(u16::from_le_bytes([p[14], p[15]]))).sum();
        assert_eq!(rows, 116);
    }

    #[test]
    fn present_converts_to_16_bit_and_big_endian_servers() {
        let (mut conn, server) = open(Script { screen: FakeScreen::Depth16, image_byte_order: 1, ..Default::default() });
        let win = conn.create_window(WindowOptions::new(2, 1)).unwrap();
        conn.present(win, &Frame::xrgb(&[0xff0000, 0x0000ff], 2, 1).unwrap()).unwrap();
        conn.client().sync().unwrap();
        let log = server.finish();
        let put = log.with_opcode(opcode::PUT_IMAGE)[0];
        assert_eq!(&put[24..28], &[0xf8, 0x00, 0x00, 0x1f], "RGB565, MSB first");
    }

    #[test]
    fn present_to_unknown_window_is_an_error() {
        let (mut conn, server) = open(Script::default());
        let frame = Frame::xrgb(&[0], 1, 1).unwrap();
        assert!(matches!(conn.present(WindowId(0xdead), &frame), Err(Error::InvalidArgument(_))));
        server.finish();
    }

    #[test]
    fn expose_repaints_from_the_backing_pixmap() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(50, 50)).unwrap();
        server.push_event(synth(12, 0, &[(4, &win.xid().to_le_bytes()), (8, &[5, 0, 6, 0, 7, 0, 8, 0])]));
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(100)).unwrap(), None, "no event surfaces");
        conn.client().sync().unwrap();
        let log = server.finish();
        let copy = log.with_opcode(opcode::COPY_AREA);
        let copy = copy.last().unwrap();
        assert_eq!(u32_at(copy, 8), win.xid());
        assert_eq!(&copy[16..28], &[5, 0, 6, 0, 5, 0, 6, 0, 7, 0, 8, 0]);
    }

    #[test]
    fn map_without_wm_takes_focus_once() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        let map = synth(19, 0, &[(8, &win.xid().to_le_bytes())]);
        server.push_event(map);
        server.push_event(map);
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(100)).unwrap(), None);
        conn.client().sync().unwrap();
        let focus = server.finish().with_opcode(opcode::SET_INPUT_FOCUS).len();
        assert_eq!(focus, 1);
    }

    #[test]
    fn map_with_wm_leaves_focus_to_it() {
        let (mut conn, server) = open(Script { window_manager: true, ..Default::default() });
        assert!(conn.has_window_manager());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        server.push_event(synth(19, 0, &[(8, &win.xid().to_le_bytes())]));
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(100)).unwrap(), None);
        conn.client().sync().unwrap();
        assert!(server.finish().with_opcode(opcode::SET_INPUT_FOCUS).is_empty());
    }

    #[test]
    fn keys_decode_with_modifiers() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        server.push_event(input(2, 8, win.xid(), 1, 2, state::SHIFT | state::CONTROL, 10));
        match next(&mut conn) {
            Event::Key { window, key, keysym, keycode, pressed, repeat, modifiers } => {
                assert_eq!((window, key, keysym, keycode, pressed, repeat), (win, Key::Character('A'), 0x41, 8, true, false));
                assert!(modifiers.shift && modifiers.ctrl && !modifiers.alt);
            }
            other => panic!("{other:?}"),
        }
        server.push_event(input(2, 12, win.xid(), 1, 2, state::MOD2, 11));
        assert!(matches!(next(&mut conn), Event::Key { key: Key::Character('7'), .. }), "Num Lock keypad");
        server.finish();
    }

    #[test]
    fn autorepeat_pairs_become_one_repeat_press() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        let mut burst = Vec::new();
        burst.extend_from_slice(&input(2, 8, win.xid(), 0, 0, 0, 100));
        burst.extend_from_slice(&input(3, 8, win.xid(), 0, 0, 0, 130));
        burst.extend_from_slice(&input(2, 8, win.xid(), 0, 0, 0, 130));
        burst.extend_from_slice(&input(3, 8, win.xid(), 0, 0, 0, 160));
        server.push_raw(burst);
        let mut got = Vec::new();
        for _ in 0..3 {
            if let Event::Key { pressed, repeat, .. } = next(&mut conn) {
                got.push((pressed, repeat));
            }
        }
        assert_eq!(got, [(true, false), (true, true), (false, false)]);
        server.finish();
    }

    #[test]
    fn buttons_wheel_and_motion() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        server.push_event(input(4, 1, win.xid(), 3, 4, 0, 1));
        assert_eq!(
            next(&mut conn),
            Event::Button {
                window: win,
                button: MouseButton::Left,
                pressed: true,
                x: 3,
                y: 4,
                root_x: 103,
                root_y: 54,
                modifiers: Modifiers::default(),
                time: 1
            }
        );
        server.push_event(input(4, 5, win.xid(), 3, 4, 0, 2));
        server.push_event(input(5, 5, win.xid(), 3, 4, 0, 3));
        assert!(matches!(next(&mut conn), Event::Scroll { dx: 0, dy: 1, .. }));
        server.push_event(input(4, 6, win.xid(), 3, 4, 0, 4));
        assert!(matches!(next(&mut conn), Event::Scroll { dx: -1, dy: 0, .. }), "wheel release is swallowed");
        server.push_event(input(5, 9, win.xid(), 0, 0, 0, 5));
        assert!(matches!(next(&mut conn), Event::Button { button: MouseButton::Forward, pressed: false, .. }));

        let mut burst = Vec::new();
        for i in 0..10 {
            burst.extend_from_slice(&input(6, 0, win.xid(), i, i, state::BUTTON1, 10 + i as u32));
        }
        burst.extend_from_slice(&input(5, 1, win.xid(), 9, 9, state::BUTTON1, 30));
        server.push_raw(burst);
        let Event::Motion { x, buttons, .. } = next(&mut conn) else { panic!() };
        assert_eq!(x, 9, "coalesced to the newest");
        assert!(buttons.left);
        assert!(matches!(next(&mut conn), Event::Button { pressed: false, .. }), "the event after the burst survives");
        server.finish();
    }

    #[test]
    fn close_request_and_focus_and_crossing() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        let close = req::client_message(win.xid(), server.atom("WM_PROTOCOLS"), [server.atom("WM_DELETE_WINDOW"), 0, 0, 0, 0]);
        server.push_event(close);
        assert_eq!(next(&mut conn), Event::CloseRequested { window: win });
        server.push_event(synth(9, 0, &[(4, &win.xid().to_le_bytes())]));
        assert_eq!(next(&mut conn), Event::Focused { window: win, focused: true });
        server.push_event(synth(10, 0, &[(4, &win.xid().to_le_bytes()), (8, &[1])]));
        server.push_event(input(7, 0, win.xid(), 2, 3, 0, 0));
        assert_eq!(next(&mut conn), Event::PointerEntered { window: win, x: 2, y: 3 }, "grab focus-out skipped");
        server.push_event(input(8, 0, win.xid(), 2, 3, 0, 0));
        assert_eq!(next(&mut conn), Event::PointerLeft { window: win });
        server.finish();
    }

    #[test]
    fn resize_replaces_the_backing_pixmap_and_collapses() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(10, 10)).unwrap();
        let cfg = |w: u16, h: u16| synth(22, 0, &[(8, &win.xid().to_le_bytes()), (20, &w.to_le_bytes()), (22, &h.to_le_bytes())]);
        server.push_event(cfg(10, 10)); // a move: same size, no event
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(50)).unwrap(), None);
        // A burst arriving together collapses to its last size.
        server.push_raw([cfg(20, 30), cfg(30, 40), cfg(40, 50)].concat());
        assert_eq!(next(&mut conn), Event::Resized { window: win, width: 40, height: 50 });
        assert_eq!(conn.window_size(win).unwrap(), (40, 50));
        assert_eq!(conn.poll_event().unwrap(), None);
        // A second resize arriving before the app reads the first replaces
        // it in the queue (both are applied to the pixmap).
        let mut feed = |w, h| {
            server.push_event(cfg(w, h));
            conn.client().sync().unwrap();
            let m = conn.client.poll_message().unwrap().unwrap();
            conn.handle(m).unwrap();
        };
        feed(60, 70);
        feed(80, 90);
        assert_eq!(conn.poll_event().unwrap(), Some(Event::Resized { window: win, width: 80, height: 90 }));
        assert_eq!(conn.poll_event().unwrap(), None);
        let log = server.finish();
        assert_eq!(log.with_opcode(opcode::CREATE_PIXMAP).len(), 4, "initial, 40x50, 60x70, 80x90");
        assert_eq!(log.with_opcode(opcode::FREE_PIXMAP).len(), 3);
    }

    #[test]
    fn async_errors_become_events() {
        let (mut conn, server) = open(Script { fail_opcodes: vec![opcode::CONFIGURE_WINDOW], ..Default::default() });
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        conn.set_position(win, 1, 1).unwrap();
        match next(&mut conn) {
            Event::Error(e) => assert_eq!(e.major_opcode, opcode::CONFIGURE_WINDOW),
            other => panic!("{other:?}"),
        }
        server.finish();
    }

    #[test]
    fn shape_requests_use_the_extension_opcode() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        conn.set_shape(win, &[Rect::new(0, 0, 5, 3)]).unwrap();
        conn.clear_shape(win).unwrap();
        conn.client().sync().unwrap();
        let log = server.finish();
        let shape = log.with_opcode(SHAPE_MAJOR);
        assert_eq!(shape.iter().map(|r| r[1]).collect::<Vec<_>>(), [1, 2]);

        let (mut conn, server) = open(Script { shape: false, ..Default::default() });
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        assert!(matches!(conn.set_shape(win, &[]), Err(Error::Unsupported(_))));
        server.finish();
    }

    #[test]
    fn drag_without_wm_moves_the_window_itself() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        // The fake reports the window's origin at (100, 50).
        conn.begin_drag_move(win, 110, 60, MouseButton::Left).unwrap();
        server.push_event(input(6, 0, win.xid(), 15, 13, state::BUTTON1, 1)); // root (115, 63)
        next(&mut conn);
        server.push_event(input(5, 1, win.xid(), 15, 13, state::BUTTON1, 2));
        next(&mut conn);
        server.push_event(input(6, 0, win.xid(), 30, 30, 0, 3));
        next(&mut conn);
        conn.client().sync().unwrap();
        let log = server.finish();
        let moves = log.with_opcode(opcode::CONFIGURE_WINDOW);
        assert_eq!(moves.len(), 1, "stops following after release");
        assert_eq!((u32_at(moves[0], 12) as i32, u32_at(moves[0], 16) as i32), (105, 53));
    }

    #[test]
    fn drag_with_wm_hands_over_via_moveresize() {
        let (mut conn, server) = open(Script { window_manager: true, ..Default::default() });
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        let supported = [server.atom("_NET_WM_MOVERESIZE")];
        conn.client().send(&req::change_property(PropMode::Replace, ROOT, server.atom("_NET_SUPPORTED"), atom::ATOM, PropData::U32(&supported))).unwrap();
        conn.begin_drag_move(win, 7, 8, MouseButton::Left).unwrap();
        conn.client().sync().unwrap();
        let moveresize = server.atom("_NET_WM_MOVERESIZE");
        let log = server.finish();
        assert_eq!(log.with_opcode(opcode::UNGRAB_POINTER).len(), 1);
        let send = log.with_opcode(opcode::SEND_EVENT);
        assert_eq!(u32_at(send[0], 4), ROOT);
        let ev = &send[0][12..44];
        assert_eq!(u32_at(ev, 8), moveresize);
        assert_eq!([u32_at(ev, 12), u32_at(ev, 16), u32_at(ev, 20), u32_at(ev, 24)], [7, 8, 8, 1]);
    }

    #[test]
    fn always_on_top_when_mapped_asks_the_wm() {
        let (mut conn, server) = open(Script { window_manager: true, ..Default::default() });
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        server.push_event(synth(19, 0, &[(8, &win.xid().to_le_bytes())]));
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(50)).unwrap(), None);
        conn.set_always_on_top(win, true).unwrap();
        conn.client().sync().unwrap();
        let log = server.finish();
        let send = log.with_opcode(opcode::SEND_EVENT);
        assert_eq!(send.len(), 1);
        assert_eq!(u32_at(&send[0][12..], 12), 1, "_NET_WM_STATE_ADD");
    }

    #[test]
    fn minimize_asks_the_wm_and_needs_one() {
        let (mut conn, server) = open(Script { window_manager: true, ..Default::default() });
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        server.push_event(synth(19, 0, &[(8, &win.xid().to_le_bytes())]));
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(50)).unwrap(), None);
        conn.minimize(win).unwrap();
        assert!(conn.minimize(WindowId::from_xid(0x1234)).is_err(), "not our window");
        conn.client().sync().unwrap();
        let change_state = server.atom("WM_CHANGE_STATE");
        let log = server.finish();
        let send = log.with_opcode(opcode::SEND_EVENT);
        assert_eq!(send.len(), 1);
        assert_eq!(u32_at(send[0], 4), ROOT, "sent to the root");
        assert_eq!(u32_at(&send[0][12..], 4), win.xid());
        assert_eq!(u32_at(&send[0][12..], 8), change_state);
        assert_eq!(u32_at(&send[0][12..], 12), 3, "IconicState");

        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        conn.minimize(win).unwrap();
        conn.client().sync().unwrap();
        assert!(server.finish().with_opcode(opcode::SEND_EVENT).is_empty(), "no WM: nothing to ask");
    }

    #[test]
    fn a_child_window_has_its_parent_and_no_top_level_properties() {
        let (mut conn, server) = open(Script::default());
        let top = conn.create_window(WindowOptions::new(50, 50)).unwrap();
        let bar = conn.create_window(WindowOptions::new(50, 10).parent(top.xid()).position(0, 3)).unwrap();
        conn.client().sync().unwrap();
        assert!(server.property(top.xid(), "WM_NAME").is_some());
        for p in ["WM_NAME", "WM_CLASS", "WM_NORMAL_HINTS", "WM_PROTOCOLS"] {
            assert!(server.property(bar.xid(), p).is_none(), "{p} on a child window");
        }
        let log = server.finish();
        let cw = log.with_opcode(opcode::CREATE_WINDOW);
        assert_eq!(u32_at(cw[0], 8), ROOT);
        assert_eq!(u32_at(cw[1], 8), top.xid(), "created inside its parent");
        assert_eq!(i16::from_le_bytes([cw[1][14], cw[1][15]]), 3);
    }

    #[test]
    fn resizing_keeps_the_position_hint() {
        let (mut conn, server) = open(Script::default());
        let w = conn.create_window(WindowOptions::new(5, 5).position(10, 20).resizable(false)).unwrap();
        conn.set_size(w, 7, 9).unwrap();
        conn.client().sync().unwrap();
        let hints = server.property(w.xid(), "WM_NORMAL_HINTS").unwrap().2;
        let hint = |i: usize| u32_at(&hints, i * 4);
        assert_eq!(hint(0) & 1, 1, "USPosition still set");
        assert_eq!((hint(1), hint(2), hint(5), hint(7)), (10, 20, 7, 7));
        // After a move, the hint follows it.
        conn.set_position(w, 30, 40).unwrap();
        conn.set_size(w, 8, 8).unwrap();
        conn.client().sync().unwrap();
        let hints = server.property(w.xid(), "WM_NORMAL_HINTS").unwrap().2;
        assert_eq!((u32_at(&hints, 4), u32_at(&hints, 8)), (30, 40));
        server.finish();
    }

    #[test]
    fn transient_owner_and_position() {
        let (mut conn, server) = open(Script::default());
        let main = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        let tool = conn.create_window(WindowOptions::new(5, 5).transient_for(main)).unwrap();
        // The fake answers TranslateCoordinates with (x + 100, y + 50).
        assert_eq!(conn.window_position(tool).unwrap(), (100, 50));
        assert!(conn.window_position(WindowId::from_xid(0x4321)).is_err(), "not ours");
        conn.client().sync().unwrap();
        let p = server.property(tool.xid(), "WM_TRANSIENT_FOR").unwrap();
        assert_eq!((p.0, p.1, p.2), (atom::WINDOW, 32, main.xid().to_le_bytes().to_vec()));
        assert!(server.property(main.xid(), "WM_TRANSIENT_FOR").is_none());
        server.finish();
    }

    #[test]
    fn icon_property_is_argb_with_size_prefix() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        conn.set_icon(win, &Frame::xrgb(&[0x123456, 0x654321], 2, 1).unwrap()).unwrap();
        conn.client().sync().unwrap();
        let icon = server.property(win.xid(), "_NET_WM_ICON").unwrap();
        assert_eq!(icon.1, 32);
        let words: Vec<u32> = icon.2.chunks(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
        assert_eq!(words, [2, 1, 0xff12_3456, 0xff65_4321]);
        server.finish();
    }

    #[test]
    fn destroy_frees_resources_and_forgets_the_window() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        conn.destroy_window(win).unwrap();
        assert!(conn.window_size(win).is_err());
        assert!(conn.destroy_window(win).is_err());
        conn.client().sync().unwrap();
        let log = server.finish();
        assert_eq!(log.with_opcode(opcode::FREE_GC).len(), 1);
        assert_eq!(log.with_opcode(opcode::FREE_PIXMAP).len(), 1);
        assert_eq!(log.with_opcode(opcode::DESTROY_WINDOW).len(), 1);
    }

    #[test]
    fn events_for_foreign_windows_are_ignored() {
        let (mut conn, server) = open(Script::default());
        let _win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        server.push_event(input(2, 8, 0x999, 0, 0, 0, 1));
        server.push_event(synth(9, 0, &[(4, &FOCUS_WINDOW.to_le_bytes())]));
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(50)).unwrap(), None);
        server.finish();
    }

    #[test]
    fn a_recycled_id_is_not_hit_by_the_old_windows_destroy_notify() {
        let (mut conn, server) = open(Script::default());
        let a = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        conn.destroy_window(a).unwrap();
        let b = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        assert_ne!(a, b, "the id is held until the server confirms the destroy");
        server.push_event(synth(10, 0, &[(4, &a.xid().to_le_bytes())])); // late FocusOut
        server.push_event(synth(17, 0, &[(4, &a.xid().to_le_bytes()), (8, &a.xid().to_le_bytes())]));
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(50)).unwrap(), None);
        assert!(conn.window_size(b).is_ok());
        let c = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        assert_eq!(c, a, "released once its DestroyNotify arrived");
        server.finish();
    }

    #[test]
    fn a_window_destroyed_by_someone_else_is_reported() {
        let (mut conn, server) = open(Script::default());
        let a = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        server.push_event(synth(17, 0, &[(4, &a.xid().to_le_bytes()), (8, &a.xid().to_le_bytes())]));
        assert_eq!(next(&mut conn), Event::Destroyed { window: a });
        assert!(conn.window_size(a).is_err());
        server.finish();
    }

    #[test]
    fn a_failed_create_keeps_unrelated_queued_events() {
        let (mut conn, server) = open(Script { fail_from: vec![(opcode::CREATE_PIXMAP, 1)], ..Default::default() });
        let a = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        server.push_event(input(2, 8, a.xid(), 0, 0, 0, 1));
        conn.client().sync().unwrap(); // the key press is now queued
        assert!(matches!(conn.create_window(WindowOptions::new(5, 5)), Err(Error::X(_))));
        assert!(matches!(next(&mut conn), Event::Key { window, .. } if window == a));
        server.finish();
    }

    #[test]
    fn present_rect_overhanging_everything_sends_well_formed_requests() {
        // The fake rejects PutImage data that disagrees with its geometry.
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(100, 50)).unwrap();
        let px = vec![0x123456u32; 30 * 30];
        let frame = Frame::xrgb(&px, 30, 30).unwrap();
        for area in [Rect::new(50, 0, 100, 10), Rect::new(0, 0, 200, 200), Rect::new(-5, -5, 12, 12), Rect::new(29, 29, 5, 5)] {
            conn.present_rect(win, &frame, area).unwrap();
        }
        conn.client().sync().unwrap();
        assert_eq!(conn.poll_event().unwrap(), None, "no BadLength");
        let log = server.finish();
        let widths: Vec<u16> = log.with_opcode(opcode::PUT_IMAGE).iter().map(|r| u16::from_le_bytes([r[12], r[13]])).collect();
        assert_eq!(widths, [30, 7, 1], "the first area misses the frame entirely");
    }

    #[test]
    fn scroll_copies_only_within_the_area() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(40, 20)).unwrap();
        conn.scroll(win, Rect::new(10, 0, 20, 20), -5, 0).unwrap();
        conn.client().sync().unwrap();
        let log = server.finish();
        let copies = log.with_opcode(opcode::COPY_AREA);
        let pix = &copies[copies.len() - 2];
        // src (15,0) -> dst (10,0), 15 wide: never writes left of x=10.
        assert_eq!(&pix[16..28], &[15, 0, 0, 0, 10, 0, 0, 0, 15, 0, 20, 0]);
    }

    #[test]
    fn grab_crossings_are_not_reported() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        let mut leave = input(8, 0, win.xid(), 1, 1, 0, 0);
        leave[30] = 1; // NotifyGrab
        server.push_event(leave);
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(50)).unwrap(), None);
        server.finish();
    }

    #[test]
    fn reshowing_a_window_without_a_wm_refocuses_it() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        let map = synth(19, 0, &[(8, &win.xid().to_le_bytes())]);
        let unmap = synth(18, 0, &[(8, &win.xid().to_le_bytes())]);
        server.push_raw([map, unmap, map].concat());
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(50)).unwrap(), None);
        conn.client().sync().unwrap();
        assert_eq!(server.finish().with_opcode(opcode::SET_INPUT_FOCUS).len(), 2);
    }

    #[test]
    fn pending_wakes_collapse_even_with_events_between() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        conn.wake_window = Some(0x77);
        let wake = req::client_message(0x77, conn.atoms.wake, [0; 5]);
        server.push_raw([wake, input(6, 0, win.xid(), 1, 1, 0, 0), wake].concat());
        conn.client().sync().unwrap();
        // All three arrive before the app looks: decode them all, then read.
        while let Some(m) = conn.client.poll_message().unwrap() {
            conn.handle(m).unwrap();
        }
        let got: Vec<Event> = conn.events.drain(..).collect();
        assert_eq!(got.iter().filter(|e| **e == Event::Wake).count(), 1, "{got:?}");
        server.finish();
    }

    #[test]
    fn with_detectable_autorepeat_a_press_of_a_held_key_is_a_repeat() {
        let (mut conn, server) = open(Script::default());
        let win = conn.create_window(WindowOptions::new(5, 5)).unwrap();
        conn.detectable_repeat = true;
        let mut got = Vec::new();
        for (code, t) in [(2, 1), (2, 2), (2, 3), (3, 4), (2, 5)] {
            server.push_event(input(code, 8, win.xid(), 0, 0, 0, t));
            if let Event::Key { pressed, repeat, .. } = next(&mut conn) {
                got.push((pressed, repeat));
            }
        }
        assert_eq!(got, [(true, false), (true, true), (true, true), (false, false), (true, false)]);
        server.finish();
    }

    #[test]
    fn waker_needs_a_named_display() {
        let (mut conn, server) = open(Script::default());
        assert!(matches!(conn.waker(), Err(Error::Unsupported(_))));
        server.finish();
    }

    #[test]
    fn mapping_notify_reloads_the_keymap() {
        let (mut conn, server) = open(Script::default());
        server.push_event(synth(34, 0, &[(4, &[1, 8, 5])]));
        assert_eq!(conn.wait_event_timeout(Duration::from_millis(50)).unwrap(), None);
        conn.client().sync().unwrap();
        assert_eq!(server.finish().with_opcode(opcode::GET_KEYBOARD_MAPPING).len(), 2);
    }
}
