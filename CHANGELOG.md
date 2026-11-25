# Changelog

## 0.1.0 — unreleased

First release, extracted from the hand-rolled X11 backend of
[Stele](https://github.com/tuckermclean/stele-browser) (`src/backend/x11.rs`
and `src/backend/fb.rs` at `9b43fc0`) and rebuilt as a general-purpose crate.

Compared with the code it came from:

- **Connection**: TCP displays (SSH X11 forwarding: `localhost:10`), the
  Linux abstract socket as a fallback, `.Xauthority` matching as libXau does
  it (file order; family, peer address, display number, wildcards),
  multiple screens.
- **Protocol correctness**: 64-bit sequence tracking with wraparound, errors
  routed to the request that caused them, pipelined replies, a sync before the
  16-bit counter can wrap, a sync on drop so the last requests are not
  discarded, the server's image byte order honored.
- **Pixels**: any TrueColor visual of 8–32 bits via channel masks (including a
  non-root visual with its own colormap on 8-bit PseudoColor servers), RGBA or
  XRGB sources, partial presents, server-side scroll, backing-pixmap exposure
  handling, read-back.
- **Windows**: several per connection; title, class, icon, size hints,
  undecorated, always-on-top, override-redirect, show/hide, move/resize,
  SHAPE (non-rectangular) windows, window dragging (WM hand-off or self-driven).
- **Input**: key press/release with Shift/Caps/Shift/Num Lock per the
  protocol's rules, autorepeat detection (XKB detectable autorepeat, with a
  core-protocol fallback), media keys, buttons, wheel, coalesced motion, enter/leave,
  focus, close requests, a cross-thread `Waker`.
- **Canvas** (new, features `canvas` and `bmp`): an owned `0x00RRGGBB`
  image with clip, fill, exactly rounded alpha blending, and blits from any
  `Frame` — copy, color key, alpha, nearest-neighbor scaled. The pixel
  operations follow Stele's `src/surface/mem.rs`, but on XRGB words and
  without its CSS-shaped `Surface`/`TextRun` API. A total BMP decoder
  (1/4/8/16/24/32-bit, and RLE8/RLE4 — 11 of the 18 bitmaps in the
  classic base skin are RLE8; bit fields are refused) reads skin sprite
  sheets. Priced at the floppy profile: +2.7 KB with every canvas
  operation in use, +5.7 KB more for the decoder, nothing when off.
- **Text** (new, feature `font`): a `Font` trait; `SheetFont`, glyphs cut
  from a skin sprite sheet by a layout string; and a 191-glyph subset of
  Terminus Font (OFL-1.1, from upstream 4.49.1 BDF via
  `tools/gen-terminus.py`), one feature per size and weight
  (`font-terminus-{12,16,20,24,32}` and `-bold` for 16–32). Stored as flat
  bitmaps rather than Stele's per-glyph slices. There is no 12 px bold:
  upstream's `ter-u12b` is a copy of the regular. `Canvas::fill_mask` draws
  1-bit bitmaps. Priced at the floppy profile: 16 px regular +3.4 KB in
  all (3,056 bytes of it glyphs), each further table its data size
  (+3.1 KB for 16 px bold, +12.6 KB for 32 px), sheet fonts ~1.9 KB of
  code over the canvas.
- **Widgets** (new, feature `ui`): `TextEdit`, a single-line buffer
  generalized from Stele's `AddressEdit` (`src/backend/address_edit.rs`):
  selection with an anchor, word motions and deletes, `Motion`-driven
  delete, a length limit, control characters never stored. `TextField`
  puts it on screen: X11 events in, `Response` out (`Ignored`, `Redraw`,
  `Changed`, `Submit`, `Cancel`); click/drag/shift-click placement,
  scrolling that keeps the cursor visible, `FieldStyle` drawing. Word
  boundaries match Unicode exactly through Latin-1 and use a cheap rule
  past it, saving 2.8 KB of Unicode tables. Priced: +6.8 KB.
- **More widgets** (feature `ui`): `Button` (fires on release over it,
  backs out if dragged off; Space/Enter when focused), `Toggle`, `Slider`
  (horizontal or vertical, grab-point or jump-to drag that follows the
  pointer anywhere, wheel, arrow/page/Home/End keys; `Submit` at drag end
  for seek bars), and `List` (click to select, double click or Enter to
  submit, keyboard and wheel scrolling, `top`/`max_top` to pair with a
  slider as scroll bar). Shared `Style` and `Response::Clicked`; focus
  follows left clicks; every widget can be disabled. Priced: +6.6 KB for
  all four, handled and drawn.
- **Sprite-sheet skins** for widgets: `ButtonSprites` (up, down),
  `ToggleSprites` (off/on × up/down), `SliderSprites` (track `Frames`
  picked by value — linearly, or outward from the center for a balance —
  and thumb up/down, centered on the thumb's slot), each with a
  `draw_sprite`. Sheets are blitted with alpha, so RGBA sheets with
  transparency work and XRGB ones are copied. Shaped by the real layouts
  of a classic skin; `examples/skin` draws a player face from a skin it
  generates in that layout, or from an unpacked classic skin directory.
  Priced: under 1 KB more than the default bevel drawing.
- **`Event::Button` carries the server `time`**, so double clicks can be
  detected.
- **`Connection::minimize`** (ICCCM `WM_CHANGE_STATE` to IconicState; a
  borderless window has no other way to be minimized; does nothing without
  a window manager, which is what would bring it back), and
  **`WindowId::from_xid`**, so code that handles events can be tested
  with events built by hand. Both found missing while building phonolite.
- **fbdev**: partial updates with positioned writes, 24 bpp, the current
  mode and live pan offset honored, never writes row padding.
