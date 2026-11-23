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
