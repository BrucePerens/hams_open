#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Session integrity after login: a request is accepted only with a MAC under the key agreed at login.
//! Capture and replay, forged or altered requests, stale requests and a swapped key are all refused.
use ham_proxy::login::{audience, respond, Challenges, Verifier};
use ham_proxy::session::{derive_key, mac, Refusal};
use ham_proxy::{ham_acceptor, ham_connector, serve_one_login, session_head, site_connector};
use openssl::bn::{BigNum, BigNumContext};
use openssl::ec::{EcGroup, EcKey, PointConversionForm};
use openssl::ecdsa::EcdsaSig;
use openssl::hash::{hash, MessageDigest};
use openssl::nid::Nid;
use openssl::pkey::{Private, Public};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::Command;

const SITE: &str = "site.example";
const NOW: u64 = 5_000_000;

fn group() -> EcGroup {
    EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap()
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
fn epk_hex(k: &EcKey<Private>) -> String {
    let mut ctx = BigNumContext::new().unwrap();
    hex(&k
        .public_key()
        .to_bytes(&group(), PointConversionForm::UNCOMPRESSED, &mut ctx)
        .unwrap())
}
fn sign_hex(key: &EcKey<Private>, aud: &str, nonce: &str) -> String {
    let msg = format!("hams-member-cert-possession-v1\n{aud}\n{nonce}");
    let sig = EcdsaSig::sign(&hash(MessageDigest::sha256(), msg.as_bytes()).unwrap(), key).unwrap();
    hex(&[
        sig.r().to_vec_padded(32).unwrap(),
        sig.s().to_vec_padded(32).unwrap(),
    ]
    .concat())
}

/// Hams.com stand-in: the recorded public key of serial "ab12", audience must be exactly what was signed.
struct Recorded(EcKey<Public>);
impl Verifier for Recorded {
    fn verify(&self, serial: &str, nonce: &str, aud: &str, sig_b64: &str) -> Option<String> {
        let raw = openssl::base64::decode_block(sig_b64).ok()?;
        let sig = EcdsaSig::from_private_components(
            BigNum::from_slice(raw.get(..32)?).ok()?,
            BigNum::from_slice(raw.get(32..)?).ok()?,
        )
        .ok()?;
        let msg = format!("hams-member-cert-possession-v1\n{aud}\n{nonce}");
        let d = hash(MessageDigest::sha256(), msg.as_bytes()).ok()?;
        (serial == "ab12" && sig.verify(&d, &self.0).ok()?).then(|| "W1AW".to_string())
    }
}

struct Logged {
    ch: Challenges,
    sid: String,
    key: Vec<u8>,
    our_epk: String,
}

fn field(resp: &[u8], name: &str) -> Option<String> {
    let s = String::from_utf8_lossy(resp).to_string();
    let j: serde_json::Value = serde_json::from_str(s.split_once("\r\n\r\n")?.1).ok()?;
    Some(j[name].as_str()?.to_string())
}

fn login(member: &EcKey<Private>, eph: &EcKey<Private>) -> Logged {
    let ch = Challenges::default();
    let v = Recorded(EcKey::from_public_key(&group(), member.public_key()).unwrap());
    let r = respond("/.ham/login", SITE, &ch, &v, NOW).unwrap();
    let nonce = field(&r, "nonce").unwrap();
    let epk = epk_hex(eph);
    let aud = format!("{}:epk={epk}", audience(SITE));
    let sig = sign_hex(member, &aud, &nonce);
    let url = format!("/.ham/login?serial=ab12&nonce={nonce}&sig={sig}&epk={epk}");
    let r = respond(&url, SITE, &ch, &v, NOW + 1).unwrap();
    let our_epk = field(&r, "epk").unwrap();
    let key = derive_key(eph, &our_epk, &nonce).unwrap();
    Logged {
        sid: field(&r, "session").unwrap(),
        ch,
        key,
        our_epk,
    }
}

fn request(l: &Logged, ts: u64, nonce: &str, path: &str) -> String {
    let m = mac(&l.key, "GET", path, SITE, ts, nonce, "").unwrap();
    format!(
        "GET {path} HTTP/1.1\r\nHost: {SITE}\r\nX-Ham-Session: {}\r\nX-Ham-Ts: {ts}\r\nX-Ham-Nonce: {nonce}\r\nX-Ham-Mac: {m}\r\n\r\n",
        l.sid
    )
}

fn keypair() -> EcKey<Private> {
    EcKey::generate(&group()).unwrap()
}

#[test]
fn a_request_with_the_session_mac_is_accepted_and_names_the_member() {
    // Tests [@ANCHOR: ham_proxy_session_open]
    // Tests [@ANCHOR: ham_proxy_session_derive_key]
    // Tests [@ANCHOR: ham_proxy_session_mac]
    // Tests [@ANCHOR: ham_proxy_session_check]
    let l = login(&keypair(), &keypair());
    let req = request(&l, NOW + 2, "n1", "/inbox");
    assert_eq!(l.ch.sessions.check(&req, NOW + 2), Ok(Some("W1AW".into())));
}

#[test]
fn a_request_without_a_session_is_anonymous() {
    let l = login(&keypair(), &keypair());
    let head = format!("GET / HTTP/1.1\r\nHost: {SITE}\r\n\r\n");
    assert_eq!(l.ch.sessions.check(&head, NOW), Ok(None));
}

#[test]
fn a_captured_request_cannot_be_replayed() {
    let l = login(&keypair(), &keypair());
    let req = request(&l, NOW + 2, "n1", "/inbox");
    assert!(l.ch.sessions.check(&req, NOW + 2).is_ok());
    assert_eq!(l.ch.sessions.check(&req, NOW + 3), Err(Refusal::Replay));
}

#[test]
fn a_captured_session_id_alone_is_useless() {
    // The eavesdropper has the session id, the server epk and every request: not the key.
    let l = login(&keypair(), &keypair());
    let seen = request(&l, NOW + 2, "n1", "/inbox");
    let guess = format!(
        "GET /inbox HTTP/1.1\r\nHost: {SITE}\r\nX-Ham-Session: {}\r\nX-Ham-Ts: {}\r\nX-Ham-Nonce: n2\r\nX-Ham-Mac: {}\r\n\r\n",
        l.sid,
        NOW + 2,
        "00".repeat(32)
    );
    assert_eq!(l.ch.sessions.check(&guess, NOW + 2), Err(Refusal::BadMac));
    // A key derived by the attacker from the public values and its own ephemeral key is wrong too.
    let forged = derive_key(&keypair(), &l.our_epk, "x").unwrap();
    let m = mac(&forged, "GET", "/inbox", SITE, NOW + 2, "n3", "").unwrap();
    let req = seen
        .replace("n1", "n3")
        .replace(seen.split("X-Ham-Mac: ").nth(1).unwrap().trim(), &m);
    assert_eq!(l.ch.sessions.check(&req, NOW + 2), Err(Refusal::BadMac));
}

#[test]
fn a_captured_mac_does_not_authorize_another_request() {
    let l = login(&keypair(), &keypair());
    let seen = request(&l, NOW + 2, "n1", "/inbox");
    for edited in [
        seen.replace("GET /inbox", "GET /admin"),
        seen.replace("GET /inbox", "POST /inbox"),
        seen.replace(SITE, "evil.example"),
        seen.replace("n1", "n9"),
    ] {
        assert_eq!(l.ch.sessions.check(&edited, NOW + 2), Err(Refusal::BadMac));
    }
}

#[test]
fn a_stale_or_unknown_session_request_is_refused() {
    let l = login(&keypair(), &keypair());
    let old = request(&l, NOW, "n1", "/inbox");
    assert_eq!(l.ch.sessions.check(&old, NOW + 61), Err(Refusal::Stale));
    let other = request(&l, NOW + 2, "n2", "/inbox").replace(&l.sid, &"0".repeat(32));
    assert_eq!(
        l.ch.sessions.check(&other, NOW + 2),
        Err(Refusal::UnknownSession)
    );
    let missing = format!(
        "GET / HTTP/1.1\r\nHost: {SITE}\r\nX-Ham-Session: {}\r\n\r\n",
        l.sid
    );
    assert_eq!(l.ch.sessions.check(&missing, NOW), Err(Refusal::Malformed));
}

#[test]
fn a_swapped_epk_breaks_the_login_signature() {
    // An attacker who injects its own epk into a captured login cannot have the member's signature cover it.
    let (member, eph) = (keypair(), keypair());
    let ch = Challenges::default();
    let v = Recorded(EcKey::from_public_key(&group(), member.public_key()).unwrap());
    let nonce = field(
        &respond("/.ham/login", SITE, &ch, &v, NOW).unwrap(),
        "nonce",
    )
    .unwrap();
    let sig = sign_hex(
        &member,
        &format!("{}:epk={}", audience(SITE), epk_hex(&eph)),
        &nonce,
    );
    let url = format!(
        "/.ham/login?serial=ab12&nonce={nonce}&sig={sig}&epk={}",
        epk_hex(&keypair())
    );
    let r = respond(&url, SITE, &ch, &v, NOW + 1).unwrap();
    assert!(String::from_utf8_lossy(&r).starts_with("HTTP/1.1 403"));
}

#[test]
fn the_forwarded_head_carries_only_the_verified_callsign() {
    // Tests [@ANCHOR: ham_proxy_session_head]
    let head = "GET / HTTP/1.1\r\nHost: s\r\nx-ham-callsign: K1FAKE\r\nX-Ham-Mac: 00\r\nAccept: */*\r\n\r\n";
    let anon = String::from_utf8(session_head(head, None)).unwrap();
    assert_eq!(anon, "GET / HTTP/1.1\r\nHost: s\r\nAccept: */*\r\n\r\n");
    let who = String::from_utf8(session_head(head, Some("W1AW"))).unwrap();
    assert!(
        who.ends_with("Accept: */*\r\nX-Ham-Callsign: W1AW\r\n\r\n") && !who.contains("K1FAKE")
    );
}

#[test]
fn interop_vector_for_the_browser_library() {
    // The browser library (ham_browser_ext/extension/ham_session.js) must reproduce exactly these values.
    let key = hash(MessageDigest::sha256(), b"vector").unwrap();
    assert_eq!(
        mac(
            &key,
            "GET",
            "/inbox?a=1",
            "site.example",
            1_700_000_000,
            "abc123",
            &"11".repeat(32)
        )
        .unwrap(),
        "71d9be15d2790b01a04f6e2e9fa3cda302093ee75f150c3d91dde27f33283585"
    );
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
fn a_bad_session_proof_is_refused_over_the_link_before_the_site_is_contacted() {
    // Tests [@ANCHOR: ham_proxy_serve_one_login]
    let (dir, c, k) = make_cert();
    let l = login(&keypair(), &keypair());
    let ok = request(&l, NOW + 2, "n1", "/inbox");
    let bad = ok.replace("X-Ham-Nonce: n1", "X-Ham-Nonce: n7");
    let (acc, conn) = (
        ham_acceptor(&c, &k).unwrap(),
        site_connector(Some(&c)).unwrap(),
    );
    let lis = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = lis.local_addr().unwrap().to_string();
    let v = Recorded(EcKey::from_public_key(&group(), keypair().public_key()).unwrap());
    let ch = std::sync::Arc::new(l.ch);
    let ch2 = ch.clone();
    std::thread::spawn(move || {
        if let Some(s) = lis.incoming().flatten().next() {
            // Site address is closed: only a refusal before the relay can answer.
            let _ = serve_one_login(s, &acc, &conn, "127.0.0.1:1", SITE, None, Some((&ch2, &v)));
        }
    });
    let mut s = ham_connector(&c)
        .unwrap()
        .connect("localhost", TcpStream::connect(&addr).unwrap())
        .unwrap();
    s.write_all(bad.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    assert!(
        out.starts_with("HTTP/1.1 403") && out.contains("session proof refused"),
        "{out}"
    );
    std::fs::remove_dir_all(&dir).ok();
}
