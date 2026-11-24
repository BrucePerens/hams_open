#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The LAN-facing listener: CONNECT terminated with leaf certificates from the node CA, plain http:// upgraded, policy before any upstream
//! connection, and the upstream leg only ever integrity-only TLS.
use ham_proxy::lan::{parse_target, serve_lan, Ca, Target, Upstream};
use ham_proxy::policy::{Policy, Whitelist};
use ham_proxy::{ham_acceptor, ProxyError};
use openssl::ssl::{SslConnector, SslMethod};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

fn run(args: &[&str]) {
    let out = Command::new("openssl").args(args).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A name-constrained node CA like the one `hams-node-proxyca` makes: `.ham` and example.org only.
fn node_ca(dir: &Path) -> (PathBuf, PathBuf) {
    let (c, k) = (dir.join("ca.pem"), dir.join("ca.key"));
    run(&[
        "ecparam",
        "-name",
        "prime256v1",
        "-genkey",
        "-noout",
        "-out",
        k.to_str().unwrap(),
    ]);
    let cnf = dir.join("ca.cnf");
    std::fs::write(
        &cnf,
        "[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN=test node CA\n[ext]\nbasicConstraints=critical,CA:TRUE,pathlen:0\n\
         keyUsage=critical,keyCertSign\nnameConstraints=critical,@nc\n[nc]\npermitted;DNS.0=ham\npermitted;DNS.1=example.org\n",
    )
    .unwrap();
    run(&[
        "req",
        "-new",
        "-x509",
        "-key",
        k.to_str().unwrap(),
        "-days",
        "2",
        "-config",
        cnf.to_str().unwrap(),
        "-out",
        c.to_str().unwrap(),
    ]);
    (c, k)
}

fn gateway_cert(dir: &Path) -> (PathBuf, PathBuf) {
    let (c, k) = (dir.join("gw.pem"), dir.join("gw.key"));
    run(&[
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
        k.to_str().unwrap(),
        "-out",
        c.to_str().unwrap(),
    ]);
    (c, k)
}

/// A site gateway stand-in: `acceptor` TLS server that records each request head and answers with a small text body.
fn start_gateway(acceptor: openssl::ssl::SslAcceptor) -> (String, Arc<Mutex<Vec<String>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("localhost:{}", l.local_addr().unwrap().port());
    let (acc, s2) = (Arc::new(acceptor), seen.clone());
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            if let Ok(mut t) = acc.accept(s) {
                let mut buf = [0u8; 4096];
                let n = t.read(&mut buf).unwrap_or(0);
                s2.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).to_string());
                let _ = t.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\nsite-body-xyz");
                let _ = t.shutdown();
            }
        }
    });
    (addr, seen)
}

struct Lan {
    addr: String,
    ca_pem: PathBuf,
    log: Arc<Mutex<Vec<u8>>>,
    seen: Arc<Mutex<Vec<String>>>,
    result: Arc<Mutex<Vec<Result<(), ProxyError>>>>,
    _dir: tempfile::TempDir,
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

/// `encrypting`: the gateway offers an ordinary encrypting TLS server instead of an integrity-only one.
fn start_lan(encrypting: bool) -> Lan {
    ham_proxy::resolve::allow_private_upstream(); // the stand-in gateway is on loopback
    let dir = tempfile::Builder::new()
        .prefix("ham_proxy_lan_")
        .tempdir()
        .unwrap();
    let (ca_pem, ca_key) = node_ca(dir.path());
    let (gc, gk) = gateway_cert(dir.path());
    let acceptor = if encrypting {
        let mut b =
            openssl::ssl::SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
        b.set_certificate_chain_file(&gc).unwrap();
        b.set_private_key_file(&gk, openssl::ssl::SslFiletype::PEM)
            .unwrap();
        b.build()
    } else {
        ham_acceptor(&gc, &gk).unwrap()
    };
    let (gw, seen) = start_gateway(acceptor);
    let ca = Arc::new(Ca::load(&ca_pem, &ca_key).unwrap());
    let up = Arc::new(Upstream::gateway(&gw, &gc).unwrap());
    let log = Arc::new(Mutex::new(Vec::new()));
    let policy = Arc::new(
        Policy::new(
            Whitelist::parse("example.org\n"),
            "W1AW",
            Box::new(Sink(log.clone())),
        )
        .unwrap(),
    );
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let result = Arc::new(Mutex::new(Vec::new()));
    let r2 = result.clone();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let r = serve_lan(s, &ca, &policy, &up, "127.0.0.1:1", None);
            r2.lock().unwrap().push(r);
        }
    });
    Lan {
        addr,
        ca_pem,
        log,
        seen,
        result,
        _dir: dir,
    }
}

fn read_all<R: Read>(r: &mut R) -> String {
    let mut v = Vec::new();
    let _ = r.read_to_end(&mut v);
    String::from_utf8_lossy(&v).to_string()
}

/// CONNECT `host:443` and, if answered 200, a browser-style TLS session verified against the node CA as `host`; returns (connect answer, page).
fn browse_connect(lan: &Lan, host: &str) -> (String, Option<String>) {
    let mut tcp = TcpStream::connect(&lan.addr).unwrap();
    write!(
        tcp,
        "CONNECT {host}:443 HTTP/1.1\r\nHost: {host}:443\r\n\r\n"
    )
    .unwrap();
    let mut answer = [0u8; 39];
    let mut got = 0;
    while got < answer.len() {
        let n = tcp.read(&mut answer[got..]).unwrap();
        if n == 0 {
            break;
        }
        got += n;
        if answer[..got].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&answer[..got]).to_string();
    if !head.starts_with("HTTP/1.1 200") {
        // a refusal carries a body; read the rest
        return (format!("{head}{}", read_all(&mut tcp)), None);
    }
    let mut b = SslConnector::builder(SslMethod::tls_client()).unwrap();
    b.set_ca_file(&lan.ca_pem).unwrap();
    let mut tls = b
        .build()
        .connect(host, tcp)
        .expect("browser must accept the leaf from the node CA");
    write!(tls, "GET /page HTTP/1.1\r\nHost: {host}\r\nConnection: keep-alive\r\nX-Ham-Callsign: FORGED\r\n\r\n").unwrap();
    (head, Some(read_all(&mut tls)))
}

fn browse_plain(lan: &Lan, request: &str) -> String {
    let mut tcp = TcpStream::connect(&lan.addr).unwrap();
    tcp.write_all(request.as_bytes()).unwrap();
    read_all(&mut tcp)
}

#[test]
// Tests [@ANCHOR: ham_proxy_lan_serve]
fn connect_is_terminated_with_a_leaf_from_the_node_ca_and_goes_upstream_integrity_only() {
    let lan = start_lan(false);
    let (answer, page) = browse_connect(&lan, "example.org");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(page.unwrap().contains("site-body-xyz"));
    let seen = lan.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].starts_with("GET /page HTTP/1.1\r\nHost: example.org\r\n"),
        "{}",
        seen[0]
    );
    assert!(seen[0].contains("Connection: close"));
    assert!(
        !seen[0].contains("FORGED"),
        "client X-Ham-* headers are dropped"
    );
    let log = String::from_utf8(lan.log.lock().unwrap().clone()).unwrap();
    assert!(log.contains("W1AW -> example.org"), "{log}");
    assert!(
        log.contains("TLS_SHA") || log.contains("NULL-SHA256"),
        "the log names an integrity-only suite: {log}"
    );
}

#[test]
// Tests [@ANCHOR: ham_proxy_lan_serve]
// Tests [@ANCHOR: ham_proxy_lan_upstream_head]
// Tests [@ANCHOR: ham_proxy_relay_response]
fn plain_http_is_accepted_and_upgraded_upstream() {
    let lan = start_lan(false);
    let out = browse_plain(&lan, "GET http://example.org/x?y=1 HTTP/1.1\r\nHost: example.org\r\nProxy-Connection: keep-alive\r\n\r\n");
    assert!(out.contains("site-body-xyz"), "{out}");
    let seen = lan.seen.lock().unwrap();
    assert!(
        seen[0].starts_with("GET /x?y=1 HTTP/1.1\r\nHost: example.org\r\n"),
        "origin form, real Host: {}",
        seen[0]
    );
    assert!(!seen[0].to_lowercase().contains("proxy-connection"));
    drop(seen);
    // origin-form with a Host header (an intercepted or transparent client) works the same way
    let out = browse_plain(&lan, "GET /z HTTP/1.1\r\nHost: example.org\r\n\r\n");
    assert!(out.contains("site-body-xyz"));
}

#[test]
// Tests [@ANCHOR: ham_proxy_lan_serve]
// Tests [@ANCHOR: ham_proxy_lan_refuse]
fn a_site_off_the_whitelist_is_refused_before_any_upstream_connection() {
    let lan = start_lan(false);
    let (answer, page) = browse_connect(&lan, "evil.com");
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert!(page.is_none());
    let out = browse_plain(
        &lan,
        "GET http://evil.com/ HTTP/1.1\r\nHost: evil.com\r\n\r\n",
    );
    assert!(out.starts_with("HTTP/1.1 403"), "{out}");
    let out = browse_plain(
        &lan,
        "GET http://example.org.evil.com/ HTTP/1.1\r\nHost: example.org.evil.com\r\n\r\n",
    );
    assert!(out.starts_with("HTTP/1.1 403"), "{out}");
    assert!(
        lan.seen.lock().unwrap().is_empty(),
        "nothing reached the gateway"
    );
    assert!(
        lan.log.lock().unwrap().is_empty(),
        "no station-ID line for a refused flow"
    );
}

#[test]
// Tests [@ANCHOR: ham_proxy_lan_serve]
// Tests [@ANCHOR: ham_proxy_lan_upstream]
// Tests [@ANCHOR: ham_proxy_lan_connect_upstream]
fn an_upstream_that_would_encrypt_is_never_used() {
    let lan = start_lan(true);
    let out = browse_plain(
        &lan,
        "GET http://example.org/ HTTP/1.1\r\nHost: example.org\r\n\r\n",
    );
    assert!(!out.contains("site-body-xyz"), "{out}");
    assert!(lan.seen.lock().unwrap().is_empty());
    for _ in 0..50 {
        if !lan.result.lock().unwrap().is_empty() {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(lan.result.lock().unwrap()[0].is_err());
}

#[test]
// Tests [@ANCHOR: ham_proxy_lan_leaf]
// Tests [@ANCHOR: ham_proxy_lan_ca]
fn a_leaf_for_a_name_outside_the_ca_constraints_fails_validation() {
    let dir = tempfile::tempdir().unwrap();
    let (ca_pem, ca_key) = node_ca(dir.path());
    let ca = Ca::load(&ca_pem, &ca_key).unwrap();
    let verify = |host: &str| {
        let (leaf, _) = ca.leaf(host).unwrap();
        let root = openssl::x509::X509::from_pem(&std::fs::read(&ca_pem).unwrap()).unwrap();
        let mut store = openssl::x509::store::X509StoreBuilder::new().unwrap();
        store.add_cert(root).unwrap();
        let store = store.build();
        let chain = openssl::stack::Stack::new().unwrap();
        let mut ctx = openssl::x509::X509StoreContext::new().unwrap();
        ctx.init(&store, &leaf, &chain, |c| c.verify_cert())
            .unwrap()
    };
    assert!(verify("w1aw.ham"));
    assert!(verify("www.example.org"));
    assert!(!verify("evil.com"));
}

#[test]
// Tests [@ANCHOR: ham_proxy_lan_parse_target]
// Tests [@ANCHOR: ham_proxy_lan_target]
fn targets_are_read_from_connect_absolute_uri_and_host_header() {
    let t = |h: &str| parse_target(h);
    let want = |connect, host: &str, port, path: &str| {
        Some(Target {
            connect,
            host: host.into(),
            port,
            path: path.into(),
        })
    };
    assert_eq!(
        t("CONNECT Example.org:443 HTTP/1.1\r\n\r\n"),
        want(true, "example.org", 443, "")
    );
    assert_eq!(
        t("GET http://w1aw.ham/a HTTP/1.1\r\n\r\n"),
        want(false, "w1aw.ham", 443, "/a")
    );
    assert_eq!(
        t("GET http://w1aw.ham:8080/ HTTP/1.1\r\n\r\n"),
        want(false, "w1aw.ham", 8080, "/")
    );
    assert_eq!(
        t("GET /p HTTP/1.1\r\nHost: a.ham\r\n\r\n"),
        want(false, "a.ham", 443, "/p")
    );
    assert_eq!(t("GET http://[::1]/ HTTP/1.1\r\n\r\n"), None);
    assert_eq!(t("GET ftp://x/ HTTP/1.1\r\n\r\n"), None);
    assert_eq!(t("CONNECT bad_host:443 HTTP/1.1\r\n\r\n"), None);
}

struct Killed(std::process::Child);
impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
// Tests [@ANCHOR: ham_proxy_run_lan]
// Tests [@ANCHOR: ham_proxy_load_policy]
// Tests [@ANCHOR: ham_proxy_load_signed]
fn the_binary_runs_the_lan_listener_from_its_flags() {
    let dir = tempfile::tempdir().unwrap();
    let (ca_pem, ca_key) = node_ca(dir.path());
    let (gc, gk) = gateway_cert(dir.path());
    let (gw, seen) = start_gateway(ham_acceptor(&gc, &gk).unwrap());
    let wl = dir.path().join("whitelist");
    std::fs::write(&wl, "example.org\n").unwrap();
    let log = dir.path().join("station.log");
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let listen = format!("127.0.0.1:{port}");
    let s = |p: &Path| p.to_str().unwrap().to_string();
    let child = Command::new(env!("CARGO_BIN_EXE_ham_proxy"))
        .args([
            "--lan",
            "--listen",
            &listen,
            "--proxy-ca",
            &s(&ca_pem),
            "--proxy-ca-key",
            &s(&ca_key),
            "--whitelist",
            &s(&wl),
        ])
        .args([
            "--callsign",
            "W1AW",
            "--log",
            &s(&log),
            "--resolver",
            "127.0.0.1:1",
            "--upstream-gateway",
            &gw,
        ])
        .args(["--upstream-ca", &s(&gc), "--allow-private-upstream"])
        .spawn()
        .unwrap();
    let _guard = Killed(child);
    let mut tcp = None;
    for _ in 0..50 {
        if let Ok(t) = TcpStream::connect(&listen) {
            tcp = Some(t);
            break;
        }
        thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut tcp = tcp.expect("the listener started");
    tcp.write_all(b"GET http://example.org/q HTTP/1.1\r\nHost: example.org\r\n\r\n")
        .unwrap();
    assert!(read_all(&mut tcp).contains("site-body-xyz"));
    assert!(seen.lock().unwrap()[0].starts_with("GET /q HTTP/1.1"));
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .contains("W1AW -> example.org"));
    let mut tcp = TcpStream::connect(&listen).unwrap();
    tcp.write_all(b"GET http://evil.com/ HTTP/1.1\r\nHost: evil.com\r\n\r\n")
        .unwrap();
    assert!(read_all(&mut tcp).starts_with("HTTP/1.1 403"));
}

#[test]
// Tests [@ANCHOR: ham_proxy_lan_serve]
// Tests [@ANCHOR: ham_proxy_body_read]
fn a_posted_form_goes_upstream_over_plain_http_and_connect() {
    let lan = start_lan(false);
    let out = browse_plain(
        &lan,
        "POST /form HTTP/1.1\r\nHost: example.org\r\nContent-Length: 7\r\n\r\na=1&b=2",
    );
    assert!(out.contains("site-body-xyz"), "{out}");
    let seen = lan.seen.lock().unwrap();
    assert!(
        seen[0].starts_with("POST /form HTTP/1.1\r\n") && seen[0].ends_with("\r\n\r\na=1&b=2"),
        "{}",
        seen[0]
    );
    drop(seen);
    // both framings at once is a smuggling shape: refused, nothing goes upstream
    let before = lan.seen.lock().unwrap().len();
    let out = browse_plain(&lan, "POST /form HTTP/1.1\r\nHost: example.org\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n");
    assert!(!out.contains("site-body-xyz"), "{out}");
    assert_eq!(lan.seen.lock().unwrap().len(), before);
}
