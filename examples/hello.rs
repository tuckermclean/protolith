//! The smallest useful protolith program: a window you can paint in.
//!
//! Drag with the left button to paint, right-click to clear, wheel to
//! change the brush, Escape or the close button to quit. Resize the window
//! and the picture follows: the gradient re-renders at the new size and
//! your strokes stay where you put them.
//!
//!     cargo run --example hello

use protolith::x11::{Connection, Event, Key, MouseButton, MouseButtons, WindowOptions};
use protolith::{Frame, Rect};

/// The picture: a gradient sized to the window, plus the strokes painted
/// on it (kept as rectangles so they survive a resize).
struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u32>,
    strokes: Vec<Rect>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Canvas {
        let mut c = Canvas { width, height, pixels: Vec::new(), strokes: Vec::new() };
        c.resize(width, height);
        c
    }

    /// Re-render at a new size: fresh gradient, then every stroke again.
    fn resize(&mut self, width: u32, height: u32) {
        (self.width, self.height) = (width, height);
        self.pixels = (0..width * height).map(|i| ((i % width) * 255 / width) << 16 | ((i / width) * 255 / height) << 8 | 0x60).collect();
        for r in std::mem::take(&mut self.strokes) {
            self.dab(r);
        }
    }

    /// Paint one white square; returns the pixels it changed.
    fn dab(&mut self, r: Rect) -> Rect {
        self.strokes.push(r);
        let r = r.intersect(&Rect::from_size(self.width, self.height));
        for y in r.y..r.y + r.height as i32 {
            let row = (y as u32 * self.width) as usize;
            self.pixels[row + r.x as usize..row + (r.x + r.width as i32) as usize].fill(0xffffff);
        }
        r
    }

    fn frame(&self) -> protolith::Result<Frame<'_>> {
        Ok(Frame::xrgb(&self.pixels, self.width, self.height)?)
    }
}

fn main() -> protolith::Result<()> {
    let mut conn = Connection::open()?;
    let win = conn.create_window(WindowOptions::new(480, 320).title("protolith: hello"))?;
    let mut canvas = Canvas::new(480, 320);
    let mut brush = 6i32;
    conn.present(win, &canvas.frame()?)?;

    loop {
        match conn.wait_event()? {
            Event::CloseRequested { .. } | Event::Key { key: Key::Escape, pressed: true, .. } => return Ok(()),
            Event::Resized { width, height, .. } => {
                canvas.resize(width, height);
                conn.present(win, &canvas.frame()?)?;
            }
            Event::Scroll { dy, .. } => brush = (brush - dy).clamp(1, 40),
            Event::Button { button: MouseButton::Right, pressed: true, .. } => {
                canvas.strokes.clear();
                canvas.resize(canvas.width, canvas.height);
                conn.present(win, &canvas.frame()?)?;
            }
            Event::Button { button: MouseButton::Left, pressed: true, x, y, .. } | Event::Motion { x, y, buttons: MouseButtons { left: true, .. }, .. } => {
                let changed = canvas.dab(Rect::new(x - brush / 2, y - brush / 2, brush as u32, brush as u32));
                // Send only the pixels that changed.
                conn.present_rect(win, &canvas.frame()?, changed)?;
            }
            _ => {}
        }
    }
}
