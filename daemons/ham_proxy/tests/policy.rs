#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Site-gateway whitelist and station-ID log in the Rust proxy (ported from ham_tls/ham_policy.py).
use ham_proxy::policy::{format_utc, log_line, Policy, Whitelist};
use ham_proxy::{ham_acceptor, ham_connector, serve_one_policed, site_connector, ProxyError};
use openssl::ssl::{SslAcceptor, SslMethod};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

#[test]
fn whitelist_permits_ham_listed_and_subdomains_only() {
    // Tests [@ANCHOR: ham_proxy_whitelist]
    // Tests [@ANCHOR: ham_proxy_whitelist_parse]
    // Tests [@ANCHOR: ham_proxy_whitelist_permits]
    let w = Whitelist::parse("# sites\nExample.org\n\n*.news.example # all\n");
    assert!(w.permits("anything.ham"));
    assert!(w.permits("EXAMPLE.org."));
    assert!(w.permits("a.news.example"));
    assert!(!w.permits("evil.example.org"));
    assert!(!w.permits("news.example"));
    assert!(!w.permits("other.com"));
}

#[test]
fn log_line_has_time_callsign_host_suite() {
    // Tests [@ANCHOR: ham_proxy_log_line]
    // Tests [@ANCHOR: ham_proxy_format_utc]
    assert_eq!(format_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    assert_eq!(
        log_line(1_700_000_000, "w1aw", "example.org", "TLS_SHA256_SHA256").unwrap(),
        "2023-11-14T22:13:20Z W1AW -> example.org TLS_SHA256_SHA256\n"
    );
    assert!(log_line(0, "  ", "h", "s").is_none());
}

struct Sink(Arc<Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn cert(n: &str) -> (PathBuf, PathBuf) {
    // temp-dir-ignore: one small per-process directory of test certificates; /tmp is cleared on reboot
    let d = std::env::temp_dir().join(format!("ham_proxy_pol_{n}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let (c, k) = (d.join("c.pem"), d.join("k.pem"));
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

/// Run one flow through the proxy for `site_name`; returns (proxy result, ham reply, log text, upstream hit).
fn flow(n: &str, white: &str, site_name: &str) -> (Result<(), ProxyError>, String, String, bool) {
    let (c, k) = cert(n);
    let mut b = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
    b.set_certificate_chain_file(&c).unwrap();
    b.set_private_key_file(&k, openssl::ssl::SslFiletype::PEM)
        .unwrap();
    let site_acc = Arc::new(b.build());
    let sl = TcpListener::bind("127.0.0.1:0").unwrap();
    let site_addr = sl.local_addr().unwrap().to_string();
    let hit = Arc::new(Mutex::new(false));
    let h2 = hit.clone();
    thread::spawn(move || {
        if let Ok((s, _)) = sl.accept() {
            *h2.lock().unwrap() = true;
            if let Ok(mut t) = site_acc.accept(s) {
                let mut buf = [0u8; 1024];
                let _ = t.read(&mut buf);
                let _ = t.write_all(b"HTTP/1.0 200 OK\r\n\r\nsite-body-xyz");
            }
        }
    });
    let buf = Arc::new(Mutex::new(Vec::new()));
    let policy = Policy::new(Whitelist::parse(white), "w1aw", Box::new(Sink(buf.clone()))).unwrap();
    let acc = ham_acceptor(&c, &k).unwrap();
    let conn = site_connector(Some(&c)).unwrap();
    let pl = TcpListener::bind("127.0.0.1:0").unwrap();
    let paddr = pl.local_addr().unwrap();
    let name = site_name.to_string();
    let t = thread::spawn(move || {
        let (s, _) = pl.accept().unwrap();
        serve_one_policed(s, &acc, &conn, &site_addr, &name, None, Some(&policy))
    });
    let cli = ham_connector(&c).unwrap();
    let mut s = cli
        .connect("localhost", TcpStream::connect(paddr).unwrap())
        .unwrap();
    let _ = s.write_all(b"GET / HTTP/1.0\r\n\r\n");
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let res = t.join().unwrap();
    thread::sleep(std::time::Duration::from_millis(100));
    let log = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    let hit = *hit.lock().unwrap();
    (res, out, log, hit)
}

#[test]
fn site_off_the_whitelist_is_refused_before_upstream_and_not_logged() {
    // Tests [@ANCHOR: ham_proxy_serve_one_policed]
    let (res, out, log, hit) = flow("deny", "example.org\n", "localhost");
    assert!(matches!(res, Err(ProxyError::NotWhitelisted(_))));
    assert!(!out.contains("site-body"));
    assert!(!hit, "upstream must not be contacted");
    assert!(log.is_empty());
}

#[test]
fn whitelisted_flow_is_served_and_logged_with_callsign_host_suite() {
    // Tests [@ANCHOR: ham_proxy_policy_new]
    // Tests [@ANCHOR: ham_proxy_policy_record]
    let (res, out, log, _) = flow("allow", "localhost\n", "localhost");
    res.unwrap();
    assert!(out.ends_with("site-body-xyz"));
    let line = log.trim_end();
    let parts: Vec<&str> = line.split(' ').collect();
    assert_eq!(parts.len(), 5, "{line}");
    assert!(parts[0].ends_with('Z') && parts[0].contains('T'));
    assert_eq!(&parts[1..4], ["W1AW", "->", "localhost"]);
    assert!(["TLS_SHA256_SHA256", "TLS_SHA384_SHA384", "NULL-SHA256"].contains(&parts[4]));
}

#[test]
fn policy_needs_a_callsign() {
    assert!(Policy::new(Whitelist::default(), " ", Box::new(Sink(Arc::default()))).is_none());
}
