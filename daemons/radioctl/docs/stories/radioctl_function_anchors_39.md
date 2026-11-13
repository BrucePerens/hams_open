# radioctl function anchors, batch 39

Function anchors for the second half of the TS-890S engine (bandscope placement, encode, feed, KY), the virtual TS-440S, TS-990S and TS-890S rigs (`virtual_ts440.rs`, `virtual_ts990.rs`, `virtual_ts890.rs`), the USB and receiver protocol names and rig-file builders in `radioctl-core`, and the shared line-oriented receiver engine, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Anything that can key a transmitter says how the gate applies: the engines only encode bytes and the gate, band check and frequency check are applied by the rig layer before `encode`; the virtual rigs have no hardware, so a key-on there only sets a state flag. Receivers have no transmit path.

## `crates/radioctl-protocols/src/kenwood_ts890.rs`

### `fn scope_edges_of(body: &str) -> Option<(Hz, Hz)>` [@ANCHOR: radioctl:radioctl_ts890_scope_edges_of]

Parses the answer to a `BSM` read. `body` is the text after `BSM`: a range digit that must be `0` (the range in use), then two eight-digit frequencies in hertz, the lower and upper edge, 17 characters in all. Returns `Some((low, high))`, or `None` for another range number, a wrong length, a non-digit, or edges that are not strictly low then high. The engine stores the result and uses it to place fixed-mode and auto-scroll sweeps; a `None` makes the reply `Unrecognised`. Pure.

### `fn filter_scope_frame(&mut self, body: &str) -> Option<FilterScopeFrame>` [@ANCHOR: radioctl:radioctl_ts890_engine_filter_scope_frame]

Turns the body of a `##DD3` frame (or a completed serial `DD3` sweep) into a `FilterScopeFrame`. `body` must hold exactly 213 two-digit hexadecimal samples, none above 0x32 (the weakest, -50 dB), checked by `scope_samples`. Each sample is flipped to `0x32 - value` so that 0 is the bottom of the display, as the core wants; the frame carries `amplitude_max` 0x32, receiver 0 and a sequence number of 0 that the rig actor fills in. A malformed body returns `None` and increments the dropped counter (`scope_frames_dropped`). Mutates only that counter.

### `fn scope_samples(body: &str, count: usize, max: u8) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_ts890_scope_samples]

Splits a `##DD2` or `##DD3` body into samples. `body` must be ASCII and exactly `2 * count` characters; each pair is a hexadecimal byte (00 strongest) and none may exceed `max` (0x8C for the bandscope, 0x32 for the filter scope). Returns the byte values, or `None` for a wrong length, a non-hexadecimal digit or a value past the guide's range. The caller counts a `None` as a dropped frame. Pure; one allocation.

### `fn expanded_span(span: Hz) -> Option<u64>` [@ANCHOR: radioctl:radioctl_ts890_expanded_span]

With the scope's expand setting on, the rig sends a sweep wider than the span shown. The guide gives the width only for two spans: a 100 kHz span covers 300 kHz and a 200 kHz span covers 400 kHz; both centred. This returns that width in hertz for those two and `None` for any other, whose expanded width the guide does not state. A `None` makes the engine count and drop the sweep rather than guess its edges. Pure.

### `fn scope_frame(&mut self, body: &str) -> Option<ScopeFrame>` [@ANCHOR: radioctl:radioctl_ts890_engine_scope_frame]

Turns a `##DD2` body (or a completed serial `DD2` sweep) of 640 samples into a `ScopeFrame`. The frame has no frequency of its own, so it is placed from what the engine last saw (`note`): in center mode the receive VFO (variable frequency oscillator, the tuning control)'s frequency plus or minus half the span (or half the expanded width, `expanded_span`, when expand is on; unknown expand state drops the sweep); in fixed mode the edges from the last `BSM0` read; in auto scroll the same edges if expand is off. Samples are flipped to `0x8C - value` (0 at the bottom) with `amplitude_max` 0x8C, receiver 0 and sequence 0 (the actor numbers it). `out_of_range` is set when the receive frequency lies outside non-center edges.

Returns `None`, and increments `scope.dropped`, when the samples are malformed or the axis, span, frequency or edges are not yet known. Mutates only that counter; no I/O.

### `fn note(&mut self, inbound: &Inbound)` [@ANCHOR: radioctl:radioctl_ts890_engine_note]

Keeps what the bandscope frames are placed from. For an observation of frequency A or B, the receive VFO, the scope axis, the scope span (receiver 0) or the expand setting, it records the value in the engine's scope context (`freq_a`, `freq_b`, `rx_vfo`, `axis`, `span`, `expand`); everything else is ignored. Called from `feed` for every decoded reply, and from `encode` for sets (frequency, axis, span, expand) so placement follows the controller's own changes before the rig confirms. No I/O.

### `fn scope_start(&self) -> String` [@ANCHOR: radioctl:radioctl_ts890_engine_scope_start]

Builds the command text that turns the waveform output on. First it reads what frames are placed from: `FA;FB;FR;BS3;BS4;BSO;BSM;`, whose answers update the engine copies as they arrive; then `DD0` and `DD1` set the bandscope and filter-scope outputs. The values depend on the link: LAN uses `DD0 2` (our choice of medium cycle) and `DD1 1`; serial (`serial_scope`) uses `DD0 4` and `DD1 2`, output to COM/USB linked to auto information. Returns one string of commands, sent as one request. Nothing here transmits. Pure.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_ts890_engine_encode]

The `ProtocolEngine` encode for the TS-890S: turns one `Op` into the command text of a `Request` (bytes and expected reply kind). It first requires the op's capability in the description (`Unsupported` otherwise). Core ops: `FA`/`FB` read and set (eleven digits, at most 99,999,999,999 Hz), mode as `OM0` plus the description's code, `SM;`, `ID;`, split `TB`, RIT and XIT (transmitter incremental tuning) `RT`/`XT`, offset `RF` read and `RC` plus `RU`/`RD` set (at most 9999 Hz), levels (`AG`, `RG`, `MG`, `KS`, `PC` as `NAME` and three digits; attenuator and preamp steps through `attenuation::step_of`; antenna as `AN<port>999`), `AI` for transceive, Morse through the description's `KY` record (`ky`), and the scope operations: output, axis, span, reference, settings, actions and fixed-mode edges.

Errors, all `Unsupported`: frequency over eleven digits, unknown mode, an offset beyond 9999 Hz, a level or step outside the guide's range or not on the model, a missing or unsupported `KY`, a scope axis or span the rig lacks, a read-only scope setting, scope marker shift in center mode, edges that are not low below high in eight digits, and any other op. Keying: `SetPtt(true)` encodes `TX0;` and false `RX;`. The engine does not decide whether transmitting is allowed; the rig layer's gate, band and frequency checks run before it is called. Side effects: it updates the scope context and the transceive and scope-output flags, so that `link_restored` can turn them on again; a frequency set while the axis is auto scroll also appends `BSM;` to re-read the edges.

### `fn ky(&self) -> Result<KyParams, RigError>` [@ANCHOR: radioctl:radioctl_ts890_engine_ky]

Returns the description's `KY` (send Morse text) record for the model, or `Unsupported` ("send-CW-text (KY) is not documented for this model") when the description has none or its form does not offer sending text. The record carries the form, the text field width (up to 24 characters), the character table and the stop command (`KY0;`), taken from the TS-890S guide. Used by `encode` for `SendCw` and `StopCw`. Morse on the air is keying: the gate, band and frequency checks happen in the rig layer before `encode`. Pure.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_ts890_engine_feed]

The `ProtocolEngine` feed for the TS-890S. `bytes` are received bytes (borrowed) and `now` their arrival time; the tokenizer (limited to 2048 bytes) cuts them at `;` and flags junk. Each command is then routed: `##DD2` and `##DD3` bodies become scope frames (`scope_frame`, `filter_scope_frame`); serial `DD2` and `DD3` splits go to `serial_split`; `BSD` (the waterfall-clear confirmation) is ignored; `BSM` answers update the stored edges (`scope_edges_of`), and a bad one is `Unrecognised`; everything else goes through `decode` and then `note`, which updates the placement context. Junk becomes `Unrecognised`. Never fails or blocks; returns whatever frames and observations completed.

## `crates/radioctl-protocols/src/virtual_ts440.rs`

### `pub(super) fn is_ts440(&self) -> bool` [@ANCHOR: radioctl:radioctl_virtual_ts440_is]

True when the virtual rig's Kenwood description uses the older dialects handled by this module: `ts440` (TS-440S), `ts450` (TS-450S and TS-690S) or `r5000` (the R-5000, Hamlib's IC-10 backend). It reads the description's kenwood block and returns false for a description without one. The virtual rig uses it to decide whether the TS-440S command table (`ts440_command`) answers and to pick the `IF` layout. Pure.

### `fn is_fn_sp(&self) -> bool` [@ANCHOR: radioctl:radioctl_virtual_ts440_is_fn_sp]

True when the described rig selects VFO and split with `FN` and `SP` (dialects `ts440` and `r5000`), false for the TS-450S and TS-690S dialect, which use `FR` and `FT` (set only). It reads the description only. `ts440_command` uses it to enable or refuse the commands of the other style. Pure.

### `pub(super) fn ts440_command(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_ts440_command]

Interprets one command of the older Kenwood dialects for a virtual rig; `text` is the command without its `;`. Answers `FA`/`FB` reads and sets, `ID`, `IF` (`kenwood_if`), `AI` 0 or 1, and sets (no reply) `MD` (the mode is set only, read from `IF`), `FN` and `SP` (TS-440S) or `FR` and `FT` (TS-450S), `RT`, `XT`, `RC`, and `RU`/`RD`, which take no parameter and move the offset by our own virtual 10 Hz step, clamped to plus or minus 9990 Hz (the manual gives no step). `TX` and `RX` set the virtual PTT (push to talk) flag. Returns the reply bytes: none for a setting, `?;` for any refusal (an unknown command, a bare `MD`, a form of the other dialect). Virtual: no RF (radio frequency).

## `crates/radioctl-protocols/src/virtual_ts990.rs`

### `pub(super) fn is_ts990(&self) -> bool` [@ANCHOR: radioctl:radioctl_virtual_ts990_is]

True when the virtual rig's Kenwood description has dialect `ts990` (the TS-990S, written from Kenwood's PC command reference of March 2013). False if the description has no kenwood block or another dialect. Used by the virtual rig to put the TS-990S forms in front of the classic command table and to build its `IF` answer. Pure.

### `pub(super) fn ts990_command(&mut self, text: &str) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_virtual_ts990_command]

Handles the TS-990S commands that differ from the classic Kenwood table, for the virtual rig. `text` is the command without `;`. `OM0` reads the main band's mode as `OM0<code>;`, and `OM0<m>` or `OM1<m>` sets it through `set_kenwood_mode`. The classic forms this model does not have (`MD`, `FR`, `FT`, `SP`, `AG`, `RG`, `MG`, `ML`, `PC`, `KS`, `RA`, `PA`) are refused with `?;`. Returns `Some(reply bytes)` when it handled the command (possibly empty for a set) and `None` to let the classic table answer (`FA`, `ID`, `AI`, `SM0`, `IF`, `RT`, `XT`, `RC`, `RU`/`RD`, `TX`, `RX`). Virtual state only.

## `crates/radioctl-protocols/src/virtual_ts890.rs`

### `pub(super) fn is_ts890(&self) -> bool` [@ANCHOR: radioctl:radioctl_virtual_ts890_is]

True when the virtual rig's Kenwood description has dialect `ts890`. False when there is no kenwood block or another dialect. The virtual rig tests it to route commands to `ts890_command` and to emit scope sweeps. Pure.

### `pub(super) fn ts890_command(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_ts890_command]

Interprets one TS-890S command on the virtual rig; `text` is the command without `;`. It answers reads and applies sets for `FA`, `FB`, `OM`, `TX` and `RX` (virtual PTT flag, no RF), `KY`, `ID`, `AI`, `SM`, `FR`, `FT`, `TB`, `RT`, `XT`, `RC`, `RU`/`RD`, `RF`, the levels (`ts890_level`), `RA`/`PA`, `AN` (port 1 or 2 within the described ports), `BS` (`ts890_scope_command`), `DD0` and `DD1` scope outputs, `PS` and `FV`. A set returns no bytes; a read returns the answer; a malformed or unknown command returns `?;`. Virtual state only; it never touches hardware.

### `fn ts890_level(&mut self, name: &str, arg: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_ts890_level]

Virtual TS-890S level command. `name` is `AG`, `RG`, `MG`, `KS` or `PC`; `arg` the digits after it. An empty `arg` is a read: the stored value (the range minimum when never set) as `NAME` plus three digits and `;`. Otherwise exactly three digits that lie in the guide's range (AG and RG 0..255, MG 0..100, KS 4..60, PC 5..100) are stored and no reply is sent; anything else answers `Some("?;")`. Setting `PC` does not key anything.

### `fn default() -> Ts890Scope` [@ANCHOR: radioctl:radioctl_virtual_ts890_scope_default]

The starting bandscope state of a virtual TS-890S (our choice, not a radio's): display on, layout 0, center mode, span code 3 (50 kHz), range 0, expand off, attenuator 0 dB, averaging off, waterfall speed 1, shift position 4, reference code 40 (0 dB), both outputs off (`DD0` and `DD1` 0), sweep counter 0, and fixed-range edges of the 20 m band for range 0 and 40 m, 80 m and 15 m for ranges 1 to 3. Fields keep the guide's digits as `u8`. Called once when a virtual TS-890S is created.

### `fn ts890_scope_command(&mut self, sub: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_ts890_scope_command]

One `BS` command on the virtual rig; `sub` is what follows `BS`: a letter and a value. A bare letter reads the setting as `BS<letter><digit>;`; a letter with a one-digit value in the guide's range sets it (no reply). Covers `BS0` to `BSO`, including the mode (`BS3`: 0 center, 1 fixed, 2 auto scroll), span (`BS4` 0..6, and 9 meaning the initial 50 kHz), reference (`BSC`, three digits up to 60) and delegates `BSE` to `ts890_marker_shift` and `BSM` to `ts890_edges_command`. `BSL` (overflow) reads `BSL0;` only; `BSD` answers `BSD;` only when auto information is on. A change of mode or range drops any marker-centred edges. Refusal is `Some("?;")` for a bad value, waterfall speed 0, or an unknown letter.

### `pub(super) fn ts890_sweep(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_ts890_sweep]

Produces the bandscope data the virtual rig volunteers. It returns nothing unless auto information (`transceive`) is on, the description has a scope, and `DD0` is a LAN cycle (1 to 3) or 4. Each call advances the sweep counter, builds 640 deterministic synthetic samples (00 to 8C) as hexadecimal pairs, and returns one `##DD2...;` frame (LAN), or the 32 serial `DD2` splits (`DD0` 4). `DD0` 5 is not modeled. The samples are synthetic, not recorded from a radio. Mutates the counter; no I/O.

### `fn sweep_digits(count: usize, sweep: u32, max: u8) -> String` [@ANCHOR: radioctl:radioctl_virtual_ts890_sweep_digits]

Makes `count` synthetic samples for sweep number `sweep`, each at most `max`, as two uppercase hexadecimal digits per sample, concatenated. The sample values come from `civ_scope::sample_value` (a deterministic function of index and sweep), so the same inputs always give the same text and successive sweeps differ. Used for 640 bandscope samples (`max` 0x8C) and 213 filter-scope samples (0x32). Pure.

### `fn serial_splits(name: &str, digits: &str, split_digits: usize, filler: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_ts890_serial_splits]

Cuts a sweep's hexadecimal `digits` into serial frames `<name><two-digit split number><split_digits digits>;`, numbered from 00. `name` is `DD2` or `DD3`. The last chunk, if shorter than `split_digits`, is padded with repeats of `filler` (the guide gives no value for the unused digits; `DD3` pads with `00`, `DD2` is full). Returns all frames concatenated. Used for 32 splits of 40 digits (`DD2`) and 12 splits of 38 digits (`DD3`). Pure.

### `pub(super) fn ts890_filter_sweep(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_ts890_filter_sweep]

The virtual filter-scope data: nothing unless auto information is on, the description has a scope, and `DD1` is 1 (LAN) or 2 (COM/USB). It builds 213 synthetic samples (00 to 32 hexadecimal) for the current sweep number (it does not advance the counter; `ts890_sweep` does) and returns one `##DD3...;` frame for LAN, or the twelve serial `DD3` splits of 38 digits for `DD1` 2. Synthetic; no I/O.

### `fn ts890_edges_command(&mut self, arg: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_ts890_edges_command]

`BSM` on the virtual rig; `arg` is what follows `BSM`. Empty: reads the edges of the range in use as `BSM0` plus two eight-digit frequencies (`ts890_current_edges`). Otherwise 17 characters: a range digit 1 to 3 (range 0 cannot be set) and two eight-digit edges; with low below high they are stored for that range (no reply); two `99999999` values restore that range's initial edges. Wrong length, range 0 or above 3, non-digits, edges not low to high, or values that do not fit 32 bits answer `Some("?;")`.

### `fn ts890_current_edges(&self) -> (u32, u32)` [@ANCHOR: radioctl:radioctl_virtual_ts890_current_edges]

The lower and upper edge in hertz the virtual scope shows now. In fixed mode (and center mode) it is the edges the marker-center command set, or the selected range's edges. In auto scroll (our model; the guide gives none) it is the page of the span's width, aligned to multiples of the span, that holds the receive frequency, so the edges move when the dial leaves the page. The receive VFO is A or B per the virtual `FR`. Values above 32 bits saturate. Pure read.

### `fn ts890_marker_shift(&mut self) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_ts890_marker_shift]

The `BSE` command on the virtual rig. Center mode: refused (`Some("?;")`). Fixed mode: the displayed range is re-centred on the receive frequency, keeping its width, until the mode or range changes (stored in `centred_edges`); no reply. Auto scroll: no effect, no reply. This mirrors the guide's rule that marker shift or centre is invalid in center mode.

## `crates/radioctl-core/src/description.rs`

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_receiver_protocol_name]

Method of `ReceiverProtocol`: the name used in rig description files: `aor`, `uniden`, `jrc`, `rohde-schwarz`, `icom-pcr`, `icom-marine`. Returns a borrowed static string; no error case. The inverse is `from_name`, so every variant round trips. The names belong to the rig file format; changing one breaks shipped descriptions. Pure.

### `pub fn from_name(name: &str) -> Option<ReceiverProtocol>` [@ANCHOR: radioctl:radioctl_core_receiver_protocol_from_name]

Method of `ReceiverProtocol`: the protocol for a rig file name (`aor`, `uniden`, `jrc`, `rohde-schwarz`, `icom-pcr`, `icom-marine`), found by comparing against `name()` of every variant. Exact, case sensitive match; `None` for anything else, which `build_receiver` turns into a load error naming the bad value. Pure.

## `crates/radioctl-core/src/rigfile.rs`

### `fn build_usb(u: &UsbSection) -> Result<UsbParams, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_usb]

Builds the USB-device parameters of a description from its parsed `[usb]` section `u` (borrowed): protocol (by `UsbProtocol::from_name`), vendor id and product id (16 bit), the manufacturer and product strings (empty when not used to tell boards apart), and the Si570 multiplier. Errors, `Malformed`: an unknown `usb.protocol`, or a multiplier not in 0, 1, 2, 4 or 8 (0 for a device with none). Pure; run once at description load so a bad file never reaches an engine.

## `crates/radioctl-core/src/description.rs` (USB protocol names)

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_usbprotocol_name]

Method of `UsbProtocol`: the name used in rig files: `si570`, `si570-legacy`, `fifisdr`, `funcube`, `elektor507`, `dwt`, `perseus`. Returns a borrowed static string; the inverse of `from_name`. The names are part of the rig file format. Pure, no error case.

### `pub fn from_name(name: &str) -> Option<UsbProtocol>` [@ANCHOR: radioctl:radioctl_core_usbprotocol_from_name]

Method of `UsbProtocol`: finds the protocol whose `name()` equals `name`, scanning the table of all seven. Exact and case sensitive; `None` for anything else, which `build_usb` reports as an unknown `usb.protocol` and the description fails to load. Pure.

## `crates/radioctl-core/src/rigfile.rs` (receiver)

### `fn build_receiver(r: &ReceiverSection) -> Result<ReceiverParams, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_receiver]

Builds and checks the receiver parameters of a description from its `[receiver]` section `r` (borrowed). It resolves the protocol (`ReceiverProtocol::from_name`) and keeps the dialect name (the engine's tables are keyed by it, for example `ar8000`, `bc780`), address, baud rate, and line settings. Errors, all `Malformed`: unknown protocol; parity not `N`, `E` or `O`; data bits outside 5 to 8, stop bits outside 1 to 2 or baud 0; flow not `none`, `xonxoff` or `rtscts`; an empty dialect. Pure; run once at description load.

## `crates/radioctl-protocols/src/receiver.rs`

### `pub fn new(desc: RigDescription, protocol: ReceiverProtocol) -> Result<LineEngine<D>, RigError>` [@ANCHOR: radioctl:radioctl_receiver_engine_new]

Builds the line engine of a receiver or scanner description for the dialect type `D` (AOR, Uniden, JRC, Icom PCR, Icom marine or Rohde and Schwarz). It takes ownership of `desc` and requires `Family::Receiver` with receiver parameters whose protocol equals `protocol`, else `RigError` of kind `Unsupported` ("not a <name> description"). Then it asks the dialect to build itself from the description (`Dialect::from_description`, whose error is passed on). The new engine expects nothing and has an empty buffer. No I/O. Receivers have no transmit path: a dialect only encodes `SetPtt` or `SendCw` for the transceivers whose description lists the capability.

### `fn split_lines(&mut self) -> Vec<String>` [@ANCHOR: radioctl:radioctl_receiver_engine_split_lines]

Takes every complete line out of the engine's receive buffer. A line ends at the dialect's terminator byte (carriage return by default). For each, the terminator is removed, line feed characters are dropped, and the text is trimmed of spaces at both ends, so there is one entry per terminator and an empty entry is an empty line (a bare acknowledgement). Bytes after the last terminator stay in the buffer for the next call. Mutates the buffer; no I/O; never fails.
