#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Request bodies reach the site: Content-Length and chunked, through the single-site proxy.
use ham_proxy::{ham_acceptor, ham_connector, serve_one, site_connector};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

/// Send `request` through a fresh proxy; returns what the site received.
fn through_proxy(request: &[u8]) -> String {
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
    let site = sl.local_addr().unwrap().to_string();
    let s2 = seen.clone();
    let site_thread = thread::spawn(move || {
        let (s, _) = sl.accept().unwrap();
        let mut t = site_acc.accept(s).unwrap();
        let mut buf = [0u8; 8192];
        let n = t.read(&mut buf).unwrap();
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
    cli.write_all(request).unwrap();
    let mut out = String::new();
    let _ = cli.read_to_string(&mut out);
    site_thread.join().unwrap();
    let got = seen.lock().unwrap().clone();
    got
}

#[test]
// Tests [@ANCHOR: ham_proxy_body_read]
// Tests [@ANCHOR: ham_proxy_body_with_body]
// Tests [@ANCHOR: ham_proxy_serve_one_policed]
fn a_posted_form_reaches_the_site_with_content_length_or_chunked() {
    let got = through_proxy(b"POST /f HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\nname=W1AW&x");
    assert!(
        got.starts_with("POST /f HTTP/1.1\r\n") && got.ends_with("\r\n\r\nname=W1AW&x"),
        "{got:?}"
    );
    assert_eq!(got.matches("Content-Length: 11").count(), 1, "{got:?}");
    let got = through_proxy(b"POST /f HTTP/1.1\r\nHost: localhost\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n");
    assert!(got.ends_with("\r\n\r\nhello world"), "{got:?}");
    assert!(
        got.contains("Content-Length: 11") && !got.to_lowercase().contains("transfer-encoding"),
        "{got:?}"
    );
}
