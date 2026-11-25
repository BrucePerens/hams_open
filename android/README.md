# Hams.com Net Android app (skeleton)

Design: `docs/proposals/IPV6_MEMBER_NETWORK.md`, "Phones: Android and iOS". Code only: not published (relay release hold).

- `core/`: pure Kotlin/JVM logic, unit tested without Android: `PeerTable` (port of `ham_ipv6net/client/peers.py`, on-demand
  WireGuard peers, 10 min idle, refuses RF links), `DirectoryClient` (members-only `/api/ipv6/directory/lookup`), `HamRouting`
  (only `.ham`, ham-network and whitelisted traffic is proxied; PAC text), `LinkGuard` (RF/unknown links refused), `CaGuidance`.
- `app/`: `VpnService` that routes only the ham-network prefix into the tunnel, and a screen with the CA guidance.

Native core: `daemons/hams_net_ffi` exposes `hams_net_core` (SIIT map, peer table) through UniFFI; `gradle :core:test` builds it with cargo and generates the Kotlin bindings (JNA), and `NativeCoreTest` calls it. The older Kotlin `PeerTable` stays until the app switches over.

The app packages `libhams_net_ffi.so` for arm64-v8a: the `:app:buildNativeArm64` task runs cargo with the NDK linker (`ANDROID_NDK_HOME`, default `/home/bruce/agent_work/tools/android/ndk/28.2.13676358`, installed with `sdkmanager "ndk;28.2.13676358"`) and the result goes to `app/build/generated/jniLibs/arm64-v8a/`; the merge-jniLibs task depends on it.

Build: JDK 21, Gradle 8.10, Android SDK (platform 35, build-tools 35.0.0), `ANDROID_HOME` set:
`gradle :core:test` and `gradle :app:assembleDebug`.

Tunnel (no GoBackend: it owns its own tun, which leaves no packet hook for on-demand peers and no `setHttpProxy`): the app keeps its `VpnService`
interface and runs WireGuard itself through the shared core (`WgSession`, boringtun). `TunnelEngine` is the `WgBackend` of `PeerTable` and the packet
hook (the first packet toward a member /60 makes the peer), `HamResolver` + `LocalProxy` serve `.ham` names and the PAC (Android 10+ via `setHttpProxy`;
on 8 to 9 set the PAC by hand), `EnrolClient`/`EnrolFlow` enrol with a code the member approves at hams.com/my/ipv6 (only the public key is sent),
`KeyVault` seals the WireGuard private key and the device token with a Keystore AES key.

Emulator test (KVM): `sdkmanager "emulator" "system-images;android-34;default;x86_64"`, an AVD, `emulator -no-window -no-audio -gpu swiftshader_indirect`,
then `ANDROID_SERIAL=emulator-5554 gradle :app:connectedDebugAndroidTest` (`EnrolAndTunnelTest`: enrol against a stub portal on loopback, tunnel up, a
`.ham` request makes the WireGuard peer and the handshake arrives; other hosts are refused). The cargo build covers arm64-v8a and x86_64.

Not done: passkey login (needs Google Play services or a real authenticator), RF links (refused by design), iOS.

## RF node (USB-OTG or Bluetooth serial TNC)

`core` holds the RF side (`RfController` is the screen logic; `OwnedTxGate` and `RfBoundary` port the relay's demo-lock, TX-ownership and readable-only rules; `SerialLines` has the USB and Bluetooth adapters): `Kiss` (KISS framing), `RfInterface` (unencrypted IP over KISS), `RfPolicy` (an RF link refuses WireGuard and every encrypting suite; `PeerTable` also refuses to be built on RF) and the station-ID reminder `enable()` returns, which the app shows when the interface is turned on. Every keying goes through the `TxGate`: it must allow only while this station owns TX and no demo lock is held. Tested against a virtual TNC (`RfInterfaceTest`).

Bench step (needs a real TNC or radio, a licensed operator, a dummy load or a test frequency; agents never transmit):
1. Implement `SerialPort` over `usb-serial-for-android` (USB-OTG TNC) or an RFCOMM socket (Bluetooth TNC such as a Mobilinkd), and a `TxGate` that returns true only while the operator has the TX-owner role and no demo lock is on.
2. Put the TNC in KISS mode, connect it to the phone, enable the RF interface with your callsign; confirm the reminder is shown.
3. Into a dummy load, send one packet; check the TNC keys and the KISS data frame matches `Kiss.encode`. Receive a frame from a second station and check it arrives.
4. Confirm turning on WireGuard for that interface is refused.

Pinned signing keys: the enrolment config and directory lookups are signed by hams.com with the deprecation key and echo the request nonce (`docs/proposals/IPV6_CONFIG_SIGNING.md` in hams_com). `core/src/main/resources/hams_pinned_keys.txt` ships the acceptable public keys (usign lines, filled at release time; see `docs/runbooks/IPV6_NET_LAUNCH.md`). While it is empty the app keeps the old behaviour and prints a warning; once it holds a key, `EnrolClient` and `DirectoryClient` (`SignedReply`) refuse an unsigned, wrongly signed, replayed or wrong-kind reply, and the deprecation key kept is the pinned one that verified the config.
