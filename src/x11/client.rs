//! [`Client`]: one connection to an X server — the socket, the setup
//! information, sequence-number bookkeeping, and the demultiplexing of the
//! single byte stream into replies, errors and events.
//!
//! The X protocol is asynchronous. Requests carry no id; the server numbers
//! them implicitly (1, 2, 3, ... per connection) and stamps every reply,
//! error and event with the low 16 bits of the last request it processed.
//! `Client` counts requests in 64 bits, widens each 16-bit stamp against
//! the last one seen, and uses the result to hand each reply or error to
//! the request that caused it. Errors for requests that expect no reply are
//! queued in order with events ([`Message::Error`]), so nothing the server
//! says is dropped.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use super::display::{Auth, DisplayName, Transport};
use super::proto::event::{packet_len, parse_event, Event};
use super::proto::setup::{encode_setup_request, parse_setup_reply, setup_reply_body_len, IdAllocator, Setup};
use super::proto::{request, ReadLe, XError, Xid};
use crate::error::{Error, Result};

/// The socket: Unix-domain for local displays, TCP for `host:N`.
#[derive(Debug)]
enum Stream {
    Unix(UnixStream),
    Tcp(TcpStream),
}

impl Stream {
    fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        match self {
            Stream::Unix(s) => s.set_nonblocking(on),
            Stream::Tcp(s) => s.set_nonblocking(on),
        }
    }

    fn set_read_timeout(&self, t: Option<Duration>) -> io::Result<()> {
        match self {
            Stream::Unix(s) => s.set_read_timeout(t),
            Stream::Tcp(s) => s.set_read_timeout(t),
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Unix(s) => s.read(buf),
            Stream::Tcp(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Unix(s) => s.write(buf),
            Stream::Tcp(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl AsFd for Stream {
    fn as_fd(&self) -> BorrowedFd<'_> {
        match self {
            Stream::Unix(s) => s.as_fd(),
            Stream::Tcp(s) => s.as_fd(),
        }
    }
}

/// Something the server said that was not a reply to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message {
    /// An event.
    Event(Event),
    /// An error for a request that expected no reply.
    Error(XError),
}

/// A request that expects a reply; redeem with [`Client::wait_reply`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[must_use = "a reply must be collected with Client::wait_reply"]
pub struct Cookie(u64);

impl Cookie {
    /// The request's sequence number.
    pub fn sequence(&self) -> u64 {
        self.0
    }
}

/// How long a read may block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    Forever,
    Never,
    For(Duration),
}

/// Buffered output is flushed once it grows past this.
const OUT_BUF_CAP: usize = 64 * 1024;
/// Insert a round trip before the 16-bit sequence counter could wrap
/// without the server having stamped anything in between.
const SYNC_INTERVAL: u64 = 0xf000;
/// Refuse replies larger than this (a server-side bug or hostile peer
/// would otherwise make the client buffer without bound).
const MAX_REPLY: usize = 256 * 1024 * 1024;

/// A connection to an X server.
#[derive(Debug)]
pub struct Client {
    stream: Stream,
    setup: Setup,
    /// `None` for a client built over a caller-supplied socket.
    display: Option<DisplayName>,
    screen: usize,
    auth: Auth,
    ids: IdAllocator,
    out: Vec<u8>,
    rbuf: Vec<u8>,
    rpos: usize,
    /// Sequence number of the last request sent.
    sent: u64,
    /// Highest sequence number the server has stamped on anything.
    seen: u64,
    /// Requests whose reply (or error) has not arrived yet.
    awaiting: BTreeSet<u64>,
    /// Replies (or errors) that arrived for requests nobody has collected.
    results: BTreeMap<u64, std::result::Result<Vec<u8>, XError>>,
    messages: VecDeque<Message>,
    nonblocking: bool,
    timeout: Option<Duration>,
    /// Set once the connection has failed; nothing more is attempted on drop.
    broken: bool,
}

impl Client {
    /// Connect to `$DISPLAY` with credentials from `$XAUTHORITY`.
    pub fn connect_env() -> Result<Client> {
        Client::connect(&DisplayName::from_env()?, None)
    }

    /// Connect to `display`. With `auth: None`, credentials are looked up
    /// in `$XAUTHORITY`/`~/.Xauthority` (falling back to none).
    pub fn connect(display: &DisplayName, auth: Option<Auth>) -> Result<Client> {
        match &display.transport {
            Transport::Unix => {
                let path = display.socket_path();
                let stream = UnixStream::connect(&path).map_err(|e| Error::Io(io::Error::new(e.kind(), format!("{}: {e}", path.display()))))?;
                let auth = auth.unwrap_or_else(|| Auth::load(display, &[]));
                Client::handshake(Stream::Unix(stream), Some(display.clone()), auth)
            }
            Transport::Tcp(host) => {
                let port =
                    u16::try_from(display.tcp_port()).map_err(|_| Error::InvalidArgument(format!("display number {} has no TCP port", display.display)))?;
                let addrs: Vec<_> = (host.as_str(), port).to_socket_addrs()?.collect();
                let mut last = io::Error::new(io::ErrorKind::NotFound, format!("{host}: no addresses"));
                for addr in &addrs {
                    match TcpStream::connect_timeout(addr, Duration::from_secs(10)) {
                        Ok(stream) => {
                            stream.set_nodelay(true)?;
                            let ips: Vec<_> = addrs.iter().map(|a| a.ip()).collect();
                            let auth = auth.unwrap_or_else(|| Auth::load(display, &ips));
                            return Client::handshake(Stream::Tcp(stream), Some(display.clone()), auth);
                        }
                        Err(e) => last = e,
                    }
                }
                Err(Error::Io(io::Error::new(last.kind(), format!("{host}:{port}: {last}"))))
            }
        }
    }

    /// Run connection setup over an already-connected Unix socket (for
    /// custom transports and for testing against an in-process server).
    /// Such a client uses screen 0 and has no [`display_name`](Client::display_name).
    pub fn from_unix_stream(stream: UnixStream, auth: &Auth) -> Result<Client> {
        Client::handshake(Stream::Unix(stream), None, auth.clone())
    }

    fn handshake(mut stream: Stream, display: Option<DisplayName>, auth: Auth) -> Result<Client> {
        stream.write_all(&encode_setup_request(&auth.name, &auth.data))?;
        let mut head = [0u8; 8];
        read_exact(&mut stream, &mut head)?;
        let body_len = setup_reply_body_len(&head).unwrap_or(0);
        // A Failed reply's reason length is in byte 1, not the length field.
        let mut full = head.to_vec();
        full.resize(8 + body_len, 0);
        read_exact(&mut stream, &mut full[8..])?;
        let setup = parse_setup_reply(&full)?;
        if setup.screens.is_empty() {
            return Err(Error::Protocol("server reported no screens".into()));
        }
        let screen = display.as_ref().map_or(0, |d| usize::from(d.screen));
        if screen >= setup.screens.len() {
            return Err(Error::InvalidArgument(format!("screen {screen} does not exist ({} screens)", setup.screens.len())));
        }
        let ids = IdAllocator::new(setup.resource_id_base, setup.resource_id_mask);
        Ok(Client {
            stream,
            setup,
            display,
            screen,
            auth,
            ids,
            out: Vec::with_capacity(4096),
            rbuf: Vec::with_capacity(4096),
            rpos: 0,
            sent: 0,
            seen: 0,
            awaiting: BTreeSet::new(),
            results: BTreeMap::new(),
            messages: VecDeque::new(),
            nonblocking: false,
            timeout: None,
            broken: false,
        })
    }

    /// The server's setup information.
    pub fn setup(&self) -> &Setup {
        &self.setup
    }

    /// The display this client connected to (`None` when built with
    /// [`from_unix_stream`](Client::from_unix_stream)).
    pub fn display_name(&self) -> Option<&DisplayName> {
        self.display.as_ref()
    }

    /// The credentials this client connected with (for opening a second
    /// connection to the same server).
    pub(crate) fn auth(&self) -> &Auth {
        &self.auth
    }

    /// The screen named by the display (`:0.1` → screen 1).
    pub fn screen(&self) -> &super::proto::setup::Screen {
        // Checked in range at handshake.
        &self.setup.screens[self.screen]
    }

    /// Allocate a resource id.
    pub fn generate_id(&mut self) -> Result<Xid> {
        self.ids.allocate().ok_or(Error::OutOfIds)
    }

    /// Return a freed resource's id for reuse.
    pub fn release_id(&mut self, id: Xid) {
        self.ids.release(id);
    }

    /// Largest request the server accepts, in bytes.
    pub fn max_request_bytes(&self) -> usize {
        usize::from(self.setup.maximum_request_length) * 4
    }

    /// Queue a request that has no reply; returns its sequence number.
    /// Requests are buffered: they reach the server on the next
    /// [`flush`](Client::flush), or before any wait for a reply or event.
    pub fn send(&mut self, req: &[u8]) -> Result<u64> {
        self.enqueue(req, false)
    }

    /// Queue a request that has a reply.
    pub fn send_with_reply(&mut self, req: &[u8]) -> Result<Cookie> {
        self.enqueue(req, true).map(Cookie)
    }

    fn enqueue(&mut self, req: &[u8], reply: bool) -> Result<u64> {
        let words = req.u16_at(2).unwrap_or(0);
        if req.len() < 4 || req.len() % 4 != 0 || usize::from(words) * 4 != req.len() {
            let max = self.max_request_bytes();
            return Err(if req.len() > max {
                Error::RequestTooLarge { bytes: req.len(), max }
            } else {
                Error::InvalidArgument("malformed request (length field)".into())
            });
        }
        if req.len() > self.max_request_bytes() {
            return Err(Error::RequestTooLarge { bytes: req.len(), max: self.max_request_bytes() });
        }
        if self.sent - self.seen >= SYNC_INTERVAL {
            self.sync()?;
        }
        if self.out.len() + req.len() > OUT_BUF_CAP {
            self.flush()?;
        }
        if req.len() > OUT_BUF_CAP {
            self.set_blocking()?;
            self.stream.write_all(req)?;
        } else {
            self.out.extend_from_slice(req);
        }
        self.sent += 1;
        if reply {
            self.awaiting.insert(self.sent);
        }
        Ok(self.sent)
    }

    /// Write buffered requests to the server.
    pub fn flush(&mut self) -> Result<()> {
        if self.out.is_empty() {
            return Ok(());
        }
        self.set_blocking()?;
        let r = self.stream.write_all(&self.out);
        self.out.clear();
        r.map_err(|e| {
            self.broken = true;
            if matches!(e.kind(), io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset) {
                Error::Disconnected
            } else {
                Error::Io(e)
            }
        })
    }

    /// Block until the reply to `cookie` arrives. An X error in place of
    /// the reply is returned as [`Error::X`].
    pub fn wait_reply(&mut self, cookie: Cookie) -> Result<Vec<u8>> {
        self.flush()?;
        loop {
            if let Some(r) = self.results.remove(&cookie.0) {
                return r.map_err(Error::X);
            }
            if !self.awaiting.contains(&cookie.0) {
                return Err(Error::InvalidArgument(format!("no reply pending for sequence {}", cookie.0)));
            }
            self.fill(Wait::Forever)?;
        }
    }

    /// Send a request and wait for its reply.
    pub fn round_trip(&mut self, req: &[u8]) -> Result<Vec<u8>> {
        let c = self.send_with_reply(req)?;
        self.wait_reply(c)
    }

    /// Round-trip to the server: when this returns, every earlier request
    /// has been processed and any error it caused is queued.
    pub fn sync(&mut self) -> Result<()> {
        // Inline the send so `enqueue` cannot recurse into `sync`.
        let req = request::get_input_focus();
        if self.out.len() + req.len() > OUT_BUF_CAP {
            self.flush()?;
        }
        self.out.extend_from_slice(&req);
        self.sent += 1;
        self.awaiting.insert(self.sent);
        self.wait_reply(Cookie(self.sent)).map(drop)
    }

    /// Sync, then return the first error caused by a request with sequence
    /// number ≥ `since`, removing it from the message queue.
    pub fn check_since(&mut self, since: u64) -> Result<()> {
        self.sync()?;
        let pos = self.messages.iter().position(|m| matches!(m, Message::Error(e) if e.sequence >= since));
        match pos.and_then(|i| self.messages.remove(i)) {
            Some(Message::Error(e)) => Err(Error::X(e)),
            _ => Ok(()),
        }
    }

    /// Sequence number of the last request sent.
    pub fn last_sequence(&self) -> u64 {
        self.sent
    }

    /// The next queued event or error, reading whatever the socket already
    /// holds but never blocking.
    pub fn poll_message(&mut self) -> Result<Option<Message>> {
        if let Some(m) = self.messages.pop_front() {
            return Ok(Some(m));
        }
        self.flush()?;
        while self.fill(Wait::Never)? {
            if let Some(m) = self.messages.pop_front() {
                return Ok(Some(m));
            }
        }
        Ok(None)
    }

    /// The next event or error, blocking up to `timeout` (`None` = forever).
    /// `Ok(None)` on timeout.
    pub fn wait_message(&mut self, timeout: Option<Duration>) -> Result<Option<Message>> {
        if let Some(m) = self.messages.pop_front() {
            return Ok(Some(m));
        }
        self.flush()?;
        let deadline = timeout.map(|t| std::time::Instant::now() + t);
        loop {
            let wait = match deadline {
                None => Wait::Forever,
                Some(d) => match d.checked_duration_since(std::time::Instant::now()) {
                    Some(left) if !left.is_zero() => Wait::For(left),
                    _ => Wait::Never,
                },
            };
            let got = self.fill(wait)?;
            if let Some(m) = self.messages.pop_front() {
                return Ok(Some(m));
            }
            if !got && wait != Wait::Forever {
                return Ok(None);
            }
        }
    }

    /// Messages already read and queued (no I/O).
    pub fn queued_messages(&self) -> usize {
        self.messages.len()
    }

    /// Put a message back at the front of the queue.
    pub(crate) fn unread(&mut self, m: Message) {
        self.messages.push_front(m);
    }

    // ------------------------------------------------------------ reading

    fn set_blocking(&mut self) -> Result<()> {
        if self.nonblocking {
            self.stream.set_nonblocking(false)?;
            self.nonblocking = false;
        }
        Ok(())
    }

    /// Read once from the socket (as `wait` allows) and process every whole
    /// packet. Returns whether any bytes arrived.
    fn fill(&mut self, wait: Wait) -> Result<bool> {
        let r = self.fill_inner(wait);
        if r.is_err() {
            self.broken = true;
        }
        r
    }

    fn fill_inner(&mut self, wait: Wait) -> Result<bool> {
        match wait {
            Wait::Never => {
                if !self.nonblocking {
                    self.stream.set_nonblocking(true)?;
                    self.nonblocking = true;
                }
            }
            Wait::Forever | Wait::For(_) => {
                self.set_blocking()?;
                let t = match wait {
                    Wait::For(d) => Some(d),
                    _ => None,
                };
                if self.timeout != t {
                    self.stream.set_read_timeout(t)?;
                    self.timeout = t;
                }
            }
        }
        let start = self.rbuf.len();
        self.rbuf.resize(start + 16 * 1024, 0);
        let r = loop {
            match self.stream.read(&mut self.rbuf[start..]) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                other => break other,
            }
        };
        match r {
            Ok(0) => {
                self.rbuf.truncate(start);
                Err(Error::Disconnected)
            }
            Ok(n) => {
                self.rbuf.truncate(start + n);
                self.process()?;
                Ok(true)
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                self.rbuf.truncate(start);
                Ok(false)
            }
            Err(e) => {
                self.rbuf.truncate(start);
                Err(if e.kind() == io::ErrorKind::ConnectionReset { Error::Disconnected } else { Error::Io(e) })
            }
        }
    }

    /// Widen a 16-bit wire sequence number to the full count, relative to
    /// the last one seen (the server's stamps never go backwards and never
    /// pass the last request sent).
    fn widen(&mut self, s16: u16) -> u64 {
        let mut s = (self.seen & !0xffff) | u64::from(s16);
        if s < self.seen {
            s += 0x1_0000;
        }
        if s > self.sent && s >= 0x1_0000 {
            s -= 0x1_0000;
        }
        self.seen = self.seen.max(s);
        s
    }

    /// Dispatch every complete packet in the read buffer.
    fn process(&mut self) -> Result<()> {
        loop {
            let avail = &self.rbuf[self.rpos..];
            let Some(len) = packet_len(avail) else { break };
            if len > MAX_REPLY {
                return Err(Error::Protocol(format!("server announced a {len}-byte packet")));
            }
            if avail.len() < len {
                break;
            }
            let kind = avail[0];
            if kind == 1 {
                let packet = self.rbuf[self.rpos..self.rpos + len].to_vec();
                let wire = packet.u16_at(2).unwrap_or(0);
                let seq = self.widen(wire);
                if self.awaiting.remove(&seq) {
                    self.results.insert(seq, Ok(packet));
                }
                // A reply nobody asked for is dropped: only a broken server
                // sends one, and the byte stream stays aligned.
                self.rpos += len;
                continue;
            }
            // Errors and core events are exactly 32 bytes; a GenericEvent's
            // trailing body is skipped (protolith selects none).
            let mut packet = [0u8; 32];
            packet.copy_from_slice(&avail[..32]);
            let packet = &packet[..];
            match kind {
                0 => {
                    let mut e = XError::parse(packet).ok_or_else(|| Error::Protocol("short error packet".into()))?;
                    let wire = e.sequence as u16;
                    e.sequence = self.widen(wire);
                    if self.awaiting.remove(&e.sequence) {
                        self.results.insert(e.sequence, Err(e));
                    } else {
                        self.messages.push_back(Message::Error(e));
                    }
                }
                _ => {
                    if let Some(ev) = parse_event(packet) {
                        if let Some(s) = ev.sequence {
                            self.widen(s);
                        }
                        self.messages.push_back(Message::Event(ev));
                    }
                }
            }
            self.rpos += len;
        }
        if self.rpos == self.rbuf.len() {
            self.rbuf.clear();
            self.rpos = 0;
        } else if self.rpos > 64 * 1024 {
            self.rbuf.drain(..self.rpos);
            self.rpos = 0;
        }
        Ok(())
    }
}

impl Drop for Client {
    /// Flush, then wait (at most a second) for the server to process every
    /// request. An X server that sees a client hang up discards requests
    /// still unread in its socket buffer, so without this round trip the
    /// last few requests before exit — a title change, a wake-up sent from
    /// a thread — can silently vanish. (Xlib's `XCloseDisplay` does the
    /// same.)
    fn drop(&mut self) {
        if self.broken || self.flush().is_err() || self.sent == self.seen {
            return;
        }
        let req = request::get_input_focus();
        if self.stream.write_all(&req).is_err() {
            return;
        }
        self.sent += 1;
        self.awaiting.insert(self.sent);
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        while !self.results.contains_key(&self.sent) {
            match deadline.checked_duration_since(std::time::Instant::now()) {
                Some(left) if !left.is_zero() => {
                    if !matches!(self.fill(Wait::For(left)), Ok(true)) {
                        return;
                    }
                }
                _ => return,
            }
        }
    }
}

impl AsFd for Client {
    /// The socket, for multiplexing with `poll(2)` alongside other fds.
    /// Readiness means bytes are available; call
    /// [`poll_message`](Client::poll_message) to consume them. Note that
    /// messages may already be queued without the fd being readable.
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }
}

impl AsRawFd for Client {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_fd().as_raw_fd()
    }
}

fn read_exact(stream: &mut Stream, buf: &mut [u8]) -> Result<()> {
    stream.read_exact(buf).map_err(|e| match e.kind() {
        io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionReset => Error::Disconnected,
        _ => Error::Io(e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x11::fake::{FakeServer, Script};
    use crate::x11::proto::event::RawEvent;

    fn expose(seq: u16) -> Vec<u8> {
        let mut e = vec![0u8; 32];
        e[0] = 12;
        e[2..4].copy_from_slice(&seq.to_le_bytes());
        e
    }

    #[test]
    fn handshake_parses_setup() {
        let (client, server) = FakeServer::start(Script::default());
        let c = client.unwrap();
        assert_eq!(c.screen().root_depth, 24);
        assert_eq!(c.setup().vendor, "protolith test");
        drop(c);
        server.finish();
    }

    #[test]
    fn handshake_sends_credentials() {
        let (client, server) = FakeServer::start_with_auth(Script::default(), Auth::cookie(&[7; 16]));
        drop(client.unwrap());
        let log = server.finish();
        assert_eq!(log.auth_name, b"MIT-MAGIC-COOKIE-1");
        assert_eq!(log.auth_data, [7; 16]);
    }

    #[test]
    fn refused_setup_is_a_setup_error() {
        let (client, server) = FakeServer::start(Script { refuse: Some("Authorization required".into()), ..Default::default() });
        match client {
            Err(Error::Setup(e)) => assert!(e.to_string().contains("Authorization required"), "{e}"),
            other => panic!("{other:?}"),
        }
        server.finish();
    }

    #[test]
    fn events_queued_before_a_reply_are_kept_in_order() {
        // The original Stele hang: events sitting ahead of a reply.
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        server.push_raw(expose(0));
        server.push_raw(expose(0));
        let r = c.round_trip(&request::get_input_focus()).unwrap();
        assert_eq!(r[0], 1);
        assert!(matches!(c.poll_message().unwrap(), Some(Message::Event(e)) if matches!(e.event, RawEvent::Expose { .. })));
        assert!(matches!(c.poll_message().unwrap(), Some(Message::Event(_))));
        assert_eq!(c.poll_message().unwrap(), None);
        server.finish();
    }

    #[test]
    fn error_for_a_reply_request_goes_to_its_waiter() {
        let (client, server) = FakeServer::start(Script { fail_opcodes: vec![request::opcode::INTERN_ATOM], ..Default::default() });
        let mut c = client.unwrap();
        match c.round_trip(&request::intern_atom("X", false)) {
            Err(Error::X(e)) => {
                assert_eq!(e.major_opcode, request::opcode::INTERN_ATOM);
                assert_eq!(e.sequence, 1);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(c.poll_message().unwrap(), None, "not also queued as async");
        server.finish();
    }

    #[test]
    fn error_for_a_void_request_is_queued_and_found_by_check() {
        let (client, server) = FakeServer::start(Script { fail_opcodes: vec![request::opcode::MAP_WINDOW], ..Default::default() });
        let mut c = client.unwrap();
        let first = c.send(&request::no_operation()).unwrap();
        c.send(&request::map_window(5)).unwrap();
        c.send(&request::no_operation()).unwrap();
        match c.check_since(first) {
            Err(Error::X(e)) => assert_eq!((e.major_opcode, e.sequence), (request::opcode::MAP_WINDOW, 2)),
            other => panic!("{other:?}"),
        }
        assert!(c.check_since(first).is_ok(), "the error was consumed");
        server.finish();
    }

    #[test]
    fn check_since_ignores_older_errors() {
        let (client, server) = FakeServer::start(Script { fail_opcodes: vec![request::opcode::MAP_WINDOW], ..Default::default() });
        let mut c = client.unwrap();
        c.send(&request::map_window(5)).unwrap();
        c.sync().unwrap();
        let later = c.send(&request::no_operation()).unwrap();
        assert!(c.check_since(later).is_ok());
        assert!(matches!(c.poll_message().unwrap(), Some(Message::Error(_))), "older error still queued");
        server.finish();
    }

    #[test]
    fn pipelined_replies_are_matched_by_sequence() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        let a = c.send_with_reply(&request::intern_atom("A", false)).unwrap();
        let b = c.send_with_reply(&request::intern_atom("B", false)).unwrap();
        // Collect in reverse order.
        let rb = c.wait_reply(b).unwrap();
        let ra = c.wait_reply(a).unwrap();
        let atom = |r: &[u8]| request::parse_intern_atom(r).unwrap();
        assert_ne!(atom(&ra), atom(&rb));
        assert_eq!(atom(&ra), server.atom("A"));
        assert!(c.wait_reply(a).is_err(), "a cookie is redeemed once");
        server.finish();
    }

    #[test]
    fn sequence_numbers_survive_16_bit_wraparound() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        // 70,000 void requests: forces automatic syncs and wraps the wire
        // counter; then a reply must still match.
        for _ in 0..70_000 {
            c.send(&request::no_operation()).unwrap();
        }
        let cookie = c.send_with_reply(&request::get_input_focus()).unwrap();
        assert!(cookie.sequence() > 70_000);
        c.wait_reply(cookie).unwrap();
        assert_eq!(c.seen, c.sent);
        let log = server.finish();
        assert!(log.requests.iter().filter(|r| r[0] == request::opcode::GET_INPUT_FOCUS).count() >= 2, "auto-sync happened");
    }

    #[test]
    fn widen_handles_wrap_relative_to_last_seen() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        c.sent = 0x2_0005;
        c.seen = 0x1_fff0;
        assert_eq!(c.widen(0xfff8), 0x1_fff8);
        assert_eq!(c.widen(0x0002), 0x2_0002, "wrapped");
        assert_eq!(c.seen, 0x2_0002);
        c.broken = true; // the counters are fiction; skip the drop-time sync
        drop(c);
        server.finish();
    }

    #[test]
    fn oversized_and_malformed_requests_are_refused_locally() {
        let (client, server) = FakeServer::start(Script { max_request_words: 16, ..Default::default() });
        let mut c = client.unwrap();
        let big = request::change_property(request::PropMode::Replace, 1, 2, 3, request::PropData::U8(&[0; 100]));
        assert!(matches!(c.send(&big), Err(Error::RequestTooLarge { max: 64, .. })));
        assert!(matches!(c.send(&[1, 2, 3]), Err(Error::InvalidArgument(_))));
        assert!(matches!(c.send(&[1, 0, 9, 0]), Err(Error::InvalidArgument(_))));
        assert_eq!(c.last_sequence(), 0, "nothing was counted");
        server.finish();
    }

    #[test]
    fn wait_with_timeout_returns_none() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        let t = std::time::Instant::now();
        assert_eq!(c.wait_message(Some(Duration::from_millis(30))).unwrap(), None);
        assert!(t.elapsed() >= Duration::from_millis(25));
        server.push_raw(expose(0));
        assert!(c.wait_message(Some(Duration::from_secs(5))).unwrap().is_some());
        server.finish();
    }

    #[test]
    fn dropping_a_client_syncs_so_queued_requests_are_not_lost() {
        // An X server that sees a client hang up discards requests still
        // in its socket buffer; like XCloseDisplay, drop must round-trip.
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        c.send(&request::map_window(5)).unwrap();
        drop(c);
        let log = server.finish();
        let ops: Vec<u8> = log.requests.iter().map(|r| r[0]).collect();
        assert_eq!(ops, [request::opcode::MAP_WINDOW, request::opcode::GET_INPUT_FOCUS]);
    }

    #[test]
    fn dropping_an_idle_client_sends_nothing() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        c.round_trip(&request::get_input_focus()).unwrap();
        drop(c);
        assert_eq!(server.finish().requests.len(), 1);
    }

    #[test]
    fn dropping_a_client_of_a_dead_server_does_not_hang() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        c.send(&request::map_window(5)).unwrap();
        server.finish();
        let t = std::time::Instant::now();
        drop(c);
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn disconnect_is_reported() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        server.finish();
        assert!(matches!(c.wait_message(None), Err(Error::Disconnected)));
    }

    #[test]
    fn events_split_across_reads_are_reassembled() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        let e = expose(0);
        server.push_raw(e[..5].to_vec());
        assert_eq!(c.wait_message(Some(Duration::from_millis(50))).unwrap(), None, "partial packet is held");
        server.push_raw(e[5..].to_vec());
        assert!(c.wait_message(Some(Duration::from_secs(5))).unwrap().is_some());
        server.finish();
    }

    #[test]
    fn a_hostile_byte_stream_never_panics() {
        for seed in 1..=40u64 {
            let (client, server) = FakeServer::start(Script::default());
            let mut c = client.unwrap();
            let mut s = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            let mut junk = vec![0u8; 4096];
            for b in junk.iter_mut() {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                *b = s as u8;
            }
            server.push_raw(junk);
            // Errors are fine; panics and hangs are not.
            for _ in 0..200 {
                match c.wait_message(Some(Duration::from_millis(20))) {
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => break,
                }
            }
            server.finish();
        }
    }

    #[test]
    fn absurd_reply_length_is_a_protocol_error() {
        let (client, server) = FakeServer::start(Script::default());
        let mut c = client.unwrap();
        let mut r = vec![0u8; 32];
        r[0] = 1;
        r[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        server.push_raw(r);
        assert!(matches!(c.wait_message(Some(Duration::from_secs(5))), Err(Error::Protocol(_))));
        server.finish();
    }
}
