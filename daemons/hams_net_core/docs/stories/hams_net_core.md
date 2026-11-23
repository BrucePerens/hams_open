# Hams.com Net shared core

Platform-free Rust crate shared by the local relay, hams-node and the phone apps. It mirrors the Python implementations; `tests/vectors/*.json` are read by both sides (parity). Consumers supply the directory transport, the clock and the WireGuard backend.

- [@ANCHOR: hams_net_core_siit_block_parse] A translation block is an IPv6 /48; any other length or anything inside 64:ff9b::/96 is refused.
- [@ANCHOR: hams_net_core_siit_embed] An IPv4 host in a domain maps to `<block>:<domain>:0:0:<IPv4>` (RFC 6052 /96 format), same as `ipv6_siit/siit_map.py`.
- [@ANCHOR: hams_net_core_siit_extract] The inverse; refuses other blocks and nonzero bits 64..95.
- [@ANCHOR: hams_net_core_boundary_check] A boundary config that would allow an encrypted service port (SSH, HTTPS, WireGuard, IKE, OpenVPN and so on) toward RF is refused.
- [@ANCHOR: hams_net_core_boundary_decide] Whether a packet may leave through an RF interface, in the order of the nftables chain from `ham_tls/ham_boundary.py`: ESP and known encrypted UDP/TCP ports refused as encrypted; listed plain or integrity-only (ham_tls) services allowed for new flows; established flows and ICMP echo allowed; everything else refused. Non-RF interfaces are not restricted. `tests/vectors/boundary.json` pins it to the rendered ruleset.
- [@ANCHOR: hams_net_core_prefix60_parse] A member prefix is an IPv6 /60 with zero host bits; anything else is refused.
- [@ANCHOR: hams_net_core_prefix60_contains] Whether an address lies inside a member /60.
- [@ANCHOR: hams_net_core_directory_lookup_request] The lookup request (path and Odoo JSON-RPC body) for a callsign or address; transport and credential are the consumer's.
- [@ANCHOR: hams_net_core_directory_hubs_request] The volunteer-hubs request.
- [@ANCHOR: hams_net_core_directory_parse_lookup] A lookup answer becomes a record (callsign, /60, WireGuard key, endpoint, interest set); an error status ("not found", "refused") is `None`; a record missing a field or with a bad /60 is an error, never a half record.
- [@ANCHOR: hams_net_core_directory_parse_hubs] A hubs answer becomes the ordered record list; a refusal is an empty list.
- [@ANCHOR: hams_net_core_directory_text] A required record field must be a non-empty string.
- [@ANCHOR: hams_net_core_directory_record] One record from its JSON object.
- [@ANCHOR: hams_net_core_directory_payload] The answer body, wrapped in JSON-RPC `result` or bare, with its `status` checked.
- [@ANCHOR: hams_net_core_peers_new] A peer table is made for one internet interface; asking for one on an RF link is refused (WireGuard encrypts, 47 CFR 97.113(a)(4)).
- [@ANCHOR: hams_net_core_peers_on_packet] The first packet toward a member /60 asks the directory (lookup supplied by the consumer) and adds a peer for exactly that /60; later packets refresh it. A destination the directory does not know, or answers with a /60 not containing it, is not asked about again for 60 seconds.
- [@ANCHOR: hams_net_core_peers_expire] A peer with no traffic for more than ten minutes is removed; the keys removed are returned.

## Cross builds
Run `tools/cross_build.sh` (optional target list) to build a small probe binary for the OpenWrt/Linux musl targets (x86_64, aarch64, armv7, mipsel), Android and iOS, and record sizes in `target/cross_sizes.txt`. Pure Rust, so the musl targets link with rust-lld and no C toolchain; mipsel (tier 3) compiles the library only unless `MIPSEL_LINKER` names an OpenWrt mipsel gcc; Android uses `ANDROID_NDK_HOME` (default: the NDK installed with sdkmanager under /home/bruce/agent_work/tools/android/ndk); iOS needs a macOS host. Skipped targets are reported as SKIP, never silently.

## Policy (moved from ham_proxy)
`hams_net_core::policy` holds the whitelist (`Whitelist::parse`, `add_site`, `permits`, `permits_path`) and the Part 97 station-ID line (`format_utc`, `log_line`); `ham_proxy` re-exports them and keeps only its log sink. Behaviour is unchanged.

- [@ANCHOR: hams_net_core_policy_whitelist] `Whitelist`: the permitted destinations.
- [@ANCHOR: hams_net_core_policy_whitelist_parse] `Whitelist::parse(text)`: one entry per line (`example.org` exact, `*.example.org` subdomains), blank lines and `#` comments ignored; same file idea as `ham_tls/ham_policy.py`.
- [@ANCHOR: hams_net_core_policy_whitelist_permits] `permits(host)`: true for any `.ham` name, an exact entry, or a subdomain of a wildcard entry; case and trailing dot ignored.
- [@ANCHOR: hams_net_core_policy_format_utc] `format_utc(secs)`: unix time as `YYYY-MM-DDTHH:MM:SSZ`.
- [@ANCHOR: hams_net_core_policy_log_line] `log_line(secs, callsign, host, suite)`: one Part 97 station-ID line `time CALLSIGN -> host suite`; `None` without a callsign.
- [@ANCHOR: hams_net_core_policy_whitelist_add_site] adds a signed-list host with its paths; [@ANCHOR: hams_net_core_policy_whitelist_permits_path] allows a request only when the host is listed and, if the entry names paths, the path is under one of them.

## Login, signed list and DANE logic (moved from ham_proxy)
`hams_net_core::login` (nonce challenges, the `/.ham/login` answers), `signed_list` (Ed25519-signed whitelist verification) and `dane` (TLSA matching, query packet, response parser) hold the I/O-free logic; the primitives they need (hashes, Ed25519 check, base64, random bytes) come from the consumer through `crypto::Crypto` (`ham_proxy::crypto::OpensslCrypto`). `ham_proxy` keeps the sockets, TLS handshake, the session key agreement and the HTTP verifier, and re-exports the same functions with unchanged signatures. Behaviour is unchanged; ham_proxy's own tests pass unchanged.

- [@ANCHOR: hams_net_core_login_challenges] A member signs in over the ham link without a password: the proxy keeps the outstanding login challenges in memory.
- [@ANCHOR: hams_net_core_login_issue] `GET /.ham/login` hands out a fresh random 128-bit nonce, remembered for 120 seconds.
- [@ANCHOR: hams_net_core_login_take] A nonce can be answered once; a replayed or expired answer is refused.
- [@ANCHOR: hams_net_core_login_audience] The member's key signs `ham-login:<site>` plus the nonce, so a signature made for one site is useless at another.
- [@ANCHOR: hams_net_core_login_respond] `GET /.ham/login?serial&nonce&sig` is answered 200 with the callsign when hams.com confirms the signature, else 403; any other path goes to the site.
- [@ANCHOR: hams_net_core_dane_tlsa_matches] `tlsa_matches(cert_der, records)`: true only if a usage 3, selector 0 record with matching type 0, 1 or 2 pins the certificate; no records means refuse.
- [@ANCHOR: hams_net_core_dane_tlsa_query] `tlsa_query(host, port, id)`: the DNS packet asking for the TLSA records of `_port._tcp.host`.
- [@ANCHOR: hams_net_core_dane_parse_tlsa] `parse_tlsa(packet)`: the TLSA records of a DNS response; empty for an error response or garbage.
- [@ANCHOR: hams_net_core_dane_skip_name] Steps over a (possibly compressed) DNS name inside a response.
- [@ANCHOR: hams_net_core_signed_list_verify_list] checks the published signed whitelist (Ed25519 over the payload, then expiry) and returns a whitelist; [@ANCHOR: hams_net_core_signed_list_list_error] says why it was refused (malformed, bad signature, expired); [@ANCHOR: hams_net_core_signed_list_parse_utc] reads its `expires` time and [@ANCHOR: hams_net_core_signed_list_list_unhex] reads the hex public key file.
- [@ANCHOR: hams_net_core_dane_tlsa_type] `Tlsa`: tuple `(usage, selector, matching_type, data)`, one TLSA record: usage 3 is DANE-EE, selector 0 the full certificate, matching type 0/1/2 exact/SHA-256/SHA-512, data the bytes to compare.
