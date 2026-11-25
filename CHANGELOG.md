# Changelog

## 0.1.0 — unreleased

First release, extracted from the hand-rolled X11 backend of
[Stele](https://github.com/tuckermclean/stele-browser) (`src/backend/x11.rs`
and `src/backend/fb.rs` at `9b43fc0`) and rebuilt as a general-purpose crate.

Compared with the code it came from:

- **Connection**: TCP displays (SSH X11 forwarding: `localhost:10`), proper
  `.Xauthority` matching (family, address, display number, wildcards),
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
- **Input**: key press/release with Shift/Caps/Num Lock and autorepeat
  detection, media keys, buttons, wheel, coalesced motion, enter/leave,
  focus, close requests, a cross-thread `Waker`.
- **fbdev**: partial updates with positioned writes, 24 bpp, panning-aware
  geometry, never writes row padding.
