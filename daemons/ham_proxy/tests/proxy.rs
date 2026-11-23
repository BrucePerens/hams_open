#![allow(clippy::unwrap_used, clippy::expect_used)]
//! End to end: ham client (integrity-only) -> ham_proxy -> TLS site. Also checks the wire is readable and an
//! encrypting ham client is refused.
use ham_proxy::{ham_acceptor, ham_connector, serve_one, site_connector};
use openssl::ssl::{SslConnector, SslMethod, SslVersion};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

fn make_cert(dir: &std::path::Path) -> (PathBuf, PathBuf) {
    let (c, k) = (dir.join("c.pem"), dir.join("k.pem"));
    let ok = Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
        ])
        .args([
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost",
        ])
        .arg("-keyout")
        .arg(&k)
        .arg("-out")
        .arg(&c)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok);
    (c, k)
}

fn tmpdir(n: &str) -> PathBuf {
    // temp-dir-ignore: one small per-process directory of test certificates; /tmp is cleared on reboot
    let d = std::env::temp_dir().join(format!("ham_proxy_{n}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Site: normal TLS server answering any request with a fixed body.
fn start_site(c: &Path, k: &Path) -> String {
    let mut b =
        openssl::ssl::SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
    b.set_certificate_chain_file(c).unwrap();
    b.set_private_key_file(k, openssl::ssl::SslFiletype::PEM)
        .unwrap();
    let acc = Arc::new(b.build());
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let a = acc.clone();
            thread::spawn(move || {
                if let Ok(mut t) = a.accept(s) {
                    let mut buf = [0u8; 1024];
                    let _ = t.read(&mut buf);
                    let _ = t.write_all(b"HTTP/1.0 200 OK\r\n\r\nsite-body-xyz");
                    let _ = t.shutdown();
                }
            });
        }
    });
    addr
}

/// Start the proxy; returns its address.
fn start_proxy(c: &Path, k: &Path, site_addr: String) -> String {
    let acc = ham_acceptor(c, k).unwrap();
    let conn = site_connector(Some(c)).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let _ = serve_one(s, &acc, &conn, &site_addr, "localhost");
        }
    });
    addr
}

/// TCP tee between ham client and proxy that records the bytes both ways.
fn start_tee(target: String, log: Arc<Mutex<Vec<u8>>>, tamper: bool) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    thread::spawn(move || {
        let Ok((c, _)) = l.accept() else { return };
        let Ok(u) = TcpStream::connect(&target) else {
            return;
        };
        let pump = |mut a: TcpStream, mut b: TcpStream, log: Arc<Mutex<Vec<u8>>>, flip: bool| {
            let mut buf = [0u8; 4096];
            while let Ok(n) = a.read(&mut buf) {
                // Tamper: change one letter of the readable page body on its way to the client.
                if let Some(i) = buf[..n]
                    .windows(9)
                    .position(|w| w == b"site-body")
                    .filter(|_| flip)
                {
                    buf[i] = b'S';
                }
                if n == 0 || b.write_all(&buf[..n]).is_err() {
                    break;
                }
                log.lock().unwrap().extend_from_slice(&buf[..n]);
            }
            let _ = b.shutdown(std::net::Shutdown::Write);
        };
        let (c2, u2, l2) = (c.try_clone().unwrap(), u.try_clone().unwrap(), log.clone());
        let t = thread::spawn(move || pump(u2, c2, l2, tamper));
        pump(c, u, log, false);
        let _ = t.join();
    });
    addr
}

#[test]
fn integrity_only_request_reaches_site_and_wire_is_readable() {
    // Tests [@ANCHOR: ham_proxy_ham_acceptor]
    // Tests [@ANCHOR: ham_proxy_ham_connector]
    // Tests [@ANCHOR: ham_proxy_null_only]
    // Tests [@ANCHOR: ham_proxy_read_head]
    // Tests [@ANCHOR: ham_proxy_serve_one]
    // Tests [@ANCHOR: ham_proxy_site_connector]
    // Tests [@ANCHOR: ham_proxy_require_integrity_only]
    let d = tmpdir("ok");
    let (c, k) = make_cert(&d);
    let proxy = start_proxy(&c, &k, start_site(&c, &k));
    let log = Arc::new(Mutex::new(Vec::new()));
    let tee = start_tee(proxy, log.clone(), false);
    let cli = ham_connector(&c).unwrap();
    let mut s = cli
        .connect("localhost", TcpStream::connect(tee).unwrap())
        .unwrap();
    assert!(ham_proxy::require_integrity_only(&s).is_ok());
    s.write_all(b"GET /marker-path HTTP/1.0\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.ends_with("site-body-xyz"), "{out}");
    let wire = log.lock().unwrap().clone();
    let has = |needle: &[u8]| wire.windows(needle.len()).any(|w| w == needle);
    assert!(
        has(b"GET /marker-path"),
        "request must be readable on the ham wire"
    );
    assert!(
        has(b"site-body-xyz"),
        "response must be readable on the ham wire"
    );
}

#[test]
fn encrypting_client_is_refused() {
    let d = tmpdir("enc");
    let (c, k) = make_cert(&d);
    let proxy = start_proxy(&c, &k, "127.0.0.1:1".to_string());
    let mut b = SslConnector::builder(SslMethod::tls_client()).unwrap();
    b.set_ca_file(&c).unwrap();
    b.set_max_proto_version(Some(SslVersion::TLS1_3)).unwrap();
    let cli = b.build();
    assert!(cli
        .connect("localhost", TcpStream::connect(proxy).unwrap())
        .is_err());
}

#[test]
fn tls12_null_sha256_also_works() {
    let d = tmpdir("t12");
    let (c, k) = make_cert(&d);
    let proxy = start_proxy(&c, &k, start_site(&c, &k));
    let mut b = SslConnector::builder(SslMethod::tls_client()).unwrap();
    b.set_ca_file(&c).unwrap();
    b.set_max_proto_version(Some(SslVersion::TLS1_2)).unwrap();
    b.set_cipher_list("NULL-SHA256:@SECLEVEL=0").unwrap();
    let mut s = b
        .build()
        .connect("localhost", TcpStream::connect(proxy).unwrap())
        .unwrap();
    assert_eq!(
        ham_proxy::require_integrity_only(&s).unwrap(),
        "NULL-SHA256"
    );
    s.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    assert!(out.ends_with("site-body-xyz"));
}

#[test]
fn binary_serves_a_request() {
    // Tests [@ANCHOR: ham_proxy_main]
    // Tests [@ANCHOR: ham_proxy_run]
    // Tests [@ANCHOR: ham_proxy_arg]
    let d = tmpdir("bin");
    let (c, k) = make_cert(&d);
    let site = start_site(&c, &k);
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ham_proxy"))
        .args([
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--site",
            &site.replace("127.0.0.1", "localhost"),
        ])
        .arg("--cert")
        .arg(&c)
        .arg("--key")
        .arg(&k)
        .arg("--ca")
        .arg(&c)
        .spawn()
        .unwrap();
    let cli = ham_connector(&c).unwrap();
    let mut tcp = None;
    for _ in 0..50 {
        if let Ok(t) = TcpStream::connect(("127.0.0.1", port)) {
            tcp = Some(t);
            break;
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut s = cli.connect("localhost", tcp.unwrap()).unwrap();
    s.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(out.ends_with("site-body-xyz"));
}

#[test]
fn tampered_page_on_the_ham_wire_is_detected() {
    // Tests [@ANCHOR: ham_proxy_serve_one]
    let d = tmpdir("tamper");
    let (c, k) = make_cert(&d);
    let proxy = start_proxy(&c, &k, start_site(&c, &k));
    let log = Arc::new(Mutex::new(Vec::new()));
    let tee = start_tee(proxy, log, true);
    let cli = ham_connector(&c).unwrap();
    let mut s = cli
        .connect("localhost", TcpStream::connect(tee).unwrap())
        .unwrap();
    s.write_all(b"GET / HTTP/1.0\r\n\r\n").unwrap();
    let mut out = String::new();
    assert!(
        s.read_to_string(&mut out).is_err(),
        "a changed byte must fail the MAC, got {out:?}"
    );
    assert!(
        !out.contains("Site-body"),
        "tampered text must not be delivered"
    );
}
