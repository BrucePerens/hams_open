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

## DANE for .ham servers

A `.ham` server is identified by a DANE-EE TLSA record in the signed `.ham` zone, not by a CA. The resolver (the node's validating resolver) is given as `ip:port`.

- [@ANCHOR: ham_proxy_tlsa_matches] `tlsa_matches(cert_der, records)`: true only if a usage 3, selector 0 record with matching type 0, 1 or 2 pins the certificate; no records means refuse.
- [@ANCHOR: ham_proxy_tlsa_query] `tlsa_query(host, port, id)`: the DNS packet asking for the TLSA records of `_port._tcp.host`.
- [@ANCHOR: ham_proxy_parse_tlsa] `parse_tlsa(packet)`: the TLSA records of a DNS response; empty for an error response or garbage.
- [@ANCHOR: ham_proxy_resolve_tlsa] `resolve_tlsa(resolver, host, port)`: one UDP query to the resolver, 3 s timeout.
- [@ANCHOR: ham_proxy_connect_ham_server] `connect_ham_server(tcp, host, port, resolver)`: integrity-only TLS client; refuses an encrypting suite, and refuses (`NotPinned`) a certificate that no TLSA record pins or a name with no records.
