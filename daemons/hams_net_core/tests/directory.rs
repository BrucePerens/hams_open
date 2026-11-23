#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Directory client against the answer shape produced by `ham_ipv6net` (`_directory_record`).
use hams_net_core::directory::*;
use serde_json::Value;

fn v(key: &str) -> Value {
    let all: Value = serde_json::from_str(include_str!("vectors/directory.json")).unwrap();
    all[key].clone()
}

// Tests [@ANCHOR: hams_net_core_directory_parse_lookup]
// Tests [@ANCHOR: hams_net_core_prefix60_parse]
#[test]
fn lookup_answers_parse() {
    for key in ["lookup_ok", "lookup_ok_unwrapped"] {
        let r = parse_lookup(&v(key).to_string()).unwrap().unwrap();
        assert_eq!(r.callsign, "K6BP");
        assert_eq!(r.endpoint, "203.0.113.7:51820");
        assert_eq!(r.prefix60, Prefix60::parse("2001:db8:ab40::/60").unwrap());
    }
    assert_eq!(
        parse_lookup(&v("lookup_ok").to_string())
            .unwrap()
            .unwrap()
            .interest,
        ["W6", "7"]
    );
    assert_eq!(parse_lookup(&v("not_found").to_string()).unwrap(), None);
}

// Tests [@ANCHOR: hams_net_core_directory_parse_lookup]
// Tests [@ANCHOR: hams_net_core_directory_text]
// Tests [@ANCHOR: hams_net_core_directory_record]
// Tests [@ANCHOR: hams_net_core_directory_payload]
// Tests [@ANCHOR: hams_net_core_prefix60_parse]
#[test]
fn malformed_answers_are_errors_not_records() {
    for bad in v("bad_records").as_array().unwrap() {
        assert!(parse_lookup(&bad.to_string()).is_err(), "{bad}");
    }
    assert!(parse_lookup("not json").is_err());
}

// Tests [@ANCHOR: hams_net_core_directory_parse_hubs]
#[test]
fn hubs_keep_order_and_refusal_is_empty() {
    let hubs = parse_hubs(&v("hubs_ok").to_string()).unwrap();
    assert_eq!(
        hubs.iter().map(|h| h.callsign.as_str()).collect::<Vec<_>>(),
        ["W1AW", "N0CALL"]
    );
    assert!(parse_hubs(&v("hubs_refused").to_string())
        .unwrap()
        .is_empty());
}

// Tests [@ANCHOR: hams_net_core_prefix60_contains]
#[test]
fn prefix_contains_only_its_sixty() {
    let p = Prefix60::parse("2001:db8:ab40::/60").unwrap();
    assert!(p.contains("2001:db8:ab40:f:ffff::1".parse().unwrap()));
    assert!(!p.contains("2001:db8:ab40:10::1".parse().unwrap()));
    assert!(!p.contains("2001:db8:ab3f:f::1".parse().unwrap()));
}

// Tests [@ANCHOR: hams_net_core_directory_lookup_request]
// Tests [@ANCHOR: hams_net_core_directory_hubs_request]
#[test]
fn requests_are_odoo_jsonrpc_bodies() {
    let (path, body) = lookup_request("K6BP");
    assert_eq!(path, "/api/ipv6/directory/lookup");
    let b: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(b["params"]["query"], "K6BP");
    assert_eq!(hubs_request().0, "/api/ipv6/directory/hubs");
}
