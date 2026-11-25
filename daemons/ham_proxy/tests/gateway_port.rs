#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The site gateway reaches port 443 only: a whitelisted name is not a way to open other TCP ports.
use ham_proxy::policy::{Policy, Whitelist};
use ham_proxy::{
    ham_acceptor, ham_connector, serve_one_policed, site_connector, ProxyError, GATEWAY,
};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;

#[test]
// Tests [@ANCHOR: ham_proxy_serve_one_policed]
fn gateway_refuses_a_port_other_than_443() {
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
    let acc = ham_acceptor(&c, &k).unwrap();
    let conn = site_connector(Some(&c)).unwrap();
    let policy = Policy::new(
        Whitelist::parse("localhost\n"),
        "N0CALL",
        Box::new(std::io::sink()),
    )
    .unwrap();
    let pl = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = pl.local_addr().unwrap().to_string();
    let h = thread::spawn(move || {
        let (s, _) = pl.accept().unwrap();
        serve_one_policed(s, &acc, &conn, GATEWAY, "", None, None, Some(&policy))
    });
    let mut cli = ham_connector(&c)
        .unwrap()
        .connect("localhost", TcpStream::connect(proxy).unwrap())
        .unwrap();
    cli.write_all(b"GET / HTTP/1.1\r\nHost: localhost:22\r\n\r\n")
        .unwrap();
    let r = h.join().unwrap();
    assert!(
        matches!(r, Err(ProxyError::NotWhitelisted(ref w)) if w == "localhost:22"),
        "{r:?}"
    );
}
