//! Software drawing with `canvas`: a sprite sheet drawn in code, then
//! blitted, color-keyed, blended and scaled into a window.
//!
//! Click the button, move the ball with the mouse, `d` toggles double
//! size, Escape / `q` quits. Pass a BMP file to use it as the background.
//!
//!     cargo run --example canvas --features bmp [-- picture.bmp]

use protolith::canvas::{bmp, Blit, Canvas};
use protolith::x11::{Connection, Event, Key, MouseButton, WindowOptions};
use protolith::Rect;

const W: u32 = 200;
const H: u32 = 120;
/// Sprite-sheet transparency: this color is never drawn.
const KEY: u32 = 0xff00ff;
const BUTTON: Rect = Rect::new(20, 80, 48, 20);

/// Three sprites side by side: button up, button down, a ball on magenta.
fn sheet() -> Canvas {
    let mut s = Canvas::new(48 * 2 + 16, 20);
    for (i, (face, hi, lo)) in [(0x7080a0, 0xd0d8f0, 0x202838), (0x5a6888, 0x202838, 0xd0d8f0)].into_iter().enumerate() {
        let r = Rect::new(i as i32 * 48, 0, 48, 20);
        s.fill_rect(r, face);
        s.fill_rect(Rect::new(r.x, 0, 48, 1), hi);
        s.fill_rect(Rect::new(r.x, 0, 1, 20), hi);
        s.fill_rect(Rect::new(r.x, 19, 48, 1), lo);
        s.fill_rect(Rect::new(r.x + 47, 0, 1, 20), lo);
        s.fill_rect(Rect::new(r.x + 18 + i as i32, 7 + i as i32, 12, 6), 0xf0e080);
    }
    s.fill_rect(Rect::new(96, 0, 16, 20), KEY);
    for y in 0..16 {
        for x in 0..16 {
            let (dx, dy) = (x * 2 - 15, y * 2 - 15);
            if dx * dx + dy * dy < 225 {
                let shade = (255 - (dx + dy + 30) * 3) as u32;
                s.put(96 + x, 2 + y, shade << 16 | (shade / 3) << 8);
            }
        }
    }
    s
}

fn main() -> protolith::Result<()> {
    let background = match std::env::args().nth(1) {
        Some(path) => Some(bmp::decode(&std::fs::read(path)?).map_err(|e| protolith::Error::InvalidArgument(e.to_string()))?),
        None => None,
    };
    let sheet = sheet();
    let mut conn = Connection::open()?;
    let win = conn.create_window(WindowOptions::new(W, H).title("protolith: canvas").resizable(false))?;
    let (mut scale, mut pressed, mut ball) = (1u32, false, (120i32, 40i32));
    let mut scene = Canvas::new(W, H);
    let mut screen = Canvas::new(W, H);

    loop {
        // Background: the BMP scaled to fit, or a gradient.
        match &background {
            Some(b) => scene.blit_with(&b.frame(), b.bounds(), scene.bounds(), Blit::Copy),
            None => {
                for y in 0..H as i32 {
                    scene.fill_rect(Rect::new(0, y, W, 1), (y as u32 * 2) << 8 | 0x30);
                }
            }
        }
        // A translucent panel, the button sprite, then the keyed ball.
        scene.blend_rect(Rect::new(10, 10, 100, 60), 0xffffff, 64);
        let face = if pressed { 48 } else { 0 };
        scene.blit(&sheet.frame(), Rect::new(face, 0, 48, 20), BUTTON.x, BUTTON.y);
        scene.blit_with(&sheet.frame(), Rect::new(96, 2, 16, 16), Rect::new(ball.0 - 8, ball.1 - 8, 16, 16), Blit::Key(KEY));
        // Then the whole scene, nearest-neighbor scaled to the window.
        screen.blit_with(&scene.frame(), scene.bounds(), screen.bounds(), Blit::Copy);
        conn.present(win, &screen.frame())?;

        match conn.wait_event()? {
            Event::CloseRequested { .. } | Event::Key { key: Key::Escape | Key::Character('q'), pressed: true, .. } => return Ok(()),
            Event::Key { key: Key::Character('d'), pressed: true, .. } => {
                scale = 3 - scale;
                screen = Canvas::new(W * scale, H * scale);
                conn.set_size(win, W * scale, H * scale)?;
            }
            Event::Button { button: MouseButton::Left, pressed: down, x, y, .. } => {
                pressed = down && BUTTON.contains(x / scale as i32, y / scale as i32);
            }
            Event::Motion { x, y, .. } => ball = (x / scale as i32, y / scale as i32),
            _ => {}
        }
    }
}
