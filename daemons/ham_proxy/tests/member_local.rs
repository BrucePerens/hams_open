#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `/.ham/login` offline: the member certificate is checked locally (chain, deprecation list, 30 day date grace, signature).
use ham_proxy::login::{HttpVerifier, Verifier};
use ham_proxy::member_local::{FallbackVerifier, LocalVerifier};
use ham_proxy::site_connector;
use openssl::asn1::Asn1Time;
use openssl::bn::BigNum;
use openssl::ec::{EcGroup, EcKey};
use openssl::ecdsa::EcdsaSig;
use openssl::hash::{hash, MessageDigest};
use openssl::nid::Nid;
use openssl::pkey::{PKey, Private};
use openssl::x509::extension::BasicConstraints;
use openssl::x509::{X509Builder, X509NameBuilder, X509};
use std::net::TcpListener;

const AUD: &str = "ham-login:site.example";
const NONCE: &str = "00112233445566778899aabbccddeeff";
const DAY: i64 = 86400;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn key() -> PKey<Private> {
    PKey::from_ec_key(
        EcKey::generate(&EcGroup::from_curve_name(Nid::X9_62_PRIME256V1).unwrap()).unwrap(),
    )
    .unwrap()
}

/// A certificate for `subject_key` signed by `issuer` (self-signed when `issuer` is None), valid from `from` to `to` days from now.
fn cert(
    cn: &str,
    call: Option<&str>,
    serial: u32,
    subject_key: &PKey<Private>,
    issuer: Option<(&X509, &PKey<Private>)>,
    from: i64,
    to: i64,
) -> X509 {
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", cn).unwrap();
    if let Some(c) = call {
        name.append_entry_by_text("1.3.6.1.4.1.66926.1.1", c)
            .unwrap();
    }
    let name = name.build();
    let mut b = X509Builder::new().unwrap();
    b.set_version(2).unwrap();
    b.set_serial_number(&BigNum::from_u32(serial).unwrap().to_asn1_integer().unwrap())
        .unwrap();
    b.set_subject_name(&name).unwrap();
    b.set_issuer_name(issuer.map_or(&name, |i| i.0.subject_name()))
        .unwrap();
    b.set_pubkey(subject_key).unwrap();
    b.set_not_before(&Asn1Time::from_unix(now() + from * DAY).unwrap())
        .unwrap();
    b.set_not_after(&Asn1Time::from_unix(now() + to * DAY).unwrap())
        .unwrap();
    if issuer.is_none() {
        b.append_extension(BasicConstraints::new().critical().ca().build().unwrap())
            .unwrap();
    }
    b.sign(issuer.map_or(subject_key, |i| i.1), MessageDigest::sha256())
        .unwrap();
    b.build()
}

fn sign_b64(k: &PKey<Private>, aud: &str, nonce: &str) -> String {
    let msg = format!("hams-member-cert-possession-v1\n{aud}\n{nonce}");
    let digest = hash(MessageDigest::sha256(), msg.as_bytes()).unwrap();
    let sig = EcdsaSig::sign(&digest, &k.ec_key().unwrap()).unwrap();
    let mut raw = sig.r().to_vec_padded(32).unwrap();
    raw.extend(sig.s().to_vec_padded(32).unwrap());
    openssl::base64::encode_block(&raw)
}

struct Fixture {
    dir: tempfile::TempDir,
    ca: X509,
    ca_key: PKey<Private>,
    member_key: PKey<Private>,
    local: LocalVerifier,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let ca_key = key();
    let ca = cert("Member CA", None, 1, &ca_key, None, -365, 3650);
    std::fs::write(dir.path().join("ca.pem"), ca.to_pem().unwrap()).unwrap();
    let local = LocalVerifier {
        dir: dir.path().join("certs"),
        ca_pem: dir.path().join("ca.pem"),
        deprecation: Some(dir.path().join("deprecated")),
    };
    std::fs::create_dir(&local.dir).unwrap();
    Fixture {
        dir,
        ca,
        ca_key,
        member_key: key(),
        local,
    }
}

impl Fixture {
    fn install(&self, c: &X509) {
        std::fs::write(self.local.dir.join("ab12.pem"), c.to_pem().unwrap()).unwrap();
    }
    fn member(&self, from: i64, to: i64) -> X509 {
        cert(
            "member",
            Some("W1AW"),
            0xab12,
            &self.member_key,
            Some((&self.ca, &self.ca_key)),
            from,
            to,
        )
    }
    fn login(&self) -> Option<String> {
        self.local
            .verify("ab12", NONCE, AUD, &sign_b64(&self.member_key, AUD, NONCE))
    }
}

#[test]
fn a_valid_cached_certificate_logs_in_with_no_hams_com() {
    let f = fixture();
    f.install(&f.member(-10, 300));
    assert_eq!(f.login().as_deref(), Some("W1AW"));
}

#[test]
fn ten_days_past_expiry_still_logs_in_forty_days_does_not() {
    let f = fixture();
    f.install(&f.member(-300, -10));
    assert_eq!(f.login().as_deref(), Some("W1AW"));
    f.install(&f.member(-300, -40));
    assert_eq!(f.login(), None);
}

#[test]
fn a_wrong_signature_audience_serial_or_missing_certificate_is_refused() {
    let f = fixture();
    assert_eq!(f.login(), None, "no cached certificate");
    f.install(&f.member(-10, 300));
    let other = sign_b64(&key(), AUD, NONCE);
    assert_eq!(f.local.verify("ab12", NONCE, AUD, &other), None);
    let good = sign_b64(&f.member_key, AUD, NONCE);
    assert_eq!(f.local.verify("ab12", NONCE, "site.example", &good), None);
    assert_eq!(f.local.verify("../ab12", NONCE, AUD, &good), None);
    assert_eq!(
        f.local.verify("ab12", "11", AUD, &good),
        None,
        "signature is over the nonce"
    );
}

#[test]
fn a_certificate_from_another_ca_is_refused() {
    let f = fixture();
    let rogue_key = key();
    let rogue_ca = cert("Rogue", None, 1, &rogue_key, None, -365, 3650);
    f.install(&cert(
        "member",
        Some("W1AW"),
        0xab12,
        &f.member_key,
        Some((&rogue_ca, &rogue_key)),
        -10,
        300,
    ));
    assert_eq!(f.login(), None);
}

#[test]
fn a_certificate_on_the_deprecation_list_is_refused() {
    let f = fixture();
    f.install(&f.member(-10, 300));
    let fp: String =
        f.ca.digest(MessageDigest::sha256())
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
    std::fs::write(
        f.dir.path().join("deprecated"),
        format!("cert:{fp}:00AB12\n"),
    )
    .unwrap();
    assert_eq!(f.login(), None);
    std::fs::write(
        f.dir.path().join("deprecated"),
        format!("cert:{fp}:00AB13\n"),
    )
    .unwrap();
    assert_eq!(f.login().as_deref(), Some("W1AW"));
}

#[test]
fn remember_keeps_only_the_right_certificate_and_forget_removes_it() {
    let f = fixture();
    let pem = String::from_utf8(f.member(-10, 300).to_pem().unwrap()).unwrap();
    f.local.remember("ab13", &pem);
    assert!(
        !f.local.dir.join("ab13.pem").exists(),
        "serial does not match"
    );
    f.local.remember("ab12", "garbage");
    assert!(!f.local.dir.join("ab12.pem").exists());
    f.local.remember("ab12", &pem);
    assert_eq!(f.login().as_deref(), Some("W1AW"));
    f.local.forget("ab12");
    assert_eq!(f.login(), None);
}

#[test]
fn with_hams_com_unreachable_the_fallback_uses_the_local_check() {
    let f = fixture();
    f.install(&f.member(-10, 300));
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port(); // closed again at once
    let http = HttpVerifier {
        connector: site_connector(None).unwrap(),
        addr: format!("127.0.0.1:{port}"),
        host: "hams.com".into(),
    };
    let v = FallbackVerifier {
        http,
        local: Some(LocalVerifier {
            dir: f.local.dir.clone(),
            ca_pem: f.local.ca_pem.clone(),
            deprecation: None,
        }),
    };
    assert_eq!(
        v.verify("ab12", NONCE, AUD, &sign_b64(&f.member_key, AUD, NONCE))
            .as_deref(),
        Some("W1AW")
    );
    assert_eq!(
        v.verify("ab12", NONCE, AUD, &sign_b64(&key(), AUD, NONCE)),
        None
    );
}
