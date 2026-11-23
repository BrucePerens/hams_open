# Integrity-only TLS for ham links

Ham-side proxy traffic is TLS that authenticates and integrity-protects but does not encrypt (Part 97).
Standard: TLS NULL-cipher suites (RFC 9150 for TLS 1.3, `NULL-SHA256` for TLS 1.2). OpenSSL 3.5 implements the 9150
suites, but Python's `ssl` module cannot select TLS 1.3 suites, so only the TLS 1.2 suite runs here; the TLS 1.3 names are already accepted by the policy check for
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

## DANE for .ham servers

- [@ANCHOR: ham_dane:tlsa_matches] `ham_dane.tlsa_matches(cert_der, records)`: records are `(usage, selector, matching_type, data)` tuples from the signed `.ham` zone. True only if a DANE-EE (3), full-certificate (0) record with matching type 0, 1 or 2 pins the certificate; no records means refuse. Resolving the TLSA records is the proxy's job.

## AREDN boundary rules (ham_boundary)

Traffic forwarded toward the AREDN side goes onto RF (AREDN RF links are unencrypted, 47 CFR 97.113(a)(4)). WireGuard (the
internet overlay) is fine on the internet side and terminates at the hams-node gateway; nothing encrypted is forwarded on to the mesh.

- [@ANCHOR: ham_boundary:render_ruleset] `render_ruleset(config=None) -> str`. nftables table `inet hams_boundary` from config (`mesh_if`, `wg_if`, `wg_port`, `ham_tls_port`, `plain_tcp_ports`, `plain_udp_ports`, `nflog_group`). Forward chain, toward `mesh_if` only: encrypted protocols (ESP, WireGuard and other VPN UDP ports, HTTPS, SSH, DoT, mail TLS) are dropped and counted in `refused_encrypted`; only `ham_tls_port` (integrity-only TLS) and the listed plain ports are accepted, each new flow logged to nflog group `nflog_group` (the station-ID consumer writes it with `ham_policy.log_connection`) and counted in `forwarded_to_mesh`; everything else is dropped and counted in `refused_other`. The input chain drops the overlay WireGuard port when it arrives from the mesh side. Raises `ValueError` if a configured allowed port is an encrypted service.
Encrypted flows are also marked in the `steer` chain (`tunnel_mark`, default 151) so the node's `hams-node-rf-route` policy rule sends them through the tunnel-only routing table: RF-first routing never steers them onto RF.

Limits: nftables sees ports, not cipher suites. A flow on `ham_tls_port` is integrity-only because the proxy only offers NULL suites and checks with `require_integrity_only`; the whitelist is enforced by the proxy (`ham_policy.Whitelist`). Unlisted destinations toward the mesh are dropped by the default rule. Load with `nft -f` on the gateway; not yet run on a device.
