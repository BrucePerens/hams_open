use super::*;

#[test]
fn only_an_unambiguous_origin_form_head_gets_a_target() {
    // Tests [@ANCHOR: ham_proxy_unambiguous_head]
    // Tests [@ANCHOR: ham_proxy_request_target]
    let ok = "GET /a HTTP/1.1\r\nHost: Site.Example.\r\nAccept: */*\r\n\r\n";
    assert_eq!(
        request_target(ok),
        Some(("site.example".into(), "443".into(), "/a".into()))
    );
    for bad in [
        // two Host headers: the site may pick the one the whitelist did not check
        "GET /a HTTP/1.1\r\nHost: ok.example\r\nHost: evil.example\r\n\r\n",
        // a header the checker does not see as Host but a lenient site does
        "GET /a HTTP/1.1\r\nHost: ok.example\r\nHost : evil.example\r\n\r\n",
        "GET /a HTTP/1.1\r\nHost: ok.example\r\n Host: evil.example\r\n\r\n",
        // absolute-form and CONNECT targets
        "GET http://evil.example/a HTTP/1.1\r\nHost: ok.example\r\n\r\n",
        "CONNECT evil.example:443 HTTP/1.1\r\nHost: ok.example\r\n\r\n",
        // bare LF or CR inside a line
        "GET /a HTTP/1.1\r\nHost: ok.example\nHost: evil.example\r\n\r\n",
        "GET /a HTTP/1.1\r\nHost: ok.example\rHost: evil.example\r\n\r\n",
        // extra request-line parts, no Host
        "GET /a /b HTTP/1.1\r\nHost: ok.example\r\n\r\n",
        "GET /a HTTP/1.1\r\nAccept: */*\r\n\r\n",
    ] {
        assert_eq!(request_target(bad), None, "{bad:?}");
    }
}
