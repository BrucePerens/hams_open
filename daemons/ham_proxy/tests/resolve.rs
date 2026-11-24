#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The AREDN-side gateway: `.ham` names looked up at the Hams.com Net resolver, never a private address, whitelist enforced.
use ham_proxy::resolve::{aaaa_query, address_allowed, parse_aaaa};
use ham_proxy::{ham_connector, GATEWAY};
use openssl::ssl::{SslAcceptor, SslMethod};
use std::io::{Read, Write};
use std::net::{IpAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::thread;

#[test]
fn private_loopback_and_mapped_addresses_are_refused() {
    // Tests [@ANCHOR: ham_proxy_address_allowed]
    for a in [
        "127.0.0.1",
        "10.1.2.3",
        "192.168.1.1",
        "172.16.0.9",
        "169.254.1.1",
        "0.0.0.0",
        "224.0.0.1",
        "100.64.0.1",
        "::1",
        "::",
        "fe80::1",
        "fd00::5",
        "ff02::1",
        "::ffff:192.168.1.1",
    ] {
        assert!(!address_allowed(&a.parse::<IpAddr>().unwrap()), "{a}");
    }
    for a in ["8.8.8.8", "2001:db8:ab01:1::10", "2600::1"] {
        assert!(address_allowed(&a.parse::<IpAddr>().unwrap()), "{a}");
    }
    assert_eq!(GATEWAY, "*");
}

#[test]
fn aaaa_query_and_response_round_trip() {
    // Tests [@ANCHOR: ham_proxy_aaaa_query]
    // Tests [@ANCHOR: ham_proxy_parse_aaaa]
    // Tests [@ANCHOR: ham_proxy_resolve_skip_name]
    let q = aaaa_query("n0call.ham", 7);
    assert_eq!(&q[..2], &[0, 7]);
    assert_eq!(&q[q.len() - 4..], &[0, 28, 0, 1]);
    assert_eq!(&q[12..19], b"\x06n0call");
    let mut r = q.clone();
    r[2] = 0x81;
    r[3] = 0x80;
    r[7] = 1; // one answer
    r.extend_from_slice(&[0xC0, 12, 0, 28, 0, 1, 0, 0, 0, 60, 0, 16]);
    r.extend_from_slice(
        &"2001:db8::10"
            .parse::<std::net::Ipv6Addr>()
            .unwrap()
            .octets(),
    );
    assert_eq!(
        parse_aaaa(&r),
        vec!["2001:db8::10".parse::<std::net::Ipv6Addr>().unwrap()]
    );
    r[3] = 0x83; // NXDOMAIN
    assert!(parse_aaaa(&r).is_empty());
    assert!(parse_aaaa(&r[..9]).is_empty());
}

fn cert(dir: &Path) -> (PathBuf, PathBuf) {
    let (c, k) = (dir.join("c.pem"), dir.join("k.pem"));
    let ok = Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "2",
            "-subj",
            "/CN=gw.ham",
        ])
        .args(["-addext", "subjectAltName=DNS:gw.ham,DNS:localhost"])
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

/// A resolver that answers every AAAA query with `answer`.
fn resolver(answer: &'static str) -> String {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = s.local_addr().unwrap().to_string();
    thread::spawn(move || {
        let mut buf = [0u8; 512];
        while let Ok((n, from)) = s.recv_from(&mut buf) {
            let mut r = buf[..n].to_vec();
            r[2] = 0x81;
            r[3] = 0x80;
            r[7] = 1;
            r.extend_from_slice(&[0xC0, 12, 0, 28, 0, 1, 0, 0, 0, 60, 0, 16]);
            r.extend_from_slice(&answer.parse::<std::net::Ipv6Addr>().unwrap().octets());
            let _ = s.send_to(&r, from);
        }
    });
    addr
}

struct Gw(Child, u16, PathBuf, PathBuf);
impl Drop for Gw {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The gateway binary as hams-node-web starts it (IPv4 listen address), with no whitelist file: only `.ham` names are permitted.
fn gateway(dir: &Path, c: &Path, k: &Path, dns: &str, allow_private: bool) -> Gw {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    std::fs::write(dir.join("none.txt"), "# no sites: only .ham names\n").unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ham_proxy"));
    cmd.args(["--gateway", "--callsign", "w1aw", "--ham-resolver", dns]);
    if allow_private {
        cmd.arg("--allow-private-upstream");
    }
    let child = cmd
        .args(["--listen", &format!("127.0.0.1:{port}")])
        .arg("--cert")
        .arg(c)
        .arg("--key")
        .arg(k)
        .arg("--ca")
        .arg(c)
        .arg("--whitelist")
        .arg(dir.join("none.txt"))
        .arg("--log")
        .arg(dir.join("id.log"))
        .spawn()
        .unwrap();
    Gw(child, port, dir.join("id.log"), c.to_path_buf())
}

fn get(g: &Gw, host: &str) -> String {
    let mut tcp = None;
    for _ in 0..50 {
        if let Ok(t) = TcpStream::connect(("127.0.0.1", g.1)) {
            tcp = Some(t);
            break;
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut s = ham_connector(&g.3)
        .unwrap()
        .connect("localhost", tcp.unwrap())
        .unwrap();
    let _ = s.write_all(format!("GET /index.html HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes());
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

fn site(c: &Path, k: &Path) -> u16 {
    let mut b = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
    b.set_certificate_chain_file(c).unwrap();
    b.set_private_key_file(k, openssl::ssl::SslFiletype::PEM)
        .unwrap();
    let acc = Arc::new(b.build());
    let l = TcpListener::bind("[::1]:0").unwrap();
    let port = l.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            if let Ok(mut t) = acc.accept(s) {
                let mut buf = [0u8; 1024];
                let _ = t.read(&mut buf);
                let _ = t.write_all(b"HTTP/1.0 200 OK\r\n\r\nham-page-xyz");
            }
        }
    });
    port
}

#[test]
fn ipv4_client_loads_a_ham_page_integrity_only_and_other_sites_are_refused() {
    // Tests [@ANCHOR: ham_proxy_connect_upstream]
    // Tests [@ANCHOR: ham_proxy_resolve_lookup]
    // Tests [@ANCHOR: ham_proxy_set_ham_resolver]
    // Tests [@ANCHOR: ham_proxy_allow_private_upstream]
    let dir = tempfile::tempdir().unwrap();
    let (c, k) = cert(dir.path());
    let port = site(&c, &k);
    // the lab site is on ::1, so the lab flag is needed; the resolver gives the AAAA, the system resolver is not asked
    let g = gateway(dir.path(), &c, &k, &resolver("::1"), true);
    let page = get(&g, &format!("gw.ham:{port}"));
    assert!(page.ends_with("ham-page-xyz"), "{page}");
    // not on the whitelist, not .ham: refused before any upstream connection
    let other = get(&g, &format!("example.org:{port}"));
    assert!(!other.contains("ham-page"), "{other}");
    assert!(std::fs::read_to_string(&g.2)
        .unwrap()
        .contains("W1AW -> gw.ham"));
}

#[test]
fn a_ham_name_resolving_to_a_private_address_is_never_connected() {
    let dir = tempfile::tempdir().unwrap();
    let (c, k) = cert(dir.path());
    let port = site(&c, &k);
    // without the lab flag the loopback answer is refused: nothing of the operator's own network is reachable by name
    let g = gateway(dir.path(), &c, &k, &resolver("::1"), false);
    assert!(!get(&g, &format!("gw.ham:{port}")).contains("ham-page"));
    let g2 = gateway(dir.path(), &c, &k, &resolver("fd00::5"), false);
    assert!(!get(&g2, &format!("gw.ham:{port}")).contains("ham-page"));
}
