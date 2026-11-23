#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Indicator, password blocking and signed header: unit tests on the pure functions plus end to end through the
//! proxy against a site returning a fixed response.
use ham_proxy::indicator::{self, VerifyError};
use ham_proxy::{ham_acceptor, ham_connector, serve_one_signed, site_connector, transform_html};
use openssl::pkey::{PKey, Private, Public};
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::thread;

fn keypair() -> (PKey<Private>, PKey<Public>) {
    let k = PKey::generate_ed25519().unwrap();
    let p = PKey::public_key_from_der(&k.public_key_to_der().unwrap()).unwrap();
    (k, p)
}

fn count(h: &[u8], n: &str) -> usize {
    String::from_utf8_lossy(h).matches(n).count()
}

#[test]
fn banner_injected_exactly_once() {
    // Tests [@ANCHOR: ham_proxy_inject]
    // Tests [@ANCHOR: ham_proxy_transform_html]
    let page = b"<html><body class=x><p>hi</p></body></html>";
    let once = indicator::inject(page);
    assert_eq!(count(&once, indicator::BANNER_ID), 1);
    assert!(
        String::from_utf8_lossy(&once).contains("<body class=x><div id=\"ham-integrity-banner\"")
    );
    let twice = indicator::inject(&once);
    assert_eq!(count(&twice, indicator::BANNER_ID), 1, "idempotent");
    assert_eq!(
        count(
            &indicator::inject(b"<p>no body tag</p>"),
            indicator::BANNER_ID
        ),
        1
    );
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 3";
    let out = transform_html(head, page, "h", None, 1);
    assert_eq!(count(&out, indicator::BANNER_ID), 1);
    let s = String::from_utf8_lossy(&out);
    let cl: usize = s
        .split("Content-Length: ")
        .nth(1)
        .unwrap()
        .split("\r\n")
        .next()
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        cl,
        out.len() - s.find("\r\n\r\n").unwrap() - 4,
        "length matches rewritten body"
    );
}

#[test]
fn password_field_blocked_unless_signature_login() {
    // Tests [@ANCHOR: ham_proxy_has_unhandled_password]
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html";
    for tag in [
        "<input type=password name=p>",
        "<INPUT NAME=p TYPE = 'Password'>",
        "<input\n type=\"password\">",
    ] {
        let page = format!("<body><form>{tag}</form></body>");
        let out = transform_html(head, page.as_bytes(), "h", None, 1);
        let s = String::from_utf8_lossy(&out);
        assert!(s.starts_with("HTTP/1.1 403"), "{tag}: {s}");
        assert!(!s.contains("name=p"), "form must not be delivered");
        assert_eq!(count(&out, indicator::BANNER_ID), 1);
    }
    let ok = "<body><input type=password data-ham-login></body>";
    let out = transform_html(head, ok.as_bytes(), "h", None, 1);
    assert!(String::from_utf8_lossy(&out).starts_with("HTTP/1.1 200"));
    let text = "<body><input type=text name=q></body>";
    assert!(
        String::from_utf8_lossy(&transform_html(head, text.as_bytes(), "h", None, 1))
            .starts_with("HTTP/1.1 200")
    );
}

#[test]
fn chunked_html_is_decoded_and_encoded_html_refused() {
    // Tests [@ANCHOR: ham_proxy_header]
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked";
    let out = transform_html(head, b"5\r\n<body\r\n3\r\n></\r\n0\r\n\r\n", "h", None, 1);
    let s = String::from_utf8_lossy(&out);
    assert!(
        s.starts_with("HTTP/1.1 200") && !s.to_lowercase().contains("transfer-encoding"),
        "{s}"
    );
    let gz = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Encoding: gzip";
    assert!(
        String::from_utf8_lossy(&transform_html(gz, b"x", "h", None, 1))
            .starts_with("HTTP/1.1 502")
    );
}

#[test]
fn signed_header_verifies_and_rejects_tamper_wrong_host_replay_and_other_key() {
    // Tests [@ANCHOR: ham_proxy_sign_header]
    // Tests [@ANCHOR: ham_proxy_verify_header]
    // Tests [@ANCHOR: ham_proxy_hex]
    // Tests [@ANCHOR: ham_proxy_unhex]
    // Tests [@ANCHOR: ham_proxy_message]
    // Tests [@ANCHOR: ham_proxy_transform_html]
    let (k, p) = keypair();
    let body = b"<body>x</body>";
    let h = indicator::sign_header(&k, "site.ham", 1000, body).unwrap();
    assert_eq!(
        indicator::verify_header(&h, &p, "site.ham", body, 1010, 300),
        Ok(())
    );
    assert_eq!(
        indicator::verify_header(&h, &p, "site.ham", b"<body>y</body>", 1010, 300),
        Err(VerifyError::BodyChanged)
    );
    assert_eq!(
        indicator::verify_header(&h, &p, "other.ham", body, 1010, 300),
        Err(VerifyError::WrongHost)
    );
    assert_eq!(
        indicator::verify_header(&h, &p, "site.ham", body, 5000, 300),
        Err(VerifyError::Stale),
        "replay of an old page"
    );
    let forged = h.replace("ts=1000", "ts=4990");
    assert_eq!(
        indicator::verify_header(&forged, &p, "site.ham", body, 5000, 300),
        Err(VerifyError::BadSignature)
    );
    let (_, other) = keypair();
    assert_eq!(
        indicator::verify_header(&h, &other, "site.ham", body, 1010, 300),
        Err(VerifyError::BadSignature)
    );
    assert_eq!(
        indicator::verify_header("junk", &p, "site.ham", body, 1010, 300),
        Err(VerifyError::Malformed)
    );
}

fn make_cert(dir: &Path) -> (PathBuf, PathBuf) {
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

/// Site returning `resp` verbatim; proxy in front; returns the ham-side response bytes.
fn via_proxy(tag: &str, resp: Vec<u8>, key: Option<PKey<Private>>) -> Vec<u8> {
    let d = std::env::temp_dir().join(format!("ham_ind_{tag}_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let (c, k) = make_cert(&d);
    let mut b = SslAcceptor::mozilla_intermediate_v5(SslMethod::tls_server()).unwrap();
    b.set_certificate_chain_file(&c).unwrap();
    b.set_private_key_file(&k, SslFiletype::PEM).unwrap();
    let acc = Arc::new(b.build());
    let sl = TcpListener::bind("127.0.0.1:0").unwrap();
    let site_addr = sl.local_addr().unwrap().to_string();
    thread::spawn(move || {
        for s in sl.incoming().flatten() {
            if let Ok(mut t) = acc.accept(s) {
                let mut buf = [0u8; 2048];
                let _ = t.read(&mut buf);
                let _ = t.write_all(&resp);
                let _ = t.shutdown();
            }
        }
    });
    let pacc = ham_acceptor(&c, &k).unwrap();
    let conn = site_connector(Some(&c)).unwrap();
    let pl = TcpListener::bind("127.0.0.1:0").unwrap();
    let paddr = pl.local_addr().unwrap().to_string();
    thread::spawn(move || {
        for s in pl.incoming().flatten() {
            let _ = serve_one_signed(s, &pacc, &conn, &site_addr, "localhost", key.as_ref());
        }
    });
    let mut s = ham_connector(&c)
        .unwrap()
        .connect("localhost", TcpStream::connect(paddr).unwrap())
        .unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nAccept-Encoding: gzip\r\n\r\n")
        .unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    out
}

#[test]
fn end_to_end_html_gets_indicator_and_verifiable_header() {
    let (k, p) = keypair();
    let out = via_proxy(
        "html",
        b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\n\r\n<body>hello</body>".to_vec(),
        Some(k),
    );
    let s = String::from_utf8_lossy(&out).to_string();
    assert_eq!(count(&out, indicator::BANNER_ID), 1, "{s}");
    let (head, body) = s.split_once("\r\n\r\n").unwrap();
    let hv = indicator::header(head, indicator::HEADER).expect("signed header");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert_eq!(
        indicator::verify_header(hv, &p, "localhost", body.as_bytes(), now, 60),
        Ok(())
    );
}

#[test]
fn end_to_end_password_page_blocked() {
    let out = via_proxy(
        "pw",
        b"HTTP/1.0 200 OK\r\nContent-Type: text/html\r\n\r\n<body><input type=password></body>"
            .to_vec(),
        None,
    );
    let s = String::from_utf8_lossy(&out);
    assert!(
        s.starts_with("HTTP/1.1 403") && !s.contains("type=password"),
        "{s}"
    );
}

#[test]
fn end_to_end_non_html_is_byte_identical() {
    let resp =
        b"HTTP/1.0 200 OK\r\nContent-Type: image/png\r\nContent-Length: 6\r\n\r\n\x89PNG\r\n"
            .to_vec();
    let out = via_proxy("png", resp.clone(), None);
    assert_eq!(out, resp);
}
