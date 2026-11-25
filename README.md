# protolith

**Pixels in a window with nothing underneath.**

protolith puts a buffer of pixels on screen, in an X11 window or straight onto
the Linux console framebuffer, and hands you back the keyboard and mouse. It
is written directly against the X11 wire protocol and the framebuffer device:

- **no dependencies** — `std` only; no Xlib, no xcb, no C toolchain;
- **no `unsafe`** — `#![forbid(unsafe_code)]`, the whole crate;
- **no surprises** — every byte it sends or parses is covered by a test, and
  every feature that does not need a window manager is tested end to end
  against a real X server.

```rust,no_run
use protolith::x11::{Connection, Event, Key, WindowOptions};
use protolith::Frame;

fn main() -> protolith::Result<()> {
    let mut conn = Connection::open()?;
    let win = conn.create_window(WindowOptions::new(256, 256).title("gradient"))?;
    let pixels: Vec<u32> = (0..256 * 256).map(|i| (i % 256) << 16 | (i / 256) << 8).collect();
    conn.present(win, &Frame::xrgb(&pixels, 256, 256)?)?;
    loop {
        match conn.wait_event()? {
            Event::Key { key: Key::Escape, .. } | Event::CloseRequested { .. } => return Ok(()),
            _ => {}
        }
    }
}
```

## What it does

| | |
|---|---|
| **Connect** | `$DISPLAY` over the local socket or TCP — including SSH X11 forwarding (`localhost:10`) — with `MIT-MAGIC-COOKIE-1` from `.Xauthority`, matched the way Xlib matches it. |
| **Draw** | Present `0x00RRGGBB` or RGBA frames, whole or by rectangle. Converted to any TrueColor visual of 8–32 bits in either byte order; split under the server's request limit. Each window keeps its last frame on the server, so exposure never needs your help. Server-side scrolling. Read-back. |
| **Windows** | Several per connection. Title, class, icon, fixed size, no decorations, always-on-top, override-redirect, show/hide, move/resize. **Non-rectangular** windows via SHAPE, with a mask-to-rectangles helper. **Window dragging** for borderless windows: handed to the window manager when it can, done by protolith when there is none. |
| **Input** | Keys (Shift, Caps/Shift Lock, Num Lock, autorepeat detection via XKB where available, media keys), buttons, wheel, coalesced motion with root coordinates, enter/leave, focus, close requests. A `Waker` to interrupt the event loop from another thread. |
| **Canvas** *(feature `canvas`)* | Software drawing on an owned image: fills, clipping, alpha blending (exactly rounded), sprite blits — copied, color-keyed or alpha-blended — and nearest-neighbor scaling, all total over any coordinates. Feature `bmp` decodes uncompressed BMP sprite sheets (1/4/8/16/24/32-bit, both row orders, Windows and OS/2 headers). |
| **Framebuffer** | `/dev/fbN` at 16/24/32 bpp with partial updates, for machines with no X at all. |
| **Escape hatches** | `x11::Client` sends any request and collects any reply, with sequence numbers, errors and events demultiplexed for you. `x11::proto` is the protocol as pure functions. |

Both backends implement one trait, `Present`, for code that should not care
where its pixels go.

## What it doesn't

Deliberately out of scope, so the crate stays small: OpenGL/Vulkan, Wayland
(XWayland works), XInput 2, XKB layout groups and input methods, the
clipboard, MIT-SHM, `BIG-REQUESTS` (large frames are banded instead), palette
(PseudoColor-only) displays, and input from the console framebuffer.

## How it is built

```text
Connection  windows, frames, events          x11::Connection
  Client    one socket, sequence numbers,     x11::Client
            replies / errors / events
    proto   the protocol as pure functions    x11::proto
PixelFormat frame → wire layout conversion    shared with fbdev
```

`proto` is sans-I/O: encoders return the exact request bytes, decoders take a
byte slice and are total — a truncated or hostile buffer gives `None` or
`Err`, never a panic. `Client` counts requests in 64 bits and widens every
16-bit stamp from the server, so a reply or error is always matched to the
request that caused it, even after the wire counter wraps.

## Testing

```sh
cargo test --all-features                            # everything; Xvfb tests skip if Xvfb is absent
PROTOLITH_REQUIRE_XVFB=1 cargo test --all-features   # what CI runs: a missing Xvfb is a failure
```

Four tiers:

1. **Pure protocol and pixels** — every encoder byte-for-byte, every decoder
   (including BMP) against truncation and random input; canvas blits against
   a per-pixel reference over random rectangles and clips, blending
   rounding checked exhaustively.
2. **Fake server** — an in-process, scripted X server over a real socket pair
   drives `Client` and `Connection` deterministically: interleaved events and
   replies, errors routed by sequence number, 70 000-request wraparound,
   hostile byte streams, resize bursts, drags.
3. **End to end** — each test starts its own Xvfb (at 24, 16 and 8 bits deep,
   over Unix sockets, the abstract socket and TCP, with and without cookies,
   including lookup through `$XAUTHORITY`), then checks pixels by reading the
   window back with `GetImage`, injects real input with XTEST (keys, Caps
   Lock, held-key autorepeat, buttons, wheel, drags), and cross-checks window
   properties with `xprop` — oracles that are not protolith checking itself.
   Not covered end to end, because Xvfb runs no window manager: the
   `_NET_WM_MOVERESIZE` hand-off and always-on-top requests to a running WM
   (both are byte-checked against the fake server).
4. **Framebuffer** — a scratch device file and fake sysfs, verifying every
   byte written and every byte that must not be.

Install `xvfb` and `x11-utils` to run all of it locally.

## Examples

```sh
cargo run --example hello     # paint with the mouse
cargo run --example shaped    # a borderless, rounded, draggable little player face
cargo run --example canvas --features bmp   # sprites, color key, blending, 2x scaling
cargo run --example fbdev     # a test card on /dev/fb0 (from a text console)
```

## Minimum Rust

1.75.

## Origin

protolith began as the X11 and framebuffer backends of
[Stele](https://github.com/tuckermclean/stele-browser), a document-web browser
for the 486 that must fit on a floppy, which is why it has no dependencies and
counts its bytes.

## License

MIT or Apache-2.0, at your option.
