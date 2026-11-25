//! A skinned player face: every widget drawn from sprite sheets.
//!
//! With no argument it uses a skin it draws itself, in the sheet layout of
//! classic 275x116 player skins. Given a directory holding such a skin's
//! unpacked bitmaps (MAIN.BMP, CBUTTONS.BMP, SHUFREP.BMP, VOLUME.BMP,
//! BALANCE.BMP, POSBAR.BMP, NUMBERS.BMP, TEXT.BMP — any case), it wears
//! that instead. Nothing plays: a clock runs while "playing".
//!
//! Click the buttons, drag the sliders, `d` toggles double size, Escape
//! quits.
//!
//!     cargo run --example skin --features x11,ui,bmp [-- path/to/unpacked-skin]

use std::path::Path;
use std::time::{Duration, Instant};

use protolith::canvas::{bmp, Canvas};
use protolith::font::{Font, SheetFont};
use protolith::ui::{Button, ButtonSprites, Frames, Orientation, Response, Slider, SliderSprites, Toggle, ToggleSprites};
use protolith::x11::{Connection, Event, Key, WindowOptions};
use protolith::Rect;

const W: u32 = 275;
const H: u32 = 116;
/// The text sheet's 5x6 cells, as far as this demo uses them (cell 28 of
/// the first row holds a symbol it never draws, hence the placeholder;
/// space is the blank cell after it).
const TEXT_LAYOUT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ\"@\u{0} \n0123456789";
const TRACK_SECONDS: u32 = 210;

/// The sheets a skin is made of.
struct Skin {
    main: Canvas,
    cbuttons: Canvas,
    shufrep: Canvas,
    volume: Canvas,
    balance: Canvas,
    posbar: Canvas,
    numbers: Canvas,
    text: Canvas,
}

fn rect(x: i32, y: i32, w: u32, h: u32) -> Rect {
    Rect::new(x, y, w, h)
}

/// Where each picture is in the classic sheets, and where each widget goes.
fn button_sprites(i: usize) -> (Rect, ButtonSprites) {
    let (x, w, h, at) = [(0, 23, 18, 16), (23, 23, 18, 39), (46, 23, 18, 62), (69, 23, 18, 85), (92, 22, 18, 108), (114, 22, 16, 136)][i];
    (rect(at, 88 + i32::from(i == 5), w, h), ButtonSprites { up: rect(x, 0, w, h), down: rect(x, h as i32, w, h) })
}

fn toggle_sprites(x: i32, w: u32) -> ToggleSprites {
    ToggleSprites { off: rect(x, 0, w, 15), off_down: rect(x, 15, w, 15), on: rect(x, 30, w, 15), on_down: rect(x, 45, w, 15) }
}

fn slider_sprites(track: Frames, centered: bool) -> SliderSprites {
    SliderSprites { track, centered, thumb: rect(15, 422, 14, 11), thumb_down: rect(0, 422, 14, 11) }
}

// ------------------------------------------------------- the built-in skin

/// A 3x5 font, one octal digit per row (4 = left pixel, 1 = right).
const GLYPHS: [(char, u16); 38] = [
    ('A', 0o75755),
    ('B', 0o65656),
    ('C', 0o74447),
    ('D', 0o65556),
    ('E', 0o74647),
    ('F', 0o74644),
    ('G', 0o74557),
    ('H', 0o55755),
    ('I', 0o72227),
    ('J', 0o11157),
    ('K', 0o55655),
    ('L', 0o44447),
    ('M', 0o57755),
    ('N', 0o75555),
    ('O', 0o75557),
    ('P', 0o75744),
    ('Q', 0o75571),
    ('R', 0o75765),
    ('S', 0o74717),
    ('T', 0o72222),
    ('U', 0o55557),
    ('V', 0o55552),
    ('W', 0o55775),
    ('X', 0o55255),
    ('Y', 0o55222),
    ('Z', 0o71247),
    ('"', 0o50000),
    ('@', 0o75547),
    ('0', 0o75557),
    ('1', 0o26227),
    ('2', 0o71747),
    ('3', 0o71717),
    ('4', 0o55711),
    ('5', 0o74717),
    ('6', 0o74757),
    ('7', 0o71111),
    ('8', 0o75757),
    ('9', 0o75717),
];

fn glyph(c: &mut Canvas, ch: char, x: i32, y: i32, color: u32) {
    if let Some(&(_, bits)) = GLYPHS.iter().find(|(g, _)| *g == ch) {
        for row in 0..5 {
            for col in 0..3 {
                if bits >> ((4 - row) * 3 + (2 - col)) & 1 == 1 {
                    c.put(x + col, y + row, color);
                }
            }
        }
    }
}

fn bevel(c: &mut Canvas, r: Rect, light: u32, dark: u32, face: u32) {
    c.fill_rect(r, face);
    c.fill_rect(rect(r.x, r.y, r.width, 1), light);
    c.fill_rect(rect(r.x, r.y, 1, r.height), light);
    c.fill_rect(rect(r.x, r.y + r.height as i32 - 1, r.width, 1), dark);
    c.fill_rect(rect(r.x + r.width as i32 - 1, r.y, 1, r.height), dark);
}

/// Channel-wise mix of two colors, `t` of 255 toward `b`.
fn mix(a: u32, b: u32, t: u32) -> u32 {
    let ch = |s: u32| ((((a >> s) & 255) * (255 - t) + ((b >> s) & 255) * t) / 255) << s;
    ch(16) | ch(8) | ch(0)
}

fn built_in() -> Skin {
    let (ink, glow, steel, deep) = (0x9fe8ff, 0xffb347, 0x4a5a78, 0x141a28);

    let mut main = Canvas::new(W, H);
    for y in 0..H as i32 {
        main.fill_rect(rect(0, y, W, 1), mix(0x2c3650, deep, y as u32 * 255 / H));
    }
    main.fill_rect(rect(0, 0, W, 14), 0x0e1320);
    for x in (6..W as i32 - 6).step_by(3) {
        main.fill_rect(rect(x, 5, 1, 4), 0x2a3550);
    }
    bevel(&mut main, rect(10, 18, 92, 34), 0x05080f, 0x50607e, 0x070b14); // clock window
    bevel(&mut main, rect(106, 22, 160, 12), 0x05080f, 0x50607e, 0x070b14); // title window
    for (i, ch) in "SKIN DEMO".chars().enumerate() {
        glyph(&mut main, ch, 108 + i as i32 * 4, 41, 0x6070a0);
    }

    let mut cbuttons = Canvas::new(136, 36);
    for i in 0..6 {
        let (_, s) = button_sprites(i);
        for (r, down) in [(s.up, false), (s.down, true)] {
            let (light, dark) = if down { (0x222b40, 0x8898c0) } else { (0x8898c0, 0x222b40) };
            bevel(&mut cbuttons, r, light, dark, if down { 0x39465f } else { steel });
            // Icons: little shapes, offset a pixel when pressed.
            let (cx, cy) = (r.x + r.width as i32 / 2 + i32::from(down), r.y + r.height as i32 / 2 + i32::from(down));
            // A triangle: its tall edge at x0, narrowing toward x0 + 4 * dir.
            let tri = |c: &mut Canvas, x0: i32, dir: i32| {
                for k in 0..5 {
                    c.fill_rect(rect(x0 + k * dir, cy - 4 + k, 1, (9 - 2 * k) as u32), ink);
                }
            };
            match i {
                0 => {
                    cbuttons.fill_rect(rect(cx - 5, cy - 4, 2, 9), ink);
                    tri(&mut cbuttons, cx + 2, -1);
                }
                1 => tri(&mut cbuttons, cx - 2, 1),
                2 => {
                    cbuttons.fill_rect(rect(cx - 3, cy - 4, 2, 9), ink);
                    cbuttons.fill_rect(rect(cx + 1, cy - 4, 2, 9), ink);
                }
                3 => cbuttons.fill_rect(rect(cx - 4, cy - 4, 8, 8), ink),
                4 => {
                    tri(&mut cbuttons, cx - 3, 1);
                    cbuttons.fill_rect(rect(cx + 3, cy - 4, 2, 9), ink);
                }
                _ => {
                    for k in 0..4 {
                        cbuttons.fill_rect(rect(cx - k, cy - 4 + k, 2 * k as u32 + 1, 1), ink);
                    }
                    cbuttons.fill_rect(rect(cx - 4, cy + 2, 9, 2), ink);
                }
            }
        }
    }

    let mut shufrep = Canvas::new(92, 85);
    for (x, w, label) in [(0, 28u32, "REP"), (28, 47, "SHUF")] {
        let s = toggle_sprites(x, w);
        for (r, on, down) in [(s.off, false, false), (s.off_down, false, true), (s.on, true, false), (s.on_down, true, true)] {
            let (light, dark) = if down || on { (0x222b40, 0x8898c0) } else { (0x8898c0, 0x222b40) };
            bevel(&mut shufrep, r, light, dark, steel);
            shufrep.fill_rect(rect(r.x + 3, r.y + 5, 3, 5), if on { glow } else { 0x202838 });
            for (k, ch) in label.chars().enumerate() {
                glyph(&mut shufrep, ch, r.x + 9 + k as i32 * 4 + i32::from(down), r.y + 5 + i32::from(down), ink);
            }
        }
    }

    // Slider tracks: 28 frames, 15 px apart, filling up (volume) or out from
    // the middle (balance, which uses 38 px from x = 9).
    let mut volume = Canvas::new(68, 433);
    let mut balance = Canvas::new(68, 433);
    for i in 0..28u32 {
        let y = i as i32 * 15;
        let color = mix(0x2f6fff, glow, i * 255 / 27);
        bevel(&mut volume, rect(0, y + 4, 68, 5), 0x05080f, 0x50607e, 0x070b14);
        volume.fill_rect(rect(1, y + 5, 1 + 66 * i / 27, 3), color);
        bevel(&mut balance, rect(9, y + 4, 38, 5), 0x05080f, 0x50607e, 0x070b14);
        let half = 18 * i / 27;
        balance.fill_rect(rect(28 - half as i32 - 1, y + 5, 2 * half + 2, 3), color);
    }
    for sheet in [&mut volume, &mut balance] {
        bevel(sheet, rect(15, 422, 14, 11), 0xd0d8f0, 0x222b40, 0x8898c0);
        bevel(sheet, rect(0, 422, 14, 11), 0x222b40, 0xd0d8f0, 0x6878a0);
    }

    let mut posbar = Canvas::new(307, 10);
    bevel(&mut posbar, rect(0, 2, 248, 6), 0x05080f, 0x50607e, 0x070b14);
    bevel(&mut posbar, rect(248, 0, 29, 10), 0xffe0b0, 0x804000, glow);
    bevel(&mut posbar, rect(278, 0, 29, 10), 0x804000, 0xffe0b0, 0xc07820);

    // Seven-segment digits, 9x13, unlit segments faintly visible.
    let mut numbers = Canvas::new(99, 13);
    numbers.fill(0x070b14);
    const SEGS: [u8; 10] = [0x3f, 0x06, 0x5b, 0x4f, 0x66, 0x6d, 0x7d, 0x07, 0x7f, 0x6f];
    for (d, segs) in SEGS.iter().enumerate() {
        let x = d as i32 * 9;
        let parts = [
            rect(x + 2, 1, 5, 1),
            rect(x + 7, 2, 1, 4),
            rect(x + 7, 7, 1, 4),
            rect(x + 2, 11, 5, 1),
            rect(x + 1, 7, 1, 4),
            rect(x + 1, 2, 1, 4),
            rect(x + 2, 6, 5, 1),
        ];
        for (k, r) in parts.into_iter().enumerate() {
            numbers.fill_rect(r, if segs >> k & 1 == 1 { glow } else { 0x1c2230 });
        }
    }

    let mut text = Canvas::new(155, 18);
    text.fill(0x070b14);
    for (row, line) in TEXT_LAYOUT.lines().enumerate() {
        for (col, ch) in line.chars().enumerate() {
            glyph(&mut text, ch, col as i32 * 5 + 1, row as i32 * 6, ink);
        }
    }
    Skin { main, cbuttons, shufrep, volume, balance, posbar, numbers, text }
}

/// Load an unpacked classic skin from `dir` (file names in any case).
fn load(dir: &Path) -> Result<Skin, String> {
    let names: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    let get = |name: &str| -> Result<Canvas, String> {
        let path = names
            .iter()
            .find(|p| p.file_name().is_some_and(|f| f.to_string_lossy().eq_ignore_ascii_case(name)))
            .ok_or(format!("no {name} in {}", dir.display()))?;
        bmp::decode(&std::fs::read(path).map_err(|e| e.to_string())?).map_err(|e| format!("{name}: {e}"))
    };
    Ok(Skin {
        main: get("main.bmp")?,
        cbuttons: get("cbuttons.bmp")?,
        shufrep: get("shufrep.bmp")?,
        volume: get("volume.bmp")?,
        balance: get("balance.bmp")?,
        posbar: get("posbar.bmp")?,
        numbers: get("numbers.bmp")?,
        text: get("text.bmp")?,
    })
}

/// An event's pointer position divided by the display scale.
fn unscale(e: Event, s: i32) -> Event {
    match e {
        Event::Button { window, button, pressed, x, y, root_x, root_y, modifiers, time } => {
            Event::Button { window, button, pressed, x: x / s, y: y / s, root_x, root_y, modifiers, time }
        }
        Event::Motion { window, x, y, root_x, root_y, buttons, modifiers } => Event::Motion { window, x: x / s, y: y / s, root_x, root_y, buttons, modifiers },
        Event::Scroll { window, dx, dy, x, y, modifiers } => Event::Scroll { window, dx, dy, x: x / s, y: y / s, modifiers },
        other => other,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let skin = match std::env::args_os().nth(1) {
        Some(dir) => load(Path::new(&dir))?,
        None => built_in(),
    };
    let digits = SheetFont::new(skin.numbers.clone(), 9, 13, "0123456789 ");
    let text = SheetFont::new(skin.text.clone(), 5, 6, TEXT_LAYOUT).with_fallback(' ');

    let mut buttons: Vec<(Button, ButtonSprites)> = (0..6).map(button_sprites).map(|(r, s)| (Button::new(r), s)).collect();
    let mut repeat = (Toggle::new(rect(210, 89, 28, 15), false), toggle_sprites(0, 28));
    let mut shuffle = (Toggle::new(rect(164, 89, 47, 15), false), toggle_sprites(28, 47));
    let mut volume = Slider::new(rect(107, 57, 68, 13), Orientation::Horizontal, 100).with_thumb(14);
    let mut balance = Slider::new(rect(177, 57, 38, 13), Orientation::Horizontal, 100).with_thumb(14);
    let mut seek = Slider::new(rect(16, 72, 248, 10), Orientation::Horizontal, TRACK_SECONDS).with_thumb(29).with_step(5);
    volume.set_value(80);
    balance.set_value(50);
    let volume_sprites = slider_sprites(Frames::column(rect(0, 0, 68, 13), 28, 15), false);
    let balance_sprites = slider_sprites(Frames::column(rect(9, 0, 38, 13), 28, 15), true);
    let seek_sprites =
        SliderSprites { track: Frames::single(rect(0, 0, 248, 10)), centered: false, thumb: rect(248, 0, 29, 10), thumb_down: rect(278, 0, 29, 10) };

    let (mut playing, mut seconds, mut track, mut status) = (false, 0u32, 1u32, String::from("PROTOLITH SKIN DEMO"));
    let mut scale = 1u32;
    let mut tick = Instant::now();
    let mut face = Canvas::new(W, H);
    let mut screen = Canvas::new(W, H);
    let mut conn = Connection::open()?;
    let win = conn.create_window(WindowOptions::new(W, H).title("protolith: skin").resizable(false))?;

    loop {
        face.blit(&skin.main.frame(), skin.main.bounds(), 0, 0);
        for (b, s) in &buttons {
            b.draw_sprite(&mut face, &skin.cbuttons.frame(), s);
        }
        for (t, s) in [&repeat, &shuffle] {
            t.draw_sprite(&mut face, &skin.shufrep.frame(), s);
        }
        volume.draw_sprite(&mut face, &skin.volume.frame(), &volume_sprites);
        balance.draw_sprite(&mut face, &skin.balance.frame(), &balance_sprites);
        seek.draw_sprite(&mut face, &skin.posbar.frame(), &seek_sprites);
        for (x, d) in [48, 60, 78, 90].into_iter().zip([seconds / 600, seconds / 60 % 10, seconds % 60 / 10, seconds % 10]) {
            digits.draw_char(&mut face, char::from(b'0' + d as u8), x, 26, 0);
        }
        face.set_clip(rect(111, 27, 154, 6));
        text.draw(&mut face, &status, 111, 27, 0);
        face.reset_clip();
        screen.blit_with(&face.frame(), face.bounds(), screen.bounds(), protolith::canvas::Blit::Copy);
        conn.present(win, &screen.frame())?;

        // A clock second passes on time whether or not events keep coming.
        let wait = if playing { tick.saturating_duration_since(Instant::now()) } else { Duration::from_secs(60) };
        let event = conn.wait_event_timeout(wait)?;
        if !playing {
            tick = Instant::now() + Duration::from_secs(1);
        } else if Instant::now() >= tick {
            tick += Duration::from_secs(1);
            seconds = (seconds + 1).min(TRACK_SECONDS);
            if !seek.is_dragging() {
                seek.set_value(seconds);
            }
        }
        let Some(event) = event else { continue };
        match event {
            Event::CloseRequested { .. } | Event::Key { key: Key::Escape, pressed: true, .. } => return Ok(()),
            Event::Key { key: Key::Character('d'), pressed: true, .. } => {
                scale = 3 - scale;
                screen = Canvas::new(W * scale, H * scale);
                conn.set_size(win, W * scale, H * scale)?;
                continue;
            }
            _ => {}
        }
        let event = unscale(event, scale as i32);
        for (i, (b, _)) in buttons.iter_mut().enumerate() {
            if b.handle(&event) == Response::Clicked {
                match i {
                    0 | 4 => {
                        track = if i == 0 { track.saturating_sub(1).max(1) } else { track + 1 };
                        seconds = 0;
                        seek.set_value(0);
                    }
                    1 => playing = true,
                    2 => playing = !playing,
                    3 => {
                        playing = false;
                        seconds = 0;
                        seek.set_value(0);
                    }
                    _ => {}
                }
                status = match i {
                    5 => "EJECT".into(),
                    _ => format!(
                        "TRACK {track:02} {}",
                        if playing {
                            "PLAYING"
                        } else if seconds > 0 {
                            "PAUSED"
                        } else {
                            "STOPPED"
                        }
                    ),
                };
            }
        }
        for (name, t) in [("REPEAT", &mut repeat.0), ("SHUFFLE", &mut shuffle.0)] {
            if t.handle(&event) == Response::Changed {
                status = format!("{name} {}", if t.is_on() { "ON" } else { "OFF" });
            }
        }
        if volume.handle(&event) == Response::Changed {
            status = format!("VOLUME {}", volume.value());
        }
        if balance.handle(&event) == Response::Changed {
            let b = balance.value() as i32 - 50;
            status = match b {
                0 => "BALANCE CENTER".into(),
                _ => format!("BALANCE {} {}", 2 * b.abs(), if b < 0 { "LEFT" } else { "RIGHT" }),
            };
        }
        match seek.handle(&event) {
            Response::Changed => {
                status = format!("SEEK TO {}", seek.value());
                if !seek.is_dragging() {
                    seconds = seek.value(); // the wheel or a key: no drag to finish
                }
            }
            Response::Submit => seconds = seek.value(),
            _ => {}
        }
    }
}
