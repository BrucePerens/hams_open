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
- [@ANCHOR: ham_proxy_error] `ProxyError`: `Tls(text)` OpenSSL refused something; `Io(error)` socket error; `EncryptedLink(suite)` the negotiated suite encrypts; `BadRequest` request head too long or cut off.
