# hams_net_ffi

UniFFI bindings over `hams_net_core` for the Android app (Kotlin) and iOS (Swift); the bindings are generated from the built library, never checked in. Build and test: `gradle :core:test` in `android/` (builds the library and generates Kotlin).

- [@ANCHOR: hams_net_ffi_siit_embed] `siit_embed(block, domain, ipv4)`: the IPv6 address of an IPv4 host in a domain.
- [@ANCHOR: hams_net_ffi_siit_extract] `siit_extract(block, address)`: the inverse.
- [@ANCHOR: hams_net_ffi_peer_table_new] `PeerTable(directory, wireguard, interface, rfLink)`: on-demand peers; the platform supplies the directory lookup and WireGuard backend; an RF link is refused.
- [@ANCHOR: hams_net_ffi_peer_table_on_packet] `onPacket(destination, now)`: refresh or create the peer for a destination; its public key or null.
- [@ANCHOR: hams_net_ffi_peer_table_expire] `expire(now)`: remove idle peers.
