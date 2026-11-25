//! Bitmap text: Terminus at several sizes, and a sprite-sheet font (whose
//! sheet is itself drawn with Terminus here; a skin would load a BMP).
//! Type to edit the bottom line; Escape quits.
//!
//!     cargo run --example text --features x11,font-terminus-12,font-terminus-16,font-terminus-16-bold,font-terminus-32-bold

use protolith::canvas::{Blit, Canvas};
use protolith::font::{Font, SheetFont, Terminus};
use protolith::x11::{Connection, Event, Key, WindowOptions};
use protolith::Rect;

const W: u32 = 420;
const H: u32 = 200;
const KEY: u32 = 0xff00ff;
const LAYOUT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ\n0123456789 .,:;!?'\"-+*/()<>";

/// A sheet of 6x12 cells, pale text on magenta: the sheet a skin would ship.
fn sheet() -> Canvas {
    let font = Terminus::REGULAR_12;
    let mut s = Canvas::new(27 * 6, 2 * 12);
    s.fill(KEY);
    for (row, line) in LAYOUT.lines().enumerate() {
        font.draw(&mut s, line, 0, row as i32 * 12, 0xc8f0a0);
    }
    s
}

fn main() -> protolith::Result<()> {
    let skin = SheetFont::new(sheet(), 6, 12, LAYOUT).with_blit(Blit::Key(KEY)).with_fallback('?');
    let mut conn = Connection::open()?;
    let win = conn.create_window(WindowOptions::new(W, H).title("protolith: text").resizable(false))?;
    let mut c = Canvas::new(W, H);
    let mut typed = String::from("Type here: héllo, wörld");

    loop {
        c.fill(0x101820);
        Terminus::BOLD_32.draw(&mut c, "protolith", 12, 8, 0xf0c040);
        Terminus::BOLD_16.draw(&mut c, "Terminus 16 bold, any color", 12, 48, 0x80c0ff);
        Terminus::REGULAR_16.draw(&mut c, "ASCII + Latin-1: ÀÉÎÕÜ ñ ß ½ © ¿", 12, 68, 0xe0e0e0);
        Terminus::REGULAR_12.draw(&mut c, "12 px: small print, 6x12 cells. Unknown: 日本", 12, 92, 0xa0a0a0);
        c.fill_rect(Rect::new(8, 112, 260, 18), 0x304030);
        skin.draw(&mut c, &typed.to_uppercase(), 12, 115, 0);

        // The edit line: text, then a block cursor.
        let font = Terminus::REGULAR_16;
        c.fill_rect(Rect::new(8, 150, W - 16, 24), 0x000000);
        let end = font.draw(&mut c, &typed, 12, 154, 0xffffff);
        c.fill_rect(Rect::new(end, 154, font.cell_width(), font.line_height()), 0x40a0ff);
        conn.present(win, &c.frame())?;

        match conn.wait_event()? {
            Event::CloseRequested { .. } | Event::Key { key: Key::Escape, pressed: true, .. } => return Ok(()),
            Event::Key { key: Key::Backspace, pressed: true, .. } => {
                typed.pop();
            }
            Event::Key { key: Key::Character(ch), pressed: true, .. } if font.width(&typed) < W - 40 => typed.push(ch),
            _ => {}
        }
    }
}
