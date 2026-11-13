# radioctl function anchors, batch 47

Function anchors for the `radioctl-station` crate, part 4: the real rig-actor link in `actor_link.rs` (`ActorLinks`, the `RadioLink` implementation that spawns a rig actor, unkeys the radio and tracks the actor per radio, and `LiveKey`, the keyed-state source that follows the current actor) and the minimal WebSocket client codec in `ws.rs` (SHA-1, base64, handshake, frame encoder and the reassembling `Decoder`) used to talk to Home Assistant.

Transmit safety in this batch: nothing here keys a transmitter or switches hardware on its own. The one operation that touches a radio is the unkey pass in `ActorLinks::unkey_pass`, which sends `Op::SetPtt(false)` (release the transmitter) to a freshly spawned actor before the link is reported up; it can only stop a transmission, never start one. The transmit gate of a spawned actor stays closed and is the spawner's business. `LiveKey` only reads keyed state, for the station interlock, and reports unknown whenever it cannot tell. `ws.rs` is pure byte and string handling with no I/O and no connection to any transmit path.

## `crates/radioctl-station/src/actor_link.rs`

### `fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T>` [@ANCHOR: radioctl:radioctl_station_actorlink_lock]

Private helper that locks a standard `Mutex` and ignores poisoning: if another thread panicked while holding the lock, the inner guard is taken anyway instead of panicking again. The argument `m` is borrowed; the returned guard borrows it and releases the lock when dropped. It blocks while another holder has the lock, so callers must not hold the guard across an await. It cannot fail and has no side effects of its own. Used for every access to the shared link state.

### `fn transport_up(watch: &Option<watch::Receiver<LinkStatus>>) -> bool` [@ANCHOR: radioctl:radioctl_station_actorlink_transport_up]

Says whether the transport status, when there is one, reports the link as usable. `watch` is borrowed and read only: `None` (the transport has no status of its own) counts as up, and `Some` is up only when the current value is `LinkStatus::Connected`; any other status (for example reconnecting) is down. Pure and synchronous apart from borrowing the watch value for an instant; it cannot fail, never blocks and has no side effects.

### `pub fn new(spawner: Arc<dyn RigSpawner>) -> ActorLinks` [@ANCHOR: radioctl:radioctl_station_actorlink_new]

Creates the set of links. `spawner` is an owned shared reference to the application-supplied object that opens a radio's transport and spawns its actor. The new value has no slots, no spawn counts and an empty journal, a connect lock that serialises attempts, and an unkey timeout of three seconds. Returns the `ActorLinks`; cannot fail, does no I/O and spawns nothing until the manager asks for a connection.

### `pub fn with_unkey_timeout(mut self, timeout: Duration) -> ActorLinks` [@ANCHOR: radioctl:radioctl_station_actorlink_with_unkey_timeout]

Builder step that changes how long the unkey pass may take before the connection attempt counts as failed. `self` is consumed and returned, so it is meant to be called while building, before the links are shared; `timeout` is a `Duration` passed to `execute_within` for the unkey operation. A short value fails slow radios sooner and lets the manager back off. Cannot fail, no I/O, no locking.

### `pub fn handle(&self, radio: &DeviceId) -> Option<RigHandle>` [@ANCHOR: radioctl:radioctl_station_actorlink_handle]

Returns the current rig actor of a radio, for the application's own use such as queries and the transmit gate. `radio` is borrowed. The result is a clone of the stored `RigHandle`, or `None` when no actor has been registered for that radio (no connection has succeeded yet). The handle may belong to an actor whose link is now down, since a respawn replaces the slot only on the next successful connect. Briefly takes the internal mutex; cannot fail, no I/O.

### `pub fn key_source(&self, radio: &DeviceId) -> Arc<dyn KeyedSource>` [@ANCHOR: radioctl:radioctl_station_actorlink_key_source]

Builds a keyed-state source for the station interlock that always follows the radio's current actor, so a respawn does not leave the interlock looking at a dead handle. `radio` is cloned into the returned `LiveKey`, which shares the link state through an `Arc`. A radio with no actor reports unknown. The call itself takes no lock, does no I/O and cannot fail; the lookup happens each time the source is asked for the key state.

### `pub fn spawns(&self, radio: &DeviceId) -> u32` [@ANCHOR: radioctl:radioctl_station_actorlink_spawns]

Returns how many actors have been spawned for a radio since the links were created, counted on each successful spawner call in `connect` (before the unkey pass, so an actor dropped by a failed unkey is still counted). `radio` is borrowed. Returns 0 for a radio never spawned. Briefly takes the internal mutex; cannot fail, no I/O. Mainly useful to check that two connection attempts did not create two actors.

### `pub fn journal(&self) -> Vec<String>` [@ANCHOR: radioctl:radioctl_station_actorlink_journal]

Returns a copy of what the links did, oldest first: spawns, unkey passes, refusals to respawn while the transport retries, and unkey failures. Takes no arguments besides `&self`. The journal is capped at 500 lines by `note`, so the oldest lines are dropped first. Briefly takes the internal mutex to clone the list; cannot fail, no I/O. Intended for diagnostics and tests.

### `fn note(&self, line: String)` [@ANCHOR: radioctl:radioctl_station_actorlink_note]

Appends one line to the journal. `line` is owned and stored as given. After the push, anything beyond the newest 500 lines is drained from the front, so memory stays bounded. Takes the internal mutex for the duration of the push, so it must not be called while the caller already holds that lock. Returns nothing, cannot fail, performs no I/O and does not log anywhere else.

### `fn slot_state(&self, radio: &DeviceId) -> SlotState` [@ANCHOR: radioctl:radioctl_station_actorlink_state]

Classifies the current slot of a radio: `Absent` when there is no slot; `Retrying` when the transport status says it is down and the status is `Reconnecting` (it recovers by itself); `Dead` when the transport status is down in any other way, or when the actor's own `link_up()` is false; otherwise `Up`. `radio` is borrowed. Reads under the mutex; no I/O, cannot fail. Decides in `connect` whether to respawn.

### `async fn unkey_pass(&self, radio: &DeviceId, handle: &RigHandle) -> Result<KnownState, StationError>` [@ANCHOR: radioctl:radioctl_station_actorlink_unkey_pass]

Runs on a newly spawned actor before it is reported up. If the actor's capabilities include `SetPtt`, it sends `Op::SetPtt(false)` with `execute_within` and the unkey timeout; this releases a transmitter that may have been left keyed by a dead link. A failure or timeout returns a `Driver` `StationError` naming the radio. A radio that cannot key skips the unkey and says so in the journal. It then reads frequency (VFO (variable frequency oscillator, the tuning control) A) and mode freshly and returns them in a `KnownState`; read failures are ignored and leave those fields unset. Settings are never restored. Async, talks to the radio.

### `fn name(&self) -> String` [@ANCHOR: radioctl:radioctl_station_livekey_name]

`KeyedSource` method of `LiveKey` that returns the radio's id as an owned `String`, a clone of the `DeviceId` text. The interlock uses it to say which radio a keyed-state answer belongs to. No arguments besides `&self`, cannot fail, takes no lock, does no I/O.

### `fn key_state(&self) -> BoxFuture<'_, KeyState>` [@ANCHOR: radioctl:radioctl_station_livekey_key_state]

Reports the keyed state of the radio's current actor for the station interlock. Each call looks the slot up under the mutex (the lock is released before awaiting), takes a clone of the handle and whether its transport is up, and then awaits the handle's own `key_state`. With no actor, or while the transport is down or retrying, it returns `KeyState::Unknown` rather than a guess. Async; may query the actor but does not itself switch anything. Cannot fail.

### `fn link_up(&self, radio: &DeviceId) -> BoxFuture<'_, bool>` [@ANCHOR: radioctl:radioctl_station_actorlink_link_up]

`RadioLink` method: true when the radio's slot is `Up`, meaning the actor's link is up and its transport, if it has a status, is connected. `radio` is borrowed. The state is computed immediately under the mutex and the returned future just yields that boolean, so there is no I/O and it cannot fail. A missing, dead or retrying slot all give false. The manager polls it every tick.

### `fn present(&self, _radio: &DeviceId) -> BoxFuture<'_, Option<bool>>` [@ANCHOR: radioctl:radioctl_station_actorlink_present]

`RadioLink` method for presence evidence such as hotplug or a ping. A bare actor has none, so it always resolves to `None`, meaning no opinion. `_radio` is ignored. No I/O, no locking, cannot fail. The manager then relies on `link_up` and connection attempts alone.

### `fn connect(&self, radio: &DeviceId) -> BoxFuture<'_, Result<Option<KnownState>, StationError>>` [@ANCHOR: radioctl:radioctl_station_actorlink_connect]

One connection attempt, called by the manager. It first takes the async connect lock, so two ticks never spawn two actors for one radio. If the slot is `Up` it returns `Ok(None)`. While the transport is `Retrying` on its own it journals this and returns a `Driver` error ("the transport is still reconnecting"), which makes the manager back off. Otherwise it spawns an actor through the spawner (errors pass through), counts the spawn, runs the unkey pass, and only on success stores the new slot (replacing any dead one) and returns `Ok(Some(KnownState))`. A failed unkey is journalled, the new actor is dropped, and the error returned. Async; opens the transport and talks to the radio.

## `crates/radioctl-station/src/ws.rs`

### `pub fn sha1(data: &[u8]) -> [u8; 20]` [@ANCHOR: radioctl:radioctl_station_ws_sha1]

Computes the SHA-1 digest of `data` (borrowed bytes, any length) by the standard algorithm: pad with 0x80, zeros and the 64-bit big-endian bit length, then process 64-byte blocks. Returns the 20-byte digest. It is used only to derive the WebSocket accept key, which is not a security boundary, so SHA-1's weakness does not matter here. Pure, cannot fail or panic (indexing is done with checked accessors), no I/O. Allocates a padded copy of the input.

### `pub fn base64(data: &[u8]) -> String` [@ANCHOR: radioctl:radioctl_station_ws_base64]

Encodes `data` as standard base64 (alphabet with `+` and `/`) with `=` padding, as the WebSocket handshake requires. `data` is borrowed and may be empty, which gives an empty string. Returns a new `String` whose length is four characters per started group of three bytes. Pure, cannot fail, no I/O.

### `pub fn accept_key(key: &str) -> String` [@ANCHOR: radioctl:radioctl_station_ws_accept_key]

Computes the `Sec-WebSocket-Accept` value for a client's `Sec-WebSocket-Key`: the key text followed by the WebSocket GUID constant, hashed with SHA-1 and base64 encoded. `key` is borrowed and used as given, with no validation. Returns a new `String`. Pure, cannot fail, no I/O. Used to check the server's handshake answer and to build expected values in tests.

### `pub fn random_bytes<const N: usize>() -> [u8; N]` [@ANCHOR: radioctl:radioctl_station_ws_random]

Returns `N` bytes that differ from run to run, taken eight at a time from the seeds of the standard library's `RandomState` hasher (each chunk hashes `N`). The WebSocket mask and key only have to be unpredictable to caching proxies, so they are not secrets and this is not suitable for cryptography. No arguments; `N` is the compile-time length. Cannot fail, no I/O, no shared state.

### `pub fn client_key() -> String` [@ANCHOR: radioctl:radioctl_station_ws_client_key]

Makes a fresh `Sec-WebSocket-Key` for a client handshake: 16 random bytes from `random_bytes`, base64 encoded (24 characters). No arguments. Different on every call. Not a secret. Cannot fail, no I/O. The caller keeps the value to check the server's answer with `check_handshake_response`.

### `pub fn handshake_request(host: &str, path: &str, key: &str) -> String` [@ANCHOR: radioctl:radioctl_station_ws_handshake_request]

Builds the HTTP/1.1 upgrade request of a WebSocket client: a `GET` for `path` with `Host`, `Upgrade: websocket`, `Connection: Upgrade`, the given `Sec-WebSocket-Key` and version 13, ending in a blank line. All three arguments are borrowed and inserted as given, with no escaping or validation. The token is deliberately not part of it, because Home Assistant authenticates inside the WebSocket. Returns the text to send; cannot fail, no I/O.

### `pub fn check_handshake_response(head: &str, key: &str) -> Result<(), String>` [@ANCHOR: radioctl:radioctl_station_ws_check_response]

Checks the server's answer to the upgrade. `head` is the response header text (borrowed) and `key` the client key that was sent. Requires the first line to contain status 101, then a `Sec-WebSocket-Accept` header (name matched without regard to case) equal to `accept_key(key)`. Errors, as human-readable `String`s: the server answered something else (quoting the status line), no accept key was sent, or the accept key is wrong. Pure, no I/O.

### `fn code(self) -> u8` [@ANCHOR: radioctl:radioctl_station_ws_opcode_code]

Returns the wire value of an `Opcode`: Continuation 0, Text 1, Binary 2, Close 8, Ping 9, Pong 10. `self` is copied. Pure, total over the enum so it cannot fail, no I/O. Used by `encode_frame` for the low four bits of the first frame byte.

### `fn from_code(code: u8) -> Option<Opcode>` [@ANCHOR: radioctl:radioctl_station_ws_opcode_from_code]

Maps a wire opcode value (the low four bits of the first frame byte, already masked by the caller) back to an `Opcode`: 0, 1, 2, 8, 9 and 10 are known. Any other value, including the reserved ones, returns `None`, which the decoder turns into an "unknown opcode" error. Pure, cannot fail or panic, no I/O.

### `pub fn encode_frame(opcode: Opcode, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_station_ws_encode]

Encodes one final (FIN set, never fragmented) frame. `opcode` is the frame type, `payload` borrowed bytes. A client passes `Some(mask)` (four bytes, normally from `random_bytes`), which sets the mask bit, appends the mask and XORs the payload with it; a server passes `None`. The length is written in one byte below 126, otherwise as 126 plus a 16-bit length, or 127 plus a 64-bit length. Returns the new byte vector. Pure, cannot fail, no I/O. It does not limit control-frame size; the caller must keep those at 125 bytes or less.

### `pub fn new(max_message: usize) -> Decoder` [@ANCHOR: radioctl:radioctl_station_ws_decoder_new]

Creates an empty frame decoder. `max_message` is a size in bytes: the decoder refuses any single frame whose declared length, or any fragmented message whose joined length, exceeds it. This bounds memory against a hostile or broken peer. The new decoder holds no buffered bytes and no partial message. Cannot fail, no I/O.

### `pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Message>, String>` [@ANCHOR: radioctl:radioctl_station_ws_decoder_push]

Appends received `bytes` to the decoder's buffer (mutating it) and returns every message that is now complete, in order, possibly none. Text and binary fragments are joined; control frames (ping, pong, close) come out on their own and must be final and at most 125 bytes. An `Err(String)` means the stream is broken or hostile and the connection must be dropped: a fragmented or oversize control frame, a new data message inside a fragmented one, a continuation with nothing to continue, a joined message over the limit, or any error from the frame parser. Incomplete trailing bytes stay buffered for the next call. Pure, no I/O. Not safe to share between threads without a lock.

### `fn frame(&self) -> Result<Option<(Frame, usize)>, String>` [@ANCHOR: radioctl:radioctl_station_ws_decoder_frame]

Parses the first complete frame in the buffer without consuming it. Returns `Ok(None)` when the buffer does not yet hold a whole frame (header, extended length, mask or payload missing), otherwise the frame as (final flag, opcode, unmasked payload) and the number of bytes it used, which `push` then drains. Errors as `String`: a reserved bit is set (no extensions are negotiated), an unknown opcode, an absurd 64-bit length, or a frame longer than `max_message`. Masked payloads are unmasked with the 4-byte key. Pure, no I/O.
