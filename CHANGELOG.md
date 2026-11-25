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
  (1/4/8/16/24/32-bit uncompressed; RLE and bit fields are refused) reads
  skin sprite sheets. Priced at the floppy profile: +2.7 KB with every
  canvas operation in use, +5.1 KB more for the decoder, nothing when off.
- **fbdev**: partial updates with positioned writes, 24 bpp, the current
  mode and live pan offset honored, never writes row padding.
