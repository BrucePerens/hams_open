#![allow(clippy::unwrap_used, clippy::expect_used)]
//! DANE-EE for `.ham` servers: a stub resolver answers TLSA queries; the proxy accepts only a pinned certificate.
use ham_proxy::dane::{connect_ham_server, parse_tlsa, tlsa_matches, tlsa_query, Tlsa};
use ham_proxy::{ham_acceptor, ProxyError};
use openssl::hash::{hash, MessageDigest};
use openssl::x509::X509;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

fn make_cert(dir: &Path, name: &str) -> (PathBuf, PathBuf) {
    let (c, k) = (
        dir.join(format!("{name}.pem")),
        dir.join(format!("{name}.key")),
    );
    let out = Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "2",
        ])
        .args(["-subj", "/CN=localhost"])
        .arg("-keyout")
        .arg(&k)
        .arg("-out")
        .arg(&c)
        .output()
        .unwrap();
    assert!(out.status.success());
    (c, k)
}

fn der(cert: &Path) -> Vec<u8> {
    X509::from_pem(&std::fs::read(cert).unwrap())
        .unwrap()
        .to_der()
        .unwrap()
}

fn sha256(d: &[u8]) -> Vec<u8> {
    hash(MessageDigest::sha256(), d).unwrap().to_vec()
}

fn tmpdir(n: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("ham_proxy_dane_{n}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Stub resolver: answers one query with the given TLSA records (none gives an empty NOERROR answer).
fn start_dns(records: Vec<Tlsa>) -> String {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = s.local_addr().unwrap().to_string();
    thread::spawn(move || {
        let mut buf = [0u8; 512];
        let Ok((n, from)) = s.recv_from(&mut buf) else {
            return;
        };
        let q = &buf[..n];
        let mut r = q.to_vec();
        r[2] = 0x81;
        r[3] = 0x80;
        r[6..8].copy_from_slice(&(records.len() as u16).to_be_bytes());
        for (u, sel, m, data) in records {
            r.extend_from_slice(&[0xC0, 0x0C, 0, 52, 0, 1, 0, 0, 0, 60]);
            r.extend_from_slice(&((data.len() + 3) as u16).to_be_bytes());
            r.extend_from_slice(&[u, sel, m]);
            r.extend_from_slice(&data);
        }
        s.send_to(&r, from).unwrap();
    });
    addr
}

/// A `.ham` server (integrity-only TLS) that sends a fixed line to every client.
fn start_ham_server(c: &Path, k: &Path) -> u16 {
    let acc = ham_acceptor(c, k).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            if let Ok(mut t) = acc.accept(s) {
                let _ = t.write_all(b"hello-from-ham");
                let _ = t.shutdown();
            }
        }
    });
    port
}

#[test]
fn tlsa_matches_pins_only_the_certificate() {
    // Tests [@ANCHOR: ham_proxy_tlsa_matches]
    let d = tmpdir("m");
    let (c, _) = make_cert(&d, "a");
    let cert = der(&c);
    assert!(tlsa_matches(&cert, &[(3, 0, 1, sha256(&cert))]));
    assert!(tlsa_matches(&cert, &[(3, 0, 0, cert.clone())]));
    let s512 = hash(MessageDigest::sha512(), &cert).unwrap().to_vec();
    assert!(tlsa_matches(&cert, &[(3, 0, 2, s512)]));
    assert!(!tlsa_matches(&cert, &[]));
    assert!(!tlsa_matches(&cert, &[(3, 0, 1, sha256(b"other"))]));
    assert!(!tlsa_matches(
        &cert,
        &[(1, 0, 1, sha256(&cert)), (3, 1, 1, sha256(&cert))]
    ));
}

#[test]
fn query_and_response_round_trip() {
    // Tests [@ANCHOR: ham_proxy_tlsa_query]
    // Tests [@ANCHOR: ham_proxy_parse_tlsa]
    let q = tlsa_query("node.ham", 443, 7);
    assert_eq!(&q[..2], &[0, 7]);
    let want = b"\x04_443\x04_tcp\x04node\x03ham\x00";
    assert!(q.windows(want.len()).any(|w| w == want));
    let mut r = q.clone();
    r[2] = 0x81;
    r[3] = 0x80;
    r[7] = 1;
    r.extend_from_slice(&[0xC0, 0x0C, 0, 52, 0, 1, 0, 0, 0, 60, 0, 6, 3, 0, 1, 9, 9, 9]);
    assert_eq!(parse_tlsa(&r), vec![(3, 0, 1, vec![9, 9, 9])]);
    r[3] = 0x83; // NXDOMAIN
    assert!(parse_tlsa(&r).is_empty());
    assert!(parse_tlsa(&[1, 2, 3]).is_empty());
}

fn try_connect(port: u16, resolver: &str) -> Result<String, ProxyError> {
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut s = connect_ham_server(tcp, "localhost", port, resolver)?;
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    Ok(out)
}

#[test]
fn pinned_ham_server_is_accepted_and_others_refused() {
    // Tests [@ANCHOR: ham_proxy_resolve_tlsa]
    // Tests [@ANCHOR: ham_proxy_connect_ham_server]
    let d = tmpdir("c");
    let (c, k) = make_cert(&d, "good");
    let (other, _) = make_cert(&d, "other");
    let port = start_ham_server(&c, &k);
    let good = vec![(3, 0, 1, sha256(&der(&c)))];
    assert_eq!(
        try_connect(port, &start_dns(good)).unwrap(),
        "hello-from-ham"
    );
    let wrong = vec![(3, 0, 1, sha256(&der(&other)))];
    assert!(matches!(
        try_connect(port, &start_dns(wrong)),
        Err(ProxyError::NotPinned(_))
    ));
    assert!(matches!(
        try_connect(port, &start_dns(vec![])),
        Err(ProxyError::NotPinned(_))
    ));
}
