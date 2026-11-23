#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The `ham_proxy` binary with `--sign-key`: an HTML page comes back with a verifiable `X-Ham-Integrity` header.
use ham_proxy::ham_connector;
use ham_proxy::indicator::{header, verify_header, HEADER};
use openssl::pkey::PKey;
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

struct Killed(Child);
impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn openssl(args: &[&str]) {
    assert!(Command::new("openssl")
        .args(args)
        .output()
        .unwrap()
        .status
        .success());
}

#[test]
fn sign_key_adds_a_verifiable_integrity_header() {
    // Tests [@ANCHOR: ham_proxy_run]
    // Tests [@ANCHOR: ham_proxy_arg]
    let d = tempfile::tempdir().unwrap();
    let p = |n: &str| d.path().join(n).to_str().unwrap().to_string();
    let (cert, key, sign, public) = (p("c.pem"), p("k.pem"), p("s.pem"), p("p.pem"));
    openssl(&[
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
        "subjectAltName=DNS:localhost",
        "-keyout",
        &key,
        "-out",
        &cert,
    ]);
    openssl(&["genpkey", "-algorithm", "ed25519", "-out", &sign]);
    openssl(&["pkey", "-in", &sign, "-pubout", "-out", &public]);
    // the site: verified TLS, one HTML page
    let mut b = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
    b.set_certificate_chain_file(&cert).unwrap();
    b.set_private_key_file(&key, SslFiletype::PEM).unwrap();
    let acceptor = Arc::new(b.build());
    let site = TcpListener::bind("127.0.0.1:0").unwrap();
    let site_port = site.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in site.incoming().flatten() {
            let a = acceptor.clone();
            thread::spawn(move || {
                if let Ok(mut t) = a.accept(s) {
                    let mut buf = [0u8; 1024];
                    let _ = t.read(&mut buf);
                    let page = b"<html><body>hello</body></html>";
                    let head = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", page.len());
                    let _ = t.write_all(head.as_bytes());
                    let _ = t.write_all(page);
                    let _ = t.shutdown();
                }
            });
        }
    });
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let _proxy = Killed(
        Command::new(env!("CARGO_BIN_EXE_ham_proxy"))
            .args([
                "--listen",
                &format!("127.0.0.1:{port}"),
                "--cert",
                &cert,
                "--key",
                &key,
                "--ca",
                &cert,
                "--site",
                &format!("localhost:{site_port}"),
                "--sign-key",
                &sign,
            ])
            .spawn()
            .unwrap(),
    );
    let cli = ham_connector(std::path::Path::new(&cert)).unwrap();
    let tcp = (0..50)
        .find_map(|_| {
            TcpStream::connect(("127.0.0.1", port)).ok().or_else(|| {
                thread::sleep(Duration::from_millis(100));
                None
            })
        })
        .unwrap();
    let mut s = cli.connect("localhost", tcp).unwrap();
    s.write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    let value = header(head, HEADER).expect("no X-Ham-Integrity header");
    let public = PKey::public_key_from_pem(&std::fs::read(&public).unwrap()).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    verify_header(value, &public, "localhost", body.as_bytes(), now, 300).unwrap();
}
