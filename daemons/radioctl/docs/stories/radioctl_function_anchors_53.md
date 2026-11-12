# radioctl function anchors, batch 53

Function anchors for the `radioctl-station` crate in `rotator.rs`: the `RotatorDescription` (the validated configuration of one antenna rotator and its travel), the sans-IO protocol engines `RotctldEngine` (Hamlib rotctld text protocol) and `Gs232Engine` (Yaesu GS-232A/B), and the async `Rotator`, which runs one command and its reply over a `Transport` with a timeout.

Transmit safety in this batch: a rotator moves an antenna; it does not key a transmitter and no code here switches a ptt line. Movement is still bounded: `encode` refuses a position outside the description's travel with a `Refused` error and never clamps it, and `stop` has no interlock so it works even when everything else is refused. No transmit gate or interlock applies in this file. The only lock is the `Rotator`'s async mutex over its transport, which serialises one command and reply at a time; a second caller waits at most the reply timeout and then gets a `Timeout` error.

## `crates/radioctl-station/src/rotator.rs`

### `fn rot_err(kind: StationErrorKind, m: impl Into<String>) -> StationError` [@ANCHOR: radioctl:radioctl_station_rot_err]

Private helper that builds a `StationError` of the given `kind` with the message `m` (any value convertible to a string, taken by value). Every error of this file goes through it. Pure, no I/O, no lock, cannot fail or panic.

### `pub fn validate(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_rot_desc_validate]

Checks one `RotatorDescription` on its own, without touching hardware. It requires a non-empty id and finite limits and slew rate; azimuth limits ordered with a travel of at most 720 degrees; elevation limits ordered within -90 to 180; an azimuth-only rotator (`has_elevation` false) must have elevation 0 to 0; the slew rate positive and the reply timeout between 1 and 60000 ms; a GS-232 rotator within azimuth 0 to 450 and elevation 0 to 180; and a park position, when given, inside the travel. Returns `Ok(())`, or a `Config` error naming the rotator id and the first rule broken. `&self` is only read; no I/O, no lock. Called by `parse_all` and `engine`.

### `pub fn contains(&self, az: f32, el: f32) -> bool` [@ANCHOR: radioctl:radioctl_station_rot_desc_contains]

Says whether the position `az`, `el` (degrees) lies inside the rotator's travel, limits inclusive. A NaN or infinite value is outside. Returns a bool; cannot fail or panic; pure, no I/O, no lock. It is the one place the travel is tested, used by `target` (so by both engines' `encode`) and by the virtual rotator.

### `pub fn parse_all(text: &str) -> Result<Vec<RotatorDescription>, StationError>` [@ANCHOR: radioctl:radioctl_station_rot_desc_parse]

Parses a TOML text containing `[[rotators]]` tables into descriptions and validates every one. `text` is borrowed. Errors, all of kind `Config`: the text does not parse as the expected TOML (the message begins "rotators:" and carries the parser's text), a description fails `validate`, or two descriptions share an id ("is defined twice"). Returns the descriptions in file order. Pure; reading the file is the caller's job; no lock.

### `pub fn builtin() -> Vec<RotatorDescription>` [@ANCHOR: radioctl:radioctl_station_rot_desc_builtin]

Returns the descriptions shipped inside the program, parsed from the built-in TOML text by `parse_all`. No arguments. If the built-in text were invalid the result is an empty list rather than an error (`unwrap_or_default`), so it cannot fail or panic. No I/O, no lock. Each call parses again and returns a new vector.

### `pub fn engine(&self) -> Result<Box<dyn RotatorEngine>, StationError>` [@ANCHOR: radioctl:radioctl_station_rot_desc_engine]

Builds the protocol engine for this description: a `RotctldEngine` for `Rotctld` and a `Gs232Engine` for `Gs232`, each holding a clone of the description. It calls `validate` first and returns its `Config` error if the description is invalid, so an engine never exists for a bad description. `&self` borrowed; returns an owned boxed engine. No I/O, no lock.

### `fn target(desc: &RotatorDescription, op: &RotOp) -> Result<Option<(f32, f32)>, StationError>` [@ANCHOR: radioctl:radioctl_station_rot_resolve]

Private: works out the position a command means. `SetPosition` yields `Some((az, el))` after the elevation is forced to 0 for an azimuth-only rotator and the position is checked with `contains`; outside the travel it is a `Refused` error naming the rotator and its limits (the value is never clamped). `Park` yields the description's park position, or a `Refused` error when there is none. `GetPosition` and `Stop` yield `None`. Both arguments are borrowed; pure, no I/O, no lock.

### `fn line(buf: &[u8]) -> Option<(String, usize)>` [@ANCHOR: radioctl:radioctl_station_rot_line]

Private: extracts the first complete line from `buf`, the bytes received so far. Leading CR and LF bytes are skipped; the line ends at the next CR or LF. Returns the trimmed text (invalid UTF-8 is replaced) and the number of bytes consumed including the terminator, or `None` when no terminator has arrived yet, meaning more bytes are needed. Cannot fail or panic; pure; no lock.

### `pub fn new(desc: RotatorDescription) -> RotctldEngine` [@ANCHOR: radioctl:radioctl_station_rotctld_new]

Creates the rotctld protocol engine for `desc`, taken by value. It does not validate; `RotatorDescription::engine` validates before calling it. Holds no connection and does no I/O; the engine is sans-IO and stateless apart from the description. Cannot fail or panic.

### `fn rprt_text(code: i32) -> &'static str` [@ANCHOR: radioctl:radioctl_station_rotctld_rprt]

Private: turns a Hamlib `RPRT` status code into words for error messages: -1 invalid parameter, -2 invalid configuration, -4 function not implemented, -5 communication timed out, -6 I/O error, -8 protocol error, -9 command rejected by the rotator, -11 function not available, -17 value out of the rotator's domain, and "error" for any other code. `code` is the negative integer the rotator returned. Pure, cannot fail, no I/O.

### `fn description(&self) -> &RotatorDescription` [@ANCHOR: radioctl:radioctl_station_rotctld_description]

Implementation of `RotatorEngine::description` for the rotctld engine: returns a borrow of the description the engine was built with. No arguments besides `self`, no side effects, cannot fail, takes no lock, performs no I/O, and the borrow lives as long as the engine itself.

### `fn encode(&self, op: &RotOp) -> Result<Encoded, StationError>` [@ANCHOR: radioctl:radioctl_station_rotctld_encode]

Encodes one command for rotctld. `op` is borrowed. `SetPosition` and `Park` become `P az el\n` with two decimals and expect a status reply; `GetPosition` becomes `p\n` and expects a position; `Stop` becomes `S\n` and expects a status. The position is first checked by `target`, so a position outside the travel, or a park with no park position, returns a `Refused` error and nothing is encoded. Pure, no I/O, no lock; the same input gives the same bytes.

### `fn parse(&self, expect: RotExpect, buf: &[u8]) -> Result<Option<(RotReply, usize)>, StationError>` [@ANCHOR: radioctl:radioctl_station_rotctld_parse]

Parses a rotctld reply from the start of `buf` according to `expect`. Returns `Ok(None)` when the line or lines are not complete yet, otherwise the reply and the bytes used. `Nothing` is done at once using 0 bytes. `Status` reads one line that must be `RPRT n`: `RPRT 0` gives `Done`; any other code is a `Driver` error carrying the code and its text from `rprt_text`; anything else is a `Driver` "unexpected reply" error. `Position` reads two lines, azimuth and elevation, each a finite number, giving `RotReply::Position`; a first line starting with `RPRT` is an error either way, and a non-number is a `Driver` error. Pure; no I/O; no lock.

### `pub fn new(desc: RotatorDescription) -> Gs232Engine` [@ANCHOR: radioctl:radioctl_station_gs232_new]

Creates the Yaesu GS-232 protocol engine for `desc`, taken by value. It does not validate (the `engine` method of the description does that first). No I/O, no state beyond the description, cannot fail or panic.

### `fn gs232_position(text: &str) -> Option<(f32, f32)>` [@ANCHOR: radioctl:radioctl_station_gs232_parse_position]

Private: parses a GS-232 position reply line into azimuth and elevation in degrees. It accepts the signed form `+0aaa+eee` (a leading plus on each field, the azimuth's leading zero optional) and the form `AZ=aaa EL=eee`. Each field must be one to four digits with no other characters. `text` is the borrowed, trimmed line. Returns `None` for anything else; the caller turns that into a `Driver` error. Pure, cannot fail or panic, no I/O.

### `fn description(&self) -> &RotatorDescription` [@ANCHOR: radioctl:radioctl_station_gs232_description]

Implementation of `RotatorEngine::description` for the GS-232 engine: returns a borrow of the description the engine was built with. No other arguments, no side effects, cannot fail, takes no lock, performs no I/O, and the borrow lives as long as the engine itself.

### `fn encode(&self, op: &RotOp) -> Result<Encoded, StationError>` [@ANCHOR: radioctl:radioctl_station_gs232_encode]

Encodes one command for GS-232. `op` is borrowed. `SetPosition` and `Park` become `Waaa eee` plus a carriage return, both numbers rounded to whole degrees and padded to three digits, and expect no reply; `GetPosition` becomes `C2` plus CR and expects a position; `Stop` becomes `S` plus CR with no reply. The position is checked by `target` first, so an out-of-travel position or a missing park position gives a `Refused` error and nothing is encoded. Pure, no I/O, no lock.

### `fn parse(&self, expect: RotExpect, buf: &[u8]) -> Result<Option<(RotReply, usize)>, StationError>` [@ANCHOR: radioctl:radioctl_station_gs232_parse]

Parses a GS-232 reply from the start of `buf` according to `expect`. `Nothing` is done at once with 0 bytes used (GS-232 acknowledges movement commands with nothing). `Status` is always a `Driver` error ("GS-232 has no status replies"), since this protocol has none. `Position` takes the first complete line and parses it with `gs232_position`, returning `Ok(None)` when no full line has arrived and a `Driver` "unexpected reply" error when the line is not a position. Pure, no I/O, no lock.

### `pub fn new(engine: Box<dyn RotatorEngine>, transport: impl Transport, link: Option<watch::Receiver<LinkStatus>>) -> Rotator` [@ANCHOR: radioctl:radioctl_station_rotator_new]

Builds a `Rotator` that drives `engine` over `transport`. The engine is owned; the transport is boxed and put behind an async mutex. `link` is the transport's link status receiver (for example from `NetLink::watch`); when given, a link that is not connected makes each command fail at once with a `Driver` error instead of writing into a dead stream; `None` means the link is always assumed up. Nothing is sent. Cannot fail, no I/O.

### `pub fn description(&self) -> &RotatorDescription` [@ANCHOR: radioctl:radioctl_station_rotator_description]

Returns a borrow of the description of the rotator's engine: id, travel, protocol, reply timeout and so on. No other arguments, no I/O, no lock taken, cannot fail, and the borrow lives as long as the rotator itself, so callers read it freely.

### `pub async fn set_position(&self, az: f32, el: f32) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_rotator_set_position]

Commands the rotator to azimuth `az` and elevation `el`, in degrees, by sending a set-position command and waiting for its reply when the protocol has one. It returns when the rotator has accepted the command, not when it has arrived; `wait_for` waits for arrival. Errors: `Refused` if the position is outside the travel (never clamped; elevation is ignored for an azimuth-only rotator), `Driver` for a down link, a write or read failure, or an error status from the rotator, `Timeout` if it is busy with another command or does not answer within the reply timeout. Async; serial or network I/O under the transport mutex. Setting the same position again is harmless.

### `pub async fn position(&self) -> Result<(f32, f32), StationError>` [@ANCHOR: radioctl:radioctl_station_rotator_position]

Reads the rotator's current position and returns `(azimuth, elevation)` in degrees. No arguments besides `self`. Errors: `Driver` for a down link, a failed write or read, a reply that is not a position, or an unexpected reply with no position in it; `Timeout` if the transport is busy or the rotator does not answer within the reply timeout. Async; one request and reply under the transport mutex; it does not move the rotator and may be repeated freely.

### `pub async fn stop(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_rotator_stop]

Stops rotation by sending the protocol's stop command. No arguments besides `self`. No interlock applies, and there is no travel check; only a dead link, a transport that stays busy, a failed write, or a rotator that does not answer can make it fail (`Driver` or `Timeout`). Async, one exchange under the transport mutex, safe to repeat.

### `pub async fn park(&self) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_rotator_park]

Sends the rotator to the park position in its description. No arguments besides `self`. Returns when the command is accepted, not when parking is complete. Errors: `Refused` if the description has no park position; otherwise the same `Driver` and `Timeout` errors as `set_position`. Async, one exchange under the transport mutex.

### `pub async fn wait_for(&self, az: f32, el: f32, tolerance: f32, limit: Duration) -> Result<(), StationError>` [@ANCHOR: radioctl:radioctl_station_rotator_wait_for]

Polls `position` until the rotator is within `tolerance` degrees of `az` (and of `el` when the rotator has elevation), or until `limit` has passed. `az`, `el` and `tolerance` are degrees; `limit` is the total time allowed. It returns `Ok(())` on arrival. A failed poll is tolerated and retried, except a `Driver` error mentioning the link, which is returned at once. On expiry it returns a `Timeout` error that says the rotator did not reach the target and where it was last seen. Async; it polls about every 50 ms, so it sends many requests over the transport; it does not itself command movement.

### `fn link_ok(&self) -> bool` [@ANCHOR: radioctl:radioctl_station_rotator_link_ok]

Private: whether the transport's link is usable. Returns true when no link status was supplied at construction, and otherwise true only when the watched status is `Connected`. Reads the watch channel's current value without waiting. Cannot fail, no I/O, no async lock.

### `async fn transact(&self, op: RotOp) -> Result<RotReply, StationError>` [@ANCHOR: radioctl:radioctl_station_rotator_transact]

Private: runs one command and its reply, the core of every public method. `op` is the operation by value. It encodes the command (a `Refused` error for a bad position), fails with `Driver` if the link is down, then waits up to the description's reply timeout for the transport mutex (a `Timeout` "busy with another command" otherwise). Under the lock it drains stale bytes, writes and flushes the command, and reads until the engine's `parse` yields a reply, within one reply timeout for the whole write and read. Errors: `Driver` on write or read failure, a closed link, a reply that exceeds 4096 bytes with no end, or an error from the engine; `Timeout` when no reply arrives. After a failure it drains again so a late reply cannot be taken for the next one. Async serial or network I/O, holding the transport mutex across the awaits on purpose.

### `async fn drain(io: &mut Box<dyn Transport>)` [@ANCHOR: radioctl:radioctl_station_rotator_drain]

Private: reads and discards whatever the transport has waiting, stopping when the line has been quiet for 10 ms or after 64 reads. `io` is the locked transport, mutably borrowed. It returns nothing and swallows read errors and timeouts, so it cannot fail. Async; reads from the transport; it never writes. Used before a command and after a failure so stale bytes are not mistaken for a reply.
