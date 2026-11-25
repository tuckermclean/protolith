//! Every widget in its default look: buttons, a toggle, sliders (one
//! paired with the list as its scroll bar), a list and a text field. Tab
//! moves focus; the status line says what happened. Escape quits.
//!
//!     cargo run --example widgets --features x11,ui,font-terminus-12

use protolith::canvas::Canvas;
use protolith::font::{Font, Terminus};
use protolith::ui::{Button, FieldStyle, List, Orientation, Response, Slider, Style, TextField, Toggle};
use protolith::x11::{Connection, Event, Key, WindowOptions};
use protolith::Rect;

const W: u32 = 320;
const H: u32 = 220;

fn main() -> protolith::Result<()> {
    let font = Terminus::REGULAR_12;
    let style = Style::default();
    let mut play = Button::new(Rect::new(10, 10, 60, 22));
    let mut stop = Button::new(Rect::new(76, 10, 60, 22));
    let mut shuffle = Toggle::new(Rect::new(142, 10, 80, 22), false);
    let mut volume = Slider::new(Rect::new(10, 44, 212, 16), Orientation::Horizontal, 100).with_thumb(10);
    volume.set_value(70);
    let mut balance = Slider::new(Rect::new(236, 10, 16, 92), Orientation::Vertical, 20).with_step(1);
    balance.set_value(10);
    let mut list = List::new(Rect::new(10, 72, 196, 104), 13);
    list.items = (1..=40).map(|i| format!("{i:02}. Track number {i}")).collect();
    let mut scroll = Slider::new(Rect::new(206, 72, 16, 104), Orientation::Vertical, list.max_top() as u32).with_step(1);
    scroll.set_value(scroll.max());
    let mut field = TextField::new(Rect::new(10, 186, 212, 22));
    let mut status = String::from("Tab moves focus");
    let mut canvas = Canvas::new(W, H);

    let mut conn = Connection::open()?;
    let win = conn.create_window(WindowOptions::new(W, H).title("protolith: widgets").resizable(false))?;
    loop {
        canvas.fill(style.face);
        play.draw(&mut canvas, &font, "Play", &style);
        stop.draw(&mut canvas, &font, "Stop", &style);
        shuffle.draw(&mut canvas, &font, "Shuffle", &style);
        volume.draw(&mut canvas, &style);
        balance.draw(&mut canvas, &style);
        list.draw(&mut canvas, &font, &style);
        scroll.draw(&mut canvas, &style);
        field.draw(&mut canvas, &font, &FieldStyle::default());
        font.draw(&mut canvas, &format!("vol {:3}", volume.value()), 236, 110, style.text);
        font.draw(&mut canvas, &format!("bal {:+}", balance.value() as i32 - 10), 236, 124, style.text);
        canvas.fill_rect(Rect::new(236, 140, 80, 70), style.background);
        for (i, chunk) in status.as_bytes().chunks(13).take(5).enumerate() {
            font.draw(&mut canvas, &String::from_utf8_lossy(chunk), 238, 142 + 13 * i as i32, style.text);
        }
        conn.present(win, &canvas.frame())?;

        let event = conn.wait_event()?;
        if let Event::CloseRequested { .. } | Event::Key { key: Key::Escape, pressed: true, .. } = event {
            return Ok(());
        }
        // Tab: focus the next widget (shift: previous).
        if let Event::Key { key: Key::Tab, pressed: true, modifiers, .. } = event {
            let mut focus = [play.is_focused(), stop.is_focused(), shuffle.is_focused(), volume.is_focused(), list.is_focused(), field.is_focused()];
            let at = focus.iter().position(|f| *f).unwrap_or(focus.len() - 1);
            focus = [false; 6];
            focus[(at + if modifiers.shift { 5 } else { 1 }) % 6] = true;
            play.set_focused(focus[0]);
            stop.set_focused(focus[1]);
            shuffle.set_focused(focus[2]);
            volume.set_focused(focus[3]);
            list.set_focused(focus[4]);
            field.set_focused(focus[5]);
            // Not in the Tab order: mouse only.
            balance.set_focused(false);
            scroll.set_focused(false);
            continue;
        }
        if play.handle(&event) == Response::Clicked {
            status = match list.selected() {
                Some(i) => format!("Playing {}", list.items[i]),
                None => "Pick a track first".into(),
            };
        }
        if stop.handle(&event) == Response::Clicked {
            status = "Stopped".into();
        }
        if shuffle.handle(&event) == Response::Changed {
            status = format!("Shuffle {}", if shuffle.is_on() { "on" } else { "off" });
        }
        volume.handle(&event);
        balance.handle(&event);
        match list.handle(&event) {
            Response::Submit => status = format!("Playing {}", list.items[list.selected().unwrap_or(0)]),
            Response::Changed | Response::Redraw => scroll.set_value((list.max_top() - list.top()) as u32),
            _ => {}
        }
        // The scroll bar runs bottom-up, so its maximum is the list's top.
        if let Response::Changed | Response::Submit = scroll.handle(&event) {
            list.set_top(list.max_top() - scroll.value() as usize);
        }
        if field.handle(&event, &font) == Response::Submit {
            let query = field.edit.text().to_lowercase();
            match list.items.iter().position(|t| t.to_lowercase().contains(&query)) {
                Some(i) => {
                    list.select(Some(i));
                    scroll.set_value((list.max_top() - list.top()) as u32);
                    status = format!("Found {}", list.items[i]);
                }
                None => status = format!("No match for {query}"),
            }
        }
    }
}
