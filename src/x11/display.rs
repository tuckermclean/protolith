//! Finding the X server: `$DISPLAY` parsing and `.Xauthority` lookup.
//!
//! Both are pure functions over strings and byte buffers, tested here; the
//! environment and filesystem reads are thin wrappers in
//! [`DisplayName::from_env`] and [`Auth::load`].

use std::fmt;
use std::net::IpAddr;
use std::path::PathBuf;

/// How to reach the server.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Transport {
    /// The local Unix-domain socket `/tmp/.X11-unix/X<display>`.
    Unix,
    /// TCP to `host`, port `6000 + display` (what SSH X11 forwarding uses:
    /// `localhost:10`).
    Tcp(String),
}

/// A parsed display name such as `:0`, `:1.0`, `unix:0` or `localhost:10.0`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayName {
    /// Where the server listens.
    pub transport: Transport,
    /// Display number.
    pub display: u16,
    /// Screen number (default 0).
    pub screen: u16,
}

/// Why a display name could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayError(pub String);

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid display name {:?}", self.0)
    }
}

impl std::error::Error for DisplayError {}

impl DisplayName {
    /// Parse a display name: `[host]:display[.screen]`, with `host` empty
    /// or `unix` meaning the local socket. IPv6 hosts may be bracketed
    /// (`[::1]:0`). DECnet (`host::0`) and `protocol/host:0` forms are
    /// rejected.
    pub fn parse(name: &str) -> Result<DisplayName, DisplayError> {
        let err = || DisplayError(name.to_string());
        let (host, rest) = name.rsplit_once(':').ok_or_else(err)?;
        if host.contains(':') && !(host.starts_with('[') && host.ends_with(']')) {
            return Err(err()); // DECnet "host::0", or an unbracketed IPv6 address
        }
        if host.contains('/') {
            return Err(err());
        }
        let (display, screen) = match rest.split_once('.') {
            Some((d, s)) => (d, Some(s)),
            None => (rest, None),
        };
        let display: u16 = display.parse().map_err(|_| err())?;
        let screen: u16 = match screen {
            Some(s) => s.parse().map_err(|_| err())?,
            None => 0,
        };
        let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
        let transport = if host.is_empty() || host == "unix" { Transport::Unix } else { Transport::Tcp(host.to_string()) };
        Ok(DisplayName { transport, display, screen })
    }

    /// Parse `$DISPLAY`, defaulting to `:0` when unset.
    pub fn from_env() -> Result<DisplayName, DisplayError> {
        DisplayName::parse(&std::env::var("DISPLAY").unwrap_or_else(|_| ":0".to_string()))
    }

    /// The Unix socket path for this display.
    pub fn socket_path(&self) -> PathBuf {
        PathBuf::from(format!("/tmp/.X11-unix/X{}", self.display))
    }

    /// The TCP port for this display.
    pub fn tcp_port(&self) -> u32 {
        6000 + u32::from(self.display)
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.transport {
            Transport::Unix => write!(f, ":{}.{}", self.display, self.screen),
            Transport::Tcp(h) if h.contains(':') => write!(f, "[{h}]:{}.{}", self.display, self.screen),
            Transport::Tcp(h) => write!(f, "{h}:{}.{}", self.display, self.screen),
        }
    }
}

// ================================================================ Xauthority

/// `.Xauthority` address families (`Xauth.h`).
pub mod family {
    /// IPv4 address (4 bytes).
    pub const INTERNET: u16 = 0;
    /// IPv6 address (16 bytes).
    pub const INTERNET6: u16 = 6;
    /// Local connection; the address is the host name.
    pub const LOCAL: u16 = 256;
    /// Matches any address.
    pub const WILD: u16 = 65535;
}

/// One `.Xauthority` entry.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthEntry {
    /// Address family.
    pub family: u16,
    /// Address bytes (a host name for `LOCAL`, raw IP bytes otherwise).
    pub address: Vec<u8>,
    /// Display number as ASCII digits; empty matches any display.
    pub number: Vec<u8>,
    /// Protocol name, e.g. `MIT-MAGIC-COOKIE-1`.
    pub name: Vec<u8>,
    /// Protocol data (the cookie).
    pub data: Vec<u8>,
}

impl fmt::Debug for AuthEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never print the cookie.
        f.debug_struct("AuthEntry")
            .field("family", &self.family)
            .field("address", &String::from_utf8_lossy(&self.address))
            .field("number", &String::from_utf8_lossy(&self.number))
            .field("name", &String::from_utf8_lossy(&self.name))
            .field("data", &format_args!("<{} bytes>", self.data.len()))
            .finish()
    }
}

/// Parse a whole `.Xauthority` file: repeated entries of a big-endian `u16`
/// family followed by four big-endian-`u16`-length-prefixed fields. Stops
/// at the first truncated entry, returning those before it.
pub fn parse_xauthority(data: &[u8]) -> Vec<AuthEntry> {
    fn field(data: &[u8], off: &mut usize) -> Option<Vec<u8>> {
        let len = data.get(*off..*off + 2)?;
        let len = usize::from(u16::from_be_bytes([len[0], len[1]]));
        let v = data.get(*off + 2..*off + 2 + len)?.to_vec();
        *off += 2 + len;
        Some(v)
    }
    let mut out = Vec::new();
    let mut off = 0;
    while off < data.len() {
        let entry = (|| {
            let fam = data.get(off..off + 2)?;
            let family = u16::from_be_bytes([fam[0], fam[1]]);
            let mut o = off + 2;
            let address = field(data, &mut o)?;
            let number = field(data, &mut o)?;
            let name = field(data, &mut o)?;
            let data_ = field(data, &mut o)?;
            off = o;
            Some(AuthEntry { family, address, number, name, data: data_ })
        })();
        match entry {
            Some(e) => out.push(e),
            None => break,
        }
    }
    out
}

/// Encode entries in `.Xauthority` format (the inverse of
/// [`parse_xauthority`]; handy for tests and tools).
pub fn encode_xauthority(entries: &[AuthEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    for e in entries {
        out.extend_from_slice(&e.family.to_be_bytes());
        for f in [&e.address, &e.number, &e.name, &e.data] {
            out.extend_from_slice(&(f.len().min(0xffff) as u16).to_be_bytes());
            out.extend_from_slice(&f[..f.len().min(0xffff)]);
        }
    }
    out
}

/// The authorization protocols this client can speak.
pub const MIT_MAGIC_COOKIE_1: &[u8] = b"MIT-MAGIC-COOKIE-1";

/// The `(family, address)` pairs an entry may be filed under for a
/// connection to `display`, best first — the same candidates Xlib tries.
///
/// A local socket, or TCP to the loopback or to this very host, is filed as
/// `LOCAL` with the host name (that is how `xauth` and SSH X11 forwarding
/// record it: `hostname/unix:10`). TCP to anything else is filed by IP.
pub fn auth_addresses(display: &DisplayName, hostname: &str, resolved: &[IpAddr]) -> Vec<(u16, Vec<u8>)> {
    let mut out = Vec::new();
    let local = match &display.transport {
        Transport::Unix => true,
        Transport::Tcp(h) => h == "localhost" || h == hostname || resolved.iter().any(|ip| ip.is_loopback()),
    };
    if local {
        out.push((family::LOCAL, hostname.as_bytes().to_vec()));
    }
    for ip in resolved {
        match ip {
            IpAddr::V4(v4) => out.push((family::INTERNET, v4.octets().to_vec())),
            IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
                Some(v4) => out.push((family::INTERNET, v4.octets().to_vec())),
                None => out.push((family::INTERNET6, v6.octets().to_vec())),
            },
        }
    }
    out
}

/// Pick the cookie for a display out of parsed `.Xauthority` entries.
///
/// An entry matches when its family/address is one of `addresses` (or its
/// family is `WILD`), its display number equals `display` (or is empty),
/// and its protocol is one this client speaks. Among matches the earliest
/// candidate address wins, then file order — Xlib's rule.
pub fn select_auth<'a>(entries: &'a [AuthEntry], addresses: &[(u16, Vec<u8>)], display: u16) -> Option<&'a AuthEntry> {
    let number = display.to_string();
    let usable = |e: &&AuthEntry| e.name == MIT_MAGIC_COOKIE_1 && (e.number.is_empty() || e.number == number.as_bytes());
    for (fam, addr) in addresses {
        if let Some(e) = entries.iter().filter(usable).find(|e| e.family == *fam && e.address == *addr) {
            return Some(e);
        }
    }
    entries.iter().filter(usable).find(|e| e.family == family::WILD)
}

/// Credentials for the connection-setup request.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Auth {
    /// Protocol name (empty for none).
    pub name: Vec<u8>,
    /// Protocol data.
    pub data: Vec<u8>,
}

impl fmt::Debug for Auth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Auth").field("name", &String::from_utf8_lossy(&self.name)).field("data", &format_args!("<{} bytes>", self.data.len())).finish()
    }
}

impl Auth {
    /// No authorization (for servers run with `-ac` or `xhost +`).
    pub fn none() -> Auth {
        Auth::default()
    }

    /// A `MIT-MAGIC-COOKIE-1` cookie.
    pub fn cookie(cookie: &[u8]) -> Auth {
        Auth { name: MIT_MAGIC_COOKIE_1.to_vec(), data: cookie.to_vec() }
    }

    /// Look up the credentials for `display` in `$XAUTHORITY` (or
    /// `~/.Xauthority`). Falls back to no authorization when there is no
    /// file or no matching entry; the server then decides.
    pub fn load(display: &DisplayName, resolved: &[IpAddr]) -> Auth {
        let Some(path) = std::env::var_os("XAUTHORITY").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".Xauthority")))
        else {
            return Auth::none();
        };
        let Ok(bytes) = std::fs::read(path) else {
            return Auth::none();
        };
        let entries = parse_xauthority(&bytes);
        let addresses = auth_addresses(display, &hostname(), resolved);
        select_auth(&entries, &addresses, display.display).map(|e| Auth { name: e.name.clone(), data: e.data.clone() }).unwrap_or_default()
    }
}

/// This machine's host name, as `xauth` records it.
pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname").or_else(|_| std::fs::read_to_string("/etc/hostname")).map(|s| s.trim().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    fn tcp(h: &str, d: u16, s: u16) -> DisplayName {
        DisplayName { transport: Transport::Tcp(h.into()), display: d, screen: s }
    }

    #[test]
    fn parses_local_forms() {
        let unix = |d, s| DisplayName { transport: Transport::Unix, display: d, screen: s };
        assert_eq!(DisplayName::parse(":0"), Ok(unix(0, 0)));
        assert_eq!(DisplayName::parse(":1.2"), Ok(unix(1, 2)));
        assert_eq!(DisplayName::parse("unix:0.0"), Ok(unix(0, 0)));
    }

    #[test]
    fn parses_tcp_forms() {
        assert_eq!(DisplayName::parse("localhost:11.0"), Ok(tcp("localhost", 11, 0)));
        assert_eq!(DisplayName::parse("10.0.0.2:0"), Ok(tcp("10.0.0.2", 0, 0)));
        assert_eq!(DisplayName::parse("[::1]:10"), Ok(tcp("::1", 10, 0)));
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "nocolon", ":", ":abc", ":0.x", "host::0", "tcp/host:0", ":70000", "::1:0"] {
            assert!(DisplayName::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn display_round_trips_through_to_string() {
        for name in [":0.0", "localhost:11.0", "[::1]:10.0"] {
            assert_eq!(DisplayName::parse(name).unwrap().to_string(), name);
        }
    }

    #[test]
    fn socket_and_port() {
        let d = DisplayName::parse("localhost:11").unwrap();
        assert_eq!(d.tcp_port(), 6011);
        assert_eq!(DisplayName::parse(":3").unwrap().socket_path(), PathBuf::from("/tmp/.X11-unix/X3"));
    }

    fn entry(family: u16, address: &[u8], number: &str, cookie: u8) -> AuthEntry {
        AuthEntry { family, address: address.to_vec(), number: number.as_bytes().to_vec(), name: MIT_MAGIC_COOKIE_1.to_vec(), data: vec![cookie; 16] }
    }

    #[test]
    fn xauthority_round_trips() {
        let entries = vec![entry(family::LOCAL, b"box", "0", 1), entry(family::INTERNET, &[127, 0, 0, 1], "10", 2)];
        assert_eq!(parse_xauthority(&encode_xauthority(&entries)), entries);
    }

    #[test]
    fn xauthority_truncation_keeps_complete_entries() {
        let entries = vec![entry(family::LOCAL, b"box", "0", 1), entry(family::LOCAL, b"box", "1", 2)];
        let bytes = encode_xauthority(&entries);
        let first_len = encode_xauthority(&entries[..1]).len();
        for cut in 0..bytes.len() {
            let got = parse_xauthority(&bytes[..cut]);
            assert_eq!(got.len(), usize::from(cut >= first_len), "cut {cut}");
        }
    }

    #[test]
    fn ssh_forwarding_cookie_is_found_over_tcp_localhost() {
        // `ssh -X` writes "hostname/unix:11" and sets DISPLAY=localhost:11.0.
        let entries = vec![entry(family::LOCAL, b"vps", "10", 1), entry(family::LOCAL, b"vps", "11", 2)];
        let d = DisplayName::parse("localhost:11.0").unwrap();
        let addrs = auth_addresses(&d, "vps", &[IpAddr::V4(Ipv4Addr::LOCALHOST)]);
        assert_eq!(select_auth(&entries, &addrs, d.display).map(|e| e.data[0]), Some(2));
    }

    #[test]
    fn local_socket_matches_by_hostname_and_number() {
        let entries = vec![entry(family::LOCAL, b"other", "0", 9), entry(family::LOCAL, b"me", "1", 1), entry(family::LOCAL, b"me", "0", 2)];
        let d = DisplayName::parse(":0").unwrap();
        let addrs = auth_addresses(&d, "me", &[]);
        assert_eq!(select_auth(&entries, &addrs, 0).map(|e| e.data[0]), Some(2));
    }

    #[test]
    fn remote_tcp_matches_by_ip_and_not_hostname() {
        let entries = vec![entry(family::LOCAL, b"me", "0", 1), entry(family::INTERNET, &[10, 0, 0, 2], "0", 2)];
        let d = DisplayName::parse("10.0.0.2:0").unwrap();
        let addrs = auth_addresses(&d, "me", &[IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2))]);
        assert_eq!(addrs.len(), 1, "not local: no LOCAL candidate");
        assert_eq!(select_auth(&entries, &addrs, 0).map(|e| e.data[0]), Some(2));
    }

    #[test]
    fn ipv6_and_mapped_addresses() {
        let d = DisplayName::parse("[::1]:0").unwrap();
        let v6 = auth_addresses(&d, "me", &[IpAddr::V6(Ipv6Addr::LOCALHOST)]);
        assert_eq!(v6[0].0, family::LOCAL, "::1 is loopback");
        assert_eq!(v6[1], (family::INTERNET6, Ipv6Addr::LOCALHOST.octets().to_vec()));
        let mapped = auth_addresses(&tcp("x", 0, 0), "me", &[IpAddr::V6(Ipv4Addr::new(10, 1, 2, 3).to_ipv6_mapped())]);
        assert_eq!(mapped, [(family::INTERNET, vec![10, 1, 2, 3])]);
    }

    #[test]
    fn wildcard_and_empty_number_entries() {
        let wild = vec![entry(family::WILD, b"", "0", 7)];
        let addrs = auth_addresses(&DisplayName::parse(":0").unwrap(), "me", &[]);
        assert_eq!(select_auth(&wild, &addrs, 0).map(|e| e.data[0]), Some(7));
        let any_display = vec![entry(family::LOCAL, b"me", "", 8)];
        assert_eq!(select_auth(&any_display, &addrs, 42).map(|e| e.data[0]), Some(8));
        assert!(select_auth(&wild, &addrs, 1).is_none(), "wrong display number");
    }

    #[test]
    fn unsupported_protocols_are_skipped() {
        let mut xdm = entry(family::LOCAL, b"me", "0", 1);
        xdm.name = b"XDM-AUTHORIZATION-1".to_vec();
        let entries = vec![xdm, entry(family::LOCAL, b"me", "0", 2)];
        let addrs = auth_addresses(&DisplayName::parse(":0").unwrap(), "me", &[]);
        assert_eq!(select_auth(&entries, &addrs, 0).map(|e| e.data[0]), Some(2));
    }

    #[test]
    fn debug_never_prints_the_cookie() {
        let e = entry(family::LOCAL, b"me", "0", 0xAB);
        let s = format!("{e:?} {:?}", Auth::cookie(&[0xAB; 16]));
        assert!(!s.contains("171") && !s.to_lowercase().contains("ab, "), "{s}");
        assert!(s.contains("<16 bytes>"));
    }
}
