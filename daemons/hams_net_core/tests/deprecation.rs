#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Signed deprecation list and its gossip (fake crypto: signature == payload, base64 is hex).
mod common;
use common::Fake;
use hams_net_core::crypto::{BadKey, Crypto};
use hams_net_core::deprecation::DeprecationList;
use std::hash::{DefaultHasher, Hash, Hasher};

/// The fake crypto with a 64-byte "signature": the message's hash repeated, so a changed payload fails.
struct Sig(Fake);

fn fake_sig(msg: &[u8]) -> Vec<u8> {
    let mut h = DefaultHasher::new();
    msg.hash(&mut h);
    h.finish().to_le_bytes().repeat(8)
}

impl Crypto for Sig {
    fn sha256(&self, d: &[u8]) -> Option<Vec<u8>> {
        self.0.sha256(d)
    }
    fn sha512(&self, d: &[u8]) -> Option<Vec<u8>> {
        self.0.sha512(d)
    }
    fn ed25519_verify(&self, key: &[u8], sig: &[u8], msg: &[u8]) -> Result<bool, BadKey> {
        if key.len() != 32 {
            return Err(BadKey);
        }
        Ok(sig == fake_sig(msg))
    }
    fn base64_encode(&self, d: &[u8]) -> String {
        self.0.base64_encode(d)
    }
    fn base64_decode(&self, t: &str) -> Option<Vec<u8>> {
        self.0.base64_decode(t)
    }
    fn random16(&self) -> Option<[u8; 16]> {
        self.0.random16()
    }
}

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn pubkey() -> String {
    hex(&[b"Ed".as_slice(), &[9; 8], &[7; 32]].concat())
}

/// (body, sig) of a document signed with key number [9;8]; a different number is a different key.
fn signed(json: &str, keynum: u8) -> (String, String) {
    let mut sig = [b"Ed".as_slice(), &[keynum; 8]].concat();
    sig.extend_from_slice(&fake_sig(json.as_bytes()));
    (hex(json.as_bytes()), hex(&sig))
}

const KEY_A: &str = "KEYAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const KEY_B: &str = "KEYBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

fn full(seq: u64, keys: &[&str]) -> String {
    let e: Vec<String> = keys
        .iter()
        .map(|k| {
            format!(
                r#"{{"id":1,"key":"{k}","prefixes":["2001:db8:ab00:30::/60"],"reason":"abuse"}}"#
            )
        })
        .collect();
    format!(
        r#"{{"entries":[{}],"issued":"x","kind":"full","seq":{seq}}}"#,
        e.join(",")
    )
}

fn delta(from: u64, seq: u64, add: &[&str], remove: &[&str]) -> String {
    let a: Vec<String> = add
        .iter()
        .map(|k| {
            format!(r#"{{"id":2,"key":"{k}","prefixes":["2001:db8:ab01::/48"],"reason":"abuse"}}"#)
        })
        .collect();
    let r: Vec<String> = remove.iter().map(|k| format!("\"{k}\"")).collect();
    format!(
        r#"{{"add":[{}],"from":{from},"kind":"delta","remove":[{}],"seq":{seq}}}"#,
        a.join(","),
        r.join(",")
    )
}

fn apply(
    l: &mut DeprecationList,
    c: &Sig,
    json: &str,
) -> Option<hams_net_core::deprecation::Change> {
    let (b, s) = signed(json, 9);
    l.apply(c, &b, &s)
}

fn fake_ok() -> Sig {
    Sig(Fake::default())
}

// Tests [@ANCHOR: hams_net_core_deprecation_apply]
// Tests [@ANCHOR: hams_net_core_deprecation_new]
// Tests [@ANCHOR: hams_net_core_deprecation_seq]
// Tests [@ANCHOR: hams_net_core_deprecation_verified]
// Tests [@ANCHOR: hams_net_core_deprecation_entries_of]
#[test]
fn full_then_delta_apply_and_a_gap_a_replay_and_a_wrong_key_are_refused() {
    let c = fake_ok();
    assert!(DeprecationList::new(&c, "zz").is_none());
    let mut l = DeprecationList::new(&c, &pubkey()).unwrap();
    let ch = apply(&mut l, &c, &full(2, &[KEY_A])).unwrap();
    assert_eq!((l.seq(), ch.added.len()), (2, 1));
    assert!(
        apply(&mut l, &c, &delta(5, 7, &[], &[KEY_A])).is_none(),
        "a gap"
    );
    assert!(
        apply(&mut l, &c, &full(1, &[])).is_none(),
        "an older full list"
    );
    let (b, s) = signed(&full(9, &[]), 1);
    assert!(l.apply(&c, &b, &s).is_none(), "another key number");
    let ch = apply(&mut l, &c, &delta(2, 3, &[KEY_B], &[KEY_A])).unwrap();
    assert_eq!(
        (ch.added[0].key.as_str(), ch.removed[0].key.as_str()),
        (KEY_B, KEY_A)
    );
    assert_eq!(l.entries().len(), 1);
    assert!(
        apply(&mut l, &c, &delta(2, 3, &[], &[KEY_B])).is_none(),
        "a replayed delta"
    );
}

// Tests [@ANCHOR: hams_net_core_deprecation_gossip_request]
// Tests [@ANCHOR: hams_net_core_deprecation_gossip_reply]
// Tests [@ANCHOR: hams_net_core_deprecation_accept_gossip]
#[test]
fn a_neighbour_hands_over_the_chain_only_when_newer_and_it_is_verified_again() {
    let c = fake_ok();
    let mut a = DeprecationList::new(&c, &pubkey()).unwrap();
    apply(&mut a, &c, &full(2, &[KEY_A])).unwrap();
    apply(&mut a, &c, &delta(2, 3, &[KEY_B], &[])).unwrap();
    let mut b = DeprecationList::new(&c, &pubkey()).unwrap();
    assert_eq!(b.gossip_request(), "HAMS-GOSSIP 0\n");
    assert_eq!(a.gossip_reply("HAMS-GOSSIP 3\n"), "");
    assert_eq!(a.gossip_reply("GET / HTTP/1.0\n"), "");
    assert_eq!(a.gossip_reply("HAMS-GOSSIP x\n"), "");
    let reply = a.gossip_reply(&b.gossip_request());
    assert_eq!(reply.lines().count(), 2);
    let changes = b.accept_gossip(&c, &reply);
    assert_eq!((b.seq(), changes.len(), b.entries().len()), (3, 2, 2));
    assert!(
        b.accept_gossip(&c, &reply).is_empty(),
        "nothing new the second time"
    );
    // a forged or truncated reply changes nothing
    let mut z = DeprecationList::new(&c, &pubkey()).unwrap();
    let (fb, fs) = signed(&full(9, &[KEY_A]), 1);
    assert!(z
        .accept_gossip(&c, &format!("full 0 {fb} {fs}\ndelta 1 zz!\nnonsense\n"))
        .is_empty());
    assert_eq!(z.seq(), 0);
}
