#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Signed whitelist verification that ham_proxy used to own (fake crypto: signature == payload).
mod common;
use common::Fake;
use hams_net_core::signed_list::{parse_utc, unhex, verify_list, ListError};

const KEY: [u8; 32] = [7; 32];
const NOW: u64 = 1_700_000_000;

fn hex(d: &[u8]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn list(payload: &str, sig: &str) -> String {
    format!(
        "{{\"payload\":\"{}\",\"signature\":\"{}\"}}",
        hex(payload.as_bytes()),
        hex(sig.as_bytes())
    )
}

// Tests [@ANCHOR: hams_net_core_signed_list_verify_list]
// Tests [@ANCHOR: hams_net_core_signed_list_list_error]
#[test]
fn good_list_yields_the_whitelist_and_bad_ones_are_refused() {
    let c = Fake::default();
    let p = r#"{"expires":"2099-01-01T00:00:00Z","entries":[{"host":"news.com","paths":["/a/"]}]}"#;
    let w = verify_list(&c, &list(p, p), &KEY, NOW).unwrap();
    assert!(w.permits_path("news.com", "/a/x") && !w.permits_path("news.com", "/b"));
    assert_eq!(
        verify_list(&c, &list(p, "x"), &KEY, NOW).err(),
        Some(ListError::BadSignature)
    );
    assert_eq!(
        verify_list(&c, &list(p, p), &KEY[..5], NOW).err(),
        Some(ListError::Malformed)
    );
    assert_eq!(
        verify_list(&c, "{}", &KEY, NOW).err(),
        Some(ListError::Malformed)
    );
    let old = r#"{"expires":"2001-01-01T00:00:00Z","entries":[]}"#;
    assert_eq!(
        verify_list(&c, &list(old, old), &KEY, NOW).err(),
        Some(ListError::Expired)
    );
    let bad = r#"{"expires":"2099-01-01T00:00:00Z","entries":[{"paths":[]}]}"#;
    assert_eq!(
        verify_list(&c, &list(bad, bad), &KEY, NOW).err(),
        Some(ListError::Malformed)
    );
}

// Tests [@ANCHOR: hams_net_core_signed_list_parse_utc]
// Tests [@ANCHOR: hams_net_core_signed_list_list_unhex]
#[test]
fn utc_and_hex_parsers() {
    assert_eq!(parse_utc("1970-01-02T00:00:01Z"), Some(86_401));
    assert_eq!(parse_utc("2024-02-30"), None);
    assert_eq!(parse_utc("2024-13-01T00:00:00Z"), None);
    assert_eq!(unhex("0aff"), Some(vec![10, 255]));
    assert_eq!(unhex("abc"), None);
    assert_eq!(unhex("zz"), None);
}
