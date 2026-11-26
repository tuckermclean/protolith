//! End-to-end tests against a real X server (Xvfb).
//!
//! Each test starts its own Xvfb on a free display (`-displayfd`), so tests
//! are isolated and run in parallel. Pixels are verified by reading the
//! *window* back with `GetImage`, input is injected with the XTEST
//! extension, and window properties are cross-checked with `xprop` when it
//! is installed — independent oracles, not protolith checking itself.
//!
//! Without Xvfb on `PATH` the tests skip (printing why), unless
//! `PROTOLITH_REQUIRE_XVFB` is set — as it is in CI — in which case a
//! missing Xvfb fails them.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use protolith::x11::proto::event::RawEvent;
use protolith::x11::proto::request as req;
use protolith::x11::{Auth, Client, Connection, DisplayName, Event, Key, Message, MouseButton, WindowId, WindowOptions};
use protolith::{Error, Frame, PixelFormat, Rect};

// ------------------------------------------------------------------ harness

struct Xvfb {
    child: Child,
    display: u32,
    _auth: Option<TempFile>,
}

impl Drop for Xvfb {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct TempFile(std::path::PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn temp_path(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    std::env::temp_dir().join(format!("protolith-{}-{}-{tag}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)))
}

/// Start Xvfb with `depth` and extra arguments; `None` (test skipped) if
/// Xvfb is not installed.
fn xvfb(depth: u8, extra: &[&str], auth: Option<TempFile>) -> Option<Xvfb> {
    let screen = format!("640x480x{depth}");
    let listen = if extra.contains(&"-listen") { &[][..] } else { &["-nolisten", "tcp"][..] };
    let spawned =
        Command::new("Xvfb").args(["-displayfd", "1", "-screen", "0", &screen]).args(listen).args(extra).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match spawned {
        Ok(c) => c,
        Err(e) => {
            if std::env::var_os("PROTOLITH_REQUIRE_XVFB").is_some() {
                panic!("Xvfb is required but could not be started: {e}");
            }
            eprintln!("skipping: Xvfb not available ({e})");
            return None;
        }
    };
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).expect("Xvfb display number");
    let display = line.trim().parse().unwrap_or_else(|_| panic!("Xvfb printed {line:?}"));
    Some(Xvfb { child, display, _auth: auth })
}

fn name(x: &Xvfb) -> String {
    format!(":{}", x.display)
}

fn open(x: &Xvfb) -> Connection {
    Connection::open_display(&name(x), Some(Auth::none())).expect("connect to Xvfb")
}

fn raw_client(x: &Xvfb) -> Client {
    Client::connect(&DisplayName::parse(&name(x)).unwrap(), Some(Auth::none())).unwrap()
}

/// Drain events for `d`, returning them.
fn pump(conn: &mut Connection, d: Duration) -> Vec<Event> {
    let end = Instant::now() + d;
    let mut out = Vec::new();
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        match conn.wait_event_timeout(left).unwrap() {
            Some(e) => out.push(e),
            None => break,
        }
    }
    out
}

/// Pump events until `pred` matches one (or 5 s pass).
fn wait_for(conn: &mut Connection, mut pred: impl FnMut(&Event) -> bool) -> Event {
    let end = Instant::now() + Duration::from_secs(5);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Some(e) = conn.wait_event_timeout(left).unwrap() {
            if pred(&e) {
                return e;
            }
        }
    }
    panic!("timed out waiting for an event");
}

/// Read the window's on-screen pixels with GetImage, as `0x00RRGGBB`.
fn screen_pixels(conn: &mut Connection, win: WindowId, area: Rect) -> Vec<u32> {
    let fmt = conn.pixel_format();
    let depth = conn.depth();
    let c = conn.client();
    let reply = c.round_trip(&req::get_image(win.xid(), area.x as i16, area.y as i16, area.width as u16, area.height as u16)).expect("GetImage");
    let (_, _, data) = req::parse_image(&reply).unwrap();
    let stride = c.setup().format(depth).unwrap().row_stride(area.width);
    let bpp = fmt.bytes_per_pixel();
    let mut out = Vec::new();
    for y in 0..area.height as usize {
        for x in 0..area.width as usize {
            out.push(fmt.unpack(fmt.load(&data[y * stride + x * bpp..]).unwrap()));
        }
    }
    out
}

/// Pump events until the window shows `expected` in `area`.
fn assert_shows(conn: &mut Connection, win: WindowId, area: Rect, expected: &[u32]) {
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        pump(conn, Duration::from_millis(20));
        let got = screen_pixels(conn, win, area);
        if got == expected {
            return;
        }
        if Instant::now() > end {
            let bad = got.iter().zip(expected).position(|(g, e)| g != e).unwrap();
            panic!("pixel {bad} of {area:?}: got {:06x}, expected {:06x}", got[bad], expected[bad]);
        }
    }
}

/// A pattern exercising every channel bit: gradients plus a hash.
fn pattern(w: u32, h: u32, seed: u32) -> Vec<u32> {
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let hash = (i ^ seed).wrapping_mul(0x9e37_79b9) >> 8;
            ((x * 255 / w.max(2)) << 16 | (y * 255 / h.max(2)) << 8 | (hash & 0xff)) & 0xff_ffff
        })
        .collect()
}

fn quantized(fmt: PixelFormat, px: &[u32]) -> Vec<u32> {
    px.iter().map(|&p| fmt.quantize(p)).collect()
}

fn intern(c: &mut Client, n: &str) -> u32 {
    req::parse_intern_atom(&c.round_trip(&req::intern_atom(n, false)).unwrap()).unwrap()
}

// -------------------------------------------------------------- XTEST input

struct Xtest {
    major: u8,
    root: u32,
}

impl Xtest {
    fn new(conn: &mut Connection) -> Xtest {
        let reply = conn.client().round_trip(&req::query_extension("XTEST")).unwrap();
        let info = req::parse_query_extension(&reply).unwrap().expect("Xvfb has XTEST");
        Xtest { major: info.major_opcode, root: conn.client().screen().root }
    }

    /// XTestFakeInput: event `kind` (2 KeyPress .. 6 Motion), `detail`, at
    /// root `(x, y)` for motion.
    fn fake(&self, conn: &mut Connection, kind: u8, detail: u8, x: i16, y: i16) {
        let mut r = vec![self.major, 2, 9, 0, kind, detail, 0, 0];
        r.extend_from_slice(&0u32.to_le_bytes()); // time: now
        r.extend_from_slice(&self.root.to_le_bytes());
        r.extend_from_slice(&[0; 8]);
        r.extend_from_slice(&x.to_le_bytes());
        r.extend_from_slice(&y.to_le_bytes());
        r.extend_from_slice(&[0; 8]);
        conn.client().send(&r).unwrap();
        conn.client().flush().unwrap();
    }

    fn move_to(&self, conn: &mut Connection, x: i32, y: i32) {
        self.fake(conn, 6, 0, x as i16, y as i16);
    }
}

/// Window origin on the root.
fn window_origin(conn: &mut Connection, win: WindowId) -> (i32, i32) {
    let root = conn.client().screen().root;
    let reply = conn.client().round_trip(&req::translate_coordinates(win.xid(), root, 0, 0)).unwrap();
    let (x, y) = req::parse_translate_coordinates(&reply).unwrap();
    (i32::from(x), i32::from(y))
}

// =================================================================== tests

fn presents_exact_pixels_at(depth: u8) {
    let Some(x) = xvfb(depth, &[], None) else { return };
    let mut conn = open(&x);
    let (w, h) = (97, 61); // odd sizes catch stride bugs
    let win = conn.create_window(WindowOptions::new(w, h).position(0, 0)).unwrap();
    let px = pattern(w, h, u32::from(depth));
    conn.present(win, &Frame::xrgb(&px, w, h).unwrap()).unwrap();
    let expected = quantized(conn.pixel_format(), &px);
    assert_shows(&mut conn, win, Rect::from_size(w, h), &expected);
    assert_eq!(conn.read_pixels(win, Rect::from_size(w, h)).unwrap(), expected, "backing store agrees");
}

#[test]
fn presents_exact_pixels_at_depth_24() {
    presents_exact_pixels_at(24);
}

#[test]
fn presents_exact_pixels_at_depth_16() {
    presents_exact_pixels_at(16);
}

/// An 8-bit PseudoColor root: protolith must find the 8-bit TrueColor
/// visual, give the window its own colormap, and pack BGR233.
#[test]
fn presents_exact_pixels_at_depth_8_via_a_non_root_visual() {
    presents_exact_pixels_at(8);
}

#[test]
fn rgba_frames_present_like_xrgb() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(16, 16).position(0, 0)).unwrap();
    let px = pattern(16, 16, 7);
    let bytes: Vec<u8> = px.iter().flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 0x80]).collect();
    conn.present(win, &Frame::rgba8(&bytes, 16, 16).unwrap()).unwrap();
    assert_shows(&mut conn, win, Rect::from_size(16, 16), &px);
}

#[test]
fn large_frames_are_banded_under_the_request_limit() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    // 640x480x4 = 1.2 MB, far past the 256 KB core request limit.
    let win = conn.create_window(WindowOptions::new(640, 480).position(0, 0)).unwrap();
    let px = pattern(640, 480, 3);
    conn.present(win, &Frame::xrgb(&px, 640, 480).unwrap()).unwrap();
    assert_shows(&mut conn, win, Rect::from_size(640, 480), &px);
}

#[test]
fn present_rect_updates_only_its_area() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let (w, h) = (40, 30);
    let win = conn.create_window(WindowOptions::new(w, h).position(0, 0)).unwrap();
    let a = pattern(w, h, 1);
    let b = vec![0x00ff_00ffu32; (w * h) as usize];
    conn.present(win, &Frame::xrgb(&a, w, h).unwrap()).unwrap();
    let area = Rect::new(5, 7, 11, 13);
    conn.present_rect(win, &Frame::xrgb(&b, w, h).unwrap(), area).unwrap();
    let expected: Vec<u32> = (0..w * h).map(|i| if area.contains((i % w) as i32, (i / w) as i32) { b[i as usize] } else { a[i as usize] }).collect();
    assert_shows(&mut conn, win, Rect::from_size(w, h), &expected);
}

#[test]
fn scroll_moves_pixels_on_the_server() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let (w, h) = (20, 40);
    let win = conn.create_window(WindowOptions::new(w, h).position(0, 0)).unwrap();
    let px = pattern(w, h, 9);
    conn.present(win, &Frame::xrgb(&px, w, h).unwrap()).unwrap();
    conn.scroll(win, Rect::from_size(w, h), 0, -10).unwrap();
    // Rows 10.. moved up to 0..; the bottom 10 rows are stale (unchanged).
    let mut expected = px[(10 * w) as usize..].to_vec();
    expected.extend_from_slice(&px[((h - 10) * w) as usize..]);
    assert_shows(&mut conn, win, Rect::from_size(w, h), &expected);
}

#[test]
fn exposure_is_repainted_from_the_backing_store() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let (w, h) = (30, 30);
    let win = conn.create_window(WindowOptions::new(w, h).position(0, 0)).unwrap();
    let px = pattern(w, h, 4);
    conn.present(win, &Frame::xrgb(&px, w, h).unwrap()).unwrap();
    assert_shows(&mut conn, win, Rect::from_size(w, h), &px);
    // Cover it with another window, then uncover it: the server discards
    // the covered pixels (Xvfb keeps no backing store) and sends Expose.
    let cover = conn.create_window(WindowOptions::new(w, h).position(0, 0).override_redirect(true)).unwrap();
    let black = vec![0u32; (w * h) as usize];
    assert_shows(&mut conn, cover, Rect::from_size(w, h), &black);
    conn.destroy_window(cover).unwrap();
    assert_shows(&mut conn, win, Rect::from_size(w, h), &px);
}

#[test]
fn window_properties_reach_the_server() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(275, 116).title("protolith ♪").class("amp", "Amp").decorations(false).resizable(false)).unwrap();
    conn.client().sync().unwrap();

    // Our own decoder...
    let net_wm_name = intern(conn.client(), "_NET_WM_NAME");
    let mut prop = |a| req::parse_property(&conn.client().round_trip(&req::get_property(false, win.xid(), a, 0, 0, 1024)).unwrap()).unwrap();
    assert_eq!(prop(net_wm_name).value, "protolith ♪".as_bytes());
    assert_eq!(prop(protolith::x11::proto::atom::WM_CLASS).value, b"amp\0Amp\0");

    // ...and an independent one, when installed.
    match Command::new("xprop").args(["-display", &name(&x), "-id", &format!("{:#x}", win.xid())]).output() {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(text.contains("_NET_WM_NAME(UTF8_STRING) = \"protolith ♪\""), "{text}");
            assert!(text.contains("WM_CLASS(STRING) = \"amp\", \"Amp\""), "{text}");
            assert!(text.contains("WM_PROTOCOLS(ATOM): protocols  WM_DELETE_WINDOW"), "{text}");
            assert!(text.contains("program specified minimum size: 275 by 116"), "{text}");
            assert!(text.contains("program specified maximum size: 275 by 116"), "{text}");
            assert!(text.contains(&format!("_NET_WM_PID(CARDINAL) = {}", std::process::id())), "{text}");
        }
        Err(e) => eprintln!("xprop cross-check skipped: {e}"),
    }
}

#[test]
fn shape_reaches_the_server() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    assert!(conn.supports_shape());
    let win = conn.create_window(WindowOptions::new(20, 10).visible(false)).unwrap();
    let mask = |x: u32, y: u32| (3..17).contains(&x) || (2..8).contains(&y);
    let rects = protolith::x11::shape::rects_from_mask(20, 10, mask);
    conn.set_shape(win, &rects).unwrap();

    let major = req::parse_query_extension(&conn.client().round_trip(&req::query_extension("SHAPE")).unwrap()).unwrap().unwrap().major_opcode;
    let reply = conn.client().round_trip(&req::shape::get_rectangles(major, win.xid(), req::shape::Kind::Bounding)).unwrap();
    let got = req::shape::parse_rectangles(&reply).unwrap();
    let covered = |rs: &[Rect]| (0..200).filter(|i| rs.iter().any(|r| r.contains(i % 20, i / 20))).collect::<Vec<i32>>();
    assert_eq!(covered(&got), covered(&rects), "server region covers exactly the mask");
    assert_eq!(covered(&got).len(), 200 - 4 * 3 * 2);

    conn.clear_shape(win).unwrap();
    let reply = conn.client().round_trip(&req::shape::get_rectangles(major, win.xid(), req::shape::Kind::Bounding)).unwrap();
    assert_eq!(req::shape::parse_rectangles(&reply).unwrap(), [Rect::new(0, 0, 20, 10)]);
}

#[test]
fn keyboard_input_with_shift() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(50, 50).position(0, 0)).unwrap();
    // Without a WM, protolith focuses the window itself once it is mapped.
    wait_for(&mut conn, |e| matches!(e, Event::Focused { focused: true, .. }));
    let xt = Xtest::new(&mut conn);
    let (a, _) = conn.keymap().keycode_for(u32::from('a')).expect("'a' on the keymap");
    let (shift, _) = conn.keymap().keycode_for(0xffe1).expect("Shift_L on the keymap");

    for (kind, code) in [(2, a), (3, a), (2, shift), (2, a), (3, a), (3, shift)] {
        xt.fake(&mut conn, kind, code, 0, 0);
    }
    let keys: Vec<(Key, bool, bool)> = pump(&mut conn, Duration::from_millis(300))
        .into_iter()
        .filter_map(|e| match e {
            Event::Key { window, key, pressed, repeat, .. } if window == win => Some((key, pressed, repeat)),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys,
        [
            (Key::Character('a'), true, false),
            (Key::Character('a'), false, false),
            (Key::Shift, true, false),
            (Key::Character('A'), true, false),
            (Key::Character('A'), false, false),
            (Key::Shift, false, false)
        ]
    );
}

#[test]
fn pointer_input_arrives_with_window_and_root_coordinates() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(100, 80).position(30, 40)).unwrap();
    wait_for(&mut conn, |e| matches!(e, Event::Focused { .. }));
    let (ox, oy) = window_origin(&mut conn, win);
    assert_eq!((ox, oy), (30, 40));
    let xt = Xtest::new(&mut conn);
    xt.move_to(&mut conn, ox + 10, oy + 20);
    xt.fake(&mut conn, 4, 1, 0, 0);
    xt.fake(&mut conn, 5, 1, 0, 0);
    xt.fake(&mut conn, 4, 5, 0, 0);
    xt.fake(&mut conn, 5, 5, 0, 0);
    let events = pump(&mut conn, Duration::from_millis(300));
    assert!(events.iter().any(|e| matches!(*e, Event::Motion { window, x: 10, y: 20, .. } if window == win)), "{events:?}");
    let presses: Vec<u32> = events
        .iter()
        .filter_map(|e| match *e {
            Event::Button { window, button: MouseButton::Left, pressed: true, x: 10, y: 20, root_x: 40, root_y: 60, modifiers, time }
                if window == win && modifiers == Default::default() =>
            {
                Some(time)
            }
            _ => None,
        })
        .collect();
    assert_eq!(presses.len(), 1, "{events:?}");
    assert!(presses[0] > 0, "a real server timestamp, not CurrentTime");
    let release = events.iter().find_map(|e| match *e {
        Event::Button { button: MouseButton::Left, pressed: false, time, .. } => Some(time),
        _ => None,
    });
    assert!(release.unwrap().wrapping_sub(presses[0]) < 1000, "server timestamps, press then release: {presses:?} {release:?}");
    assert!(events.iter().any(|e| matches!(*e, Event::Button { button: MouseButton::Left, pressed: false, .. })));
    assert!(events.iter().any(|e| matches!(*e, Event::Scroll { dy: 1, x: 10, y: 20, .. })));
}

#[test]
fn drag_move_without_a_window_manager() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(100, 50).position(100, 100).decorations(false)).unwrap();
    wait_for(&mut conn, |e| matches!(e, Event::Focused { .. }));
    let xt = Xtest::new(&mut conn);
    xt.move_to(&mut conn, 105, 105);
    xt.fake(&mut conn, 4, 1, 0, 0);
    let Event::Button { root_x, root_y, button, .. } = wait_for(&mut conn, |e| matches!(e, Event::Button { pressed: true, .. })) else { unreachable!() };
    assert_eq!((root_x, root_y), (105, 105));
    conn.begin_drag_move(win, root_x, root_y, button).unwrap();
    xt.move_to(&mut conn, 135, 125);
    pump(&mut conn, Duration::from_millis(100));
    xt.fake(&mut conn, 5, 1, 0, 0);
    xt.move_to(&mut conn, 400, 400);
    pump(&mut conn, Duration::from_millis(100));
    assert_eq!(window_origin(&mut conn, win), (130, 120), "followed the drag, then stopped at release");
}

#[test]
fn close_request_from_another_client() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(10, 10)).unwrap();
    // Play window manager: send WM_DELETE_WINDOW the way a WM does.
    let mut other = raw_client(&x);
    let (protocols, delete) = (intern(&mut other, "WM_PROTOCOLS"), intern(&mut other, "WM_DELETE_WINDOW"));
    other.send(&req::send_event(false, win.xid(), 0, &req::client_message(win.xid(), protocols, [delete, 0, 0, 0, 0]))).unwrap();
    other.flush().unwrap();
    assert_eq!(wait_for(&mut conn, |e| matches!(e, Event::CloseRequested { .. })), Event::CloseRequested { window: win });
}

#[test]
fn resize_from_outside_is_reported_and_presentable() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(20, 20).position(0, 0)).unwrap();
    let mut other = raw_client(&x);
    other.send(&req::configure_window(win.xid(), &req::ConfigureWindow { width: Some(64), height: Some(48), ..Default::default() })).unwrap();
    other.flush().unwrap();
    assert_eq!(wait_for(&mut conn, |e| matches!(e, Event::Resized { .. })), Event::Resized { window: win, width: 64, height: 48 });
    let px = pattern(64, 48, 11);
    conn.present(win, &Frame::xrgb(&px, 64, 48).unwrap()).unwrap();
    assert_shows(&mut conn, win, Rect::from_size(64, 48), &px);
}

#[test]
fn set_size_and_position_apply() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(20, 20).position(0, 0).resizable(false)).unwrap();
    conn.set_size(win, 50, 40).unwrap();
    assert_eq!(wait_for(&mut conn, |e| matches!(e, Event::Resized { .. })), Event::Resized { window: win, width: 50, height: 40 });
    conn.set_position(win, 123, 45).unwrap();
    pump(&mut conn, Duration::from_millis(50));
    assert_eq!(window_origin(&mut conn, win), (123, 45));
    assert_eq!(conn.window_position(win).unwrap(), (123, 45), "agrees with a raw TranslateCoordinates");
    // A window created at a position, with no window manager to move it.
    let other = conn.create_window(WindowOptions::new(20, 20).position(31, 17).transient_for(win)).unwrap();
    pump(&mut conn, Duration::from_millis(50));
    assert_eq!(conn.window_position(other).unwrap(), (31, 17));
}

#[test]
fn waker_interrupts_a_blocking_wait_from_another_thread() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let _win = conn.create_window(WindowOptions::new(10, 10)).unwrap();
    pump(&mut conn, Duration::from_millis(50));
    let waker = conn.waker().unwrap();
    // The thread wakes once and exits at once, dropping its connection:
    // the regression case where the server discarded the unread wake.
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        waker.wake().unwrap();
    });
    // The real blocking wait. A watchdog turns a lost wake into a test
    // failure instead of a hung CI job.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let watchdog = {
        let done = done.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            if !done.load(std::sync::atomic::Ordering::SeqCst) {
                eprintln!("waker test: wait_event() never returned");
                std::process::exit(101);
            }
        })
    };
    let start = Instant::now();
    assert_eq!(conn.wait_event().unwrap(), Event::Wake);
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    drop(watchdog);
    let waited = start.elapsed();
    assert!(waited >= Duration::from_millis(50), "{waited:?}");
    t.join().unwrap();
}

#[test]
fn async_errors_surface_as_events() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    conn.client().send(&req::map_window(0x0dea_dbee)).unwrap();
    match wait_for(&mut conn, |e| matches!(e, Event::Error(_))) {
        Event::Error(e) => {
            assert_eq!((e.code_name(), e.major_opcode, e.bad_value), ("BadWindow", req::opcode::MAP_WINDOW, 0x0dea_dbee));
        }
        _ => unreachable!(),
    }
}

#[test]
fn sequence_numbers_survive_wraparound_on_a_real_server() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let c = conn.client();
    for _ in 0..70_000 {
        c.send(&req::no_operation()).unwrap();
    }
    let bad = c.send(&req::map_window(0x0bad_0001)).unwrap(); // an error past the wrap
    assert!(bad > 70_000);
    let reply = c.round_trip(&req::get_input_focus()).unwrap();
    assert_eq!(reply[0], 1);
    match wait_for(&mut conn, |e| matches!(e, Event::Error(_))) {
        Event::Error(e) => assert_eq!(e.sequence, bad, "error attributed to the right request"),
        _ => unreachable!(),
    }
}

#[test]
fn tcp_display_connects() {
    let Some(x) = xvfb(24, &["-listen", "tcp"], None) else { return };
    let mut conn = match Connection::open_display(&format!("localhost:{}", x.display), Some(Auth::none())) {
        Ok(c) => c,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            eprintln!("skipping: this Xvfb does not listen on TCP ({e})");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let win = conn.create_window(WindowOptions::new(8, 8).position(0, 0)).unwrap();
    let px = pattern(8, 8, 5);
    conn.present(win, &Frame::xrgb(&px, 8, 8).unwrap()).unwrap();
    assert_shows(&mut conn, win, Rect::from_size(8, 8), &px);
}

#[test]
fn magic_cookie_authentication() {
    use protolith::x11::display::{encode_xauthority, family, AuthEntry};
    let cookie: Vec<u8> = (0..16).map(|i| (i * 37 + 11) as u8).collect();
    let path = temp_path("xauth");
    let entry = AuthEntry { family: family::WILD, address: vec![], number: vec![], name: b"MIT-MAGIC-COOKIE-1".to_vec(), data: cookie.clone() };
    std::fs::write(&path, encode_xauthority(&[entry])).unwrap();
    let path_str = path.to_str().unwrap().to_string();
    let Some(x) = xvfb(24, &["-auth", &path_str], Some(TempFile(path))) else { return };

    // Keep this connection open: when an X server's last client leaves it
    // resets, and a connection racing that reset is dropped mid-handshake.
    let _held = Connection::open_display(&name(&x), Some(Auth::cookie(&cookie))).expect("right cookie");
    match Connection::open_display(&name(&x), Some(Auth::cookie(&[0; 16]))) {
        Err(Error::Setup(e)) => assert!(e.to_string().contains("refused"), "{e}"),
        other => panic!("wrong cookie must be refused, got {other:?}"),
    }
    assert!(matches!(Connection::open_display(&name(&x), Some(Auth::none())), Err(Error::Setup(_))), "no cookie");
}

#[test]
fn raw_client_sees_raw_events() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut c = raw_client(&x);
    let wid = c.generate_id().unwrap();
    let root = c.screen().root;
    c.send(&req::create_window(&req::CreateWindow {
        wid,
        parent: root,
        depth: 0,
        x: 0,
        y: 0,
        width: 10,
        height: 10,
        border_width: 0,
        class: req::WindowClass::InputOutput,
        visual: 0,
        attributes: req::WindowAttributes { event_mask: Some(req::event_mask::STRUCTURE_NOTIFY), ..Default::default() },
    }))
    .unwrap();
    c.send(&req::map_window(wid)).unwrap();
    match c.wait_message(Some(Duration::from_secs(5))).unwrap() {
        Some(Message::Event(e)) => assert_eq!(e.event, RawEvent::MapNotify { window: wid }),
        other => panic!("{other:?}"),
    }
    let geom = req::parse_geometry(&c.round_trip(&req::get_geometry(wid)).unwrap()).unwrap();
    assert_eq!((geom.width, geom.height, geom.root, geom.depth), (10, 10, root, 24));
}

// ------------------------------------------------- review regressions

#[test]
fn a_recreated_window_survives_its_predecessors_late_events() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let a = conn.create_window(WindowOptions::new(30, 30).position(0, 0)).unwrap();
    wait_for(&mut conn, |e| matches!(e, Event::Focused { focused: true, .. }));
    conn.destroy_window(a).unwrap();
    let b = conn.create_window(WindowOptions::new(30, 30).position(0, 0)).unwrap();
    let events = pump(&mut conn, Duration::from_millis(200));
    assert!(!events.iter().any(|e| matches!(e, Event::Focused { window, focused: false } if *window == b)), "{events:?}");
    let px = pattern(30, 30, 2);
    conn.present(b, &Frame::xrgb(&px, 30, 30).unwrap()).expect("the new window is still known");
    assert_shows(&mut conn, b, Rect::from_size(30, 30), &px);
}

#[test]
fn present_rect_clips_areas_that_overhang_the_window_or_frame() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let (w, h) = (40, 30);
    let win = conn.create_window(WindowOptions::new(w, h).position(0, 0)).unwrap();
    let mut expected = pattern(w, h, 1);
    conn.present(win, &Frame::xrgb(&expected, w, h).unwrap()).unwrap();

    let b = vec![0x00ff_00ffu32; (w * h) as usize];
    let small = vec![0x0000_ff00u32; 10 * 10];
    let cases: [(Frame<'_>, Rect); 3] = [
        (Frame::xrgb(&b, w, h).unwrap(), Rect::new(30, -5, 20, 20)),     // past the right and top edges
        (Frame::xrgb(&small, 10, 10).unwrap(), Rect::new(0, 0, 40, 30)), // larger than the frame
        (Frame::xrgb(&b, w, h).unwrap(), Rect::new(-5, 20, 12, 5)),      // negative x
    ];
    for (frame, area) in &cases {
        conn.present_rect(win, frame, *area).unwrap();
        let visible = area.intersect(&frame.bounds()).intersect(&Rect::from_size(w, h));
        for y in visible.y..visible.y + visible.height as i32 {
            for x in visible.x..visible.x + visible.width as i32 {
                expected[(y as u32 * w + x as u32) as usize] = frame.get(x as u32, y as u32).unwrap();
            }
        }
    }
    assert_shows(&mut conn, win, Rect::from_size(w, h), &expected);
    let errors: Vec<_> = pump(&mut conn, Duration::from_millis(50)).into_iter().filter(|e| matches!(e, Event::Error(_))).collect();
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn scrolling_a_sub_area_leaves_everything_outside_it_alone() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let (w, h) = (40, 20);
    let win = conn.create_window(WindowOptions::new(w, h).position(0, 0)).unwrap();
    let px = pattern(w, h, 6);
    conn.present(win, &Frame::xrgb(&px, w, h).unwrap()).unwrap();
    let area = Rect::new(10, 0, 20, h);
    conn.scroll(win, area, -5, 0).unwrap();
    // Columns 10..25 take 15..30; 25..30 keep stale pixels; the rest is untouched.
    let expected: Vec<u32> = (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            if (10..25).contains(&x) {
                px[(y * w + x + 5) as usize]
            } else {
                px[i as usize]
            }
        })
        .collect();
    assert_shows(&mut conn, win, Rect::from_size(w, h), &expected);
    assert_eq!(conn.read_pixels(win, Rect::from_size(w, h)).unwrap(), expected, "the backing store, which repaints exposures, agrees");
}

#[test]
fn connects_through_the_abstract_socket_when_the_file_is_gone() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let path = format!("/tmp/.X11-unix/X{}", x.display);
    std::fs::remove_file(&path).expect("remove our own Xvfb's socket file");
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(4, 4).position(0, 0)).unwrap();
    let px = pattern(4, 4, 1);
    conn.present(win, &Frame::xrgb(&px, 4, 4).unwrap()).unwrap();
    assert_shows(&mut conn, win, Rect::from_size(4, 4), &px);
}

fn xprop(x: &Xvfb, win: WindowId, props: &[&str]) -> Option<String> {
    let out = Command::new("xprop").args(["-display", &name(x), "-id", &format!("{:#x}", win.xid())]).args(props).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn icon_decorations_and_always_on_top_reach_the_server() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(20, 20).position(0, 0)).unwrap();
    pump(&mut conn, Duration::from_millis(50));
    conn.set_icon(win, &Frame::xrgb(&[0x112233, 0x445566], 2, 1).unwrap()).unwrap();
    conn.set_decorations(win, false).unwrap();
    conn.set_always_on_top(win, true).unwrap();
    conn.client().sync().unwrap();
    fn prop32(conn: &mut Connection, win: WindowId, name: &str) -> Vec<u32> {
        let atom = intern(conn.client(), name);
        req::parse_property(&conn.client().round_trip(&req::get_property(false, win.xid(), atom, 0, 0, 64)).unwrap()).unwrap().as_u32s().unwrap()
    }
    let prop = |conn: &mut Connection, name: &str| prop32(conn, win, name);
    assert_eq!(prop(&mut conn, "_NET_WM_ICON"), [2, 1, 0xff11_2233, 0xff44_5566]);
    assert_eq!(prop(&mut conn, "_MOTIF_WM_HINTS"), [2, 0, 0, 0, 0]);
    let above = intern(conn.client(), "_NET_WM_STATE_ABOVE");
    assert_eq!(prop(&mut conn, "_NET_WM_STATE"), [above], "no WM: the state is set directly");
    conn.set_always_on_top(win, false).unwrap();
    conn.set_decorations(win, true).unwrap();
    assert!(prop(&mut conn, "_NET_WM_STATE").is_empty());
    assert_eq!(prop(&mut conn, "_MOTIF_WM_HINTS"), [2, 0, 1, 0, 0]);
    if let Some(text) = xprop(&x, win, &["_NET_WM_ICON"]) {
        assert!(text.contains("Icon (2 x 1)") || text.contains("_NET_WM_ICON(CARDINAL)"), "{text}");
    }
}

#[test]
fn caps_lock_and_server_autorepeat() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(50, 50).position(0, 0)).unwrap();
    wait_for(&mut conn, |e| matches!(e, Event::Focused { focused: true, .. }));
    let xt = Xtest::new(&mut conn);
    let (a, _) = conn.keymap().keycode_for(u32::from('a')).unwrap();
    let (caps, _) = conn.keymap().keycode_for(0xffe5).expect("Caps_Lock on the keymap");

    // Caps Lock on: 'a' types 'A' and the modifier says so.
    for (kind, code) in [(2, caps), (3, caps), (2, a), (3, a), (2, caps), (3, caps)] {
        xt.fake(&mut conn, kind, code, 0, 0);
    }
    let typed: Vec<(Key, bool)> = pump(&mut conn, Duration::from_millis(300))
        .into_iter()
        .filter_map(|e| match e {
            Event::Key { window, key: Key::Character(c), pressed: true, modifiers, .. } if window == win => Some((Key::Character(c), modifiers.caps_lock)),
            _ => None,
        })
        .collect();
    assert_eq!(typed, [(Key::Character('A'), true)]);

    // Hold a key past the server's autorepeat delay: the repeats arrive as
    // presses flagged `repeat`, with no releases between them.
    xt.fake(&mut conn, 2, a, 0, 0);
    let held = pump(&mut conn, Duration::from_millis(1200));
    xt.fake(&mut conn, 3, a, 0, 0);
    let after = pump(&mut conn, Duration::from_millis(200));
    let keys: Vec<(bool, bool)> = held
        .iter()
        .chain(&after)
        .filter_map(|e| match *e {
            Event::Key { key: Key::Character('a'), pressed, repeat, .. } => Some((pressed, repeat)),
            _ => None,
        })
        .collect();
    if keys.len() <= 2 {
        eprintln!("skipping autorepeat half: this server does not autorepeat XTEST keys ({keys:?})");
        return;
    }
    assert_eq!(keys.first(), Some(&(true, false)));
    assert_eq!(keys.last(), Some(&(false, false)));
    assert!(keys[1..keys.len() - 1].iter().all(|k| *k == (true, true)), "{keys:?}");
}

/// A text field driven by real server input: XTEST keys and clicks,
/// decoded by protolith, edit the field; what it draws reaches the window.
#[cfg(feature = "ui")]
#[test]
fn text_field_takes_real_keys_and_clicks() {
    use protolith::canvas::Canvas;
    use protolith::font::Font;
    use protolith::ui::{FieldStyle, Response, TextField};

    /// 5-pixel cells, each a solid block.
    struct Blocks;
    impl Font for Blocks {
        fn line_height(&self) -> u32 {
            8
        }
        fn advance(&self, _: char) -> u32 {
            5
        }
        fn draw_char(&self, canvas: &mut Canvas, _: char, x: i32, y: i32, color: u32) -> u32 {
            canvas.fill_rect(Rect::new(x, y, 5, 8), color);
            5
        }
    }

    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(100, 20).position(0, 0)).unwrap();
    wait_for(&mut conn, |e| matches!(e, Event::Focused { focused: true, .. }));
    let xt = Xtest::new(&mut conn);
    let code = |conn: &mut Connection, keysym: u32| conn.keymap().keycode_for(keysym).unwrap_or_else(|| panic!("keysym {keysym:#x} on the keymap")).0;
    let [a, b, c, ex, left, shift, ctrl, backspace, enter] = [0x61, 0x62, 0x63, 0x78, 0xff51, 0xffe1, 0xffe3, 0xff08, 0xff0d].map(|k| code(&mut conn, k));
    let tap = |conn: &mut Connection, k: u8| {
        xt.fake(conn, 2, k, 0, 0);
        xt.fake(conn, 3, k, 0, 0);
    };

    let mut field = TextField::new(Rect::new(0, 0, 100, 20)).with_padding(2);
    let feed = |conn: &mut Connection, field: &mut TextField| -> Vec<Response> {
        pump(conn, Duration::from_millis(300))
            .iter()
            .filter(|e| e.window() == Some(win))
            .map(|e| field.handle(e, &Blocks))
            .filter(|r| *r != Response::Ignored)
            .collect()
    };

    // Click into the field to focus it, then type, move, select, replace.
    xt.move_to(&mut conn, 50, 10);
    xt.fake(&mut conn, 4, 1, 0, 0);
    xt.fake(&mut conn, 5, 1, 0, 0);
    for k in [a, b, c, left] {
        tap(&mut conn, k);
    }
    xt.fake(&mut conn, 2, shift, 0, 0);
    tap(&mut conn, left);
    xt.fake(&mut conn, 3, shift, 0, 0);
    tap(&mut conn, ex);
    feed(&mut conn, &mut field);
    assert!(field.is_focused());
    assert_eq!(field.edit.text(), "axc");

    // Ctrl+A, Backspace; then click between the two typed letters.
    xt.fake(&mut conn, 2, ctrl, 0, 0);
    tap(&mut conn, a);
    xt.fake(&mut conn, 3, ctrl, 0, 0);
    tap(&mut conn, backspace);
    tap(&mut conn, b);
    tap(&mut conn, c);
    xt.move_to(&mut conn, 7, 10); // text starts at x = 2; 7 is the 'b'|'c' boundary
    xt.fake(&mut conn, 4, 1, 0, 0);
    xt.fake(&mut conn, 5, 1, 0, 0);
    tap(&mut conn, a);
    tap(&mut conn, enter);
    let responses = feed(&mut conn, &mut field);
    assert_eq!(field.edit.text(), "bac");
    assert_eq!(responses.last(), Some(&Response::Submit), "{responses:?}");

    // What the field draws is what the window shows.
    let mut canvas = Canvas::new(100, 20);
    field.draw(&mut canvas, &Blocks, &FieldStyle::default());
    conn.present(win, &canvas.frame()).unwrap();
    assert_shows(&mut conn, win, Rect::new(0, 0, 100, 20), canvas.pixels());
}

/// Widgets on a real server: a slider drag that leaves the window (X's
/// implicit grab keeps it coming), a button click, and a list double
/// click timed by the server's own timestamps.
#[cfg(feature = "ui")]
#[test]
fn widgets_take_real_pointer_input() {
    use protolith::ui::{Button, List, Orientation, Response, Slider};

    let Some(x) = xvfb(24, &[], None) else { return };
    let mut conn = open(&x);
    let win = conn.create_window(WindowOptions::new(100, 40).position(0, 0)).unwrap();
    wait_for(&mut conn, |e| matches!(e, Event::Focused { focused: true, .. }));
    let xt = Xtest::new(&mut conn);
    let mut slider = Slider::new(Rect::new(0, 0, 100, 10), Orientation::Horizontal, 9);
    let mut button = Button::new(Rect::new(0, 15, 30, 10));
    let mut list = List::new(Rect::new(40, 15, 60, 25), 10);
    list.items = vec!["one".into(), "two".into(), "three".into()];

    let gesture = |conn: &mut Connection, path: &[(i32, i32)]| {
        let (x0, y0) = path[0];
        xt.move_to(conn, x0, y0);
        xt.fake(conn, 4, 1, 0, 0);
        for &(x, y) in &path[1..] {
            xt.move_to(conn, x, y);
        }
        xt.fake(conn, 5, 1, 0, 0);
    };
    gesture(&mut conn, &[(5, 5), (60, 6), (400, 300)]); // drag the slider off the window's right
    gesture(&mut conn, &[(10, 20)]); // click the button
    gesture(&mut conn, &[(50, 20)]); // double-click "one"
    gesture(&mut conn, &[(50, 20)]);

    let mut got = Vec::new();
    for e in pump(&mut conn, Duration::from_millis(400)).iter().filter(|e| e.window() == Some(win)) {
        for (name, r) in [("slider", slider.handle(e)), ("button", button.handle(e)), ("list", list.handle(e))] {
            if matches!(r, Response::Clicked | Response::Submit) {
                got.push((name, r));
            }
        }
    }
    assert_eq!(got, [("slider", Response::Submit), ("button", Response::Clicked), ("list", Response::Submit)]);
    assert_eq!(slider.value(), 9, "the drag followed the pointer past the window edge");
    assert_eq!(list.selected(), Some(0));
    assert!(list.is_focused() && !button.is_focused() && !slider.is_focused(), "focus followed the last click");
}

/// The foreign event `pred` picks, waiting for it.
fn wait_foreign(conn: &mut Connection, mut pred: impl FnMut(&RawEvent) -> bool) -> RawEvent {
    match wait_for(conn, |e| matches!(e, Event::Foreign(f) if pred(&f.event))) {
        Event::Foreign(f) => f.event,
        _ => unreachable!(),
    }
}

fn parent_of(x: &Xvfb, window: u32) -> u32 {
    let mut c = raw_client(x);
    let reply = c.round_trip(&req::query_tree(window)).unwrap();
    req::parse_query_tree(&reply).unwrap().parent
}

#[test]
fn a_window_manager_frames_a_window_and_the_save_set_rescues_it() {
    let Some(x) = xvfb(24, &[], None) else { return };
    let mut wm = open(&x);
    wm.become_window_manager().unwrap();
    assert!(wm.has_window_manager());
    match open(&x).become_window_manager() {
        Err(Error::X(e)) => assert_eq!(e.code, 10, "Access: one window manager at a time"),
        other => panic!("a second window manager: {other:?}"),
    }
    let mut app = open(&x);
    let w = app.create_window(WindowOptions::new(100, 50).title("app")).unwrap();
    // The app's MapWindow went to the window manager instead.
    wait_foreign(&mut wm, |e| matches!(*e, RawEvent::MapRequest { window, .. } if window == w.xid()));
    let frame = wm.create_window(WindowOptions::new(104, 70).frame(true).visible(false).decorations(false).position(30, 40)).unwrap();
    for r in [req::change_save_set(true, w.xid()), req::reparent_window(w.xid(), frame.xid(), 2, 18), req::map_window(w.xid())] {
        wm.client().send(&r).unwrap();
    }
    wm.show(frame).unwrap();
    wm.client().sync().unwrap();
    assert_eq!(parent_of(&x, w.xid()), frame.xid());
    assert_eq!(app.window_position(w).unwrap(), (32, 58), "inside the frame");
    // Now the app's own resize goes to the frame's owner, as a request.
    app.set_size(w, 120, 60).unwrap();
    let e = wait_foreign(&mut wm, |e| matches!(*e, RawEvent::ConfigureRequest { window, .. } if window == w.xid()));
    assert!(matches!(e, RawEvent::ConfigureRequest { width: 120, height: 60, parent, .. } if parent == frame.xid()), "{e:?}");
    assert_eq!(app.window_size(w).unwrap(), (100, 50), "not applied: that is the window manager's call");
    // The frame's own events still come as protolith's.
    wm.set_size(frame, 124, 80).unwrap();
    wait_for(&mut wm, |e| matches!(e, Event::Resized { window, width: 124, .. } if *window == frame));
    // The window manager goes away: the window goes back to the root, alive.
    drop(wm);
    let deadline = Instant::now() + Duration::from_secs(5);
    while parent_of(&x, w.xid()) != app.root() {
        assert!(Instant::now() < deadline, "not reparented back to the root");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!pump(&mut app, Duration::from_millis(100)).iter().any(|e| matches!(e, Event::Destroyed { .. })), "the app's window lives");
}
