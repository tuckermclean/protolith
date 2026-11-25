//! A borderless, non-rectangular, draggable little player face — the shape
//! of the thing protolith was built for. Everything is drawn in code.
//!
//! Drag the title strip to move it, `t` toggles always-on-top, space
//! pauses the analyzer, Escape / `q` / the × button quits.
//!
//!     cargo run --example shaped

use std::time::{Duration, Instant};

use protolith::x11::{shape, Connection, Event, Key, MouseButton, WindowOptions};
use protolith::{Frame, Rect};

const W: u32 = 275;
const H: u32 = 116;
const TITLE: Rect = Rect::new(0, 0, W, 14);
const CLOSE: Rect = Rect::new(W as i32 - 13, 3, 9, 9);
const LCD: Rect = Rect::new(10, 22, 95, 42);
const BARS: Rect = Rect::new(115, 22, 150, 42);

struct Canvas(Vec<u32>);

impl Canvas {
    fn fill(&mut self, r: Rect, c: u32) {
        let r = r.intersect(&Rect::from_size(W, H));
        for y in r.y..r.y + r.height as i32 {
            let row = (y as u32 * W) as usize;
            self.0[row + r.x as usize..row + (r.x + r.width as i32) as usize].fill(c);
        }
    }

    /// A raised (or sunken) 3D bevel, 1996-style.
    fn bevel(&mut self, r: Rect, face: u32, raised: bool) {
        let (hi, lo) = if raised { (0xe8e8f0, 0x303040) } else { (0x303040, 0xe8e8f0) };
        self.fill(r, face);
        self.fill(Rect::new(r.x, r.y, r.width, 1), hi);
        self.fill(Rect::new(r.x, r.y, 1, r.height), hi);
        self.fill(Rect::new(r.x, r.y + r.height as i32 - 1, r.width, 1), lo);
        self.fill(Rect::new(r.x + r.width as i32 - 1, r.y, 1, r.height), lo);
    }

    /// A seven-segment digit, 9x17, at `(x, y)`.
    fn digit(&mut self, x: i32, y: i32, d: u32, on: u32, off: u32) {
        //            a      b      c      d      e      f      g
        const SEGS: [[bool; 7]; 10] = [
            [true, true, true, true, true, true, false],
            [false, true, true, false, false, false, false],
            [true, true, false, true, true, false, true],
            [true, true, true, true, false, false, true],
            [false, true, true, false, false, true, true],
            [true, false, true, true, false, true, true],
            [true, false, true, true, true, true, true],
            [true, true, true, false, false, false, false],
            [true, true, true, true, true, true, true],
            [true, true, true, true, false, true, true],
        ];
        let rects = [
            Rect::new(x + 2, y, 5, 2),
            Rect::new(x + 7, y + 2, 2, 6),
            Rect::new(x + 7, y + 9, 2, 6),
            Rect::new(x + 2, y + 15, 5, 2),
            Rect::new(x, y + 9, 2, 6),
            Rect::new(x, y + 2, 2, 6),
            Rect::new(x + 2, y + 7, 5, 2),
        ];
        for (seg, r) in SEGS[d as usize % 10].iter().zip(rects) {
            self.fill(r, if *seg { on } else { off });
        }
    }
}

fn draw_static(c: &mut Canvas, on_top: bool) {
    c.bevel(Rect::from_size(W, H), 0x8a8aa0, true);
    // Title strip: alternating "grip" lines, brighter when on top.
    c.fill(TITLE, 0x202038);
    for y in (3..12).step_by(2) {
        c.fill(Rect::new(6, y, W - 24, 1), if on_top { 0xc8a040 } else { 0x6a6a90 });
    }
    c.bevel(CLOSE, 0x9090a8, true);
    for i in 2..7 {
        c.fill(Rect::new(CLOSE.x + i, CLOSE.y + i, 1, 1), 0x101020);
        c.fill(Rect::new(CLOSE.x + 8 - i, CLOSE.y + i, 1, 1), 0x101020);
    }
    c.bevel(Rect::new(LCD.x - 2, LCD.y - 2, LCD.width + 4, LCD.height + 4), 0, false);
    c.bevel(Rect::new(BARS.x - 2, BARS.y - 2, BARS.width + 4, BARS.height + 4), 0, false);
    // A row of chunky transport buttons, purely decorative.
    for i in 0..5 {
        c.bevel(Rect::new(10 + i * 26, 82, 23, 18), 0xa0a0b8, true);
    }
    c.bevel(Rect::new(150, 86, 115, 8), 0x505068, false);
}

fn draw_lcd(c: &mut Canvas, secs: u64) {
    c.fill(LCD, 0x000000);
    let (m, s) = ((secs / 60) % 100, secs % 60);
    let (on, off) = (0x40ff60, 0x0a2a10);
    for (i, d) in [m / 10, m % 10, s / 10, s % 10].into_iter().enumerate() {
        let x = LCD.x + 30 + i as i32 * 13 + if i >= 2 { 6 } else { 0 };
        c.digit(x, LCD.y + 12, d as u32, on, off);
    }
    c.fill(Rect::new(LCD.x + 57, LCD.y + 17, 2, 2), on);
    c.fill(Rect::new(LCD.x + 57, LCD.y + 23, 2, 2), on);
}

fn draw_bars(c: &mut Canvas, t: f32) {
    c.fill(BARS, 0x000000);
    let n = 19;
    for i in 0..n {
        let f = i as f32;
        let level = ((t * 3.1 + f * 0.7).sin() * 0.5 + (t * 1.7 - f * 0.31).sin() * 0.3 + 0.9) / 1.8;
        let h = (level.clamp(0.0, 1.0) * BARS.height as f32) as u32;
        for y in 0..h {
            let frac = y as f32 / BARS.height as f32;
            let color = if frac > 0.8 {
                0xff3020
            } else if frac > 0.55 {
                0xf0d020
            } else {
                0x30e040
            };
            c.fill(Rect::new(BARS.x + i * 8 + 1, BARS.y + (BARS.height - 1 - y) as i32, 6, 1), color);
        }
    }
}

fn main() -> protolith::Result<()> {
    let mut conn = Connection::open()?;
    let win = conn
        .create_window(WindowOptions::new(W, H).title("protolith: shaped").class("shaped", "Protolith").decorations(false).resizable(false).visible(false))?;
    if conn.supports_shape() {
        // Clip the four corners round, 3 pixels in.
        let cut = |x: u32, y: u32| {
            let dx = x.min(W - 1 - x);
            let dy = y.min(H - 1 - y);
            dx + dy < 3 && dx < 3 && dy < 3
        };
        conn.set_shape(win, &shape::rects_from_mask(W, H, |x, y| !cut(x, y)))?;
    }
    let mut on_top = false;
    let mut canvas = Canvas(vec![0; (W * H) as usize]);
    draw_static(&mut canvas, on_top);
    draw_lcd(&mut canvas, 0);
    draw_bars(&mut canvas, 0.0);
    conn.present(win, &Frame::xrgb(&canvas.0, W, H)?)?;
    conn.show(win)?;

    let start = Instant::now();
    let mut paused = false;
    let mut t = 0.0f32;
    let frame = Duration::from_millis(33);
    let mut next = Instant::now() + frame;
    loop {
        // Handle input until the next animation tick is due.
        while let Some(ev) = conn.wait_event_timeout(next.saturating_duration_since(Instant::now()))? {
            match ev {
                Event::CloseRequested { .. } => return Ok(()),
                Event::Key { key: Key::Escape | Key::Character('q'), pressed: true, .. } => return Ok(()),
                Event::Key { key: Key::Character(' '), pressed: true, repeat: false, .. } => paused = !paused,
                Event::Key { key: Key::Character('t'), pressed: true, repeat: false, .. } => {
                    on_top = !on_top;
                    conn.set_always_on_top(win, on_top)?;
                    draw_static(&mut canvas, on_top);
                    draw_lcd(&mut canvas, start.elapsed().as_secs());
                    draw_bars(&mut canvas, t);
                    conn.present(win, &Frame::xrgb(&canvas.0, W, H)?)?;
                }
                Event::Key { key, pressed: true, .. } => eprintln!("key: {key:?}"),
                Event::Button { button: MouseButton::Left, pressed: true, x, y, root_x, root_y, .. } => {
                    if CLOSE.contains(x, y) {
                        return Ok(());
                    }
                    if TITLE.contains(x, y) {
                        conn.begin_drag_move(win, root_x, root_y, MouseButton::Left)?;
                    }
                }
                _ => {}
            }
        }
        // Fall behind (a stalled server, a suspended laptop)? Skip ahead
        // rather than replaying every missed frame.
        next = (next + frame).max(Instant::now());
        if !paused {
            t += 0.033;
            draw_lcd(&mut canvas, start.elapsed().as_secs());
            draw_bars(&mut canvas, t);
            let f = Frame::xrgb(&canvas.0, W, H)?;
            // Only the two animated panels cross the wire each frame.
            conn.present_rect(win, &f, LCD)?;
            conn.present_rect(win, &f, BARS)?;
        }
    }
}
