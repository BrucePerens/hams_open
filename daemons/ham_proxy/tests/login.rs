#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `/.ham/login`: signed-challenge login. Replay, eavesdropper, wrong key, expired challenge, wrong site.
use ham_proxy::login::{audience, respond, Challenges, Verifier};
use ham_proxy::{ham_acceptor, ham_connector, serve_one_login, site_connector};
use openssl::bn::BigNum;
use openssl::ec::{EcGroup, EcKey};
use openssl::ecdsa::EcdsaSig;
use openssl::hash::{hash, MessageDigest};
use openssl::nid::Nid;
use openssl::pkey::{Private, Public};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;
use std::thread;

const SITE: &str = "site.example";
const NOW: u64 = 1_000_000;

fn keypair() -> EcKey<Private> {
    EcKey::generate(&EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap()).unwrap()
}

fn message(aud: &str, nonce: &str) -> Vec<u8> {
    format!("hams-member-cert-possession-v1\n{aud}\n{nonce}").into_bytes()
}

/// Raw r||s ECDSA P-256 signature, hex: what a browser's WebCrypto produces.
fn sign_hex(key: &EcKey<Private>, aud: &str, nonce: &str) -> String {
    let digest = hash(MessageDigest::sha256(), &message(aud, nonce)).unwrap();
    let sig = EcdsaSig::sign(&digest, key).unwrap();
    let (r, s) = (
        sig.r().to_vec_padded(32).unwrap(),
        sig.s().to_vec_padded(32).unwrap(),
    );
    r.iter()
        .chain(s.iter())
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Stands in for hams.com: knows the recorded public key of certificate serial "ab12" and checks exactly what
/// `ham_member_certs._verify_ham_login` checks (audience prefix, signature over the possession message).
struct Recorded(EcKey<Public>);

impl Verifier for Recorded {
    fn verify(&self, serial: &str, nonce: &str, aud: &str, sig_b64: &str) -> Option<String> {
        if serial != "ab12" || !aud.starts_with("ham-login:") {
            return None;
        }
        let raw = openssl::base64::decode_block(sig_b64).ok()?;
        let sig = EcdsaSig::from_private_components(
            BigNum::from_slice(raw.get(..32)?).ok()?,
            BigNum::from_slice(raw.get(32..)?).ok()?,
        )
        .ok()?;
        let digest = hash(MessageDigest::sha256(), &message(aud, nonce)).ok()?;
        sig.verify(&digest, &self.0)
            .ok()?
            .then(|| "W1AW".to_string())
    }
}

fn recorded(key: &EcKey<Private>) -> Recorded {
    Recorded(EcKey::from_public_key(key.group(), key.public_key()).unwrap())
}

fn body(resp: &[u8]) -> String {
    let s = String::from_utf8_lossy(resp).to_string();
    s.split_once("\r\n\r\n")
        .map_or(s.clone(), |x| x.1.to_string())
}

fn challenge(ch: &Challenges, v: &Recorded, now: u64) -> String {
    let r = respond("/.ham/login", SITE, ch, v, now).unwrap();
    let j: serde_json::Value = serde_json::from_str(&body(&r)).unwrap();
    assert_eq!(j["audience"], audience(SITE));
    j["nonce"].as_str().unwrap().to_string()
}

fn answer(target: &str, ch: &Challenges, v: &Recorded, now: u64) -> String {
    String::from_utf8_lossy(&respond(target, SITE, ch, v, now).unwrap()).to_string()
}

fn login_url(serial: &str, nonce: &str, sig: &str) -> String {
    format!("/.ham/login?serial={serial}&nonce={nonce}&sig={sig}")
}

#[test]
fn a_signed_challenge_logs_in() {
    // Tests [@ANCHOR: ham_proxy_login_respond]
    // Tests [@ANCHOR: ham_proxy_login_issue]
    // Tests [@ANCHOR: ham_proxy_login_challenges]
    // Tests [@ANCHOR: ham_proxy_login_audience]
    let (key, ch) = (keypair(), Challenges::default());
    let v = recorded(&key);
    let nonce = challenge(&ch, &v, NOW);
    let sig = sign_hex(&key, &audience(SITE), &nonce);
    let r = answer(&login_url("ab12", &nonce, &sig), &ch, &v, NOW + 5);
    assert!(
        r.starts_with("HTTP/1.1 200") && r.contains("\"callsign\":\"W1AW\""),
        "{r}"
    );
}

#[test]
fn a_replayed_login_is_refused() {
    // Tests [@ANCHOR: ham_proxy_login_take]
    let (key, ch) = (keypair(), Challenges::default());
    let v = recorded(&key);
    let nonce = challenge(&ch, &v, NOW);
    let url = login_url("ab12", &nonce, &sign_hex(&key, &audience(SITE), &nonce));
    assert!(answer(&url, &ch, &v, NOW + 1).starts_with("HTTP/1.1 200"));
    assert!(answer(&url, &ch, &v, NOW + 2).starts_with("HTTP/1.1 403"));
}

#[test]
fn an_eavesdropper_learns_nothing_reusable() {
    // The eavesdropper sees every byte both ways: the challenge answer and the request. Neither holds the key
    // or a password, and neither is accepted again, for that nonce or for a fresh challenge.
    let (key, ch) = (keypair(), Challenges::default());
    let v = recorded(&key);
    let nonce = challenge(&ch, &v, NOW);
    let url = login_url("ab12", &nonce, &sign_hex(&key, &audience(SITE), &nonce));
    let private = key.private_key().to_hex_str().unwrap().to_lowercase();
    let wire = format!("{url}{}", answer(&url, &ch, &v, NOW + 1));
    assert!(
        !wire.to_lowercase().contains(&private) && !wire.contains("assword"),
        "{wire}"
    );
    let fresh = challenge(&ch, &v, NOW + 2);
    let sig_of_old = url.rsplit('=').next().unwrap();
    assert!(
        answer(&login_url("ab12", &fresh, sig_of_old), &ch, &v, NOW + 3)
            .starts_with("HTTP/1.1 403")
    );
    assert!(answer(&url, &ch, &v, NOW + 3).starts_with("HTTP/1.1 403"));
}

#[test]
fn a_wrong_key_is_refused() {
    let (key, ch) = (keypair(), Challenges::default());
    let v = recorded(&key);
    let nonce = challenge(&ch, &v, NOW);
    let sig = sign_hex(&keypair(), &audience(SITE), &nonce);
    assert!(answer(&login_url("ab12", &nonce, &sig), &ch, &v, NOW + 1).starts_with("HTTP/1.1 403"));
}

#[test]
fn an_expired_challenge_is_refused() {
    let (key, ch) = (keypair(), Challenges::default());
    let v = recorded(&key);
    let nonce = challenge(&ch, &v, NOW);
    let url = login_url("ab12", &nonce, &sign_hex(&key, &audience(SITE), &nonce));
    assert!(answer(&url, &ch, &v, NOW + 121).starts_with("HTTP/1.1 403"));
}

#[test]
fn a_signature_for_another_site_is_refused() {
    let (key, ch) = (keypair(), Challenges::default());
    let v = recorded(&key);
    let nonce = challenge(&ch, &v, NOW);
    let url = login_url(
        "ab12",
        &nonce,
        &sign_hex(&key, &audience("evil.example"), &nonce),
    );
    assert!(answer(&url, &ch, &v, NOW + 1).starts_with("HTTP/1.1 403"));
}

#[test]
fn junk_and_unknown_nonces_are_refused_and_other_paths_pass_through() {
    let (key, ch) = (keypair(), Challenges::default());
    let v = recorded(&key);
    for t in [
        "/.ham/login?serial=ab12",
        "/.ham/login?serial=ab12&nonce=zz&sig=00",
        "/.ham/login?serial=ab12&nonce=00112233&sig=abc",
        "/.ham/login?serial=ab12&nonce=00112233&sig=00",
    ] {
        assert!(answer(t, &ch, &v, NOW).starts_with("HTTP/1.1 403"), "{t}");
    }
    assert!(respond("/index.html", SITE, &ch, &v, NOW).is_none());
    assert!(respond("/.ham/loginx", SITE, &ch, &v, NOW).is_none());
}

fn make_cert() -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ham_proxy_login_{}_{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (c, k) = (dir.join("c.pem"), dir.join("k.pem"));
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
            "/CN=localhost"
        ])
        .args(["-addext", "subjectAltName=DNS:localhost", "-keyout"])
        .arg(&k)
        .arg("-out")
        .arg(&c)
        .output()
        .unwrap()
        .status
        .success());
    (dir, c, k)
}

#[test]
fn login_works_over_the_integrity_only_link() {
    // Tests [@ANCHOR: ham_proxy_serve_one_login]
    let (dir, c, k) = make_cert();
    let key = keypair();
    let v = recorded(&key);
    let acc = ham_acceptor(&c, &k).unwrap();
    let conn = site_connector(Some(&c)).unwrap();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let ch = std::sync::Arc::new(Challenges::default());
    let ch2 = ch.clone();
    thread::spawn(move || {
        for s in l.incoming().flatten() {
            let _ = serve_one_login(s, &acc, &conn, "127.0.0.1:1", SITE, None, Some((&ch2, &v)));
        }
    });
    let get = |target: &str| {
        let mut s = ham_connector(&c)
            .unwrap()
            .connect("localhost", TcpStream::connect(&addr).unwrap())
            .unwrap();
        s.write_all(format!("GET {target} HTTP/1.1\r\nHost: {SITE}\r\n\r\n").as_bytes())
            .unwrap();
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        String::from_utf8_lossy(&out).to_string()
    };
    let j: serde_json::Value = serde_json::from_str(&body(get("/.ham/login").as_bytes())).unwrap();
    let nonce = j["nonce"].as_str().unwrap();
    let r = get(&login_url(
        "ab12",
        nonce,
        &sign_hex(&key, j["audience"].as_str().unwrap(), nonce),
    ));
    assert!(r.starts_with("HTTP/1.1 200") && r.contains("W1AW"), "{r}");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_http_verifier_asks_hams_com_and_trusts_only_a_success_result() {
    // Tests [@ANCHOR: ham_proxy_login_http_verifier]
    // Tests [@ANCHOR: ham_proxy_login_http_verify]
    let (dir, c, k) = make_cert();
    let mut b =
        openssl::ssl::SslAcceptor::mozilla_intermediate_v5(openssl::ssl::SslMethod::tls_server())
            .unwrap();
    b.set_certificate_chain_file(&c).unwrap();
    b.set_private_key_file(&k, openssl::ssl::SslFiletype::PEM)
        .unwrap();
    let acc = b.build();
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    thread::spawn(move || {
        for (i, t) in l.incoming().flatten().enumerate() {
            let mut s = acc.accept(t).unwrap();
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap();
            seen2
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).to_string());
            let reply = if i == 0 {
                r#"{"jsonrpc":"2.0","id":null,"result":{"status":"success","callsign":"W1AW"}}"#
            } else {
                r#"{"jsonrpc":"2.0","id":null,"result":{"status":"refused","code":"bad_proof"}}"#
            };
            let _ = s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{reply}",
                    reply.len()
                )
                .as_bytes(),
            );
        }
    });
    let v = ham_proxy::login::HttpVerifier {
        connector: site_connector(Some(&c)).unwrap(),
        addr,
        host: "localhost".into(),
    };
    assert_eq!(
        v.verify("ab12", "00ff", "ham-login:s", "c2ln").as_deref(),
        Some("W1AW")
    );
    assert_eq!(v.verify("ab12", "00ff", "ham-login:s", "c2ln"), None);
    let req = seen.lock().unwrap()[0].clone();
    assert!(
        req.starts_with("POST /ham/login/verify HTTP/1.1")
            && req.contains("\"signature_b64\":\"c2ln\""),
        "{req}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
