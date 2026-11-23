#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `/.ham/login` challenge logic that ham_proxy used to own.
mod common;
use common::Fake;
use hams_net_core::login::{
    audience, respond, Challenges, SessionOpener, SessionRefused, Verifier, TTL_SECS,
};

struct V;
impl Verifier for V {
    fn verify(&self, serial: &str, _nonce: &str, aud: &str, sig_b64: &str) -> Option<String> {
        (serial == "ab" && aud.starts_with("ham-login:site.example") && sig_b64 == "0102")
            .then(|| "W1AW".into())
    }
}
struct S;
impl SessionOpener for S {
    fn open(
        &self,
        call: &str,
        peer: &str,
        _n: &str,
        _now: u64,
    ) -> Result<(String, String), SessionRefused> {
        if peer == "dead" {
            Err(SessionRefused)
        } else {
            Ok((format!("sid-{call}"), "beef".into()))
        }
    }
}

fn text(b: Vec<u8>) -> String {
    String::from_utf8(b).unwrap()
}

// Tests [@ANCHOR: hams_net_core_login_challenges]
// Tests [@ANCHOR: hams_net_core_login_issue]
// Tests [@ANCHOR: hams_net_core_login_audience]
// Tests [@ANCHOR: hams_net_core_login_respond]
#[test]
fn challenge_then_answer_signs_in_once() {
    let (c, ch) = (Fake::default(), Challenges::default());
    assert_eq!(audience("site.example"), "ham-login:site.example");
    let first = text(respond("/.ham/login", "site.example", &ch, &V, &S, &c, 100).unwrap());
    assert!(
        first.contains("\"audience\":\"ham-login:site.example\"")
            && first.contains("\"expires_in\":120")
    );
    let nonce = "00".repeat(16);
    assert!(first.contains(&nonce));
    let q = format!("/.ham/login?serial=ab&nonce={nonce}&sig=0102");
    let ok = text(respond(&q, "site.example", &ch, &V, &S, &c, 101).unwrap());
    assert!(ok.starts_with("HTTP/1.1 200") && ok.ends_with("{\"callsign\":\"W1AW\"}"));
    let replay = text(respond(&q, "site.example", &ch, &V, &S, &c, 102).unwrap());
    assert!(replay.starts_with("HTTP/1.1 403"));
    assert!(respond("/other", "site.example", &ch, &V, &S, &c, 1).is_none());
}

// Tests [@ANCHOR: hams_net_core_login_take]
#[test]
fn nonce_expires_and_is_single_use() {
    let (c, ch) = (Fake::default(), Challenges::default());
    let n = ch.issue(&c, 10).unwrap();
    assert!(!ch.take(&n, 10 + TTL_SECS + 1) && !ch.take(&n, 11));
    let n = ch.issue(&c, 10).unwrap();
    assert!(ch.take(&n, 10 + TTL_SECS) && !ch.take(&n, 10));
}

#[test]
fn epk_opens_a_session_or_refuses() {
    let (c, ch) = (Fake::default(), Challenges::default());
    for (epk, want) in [
        ("ff", "\"session\":\"sid-W1AW\",\"epk\":\"beef\""),
        ("dead", "403"),
    ] {
        let nonce = ch.issue(&c, 1).unwrap();
        let q = format!("/.ham/login?serial=ab&nonce={nonce}&sig=0102&epk={epk}");
        // the verifier's audience carries ":epk=..", signature check is by prefix in V
        let out = text(respond(&q, "site.example", &ch, &V, &S, &c, 2).unwrap());
        assert!(out.contains(want), "{out}");
    }
}
