# ham_proxy

Prototype of the ham-side proxy for the Hams.com Net (design: `hams_com/docs/proposals/IPV6_MEMBER_NETWORK.md`). Towards the ham it speaks
integrity-only TLS (RFC 9150 `TLS_SHA256_SHA256`/`TLS_SHA384_SHA384`, or TLS 1.2 `NULL-SHA256`): authenticated and integrity-protected, not encrypted,
as Part 97 requires on the air. Towards the Internet site it speaks ordinary verified TLS. It was built to measure against Squid; the result and the
choice of base are in `hams_com/docs/runbooks/HAM_PROXY_IMPLEMENTATION_BASE.md`.

Limits: half-duplex request/response relay (one request head, no body; the response streams back until the site closes), a new site handshake per
request, thread per connection. Not a finished proxy.

Run: `ham_proxy --listen 127.0.0.1:8443 --cert cert.pem --key key.pem --site example.org:443 [--ca site-ca.pem]`.
Test: `cargo test` (needs the `openssl` command line tool for test certificates). Benchmark client: `cargo run --release --example bench`.
OpenWrt/static builds: `--features vendored` builds OpenSSL from source.

## Login without a password (`/.ham/login`)

Pages asking for a password are blocked on the ham link, because it is not encrypted. Instead a member proves possession of the private key of a
hams.com member certificate (ECDSA P-256, the same proof `ham_member_certs` already uses) by signing a challenge: `GET /.ham/login` returns a fresh
128-bit nonce and the audience `ham-login:<site>`; `GET /.ham/login?serial=HEX&nonce=HEX&sig=HEX` (raw r||s signature over
`hams-member-cert-possession-v1\n<audience>\n<nonce>`) is answered 200 with the callsign or 403. The proxy mints and spends the nonces (single use, 120 s);
hams.com checks the signature against the key it recorded (`POST /ham/login/verify`, over verified TLS). Nothing secret or reusable crosses the air, no
custom crypto is involved, and no key is derived or used for encryption. Enable with `--verify HAMS_COM_HOST:443`. Code: `src/login.rs`; tests:
`tests/login.rs` (replay, eavesdropper, wrong key, expired challenge, wrong site).
## Types

- [@ANCHOR: ham_proxy_tlsa_type] `Tlsa`: tuple `(usage, selector, matching_type, data)`, one TLSA record: usage 3 is DANE-EE, selector 0 the full certificate, matching type 0/1/2 exact/SHA-256/SHA-512, data the bytes to compare.

## Whitelist and station-ID log

`--whitelist FILE --callsign CALL --log FILE` turns on the site-gateway policy (same whitelist file idea as
`ham_tls/ham_policy.py`: one entry per line, `example.org` exact, `*.example.org` subdomains, `#` comments; every
`.ham` name is always allowed). A site not listed is refused before any upstream connection. Each permitted flow
appends `UTC-time CALLSIGN -> host suite` to the log (Part 97 station identification).
