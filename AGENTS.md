# Working on protolith — AGENTS.md

How to work on this crate, for AI agents and humans alike. Short on purpose;
the code's doc comments and `README.md` say *what* it is.

## The non-negotiables

1. **No dependencies, no `unsafe`.** `[dependencies]` stays empty and
   `#![forbid(unsafe_code)]` stays on. If something seems to need `unsafe`
   (mmap, ioctl, SCM_RIGHTS, MIT-SHM), find the std-only route or leave the
   feature out — that is how fbdev ended up on sysfs + `pwrite`.
2. **Small, and priced.** protolith started as the X11/fbdev backends of
   Stele (`~/gitrepos/stele-browser`), which must fit a 1.44 MB floppy and
   will depend on this crate. New subsystems go behind cargo features so
   Stele pays only for what it uses. Report the release-size delta of
   anything substantial (build an example with `opt-level=z`, fat LTO,
   `panic=abort`, stripped, and compare).
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
for f in "" "--features x11" "--features fbdev" "--features canvas" "--features bmp"; do cargo build --no-default-features $f; done
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
2. Fonts — a generic glyph-sheet bitmap font (Winamp `text.bmp` style),
   then Stele's Terminus (`src/text/terminus*.rs`, OFL-1.1). **Decided
   (2026-11-25): bring Terminus in now, one cargo feature per pixel size**
   (and weight), so the smallest useful slice is ~3 KB (16px regular)
   rather than ~69 KB for all 5 sizes × 2 weights. Ship the OFL text and
   attribution with it.
3. Text field — the cursor/edit core from Stele's
   `src/backend/address_edit.rs`, generalized.
4. Widgets — Button, Toggle, Slider, List as plain state structs: protolith
   `Event` in, action out (press-inside/release-inside, drag capture,
   focus); appearance separate (sprite sheets, or a default drawn bevel).
   No layout engine, no widget tree, no callbacks.

Out of scope for protolith: `.wsz` (ZIP/inflate) skin archives, audio,
mpg123 control — those belong to the player.

**Later: rewire Stele** onto protolith (a Stele packet, following Stele's
own AGENTS.md; measure the i486 size delta in Stele's CI).

## Housekeeping

- Commits: imperative subject, a body that says why, ending with the
  `Co-Authored-By` trailer the harness provides.
- No remote yet; pushing/creating the GitHub repo needs the user's go-ahead.
- Append notable decisions to `CHANGELOG.md` under the unreleased version.
