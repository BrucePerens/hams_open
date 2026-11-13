# radioctl function anchors, batch 54

Function anchors for the `radioctl-core` crate: the network link layer in `net.rs` (the `NetConfig` constructor, the `Framing` trait defaults, `RawFraming`, `LengthFraming`, the `NetLink` and `NetMonitor` accessors, the private `Wire` socket wrapper and the `Supervisor` task that opens, handshakes, keeps alive and reconnects the socket, and `open_link`) and the `ProtocolEngine` implementation for `Box<dyn ProtocolEngine>` in `engine.rs`. The 41 entries below replace the one-line stubs in the crate README.

Transmit safety in this batch: nothing here keys a transmitter or sends a transmit command on its own. The network layer moves opaque bytes between the rig actor and a TCP or UDP socket; it never interprets them, so the interlock, the transmit gate and the band check live in the actor and the callers above it. The one safety property implemented here is that bytes the actor writes while the link is down or still handshaking are dropped and counted, never queued, so a stale command cannot be replayed into a radio after a reconnect; after a reconnect the actor, not this module, unkeys the rig. The `Box<dyn ProtocolEngine>` forwarders in `engine.rs` are pure delegation and add no checks of their own. The only hardware touched is a network socket.

## `crates/radioctl-core/src/net.rs`

### `pub fn new(address: impl Into<String>) -> NetConfig` [@ANCHOR: radioctl:radioctl_core_net_config_new]

Constructor of `NetConfig`: builds the default configuration for reaching a rig at `address`, which is owned (converted with `Into<String>`) and expected as `host:port`; a host name is resolved again at every connect. The result uses TCP, a three-second connect timeout, a three-second handshake timeout, a three-second write timeout, no idle timeout (`None`), `Backoff::default()` and `first_attempts` of 1. Pure, no I/O, cannot fail or panic; the address is not validated until `open_link` connects. Callers adjust the public fields afterwards, for example to select UDP or set an idle timeout.

### `fn keepalive_sent(&mut self)` [@ANCHOR: radioctl:radioctl_core_net_framing_keepalive_sent]

Default method of the `Framing` trait, called by the session loop right after each keep-alive probe was written to the socket, so a framing that expects an answer to its probe can recognise and swallow that answer in `decode` instead of passing it up as CAT (computer-aided transceiver control) data. The only argument is `&mut self`, the framing's own state. The default body does nothing and returns nothing, so a framing without a keep-alive never needs it. It is called once per probe, after a successful send; a failed send ends the session before the call. No I/O of its own, cannot fail.

### `fn reset(&mut self)` [@ANCHOR: radioctl:radioctl_core_net_raw_reset]

`Framing::reset` for `RawFraming`: forgets all framing state, which for this stateless identity framing means nothing at all, so the body is empty. It takes `&mut self` only to satisfy the trait. `open_and_handshake` calls it at the start of every connection, after the socket opens and before the greeting, so a framing never carries half a frame or a login state from an earlier connection. Idempotent, pure, no I/O, cannot fail or panic.

### `fn greeting(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_core_net_raw_greeting]

`Framing::greeting` for `RawFraming`: the bytes to send first at every connection, which for a plain CAT stream is an empty vector, so nothing is greeted and no login is sent. Takes `&mut self` (the trait allows a framing that changes state when it greets) and returns an owned `Vec<u8>`. `open_and_handshake` calls it once per connection after `reset` and sends the bytes only when the vector is not empty. Pure, no I/O, cannot fail.

### `fn needs_handshake(&self) -> bool` [@ANCHOR: radioctl:radioctl_core_net_raw_needs_handshake]

`Framing::needs_handshake` for `RawFraming`: always `false`, meaning the link is declared up as soon as the greeting (empty here) has been sent, with no wait for an answer from the radio. Borrows `self` immutably and reads nothing. `open_and_handshake` checks it after the greeting; when it is true the supervisor waits up to `handshake_timeout` for the framing's `decode` to report `ready`. Pure, idempotent, no I/O, cannot fail.

### `fn encode(&mut self, payload: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_core_net_raw_encode]

`Framing::encode` for `RawFraming`: wraps bytes from the rig actor for the wire, and for the identity framing returns a copy of `payload` unchanged. `payload` is borrowed and not modified; the result is a new owned `Vec<u8>` of the same length. The session loop calls it for each chunk (up to 4096 bytes) read from the actor's stream, then writes the result with `send`. Pure apart from the allocation, no I/O of its own, cannot fail or panic, and an empty payload gives an empty vector.

### `fn decode(&mut self, wire: &[u8]) -> Decoded` [@ANCHOR: radioctl:radioctl_core_net_raw_decode]

`Framing::decode` for `RawFraming`: unwraps bytes read from the socket, and for the identity framing returns a `Decoded` whose `payload` is a copy of `wire` and whose other fields keep their defaults: empty `reply`, `ready` false, no `failure`. `wire` is borrowed and unchanged. The session loop calls it for every read and passes a non-empty payload to the actor. Because `needs_handshake` is false, `ready` is never consulted. Pure, no I/O, cannot fail or panic; a partial chunk is simply passed on, there is nothing to reassemble.

### `fn keepalive(&self) -> Option<(Duration, Vec<u8>)>` [@ANCHOR: radioctl:radioctl_core_net_raw_keepalive]

`Framing::keepalive` for `RawFraming`: returns `None`, meaning no keep-alive is wanted, so the session sends no probes and relies only on the idle timeout, when one is configured, to notice a dead link. A framing that wants one returns the interval and the bytes to send each time. Borrows `self` immutably and is read once at the start of each session. Pure, idempotent, no I/O, cannot fail.

### `pub fn new(width: usize) -> LengthFraming` [@ANCHOR: radioctl:radioctl_core_net_length_new]

Constructor of `LengthFraming`: a framing in which every frame is a big-endian length followed by that many payload bytes. `width` is the size of the length field in bytes; only 2 is kept, and any other value, including 0, 1, 3 and 4, silently becomes 4, so there is no error path. The new value has an empty reassembly buffer. Frames longer than `MAX_FRAME` are refused by `decode`. Pure, no I/O, cannot panic. The result is passed as a `Box<dyn Framing>` to `open_link`.

### `fn reset(&mut self)` [@ANCHOR: radioctl:radioctl_core_net_length_reset]

`Framing::reset` for `LengthFraming`: clears the `pending` buffer that holds the bytes of a partly received frame, so after a reconnect the new connection starts at a frame boundary and half a frame from the old socket is never glued to the new stream. `&mut self` is mutated; nothing is returned. Called by `open_and_handshake` at the start of every connection. Idempotent, no I/O, cannot fail or panic. The width is kept.

### `fn greeting(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_core_net_length_greeting]

`Framing::greeting` for `LengthFraming`: returns an empty vector, so nothing is sent when a connection opens; this framing implements only the length prefix, not any radio's login. Takes `&mut self` but changes nothing, and the return is an owned empty `Vec<u8>`. `open_and_handshake` skips the send when the greeting is empty. Pure, no I/O, cannot fail. A radio that needs a login would use its own framing type.

### `fn needs_handshake(&self) -> bool` [@ANCHOR: radioctl:radioctl_core_net_length_needs_handshake]

`Framing::needs_handshake` for `LengthFraming`: always `false`, so the link is declared up straight after the (empty) greeting, without waiting for any answer from the radio and without using `handshake_timeout`. Borrows `self` immutably. Checked by `open_and_handshake` once per connection. Pure, idempotent, no I/O, cannot fail.

### `fn encode(&mut self, payload: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_core_net_length_encode]

`Framing::encode` for `LengthFraming`: turns one write from the actor into one frame, a big-endian length field of `width` bytes (2 or 4) followed by `payload`. `payload` is borrowed and copied into a new owned vector of `width + payload.len()` bytes. A length that does not fit the field is clamped to `u16::MAX` or `u32::MAX` rather than wrapping or failing, so a payload over 65535 bytes with a 2-byte field would produce a length that disagrees with its content; the actor writes at most 4096 bytes per chunk, so this does not arise in the session loop. Cannot fail or panic, no I/O, does not touch `pending`.

### `fn decode(&mut self, wire: &[u8]) -> Decoded` [@ANCHOR: radioctl:radioctl_core_net_length_decode]

`Framing::decode` for `LengthFraming`: appends `wire` (borrowed, any size, possibly a partial or several frames) to the internal `pending` buffer and extracts every complete frame, concatenating the frame bodies into `Decoded::payload`; an incomplete trailing frame stays in `pending` for the next call. The length is read big-endian from `width` bytes. If a length exceeds `MAX_FRAME` (1 MiB) it sets `failure` to a message such as "frame of N bytes is over the limit", clears `pending` and stops, which makes the session drop the connection and reconnect. It never allocates for the claimed length. `reply` is never set and `ready` stays false. No I/O, cannot panic.

### `fn keepalive(&self) -> Option<(Duration, Vec<u8>)>` [@ANCHOR: radioctl:radioctl_core_net_length_keepalive]

`Framing::keepalive` for `LengthFraming`: returns `None`, so no probe is sent; the length prefix says nothing about what a radio would accept as a keep-alive. A link over this framing that must detect silence therefore relies on the configured `idle_timeout`, and a quiet rig is reconnected needlessly if that is set without traffic. Borrows `self` immutably, read once per session. Pure, idempotent, no I/O, cannot fail.

### `fn snapshot(counters: &Counters) -> NetStats` [@ANCHOR: radioctl:radioctl_core_net_snapshot]

Private helper that copies the four shared atomic counters into a plain `NetStats` (the same type as `SerialStats`): `bytes_in`, `bytes_out`, `bytes_dropped` and `reconnects`. `counters` is borrowed and only read, each with a relaxed atomic load, so the four values are individually current but not a single consistent instant, and a reading may lag a concurrent update. Used by `NetLink::stats` and `NetMonitor::stats`. No locks, no I/O, cannot fail or panic, and it is safe to call at any time and any number of times.

### `pub fn status(&self) -> LinkStatus` [@ANCHOR: radioctl:radioctl_core_net_link_status]

`NetLink::status`: returns a clone of the current link status held in the watch channel, one of `Connected { port, generation }` (the port string is `tcp://address` or `udp://address`), `Reconnecting { attempt, last_error }` or `Closed`. Borrows the link, does not block and does not wait for a change, only reads the latest value. Right after `open_link` returns it is `Connected` with generation 1. Cannot fail; no network I/O. For change notification use `watch`.

### `pub fn watch(&self) -> watch::Receiver<LinkStatus>` [@ANCHOR: radioctl:radioctl_core_net_link_watch]

`NetLink::watch`: returns a new clone of the tokio watch receiver on which the supervisor publishes every `LinkStatus` change; pass it as `RigOptions::link_watch` so the rig actor refuses commands while the link is `Reconnecting` and unkeys the rig when it reports `Connected` again. The receiver is owned by the caller and independent of the link's own. Awaiting `changed()` on it wakes on each status change. Cannot fail itself; once the supervisor ends and drops the sender, `changed()` returns an error, and the last value is `Closed`.

### `pub fn stats(&self) -> NetStats` [@ANCHOR: radioctl:radioctl_core_net_link_stats]

`NetLink::stats`: returns a point-in-time copy of the link counters as `NetStats`, namely bytes read from the socket (`bytes_in`), bytes written to it (`bytes_out`), bytes dropped because the link was down, handshaking or the actor was not reading (`bytes_dropped`), and successful reconnects (`reconnects`, not counting the first connection). Borrows the link; reads atomics with relaxed ordering through `snapshot`, so it never blocks. Counters only grow. Cannot fail, no I/O, idempotent.

### `pub fn into_parts(self) -> (DuplexStream, NetMonitor)` [@ANCHOR: radioctl:radioctl_core_net_link_into_parts]

Consumes the `NetLink` and splits it into the in-memory byte stream (hand it to `crate::rig::spawn` as the transport) and a `NetMonitor` that the owner keeps for status and counters. The link is moved, so it cannot be used afterwards. Dropping the monitor does not close the socket; dropping the stream does, because the supervisor then sees the actor's end close and ends with `Closed`. Pure moves, no I/O, cannot fail or panic, and it has no effect on the supervisor task, which keeps running.

### `pub fn status(&self) -> LinkStatus` [@ANCHOR: radioctl:radioctl_core_net_monitor_status]

`NetMonitor::status`: the same as `NetLink::status` for the part of the link the owner keeps. Returns a clone of the latest `LinkStatus` (`Connected` with port and generation, `Reconnecting` with attempt count and last error text, or `Closed`) read from the watch receiver; it borrows the monitor, never waits and does no network I/O. Cannot fail. After the supervisor ends the last published value, normally `Closed`, is still returned.

### `pub fn watch(&self) -> watch::Receiver<LinkStatus>` [@ANCHOR: radioctl:radioctl_core_net_monitor_watch]

`NetMonitor::watch`: returns a fresh clone of the watch receiver for status changes, suitable for `RigOptions::link_watch` or for a UI that shows the link state. The receiver is owned by the caller and wakes on every change the supervisor publishes (`Connected`, `Reconnecting`, `Closed`). Does not block, does no I/O and cannot fail; awaiting `changed()` on it errors only once the supervisor has dropped its sender.

### `pub fn stats(&self) -> NetStats` [@ANCHOR: radioctl:radioctl_core_net_monitor_stats]

`NetMonitor::stats`: returns a snapshot of the link counters (`bytes_in`, `bytes_out`, `bytes_dropped`, `reconnects`) as `NetStats`, computed by `snapshot` from the atomics shared with the supervisor. Borrows the monitor, uses relaxed loads, does not block or do I/O and cannot fail. The four values are read one after another, so they need not be mutually consistent under concurrent traffic; they only ever grow, and the monitor stays valid after the supervisor has stopped.

### `fn net_error(detail: impl Into<String>) -> RigError` [@ANCHOR: radioctl:radioctl_core_net_error]

Private helper that builds the error this module reports for network failures: a `RigError` of kind `ErrorKind::LinkDown` carrying `detail`, which is owned through `Into<String>`. Used by `run` for the first-connection failure ("connect failed after N attempts: reason") and by `open_link` when the supervisor stops before reporting. Pure, no I/O, cannot fail or panic. Everything else in the module uses plain `String` reasons that are folded into these or into `LinkStatus::Reconnecting::last_error`.

### `async fn open(config: &NetConfig) -> Result<Wire, String>` [@ANCHOR: radioctl:radioctl_core_net_wire_open]

`Wire::open`: resolves `config.address` with `lookup_host` (taking the first address returned) and opens a socket for `config.protocol`, the whole operation bounded by `config.connect_timeout`. For TCP it connects and sets `TCP_NODELAY`; for UDP it binds an unspecified local address of the matching family on an ephemeral port and connects the socket to the peer, which only fixes the peer and sends no packet. `config` is borrowed. Errors are strings: "resolve ...", "has no address", "connect ...", "set_nodelay ...", "bind udp ...", "connect udp ..." or "no answer within <timeout>". Network I/O, async, cancel-safe because nothing is held across awaits except the new socket.

### `async fn recv(&mut self, buffer: &mut [u8]) -> std::io::Result<usize>` [@ANCHOR: radioctl:radioctl_core_net_wire_recv]

`Wire::recv`: reads whatever is available into `buffer` (mutated, its length is the maximum read): for TCP one chunk of the byte stream, for UDP one whole datagram, longer datagrams being truncated to the buffer by the operating system. Returns the number of bytes read; `Ok(0)` on TCP means the peer closed the connection. Errors are the underlying `std::io::Error` (for UDP this includes an ICMP error such as connection refused reported by the stack). Async, network I/O, no timeout of its own: the callers bound it with a handshake timeout or the session's idle deadline. Cancel-safe.

### `async fn send(&mut self, bytes: &[u8]) -> std::io::Result<()>` [@ANCHOR: radioctl:radioctl_core_net_wire_send]

`Wire::send`: writes all of `bytes` (borrowed) to the socket; on TCP with `write_all`, which may take several writes, and on UDP as exactly one datagram, whose count is discarded. Returns `Ok(())` or the underlying `std::io::Error` (broken pipe, connection refused and so on). Async network I/O with no timeout of its own, so `Supervisor::send` wraps it in `write_timeout`. A cancelled TCP write can leave part of a frame on the wire, which is why a send timeout ends the connection rather than being retried.

### `async fn sleep_until_opt(at: Option<Instant>)` [@ANCHOR: radioctl:radioctl_core_net_sleep_until]

Private helper for the session's `select!`: resolves when the tokio `Instant` `at` is reached, or never when `at` is `None`, by awaiting a pending future that stays pending forever. Used so that the keep-alive deadline and the silence deadline can be optional branches of one loop; a disabled branch simply never fires. An `at` already in the past resolves immediately. Takes the instant by value, returns nothing, cannot fail. Cancel-safe, no I/O.

### `async fn run(mut self, ours: DuplexStream, first: oneshot::Sender<Result<(), RigError>>)` [@ANCHOR: radioctl:radioctl_core_net_supervisor_run]

`Supervisor::run`, the supervisor task spawned by `open_link`; it owns the `Supervisor` (consumed), the supervisor's end `ours` of the in-memory stream (split into read and write halves) and the one-shot `first` used to report the first connection. It loops: `attempt`, then on success it counts a reconnect (from the second connection on), publishes `Connected` with an increasing generation, answers `first` with `Ok(())` and runs `session`. A failed attempt or a lost session increments the failure count; while the first connection is still pending and failures reach `first_attempts` it sends `first` an `Err` of kind `LinkDown` and ends. Otherwise it publishes `Reconnecting { attempt, last_error }` and waits `backoff.delay(failures)` in `sleep_dropping`. It ends, publishing `Closed`, when the actor drops its end of the stream. Never returns a value; network I/O, runs until closed.

### `async fn sleep_dropping(&self, from_actor: &mut ReadHalf<DuplexStream>, delay: Duration) -> bool` [@ANCHOR: radioctl:radioctl_core_net_sleep_dropping]

`Supervisor::sleep_dropping`: waits out the reconnect backoff `delay` while still reading from the actor's stream (`from_actor`, mutated by the reads) into a 512-byte buffer and discarding what it reads, adding the byte count to `bytes_dropped`, so the actor's commands are never queued for replay after the reconnect. Returns `true` when the delay has elapsed and `false` when the actor's end is closed or errors, which tells `run` to stop. Async; no network I/O, only the in-memory stream and a timer; cancel-safe.

### `async fn attempt(&mut self, from_actor: &mut ReadHalf<DuplexStream>) -> Result<(Wire, Vec<u8>), AttemptEnd>` [@ANCHOR: radioctl:radioctl_core_net_attempt]

`Supervisor::attempt`: makes one connection attempt by driving `open_and_handshake` while concurrently draining and counting whatever the actor writes (`from_actor`, mutated), so nothing the actor sends during the attempt reaches the radio. On success returns the open `Wire` plus any payload bytes that arrived with the handshake's last read; on failure `AttemptEnd::Failed(reason)`, and `AttemptEnd::Shutdown` if the actor closed its end meanwhile. The dropped count is added to `bytes_dropped` after the attempt (not on the shutdown path). Async network I/O bounded by the connect, handshake and write timeouts.

### `async fn open_and_handshake(&mut self) -> Result<(Wire, Vec<u8>), String>` [@ANCHOR: radioctl:radioctl_core_net_open_and_handshake]

`Supervisor::open_and_handshake`, the cancel-safe core of an attempt: opens the socket with `Wire::open`, calls `framing.reset()`, sends the framing's greeting when it is not empty, and returns at once if the framing needs no handshake. Otherwise it reads until `handshake_timeout` (one deadline for the whole handshake), decodes each read, sends any `reply`, and returns when `ready` is set, with the payload decoded in that read. Errors as strings: "handshake timed out", "handshake read: ...", "closed during handshake", "handshake failed: <reason>" and write errors from `send`. Network I/O; dropping the future simply drops the socket.

### `async fn send(&self, wire: &mut Wire, bytes: &[u8]) -> Result<(), String>` [@ANCHOR: radioctl:radioctl_core_net_send]

`Supervisor::send`: writes `bytes` to `wire` (mutated) bounded by `config.write_timeout`. On success it adds `bytes.len()` to `bytes_out` and returns `Ok(())`. Errors as strings: "write timed out after <timeout>" or "write: <io error>". The timeout is one the caller treats as a lost link, since a cancelled TCP write may have sent only part of the data. Used for greetings, handshake replies, keep-alive probes and framed actor data. Async network I/O; the counter update is a relaxed atomic.

### `async fn session(&mut self, wire: Wire, leftover: Vec<u8>, from_actor: &mut ReadHalf<DuplexStream>, to_actor: &mut WriteHalf<DuplexStream>) -> SessionEnd` [@ANCHOR: radioctl:radioctl_core_net_session]

`Supervisor::session`: runs one ready connection until it ends. It first delivers `leftover` to the actor, then loops over four events: data from the wire (counted in `bytes_in`, decoded, any `reply` sent back, payload delivered to the actor), data from the actor (encoded and sent; if that send fails the bytes are added to `bytes_dropped`), the keep-alive timer (probe sent, then `keepalive_sent`, timer rearmed) and the silence deadline (`idle_timeout`, re-armed by every read). Returns `SessionEnd::Lost(reason)` for a closed connection, a read or write error, a framing failure or silence, and `SessionEnd::Shutdown` when the actor's end is closed. Async network I/O; a framing failure drops the connection.

### `async fn deliver(&self, to_actor: &mut WriteHalf<DuplexStream>, bytes: &[u8]) -> bool` [@ANCHOR: radioctl:radioctl_core_net_deliver]

`Supervisor::deliver`: writes `bytes` to the actor's in-memory stream (`to_actor`, mutated), waiting at most `write_timeout`. Returns `true` when the bytes were written, and also `true` after a timeout, in which case the bytes are discarded and counted in `bytes_dropped` so an actor that is not reading cannot stall the link; returns `false` when the write fails because the actor's end is gone, which the session treats as shutdown. Async, no network I/O, cannot panic.

### `fn proto_name(protocol: NetProtocol) -> &'static str` [@ANCHOR: radioctl:radioctl_core_net_proto_name]

Private helper that maps a `NetProtocol` (by value, `Copy`) to its lower-case name, `"tcp"` or `"udp"`. It is used to build the port string in `LinkStatus::Connected` (`tcp://host:port`) and the first-connection failure message ("tcp connect failed after N attempts"). Pure, no I/O, cannot fail or panic, and the match is exhaustive over the two protocols.

### `fn read_size(protocol: NetProtocol) -> usize` [@ANCHOR: radioctl:radioctl_core_net_read_size]

Private helper returning the size of the receive buffer for a protocol: 4096 bytes for TCP and 65,536 for UDP, so a UDP read can take any datagram in one piece while a TCP read takes a chunk of the stream. `protocol` is passed by value. Used by `open_and_handshake` and `session` to size their wire buffer. Pure, no I/O, cannot fail or panic.

### `pub async fn open_link(runtime: &Handle, config: NetConfig, framing: Box<dyn Framing>) -> Result<NetLink, RigError>` [@ANCHOR: radioctl:radioctl_core_net_open_link]

Opens a network link: creates the 4096-byte in-memory stream pair, the status watch (initially `Reconnecting` attempt 0, "not connected yet") and the counters, spawns the supervisor on `runtime` (borrowed handle) and waits for it to report the first connection. `config` and `framing` are owned; use `Box::new(RawFraming)` for plain CAT. Returns a `NetLink` once connected, or the supervisor's error, an `ErrorKind::LinkDown` carrying the last attempt's reason after `config.first_attempts` tries, or `LinkDown` "network supervisor stopped before connecting". After success the supervisor reconnects without limit until the stream is dropped. Network I/O; nothing is transmitted beyond the framing's greeting.

## `crates/radioctl-core/src/engine.rs`

### `fn description(&self) -> &RigDescription` [@ANCHOR: radioctl:radioctl_core_engine_box_description]

`ProtocolEngine::description` for `Box<dyn ProtocolEngine>`: forwards to the boxed engine and returns a borrow of the `RigDescription` it was built from, so `rig::spawn` can take any protocol family through one boxed type. The borrow lives as long as the box. Pure, no side effects, no I/O, cannot fail or panic, and it is idempotent: the description does not change while the engine runs.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_core_engine_box_encode]

`ProtocolEngine::encode` for `Box<dyn ProtocolEngine>`: forwards to the boxed engine's `encode`, so the result and errors are the engine's own. `op` is the borrowed command; the return is a `Request` (the bytes to write and the `ReplyKind` to wait for) or a `RigError` saying why the rig cannot do the command (for example an unsupported operation). It adds no check of its own; the transmit gate and unkey logic stay in the actor. `&mut self` because engines keep state. No I/O, no panic of its own.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_core_engine_box_feed]

`ProtocolEngine::feed` for `Box<dyn ProtocolEngine>`: forwards to the boxed engine, passing received `bytes` (borrowed, possibly a partial frame) and `now`, the caller's monotonic time as a `Duration`, and returns the engine's decoded `Inbound` items (observations, scope frames, `Ack`, `Nak` or unrecognised bytes) in order, an empty vector when nothing is complete yet. The engine buffers partial input between calls. `&mut self`; no I/O, no errors returned, and no behaviour added by the box.
