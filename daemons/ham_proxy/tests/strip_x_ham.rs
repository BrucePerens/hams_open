#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Client-supplied `X-Ham-*` headers never reach the site, with or without `--verify` login.
use ham_proxy::{ham_acceptor, ham_connector, serve_one, site_connector};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

/// Send `req` through a `serve_one` proxy; returns what the site received (empty when it was never contacted).
fn through_proxy(req: &[u8]) -> String {
    let d = tempfile::tempdir().unwrap();
    let (c, k) = (d.path().join("c.pem"), d.path().join("k.pem"));
    assert!(Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "2",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost"
        ])
        .arg("-keyout")
        .arg(&k)
        .arg("-out")
        .arg(&c)
        .output()
        .unwrap()
        .status
        .success());
    let seen = Arc::new(Mutex::new(String::new()));
    let mut b =
        openssl::ssl::SslAcceptor::mozilla_intermediate_v5(openssl::ssl::SslMethod::tls_server())
            .unwrap();
    b.set_certificate_chain_file(&c).unwrap();
    b.set_private_key_file(&k, openssl::ssl::SslFiletype::PEM)
        .unwrap();
    let site_acc = b.build();
    let sl = TcpListener::bind("127.0.0.1:0").unwrap();
    sl.set_nonblocking(false).unwrap();
    let site = sl.local_addr().unwrap().to_string();
    let s2 = seen.clone();
    thread::spawn(move || {
        let Ok((s, _)) = sl.accept() else { return };
        let Ok(mut t) = site_acc.accept(s) else {
            return;
        };
        let mut buf = [0u8; 4096];
        let n = t.read(&mut buf).unwrap_or(0);
        *s2.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).to_string();
        let _ = t.write_all(b"HTTP/1.0 200 OK\r\n\r\nok");
        let _ = t.shutdown();
    });
    ham_proxy::resolve::allow_private_upstream();
    let acc = ham_acceptor(&c, &k).unwrap();
    let conn = site_connector(Some(&c)).unwrap();
    let pl = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = pl.local_addr().unwrap().to_string();
    thread::spawn(move || {
        let (s, _) = pl.accept().unwrap();
        let _ = serve_one(s, &acc, &conn, &site, "localhost");
    });
    let mut cli = ham_connector(&c)
        .unwrap()
        .connect("localhost", TcpStream::connect(proxy).unwrap())
        .unwrap();
    cli.write_all(req).unwrap();
    let mut out = String::new();
    let _ = cli.read_to_string(&mut out);
    let got = seen.lock().unwrap().to_lowercase();
    drop(cli);
    got
}

#[test]
// Tests [@ANCHOR: ham_proxy_serve_one_policed]
// Tests [@ANCHOR: ham_proxy_session_head]
fn forged_x_ham_headers_are_dropped_even_without_login() {
    let got = through_proxy(b"GET / HTTP/1.1\r\nHost: localhost\r\nX-Ham-Callsign: K1FAKE\r\nx-ham-integrity: forged\r\nAccept: */*\r\n\r\n");
    assert!(got.starts_with("get /"), "{got:?}");
    assert!(!got.contains("x-ham") && !got.contains("k1fake"), "{got}");
    assert!(got.contains("accept: */*"), "{got}");
}

#[test]
// Tests [@ANCHOR: ham_proxy_read_head]
fn a_bare_line_feed_cannot_smuggle_an_x_ham_header_past_the_filter() {
    for req in [
        &b"GET / HTTP/1.1\r\nHost: localhost\r\nAccept: */*\nX-Ham-Callsign: K1FAKE\r\n\r\n"[..],
        b"GET / HTTP/1.1\r\nHost: localhost\r\nAccept: */*\rX-Ham-Callsign: K1FAKE\r\n\r\n",
    ] {
        let got = through_proxy(req);
        assert!(!got.contains("k1fake"), "{got:?}");
    }
}
