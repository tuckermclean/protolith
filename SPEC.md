# SPEC: protolith

## What this is
protolith puts a buffer of pixels on screen, in an X11 window or on the Linux console framebuffer, and hands back
the keyboard and mouse. It is a Rust crate written against the X11 wire protocol and the framebuffer device, with
no Xlib, no xcb and no C toolchain: `std` only, no dependencies and no `unsafe`. It began as the X11 and framebuffer
backends of Stele, a document-web browser for the 486 that must fit on a floppy, which is why it counts its bytes.

Default features `x11` and `fbdev`; opt-in features `canvas` (software drawing), `bmp` (BMP decoding), `font` and
one `font-terminus-<size>[-bold]` feature per Terminus size and weight, `ui` (widgets) and `x11-dns` (DNS names
for TCP displays). Its programs: phonolite (a skinned audio player), lintel (a window manager), and later Stele.

Not part of protolith, so the crate stays small: OpenGL/Vulkan, Wayland (XWayland works), XInput 2, XKB layout
groups and input methods, the clipboard, MIT-SHM, `BIG-REQUESTS` (large frames are banded instead), palette
(PseudoColor-only) displays, and input from the console framebuffer. Also not here: `.wsz` (ZIP/inflate) skin
archives, audio and mpg123 control (the player's work), and window-manager policy such as layout, decorations
and EWMH (lintel's work; protolith gives a window manager its requests and events).

## Principles
Every node respects these (from `AGENTS.md`, which has the detail):
1. **No dependencies, no `unsafe`.** `[dependencies]` stays empty and `#![forbid(unsafe_code)]` stays on. A
   feature that seems to need `unsafe` gets a std-only route or is left out.
2. **Small, and priced, for a Pentium stretching to a 486DX2.** New subsystems go behind cargo features, so a
   program pays only for what it uses. A substantial change reports its release-size delta, judged on Stele's
   i486 musl build, not on x86_64.
3. **MSRV 1.75, stable Rust.** Tested on both.
4. **Test-first, root cause first.** A bug gets a failing test before the fix, against Xvfb when it is about real
   server behaviour. A test is never weakened to make it pass; a test that was wrong says why in the commit.
5. **Docs never outrun the tests.** README and CHANGELOG claim only what a test checks, and are updated in the
   same change as the feature.
6. **The hard-won X11 lessons** in `AGENTS.md` hold; a change does not relearn them.
7. **Original art only.** Art in examples is original. Classic skins (such as `base-2.91.wsz`) are reference
   material only: never vendored, nor anything extracted from them.

## State
Done (nothing is half-built):
- the X11 client: connection and authentication, windows, frames, events, input, SHAPE windows, dragging;
- the framebuffer presenter, `/dev/fbN` at 16, 24 and 32 bpp;
- the UI layer, as opt-in features, motivated by a frontend that wears classic 2.x player skins (several
  borderless, shaped, draggable windows, sprite-sheet widgets, an animated visualizer), with the size each
  part measured:
  - `canvas` (+2.7 KB with all ops used) and `bmp` (+5.1 KB; uncompressed 1–32-bit, and RLE8 and RLE4, since most
    classic skin bitmaps are RLE8). Pixel ops follow Stele's `src/surface/mem.rs`, not its CSS-coupled API;
  - `font`: `SheetFont` and Terminus, regenerated from upstream BDF by `tools/gen-terminus.py`, byte-identical to
    Stele's tables (+3.4 KB for 16 px regular, all in);
  - `ui::TextEdit` and `ui::TextField` (+6.8 KB), then `ui::{Button, Toggle, Slider, List}` (+6.6 KB for all four);
  - sprite-sheet skins (`ui::{ButtonSprites, ToggleSprites, SliderSprites, Frames}`, `draw_sprite`) and
    `examples/skin`, with original art in the classic sheet layout;
- what phonolite needed (`Connection::minimize`, `WindowId::from_xid`), what a window manager such as lintel
  needs (`become_window_manager`, frame windows, `WindowOptions::parent`, `Event::Foreign` and the WM requests),
  and `Connection::window_position` and `WindowOptions::transient_for`.

Planned, in this order:
1. Unit tests move out of the source files: each inline `#[cfg(test)] mod tests { … }` goes to a file of its own
   (`src/x11/client.rs` → `src/x11/client/tests.rs`, with `#[cfg(test)] mod tests;` left in its place), the tests
   unchanged, so the tester role can write unit tests. This comes before any other planned work.
2. Client memory, issue #1 part A: PutImages in bands of a fixed budget, written straight into `Client::out`,
   which stays at or below `OUT_BUF_CAP`; no whole-window conversion buffer and no per-request copy.
3. Client memory, issue #1 part B: palette BMPs kept indexed at one byte per pixel, with blits from the indexed
   image into a `Canvas`.

Not planned here: rewiring Stele onto protolith is Stele's own work (when it happens, Stele maps its 12 px bold to
`REGULAR_12`, because upstream Terminus has no real 12 px bold). New needs of phonolite or lintel come in as
issues and are agreed with the owner before they are planned: what a program lacks is a request to protolith,
not a workaround in that program.

## Architecture
```text
Connection  windows, frames, events          x11::Connection   (src/x11/connection.rs)
  Client    one socket, sequence numbers,     x11::Client       (src/x11/client.rs)
            replies / errors / events
    proto   the protocol as pure functions    x11::proto        (src/x11/proto/)
PixelFormat frame → wire layout conversion    shared with fbdev (src/pixel.rs)
```
- `x11::proto` is sans-I/O: encoders return the exact request bytes; decoders are total over any byte slice
  (`None` or `Err`, never a panic). `Client` counts requests in 64 bits and widens every 16-bit stamp from the
  server, so replies and errors match their requests across wraparound.
- `src/x11/` also holds the display and `.Xauthority` lookup (`display.rs`), the keyboard map (`keyboard.rs`),
  SHAPE helpers (`shape.rs`) and, for tests only, a strict in-process fake X server (`fake.rs`).
- `src/fbdev.rs` presents to `/dev/fbN` through sysfs and `pwrite`. Both backends implement the `Present` trait
  (`src/lib.rs`); `src/error.rs` is the crate's error type.
- `src/canvas/` (feature `canvas`; `bmp.rs` behind `bmp`), `src/font/` (feature `font`; the Terminus tables in
  `src/font/terminus/*.bin` are generated by `tools/gen-terminus.py`) and `src/ui/` (feature `ui`) are the UI
  layer: plain state structs, events in and a `Response` out, drawn onto a canvas when the program chooses.
- `examples/` are runnable programs; `tests/` holds the Xvfb end-to-end tests (`xvfb.rs`, `xauth.rs`) and the
  framebuffer tests (`fbdev.rs`). CI is `.github/workflows/ci.yml`.

## The regime
The graph (`docs/graph.yaml`) holds the gate (the test command, its failure pattern and the failures it already
had), the paths out of scope with a reason each, and the nodes; each node owns files through its `owns` globs and
has a spec `nodes/<ID>-<name>.md`. Open issues become planned nodes through spec PRs. How a node is built is in
`AGENTS.md`, section "Building a node".

The graph's gate is CI's test step. CI's other checks (the MSRV 1.75 run, the parallel Xvfb stress loop, `cargo
fmt`, clippy, rustdoc and the feature builds) stay required on every PR and are AGENTS.md's pre-commit list, but
they are not the graph's gate: they name no failing test in a line `fail_pattern` can read.

## Node spec template
A node spec `nodes/<ID>-<name>.md` is titled `# <ID> — <name>` and has a header row
`| Track | Deps | Unlocks | Size |`, an **Owns:** line naming its `owns` globs in backticks, and the headings
Purpose, Tests, Changes, Gate and Risks. The Gate's criteria read `- **<ID>-AC-<n>** — … *Verify:* …`, numbered
from 1; *Verify:* names the test that shows the criterion, or reads `unpinned` when no test does.

## Open decisions
Each has a default, used unless the owner decides otherwise; issue #1's two are settled when its nodes are written.
- **OD1, the band budget (issue #1 A).** Default: 32 KB, by lowering `OUT_BUF_CAP` to it, so every band fits the
  output buffer and the buffer's size is the band budget.
- **OD2, the indexed image API (issue #1 B).** Default: a new `bmp::decode_indexed` returning a new
  `canvas::IndexedImage` (width, height, `Vec<u8>` pixels, a palette of up to 256 colours); `bmp::decode` keeps
  returning a `Canvas`, unchanged.
- **OD3, unit tests for a new source file.** The tester may not write source files, so it cannot add the
  `#[cfg(test)] mod tests;` line a new file would need. Default: a new file's unit tests go in its parent
  module's `tests.rs` (reaching it by its crate path), or in `tests/` when they test public API; a later node may
  move them next to the file once it exists.

## Conventions
- Branches `node/<ID>-<name>` for a node and `spec/<topic>` for a spec change; never a push to `main`.
- Sizes: S (under a day), M (two or three days), L (more; split it if it can be split).
- Commits: an imperative subject and a body that says why; notable decisions go into `CHANGELOG.md` under the
  unreleased version.
- A node is done when its gate passes in CI, CI's other required checks pass, its README and CHANGELOG claims
  are tested, its size delta is reported where it is substantial, and its PR is merged.
