#![allow(clippy::unwrap_used, clippy::expect_used)]
//! The whitelist and station-ID line that ham_proxy used to own.
use hams_net_core::policy::{format_utc, log_line, Whitelist};

// Tests [@ANCHOR: hams_net_core_policy_whitelist]
// Tests [@ANCHOR: hams_net_core_policy_whitelist_parse]
// Tests [@ANCHOR: hams_net_core_policy_whitelist_add_site]
// Tests [@ANCHOR: hams_net_core_policy_whitelist_permits]
// Tests [@ANCHOR: hams_net_core_policy_whitelist_permits_path]
#[test]
fn whitelist_permits_ham_names_exact_wildcard_and_signed_paths() {
    let mut w = Whitelist::parse("example.org # c\n*.wild.net\n");
    assert!(w.permits("W1AW.ham") && w.permits("Example.org.") && w.permits("a.wild.net"));
    assert!(!w.permits("wild.net") && !w.permits("evil.com"));
    w.add_site("news.com", vec!["/a/".into()]);
    assert!(w.permits_path("news.com", "/a/x?q=1") && w.permits_path("news.com", "/a"));
    assert!(!w.permits_path("news.com", "/ab"));
}

// Tests [@ANCHOR: hams_net_core_policy_format_utc]
// Tests [@ANCHOR: hams_net_core_policy_log_line]
#[test]
fn log_line_needs_a_callsign() {
    assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
    assert_eq!(
        log_line(86_400, " w1aw ", "a.ham", "NULL").unwrap(),
        "1970-01-02T00:00:00Z W1AW -> a.ham NULL\n"
    );
    assert!(log_line(0, " ", "a.ham", "NULL").is_none());
}

// Tests [@ANCHOR: hams_net_core_policy_whitelist_permits_path]
// Tests [@ANCHOR: hams_net_core_policy_climbs]
#[test]
fn path_prefix_cannot_be_climbed_out_of() {
    let mut w = Whitelist::default();
    w.add_site("news.com", vec!["/ham".into()]);
    w.add_site("open.com", vec![]);
    assert!(
        w.permits_path("news.com", "/ham/a.b/c") && w.permits_path("news.com", "/ham/x?next=/../")
    );
    for p in [
        "/ham/../secret",
        "/ham/%2e%2e/secret",
        "/ham/.%2E/secret",
        "/ham/%2e./secret",
        "/ham/..%2fsecret",
        "/ham/..\\secret",
        "/ham/..;x=1/secret",
        "/ham/./../secret",
        "/ham/a%00",
    ] {
        assert!(!w.permits_path("news.com", p), "{p}");
    }
    // a host with no path limit is unaffected
    assert!(w.permits_path("open.com", "/a/../b"));
}
