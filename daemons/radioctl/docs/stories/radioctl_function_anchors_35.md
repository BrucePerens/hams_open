# radioctl function anchors, batch 35

Function anchors for the `radioctld` TCP server (`server.rs`) and the per-rig command service (`service.rs`): the accept loop, the per-client task, reply writing, and the service functions behind the Hamlib `rigctld` commands for mode, split, RIT/XIT, keying and Morse. Each entry follows ADR 0113 and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Several of these functions can put RF on the air: `check_transmit`, `key` and `send_morse` take the fail-closed path (transmit gate open and link up, and the single transmitter owner), while `unkey`, `stop_morse`, `disconnect` and `shutdown` are stop paths that always work, with the gate open or closed.

## `crates/radioctld/src/server.rs`

### `pub async fn shutdown(self)` [@ANCHOR: radioctl:radioctld_server_shutdown]

Stops the TCP server. It consumes the `ServerHandle`, aborts the accept task so no new client is taken, and sends the shutdown signal that every client task is watching; each client then leaves its loop and calls the service's `disconnect`, so a client that owned the transmitter unkeys the rig on its way out. Then it polls every 5 ms for the active-client count to reach zero and gives up after `SHUTDOWN_WAIT` (3 seconds), so a stuck client cannot hold it forever; there is no error return and no indication whether the wait timed out. Async; sends no signal to the process, only to its own tasks.

### `async fn accept_loop(listener: TcpListener, shared: Arc<Shared>, handle: Handle)` [@ANCHOR: radioctl:radioctld_server_accept_loop]

The task that takes connections until it is aborted by `shutdown`. For each connection it first checks the peer address against the allow-list (a peer outside it is dropped before any byte is read, counted as `denied`), then takes a client slot from a semaphore sized by `max_clients` (when none is free the connection is dropped at once and counted as `busy`). An accepted socket gets TCP no-delay, the `accepted` and `active` counters go up, and the client number is taken in accept order (starting at 1) before the client task is spawned, so ids follow connection order. An accept error pauses 100 ms and continues. Private, no return value; the slot is released when the client task ends.

### `pub async fn answer(service: &RigService, client: u64, line: &Line) -> (String, bool)` [@ANCHOR: radioctl:radioctld_server_answer]

Produces the reply to one request line for one client. A line that is not text (`Line::Invalid`) answers `RPRT -1` and does not close. Text that parses to nothing (a blank line) answers an empty string. Otherwise it builds the response header (extended-format flag, command name, VFO echo from `vfo_echo`, arguments), runs the command through `RigService::execute` with the client id and VFO selection (or uses the parse error code), and renders the result with `render`. The second value is true when the command asked to close the connection (the `q` command). `client` identifies who is asking, for transmitter ownership. Async; the command may reach the radio, including a transmit command that the service gates.

### `fn render_invalid() -> String` [@ANCHOR: radioctl:radioctld_server_render_invalid]

The fixed answer to a request line that is not valid text (for example non-UTF-8 bytes, or an over-long line): `RPRT -1` and a newline, the Hamlib invalid-parameter code. No arguments, no allocation beyond the returned string, no failure. Private; used by `answer` and by `run_client` when a line is too long.

### `async fn write_reply(stream: &mut tokio::net::tcp::OwnedWriteHalf, shared: &Shared, text: &str) -> bool` [@ANCHOR: radioctl:radioctld_server_write]

Writes one reply to the client within `WRITE_TIMEOUT` (10 seconds). Empty text is a success and writes nothing (some commands answer nothing). Returns true when all bytes were written; false when the write fails (client gone) or times out. A timeout also increments the `timeouts` counter, so a client that never reads is counted and dropped by the caller rather than blocking the task. The stream is the client's write half, mutably borrowed. Async; network I/O; it does not close the stream itself.

### `async fn run_client(socket: TcpStream, client: u64, shared: Arc<Shared>, _permit: OwnedSemaphorePermit)` [@ANCHOR: radioctl:radioctld_server_run_client]

The task of one client, from accept to disconnect. It reads into a 1024-byte buffer and feeds a `LineBuffer` that holds at most `max_line_bytes`. Each read waits up to the idle timeout (or, once a partial line has started, the line timeout measured from the first byte), and a timeout, a closed or failed read, or the server shutdown signal ends the loop (a timeout counts in `timeouts`). A line that is too long counts as `line_too_long`, gets `RPRT -1` and ends the client. Each complete line is answered in order with `service.answer` and written with `write_reply`; a failed write or a close command ends the loop. On the way out it always calls `service.disconnect(client)`, which unkeys the rig if this client owned the transmitter, then decrements `active`. The semaphore permit is held until the function returns.

## `crates/radioctld/src/service.rs`

### `pub fn fmt_float(value: f64) -> String` [@ANCHOR: radioctl:radioctld_service_fmt_float]

Formats a level value for the Hamlib text protocol: six decimal places with trailing zeros and then a trailing decimal point removed, so `0.5` stays `0.5`, `1.0` becomes `1` and one-eighth-ish values print as `0.117647`. Rounds, not truncates, at the sixth place. Zero prints as `0`. A NaN or infinite value is not specially handled (it would print as the float formatter does); the callers pass finite ratios. Pure, no errors.

### `pub fn new(rig: RigHandle, description: RigDescription, config: ServiceConfig) -> RigService` [@ANCHOR: radioctl:radioctld_service_new]

Creates the service for one rig. It takes ownership of the rig handle (the connection to the rig actor), the description (capabilities and tables) and the `ServiceConfig` (VFO mode, cache age, request timeout, advertised model, receive range and power), and starts with no transmitter owner, the current VFO set to A and all counters at zero. No I/O and no failure. The result is shared between client tasks behind an `Arc`; internal locks (owner and current VFO) make that safe.

### `pub fn rig(&self) -> &RigHandle` [@ANCHOR: radioctl:radioctld_service_rig]

Gives read access to the rig handle that the service was built with, so other parts of the daemon (the status page, rotator glue, tests) can query the rig's cache, capabilities or transmit permit directly. A shared reference, valid as long as the service; no copy, no lock, no failure. Taking this does not bypass the transmit gate: opening it still needs a permit.

### `pub fn config(&self) -> &ServiceConfig` [@ANCHOR: radioctl:radioctld_service_config]

Returns the service settings the service was created with: whether VFO mode is on, the cache maximum age, the request timeout, the Hamlib model number, the advertised receive range in hertz and the advertised transmit power in milliwatts. The settings never change after creation. A shared reference; no lock, no failure. The server reads `vfo_mode` from it to echo VFO names.

### `pub fn stats(&self) -> ServiceStats` [@ANCHOR: radioctl:radioctld_service_stats]

Takes a snapshot of the service counters: commands executed, reads answered from the cache, transmit commands refused (gate closed, link down or another client owning the transmitter) and keying attempts refused because another client owns the transmitter. Each is read atomically with relaxed ordering, so values are individually right but not one consistent instant. Cheap, lock-free, no failure; for status pages and tests.

### `pub fn ptt_owner(&self) -> Option<ClientId>` [@ANCHOR: radioctl:radioctld_service_ptt_owner]

Returns the number of the client that currently owns the transmitter, or `None` when nobody does. It reads the owner record under its mutex (a poisoned lock is recovered, not propagated). The record is set by `key` and `send_morse` and cleared by `unkey` and `disconnect`. It reflects what this service believes, not necessarily the radio's state (the radio may also be keyed by its own front panel). No failure; a momentary lock.

### `fn caps_view(&self) -> CapsView<'_>` [@ANCHOR: radioctl:radioctld_service_caps_view]

Builds the borrowed view from which `\dump_state` and `\dump_caps` are generated: the rig description, the Hamlib model number, the advertised receive range in hertz, the advertised transmit power in milliwatts, and whether transmit is enabled (true exactly when the rig's transmit gate currently has a permit). The view borrows from the service, so it lives no longer than the call. Private; no I/O or locking beyond reading the permit; no failure. The transmit flag decides whether the dump advertises a usable transmit side.

### `pub async fn disconnect(&self, client: ClientId)` [@ANCHOR: radioctl:radioctld_service_disconnect]

Handles a client going away. It releases the client's claim on the transmitter and, if that client was in fact the owner, sends an unkey (`SetPtt(false)`) to the rig, ignoring any error. A client that did not own the transmitter causes no rig traffic. This is how a crash or disconnect of the keying client stops transmission. No gate is needed because unkeying is always allowed. Called once per client by the server task when it ends. Async; at most one command to the rig, waiting up to the request timeout.

### `pub async fn shutdown(&self)` [@ANCHOR: radioctl:radioctld_service_shutdown]

Called when the daemon is stopping: it unkeys the rig unconditionally through `unkey`, whether or not an owner is recorded, because a rig keyed by Morse, or one whose owner record was lost, has no owner and unkeying an unkeyed rig costs a single command. The result is ignored; a rig that cannot be unkeyed by command answers Unsupported and nothing more can be done. Async; one command, bounded by the request timeout. It is safe to call more than once.

### `async fn exec(&self, op: Op) -> Result<Option<Observation>, i32>` [@ANCHOR: radioctl:radioctld_service_exec]

Runs one operation on the rig actor's queue, as an API-sourced request, waiting at most the configured request timeout. Success is the observation the rig returned, or `None` for a command with no answer; a failure is mapped to the negative Hamlib `RPRT` code by `error_code` (timeout, rejected, gate closed, unsupported, link down and so on). `op` is owned and consumed. All reads that miss the cache and all sets go through here. It performs no transmit-gate check of its own: the actor enforces the gate for transmitting operations, and `check_transmit` gives an earlier, cheaper refusal. Async.

### `async fn read(&self, field: Field, op: Op) -> Result<Observation, i32>` [@ANCHOR: radioctl:radioctld_service_read]

Reads a value, preferring the cache. If the rig's cache holds `field` and the value is no older than the configured `cache_max_age`, it counts a cache hit and returns the cached observation without any rig traffic; otherwise it executes `op` on the rig. An error from the rig is the `RPRT` code. A command that returned no observation (`None`) becomes `EIO` (-6), since a read must produce a value. Async; `field` names the cache slot and `op` the matching get operation.

### `async fn set(&self, op: Op) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_set]

Runs a setting operation on the rig and answers `Body::Done` (rendered as `RPRT 0`), discarding any observation the rig returned. Errors are the `RPRT` code from `exec`. It does not use or update the cache itself and does no gating, so callers that can transmit (`key`, `send_morse`) check the gate first. Async; waits up to the request timeout.

### `fn resolve_vfo(&self, selection: Option<VfoSel>) -> Result<Vfo, i32>` [@ANCHOR: radioctl:radioctld_service_resolve_vfo]

Turns a client's VFO selection into one of the rig's two VFOs. No selection or `currVFO` gives the VFO most recently selected with `V` (initially A); `VFOA` and `VFOB` give A and B; `RX` and `TX` give the receive and transmit VFO from `tx_state`. `Absent` (a VFO Hamlib knows that radioctl rigs lack, such as `VFOC` or `MEM`) returns `ENAVAIL` (-11). Private; reads the current-VFO mutex; no rig traffic.

### `async fn frequency(&self, vfo: Vfo) -> Result<u64, i32>` [@ANCHOR: radioctl:radioctld_service_frequency]

Reads one VFO's frequency in hertz, through `read` (cache first, then the rig). Returns the whole number of hertz. Errors are the `RPRT` code of a failed read, or `EIO` when the rig answers with something other than a frequency. Async; no side effects on the radio.

### `async fn mode_and_width(&self) -> Result<(Mode, u32), i32>` [@ANCHOR: radioctl:radioctld_service_mode_and_width]

Reads the current mode and the receive bandwidth in hertz. The mode is read through the cache. If the description lacks the Filter capability the width is the mode's nominal typical width (`nominal_width`); otherwise the rig's filter width is read, and a width that does not fit in 32 bits gives `EIO`. Any other kind of answer is `EIO`, and a failed read is its `RPRT` code. Async; reads only.

### `async fn mode_fields(&self, mode_label: &'static str, width_label: &'static str) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_mode_fields]

Builds the two-field answer of the mode commands: the Hamlib mode name (`USB`, `CW` and so on, through `mode_to_hamlib`) labelled `mode_label`, and the passband width in hertz labelled `width_label`, as a `Body::Fields` that the reply renderer prints one per line (with labels in extended format). The caller chooses the labels, so each command can use Hamlib's own wording for its extended-format answer. Errors are those of `mode_and_width`. Async; reads only.

### `async fn set_mode(&self, mode: Mode, passband: i64) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_set_mode]

Sets the mode and, when asked, the receive bandwidth. `passband` is in hertz; 0 and -1 leave the rig's own width, and anything outside -1 to 1,000,000 returns `EINVAL` before the rig is touched. A width is sent only if it is positive and the description has the Filter capability. In that case the previous mode is read first; if setting the width is refused, the previous mode is put back (best effort, errors ignored) and the width's error code is returned, so a refused `M FM 15000` does not leave the rig changed. Answers `Done`. Async; up to three rig commands. Not a transmit command.

### `async fn mode_before(&self) -> Option<Mode>` [@ANCHOR: radioctl:radioctld_service_mode_before]

Reads the mode that the rig is in now, through the cache, so `set_mode` can restore it if a bandwidth change is refused. Returns `None` when the mode cannot be read (any error, or an unexpected observation), in which case the caller simply cannot restore it. Async; no side effect on the radio; the error code is deliberately dropped.

### `async fn require_current_mode(&self, wanted: Mode) -> Result<(), i32>` [@ANCHOR: radioctl:radioctld_service_require_mode]

Used for the split transmit mode. radioctl rigs have one mode, so a split transmit mode other than the current one cannot be honoured, since changing it would also change the receive mode. It reads the current mode and returns `Ok(())` if it equals `wanted`, else `ENAVAIL` (-11); a failed read returns its `RPRT` code. Async; reads only.

### `fn refuse(&self, code: i32) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_refuse]

Counts one refused transmit command in the `transmit_refused` counter and returns it as the error `code` that the caller supplies (the transmit conflict path passes `ERJCTED`, -9). Never succeeds: the return value is always `Err(code)`. A relaxed atomic increment; no I/O and no locks. It exists so the refusal statistics are counted in one place.

### `fn check_transmit(&self) -> Result<(), i32>` [@ANCHOR: radioctl:radioctld_service_check_transmit]

The fail-closed check in front of every command that can put RF on the air. First, the rig's transmit gate must hold a permit; if it does not, the answer is `ERJCTED` (-9) and `transmit_refused` is incremented. Second, the link to the rig must be up; if it is down the answer is `EIO` (-6), also counted. Only if both hold does it return `Ok(())`. When it refuses, nothing has been queued to the rig and no transmitter claim has been made. A refusal is not an error in the radio, only the daemon declining. No I/O.

### `async fn key(&self, client: ClientId) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_key]

Keys the transmitter (PTT on) on behalf of `client`. Steps: `check_transmit` (gate open and link up, otherwise `ERJCTED` or `EIO`); then try to claim the single transmitter owner record for this client, and if another client owns it count a conflict and refuse with `ERJCTED` (-9), also counted as a refusal; then send `SetPtt(true)` to the rig. If the rig refuses or fails, the claim is given back so the owner record never claims a transmitter that is not keyed. Answers `Done` or the `RPRT` code. The owner lock is held only briefly and never across the rig call. Async. The rig actor's own limit (`max_transmit`) can still unkey after a time.

### `async fn unkey(&self) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_unkey]

Unkeys the transmitter by sending `SetPtt(false)`. It is open to every client and works with the gate open or closed, whoever owns the transmitter, because stopping RF is never refused. If the rig accepts, the owner record is cleared. On failure the owner record is left as it is (so a later disconnect will try again) and the `RPRT` code is returned, for example `EIO` if the link is down. Answers `Done`. Async; one rig command bounded by the request timeout.

### `async fn send_morse(&self, client: ClientId, text: &str) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_send_morse]

Sends Morse (CW) text from the rig's keyer, which puts RF on the air, so it follows the same rules as keying. It runs `check_transmit` (gate open, link up), then claims the transmitter for `client` (refused with `ERJCTED` and counted as a conflict if another client owns it), then sends the text with `SendCw`. If the rig refuses, the claim is released. The sender keeps ownership afterwards, so a disconnect unkeys the rig. `text` is borrowed and copied; its length limits are the engine's (`unknown` here: check the engine for the allowed characters and length). Async.

### `async fn stop_morse(&self) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_stop_morse]

Stops Morse that is being sent. Like the unkey it is a stop path: open to every client, whether or not it owns the transmitter and whether the gate is open or closed, and it jumps the rig's queue. The transmitter owner record is not touched, because the rig may still be keyed by PTT. Answers `Done`, or `ENAVAIL` (-11) on a rig whose engine has no stop command, or another `RPRT` code if the rig fails. Async; one command.

### `async fn split_vfo(&self) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_split_vfo]

The answer of the `s` command: whether split is on and which VFO transmits. Split is read from the rig only if the description has the Split capability; otherwise it is off. The transmit VFO is B when split is on and A when off. The answer has two fields: `Split` as 0 or 1 and `TX VFO` as `VFOA` or `VFOB`. A failed read is its `RPRT` code. Async; reads only (cache first).

### `async fn set_split_vfo(&self, on: bool, tx: VfoSel) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_set_split_vfo]

The `S` command. Split on means receive on VFO A and transmit on VFO B, the only arrangement radioctl rigs have. Turning it on with any transmit VFO other than B, or on a rig without the Split capability, returns `ENAVAIL` (-11). Turning it off sends `SetSplit(false)` if the rig has split, and on a rig without it just answers `Done`. The transmit VFO argument is ignored when switching off. Changes which VFO a later key transmits on; it does not itself key the transmitter. Async.

### `async fn rit_offset(&self, xit: bool) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_rit_offset]

The `j` (RIT) and `z` (XIT) commands, chosen by `xit`: the offset in hertz while the function is switched on, else 0. It reads whether the function is on and, only if it is, reads the offset. The single answer field is labelled `RIT` or `XIT`. A failed read is its `RPRT` code and an answer of the wrong kind is `EIO`. Async; reads only (cache first).

### `async fn set_rit_offset(&self, xit: bool, hz: i64) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_set_rit_offset]

The `J` (RIT) and `Z` (XIT) commands, chosen by `xit`. An offset of 0 switches the function off and leaves the stored offset alone. Any other offset must be within -9999 to 9999 hertz, otherwise `EINVAL` (-1) before anything is sent; it is then sent to the rig and the function switched on, in that order, so the function is on only if the offset was accepted. Errors are the `RPRT` code of the failed step; if the offset is set but switching on fails the offset stays changed. Answers `Done`. Async; up to two commands. Changes the transmit frequency when XIT is on, so on a transmitting rig the effect is on-air, but it does not key.
