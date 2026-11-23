# Ham-side proxy prototype

A ham client connects with integrity-only TLS; the proxy relays one request to an Internet site over verified, encrypting TLS and streams the answer back.

- [@ANCHOR: ham_proxy:ham_acceptor] `ham_acceptor(cert, key)` builds a server that offers only `TLS_SHA256_SHA256`, `TLS_SHA384_SHA384` and `NULL-SHA256` (security level 0, required by OpenSSL for NULL suites).
- [@ANCHOR: ham_proxy:require_integrity_only] After the handshake the negotiated suite must be one of those three, otherwise `ProxyError::EncryptedLink`.
- [@ANCHOR: ham_proxy:site_connector] The site side is a normal verified TLS client (default suites; optional extra CA file).
- [@ANCHOR: ham_proxy:serve_one] One ham connection: handshake, read one request head (at most 64 KiB, ends at a blank line), send it to the site, stream the response back until the site closes.

Tested: the request and response are readable on the ham-side wire; TLS 1.2 `NULL-SHA256` works; a client that offers encrypting suites is refused.
