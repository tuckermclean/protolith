//! `.Xauthority` lookup end to end: the cookie is found through
//! `$XAUTHORITY` by host name and display number, exactly as `xauth` files
//! it. This is its own test binary because it sets an environment
//! variable, which is process-global.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use protolith::x11::display::{encode_xauthority, family, hostname, AuthEntry};
use protolith::x11::{Connection, WindowOptions};

#[test]
fn cookie_is_found_through_xauthority() {
    let dir = std::env::temp_dir().join(format!("protolith-xauth-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let server_auth = dir.join("server");
    let client_auth = dir.join("client");
    let cookie: Vec<u8> = (0..16u8).map(|i| i.wrapping_mul(73) ^ 0x5a).collect();
    let entry = |family, address: Vec<u8>, number: &str, data: Vec<u8>| AuthEntry {
        family,
        address,
        number: number.as_bytes().to_vec(),
        name: b"MIT-MAGIC-COOKIE-1".to_vec(),
        data,
    };
    std::fs::write(&server_auth, encode_xauthority(&[entry(family::WILD, vec![], "", cookie.clone())])).unwrap();

    let spawned =
        Command::new("Xvfb").args(["-displayfd", "1", "-nolisten", "tcp", "-auth"]).arg(&server_auth).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut xvfb = match spawned {
        Ok(c) => c,
        Err(e) if std::env::var_os("PROTOLITH_REQUIRE_XVFB").is_none() => return eprintln!("skipping: Xvfb not available ({e})"),
        Err(e) => panic!("Xvfb is required: {e}"),
    };
    let mut line = String::new();
    BufReader::new(xvfb.stdout.take().unwrap()).read_line(&mut line).unwrap();
    let display: u32 = line.trim().parse().unwrap();

    // A decoy for another display first, then ours, filed the way xauth
    // and ssh -X file them: FamilyLocal, this host's name, display number.
    let entries = [
        entry(family::LOCAL, hostname().into_bytes(), &(display + 1).to_string(), vec![0; 16]),
        entry(family::LOCAL, hostname().into_bytes(), &display.to_string(), cookie),
    ];
    std::fs::write(&client_auth, encode_xauthority(&entries)).unwrap();
    std::env::set_var("XAUTHORITY", &client_auth);

    let result = Connection::open_display(&format!(":{display}"), None).and_then(|mut c| c.create_window(WindowOptions::new(4, 4)).map(drop));
    let _ = xvfb.kill();
    let _ = xvfb.wait();
    let _ = std::fs::remove_dir_all(&dir);
    result.expect("connected with the cookie from $XAUTHORITY");
}
