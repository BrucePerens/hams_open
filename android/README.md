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
