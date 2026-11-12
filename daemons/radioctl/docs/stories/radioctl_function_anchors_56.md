# radioctl function anchors, batch 56

Function anchors for the `radioctl-protocols` crate that still had one-line stubs: the Kenwood TS-890S LAN session in `kenwood_ts890_lan.rs`. `Ts890LanFraming` implements `radioctl_core::net::Framing` for the radio's LAN dialect: the `##CN;` connection request, the `##ID` login with the account and password, and the `ID;` keep-alive, in front of the ordinary TS-890S CAT stream. `LanCredentials` holds the account. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.

Transmit safety in this batch: nothing here keys a transmitter. The framing only logs in, passes CAT bytes through and sends the read-only `ID;` probe; it never builds a CAT command, and the transmit gate and every keying decision stay with the rig actor and the protocol engine above it. The only secret handled is the LAN password, which `Debug` hides and which is sent only in the login frame.

## `crates/radioctl-protocols/src/kenwood_ts890_lan.rs`

### `pub fn new(administrator: bool, account: impl Into<String>, password: impl Into<String>) -> Result<LanCredentials, RigError>` [@ANCHOR: radioctl:radioctl_ts890lan_credentials_new]

Builds the LAN account record for the TS-890S login. `administrator` picks the account type (true is the administrator account, false a user account); `account` and `password` are any `Into<String>` and are taken by value, so the credentials struct owns its own copies. Each must be 1 to 32 printable ASCII characters (0x20 to 0x7E) and must not contain `;`. The check is made by building the login frame once with `lan_login` and discarding it. Returns `Err` of kind `Unsupported` for an empty, over-long, non-ASCII or `;`-bearing value; there are no panics, no I/O and no locking. The caller (the application, which holds the secrets) is responsible for where the values come from.

### `fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result` [@ANCHOR: radioctl:radioctl_ts890lan_credentials_debug]

The `Debug` form of `LanCredentials`: it prints `LanCredentials { administrator: <bool>, account: "<name>", password: <hidden> }`. The account name is shown, quoted; the password is replaced by the literal text `<hidden>` and is never written. `self` is borrowed and the result is the formatter's own result (an error only if the sink fails). Because `Ts890LanFraming` also derives `Debug`, this keeps the password out of any log that prints a framing. Pure, no I/O, no panics.

### `pub fn new(credentials: LanCredentials) -> Ts890LanFraming` [@ANCHOR: radioctl:radioctl_ts890lan_new]

Creates the TS-890S LAN session state machine used as a `radioctl_core::net::Framing`. `credentials` is moved in and kept for every login. The session starts in the first phase (waiting for the answer to the connection request), with an empty receive buffer, no keep-alive probe outstanding, and a keep-alive interval of `KEEPALIVE_EVERY` (4 seconds, well inside the radio's 10 second idle limit). Infallible, no I/O, no panics. The returned value is passed to the transport (`net::open_link`) as a boxed framing; this layer never opens a socket and the `Never run against a radio` note of the module applies.

### `pub fn with_keepalive(mut self, every: Duration) -> Ts890LanFraming` [@ANCHOR: radioctl:radioctl_ts890lan_with_keepalive]

Builder method that replaces the keep-alive interval. `every` is the time between keep-alive probes, as a `Duration`; `self` is consumed and returned so it chains after `new`. It exists for tests against a fake radio with a short idle limit; a real radio drops the connection after 10 seconds without traffic, so a value of 10 seconds or more would let it lapse. No validation is made: a zero duration is stored as is. It changes only what `keepalive` reports. Infallible, no I/O.

### `fn frame(&mut self, frame: &[u8], out: &mut Decoded)` [@ANCHOR: radioctl:radioctl_ts890lan_handle_frame]

Private worker of `decode`: handles one complete `;`-terminated frame according to the current phase, writing its effect into `out` (a `Decoded` with `reply`, `ready`, `failure` and `payload` fields). The frame is read as lossy UTF-8. While waiting for the connection answer, `##CN1;` sets `out.reply` to the login frame (`##ID` plus account type, two-digit lengths, account and password) and moves to the login phase, `##CN0;` sets a failure that says the radio denied the connection because one already exists, and any other frame is ignored. While waiting for the login answer, `##ID1;` sets `out.ready` and moves to the up phase, `##ID0;` sets a failure for a wrong account or password, and other frames are ignored. In the up phase a frame that starts with `ID` and ends with `;` is swallowed if a keep-alive probe is outstanding (the count drops by one); everything else is appended to `out.payload`. A login-frame build error becomes a failure with its detail. No I/O, no panics.

### `fn reset(&mut self)` [@ANCHOR: radioctl:radioctl_ts890lan_reset]

Framing hook called at the start of every connection and reconnection. It returns the session to the first phase (waiting for the answer to `##CN;`), empties the partial-frame buffer and forgets any outstanding keep-alive probes, so a reconnect always logs in again from the start. The credentials and the keep-alive interval are kept. `self` is mutated in place; infallible, no I/O, idempotent.

### `fn greeting(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ts890lan_greeting]

Framing hook returning the bytes the transport sends first on every connection: the LAN connection request `##CN;` (built by `lan_connect_request`), a fresh vector owned by the caller. The radio answers `##CN1;` (authorised) or `##CN0;` (denied), which `decode` handles. It does not touch the session state and does not send anything itself; the transport writes the bytes. Infallible, no I/O.

### `fn needs_handshake(&self) -> bool` [@ANCHOR: radioctl:radioctl_ts890lan_needs_handshake]

Framing hook telling the transport whether the link is up only after the radio answers. Always returns true for this session: the link counts as up only when `decode` reports `ready`, which happens after the radio accepts the login (`##ID1;`). Until then CAT commands should not be sent. Pure, no state read, no I/O.

### `fn encode(&mut self, payload: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ts890lan_encode]

Framing hook that wraps bytes from the actor for the wire. This session adds nothing after the login: it returns a copy of `payload` unchanged (CAT text such as `FA;` goes out as is). `payload` is borrowed and the returned vector is owned by the caller. It does not check the session phase; the transport is expected not to send CAT before the handshake completes. Infallible, no I/O.

### `fn decode(&mut self, wire: &[u8]) -> Decoded` [@ANCHOR: radioctl:radioctl_ts890lan_decode]

Framing hook that unwraps bytes read from the socket; `wire` may be any slice, including a partial frame or several frames. The bytes are appended to the internal buffer, and each complete `;`-terminated frame is removed and handed to the phase handler, so a frame split across reads is reassembled. The returned `Decoded` carries the login reply to send, the ready flag, the CAT payload to pass up, and an optional failure text. After the first failure the buffer is cleared and the call returns at once. A buffer of more than 4096 bytes without a `;` is cleared and reported as a failure (`a LAN frame has no end`). Mutates the session; no I/O, no panics. Order matters: it must be fed the stream in arrival order.

### `fn keepalive(&self) -> Option<(Duration, Vec<u8>)>` [@ANCHOR: radioctl:radioctl_ts890lan_keepalive]

Framing hook describing the keep-alive the transport must send: always `Some((interval, b"ID;".to_vec()))`, where the interval is the configured one (4 seconds by default). `ID;` is the read-only identification command, so the probe never changes the radio and never keys it. It is meant to keep the radio from closing the TCP connection after 10 seconds without communication. The caller owns the returned bytes. Pure, no I/O.

### `fn keepalive_sent(&mut self)` [@ANCHOR: radioctl:radioctl_ts890lan_keepalive_sent]

Framing hook the transport calls after each keep-alive probe has been written. It adds one to the count of outstanding probes (saturating at the largest `u32`), so that `decode` can recognise and swallow the radio's matching `ID...;` answer in the up phase instead of passing it to the CAT engine. Without outstanding probes an `ID` answer passes through as ordinary payload. Mutates the session; infallible, no I/O.
