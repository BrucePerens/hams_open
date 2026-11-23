#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Peer table against the scenario shared with `ham_ipv6net/tests/test_ipv6_peers.py`.
use hams_net_core::directory::{Prefix60, Record};
use hams_net_core::peers::{LinkType, PeerTable, Wg};
use serde_json::Value;
use std::cell::RefCell;
use std::net::Ipv6Addr;
use std::rc::Rc;

struct FakeWg(Rc<RefCell<Vec<Vec<String>>>>);

impl Wg for FakeWg {
    fn add_peer(&mut self, i: &str, key: &str, endpoint: &str, p: Prefix60) -> Result<(), String> {
        self.0.borrow_mut().push(vec![
            "add".into(),
            i.into(),
            key.into(),
            endpoint.into(),
            p.to_string(),
        ]);
        Ok(())
    }
    fn remove_peer(&mut self, i: &str, key: &str) -> Result<(), String> {
        self.0
            .borrow_mut()
            .push(vec!["remove".into(), i.into(), key.into()]);
        Ok(())
    }
}

fn records(v: &Value) -> Vec<(Record, Option<Ipv6Addr>)> {
    v["directory"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let s = |k: &str| r[k].as_str().unwrap().to_string();
            let rec = Record {
                callsign: s("callsign"),
                prefix60: Prefix60::parse(&s("prefix60")).unwrap(),
                public_key: s("public_key"),
                endpoint: s("endpoint"),
                interest: vec![],
            };
            (
                rec,
                r.get("answers_for")
                    .map(|a| a.as_str().unwrap().parse().unwrap()),
            )
        })
        .collect()
}

// Tests [@ANCHOR: hams_net_core_peers_new]
// Tests [@ANCHOR: hams_net_core_peers_on_packet]
// Tests [@ANCHOR: hams_net_core_peers_expire]
#[test]
fn scenario_matches_the_python_peer_table() {
    let v: Value = serde_json::from_str(include_str!("vectors/peers.json")).unwrap();
    let dir = records(&v);
    let asked = Rc::new(RefCell::new(0));
    let calls = Rc::new(RefCell::new(vec![]));
    let counter = asked.clone();
    let lookup = move |a: Ipv6Addr| {
        *counter.borrow_mut() += 1;
        dir.iter()
            .find(|(r, liar)| liar.map_or(r.prefix60.contains(a), |l| l == a))
            .map(|(r, _)| r.clone())
    };
    let mut table =
        PeerTable::new(lookup, FakeWg(calls.clone()), "wg-ham", LinkType::Internet).unwrap();
    for e in v["events"].as_array().unwrap() {
        let now = e["t"].as_f64().unwrap();
        let before = *asked.borrow();
        if let Some(dest) = e.get("packet") {
            let key = table
                .on_packet(dest.as_str().unwrap().parse().unwrap(), now)
                .unwrap();
            assert_eq!(key.as_deref(), e["key"].as_str(), "{e}");
            assert_eq!(
                *asked.borrow() > before,
                e["asked"].as_bool().unwrap(),
                "{e}"
            );
        } else {
            let gone = table.expire(now).unwrap();
            let want: Vec<String> = e["expire"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| k.as_str().unwrap().into())
                .collect();
            assert_eq!(gone, want, "{e}");
        }
    }
    let got: Vec<Vec<String>> = calls.borrow().iter().cloned().collect();
    let want: Vec<Vec<String>> = v["wg_calls"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            c.as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().into())
                .collect()
        })
        .collect();
    assert_eq!(got, want);
}

// Tests [@ANCHOR: hams_net_core_peers_new]
#[test]
fn no_peer_on_an_rf_link() {
    let lookup = |_: Ipv6Addr| None;
    assert!(PeerTable::new(lookup, FakeWg(Rc::default()), "wg-ham", LinkType::Rf).is_err());
}
