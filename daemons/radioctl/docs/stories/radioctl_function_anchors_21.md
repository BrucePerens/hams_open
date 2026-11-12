# radioctl function anchors, batch 21

Function anchors for the `radioctl-core` crate (serial transport and baud probe) and the `radioctl-splitter` crate (CI-V and TS-2000 personalities, PTT arbiter, pty endpoint, session), written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Transmit-related entries say how the transmit gate applies: the baud probe can only send a frequency read and never keys; the splitter's key and send-CW paths go through the rig handle, so the rig's gate, lock and band checks still decide, and the arbiter only chooses which client may ask.

## `crates/radioctl-core/src/serial.rs`

### `fn write_fully(port: &mut dyn SerialPort, mut bytes: &[u8]) -> io::Result<()>` [@ANCHOR: radioctl:radioctl_core_serial_write_fully]

Writes every byte of a command to the serial port and then flushes it, called by the supervisor's session loop for each `Command::Write`. `port` is borrowed mutably (it is the session's own handle) and `bytes` is a borrowed slice that is consumed from the front as partial writes succeed. A short write simply continues with the rest; `Interrupted` is retried at once; `TimedOut` is retried up to `WRITE_RETRIES` (40) times, which at the 50 ms port timeout is about two seconds, after which the timeout error is returned. A write of zero bytes returns `WriteZero` (the port is gone), any other error is returned unchanged, and the final `flush` error is returned too. Blocking: it runs on the supervisor thread, never on the async runtime. A failure ends the session and starts a reconnect, so some bytes of the command may already have reached the radio.

### `fn run(self, first: oneshot::Sender<Result<(), RigError>>)` [@ANCHOR: radioctl:radioctl_core_serial_supervisor_run]

The body of the `radioctl-serial` thread, started by `open_serial`; it takes the supervisor by value and runs until shutdown. `first` is a one-shot channel that is answered exactly once: `Ok(())` after the first open succeeds, or the last open error after `first_open_attempts` (at least 1) tries, each separated by the configured backoff delay (`sleep_dropping`), in which case the status is set to `Closed` and the thread ends. Once open, the loop publishes the port (a cloned handle for control lines, plus `Connected { port, generation }`, generation starting at 1), runs one `session`, clears the shared handle, and on a loss publishes `Reconnecting { attempt, last_error }` and retries opening forever with the growing backoff, counting a `reconnects` and bumping the generation each time it succeeds. Shutdown (or the command channel closing) ends the loop and publishes `Closed`. Bytes the actor writes while the port is down are dropped and counted, never queued, so a radio cannot receive stale commands after a reconnect. No transmit gate is involved here: this layer only moves bytes the rig actor already approved.

### `fn sleep_dropping(&self, delay: Duration) -> bool` [@ANCHOR: radioctl:radioctl_core_serial_sleep_dropping]

Waits out a backoff `delay` (a `Duration`, for example 100 ms up to the backoff ceiling) while the port is closed. During the wait it keeps draining the command channel so the rig actor never blocks on a full queue: every `Command::Write` received is discarded and its length added to the `bytes_dropped` counter. It returns `true` when the full delay passed, and `false` as soon as a `Shutdown` command arrives or the sender is disconnected, so the caller stops reconnecting. Blocking on the supervisor thread only (it uses `recv_timeout`). Dropping rather than buffering is deliberate: commands for a radio that is not connected must not be replayed later, least of all anything that could key it.

### `fn session(&self, mut port: Box<dyn SerialPort>) -> SessionEnd` [@ANCHOR: radioctl:radioctl_core_serial_session]

Runs one connection to its end. It takes ownership of the open `port`, clones it for a reader thread named `radioctl-serial-rx` (running `read_loop`), and then loops on this thread: if the reader has recorded a loss it returns `SessionEnd::Lost(reason)`; otherwise it waits up to `SUPERVISOR_TICK` (100 ms) for a command, writes `Command::Write` bytes with `write_fully` and adds their count to `bytes_out`, and returns `Shutdown` on a shutdown command or a disconnected channel. A failed clone, a failed reader spawn, or a write error each return `Lost` with a reason string (`clone for reading`, `could not start the reader thread`, `write: ...`). On the way out it sets the stop flag and joins the reader, so no thread outlives the session. Blocking, and the reader exits within one 50 ms port timeout.

### `fn read_loop( mut port: Box<dyn SerialPort>, to_actor: mpsc::Sender<Vec<u8>>, counters: Arc<Counters>, lost: Arc<Mutex<Option<String>>>, stop: Arc<AtomicBool>, )` [@ANCHOR: radioctl:radioctl_core_serial_read_loop]

The reader thread body. It reads up to 256 bytes at a time from its own clone of the port and sends each chunk to the async side through `to_actor` with `blocking_send`, adding the count to `bytes_in`. Arguments: `port` (owned, read), `to_actor` (bounded channel of 64 chunks, so a slow actor applies back pressure), `counters` (shared atomics), `lost` (a mutex-guarded reason slot the session polls) and `stop` (set by the session to end the thread). `TimedOut` and `Interrupted` are normal and just re-check `stop`. A zero-length read records `port closed` and any other error records `read: ...` in `lost` and returns; a closed receiver also returns silently. The loop checks `stop` between reads, so it ends within about 50 ms.

### `pub async fn open_serial(runtime: &Handle, config: SerialConfig) -> Result<SerialLink, RigError>` [@ANCHOR: radioctl:radioctl_core_serial_open]

Opens the serial port described by `config` and returns a `SerialLink`: an in-memory byte stream (4096 bytes of buffer) that the rig actor reads and writes, the control lines (DTR/RTS) handle, a status watch and the counters. `runtime` is borrowed and gets two bridge tasks (stream to command queue, reader chunks to stream); one OS thread (`radioctl-serial`) runs the supervisor. It waits for the first open: it returns `Ok` once that succeeded, or the supervisor's error (a `LinkDown` error from `open_port` naming the port and the cause) after `first_open_attempts` tries, or `LinkDown` if the thread ended before answering or could not be spawned. The write queue holds 64 commands; when full the bridge retries every 5 ms rather than dropping. Dropping the stream sends `Shutdown` and ends the thread and closes the port. It does not itself key anything; DTR and RTS are driven to their configured idle levels when the line control is `Managed`.

## `crates/radioctl-core/src/baud_probe.rs`

### `pub fn read_request<E: ProtocolEngine>(engine: &mut E) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_core_baud_probe_read_request]

Asks `engine` (borrowed mutably, because encoding may advance its state) to encode `Op::GetFrequency(Vfo::A)` and returns the resulting `Request` (its bytes plus the expected reply kind). It then checks that the request's expected reply is exactly `ReplyKind::Observation(Field::Frequency(Vfo::A))`; if the engine would answer with an acknowledgement or no reply it returns an `Unsupported` error that names the reply kind and says it is not probing. An encode error from the engine is returned as is. This guard is the safety property of the whole probe: the only bytes the baud probe can ever put on the wire are the engine's own frequency read, never a hand-written frame and never a command that changes the radio or keys it. Pure apart from the encode call.

### `async fn wait_for_observation<E: ProtocolEngine>( transport: &mut Box<dyn Transport>, engine: &mut E, timeout: Duration, epoch: Instant, ) -> bool` [@ANCHOR: radioctl:radioctl_core_baud_probe_wait_for_observation]

Listens on `transport` for up to `timeout` (the engine description's reply timeout) and feeds every chunk read (up to 256 bytes) to `engine.feed`, using `epoch.elapsed()` as the timestamp so the engine's own timing logic sees consistent times. It returns `true` at the first `Inbound::Observation` the engine decodes, which proves the radio answered at this rate. It returns `false` when the deadline passes, the read fails, or the stream ends (a zero-length read). Garbage at a wrong baud rate is fed to the engine and normally decodes to nothing, so the loop keeps listening until the deadline. `transport` and `engine` are mutably borrowed; it never writes, so it cannot transmit.

### `pub async fn probe<P: ProbePort, E: ProtocolEngine>( port: &mut P, engine: &mut E, rates: &[u32], ) -> Result<ProbeResult, RigError>` [@ANCHOR: radioctl:radioctl_core_baud_probe_probe]

Finds the baud rate a serial radio answers at. `rates` is the candidate list in the order to try (empty is an `Unsupported` error); `port` opens and closes the device per rate; `engine` supplies the frequency read and decodes answers. For each rate it records the rate in `tried`, opens the port, writes the engine's frequency read once, waits one description timeout for an observation, drops the stream and calls `close` (so the device is free again). The first rate that answers returns `ProbeResult { baud, tried }`; no further rate is opened and no second request is sent. If none answers the result is `LinkDown` listing every rate tried. An error from `port.open` (device missing or not permitted) ends the probe at once with that error. A failed write counts as no answer. Runs before the rig actor exists, so the actor's start-up unkey pass still happens at the chosen rate. Never keys.

### `pub fn rates_text(rates: &[u32]) -> String` [@ANCHOR: radioctl:radioctl_core_baud_probe_rates_text]

Formats a list of baud rates as a comma and space separated string, for example `4800, 9600, 19200`, for log lines and for the `LinkDown` error text that lists the rates the probe tried. `rates` is borrowed and unchanged; each rate is printed in decimal with no unit. An empty slice gives an empty string. Pure, allocates one `String`, cannot fail.

### `pub fn new(runtime: Handle, config: SerialConfig) -> SerialProbePort` [@ANCHOR: radioctl:radioctl_core_baud_probe_serial_port_new]

Builds a probe port that opens the real serial device named in `config`. `runtime` (a Tokio handle, owned) is where each `open_serial` link runs its bridge tasks; `config` (owned) carries the device, line settings and the DTR/RTS handling the real link will use. Its `baud` field is a placeholder: every `open` overwrites it with the rate being tried, and sets `first_open_attempts` to 1 so a missing device fails the probe quickly instead of retrying. No port is opened by `new` itself; the monitor slot starts empty. Cannot fail.

### `async fn open(&mut self, baud: u32) -> Result<Box<dyn Transport>, RigError>` [@ANCHOR: radioctl:radioctl_core_baud_probe_serial_port_open]

The `ProbePort` implementation for a real port: opens the configured device at `baud` (bits per second) with exactly one attempt. It first calls `close`, so the previous link's supervisor has released the exclusively held device (waiting at most 2 seconds). It clones the stored config with the new baud, calls `open_serial`, keeps the link's status monitor for the next `close`, and returns the byte stream as a boxed `Transport`. An error from `open_serial` (the device is missing, busy or not permitted) is returned as the `RigError`, which the probe treats as fatal rather than a baud mismatch. DTR and RTS are made inactive after every open exactly as for the real link, so probing does not assert them.

### `async fn close(&mut self)` [@ANCHOR: radioctl:radioctl_core_baud_probe_serial_port_close]

Called after the stream returned by the last `open` was dropped. It takes the stored status monitor (doing nothing if there is none) and waits, bounded by `CLOSE_WAIT` of 2 seconds, until the supervisor reports `LinkStatus::Closed`, which is the moment the device is released. This prevents the next open, or the real link started after the probe, from finding the port busy. A timeout is ignored silently (the next open will then report the busy device). Idempotent: a second call finds no monitor and returns at once. No error is returned.

## `crates/radioctl-splitter/src/civ_personality.rs`

### `fn status_frame(&self, to: u8, ok: bool) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_splitter_civ_status_frame]

Builds the bare acknowledgement frame of CI-V: from this personality's address to `to` (the controller address of the client, such as 0xE0), with command byte `OK` (0xFB) when `ok` is true and `NG` (0xFA) when false, and no data. Returns the complete wire bytes, preamble and terminator included (`FE FE to from cmd FD`). Used for a done outcome, for refusals, and for unknown or malformed commands. Pure, no errors.

### `pub fn new(address: u8, echo: bool) -> CivPersonality` [@ANCHOR: radioctl:radioctl_splitter_civ_new]

Creates the IC-7300 CI-V personality. `address` is the CI-V address it answers at (0x94 for the real IC-7300, and the default used by the splitter config); frames addressed to it or to the broadcast address 0x00 are served. `echo` turns on the single-wire bus echo: each complete frame the client sends is first sent back to it. The mode table is taken from `RigDescription::ic7300()` regardless of which radio is really connected, and the frame parser starts empty. Cannot fail.

### `pub fn mode_code(&self, mode: Mode) -> Option<u8>` [@ANCHOR: radioctl:radioctl_splitter_civ_mode_code]

Maps a `Mode` to the IC-7300 CI-V mode byte. The data modes have no byte of their own here and map to the sideband they sit on (`Data` to USB, `DataR` to LSB), which is a stated limit of this personality. Any other mode is looked up in the IC-7300 table; `None` means the IC-7300 has no code for it, and callers answer NG or send no unsolicited mode frame. Pure.

### `fn classify(&self, frame: &Frame) -> Action` [@ANCHOR: radioctl:radioctl_splitter_civ_classify]

Turns one decoded CI-V frame addressed to us into an `Action`. It matches the command byte and data: `03` read frequency, `05` and `25 00/01` set or read a VFO (the data is five BCD bytes, `00` is the selected VFO A, `01` the unselected B), `04` and `26 00` read mode, `06` and `26 00` set mode (an unknown mode byte is refused), `1C 00` read or set PTT (any non-zero state keys), `19 00` read the ID (answered locally with our address), `15 02` S-meter, `15 11` power meter, `17` send CW (printable ASCII, or `FF` to stop). Rig-bound items become `Action::Rig` with a context holding the requester's address, the command and any prefix to repeat in the reply. Bad BCD, an unknown command or odd lengths give `Action::Local` with an NG frame. Setting PTT or sending CW here only produces a request; the splitter's arbiter and the rig's gate decide whether it transmits.

### `fn parse(&mut self, bytes: &[u8], now: Duration) -> Vec<Pending>` [@ANCHOR: radioctl:radioctl_splitter_civ_parse]

The `Personality::parse` for CI-V. `bytes` are raw bytes from one client (borrowed), `now` is a monotonic time used by the frame parser to discard stale partial frames. Complete frames are extracted by the stateful `FrameParser`; line noise and bytes not forming a frame are ignored. For each frame it prepares the bus echo (the frame's own bytes when `echo` is on, else empty, and always including frames meant for other radios, as a shared wire would), and decides the action: `classify` if the frame is for our address or the broadcast address and did not come from our own address, else `Action::Ignore`. Returns one `Pending` per frame, in order; empty when nothing complete arrived. Mutates the parser state; no I/O.

### `fn reply(&mut self, context: &[u8], _request: &Request, outcome: &Outcome) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_splitter_civ_reply]

Renders the splitter's `outcome` as CI-V reply bytes. `context` is what `classify` stored: first byte the address to answer, second the command, the rest a prefix (for example the VFO selector) that is repeated before the value; if it is shorter than two bytes the result is empty. `Refused` gives NG; `Done` gives OK; a frequency is encoded as five BCD bytes (NG if it does not fit); a mode is a mode byte plus filter, and for command `26 00` also the DATA flag (1 for Data and DataR); PTT is one byte 0 or 1; signal and power are BCD meter values. Any outcome that does not match the request, such as a composite status, which CI-V has no use for, is NG. `_request` is unused. Pure.

### `fn unsolicited(&mut self, change: &Change) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_splitter_civ_unsolicited]

Produces the CI-V transceive frame a client expects when the rig changes: a VFO A frequency change becomes command `00` to the broadcast address with five BCD bytes, and a mode change becomes command `01` with the mode byte and filter 01. Changes to VFO B, PTT and every other field give an empty vector (nothing is sent), as does a frequency or mode that cannot be encoded. `change` is borrowed (new value read only). Pure, no errors; the splitter sends the bytes to every client regardless of who made the change.

### `fn reset(&mut self)` [@ANCHOR: radioctl:radioctl_splitter_civ_reset]

Forgets partial input when a client disconnects: it replaces the frame parser with a fresh default one, so a half-received frame from a program that closed its pty cannot be glued to the first bytes of the next program. The address, echo flag and mode table are kept. No arguments, no return, cannot fail.

## `crates/radioctl-splitter/src/session.rs`

### `fn personality(&self) -> Box<dyn Personality>` [@ANCHOR: radioctl:radioctl_splitter_personality]

Creates a fresh personality object for each new client according to the configured protocol: `Protocol::Civ { address, echo }` gives a `CivPersonality` for that address and echo setting, `Protocol::Ts2000` gives a `Ts2000Personality`. Every client gets its own object because each holds per-connection state (the CI-V frame parser, the TS-2000 tokenizer and auto-information flag). Called by the attach functions. Cannot fail.

### `async fn set(&self, op: Op) -> Outcome` [@ANCHOR: radioctl:radioctl_splitter_set]

Performs a set operation on the rig for a client. `op` (owned) is executed through the rig handle's `execute_from` with source `Splitter` and the configured `request_timeout` (default 2 seconds). Any success is `Outcome::Done`; any error (unsupported, link down, timeout, transmit gate closed, band or lock refusal, rig error) is `Outcome::Refused`, with the reason not passed on to the client, which only sees NG or `?;`. Because the call goes through the rig handle, every safety check of the rig, including the transmit gate for `SetPtt(true)` and `SendCw`, still applies; the splitter cannot bypass it. Async; it waits for the rig actor's reply.

## `crates/radioctl-splitter/src/ts2000_personality.rs`

### `fn code(&self, mode: Mode) -> Option<u8>` [@ANCHOR: radioctl:radioctl_splitter_ts2000_code]

Maps a `Mode` to the TS-2000 mode digit used in `MD` and in the `IF` status. The table is the one of `RigDescription::ts2000`. Data modes are mapped to the sideband they sit on (`Data` to USB, `DataR` to LSB), because the TS-2000 digit table has no separate data digits. Returns `None` when the table has no digit for the mode; callers then answer `?;` or omit the unsolicited update, and `status_answer` returns `None`. Pure.

## `crates/radioctl-splitter/src/arbiter.rs`

### `pub fn try_key(&mut self, client: ClientId) -> Result<(), ClientId>` [@ANCHOR: radioctl:radioctl_splitter_arbiter_try_key]

Claims the transmitter for `client` (a `u64` assigned by the splitter). It returns `Ok(())` when nobody owns it (and records `client` as owner) or when `client` already owns it (idempotent), and `Err(current_owner)` when a different client holds it, leaving ownership unchanged. The claim is made before the rig is asked to key, and `release` gives it back if the rig refuses, so a refused key (gate closed, link down) never strands ownership. The splitter calls it under the owner mutex for both `SetPtt(true)` and send-CW. It does not talk to the rig and is not a transmit authorization: the rig's gate still decides.

### `pub fn release(&mut self, client: ClientId) -> bool` [@ANCHOR: radioctl:radioctl_splitter_arbiter_release]

Gives up the transmitter if `client` holds it. Returns `true` and clears the owner when `client` was the owner, and `false` (no change) when someone else owns it or nobody does, so a client can never release another client's claim. Used to undo a claim when the rig refused to key, and when a client that held the transmitter disconnects, after which the splitter also unkeys the rig. Pure state change, no I/O.

### `pub fn clear(&mut self)` [@ANCHOR: radioctl:radioctl_splitter_arbiter_clear]

Clears ownership unconditionally. It is used when the transmitter was unkeyed successfully by any client: unkeying is always allowed, even for a non-owner, so after an `Op::SetPtt(false)` returns done the splitter clears the owner, whoever it was. Idempotent, no arguments or return value, cannot fail.

### `pub fn owner(&self) -> Option<ClientId>` [@ANCHOR: radioctl:radioctl_splitter_arbiter_owner]

Returns the id of the client that currently holds the transmitter, or `None` when it is free. Read-only accessor, `Copy` result. The splitter exposes it as `ptt_owner` for status and tests. The answer is only a snapshot: another client may claim it a moment later, so decisions must go through `try_key`.

## `crates/radioctl-splitter/src/pty.rs`

### `pub fn path(&self) -> &Path` [@ANCHOR: radioctl:radioctl_splitter_pty_path]

Returns the filesystem path outside programs should open as their serial port: the symlink given to `attach_pty` when one was requested, otherwise the slave device itself (`/dev/pts/N`). Borrowed from the endpoint, valid while it lives. Read-only; the symlink is removed when the endpoint is dropped.

### `pub fn slave_path(&self) -> &Path` [@ANCHOR: radioctl:radioctl_splitter_pty_slave_path]

Returns the real pseudo-terminal slave device path (for example `/dev/pts/7`), even when a symlink was also created. Useful for tests and for diagnostics, since the symlink may point elsewhere after a restart. Borrowed from the endpoint, read-only, cannot fail. The number changes on every run, so do not store it.

### `fn drop(&mut self)` [@ANCHOR: radioctl:radioctl_splitter_pty_drop]

Shuts the endpoint down when the `PtyEndpoint` is dropped. It sets the stop flag, which makes the master reader thread exit within about 50 ms, and removes the symlink, but only if the link still points at this endpoint's slave device, so a link another process or a newer endpoint has replaced is left alone. A failure to remove the link is ignored. The writer thread ends when its channel closes with the client. It does not unkey the rig; the client task does that when it sees the disconnect.

### `pub fn attach_pty( splitter: &Splitter, runtime: &Handle, link: Option<PathBuf>, ) -> Result<(PtyEndpoint, ClientHandle), RigError>` [@ANCHOR: radioctl:radioctl_splitter_pty_attach]

Creates a pseudo-terminal pair, drops the slave end (so no program is attached yet), optionally replaces any file at `link` with a symlink to the slave, clones the master for writing, starts two OS threads (`radioctl-pty-rx` for `read_master`, `radioctl-pty-tx` for `write_master`) with channels of 64 items, and attaches the channels to `splitter` as a persistent client. `splitter` and `runtime` are borrowed; `link` is owned (Unix only). Returns the endpoint and the client handle. Any failure (pty creation, no slave name, symlink, clone, thread spawn) is a `LinkDown` error with the cause in the text. One endpoint is one client, so each program needs its own. A program closing the slave gives up that client's PTT.

### `fn read_master(mut master: TTYPort, input: mpsc::Sender<ClientInput>, stop: Arc<AtomicBool>)` [@ANCHOR: radioctl:radioctl_splitter_pty_read_master]

The pty reader thread. It sets a 50 ms read timeout and then reads up to 256 bytes at a time from the master, sending each chunk as `ClientInput::Bytes` with `blocking_send`. Timeouts and interrupts just re-check `stop`. Any other result (an error or end of file) is what the master reports when the program closed the slave; it sends one `ClientInput::Disconnected` (not repeated until bytes arrive again) and sleeps 50 ms before trying again, so the endpoint stays usable for the next program that opens it. It returns when `stop` is set, if the timeout cannot be set, or if the receiver is gone. Blocking, on its own thread.

### `fn write_master(mut master: TTYPort, mut output: mpsc::Receiver<Vec<u8>>)` [@ANCHOR: radioctl:radioctl_splitter_pty_write_master]

The pty writer thread: takes reply and unsolicited byte vectors from `output` (blocking receive) and writes them to the master with a 50 ms timeout. A timed-out write is retried up to 40 times (about two seconds), `Interrupted` is retried, and any other error or exhausted retries drops the rest of that chunk and moves on, which is how bytes written while no program has the slave open are discarded without blocking the splitter. The thread ends when the channel closes or the timeout cannot be set. It never reports errors; loss is silent by design.

## `crates/radioctl-splitter/src/session.rs`

### `pub fn new(protocol: Protocol) -> SplitterConfig` [@ANCHOR: radioctl:radioctl_splitter_config_new]

Builds the splitter settings with defaults for the given `protocol` (CI-V with an address and echo flag, or TS-2000). Defaults: `max_age` 5 seconds (a read is answered from the rig cache if the cached value is no older), `request_timeout` 2 seconds, `report_tx_frequency_while_keyed` true, `max_clients` 16 and `idle_timeout` 600 seconds (the same as radioctld's server). The result is `Copy` and every field is public so callers can change them. Cannot fail.
