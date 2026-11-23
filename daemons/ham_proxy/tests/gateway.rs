#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Central site gateway: signed whitelist (host and paths), real-site certificate verification, integrity-only ham side.
use ham_proxy::policy::{Policy, Whitelist};
use ham_proxy::signed_list::{parse_utc, unhex, verify_list, ListError};
use ham_proxy::{
    ham_acceptor, ham_connector, serve_one_policed, site_connector, ProxyError, GATEWAY,
};
use openssl::base64::encode_block;
use openssl::pkey::{PKey, Private};
use openssl::sign::Signer;
use openssl::ssl::{SslAcceptor, SslConnector, SslMethod};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

const NOW: u64 = 1_800_000_000; // 2027-01-15
const FUTURE: &str = "2027-01-17T00:00:00Z";
const PAST: &str = "2027-01-10T00:00:00Z";

fn key() -> (PKey<Private>, Vec<u8>) {
    let k = PKey::generate_ed25519().unwrap();
    let raw = k.raw_public_key().unwrap();
    (k, raw)
}

fn sign(k: &PKey<Private>, expires: &str, entries: &str) -> String {
    let payload = format!(r#"{{"entries":{entries},"expires":"{expires}","version":1}}"#);
    let sig = Signer::new_without_digest(k)
        .unwrap()
        .sign_oneshot_to_vec(payload.as_bytes())
        .unwrap();
    format!(
        r#"{{"payload":"{}","signature":"{}"}}"#,
        encode_block(payload.as_bytes()),
        encode_block(&sig)
    )
}

#[test]
fn signed_list_verifies_and_modified_wrong_key_or_old_lists_are_refused() {
    // Tests [@ANCHOR: ham_proxy_verify_list]
    // Tests [@ANCHOR: ham_proxy_list_error]
    // Tests [@ANCHOR: ham_proxy_whitelist_add_site]
    // Tests [@ANCHOR: ham_proxy_whitelist_permits_path]
    let (k, raw) = key();
    let good = sign(
        &k,
        FUTURE,
        r#"[{"host":"www.example.org","contact":"a@b.c"},{"host":"news.example.net","paths":["/ham","/pub/"]}]"#,
    );
    let w = verify_list(&good, &raw, NOW).unwrap();
    assert!(w.permits_path("WWW.example.org.", "/anything"));
    assert!(w.permits_path("news.example.net", "/ham"));
    assert!(w.permits_path("news.example.net", "/ham/x?q=1"));
    assert!(w.permits_path("news.example.net", "/pub/a"));
    assert!(!w.permits_path("news.example.net", "/hamburger"));
    assert!(!w.permits_path("news.example.net", "/"));
    assert!(!w.permits_path("other.example.org", "/"));
    assert!(w.permits_path("anything.ham", "/"));
    // modified payload (an entry added) keeps the old signature: refused
    let doc: serde_json::Value = serde_json::from_str(&good).unwrap();
    let forged = format!(
        r#"{{"entries":[{{"host":"evil.example.com"}}],"expires":"{FUTURE}","version":1}}"#
    );
    let tampered = format!(
        r#"{{"payload":"{}","signature":"{}"}}"#,
        encode_block(forged.as_bytes()),
        doc["signature"].as_str().unwrap()
    );
    assert_eq!(
        verify_list(&tampered, &raw, NOW).err(),
        Some(ListError::BadSignature)
    );
    assert_eq!(
        verify_list(&good, &key().1, NOW).err(),
        Some(ListError::BadSignature)
    );
    // an older list: correctly signed but expired
    let old = sign(&k, PAST, "[]");
    assert_eq!(verify_list(&old, &raw, NOW).err(), Some(ListError::Expired));
    assert_eq!(
        verify_list("not json", &raw, NOW).err(),
        Some(ListError::Malformed)
    );
    assert_eq!(
        verify_list(&sign(&k, FUTURE, r#"[{"paths":[]}]"#), &raw, NOW).err(),
        Some(ListError::Malformed)
    );
}

#[test]
fn utc_times_parse_and_odd_ones_do_not() {
    // Tests [@ANCHOR: ham_proxy_parse_utc]
    // Tests [@ANCHOR: ham_proxy_list_unhex]
    assert_eq!(parse_utc("2023-11-14T22:13:20Z"), Some(1_700_000_000));
    assert_eq!(parse_utc("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_utc("2024-02-29T00:00:00Z"), Some(1_709_164_800));
    assert_eq!(parse_utc("2023-13-14T22:13:20Z"), None);
    assert_eq!(parse_utc("2023-11-14 22:13:20"), None);
    assert_eq!(unhex("00ff"), Some(vec![0, 255]));
    assert_eq!(unhex("0"), None);
    assert_eq!(unhex("zz"), None);
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
    let d = std::env::temp_dir().join(format!("ham_proxy_gw_{n}_{}", std::process::id()));
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

/// One request through the gateway. `list` is the whitelist the gateway holds, `trust_site` whether it trusts the site's certificate
/// (the site's is self-signed). Returns (proxy result, ham reply, log text, upstream was contacted).
fn gateway_flow(
    n: &str,
    list: Whitelist,
    host: &str,
    path: &str,
    trust_site: bool,
) -> (Result<(), ProxyError>, String, String, bool) {
    let (c, k) = cert(n);
    let mut b = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
    b.set_certificate_chain_file(&c).unwrap();
    b.set_private_key_file(&k, openssl::ssl::SslFiletype::PEM)
        .unwrap();
    let site_acc = Arc::new(b.build());
    let sl = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = sl.local_addr().unwrap().port();
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
    let policy = Policy::new(list, "w1aw", Box::new(Sink(buf.clone()))).unwrap();
    let acc = ham_acceptor(&c, &k).unwrap();
    let conn: SslConnector = site_connector(trust_site.then_some(c.as_path())).unwrap();
    let pl = TcpListener::bind("127.0.0.1:0").unwrap();
    let paddr = pl.local_addr().unwrap();
    let t = thread::spawn(move || {
        let (s, _) = pl.accept().unwrap();
        serve_one_policed(s, &acc, &conn, GATEWAY, "", None, None, Some(&policy))
    });
    let cli = ham_connector(&c).unwrap();
    let mut s = cli
        .connect("localhost", TcpStream::connect(paddr).unwrap())
        .unwrap();
    let _ = s.write_all(format!("GET {path} HTTP/1.0\r\nHost: {host}:{port}\r\n\r\n").as_bytes());
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let res = t.join().unwrap();
    thread::sleep(std::time::Duration::from_millis(100));
    let log = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    let hit = *hit.lock().unwrap();
    (res, out, log, hit)
}

fn list_for(entries: &str) -> Whitelist {
    let (k, raw) = key();
    verify_list(&sign(&k, FUTURE, entries), &raw, NOW).unwrap()
}

#[test]
fn whitelisted_site_passes_and_is_logged() {
    // Tests [@ANCHOR: ham_proxy_serve_one_policed]
    // Tests [@ANCHOR: ham_proxy_request_target]
    // Tests [@ANCHOR: ham_proxy_policy_permits]
    let (res, out, log, _) = gateway_flow(
        "ok",
        list_for(r#"[{"host":"localhost","paths":["/ham"]}]"#),
        "localhost",
        "/ham/page",
        true,
    );
    res.unwrap();
    assert!(out.ends_with("site-body-xyz"), "{out}");
    assert!(log.contains("W1AW -> localhost"), "{log}");
}

#[test]
fn site_or_path_not_on_the_signed_list_is_refused_before_upstream() {
    let wl = r#"[{"host":"localhost","paths":["/ham"]}]"#;
    let (res, out, log, hit) = gateway_flow("bad_path", list_for(wl), "localhost", "/other", true);
    assert!(matches!(res, Err(ProxyError::NotWhitelisted(_))));
    assert!(!out.contains("site-body") && log.is_empty() && !hit);
    let (res, _, log, hit) = gateway_flow(
        "bad_host",
        list_for(r#"[{"host":"example.org"}]"#),
        "localhost",
        "/",
        true,
    );
    assert!(matches!(res, Err(ProxyError::NotWhitelisted(_))));
    assert!(log.is_empty() && !hit);
}

#[test]
fn unsigned_or_expired_list_leaves_nothing_permitted() {
    // what the binary builds when the list file fails verification: the empty whitelist
    let (k, raw) = key();
    let expired = sign(&k, PAST, r#"[{"host":"localhost"}]"#);
    let w = verify_list(&expired, &raw, NOW).unwrap_or_default();
    let (res, _, _, hit) = gateway_flow("expired", w, "localhost", "/", true);
    assert!(matches!(res, Err(ProxyError::NotWhitelisted(_))) && !hit);
}

#[test]
fn site_with_an_invalid_certificate_is_refused() {
    // the gateway does not trust the site's self-signed certificate
    let (res, out, _, hit) = gateway_flow(
        "selfsigned",
        list_for(r#"[{"host":"localhost"}]"#),
        "localhost",
        "/",
        false,
    );
    assert!(matches!(res, Err(ProxyError::Tls(_))), "{res:?}");
    assert!(hit, "the connection was tried");
    assert!(!out.contains("site-body"));
}

#[test]
fn gateway_without_a_whitelist_permits_nothing_and_encrypting_ham_clients_are_refused() {
    let (c, k) = cert("enc");
    let acc = ham_acceptor(&c, &k).unwrap();
    let conn = site_connector(Some(&c)).unwrap();
    let pl = TcpListener::bind("127.0.0.1:0").unwrap();
    let paddr = pl.local_addr().unwrap();
    let t = thread::spawn(move || {
        let (s, _) = pl.accept().unwrap();
        serve_one_policed(s, &acc, &conn, GATEWAY, "", None, None, None)
    });
    // an ordinary client (offers encrypting suites) cannot even finish the handshake
    let mut b = SslConnector::builder(SslMethod::tls()).unwrap();
    b.set_ca_file(&c).unwrap();
    let cli = b.build();
    assert!(cli
        .connect("localhost", TcpStream::connect(paddr).unwrap())
        .is_err());
    assert!(t.join().unwrap().is_err());
}

#[test]
fn policy_replace_swaps_the_whitelist() {
    // Tests [@ANCHOR: ham_proxy_policy_replace]
    let p = Policy::new(
        list_for(r#"[{"host":"a.example.org"}]"#),
        "w1aw",
        Box::new(Sink(Arc::default())),
    )
    .unwrap();
    assert!(p.permits("a.example.org", "/"));
    p.replace(list_for(r#"[{"host":"b.example.org"}]"#));
    assert!(!p.permits("a.example.org", "/") && p.permits("b.example.org", "/"));
}

fn binary_get(port: u16, c: &std::path::Path, host: &str, site_port: u16) -> String {
    let cli = ham_connector(c).unwrap();
    let mut tcp = None;
    for _ in 0..50 {
        if let Ok(t) = TcpStream::connect(("127.0.0.1", port)) {
            tcp = Some(t);
            break;
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut s = cli.connect("localhost", tcp.unwrap()).unwrap();
    let _ = s.write_all(format!("GET /x HTTP/1.0\r\nHost: {host}:{site_port}\r\n\r\n").as_bytes());
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn binary_gateway_reads_the_signed_file_each_connection() {
    // Tests [@ANCHOR: ham_proxy_load_key]
    // Tests [@ANCHOR: ham_proxy_load_list]
    let (c, k) = cert("bin");
    let d = c.parent().unwrap().to_path_buf();
    let mut b = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
    b.set_certificate_chain_file(&c).unwrap();
    b.set_private_key_file(&k, openssl::ssl::SslFiletype::PEM)
        .unwrap();
    let site_acc = Arc::new(b.build());
    let sl = TcpListener::bind("127.0.0.1:0").unwrap();
    let site_port = sl.local_addr().unwrap().port();
    thread::spawn(move || {
        for s in sl.incoming().flatten() {
            if let Ok(mut t) = site_acc.accept(s) {
                let mut buf = [0u8; 1024];
                let _ = t.read(&mut buf);
                let _ = t.write_all(b"HTTP/1.0 200 OK\r\n\r\nsite-body-xyz");
            }
        }
    });
    let (sk, raw) = key();
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::write(d.join("key.hex"), format!("{hex}\n")).unwrap();
    let list = d.join("whitelist.json");
    std::fs::write(
        &list,
        sign(&sk, "2099-01-01T00:00:00Z", r#"[{"host":"localhost"}]"#),
    )
    .unwrap();
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ham_proxy"))
        .args([
            "--gateway",
            "--listen",
            &format!("127.0.0.1:{port}"),
            "--callsign",
            "w1aw",
        ])
        .arg("--cert")
        .arg(&c)
        .arg("--key")
        .arg(&k)
        .arg("--ca")
        .arg(&c)
        .arg("--signed-whitelist")
        .arg(&list)
        .arg("--whitelist-key")
        .arg(d.join("key.hex"))
        .arg("--log")
        .arg(d.join("id.log"))
        .spawn()
        .unwrap();
    let good = binary_get(port, &c, "localhost", site_port);
    // the file is replaced by one signed with another key: the next connection gets nothing
    std::fs::write(
        &list,
        sign(
            &key().0,
            "2099-01-01T00:00:00Z",
            r#"[{"host":"localhost"}]"#,
        ),
    )
    .unwrap();
    let bad = binary_get(port, &c, "localhost", site_port);
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(good.ends_with("site-body-xyz"), "{good}");
    assert!(!bad.contains("site-body"), "{bad}");
    assert!(std::fs::read_to_string(d.join("id.log"))
        .unwrap()
        .contains("W1AW -> localhost"));
}
