// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-990S LAN session (`kenwood_ts990_lan`) against a scripted virtual radio: nothing is
//! connected to a radio.
#![allow(clippy::expect_used, clippy::panic)]

use radioctl_core::net::Framing;
use radioctl_protocols::kenwood_ts990_lan::{
    lan_connect_request, lan_login, utf16, Ts990LanFraming,
};

fn wire(text: &str) -> Vec<u8> {
    utf16(text.as_bytes())
}

// Tests [@ANCHOR: radioctl_ts990_lan_login]
// Tests [@ANCHOR: radioctl_ts990_lan_connect_request]
// Tests [@ANCHOR: radioctl_ts990_lan_utf16]
#[test]
fn the_login_frame_is_the_references_example() {
    // PDF p. 62: account "kenwood" (7) and password "admin" (5) are sent as `##ID75kenwoodadmin`.
    assert_eq!(
        lan_login("kenwood", "admin").expect("fits"),
        b"##ID75kenwoodadmin;"
    );
    assert_eq!(lan_connect_request(), b"##CN;");
    for (a, p) in [
        ("", "x"),
        ("x", ""),
        ("123456789", "x"),
        ("x", "123456789"),
        ("a;", "x"),
    ] {
        assert!(lan_login(a, p).is_err(), "{a:?} {p:?}");
    }
    assert_eq!(utf16(b"ID;"), [b'I', 0, b'D', 0, b';', 0]);
}

// Tests [@ANCHOR: radioctl_ts990lan_greeting]
// Tests [@ANCHOR: radioctl_ts990lan_decode]
// Tests [@ANCHOR: radioctl_ts990lan_encode]
// Tests [@ANCHOR: radioctl_ts990lan_keepalive]
// Tests [@ANCHOR: radioctl_ts990lan_keepalive_sent]
// Tests [@ANCHOR: radioctl_ts990lan_needs_handshake]
// Tests [@ANCHOR: radioctl_ts990lan_reset]
// Tests [@ANCHOR: radioctl_ts990lan_new]
// Tests [@ANCHOR: radioctl_ts990lan_handle_frame]
#[test]
fn the_session_connects_logs_in_then_passes_cat_through() {
    let mut f = Ts990LanFraming::new("kenwood", "admin").expect("credentials");
    assert!(f.needs_handshake());
    assert_eq!(f.greeting(), wire("##CN;"));
    // The radio answers in two pieces, split inside a UTF-16 unit; nothing happens until it is whole.
    let answer = wire("##CN1;");
    let part = f.decode(&answer[..5]);
    assert!(part.reply.is_empty() && !part.ready && part.failure.is_none());
    let d = f.decode(&answer[5..]);
    assert_eq!(d.reply, wire("##ID75kenwoodadmin;"));
    assert!(!d.ready);
    let d = f.decode(&wire("##ID1;"));
    assert!(d.ready && d.failure.is_none());
    // Now the plain CAT stream, as ASCII for the engine, and the engine's bytes go out as UTF-16.
    assert_eq!(f.decode(&wire("FA00014195000;")).payload, b"FA00014195000;");
    assert_eq!(f.encode(b"FA;"), wire("FA;"));
    // The keep-alive is `ID;`; its answer is swallowed, an unsolicited ID answer is not.
    let (every, probe) = f.keepalive().expect("keep-alive");
    assert!(every.as_secs() < 10);
    assert_eq!(probe, wire("ID;"));
    f.keepalive_sent();
    assert!(f.decode(&wire("ID022;")).payload.is_empty());
    assert_eq!(f.decode(&wire("ID022;")).payload, b"ID022;");
    // A reset starts the handshake again.
    f.reset();
    assert!(f.decode(&wire("FA00014195000;")).payload.is_empty());
}

#[test]
fn a_denied_connection_or_a_refused_login_fails_the_handshake() {
    let mut f = Ts990LanFraming::new("kenwood", "admin").expect("credentials");
    f.greeting();
    assert!(f.decode(&wire("##CN0;")).failure.is_some());
    let mut f = Ts990LanFraming::new("kenwood", "admin").expect("credentials");
    f.greeting();
    f.decode(&wire("##CN1;"));
    assert!(f.decode(&wire("##ID0;")).failure.is_some());
}
