#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The exported functions called from Rust (the Kotlin test in android/core calls them through UniFFI).
use hams_net_ffi::*;
use std::sync::{Arc, Mutex};

struct Dir;
impl DirectoryLookup for Dir {
    fn lookup(&self, _destination: String) -> Option<DirectoryRecord> {
        Some(DirectoryRecord {
            callsign: "W1AW".into(),
            prefix60: "2001:db8:1:10::/60".into(),
            public_key: "KEY1".into(),
            endpoint: "198.51.100.7:51820".into(),
        })
    }
}

#[derive(Default)]
struct Wg(Mutex<Vec<String>>);
impl WireGuard for Wg {
    fn add_peer(&self, i: String, key: String, _e: String, p: String) {
        self.0.lock().unwrap().push(format!("add {i} {key} {p}"));
    }
    fn remove_peer(&self, i: String, key: String) {
        self.0.lock().unwrap().push(format!("remove {i} {key}"));
    }
}

// Tests [@ANCHOR: hams_net_ffi_siit_embed]
// Tests [@ANCHOR: hams_net_ffi_siit_extract]
#[test]
fn siit_round_trips_and_refuses_a_bad_block() {
    let v6 = siit_embed("2001:db8:1::/48".into(), 7, "10.1.2.3".into()).unwrap();
    assert_eq!(v6, "2001:db8:1:7::a01:203");
    let host = siit_extract("2001:db8:1::/48".into(), v6).unwrap();
    assert_eq!((host.domain, host.ipv4.as_str()), (7, "10.1.2.3"));
    assert!(siit_embed("64:ff9b::/48".into(), 1, "10.0.0.1".into()).is_err());
}

// Tests [@ANCHOR: hams_net_ffi_peer_table_new]
// Tests [@ANCHOR: hams_net_ffi_peer_table_on_packet]
// Tests [@ANCHOR: hams_net_ffi_peer_table_expire]
#[test]
fn peer_table_makes_expires_and_refuses_rf() {
    let wg = Arc::new(Wg::default());
    assert!(PeerTable::new(Arc::new(Dir), wg.clone(), "wg0".into(), true).is_err());
    let t = PeerTable::new(Arc::new(Dir), wg.clone(), "wg0".into(), false).unwrap();
    assert_eq!(
        t.on_packet("2001:db8:1:10::5".into(), 0.0)
            .unwrap()
            .as_deref(),
        Some("KEY1")
    );
    assert_eq!(t.on_packet("2001:db8:2::1".into(), 1.0).unwrap(), None);
    assert!(t.expire(100.0).unwrap().is_empty());
    assert_eq!(t.expire(700.0).unwrap(), vec!["KEY1".to_string()]);
    assert_eq!(wg.0.lock().unwrap().len(), 2);
}

struct Crypt;
fn tag(msg: &[u8]) -> Vec<u8> {
    msg.iter()
        .fold(0u8, |a, b| a.wrapping_add(*b))
        .to_le_bytes()
        .repeat(64)
}
impl PlatformCrypto for Crypt {
    fn ed25519_verify(&self, _key: Vec<u8>, signature: Vec<u8>, message: Vec<u8>) -> bool {
        signature == tag(&message)
    }
    fn base64_decode(&self, text: String) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
            .collect()
    }
}
fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}
fn envelope(json: &str) -> (String, String) {
    let sig = [b"Ed".as_slice(), &[1; 8], &tag(json.as_bytes())].concat();
    (hex(json.as_bytes()), hex(&sig))
}

// Tests [@ANCHOR: hams_net_ffi_foreign_crypto_ed25519_verify]
// Tests [@ANCHOR: hams_net_ffi_deprecation_new]
// Tests [@ANCHOR: hams_net_ffi_deprecation_seq]
// Tests [@ANCHOR: hams_net_ffi_deprecation_apply]
// Tests [@ANCHOR: hams_net_ffi_deprecation_gossip_request]
// Tests [@ANCHOR: hams_net_ffi_deprecation_gossip_reply]
// Tests [@ANCHOR: hams_net_ffi_deprecation_accept_gossip]
#[test]
fn deprecation_list_applies_and_gossips_through_the_platform_crypto() {
    let key = hex(&[b"Ed".as_slice(), &[1; 8], &[7; 32]].concat());
    assert!(DeprecationList::new(Arc::new(Crypt), "nope".into()).is_err());
    let a = DeprecationList::new(Arc::new(Crypt), key.clone()).unwrap();
    let b = DeprecationList::new(Arc::new(Crypt), key).unwrap();
    let doc = r#"{"entries":[{"id":1,"key":"KEYA","prefixes":["2001:db8:ab00:30::/60"],"reason":"abuse"}],"kind":"full","seq":2}"#;
    let (body, sig) = envelope(doc);
    let change = a.apply(body, sig.clone()).unwrap().unwrap();
    assert_eq!(
        (a.seq().unwrap(), change.added[0].key.as_str()),
        (2, "KEYA")
    );
    assert_eq!(
        a.gossip_reply(b.gossip_request().unwrap())
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(a.gossip_reply(a.gossip_request().unwrap()).unwrap(), "");
    let changes = b
        .accept_gossip(a.gossip_reply(b.gossip_request().unwrap()).unwrap())
        .unwrap();
    assert_eq!((b.seq().unwrap(), changes.len()), (2, 1));
    assert!(
        b.apply(hex(b"{}"), sig).unwrap().is_none(),
        "a signature for another payload is refused"
    );
}

fn v6_packet(payload: &[u8]) -> Vec<u8> {
    let mut p = vec![0x60, 0, 0, 0, 0, payload.len() as u8, 17, 64];
    p.extend_from_slice(&[0x20, 0x01, 0xd, 0xb8, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]); // src
    p.extend_from_slice(&[0x20, 0x01, 0xd, 0xb8, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]); // dst
    p.extend_from_slice(payload);
    p
}

// Tests [@ANCHOR: hams_net_ffi_wg_public_key]
// Tests [@ANCHOR: hams_net_ffi_wg_session_new]
// Tests [@ANCHOR: hams_net_ffi_wg_encapsulate]
// Tests [@ANCHOR: hams_net_ffi_wg_decapsulate]
// Tests [@ANCHOR: hams_net_ffi_wg_tick]
#[test]
fn two_sessions_handshake_and_carry_an_ipv6_packet() {
    let (ka, kb) = (vec![7u8; 32], vec![9u8; 32]);
    let (pa, pb) = (
        wg_public_key(ka.clone()).unwrap(),
        wg_public_key(kb.clone()).unwrap(),
    );
    let a = WgSession::new(ka, pb, 1).unwrap();
    let b = WgSession::new(kb, pa, 2).unwrap();
    let packet = v6_packet(b"hello");
    let mut to_b = a.encapsulate(packet.clone()).unwrap(); // the handshake initiation (148 bytes), packet queued
    assert_eq!(to_b[0].len(), 148);
    let mut to_a: Vec<Vec<u8>> = vec![];
    let mut got = None;
    for _ in 0..6 {
        for d in to_b.drain(..) {
            let r = b.decapsulate(d).unwrap();
            to_a.extend(r.replies);
            got = got.or(r.packet);
        }
        for d in to_a.drain(..) {
            let r = a.decapsulate(d).unwrap();
            to_b.extend(r.replies);
        }
        if got.is_some() {
            break;
        }
    }
    assert_eq!(got.unwrap(), packet);
    // Established: the next packet is one data datagram.
    let again = a.encapsulate(v6_packet(b"again")).unwrap();
    assert_eq!(
        b.decapsulate(again[0].clone()).unwrap().packet.unwrap(),
        v6_packet(b"again")
    );
    assert!(a.tick().is_ok());
}

#[test]
fn a_bad_key_length_and_a_garbage_datagram_are_refused() {
    assert!(wg_public_key(vec![1; 31]).is_err());
    let s = WgSession::new(vec![7; 32], wg_public_key(vec![9; 32]).unwrap(), 1).unwrap();
    assert!(s.decapsulate(vec![1, 2, 3]).is_err());
}
