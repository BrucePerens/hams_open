# Hams.com Net shared core

Platform-free Rust crate shared by the local relay, hams-node and the phone apps. It mirrors the Python implementations; `tests/vectors/*.json` are read by both sides (parity). Consumers supply the directory transport, the clock and the WireGuard backend.

- [@ANCHOR: hams_net_core_siit_block_parse] A translation block is an IPv6 /48; any other length or anything inside 64:ff9b::/96 is refused.
- [@ANCHOR: hams_net_core_siit_embed] An IPv4 host in a domain maps to `<block>:<domain>:0:0:<IPv4>` (RFC 6052 /96 format), same as `ipv6_siit/siit_map.py`.
- [@ANCHOR: hams_net_core_siit_extract] The inverse; refuses other blocks and nonzero bits 64..95.
