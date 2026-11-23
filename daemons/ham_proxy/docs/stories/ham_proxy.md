# Ham-side proxy prototype

A ham client connects with integrity-only TLS; the proxy relays one request to an Internet site over verified, encrypting TLS and streams the answer back.

- [@ANCHOR: ham_proxy_ham_acceptor] `ham_acceptor(cert, key)` builds a server that offers only `TLS_SHA256_SHA256`, `TLS_SHA384_SHA384` and `NULL-SHA256` (security level 0, required by OpenSSL for NULL suites).
- [@ANCHOR: ham_proxy_require_integrity_only] After the handshake the negotiated suite must be one of those three, otherwise `ProxyError::EncryptedLink`.
- [@ANCHOR: ham_proxy_site_connector] The site side is a normal verified TLS client (default suites; optional extra CA file).
- [@ANCHOR: ham_proxy_serve_one] One ham connection: handshake, read one request head (at most 64 KiB, ends at a blank line), send it to the site, stream the response back until the site closes. HTML responses are buffered instead and passed through `transform_html` (below); all other responses stay byte for byte.
- [@ANCHOR: ham_proxy_serve_one_signed] Same as `serve_one` but with an optional Ed25519 key that signs HTML responses.
- [@ANCHOR: ham_proxy_http_response] Builds the rewritten response (status, optional signed header, correct `Content-Length`, `Connection: close`).
- [@ANCHOR: ham_proxy_inject] Banner insertion after `<body>`; idempotent.
- [@ANCHOR: ham_proxy_find_ci] Case-insensitive byte search used by the HTML scans.
- [@ANCHOR: ham_proxy_has_unhandled_password] Detects `type=password` inputs (any quoting or case) lacking `data-ham-login`.
- [@ANCHOR: ham_proxy_dechunk] Decodes a chunked body so the HTML can be rewritten.
- [@ANCHOR: ham_proxy_sign_header] Builds the `X-Ham-Integrity` value (Ed25519 over version, ts, host, body sha256).
- [@ANCHOR: ham_proxy_verify_header] Verifies it: signature, body hash, host, age.
- [@ANCHOR: ham_proxy_unhex] Hex decoding of the signature field.
- [@ANCHOR: ham_proxy_request_identity] The request sent to the site has any `Accept-Encoding` replaced by `identity`, so HTML arrives uncompressed and can be inspected.
- [@ANCHOR: ham_proxy_transform_html] HTML on a ham link gets the "signed, not encrypted" banner exactly once, a page with an `<input type=password>` not marked `data-ham-login` is replaced by a 403 block page pointing at the signature login, and with a signing key the response carries `X-Ham-Integrity: v1; ts; host; sha256; sig` (Ed25519), checked by `indicator::verify_header` (tamper, wrong host, stale, wrong key all fail).

Tested: the request and response are readable on the ham-side wire; TLS 1.2 `NULL-SHA256` works; a client that offers encrypting suites is refused.

Internals:

- [@ANCHOR: ham_proxy_null_only] Shared setup for both ends of a ham link: TLS 1.2 minimum, the `NULL-SHA256` cipher string at security level 0, and the two RFC 9150 TLS 1.3 suites.
- [@ANCHOR: ham_proxy_ham_connector] `ham_connector(ca)` is the matching ham-side client (used by tests and the benchmark): same suites, verifies the proxy certificate against `ca`.
- [@ANCHOR: ham_proxy_read_head] Reads from the ham until the blank line that ends a request head; fails with `BadRequest` after 64 KiB or an early close.
- [@ANCHOR: ham_proxy_main] The `ham_proxy` binary entry point: runs the proxy and exits 1 with the error printed if setup fails.
- [@ANCHOR: ham_proxy_run] Parses `--listen --cert --key --site [--ca]`, binds, and serves each connection on its own thread.
- [@ANCHOR: ham_proxy_arg] Looks up the value following a command-line flag.
- [@ANCHOR: ham_proxy_login_challenges] A member signs in over the ham link without a password: the proxy keeps the outstanding login challenges in memory.
- [@ANCHOR: ham_proxy_login_issue] `GET /.ham/login` hands out a fresh random 128-bit nonce, remembered for 120 seconds.
- [@ANCHOR: ham_proxy_login_take] A nonce can be answered once; a replayed or expired answer is refused.
- [@ANCHOR: ham_proxy_login_audience] The member's key signs `ham-login:<site>` plus the nonce, so a signature made for one site is useless at another.
- [@ANCHOR: ham_proxy_login_respond] `GET /.ham/login?serial&nonce&sig` is answered 200 with the callsign when hams.com confirms the signature, else 403; any other path goes to the site.
- [@ANCHOR: ham_proxy_login_http_verifier] hams.com checks the signature against the key it recorded for the member certificate, over verified TLS.
- [@ANCHOR: ham_proxy_login_http_verify] The verifier posts the serial, nonce, audience and signature to `/ham/login/verify` and accepts only a `success` answer that names a callsign.
- [@ANCHOR: ham_proxy_session_derive_key] After login the browser's ephemeral P-256 key (`epk`, covered by its login signature) and the proxy's give an ECDH secret; `derive_key` hashes it with the login nonce into a 32-byte MAC key. The key is used for MAC only, never encryption.
- [@ANCHOR: ham_proxy_session_unhex] Hex decoding for the peer public point (odd length or non-hex is refused).
- [@ANCHOR: ham_proxy_session_mac] `mac` is HMAC-SHA256 over `ham-req-v1`, method, target, host, timestamp, nonce and body hash.
- [@ANCHOR: ham_proxy_session_open] A login that sent `epk` answers with a `session` id and the proxy's `epk`; the key is stored with the callsign for 8 hours.
- [@ANCHOR: ham_proxy_session_check] A request with `X-Ham-Session` must carry `X-Ham-Ts` (within 60 s), a fresh `X-Ham-Nonce` and the right `X-Ham-Mac`; a captured session id, an edited request or a replayed one is refused (403).
- [@ANCHOR: ham_proxy_session_head] The head sent to the site has all client `X-Ham-*` headers removed and, for a verified session, `X-Ham-Callsign`.
- [@ANCHOR: ham_proxy_serve_one_login] `serve_one_login` is `serve_one_signed` plus the `/.ham/login` answers.

## DANE for .ham servers

A `.ham` server is identified by a DANE-EE TLSA record in the signed `.ham` zone, not by a CA. The resolver (the node's validating resolver) is given as `ip:port`.

- [@ANCHOR: ham_proxy_tlsa_matches] `tlsa_matches(cert_der, records)`: true only if a usage 3, selector 0 record with matching type 0, 1 or 2 pins the certificate; no records means refuse.
- [@ANCHOR: ham_proxy_tlsa_query] `tlsa_query(host, port, id)`: the DNS packet asking for the TLSA records of `_port._tcp.host`.
- [@ANCHOR: ham_proxy_parse_tlsa] `parse_tlsa(packet)`: the TLSA records of a DNS response; empty for an error response or garbage.
- [@ANCHOR: ham_proxy_resolve_tlsa] `resolve_tlsa(resolver, host, port)`: one UDP query to the resolver, 3 s timeout.
- [@ANCHOR: ham_proxy_connect_ham_server] `connect_ham_server(tcp, host, port, resolver)`: integrity-only TLS client; refuses an encrypting suite, and refuses (`NotPinned`) a certificate that no TLSA record pins or a name with no records.
- [@ANCHOR: ham_proxy_skip_name] Steps over a (possibly compressed) DNS name inside a response.
- [@ANCHOR: ham_proxy_client_builder] The integrity-only client builder shared by `ham_connector` and `connect_ham_server`.

## Page indicator (signed, not encrypted)

- [@ANCHOR: ham_proxy_request_identity] Before the request goes to the site its `Accept-Encoding` is replaced by `identity`, so an HTML body can be inspected and given the banner.
- [@ANCHOR: ham_proxy_transform_html] `transform_html(head, body, host, key, ts)`: turns a buffered HTML response into what the ham gets: the banner exactly once, an optional signed `X-Ham-Integrity` header, and a block page if a password field would be sent in clear; chunked bodies are decoded first and encoded bodies refused.
- [@ANCHOR: ham_proxy_find_ci] Case-insensitive byte search used to find `<body`, `<input` and attributes in HTML.
- [@ANCHOR: ham_proxy_inject] `inject(html)`: puts the banner after the opening `<body>` tag (or at the start) unless it is already there.
- [@ANCHOR: ham_proxy_has_unhandled_password] True if some password input is not marked as handled by the signature login.
- [@ANCHOR: ham_proxy_unhex] Decodes a hex string (the header's hash and signature fields); `None` if malformed.
- [@ANCHOR: ham_proxy_sign_header] `sign_header(...)`: builds the `X-Ham-Integrity` value for a body, host and time, signed with Ed25519.
- [@ANCHOR: ham_proxy_verify_header] `verify_header(...)`: checks a header value against the received body, host, proxy key and clock.
- [@ANCHOR: ham_proxy_verify_error] `VerifyError`: `Malformed` not a v1 header; `BodyChanged` the page was modified; `WrongHost` issued for another host; `Stale` timestamp outside the window; `BadSignature` the signature fails under the proxy key.
- [@ANCHOR: ham_proxy_dechunk] `dechunk(body)`: decodes a chunked body; `None` if malformed.
- [@ANCHOR: ham_proxy_http_response] Builds a complete HTTP/1.1 response (status, extra headers, body) for block pages and rewritten HTML.
- [@ANCHOR: ham_proxy_serve_one_signed] `serve_one_signed(...)`: one ham connection with the indicator and optional signed header; non-HTML is streamed unchanged.
- [@ANCHOR: ham_proxy_error] `ProxyError`: `Tls(text)` OpenSSL refused something; `Io(error)` socket error; `EncryptedLink(suite)` the negotiated suite encrypts; `NotPinned(host)` no TLSA record pins the `.ham` server's certificate; `BadRequest` request head too long or cut off; `NotWhitelisted(host)` the site is not on the whitelist.

## Site whitelist and station-ID log

- [@ANCHOR: ham_proxy_whitelist] `Whitelist`: the permitted destinations.
- [@ANCHOR: ham_proxy_whitelist_parse] `Whitelist::parse(text)`: one entry per line (`example.org` exact, `*.example.org` subdomains), blank lines and `#` comments ignored; same file idea as `ham_tls/ham_policy.py`.
- [@ANCHOR: ham_proxy_whitelist_permits] `permits(host)`: true for any `.ham` name, an exact entry, or a subdomain of a wildcard entry; case and trailing dot ignored.
- [@ANCHOR: ham_proxy_format_utc] `format_utc(secs)`: unix time as `YYYY-MM-DDTHH:MM:SSZ`.
- [@ANCHOR: ham_proxy_log_line] `log_line(secs, callsign, host, suite)`: one Part 97 station-ID line `time CALLSIGN -> host suite`; `None` without a callsign.
- [@ANCHOR: ham_proxy_policy_new] `Policy::new(whitelist, callsign, sink)`: whitelist plus the log sink for one proxy; refuses an empty callsign.
- [@ANCHOR: ham_proxy_policy_record] `Policy::record(...)`: appends the station-ID line for one permitted flow.
- [@ANCHOR: ham_proxy_serve_one_policed] `serve_one_policed(...)`: like `serve_one_login` plus an optional `Policy`: a site not on the whitelist is refused before any upstream connection (`NotWhitelisted`), and each permitted flow is logged with callsign, host and suite.
