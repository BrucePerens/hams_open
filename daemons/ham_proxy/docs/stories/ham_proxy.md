# Ham-side proxy prototype

A ham client connects with integrity-only TLS; the proxy relays one request to an Internet site over verified, encrypting TLS and streams the answer back.

- [@ANCHOR: ham_proxy_ham_acceptor] `ham_acceptor(cert, key)` builds a server that offers only `TLS_SHA256_SHA256`, `TLS_SHA384_SHA384` and `NULL-SHA256` (security level 0, required by OpenSSL for NULL suites).
- [@ANCHOR: ham_proxy_require_integrity_only] After the handshake the negotiated suite must be one of those three, otherwise `ProxyError::EncryptedLink`.
- [@ANCHOR: ham_proxy_site_connector] The site side is a normal verified TLS client (default suites; optional extra CA file).
- [@ANCHOR: ham_proxy_serve_one] One ham connection: handshake, read one request head (at most 64 KiB, ends at a blank line), send it to the site, stream the response back until the site closes.

Tested: the request and response are readable on the ham-side wire; TLS 1.2 `NULL-SHA256` works; a client that offers encrypting suites is refused.

Internals:

- [@ANCHOR: ham_proxy_null_only] Shared setup for both ends of a ham link: TLS 1.2 minimum, the `NULL-SHA256` cipher string at security level 0, and the two RFC 9150 TLS 1.3 suites.
- [@ANCHOR: ham_proxy_ham_connector] `ham_connector(ca)` is the matching ham-side client (used by tests and the benchmark): same suites, verifies the proxy certificate against `ca`.
- [@ANCHOR: ham_proxy_read_head] Reads from the ham until the blank line that ends a request head; fails with `BadRequest` after 64 KiB or an early close.
- [@ANCHOR: ham_proxy_main] The `ham_proxy` binary entry point: runs the proxy and exits 1 with the error printed if setup fails.
- [@ANCHOR: ham_proxy_run] Parses `--listen --cert --key --site [--ca]`, binds, and serves each connection on its own thread.
- [@ANCHOR: ham_proxy_arg] Looks up the value following a command-line flag.
- [@ANCHOR: ham_proxy_request_identity] Rewrites the request head for the site: any `Accept-Encoding` header is dropped and `Accept-Encoding: identity` added, so the HTML body can be inspected.
- [@ANCHOR: ham_proxy_transform_html] Turns a buffered HTML response into what the ham gets: the signed-not-encrypted banner exactly once, the signed integrity header, and a block page when a password field appears without a signature login.
- [@ANCHOR: ham_proxy_find_ci] Case-insensitive byte search for a tag or id in an HTML page, starting at an offset; an empty needle finds nothing.
- [@ANCHOR: ham_proxy_dechunk] Decodes a chunked response body (chunk extensions ignored); a malformed or truncated body gives nothing, and the page is then refused.
- [@ANCHOR: ham_proxy_inject] Inserts the not-encrypted banner after the opening body tag (or at the start) unless the page already has it.
- [@ANCHOR: ham_proxy_has_unhandled_password] True when a password input is not marked as handled by the signature login.
- [@ANCHOR: ham_proxy_hex] Lower-case hex of a byte string.
- [@ANCHOR: ham_proxy_unhex] Decodes lower-case hex; an odd length or a non-hex character gives nothing.
- [@ANCHOR: ham_proxy_message] The exact bytes that are signed: version, time, host and body hash.
- [@ANCHOR: ham_proxy_sign_header] Builds the signed integrity header for a body, host and time with an Ed25519 key.
- [@ANCHOR: ham_proxy_verify_header] Checks a signed integrity header against the body, host, time window and public key, saying why it failed.
- [@ANCHOR: ham_proxy_header] Case-insensitive header lookup in a response head.
- [@ANCHOR: ham_proxy_serve_one_signed] One ham connection with the indicator on: buffers the site response, adds the banner and signed integrity header to HTML, blocks an unhandled password page, and passes anything else through unchanged.
- [@ANCHOR: ham_proxy_http_response] Builds a complete HTTP/1.1 response (status, extra headers, length, body) for the block page and refusals.
