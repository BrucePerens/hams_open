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
Run `tools/cross_build.sh` (optional target list) to build a small probe binary for the OpenWrt/Linux musl targets (x86_64, aarch64, armv7, mipsel), Android and iOS, and record sizes in `target/cross_sizes.txt`. Pure Rust, so the musl targets link with rust-lld and no C toolchain; mipsel (tier 3, built with nightly build-std) links with the OpenWrt 25.12.6 ramips/mt7621 SDK gcc (default under /home/bruce/agent_work/tools/openwrt-sdk, or set `MIPSEL_LINKER`; probe 745396 bytes); Android uses `ANDROID_NDK_HOME` (default: the NDK installed with sdkmanager under /home/bruce/agent_work/tools/android/ndk); iOS needs a macOS host. Skipped targets are reported as SKIP, never silently.

## Policy (moved from ham_proxy)
`hams_net_core::policy` holds the whitelist (`Whitelist::parse`, `add_site`, `permits`, `permits_path`) and the Part 97 station-ID line (`format_utc`, `log_line`); `ham_proxy` re-exports them and keeps only its log sink. Behaviour is unchanged.

- [@ANCHOR: hams_net_core_policy_whitelist] `Whitelist`: the permitted destinations.
- [@ANCHOR: hams_net_core_policy_whitelist_parse] `Whitelist::parse(text)`: one entry per line (`example.org` exact, `*.example.org` subdomains), blank lines and `#` comments ignored; same file idea as `ham_tls/ham_policy.py`.
- [@ANCHOR: hams_net_core_policy_whitelist_permits] `permits(host)`: true for any `.ham` name, an exact entry, or a subdomain of a wildcard entry; case and trailing dot ignored.
- [@ANCHOR: hams_net_core_policy_format_utc] `format_utc(secs)`: unix time as `YYYY-MM-DDTHH:MM:SSZ`.
- [@ANCHOR: hams_net_core_policy_log_line] `log_line(secs, callsign, host, suite)`: one Part 97 station-ID line `time CALLSIGN -> host suite`; `None` without a callsign.
- [@ANCHOR: hams_net_core_policy_whitelist_add_site] adds a signed-list host with its paths; [@ANCHOR: hams_net_core_policy_whitelist_permits_path] allows a request only when the host is listed and, if the entry names paths, the path is under one of them.
