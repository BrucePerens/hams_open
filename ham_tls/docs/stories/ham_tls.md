# Integrity-only TLS for ham links

Ham-side proxy traffic is TLS that authenticates and integrity-protects but does not encrypt (Part 97).
Standard: TLS NULL-cipher suites (RFC 9150 for TLS 1.3, `NULL-SHA256` for TLS 1.2). OpenSSL 3.5 implements only
the TLS 1.2 suite, so that is what runs today; the TLS 1.3 names are already accepted by the policy check for
when a library (survey to-do) supports them. Nothing here invents a protocol: record MACs give tamper detection
and implicit sequence numbers give replay and reorder detection; the certificate gives server identity.

- [@ANCHOR: ham_tls:server_context] `server_context(certfile: str, keyfile: str) -> ssl.SSLContext`. TLS 1.2 only, offers only NULL-SHA256. Raises `ssl.SSLError`/`OSError` on bad files.
- [@ANCHOR: ham_tls:client_context] `client_context(cafile: str) -> ssl.SSLContext`. Same suites; verifies the certificate chain against `cafile` and the hostname.
- [@ANCHOR: ham_tls:require_integrity_only] `require_integrity_only(tls_socket) -> str`. Returns the suite name; raises `EncryptedLinkError` (an `ssl.SSLError`) if the negotiated suite is not in `INTEGRITY_ONLY_SUITES`. Defence in depth: the contexts already offer only NULL suites.
- [@ANCHOR: ham_tls:connect] `connect(sock, context, server_hostname) -> ssl.SSLSocket`. Handshake plus the suite check; closes the socket on refusal. Blocking; raises `ssl.SSLCertVerificationError` for a wrong or untrusted certificate.

Wire behaviour (tested): the payload is readable on the wire; a flipped byte makes the receiver fail with a bad record MAC; a replayed record fails the MAC (sequence number mismatch).

## Policy (engine-independent)

- [@ANCHOR: ham_policy:whitelist_init] `Whitelist(entries)` builds the permit set.
- [@ANCHOR: ham_policy:whitelist] `Whitelist.permits(host)`: every `.ham` name plus listed domains (`example.org` exact, `*.example.org` subdomains); anything else is refused.
- [@ANCHOR: ham_policy:log_connection] `log_connection(stream, callsign, host, suite, now=None)`: one UTC-stamped station-identification line per connection; raises `ValueError` without a callsign.
