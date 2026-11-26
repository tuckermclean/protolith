# Working on protolith — AGENTS.md

How to work on this crate, for AI agents and humans alike. Short on purpose;
the code's doc comments and `README.md` say *what* it is.

## The non-negotiables

1. **No dependencies, no `unsafe`.** `[dependencies]` stays empty and
   `#![forbid(unsafe_code)]` stays on. If something seems to need `unsafe`
   (mmap, ioctl, SCM_RIGHTS, MIT-SHM), find the std-only route or leave the
   feature out — that is how fbdev ended up on sysfs + `pwrite`.
2. **Small, and priced — for a Pentium, stretching to a 486DX2.** That is
   the hardware protolith, Stele (`~/gitrepos/stele-browser`, which must
   also fit a 1.44 MB floppy) and phonolite (`~/gitrepos/phonolite`) are
   for; it is the reason this crate exists. New subsystems go behind cargo
   features so programs pay only for what they use. Report the
   release-size delta of anything substantial — and judge it on the real
   target build, not x86_64: Stele's i486 musl target with std rebuilt
   (`-Zbuild-std`, `-Cpanic=immediate-abort`, no unwind tables; see
   phonolite's `tools/build-i486.sh`). What it showed, so far: libc's DNS
   resolver (~14 KB, now behind `x11-dns`), std's float formatting and
   parsing (~24 KB; avoid `{}` on floats and `str::parse::<f64>`), libm
   (~17 KB), unwind tables and the unwinder (~75 KB, a build setting).
3. **MSRV 1.75, stable Rust.** Test on both (`cargo +1.75 test`).
4. **Test-first, root cause first.** A bug gets a failing test before the
   fix — against Xvfb when it is about real server behavior. Never weaken a
   test to make it pass; if a test was wrong, say why in the commit.
5. **Docs never outrun the tests.** README/CHANGELOG claims must be things a
   test checks. When a feature lands, update both in the same commit.

## The gate (run before every commit)

```sh
cargo fmt --check
RUSTFLAGS="-D warnings" cargo clippy --all-targets --all-features
for f in "" "--features x11" "--features x11-dns" "--features fbdev" "--features canvas" "--features bmp" "--features font" "--features font-terminus-16" "--features ui"; do cargo build --no-default-features $f; done
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps
PROTOLITH_REQUIRE_XVFB=1 cargo test --all-features
PROTOLITH_REQUIRE_XVFB=1 cargo +1.75 test --all-features
for i in $(seq 1 10); do cargo test -q --test xvfb -- --test-threads=16 || break; done
```

The last line matters: races only show under parallel load (two real bugs
were found that way). `.github/workflows/ci.yml` runs the same gate. Needs
`xvfb` and `x11-utils` installed (`xdotool`, `x11-apps` help for driving
and screenshotting examples by hand).

## Test tiers

1. Pure protocol and pixels (`src/x11/proto`, `src/canvas`) — exact bytes;
   decoders (X11, BMP) fuzzed for panics/truncation; canvas ops checked
   against a naive per-pixel reference. BMP fixtures are built field by
   field from the spec in the test, so their expected pixels are visible.
2. Fake server (`src/x11/fake.rs`) — scripted, in-process, real socket. Keep
   it **strict**: it validates `PutImage` geometry like a real server; a
   lenient fake hid two bugs once. Extend its strictness when adding requests.
3. Xvfb end to end (`tests/xvfb.rs`) — one Xvfb per test via `-displayfd`;
   oracles are `GetImage` readback of the *window*, XTEST input, `xprop`.
   `tests/xauth.rs` is separate because it sets env vars.
4. fbdev (`tests/fbdev.rs`) — scratch device file + fake sysfs; checks every
   byte written and every byte that must not be.

## Hard-won X11 lessons (don't relearn them)

- A server discards unread requests from a client that hangs up →
  `Client::drop` round-trips (like `XCloseDisplay`).
- When the server's last client disconnects it resets → tests that connect
  sequentially must hold a connection open.
- Don't recycle a window XID until its `DestroyNotify` arrives.
- Clip before computing `PutImage` row stride, or it's `BadLength`.
- Autorepeat Release+Press pairs can split across reads → XKB detectable
  autorepeat, pair detection only as fallback.
- Without a window manager (Xfbdev, Xvfb) nothing gives you focus; and
  size hints are only hints — some WMs (and XQuartz/VcXsrv) ignore them.
- `WM_NORMAL_HINTS` is rewritten whole: whatever rewrites it (`set_size`
  on a fixed-size window) must keep `USPosition`, or a window manager
  reading it later takes the window as unplaced — twm then grabs the
  server and waits for the user to place it by hand, which looks like a
  hang of every client.
- Without static gravity, a window manager puts the *frame* where a
  position asks and the client inside it: `set_position` to where
  `window_position` says the window is moves it by the frame's border and
  title. Hence `StaticGravity` in every window's size hints.
- Check placement under real window managers (twm, openbox, IceWM,
  Fluxbox, xfwm4: phonolite's `tools/check-windows.sh`); each has its own
  idea (openbox centers transient windows on their owner).
- Don't `pkill -f Xvfb…` from a shell whose own command line contains that
  string; use `pkill -x Xvfb` or kill by PID.

## Where this is going

The motivating application is an mpg123 frontend that loads classic Winamp
2.x skins — several borderless, shaped, draggable windows, sprite-sheet
widgets, an animated visualizer. Art in examples must be original (no
copied skin assets).

**Next: a UI layer** (agreed 2026-11-25), as opt-in features in this crate:

1. **Done:** `canvas` (fill, clip, blend, copy/key/alpha blits,
   nearest-neighbor scaling) and `bmp` (uncompressed 1–32-bit). Pixel ops
   follow Stele's `src/surface/mem.rs`, **not** its CSS-coupled
   `Surface`/`TextRun` API. Measured: +2.7 KB all ops used, +5.1 KB the
   decoder.
2. **Done:** `font` — the `Font` trait, `SheetFont` (sprite-sheet glyphs
   by layout string) and Terminus (OFL-1.1, `third_party/terminus-font/`),
   one feature per size/weight, regenerated from upstream BDF by
   `tools/gen-terminus.py` (byte-identical to Stele's tables). No 12 px
   bold: upstream's is a copy of the regular — when rewiring Stele, map its
   bold 12 to `REGULAR_12`. Measured: 16 px regular +3.4 KB all in.
3. **Done:** `ui::TextEdit` (Stele's `address_edit.rs` core plus
   selection, words, limit) and `ui::TextField` (events → `Response`,
   mouse, scrolling, `FieldStyle`). Measured +6.8 KB. Stele's address bar
   can sit on `TextEdit` directly when rewired.
4. **Done:** `ui::{Button, Toggle, Slider, List}` — events in, `Response`
   out, focus follows clicks, default bevel look or state for sprites.
   Measured +6.6 KB for all four. `Event::Button` gained `time` for the
   list's double click.

5. **Done:** sprite-sheet skins — `ui::{ButtonSprites, ToggleSprites,
   SliderSprites, Frames}` and `draw_sprite`; `examples/skin` (original
   art generated in the classic sheet layout; optionally an unpacked
   classic skin directory). Testing on a real skin showed most classic
   bitmaps are RLE8, so `bmp` decodes RLE8/RLE4 now. The user's
   `~/Downloads/base-2.91.wsz` is reference material only: never vendor
   it or anything extracted from it.

The UI layer is complete, and the player exists: **phonolite**
(`~/gitrepos/phonolite`, path dependency on this crate) — docked shaped
window, sprite-skinned widgets, popups, dialogs from the stock widgets. It
needed two small additions here (`Connection::minimize`,
`WindowId::from_xid`); anything else it lacks is a protolith request to
agree with the user, not a workaround there. Natural next step, to agree
with the user first: the Stele rewire below.

Out of scope for protolith: `.wsz` (ZIP/inflate) skin archives, audio,
mpg123 control — those belong to the player.

**Later: rewire Stele** onto protolith (a Stele packet, following Stele's
own AGENTS.md; measure the i486 size delta in Stele's CI).

## Housekeeping

- Commits: imperative subject, a body that says why, ending with the
  `Co-Authored-By` trailer the harness provides.
- No remote yet; pushing/creating the GitHub repo needs the user's go-ahead.
- Append notable decisions to `CHANGELOG.md` under the unreleased version.
