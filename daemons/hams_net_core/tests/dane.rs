#![allow(clippy::unwrap_used, clippy::expect_used)]
//! DANE-EE logic that ham_proxy used to own: matching, query packet, response parser.
mod common;
use common::Fake;
use hams_net_core::dane::{parse_tlsa, tlsa_matches, tlsa_query, Tlsa};

// Tests [@ANCHOR: hams_net_core_dane_tlsa_matches]
// Tests [@ANCHOR: hams_net_core_dane_tlsa_type]
#[test]
fn only_dane_ee_full_cert_records_pin() {
    let c = Fake::default();
    let cert = b"CERT";
    let rec = |u, s, m, d: &[u8]| -> Tlsa { (u, s, m, d.to_vec()) };
    assert!(tlsa_matches(&c, cert, &[rec(3, 0, 0, b"CERT")]));
    assert!(tlsa_matches(&c, cert, &[rec(3, 0, 1, b"s256:CERT")]));
    assert!(tlsa_matches(&c, cert, &[rec(3, 0, 2, b"s512:CERT")]));
    assert!(!tlsa_matches(&c, cert, &[rec(3, 0, 1, b"s512:CERT")]));
    assert!(!tlsa_matches(&c, cert, &[rec(2, 0, 0, b"CERT")]));
    assert!(!tlsa_matches(&c, cert, &[rec(3, 1, 0, b"CERT")]));
    assert!(!tlsa_matches(&c, cert, &[rec(3, 0, 3, b"CERT")]));
    assert!(!tlsa_matches(&c, cert, &[]));
}

// Tests [@ANCHOR: hams_net_core_dane_tlsa_query]
// Tests [@ANCHOR: hams_net_core_dane_parse_tlsa]
// Tests [@ANCHOR: hams_net_core_dane_skip_name]
#[test]
fn query_round_trips_through_a_compressed_response() {
    let q = tlsa_query("w1aw.ham.", 443, 0x4841);
    assert_eq!(&q[..2], &[0x48, 0x41]);
    assert_eq!(&q[q.len() - 4..], &[0, 52, 0, 1]);
    let mut r = q.clone();
    r[2] = 0x81;
    r[3] = 0x80;
    r[7] = 1; // one answer
    r.extend_from_slice(&[0xC0, 12, 0, 52, 0, 1, 0, 0, 0, 60, 0, 6, 3, 0, 1, 9, 8, 7]);
    assert_eq!(parse_tlsa(&r), vec![(3, 0, 1, vec![9, 8, 7])]);
    r[3] = 0x83; // NXDOMAIN
    assert!(parse_tlsa(&r).is_empty());
    assert!(parse_tlsa(&[1, 2, 3]).is_empty());
}
