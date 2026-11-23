#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Boundary decisions equal the chain rendered by `ham_tls/ham_boundary.py` (shared vectors).
use hams_net_core::boundary::{Config, Proto, Verdict};
use serde_json::Value;

fn vectors() -> Value {
    serde_json::from_str(include_str!("vectors/boundary.json")).unwrap()
}

fn config(v: &Value) -> Config {
    let mut c = Config::default();
    let ports = |k: &str| -> Option<Vec<u16>> {
        v.get(k).map(|a| {
            a.as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_u64().unwrap() as u16)
                .collect()
        })
    };
    if let Some(p) = v.get("wg_port") {
        c.wg_port = p.as_u64().unwrap() as u16;
    }
    if let Some(p) = v.get("ham_tls_port") {
        c.ham_tls_port = p.as_u64().unwrap() as u16;
    }
    c.plain_tcp_ports = ports("plain_tcp_ports").unwrap_or(c.plain_tcp_ports);
    c.plain_udp_ports = ports("plain_udp_ports").unwrap_or(c.plain_udp_ports);
    c
}

// Tests [@ANCHOR: hams_net_core_boundary_decide]
#[test]
fn decisions_match_the_rendered_chain() {
    for case in vectors()["configs"].as_array().unwrap() {
        let cfg = config(&case["config"]);
        cfg.check().unwrap();
        for flow in case["flows"].as_array().unwrap() {
            let port = flow
                .get("dport")
                .map(|p| p.as_u64().unwrap() as u16)
                .unwrap_or(0);
            let proto = match flow["proto"].as_str().unwrap() {
                "tcp" => Proto::Tcp(port),
                "udp" => Proto::Udp(port),
                "icmp" => Proto::IcmpEcho,
                "icmpv6" => Proto::Icmpv6Echo,
                "esp" => Proto::Esp,
                _ => Proto::Other,
            };
            let want = match flow["verdict"].as_str().unwrap() {
                "forward" => Verdict::Forward,
                "refused_encrypted" => Verdict::RefusedEncrypted,
                _ => Verdict::RefusedOther,
            };
            let got = cfg.decide(
                flow["to_rf"].as_bool().unwrap(),
                proto,
                flow["new"].as_bool().unwrap(),
            );
            assert_eq!(got, want, "{flow}");
        }
    }
}

// Tests [@ANCHOR: hams_net_core_boundary_check]
#[test]
fn encrypted_ports_cannot_be_allowed() {
    for bad in vectors()["bad_configs"].as_array().unwrap() {
        assert!(config(bad).check().is_err(), "{bad}");
    }
}
