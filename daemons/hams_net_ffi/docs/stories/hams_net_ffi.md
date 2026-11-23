# hams_net_ffi

UniFFI bindings over `hams_net_core` for the Android app (Kotlin) and iOS (Swift); the bindings are generated from the built library, never checked in. Build and test: `gradle :core:test` in `android/` (builds the library and generates Kotlin).

- [@ANCHOR: hams_net_ffi_siit_embed] `siit_embed(block, domain, ipv4)`: the IPv6 address of an IPv4 host in a domain.
- [@ANCHOR: hams_net_ffi_siit_extract] `siit_extract(block, address)`: the inverse.
- [@ANCHOR: hams_net_ffi_peer_table_new] `PeerTable(directory, wireguard, interface, rfLink)`: on-demand peers; the platform supplies the directory lookup and WireGuard backend; an RF link is refused.
- [@ANCHOR: hams_net_ffi_peer_table_on_packet] `onPacket(destination, now)`: refresh or create the peer for a destination; its public key or null.
- [@ANCHOR: hams_net_ffi_peer_table_expire] `expire(now)`: remove idle peers.
- [@ANCHOR: hams_net_ffi_deprecation_new] `DeprecationList(crypto, publicKey)`: the signed deprecation list; the platform supplies Ed25519 and base64 (`PlatformCrypto`) and the usign public key line from enrolment.
- [@ANCHOR: hams_net_ffi_foreign_crypto_ed25519_verify] The platform's Ed25519 check behind the core's `Crypto` (a key that is not 32 bytes is refused before the platform is asked).
- [@ANCHOR: hams_net_ffi_deprecation_seq] `seq()`: the held sequence number.
- [@ANCHOR: hams_net_ffi_deprecation_apply] `apply(payload, signature)`: a signed full list or delta from hams.com; the changes to enforce (evict and block added, lift removed), or null when refused.
- [@ANCHOR: hams_net_ffi_deprecation_gossip_request] `gossipRequest()`: the line to send a neighbour on tcp 5710.
- [@ANCHOR: hams_net_ffi_deprecation_gossip_reply] `gossipReply(request)`: the stored signed chain for a neighbour that is behind, else nothing.
- [@ANCHOR: hams_net_ffi_deprecation_accept_gossip] `acceptGossip(reply)`: apply a neighbour's reply with the same checks as a document from hams.com.

Swift: `tools/swift_bindings.sh [OUT_DIR]` generates `hams_net_ffi.swift` plus the C header and modulemap from the built library (any host), and on macOS builds the static library, compiles `swift/hello.swift` against the generated bindings with `swiftc` and runs it (SIIT round trip, refused address; verified on mac1 with the Command Line Tools only). With a full Xcode it also builds iOS and iOS-simulator static libraries and `HamsNetFfi.xcframework`; without Xcode it prints SKIP (mac1 has no Xcode, so the XCFramework step is untested).
