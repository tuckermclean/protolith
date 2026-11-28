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

What is done, what is planned and in what order, and the open decisions are in
[`SPEC.md`](SPEC.md) (its sections "State" and "Open decisions"); the scope
and the principles every change keeps are there too.

## Housekeeping

- Commits: imperative subject, a body that says why, ending with the
  `Co-Authored-By` trailer the harness provides.
- Branches and pull requests follow "Building a node" below; never push to
  `main`.
- Append notable decisions to `CHANGELOG.md` under the unreleased version.

## Building a node

The work is planned as a graph of nodes (`docs/graph.yaml`, one spec
`nodes/<ID>-<name>.md` each); `SPEC.md` says what is done and what comes
next.

- **Building.** Build a node with `/noodlebot:node <ID>`.
- **Authority order.** `SPEC.md`, then `docs/graph.yaml`, then the node
  spec, then this file. When two disagree, the earlier one wins and the
  later one is fixed by a spec PR.
- **Roles.** The plugin's tester writes the failing tests, its implementer
  makes them pass, its reviewer judges the result; this repository has no
  role agents of its own. `.noodlebot/roles.yaml` gives the role guard this
  crate's test paths (below).
- **Spec checkers.** From the repository root,
  `python3 -I <plugin dir>/skills/node/check-regime.py` and
  `python3 -I <plugin dir>/skills/node/check-coverage.py` must both print
  `PROBLEMS 0` before a spec PR is opened.
- **Gate command.** `PROTOLITH_REQUIRE_XVFB=1 cargo test --all-features`,
  the whole suite (unit, integration and doc tests). It runs in CI on the
  PR (`.github/workflows/ci.yml`, jobs `test (stable)` and `test (1.75)`).
  A failing test prints `test <name> ... FAILED` (a doc-test's name reads
  like `src/canvas/mod.rs - canvas (line 11)`), the line the graph's
  `fail_pattern` matches. The other commands of "The gate" above are still
  run before every commit and are required in CI, but they are not the
  graph's gate.
- **CI's output.** A CI log can add colour escape codes
  (`\x1b\[[0-9;]*m`; the workflow sets `CARGO_TERM_COLOR: always`) and, in
  a fetched job log, a timestamp prefix on every line (`^\S+Z `).
  `fail_pattern` matches the lines once both are stripped. Run the gate
  locally with `CI=true` and the agent session's variables (such as
  `CLAUDECODE`, `AI_AGENT`) unset, as CI sees it.
- **Quick gate.** In a session, run locally instead the red targets and
  the tests of what a change touches, through cargo's own test runner, which
  prints the same `... FAILED` lines (cargo takes test targets and name
  filters, not file paths):
  - a touched `tests/<stem>.rs`:
    `PROTOLITH_REQUIRE_XVFB=1 cargo test --all-features --test <stem>`;
  - a touched source file (or its `tests.rs`):
    `PROTOLITH_REQUIRE_XVFB=1 cargo test --all-features --lib -- <module path>::`,
    the filter being the file's module path (`src/x11/client.rs` →
    `x11::client::`, `src/canvas/mod.rs` → `canvas::`; `src/x11/fake.rs`
    serves the x11 tests, so `x11::`); several filters may follow `--`;
  - touched doc comments with examples:
    `PROTOLITH_REQUIRE_XVFB=1 cargo test --all-features --doc`.
  When unsure which tests a change reaches, run the whole gate (about ten
  seconds once built).
- **Bounded workers.** libtest's `-- --test-threads=<n>` limits the test
  threads (CI's Xvfb stress loop uses 16); each Xvfb test starts its own
  server.
- **Known red.** The graph's `gate.known_red` lists the tests that fail
  today, by the full name their `FAILED` line prints. A failing test that
  is not listed is a failure of the change. A listed failure stays until a
  node fixes it; that node removes it from the list in its own PR.
- **Owns.** A node changes only the files it owns (its `owns` globs) and
  its direct deps' files, and puts every file it adds where the graph's
  globs cover it.
- **Red target.** A command that exits non-zero before the node's change
  and 0 after, normally one of the quick-gate commands narrowed to the
  node's new tests (`... --lib -- <module path>::tests::<test name>`, or
  `... --test <stem> <test name>`). A test that does not compile yet is
  red too.
- **Tests and fixtures.** Integration tests are `tests/*.rs`. Unit tests
  are a source file's `tests` module; in their own file
  `src/<path>/<file>/tests.rs` (or `src/<dir>/tests.rs` for a `mod.rs`),
  declared by `#[cfg(test)] mod tests;` in the source file, they are
  test paths for the role guard. `src/x11/fake.rs` (the fake X server) is a
  test path too. There are no fixture files: BMP fixtures are built field
  by field in the test code. A new source file's unit tests follow
  `SPEC.md`'s open decision OD3.
- **Spec files:** `SPEC.md`, `docs/graph.yaml`, `nodes/` (a trailing slash marks a folder); a spec PR changes only these.
- **Porting rules.** None.
- **CI evidence.** In the node's PR, record the `test` job's gate output
  (stable and 1.75) for the PR's head commit with every other CI job green,
  the red target's output before and after, and, for a substantial change,
  the release-size delta on the i486 build (non-negotiable 2).
