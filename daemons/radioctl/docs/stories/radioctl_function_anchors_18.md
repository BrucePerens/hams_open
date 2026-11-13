# radioctl function anchors, batch 18

Function anchors for `radioctl-protocols`: the FlexRadio SmartSDR engine (reply, status and discovery parsing, the shadow state, command building), the virtual FlexRadio used by tests and the TCP server that hosts it (`flex_io.rs`), and the OpenHPSDR control-plane engine. Tests cite each anchor with `Tests [@ANCHOR: ...]`. Transmit gate: `Op::SetPtt(true)` and `Op::SendCw` are the only ops that can key a Flex or HPSDR radio; the rig actor refuses them while the gate is closed before an engine encodes them, and `FlexEngine::encode` additionally refuses any non-transmitting op that would produce a line `line_may_transmit` flags. The virtual radio never transmits: keying only changes its in-memory interlock state.

## `crates/radioctl-protocols/src/flex.rs`

### `pub fn parse_reply(line: &str) -> Option<Reply>` [@ANCHOR: radioctl:radioctl_flex_parse_reply]

Decodes a SmartSDR reply line `R<seq>|<hex>|message[|debug]` into a `Reply` (`seq`, `code`, `message`). `line` is borrowed and must start with `R` and have no terminator. The sequence is decimal `u32`; the response code is hexadecimal `u32` with or without a `0x`/`0X` prefix, where 0 means success. The message is everything after the second `|` (it may be empty and may contain further `|`, because only the first two bars split), or empty when there is no third part. Returns `None` for a missing `R`, a missing second field, a non-numeric sequence or a code that is not hex or does not fit 32 bits. Pure, no panics.

### `pub fn parse_pairs(text: &str) -> Vec<(&str, &str)>` [@ANCHOR: radioctl:radioctl_flex_parse_pairs]

Splits a status text into its `key=value` words, in the order they appear. `text` is borrowed and the returned slices borrow from it, so no copying is done. Words are separated by whitespace; a word without `=` is skipped, and only the first `=` splits a word (so `a=b=c` gives key `a`, value `b=c`). A value containing a blank cannot be represented, because the text is split on whitespace first. Empty input gives an empty vector. Pure, no errors. Used for slice, display, interlock and transmit status and by the virtual radio's command parser.

### `pub fn parse_info(text: &str) -> RadioInfo` [@ANCHOR: radioctl:radioctl_flex_parse_info]

Decodes the reply message of the `info` command, a comma-separated list `key=value,key="quoted value",...`, into a `RadioInfo`. `text` is borrowed. Quotes group a value so commas inside them stay part of the value and the quote marks are dropped; CR and LF characters are ignored; keys and values are trimmed. Recognised keys: `model`, `software_ver`, `slice` (slice receiver count, `u8`), `scu` (spectrum capture units, `u8`), `name` and `callsign` (empty values become `None`). Unknown keys are ignored, a missing or unparsable number leaves its field `None`, and when a key repeats the first wins. Never fails. Pure.

### `pub fn parse_firmware(text: &str) -> Option<(u8, u8)>` [@ANCHOR: radioctl:radioctl_flex_parse_firmware]

Extracts the firmware (major, minor) from the reply message of `version`, which is `#`-separated `name=value` entries such as `SmartSDR-MB=3.2.31.1#PSoC-MBTRX=...`. Only the `SmartSDR-MB` entry is used and only its first two dot-separated numbers: `3.2.31.1` gives `(3, 2)`. `text` is borrowed. Returns `None` when the entry is absent or either number is missing, not decimal, or above 255. Pure. Used by the engine to answer `Op::GetFirmware`.

### `pub fn parse_discovery(packet: &[u8]) -> Option<Discovery>` [@ANCHOR: radioctl:radioctl_flex_parse_discovery]

Decodes a radio's UDP discovery broadcast into a `Discovery`. The datagram `packet` (borrowed) must be at least 28 bytes: a VITA-49 extension header of seven 32-bit big-endian words, with stream id `0x800` in word 1 and class id `0x534CFFFF` in word 2 or word 3. The payload after the 28 bytes is UTF-8 text of `key=value` words, ended by a NUL or the end of the packet: `model`, `serial`, `version`, `name`, `callsign` (underscores turned back into spaces in name and callsign), `ip` and `port`. Returns `None` for a short packet, wrong stream or class id, invalid UTF-8, a port that is not a valid `u16`, or when the port, the model or the address is missing. Unknown keys are ignored. Pure, no I/O; the caller owns the socket and should treat the result as unauthenticated.

### `pub fn new(desc: RigDescription) -> Result<FlexEngine, RigError>` [@ANCHOR: radioctl:radioctl_flex_engine_new]

Builds the SmartSDR engine for a Flex description. `desc` is taken by value and kept. It must be a `Flex` family description carrying the Flex section; otherwise `Unsupported` with "not a FlexRadio description". VFO A of the core vocabulary is the slice receiver numbered by the description's `slice` (0 unless `with_slice` changes it). The new engine has sent no subscription yet, has no slice, panadapter, transmit or `info` state, no pending commands, and sequence number 0 (the first command is `C1`). No I/O.

### `fn subscription_lines(on: bool) -> [String; 3]` [@ANCHOR: radioctl:radioctl_flex_subscription_lines]

Builds the three command lines that turn the radio's pushed status on or off: `sub slice all`, `sub tx all`, `sub pan all` when `on` is true, and the same with `unsub` when false. SmartSDR has no read commands, so the engine depends on these subscriptions to learn anything. The order is fixed (slice, tx, pan). Pure, no errors. Used for the prefix of the first command, for `Op::SetTransceive`, and when the link is restored.

### `fn shadow_value(&self, field: Field) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_flex_shadow_value]

Answers a read from the status the engine has already seen. Supported fields, all for the engine's slice: frequency of VFO A, mode, filter width (high edge minus low edge, and `None` if the difference is negative), AF gain from the slice's `audio_level`, mic gain and RF power from the transmit status (percent 0 to 100), and PTT (push to talk) from the interlock state through `interlock_keyed`. Returns `None` for any other field or when the needed status has not been received yet, in which case the engine parks the read and answers it from the first status line that carries the field. Borrows the engine immutably; pure.

### `fn commands(&self, op: &Op) -> Result<Vec<(String, Kind)>, RigError>` [@ANCHOR: radioctl:radioctl_flex_engine_commands]

Builds the command lines for an op together with a `Kind` that says how each reply is treated; the last line is the op's own. Reads are a `ping` whose answer comes from the shadow (`GetFirmware` sends `version`). Writes: `slice t <n> <MHz>` for frequency, `slice s <n> mode=<name>`, `filt <n> <lo> <hi>` for filter width, `slice s <n> audio_level=`, `transmit set miclevel=` and `rfpower=`, `xmit 1` or `xmit 0` for PTT, `cwx send "text"`, `cwx clear` for stop, and the three `sub` or `unsub` lines for `SetTransceive`. Errors: `Rejected` for a level above 100, a filter width outside 1 to 24000 Hz, a filter change before the slice's low edge has been reported, or bad CW text; `Unsupported` for a mode with no SmartSDR spelling and for every op not listed. Borrows the engine; no I/O. `xmit 1` and `cwx send` can transmit; the actor gates them before `encode` runs.

### `fn apply_slice(&mut self, index: u8, pairs: &[(&str, &str)]) -> Vec<Observation>` [@ANCHOR: radioctl:radioctl_flex_apply_slice]

Applies the `key=value` list of a slice status line to the engine's per-slice state for slice `index`, creating it if new. Keys: `in_use`, `active`, `tx` (true for `1`, `T`, `t`, `on`, `ON`, `+`), `antenna_id` or `rxant`, `RF_frequency` (megahertz), `mode`, `filter_lo` and `filter_hi` (megahertz, signed) and `audio_level` (0 to 100). A value that does not parse is ignored and keeps the old field. For the engine's own slice only, it returns observations for the fields that changed in this line: frequency, mode (when the core vocabulary has it), filter width, and AF gain; other slices update their state but return nothing. Mutates the engine state; no I/O.

### `fn apply_pan(&mut self, id: u32, pairs: &[(&str, &str)])` [@ANCHOR: radioctl:radioctl_flex_apply_pan]

Applies a panadapter status line's `key=value` words to the state kept for the panadapter with stream id `id` (a hexadecimal id parsed by the caller), creating it if new: `center` and `bandwidth` (megahertz, stored as hertz), `x_pixels` or `x-pixels`, `y_pixels` or `y-pixels`, and `fps`. A value that does not parse leaves the old value in place. Returns nothing: panadapter state is not part of the core vocabulary and is read through `FlexEngine::panadapter`. Mutates the engine's panadapter map; no I/O.

### `fn apply_interlock(&mut self, pairs: &[(&str, &str)]) -> Vec<Observation>` [@ANCHOR: radioctl:radioctl_flex_apply_interlock]

Applies an interlock status line. `state=<text>` is stored as the interlock state and yields `Observation::Ptt` from `interlock_keyed` (so a front-panel MOX changes the cached transmit state); `tx_allowed=1` is stored as a boolean (any other value is false). Other keys are ignored. Returns at most one PTT observation per `state` word. Mutates the engine's transmit state. It reads the transmitter state only; it never sends anything.

### `fn apply_transmit(&mut self, pairs: &[(&str, &str)]) -> Vec<Observation>` [@ANCHOR: radioctl:radioctl_flex_apply_transmit]

Applies a `transmit` status line: `rfpower` and `miclevel`, each an integer 0 to 100 (percent), are stored and reported as `Observation::Level` for RF power and mic gain. A value above 100 or not a number, and any other key, is ignored. The key names are assumed to match the `transmit set` parameters (documented in the engine's header as an assumption to check against a radio). Mutates the transmit state; no I/O.

### `fn status(&mut self, rest: &str) -> Vec<Observation>` [@ANCHOR: radioctl:radioctl_flex_engine_status]

Decodes an `S<handle>|object ...` status line, given the text after the `S`. The handle is ignored. Objects: `slice <n> key=value...` goes to `apply_slice`; `display pan <hex id> ...` goes to `apply_pan` (and yields nothing); `interlock ...` and `transmit ...` go to their handlers; every other object, a slice number that is not a `u8`, or a line without a `|` is ignored and yields nothing. Returns the observations the status carries. Mutates the engine state; pure otherwise.

### `fn answer_reply(&mut self, reply: Reply) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_flex_answer_reply]

Matches an `R` reply to the pending command with the same sequence number and produces the inbound event. A reply with no matching command becomes `Unrecognised`. For subscriptions, a refusal only increments the refusal counter and stores the code (nothing is shown to the actor); for `info`, success parses the message into the radio info; for a set, code 0 is `Ack` and any other code is `Nak`; for `version`, a good reply with a parsable `SmartSDR-MB` entry is a firmware observation and anything else a `Nak`; for a read, a failure code is `Nak`, success is answered from the shadow, and if the shadow lacks the value the field is parked to be answered by the next status line. Removes the matched command from the pending list. No I/O.

### `fn handle_line(&mut self, line: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_flex_engine_line]

Handles one complete line from the radio, by its first character. `V` stores the protocol version; `H` stores the client handle (hexadecimal; a non-hex handle is unrecognised); `R` goes to `parse_reply` and `answer_reply`; `S` goes to `status`, turns each observation into an event and clears any parked read for a field that the status just answered; `M<hex>|text` stores the last message (number, text) and emits nothing. An empty line gives nothing; anything else, and any malformed `R`, `H` or `M`, becomes `Unrecognised` with the line's bytes. Mutates the engine; no I/O.

### `pub fn interlock_keyed(state: &str) -> bool` [@ANCHOR: radioctl:radioctl_flex_interlock_keyed]

True when a SmartSDR interlock state means RF is, or may still be, on the air: exactly `PTT_REQUESTED`, `TRANSMITTING` or `UNKEY_REQUESTED` (the transmitter stays engaged until the unkey completes). Any other text, including `READY` and unknown states, gives false. The comparison is exact and case sensitive. `state` is borrowed. A state name this function does not know is treated as not keyed, so a new state name introduced by a firmware update would have to be added here. Pure.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_flex_engine_encode]

`ProtocolEngine::encode` for Flex: turns an op into one or more `C<seq>|command` lines, terminated by LF, and says what to wait for (an observation for reads and `GetFirmware`, an `Ack` otherwise). Errors: `Unsupported` when the description lacks the op's capability, plus everything `commands` refuses. It also refuses (`Rejected`, "internal: a non-transmit op produced a transmit command") if a non-transmitting op would produce a line that `line_may_transmit` flags, which guards the gate. The first command on a link (other than `SetTransceive`) is prefixed with the three `sub` lines and `info`, once. Every line gets the next sequence number and is recorded as pending (when 64 are already pending the oldest is forgotten). Sequence numbers wrap at `u32`. The actor applies the transmit gate to `SetPtt(true)` and `SendCw` before this is called.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_flex_engine_feed]

`ProtocolEngine::feed`: collects received bytes into lines ended by CR, LF or CRLF and passes each non-empty line to `handle_line`. A partial line older than the 500 ms partial-frame timeout is dropped when more bytes arrive. A line longer than 16 KiB is discarded to its terminator and reported as `Unrecognised("line too long")`; a line that is not valid UTF-8 is `Unrecognised`. Returns events in order, empty while a line is incomplete. Mutates the line buffer and the engine's shadow; no I/O, never panics.

## `crates/radioctl-protocols/src/virtual_flex.rs`

### `pub fn format_signed_mhz(hz: i64) -> String` [@ANCHOR: radioctl:radioctl_virtual_flex_signed_mhz]

Formats signed hertz as megahertz with six decimals and a leading minus sign when negative: -2700 is `-0.002700`, 100 is `0.000100`. Used for the filter edges in the virtual radio's slice status, because the edges are negative on lower sidebands. `hz` is whole hertz; `i64::MIN` is handled through the unsigned magnitude. Pure, never fails. The inverse is `parse_mhz_signed` in the engine module.

### `pub fn from_description(desc: &RigDescription) -> Result<VirtualFlex, RigError>` [@ANCHOR: radioctl:radioctl_virtual_flex_new]

Builds the virtual FlexRadio from a Flex description. `desc` is borrowed. Error: `Unsupported` ("not a FlexRadio description") unless the family is Flex and has a Flex section. The radio starts at the description's default frequency and mode, filter edges 100 to 2800 Hz, audio level 50, mic level 40, RF power 10, not keyed, no CW text sent, a 200 kHz panadapter centred on the default frequency, no subscriptions, and `Order::ReplyFirst`. The client handle is fixed at `0x6F4E_C23D`. The radio never transmits.

### `fn command(&mut self, seq: &str, text: &str) -> String` [@ANCHOR: radioctl:radioctl_virtual_flex_command]

Answers one parsed command line (`text` has no `C<seq>|` prefix) with the reply text, possibly followed or preceded by status lines per `order`. Handled: `ping`, `version`, `info`, `sub` and `unsub` of slice, tx and pan (an unknown object is answered `0x500000A3`), `slice t <n> <MHz>`, `slice s <n> mode=` and `audio_level=`, `filt`, `transmit set miclevel=` and `rfpower=`, `xmit`, `cwx send` and `cwx clear`. A wrong slice number gives `INVALID_SLICE`, a bad mode `BAD_MODE`, a value out of range `OUT_OF_RANGE`, an unknown key `0x5000002D`, and anything unknown `BAD_COMMAND`. `xmit 1` only sets the in-memory keyed flag; `cwx send` appends the text to `cw_sent` (spaces restored) and `cwx clear` empties it and counts a stop. Status lines are produced only for subscribed groups. Mutates the radio; no I/O.

### `pub fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_flex_feed]

Consumes bytes from the client and returns everything the radio sends back, concatenated. Lines end with CR, LF or CRLF; each complete non-empty line that is valid UTF-8 goes to `line`. A line longer than 16 KiB is discarded up to its terminator. A line that is not a command is ignored (empty output). `_now` is unused: the virtual radio has no timers. Partial lines wait in the buffer for the next call. Mutates the radio; no I/O.

### `pub fn front_panel(&mut self, change: FlexPanel) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_flex_front_panel]

Simulates a change made at the radio itself rather than by the connected client. `FlexPanel::Tune(hz)` sets the frequency and `SetMode(mode)` the mode; both produce a slice status line if the client subscribed to slice status. `FlexPanel::Mox(on)` sets the keyed flag (the virtual radio moves its interlock state to `TRANSMITTING` or `READY`; it does not emit RF) and produces an interlock line if the client subscribed to tx. Returns the bytes to push to the client, empty when nothing was subscribed. Mutates the radio; no I/O.

### `fn interlock_line(&self) -> String` [@ANCHOR: radioctl:radioctl_virtual_flex_interlock_line]

Builds the interlock status line: `S0|interlock state=TRANSMITTING source=SW tx_allowed=1` while keyed, `S0|interlock state=READY tx_allowed=1` otherwise, ended by LF. The state is the only thing that tests use to see the radio as transmitting; no RF is generated. Pure, no errors.

### `fn line(&mut self, line: &str) -> String` [@ANCHOR: radioctl:radioctl_virtual_flex_line]

Parses one input line `C[D]<seq>|text`. The `D` (debug) after `C` is allowed and ignored. Lines that do not start with `C`, have no `|`, or whose sequence is not a decimal `u32` return an empty string and are not recorded. Otherwise the command text (without prefix) is appended to the list of received lines that transmit-gate tests inspect, and `command` is called to answer it. Mutates the radio; no I/O.

### `fn slice_status_if_subscribed(&self) -> String` [@ANCHOR: radioctl:radioctl_virtual_flex_slice_status_if_subscribed]

Returns the slice status line when the client has subscribed to slice status, an empty string otherwise. The line holds slice number, `in_use=1`, frequency, mode in upper case, filter edges in signed megahertz and the audio level. Used after every command that changes slice state so unsubscribed clients see no unsolicited status. Pure.

### `fn with_status(&self, reply: String, status: String) -> String` [@ANCHOR: radioctl:radioctl_virtual_flex_with_status]

Joins a reply line and the status line it caused in the order the radio is configured for: `reply` then `status` for `Order::ReplyFirst`, `status` then `reply` for `Order::StatusFirst`. The SmartSDR wiki does not say which comes first, so a test can choose either to check that the engine does not depend on it. Both strings are owned and consumed. Pure.

## `crates/radioctl-protocols/src/flex_io.rs`

### `pub fn serve_virtual_flex( runtime: &Handle, listener: TcpListener, rig: VirtualFlex, ) -> VirtualFlexControl` [@ANCHOR: radioctl:radioctl_virtual_flex_serve]

Serves a `VirtualFlex` on an already bound tokio `TcpListener` from a task spawned on `runtime` (borrowed handle). The caller binds the listener, normally to `127.0.0.1:0`. The radio is moved into shared state guarded by a `std::sync::Mutex` (a poisoned lock is recovered). One client at a time: each accepted connection replaces the previous one, gets TCP no-delay, discards pushed bytes queued before it, and receives the `V` and `H` greeting; bytes read from the client go through `feed` and the replies are written back, up to 512 bytes per read. A read or write failure ends that client and the task waits for the next. Returns a cloneable `VirtualFlexControl`; the task runs until the runtime ends. No transmitter is involved.

### `pub fn state(&self) -> FlexRigState` [@ANCHOR: radioctl:radioctl_virtual_flex_control_state]

Returns a copy of the served virtual radio's `FlexRigState` (frequency, mode, filter edges, levels, keyed flag, CW texts sent and CW stop count, panadapter). Takes the radio's mutex briefly (a poisoned mutex is recovered), so it may be called from any thread and it does not block on network I/O. Used by tests to assert what the engine did to the radio. Never fails.

### `pub fn front_panel(&self, change: FlexPanel)` [@ANCHOR: radioctl:radioctl_virtual_flex_control_front_panel]

Makes a change at the served radio as if the operator did it, and pushes the resulting status lines to the connected client at once (nothing is sent when no status is due, for example when the client has not subscribed). `change` is `Tune`, `SetMode` or `Mox`. Takes the radio's mutex briefly, then queues bytes on an unbounded channel that the serving task writes to the stream; a send failure is ignored. The pushed bytes queued while no client is connected are discarded at the next accept. `Mox(true)` only changes the in-memory interlock state; no RF exists.

## `crates/radioctl-protocols/src/hpsdr_net.rs`

### `pub fn new(desc: RigDescription) -> Result<HpsdrEngine, RigError>` [@ANCHOR: radioctl:radioctl_hpsdr_engine_new]

Builds the OpenHPSDR control-plane engine. `desc` is taken by value and kept. It must be an `Hpsdr` family description with an `hpsdr` section; otherwise `Unsupported` ("not an OpenHPSDR description"). The engine starts with an empty line buffer. It speaks a small line protocol to the bridge in `hpsdr_io`, which owns the real session; the engine itself does no I/O.

### `fn parse_line(line: &str) -> Inbound` [@ANCHOR: radioctl:radioctl_hpsdr_engine_parse_line]

Decodes one trimmed line from the bridge. `OK` is `Ack` and `ERR <text>` is `Nak`. `F <hz>` is VFO A's frequency, `P <0|1>` the PTT state (any non-zero is true), `W <raw>` the forward power and `R <tenths>` the SWR (both saturated to `u16::MAX` if larger), and `A <db>` the attenuator level in whole decibels. Anything else, a number that is not an unsigned integer, or an attenuation above `u16::MAX`, is `Unrecognised` with the line bytes. Pure, no panics.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_hpsdr_engine_encode]

`ProtocolEngine::encode` for the HPSDR bridge: one LF-terminated line per op. `GF` and `F <hz>` for VFO A frequency (settable up to `u32::MAX` hertz), `GP` and `P 0|1` for PTT, `GW` for forward power, `GR` for SWR, `GA` and `A <db>` for the attenuator. Writes expect an `Ack`; reads expect the matching observation. The attenuator ops check the description first (`Unsupported` or the step error from the attenuation table when the value is not allowed). Every other op, including a frequency above 32 bits or VFO B, is `Unsupported` ("the OpenHPSDR control plane does not support this command"). Unlike most engines it does not check capabilities. `P 1` is the only command that can key the transmitter: the rig actor refuses `SetPtt(true)` while the transmit gate is closed before this runs, and the bridge opens the `hpsdr` crate's own gate for a bounded lease. `P 0` is always allowed.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_hpsdr_engine_feed]

`ProtocolEngine::feed`: collects bytes into LF-terminated lines (at most 128 bytes) and decodes each with `parse_line`. A line that is not valid UTF-8 is `Unrecognised`. An over-long line is dropped silently up to its terminator and produces no event. Blank lines still produce `Unrecognised` (an empty line after trimming is not a known reply). `_now` is unused, so a partial line never times out here. Returns the events in order; empty while a line is incomplete. Mutates the line buffer only.
