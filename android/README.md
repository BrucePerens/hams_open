# Hams.com Net Android app (skeleton)

Design: `docs/proposals/IPV6_MEMBER_NETWORK.md`, "Phones: Android and iOS". Code only: not published (relay release hold).

- `core/`: pure Kotlin/JVM logic, unit tested without Android: `PeerTable` (port of `ham_ipv6net/client/peers.py`, on-demand
  WireGuard peers, 10 min idle, refuses RF links), `DirectoryClient` (members-only `/api/ipv6/directory/lookup`), `HamRouting`
  (only `.ham`, ham-network and whitelisted traffic is proxied; PAC text), `LinkGuard` (RF/unknown links refused), `CaGuidance`.
- `app/`: `VpnService` that routes only the ham-network prefix into the tunnel, and a screen with the CA guidance.

Build: JDK 21, Gradle 8.10, Android SDK (platform 35, build-tools 35.0.0), `ANDROID_HOME` set:
`gradle :core:test` and `gradle :app:assembleDebug`.

Not done (needs the portal enrolment flow, an emulator with KVM or a device): enrolling a test node against the stub portal,
the GoBackend-backed `WgBackend`, the DNS/packet hook that calls `PeerTable.onPacket`, the .ham resolver, passkey login, Keystore
key storage, and the emulator test.
