//! An in-process, scripted X server for unit tests: real bytes over a real
//! `UnixStream::pair()`, so [`Client`](super::Client) and
//! [`Connection`](super::Connection) are exercised exactly as against a
//! real server, deterministically and without one installed.
//!
//! It records every request, answers the reply-bearing requests protolith
//! uses with plausible replies (atoms are numbered from 100; properties set
//! with `ChangeProperty` are returned by `GetProperty`), fails requests by
//! opcode on demand, and lets a test inject arbitrary bytes (events,
//! errors, garbage) into the stream.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::display::Auth;
use super::proto::request::opcode;
use super::proto::setup::tests::{formats_24, synth_setup, truecolor};
use super::proto::setup::{Depth, Format, Visual, VisualClass, SUBSTRUCTURE_REDIRECT_MASK};
use super::proto::ReadLe;
use super::Client;
use crate::error::Result;

/// What the fake server should do.
#[derive(Debug, Clone)]
pub(crate) struct Script {
    /// Refuse the connection with this reason.
    pub(crate) refuse: Option<String>,
    /// Answer requests with these major opcodes with a `BadMatch` error.
    pub(crate) fail_opcodes: Vec<u8>,
    /// `(opcode, n)`: fail that opcode's n-th (0-based) and later uses.
    pub(crate) fail_from: Vec<(u8, usize)>,
    /// Advertised maximum request length (words).
    pub(crate) max_request_words: u16,
    /// Pretend a window manager is running.
    pub(crate) window_manager: bool,
    /// Screen setup: `Depth24` (TrueColor root) or `Pseudo8` (8-bit
    /// PseudoColor root with an 8-bit TrueColor visual beside it).
    pub(crate) screen: FakeScreen,
    /// Advertise the SHAPE extension (major opcode 129).
    pub(crate) shape: bool,
    /// `0` LSBFirst, `1` MSBFirst image byte order.
    pub(crate) image_byte_order: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FakeScreen {
    Depth24,
    Depth16,
    Pseudo8,
}

impl Default for Script {
    fn default() -> Script {
        Script {
            refuse: None,
            fail_opcodes: vec![],
            fail_from: vec![],
            max_request_words: 65535,
            window_manager: false,
            screen: FakeScreen::Depth24,
            shape: true,
            image_byte_order: 0,
        }
    }
}

/// The fake's root window, and the SHAPE major opcode it advertises.
pub(crate) const ROOT: u32 = 0x1ab;
pub(crate) const SHAPE_MAJOR: u8 = 129;
pub(crate) const FOCUS_WINDOW: u32 = 0x55;

/// What the server saw.
#[derive(Debug, Default, Clone)]
pub(crate) struct Log {
    pub(crate) auth_name: Vec<u8>,
    pub(crate) auth_data: Vec<u8>,
    /// Every request, verbatim, in order.
    pub(crate) requests: Vec<Vec<u8>>,
}

impl Log {
    /// Requests with major opcode `op`.
    pub(crate) fn with_opcode(&self, op: u8) -> Vec<&Vec<u8>> {
        self.requests.iter().filter(|r| r[0] == op).collect()
    }
}

#[derive(Default)]
struct State {
    log: Log,
    atoms: HashMap<String, u32>,
    props: HashMap<(u32, u32), (u32, u8, Vec<u8>)>,
}

/// A running fake server.
pub(crate) struct FakeServer {
    writer: Arc<Mutex<UnixStream>>,
    state: Arc<Mutex<State>>,
    thread: Option<JoinHandle<()>>,
}

impl FakeServer {
    pub(crate) fn start(script: Script) -> (Result<Client>, FakeServer) {
        FakeServer::start_with_auth(script, Auth::none())
    }

    pub(crate) fn start_with_auth(script: Script, auth: Auth) -> (Result<Client>, FakeServer) {
        let (client_end, server_end) = UnixStream::pair().expect("socketpair");
        let writer = Arc::new(Mutex::new(server_end.try_clone().expect("clone")));
        let state = Arc::new(Mutex::new(State::default()));
        let thread = {
            let writer = writer.clone();
            let state = state.clone();
            std::thread::spawn(move || serve(server_end, writer, state, script))
        };
        let client = Client::from_unix_stream(client_end, &auth);
        (client, FakeServer { writer, state, thread: Some(thread) })
    }

    /// Write raw bytes to the client.
    pub(crate) fn push_raw(&self, bytes: Vec<u8>) {
        let _ = self.writer.lock().unwrap().write_all(&bytes);
    }

    /// Push a 32-byte event (sequence field left as given).
    pub(crate) fn push_event(&self, event: [u8; 32]) {
        self.push_raw(event.to_vec());
    }

    /// The atom the server assigned to `name` (interning it if new).
    pub(crate) fn atom(&self, name: &str) -> u32 {
        intern(&mut self.state.lock().unwrap(), name)
    }

    /// A property as last set: `(type, format, bytes)`.
    pub(crate) fn property(&self, window: u32, name: &str) -> Option<(u32, u8, Vec<u8>)> {
        let atom = predefined(name).unwrap_or_else(|| self.atom(name));
        self.state.lock().unwrap().props.get(&(window, atom)).cloned()
    }

    /// Snapshot of the log so far.
    pub(crate) fn log(&self) -> Log {
        self.state.lock().unwrap().log.clone()
    }

    /// Close the connection, stop the server, and return its log.
    pub(crate) fn finish(mut self) -> Log {
        self.stop();
        self.state.lock().unwrap().log.clone()
    }

    fn stop(&mut self) {
        let _ = self.writer.lock().unwrap().shutdown(Shutdown::Both);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn predefined(name: &str) -> Option<u32> {
    use super::proto::atom::*;
    Some(match name {
        "WM_NAME" => WM_NAME,
        "WM_ICON_NAME" => WM_ICON_NAME,
        "WM_CLASS" => WM_CLASS,
        "WM_NORMAL_HINTS" => WM_NORMAL_HINTS,
        "STRING" => STRING,
        "ATOM" => ATOM,
        "CARDINAL" => CARDINAL,
        _ => return None,
    })
}

fn intern(state: &mut State, name: &str) -> u32 {
    if let Some(a) = predefined(name) {
        return a;
    }
    let next = 100 + state.atoms.len() as u32;
    *state.atoms.entry(name.to_string()).or_insert(next)
}

fn setup_bytes(script: &Script) -> Vec<u8> {
    let masks = if script.window_manager { SUBSTRUCTURE_REDIRECT_MASK } else { 0 };
    match script.screen {
        FakeScreen::Depth24 => {
            let depths = vec![Depth { depth: 24, visuals: vec![truecolor(0x21, 0xff0000, 0xff00, 0xff)] }];
            synth_setup(script.max_request_words, script.image_byte_order, &formats_24(), &[(ROOT, 0x21, 24, masks, depths)])
        }
        FakeScreen::Depth16 => {
            let depths = vec![Depth { depth: 16, visuals: vec![truecolor(0x21, 0xf800, 0x07e0, 0x1f)] }];
            synth_setup(script.max_request_words, script.image_byte_order, &formats_24(), &[(ROOT, 0x21, 16, masks, depths)])
        }
        FakeScreen::Pseudo8 => {
            let pseudo = Visual { id: 0x21, class: VisualClass::PseudoColor, bits_per_rgb: 8, colormap_entries: 256, red_mask: 0, green_mask: 0, blue_mask: 0 };
            let depths = vec![Depth { depth: 8, visuals: vec![pseudo, truecolor(0x24, 0x07, 0x38, 0xc0)] }];
            let formats = vec![Format { depth: 8, bits_per_pixel: 8, scanline_pad: 32 }];
            synth_setup(script.max_request_words, script.image_byte_order, &formats, &[(ROOT, 0x21, 8, masks, depths)])
        }
    }
}

fn reply(seq: u16, data1: u8, body: &[u8], extra: &[u8]) -> Vec<u8> {
    // body: bytes 8..32; extra: trailing words.
    let mut r = vec![1, data1];
    r.extend_from_slice(&seq.to_le_bytes());
    r.extend_from_slice(&((extra.len() / 4) as u32).to_le_bytes());
    r.extend_from_slice(body);
    r.resize(32, 0);
    r.extend_from_slice(extra);
    r
}

fn serve(mut stream: UnixStream, writer: Arc<Mutex<UnixStream>>, state: Arc<Mutex<State>>, script: Script) {
    // --- connection setup
    let mut head = [0u8; 12];
    if stream.read_exact(&mut head).is_err() {
        return;
    }
    let name_len = usize::from(u16::from_le_bytes([head[6], head[7]]));
    let data_len = usize::from(u16::from_le_bytes([head[8], head[9]]));
    let pad = |n: usize| (4 - n % 4) % 4;
    let mut rest = vec![0u8; name_len + pad(name_len) + data_len + pad(data_len)];
    if stream.read_exact(&mut rest).is_err() {
        return;
    }
    {
        let mut st = state.lock().unwrap();
        st.log.auth_name = rest[..name_len].to_vec();
        let d0 = name_len + pad(name_len);
        st.log.auth_data = rest[d0..d0 + data_len].to_vec();
    }
    let send = |bytes: &[u8]| {
        let _ = writer.lock().unwrap().write_all(bytes);
    };
    if let Some(reason) = &script.refuse {
        let mut r = vec![0, reason.len() as u8, 11, 0, 0, 0];
        let words = (reason.len() + pad(reason.len())) / 4;
        r.extend_from_slice(&(words as u16).to_le_bytes());
        r.extend_from_slice(reason.as_bytes());
        r.resize(8 + words * 4, 0);
        send(&r);
        return;
    }
    send(&setup_bytes(&script));

    // --- requests
    let mut seq: u16 = 0;
    let mut uses: HashMap<u8, usize> = HashMap::new();
    let bits_per_pixel = |depth: u8| match depth {
        24 | 32 => 32usize,
        16 => 16,
        _ => 8,
    };
    let error = |code: u8, seq: u16, req: &[u8]| {
        let mut e = vec![0u8, code];
        e.extend_from_slice(&seq.to_le_bytes());
        e.extend_from_slice(&req.u32_at(4).unwrap_or(0).to_le_bytes());
        e.extend_from_slice(&0u16.to_le_bytes());
        e.push(req[0]);
        e.resize(32, 0);
        e
    };
    loop {
        let mut h = [0u8; 4];
        if stream.read_exact(&mut h).is_err() {
            return;
        }
        let words = usize::from(u16::from_le_bytes([h[2], h[3]]));
        let mut req = h.to_vec();
        req.resize(words.max(1) * 4, 0);
        if stream.read_exact(&mut req[4..]).is_err() {
            return;
        }
        seq = seq.wrapping_add(1);
        let op = req[0];
        state.lock().unwrap().log.requests.push(req.clone());

        let n = uses.entry(op).or_insert(0);
        let nth = *n;
        *n += 1;
        if script.fail_opcodes.contains(&op) || script.fail_from.iter().any(|&(o, from)| o == op && nth >= from) {
            send(&error(8, seq, &req));
            continue;
        }
        if op == opcode::PUT_IMAGE {
            // Like a real server: the data must be exactly `height` rows of
            // the format's padded stride, or the request is BadLength.
            let (w, h, depth) = (usize::from(req.u16_at(12).unwrap()), usize::from(req.u16_at(14).unwrap()), req[21]);
            let stride = (w * bits_per_pixel(depth)).div_ceil(32) * 4;
            if req.len() - 24 != stride * h {
                send(&error(16, seq, &req));
                continue;
            }
        }

        let r = match op {
            opcode::GET_INPUT_FOCUS => Some(reply(seq, 2, &FOCUS_WINDOW.to_le_bytes(), &[])),
            opcode::INTERN_ATOM => {
                let n = usize::from(req.u16_at(4).unwrap_or(0));
                let name = String::from_utf8_lossy(&req[8..8 + n]).into_owned();
                let atom = intern(&mut state.lock().unwrap(), &name);
                Some(reply(seq, 0, &atom.to_le_bytes(), &[]))
            }
            opcode::QUERY_EXTENSION => {
                let n = usize::from(req.u16_at(4).unwrap_or(0));
                let present = script.shape && &req[8..8 + n] == b"SHAPE";
                Some(reply(seq, 0, &[u8::from(present), if present { SHAPE_MAJOR } else { 0 }, 64, 128], &[]))
            }
            opcode::GET_KEYBOARD_MAPPING => {
                // keycode 8 a/A, 9 1/!, 10 Return, 11 Num_Lock, 12 KP_Home/KP_7
                let syms: [u32; 10] = [0x61, 0x41, 0x31, 0x21, 0xff0d, 0, 0xff7f, 0, 0xff95, 0xffb7];
                let first = req[4];
                let count = usize::from(req[5]);
                let mut extra = Vec::new();
                for i in 0..count {
                    let kc = usize::from(first) + i;
                    for col in 0..2 {
                        let s = if (8..13).contains(&kc) { syms[(kc - 8) * 2 + col] } else { 0 };
                        extra.extend_from_slice(&s.to_le_bytes());
                    }
                }
                Some(reply(seq, 2, &[], &extra))
            }
            opcode::GET_MODIFIER_MAPPING => {
                let mut extra = vec![0u8; 8];
                extra[4] = 11; // Mod2 = Num_Lock (keycode 11)
                Some(reply(seq, 1, &[], &extra))
            }
            opcode::CHANGE_PROPERTY => {
                let (w, prop, ty, fmt, n) =
                    (req.u32_at(4).unwrap(), req.u32_at(8).unwrap(), req.u32_at(12).unwrap(), req[16], req.u32_at(20).unwrap() as usize);
                let bytes = req[24..24 + n * usize::from(fmt / 8)].to_vec();
                state.lock().unwrap().props.insert((w, prop), (ty, fmt, bytes));
                None
            }
            opcode::GET_PROPERTY => {
                let (w, prop) = (req.u32_at(4).unwrap(), req.u32_at(8).unwrap());
                let p = state.lock().unwrap().props.get(&(w, prop)).cloned();
                Some(match p {
                    Some((ty, fmt, bytes)) => {
                        let mut body = ty.to_le_bytes().to_vec();
                        body.extend_from_slice(&0u32.to_le_bytes());
                        body.extend_from_slice(&((bytes.len() / usize::from(fmt / 8)) as u32).to_le_bytes());
                        let mut extra = bytes.clone();
                        extra.resize(bytes.len() + pad(bytes.len()), 0);
                        reply(seq, fmt, &body, &extra)
                    }
                    None => reply(seq, 0, &[], &[]),
                })
            }
            opcode::GET_WINDOW_ATTRIBUTES => {
                let mut body = vec![0u8; 24];
                body[18] = 2; // map-state Viewable (offset 26)
                let masks = if script.window_manager { SUBSTRUCTURE_REDIRECT_MASK } else { 0 };
                let mut extra = vec![0u8; 12];
                extra[0..4].copy_from_slice(&masks.to_le_bytes()); // all-event-masks @32
                Some(reply(seq, 0, &body, &extra))
            }
            opcode::TRANSLATE_COORDINATES => {
                let x = req.i16_at(12).unwrap();
                let y = req.i16_at(14).unwrap();
                let mut body = vec![0u8; 8];
                body[4..6].copy_from_slice(&(x + 100).to_le_bytes());
                body[6..8].copy_from_slice(&(y + 50).to_le_bytes());
                Some(reply(seq, 1, &body, &[]))
            }
            opcode::GET_GEOMETRY => {
                let mut body = ROOT.to_le_bytes().to_vec();
                body.extend_from_slice(&[0, 0, 0, 0, 0x13, 1, 0x74, 0]); // 0,0 275x116
                Some(reply(seq, 24, &body, &[]))
            }
            op if op == SHAPE_MAJOR && req[1] == 8 => Some(reply(seq, 0, &0u32.to_le_bytes(), &[])),
            _ => None,
        };
        if let Some(r) = r {
            send(&r);
        }
    }
}
