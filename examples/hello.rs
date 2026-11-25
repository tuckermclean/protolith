//! The smallest useful protolith program: a window you can paint in.
//!
//! Drag with the left button to paint, right-click to clear, wheel to
//! change the brush, Escape or the close button to quit.
//!
//!     cargo run --example hello

use protolith::x11::{Connection, Event, Key, MouseButton, WindowOptions};
use protolith::{Frame, Rect};

const W: u32 = 480;
const H: u32 = 320;

fn background() -> Vec<u32> {
    (0..W * H).map(|i| ((i % W) * 255 / W) << 16 | ((i / W) * 255 / H) << 8 | 0x60).collect()
}

fn main() -> protolith::Result<()> {
    let mut conn = Connection::open()?;
    let win = conn.create_window(WindowOptions::new(W, H).title("protolith: hello").resizable(false))?;
    let mut canvas = background();
    let mut brush = 6i32;
    conn.present(win, &Frame::xrgb(&canvas, W, H)?)?;

    loop {
        match conn.wait_event()? {
            Event::CloseRequested { .. } | Event::Key { key: Key::Escape, pressed: true, .. } => return Ok(()),
            Event::Scroll { dy, .. } => brush = (brush - dy).clamp(1, 40),
            Event::Button { button: MouseButton::Right, pressed: true, .. } => {
                canvas = background();
                conn.present(win, &Frame::xrgb(&canvas, W, H)?)?;
            }
            Event::Button { button: MouseButton::Left, pressed: true, x, y, .. }
            | Event::Motion { x, y, buttons: protolith::x11::MouseButtons { left: true, .. }, .. } => {
                let dab = Rect::new(x - brush / 2, y - brush / 2, brush as u32, brush as u32).intersect(&Rect::from_size(W, H));
                for py in dab.y..dab.y + dab.height as i32 {
                    for px in dab.x..dab.x + dab.width as i32 {
                        canvas[(py as u32 * W + px as u32) as usize] = 0xffffff;
                    }
                }
                // Send only the pixels that changed.
                conn.present_rect(win, &Frame::xrgb(&canvas, W, H)?, dab)?;
            }
            _ => {}
        }
    }
}
