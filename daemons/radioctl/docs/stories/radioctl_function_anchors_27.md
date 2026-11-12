# radioctl function anchors, batch 27

Anchors for the `radioctl-core` and `radioctl-protocols` crates (ADR 0113 standard): the conformance stand-ins, rig file section validators (timing, levels, scope), the Icom CI-V RIT offset, filter and BCD helpers, and the CI-V spectrum scope (command 27) codec, assembler and simulator. Tests cite each anchor with `Tests [@ANCHOR: ...]`.
Transmit gate: nothing here opens the transmit gate. The CI-V functions only encode, decode or simulate bytes; a scope or filter command does not key the radio, and the virtual rig functions change only simulated state.


## `conformance.rs`

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_conformance_stand_in_front_panel]

Makes the stand-in radio behave as if the operator turned a knob, and returns the bytes the radio would send unasked (an unsolicited report) so a transcript replay can check them. `&mut self` advances the stand-in's state; `panel` is the typed knob change by value (frequency, mode, PTT, signal, split, RIT offset).
The generic virtual rig accepts every knob through `panel_change`. The virtual FlexRadio accepts only frequency (tune), mode and PTT (MOX) and returns an `Err` string for any other knob; the vendor twin handles it itself. The HPSDR bridge, the radiod bridge and the USB board have no unsolicited output and return an `Err` naming that. Errors are messages, never panics. No hardware is touched.

### `fn feed(&mut self, bytes: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_conformance_bridge_stand_in_feed]

Stand-in for the `hpsdr_io` bridge's line protocol, used when replaying transcripts of the HPSDR network family, with no radio behind it. `bytes` is the controller's request, possibly several newline-separated lines, decoded with lossy UTF-8; `&mut self` keeps the frequency, PTT flag and attenuator state between calls.
Per line: `GF`, `GP`, `GW`, `GR` and `GA` answer frequency (`F <hz>`), PTT (`P 0|1`), power (`W 0`), SWR (`R 10`, meaning 1.0 in tenths) and attenuation (`A <db>`). `F <hz>` (if it fits in u32), `P 0|1` and `A <db>` with db up to 31 set the state and answer `OK`; anything else answers `ERR command`. The answers are concatenated and returned as bytes. A `P 1` only sets the simulated flag; no transmitter exists.

### `fn expected_inbound(expect: Expect) -> Inbound` [@ANCHOR: radioctl:radioctl_conformance_expected_inbound]

Converts a transcript's typed expectation (`Expect`, by value) into the `Inbound` value an engine produces when it decodes bytes, so the two can be compared with `==`. An observation maps to `Inbound::Observation` carrying the same value, `Ack` to `Inbound::Ack` and `Nak` to `Inbound::Nak`.
Total and pure: no errors, no panics, no I/O. Used by `check_decoding` for the `expect` and each `expect_more` of a step.

### `fn panel_change(panel: Panel) -> PanelChange` [@ANCHOR: radioctl:radioctl_conformance_panel_change]

Converts a transcript front-panel change (`Panel`, by value) into the generic virtual rig's `PanelChange` with the same value: frequency in hertz, mode, PTT, signal level, split and RIT offset. The two enums mirror each other; this function is the one place the mapping lives.
Total, pure, no errors, no I/O. Used by the stand-in's `front_panel` for the generic virtual rig, whose own `front_panel` then returns the unsolicited bytes.


## `rigfile.rs`

### `fn check_timing(timing: &TimingSection) -> Result<Timing, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_check_timing]

Validates the `[timing]` block of a description and turns it into a `Timing` of durations. `timing` is borrowed. `timeout_ms` must be in 1 to 30000 milliseconds and `inter_command_ms` at most 2000; `retries` is passed through unchanged.
The limits exist because the timing applies to every command, including an unkey: a huge timeout or inter-command delay would hold up or defeat an unkey on a stuck link. A value outside the range returns a `Malformed` `RigError` naming the key, the value and the limit. Pure, no I/O.

### `fn build_levels(section: Option<&LevelsSection>) -> Result<LevelParams, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_levels]

Validates the optional `[levels]` block and builds `LevelParams`. `section` is borrowed; an absent block gives the default (no attenuator steps, no preamp or antenna choices, nothing absent).
Rules: `attenuator_db` must be strictly ascending, positive and at most 9 values, or instead `attenuator_any_up_to_db` gives a whole-decibel maximum (the two are alternatives, not both); `preamp_steps` at most 6; `antenna_ports` at most 8; `absent` may name only plain levels (af-gain, rf-gain, mic-gain, monitor-level, keyer-speed, rf-power) and duplicates are merged, while attenuator, preamp and antenna cannot be listed absent. Violations return a `Malformed` `RigError`. Pure, no I/O.

### `fn build_scope(section: Option<&ScopeSection>, family: Family) -> Result<Option<ScopeParams>, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_scope]

Validates the optional `[scope]` block and builds `ScopeParams` (`Option`, `None` when the block is absent). `section` is borrowed; `family` is the rig's family. A scope block is accepted only for the `icom-civ` and `kenwood-ascii` families.
Values must be sane: `receivers` 1 or 2, and `line_length`, `samples_per_frame`, `amplitude_max` positive and the `spans_hz` list non-empty; otherwise a `Malformed` `RigError`. Receivers is the number of independent scopes (Main and Sub), line length the pixels per sweep, samples per frame how many pixels one CI-V frame carries, amplitude max the highest sample value, and spans the span choices in hertz. Pure, no I/O.


## `civ.rs`

### `fn scope_request(&self, command: ScopeCommand) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_civ_engine_scope_request]

Builds the CI-V request for one scope command (command 27) on a `CivEngine`. `&self` is read-only; `command` is a `ScopeCommand` by value. It sets the reply kind: a query (a get command) gives `ReplyKind::None`, since the answer arrives as a separate frame the decoder handles, and a set command gives `ReplyKind::Ack`, waiting for the radio's OK.
Errors (`Unsupported` `RigError`): the description has no scope, or a value does not fit its field (see `command_data`). Does not send anything and does not key the radio; the request is written by the caller.

### `fn scope_dropped(&self) -> u64` [@ANCHOR: radioctl:radioctl_civ_engine_scope_dropped]

Returns how many scope frames or whole sweeps the engine has discarded as out of order, oversized, short or malformed, since the engine was created. `&self`; the result is a `u64` counter, 0 when the model has no scope.
It reads the assembler's counter and is for diagnostics, so a flaky link shows up without the stream failing. Read-only, cheap, no errors.

### `fn encode_rit_offset(hz: i16) -> Option<[u8; 3]>` [@ANCHOR: radioctl:radioctl_civ_encode_rit_offset]

Encodes a RIT or XIT offset in hertz into the 3 data bytes that follow `21 00` in a CI-V frame: tens and ones as packed BCD, then thousands and hundreds as packed BCD, then a sign byte (1 is negative, 0 positive). `hz` is a signed 16-bit value by value; for example 150 gives `50 01 00`.
Returns `None` when the magnitude is above `MAX_OFFSET_HZ`, 9999 Hz, which is the most the field can hold. Pure, no I/O; the inverse is `decode_rit_offset`.

### `fn decode_rit_offset(bytes: &[u8]) -> Option<i16>` [@ANCHOR: radioctl:radioctl_civ_decode_rit_offset]

Decodes the 3 data bytes after `21 00` of a CI-V frame into a signed offset in hertz: low pair of digits, high pair of digits, then a sign byte, where 0 is positive and any other value negative. `bytes` is borrowed and must be exactly 3 bytes.
Returns `None` for any other length or when a nibble of a digit byte is above 9 (not valid BCD). The result is within -9999 to 9999. Pure, no I/O; the inverse of `encode_rit_offset`.

### `fn filter_index_to_hz(am: bool, index: u8) -> Option<u32>` [@ANCHOR: radioctl:radioctl_civ_filter_index_to_hz]

Converts an Icom filter index, as read with command `1A 03`, into a filter width in hertz. `am` selects the table: in AM the index 0 to 49 gives 200 to 10000 Hz in steps of 200; otherwise (SSB, CW, RTTY) the index 0 to 9 gives 50 to 500 Hz in steps of 50 and 10 to 40 gives 600 to 3600 Hz in steps of 100.
`index` is a plain binary index (the BCD decoding is done by the caller). Returns `None` outside the table. Pure, no I/O.

### `fn filter_hz_to_index(am: bool, hz: u32) -> Option<u8>` [@ANCHOR: radioctl:radioctl_civ_filter_hz_to_index]

Converts a filter width in hertz into the Icom filter index to send, the inverse of `filter_index_to_hz`. `am` selects the AM or the SSB/CW/RTTY table and `hz` must be exactly a width in that table, such as 2400 or 3600.
It searches indices 0 to 49 for an exact match and returns the first, or `None` when the table has no such width (the caller then refuses the set rather than rounding). Pure, no I/O.

### `fn decode_bcd_byte(byte: u8) -> Option<u16>` [@ANCHOR: radioctl:radioctl_civ_decode_bcd_byte]

Decodes one packed BCD byte of a CI-V frame to its number: the high nibble is the tens digit and the low nibble the ones digit, so `0x20` is 20 and `0x99` is 99. `byte` is passed by value.
Returns `None` when either nibble is above 9, which means the byte is not BCD and the frame is malformed or misaligned. The result is 0 to 99. Pure, no I/O.

### `fn decode_filter(&self, index: u8) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_civ_engine_decode_filter]

Decodes a filter index byte received from the radio into an observation of the filter width. `&self` is read-only; `index` is the BCD-coded index byte (for example `0x15` is index 15). A byte whose nibbles are not digits gives `None`.
Which table applies depends on the mode last reported by the radio, through `filter_table_is_am`: AM and the synchronous modes (SAM, SAM-L, SAM-U) use the AM table, every other mode except FM and WFM the SSB/CW/RTTY table. If the mode is not yet known, or is FM or WFM (which have no filter width), or the index is outside the table, the result is `None`. Otherwise `Some(Observation::FilterWidth(Hz))`. Pure.

### `fn decode_scope(&mut self, data: &[u8]) -> Option<Inbound>` [@ANCHOR: radioctl:radioctl_civ_engine_decode_scope]

Decodes the data of a CI-V command 27 frame from the radio. `&mut self` because a waveform frame goes into the scope assembler; `data` is borrowed and starts with the scope sub-command.
A waveform frame (sub-command `00`) is parsed with `parse_segment` and pushed to the assembler; only the frame that completes a sweep gives `Some(Inbound::ScopeFrame)` (receiver, axis kind, low and high frequency, out-of-range flag, maximum amplitude and samples; `seq` is 0 here). An unparseable waveform frame is counted as dropped and gives `None`. Setting answers (display, output, mode, span, reference) become observations; span is doubled because the radio reports half-span. Unknown sub-commands and models without a scope give `None`.


## `civ_scope.rs`

### `fn code(self) -> u8` [@ANCHOR: radioctl:radioctl_scope_mode_code]

Returns the wire code of a scope mode, as sent in command `27 14`: 0 for `Center`, 1 for `Fixed`, 2 for `ScrollCenter` and 3 for `ScrollFixed`. `self` is by value.
Total, pure, no errors; the inverse is `from_code`.

### `fn from_code(code: u8) -> Option<ScopeMode>` [@ANCHOR: radioctl:radioctl_scope_mode_from_code]

Returns the scope mode for a wire code, the inverse of `ScopeMode::code`: 0 is `Center`, 1 `Fixed`, 2 `ScrollCenter`, 3 `ScrollFixed`. `code` is a byte by value.
Any other value gives `None`, and the callers treat the frame as unrecognised rather than guessing a mode. Pure.

### `fn code(self) -> u8` [@ANCHOR: radioctl:radioctl_scope_speed_code]

Returns the wire code of a scope sweep speed: 0 for `Fast`, 1 for `Mid` and 2 for `Slow`. `self` is by value.
Total, pure, no errors; the inverse is `ScopeSpeed::from_code`. The code is sent as the data byte of the speed sub-command.

### `fn from_code(code: u8) -> Option<ScopeSpeed>` [@ANCHOR: radioctl:radioctl_scope_speed_from_code]

Returns the scope sweep speed for a wire code, the inverse of `ScopeSpeed::code`: 0 is `Fast`, 1 `Mid`, 2 `Slow`. `code` is a byte by value.
Any other value gives `None`. Pure, no side effects.

### `fn is_query(self) -> bool` [@ANCHOR: radioctl:radioctl_scope_command_is_query]

Says whether a scope command reads a setting (`GetDisplay`, `GetOutput`, `GetMode`, `GetSpan`, `GetReference`, `GetSpeed`) rather than changes one. `self` is by value.
It decides the expected reply: a read is answered with a data frame, not the plain OK/NG acknowledgement that a set command gets, so `scope_request` gives queries `ReplyKind::None` and sets `ReplyKind::Ack`. Pure, total, no errors.

### `fn command_data(command: ScopeCommand) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_scope_command_data]

Builds the sub-command and data bytes, the part after the `27` command byte, for a scope command. `command` is a `ScopeCommand` by value: set or get of display, output, mode, span, hold, reference level and speed, or set edges.
The receiver number must be 0 or 1. A span is sent as half the span in hertz as BCD frequency; a reference level is in hundredths of a dB, magnitude up to 9999, sent as two BCD bytes and a sign byte (1 is negative); edges are BCD range and edge numbers plus two BCD frequencies. Returns `None` when a value does not fit its field (receiver above 1, reference beyond 9999, a frequency that does not encode). Pure, no I/O.

### `fn decode_reference(bytes: &[u8]) -> Option<i16>` [@ANCHOR: radioctl:radioctl_scope_decode_reference]

Decodes the scope reference level, in hundredths of a dB, from the 3 data bytes after `27 19 <receiver>`: hundreds-of-hundredths pair (BCD), units pair (BCD) and a sign byte where 0 is positive and anything else negative. `bytes` is borrowed and must be exactly 3 bytes.
Returns `None` for any other length or non-BCD bytes. The result is -9999 to 9999 (that is -99.99 to +99.99 dB). Pure; the inverse of the encoding in `command_data`.

### `fn parse_segment(data: &[u8]) -> Option<ScopeSegment>` [@ANCHOR: radioctl:radioctl_scope_parse_segment]

Decodes one waveform frame, the data after `27 00`: receiver byte, BCD frame number (division), BCD frame count (divisions), then the rest. `data` is borrowed.
The receiver must be 0 or 1 and the division from 1 up to divisions, else `None`. Frame 1 also carries a header: mode code, two 5-byte BCD frequencies (edge a, edge b), an out-of-range flag, then the first samples; a short or bad header gives `None`. Later frames carry samples only. The result is a `ScopeSegment` owning copies of the samples. Pure; the caller counts a `None` as a dropped frame.

### `fn new(params: ScopeParams) -> ScopeAssembler` [@ANCHOR: radioctl:radioctl_scope_assembler_new]

Creates the assembler that joins waveform frames into complete sweep lines for one model. `params` is the model's `ScopeParams` (receivers, line length in pixels, samples per frame, maximum amplitude, spans), taken by value.
It starts with no partial sweep for either receiver and a dropped count of 0. Cannot fail. One assembler serves both receivers of a model; its state is mutated by `push`.

### `fn dropped(&self) -> u64` [@ANCHOR: radioctl:radioctl_scope_assembler_dropped]

Returns the number of frames or sweeps the assembler has thrown away since it was created, as a `u64`. `&self`, read-only. The count rises for a frame from a receiver the model does not have, a frame out of order or without its first frame, a sweep that is longer or shorter than the model's line length, and malformed frames noted by `note_malformed`.
It never resets. Used for diagnostics only. No errors.

### `fn note_malformed(&mut self)` [@ANCHOR: radioctl:radioctl_scope_assembler_note_malformed]

Adds one to the assembler's dropped count. `&mut self`. It is called by the engine when a waveform frame could not be parsed at all, so such frames are visible in the count, because they never reach `push`.
Never fails; the counter is a `u64` and only grows. No other side effects.

### `fn push(&mut self, segment: ScopeSegment) -> Option<ScopeLine>` [@ANCHOR: radioctl:radioctl_scope_assembler_push]

Adds one decoded waveform frame (`ScopeSegment`, by value) and returns the finished sweep when this frame completes it. `&mut self`. A frame for a receiver beyond the model's count is dropped. Frame 1 starts a new partial sweep for its receiver, replacing any old one; a later frame must carry exactly the next expected number, otherwise the partial sweep is discarded and the drop counted.
If the samples exceed the model's `line_length`, the sweep is discarded. On the last frame (division equal to divisions) the sweep must have exactly `line_length` samples or it is dropped; otherwise `Some(ScopeLine)` is returned with its low and high frequency computed. Returns `None` for every incomplete or dropped case. No I/O.

### `fn new(params: &ScopeParams) -> ScopeSim` [@ANCHOR: radioctl:radioctl_scope_sim_new]

Creates the simulated scope that stands in for a radio's scope in the virtual rig. `params` is borrowed and cloned. The power-on state is: display on, waveform output off, `Center` mode, hold off, reference 0, speed `Mid`, and the narrowest span of the model (half-span half of it; 10000 Hz if the list is empty), one such state per receiver.
No fixed edges are set and the sweep counter is 0. Cannot fail. Simulation only, no hardware.

### `fn output_on(&self) -> bool` [@ANCHOR: radioctl:radioctl_scope_sim_output_on]

Says whether the simulated scope's waveform output is switched on, which is what command `27 11` sets. `&self`, read-only, returns a `bool`; it starts as false.
The virtual rig checks it to decide whether to push sweeps without being asked. No errors.

### `fn handle(&mut self, data: &[u8]) -> ScopeAnswer` [@ANCHOR: radioctl:radioctl_scope_sim_handle]

Answers a scope command for the simulated radio. `&mut self`; `data` is borrowed and holds the bytes after `27`, sub-command first. It runs the command with the private handler and wraps the outcome: a set command that is accepted gives `ScopeAnswer::Ok`, a query gives `ScopeAnswer::Data(bytes)` with the current value, and anything the model refuses (unknown sub-command, bad receiver, out-of-range value) gives `ScopeAnswer::Refused`, which the virtual rig turns into an NG.
It changes only the simulated scope state. Never panics on bad input.

### `fn sweep(&mut self, receiver: u8, tuned: Hz) -> Vec<Vec<u8>>` [@ANCHOR: radioctl:radioctl_scope_sim_sweep]

Produces one sweep of waveform frames from the simulated scope. `&mut self`; `receiver` is 0 or 1; `tuned` is the receiver's frequency in hertz, used to place the axis. Returns a list of frames, each the data after `27 00` without the CI-V envelope, in order.
It returns an empty list when output is off, the receiver does not exist, or hold is on. Otherwise it advances the sweep counter, builds the first frame (receiver, BCD division and count, mode code, two BCD edges, out-of-range flag 0) and splits the deterministic sample ramp across frames of `samples_per_frame`. Centred modes give centre and half-span; fixed modes give the set edges or tuned minus and plus half-span. An edge that cannot be encoded as BCD gives no frames. No hardware.

### `fn sample_value(index: usize, sweep: u32, amplitude_max: u16) -> u8` [@ANCHOR: radioctl:radioctl_scope_sample_value]

Gives the synthetic amplitude of one pixel of a simulated sweep, so a reassembled line can be checked sample by sample. `index` is the pixel (0 up to the line length), `sweep` the sweep counter (wrapping), `amplitude_max` the highest value, 160 for a typical model.
The value is `(index * 7 + sweep) mod (amplitude_max + 1)`, with wrapping arithmetic, so it moves each sweep. It is converted to a byte, with 255 as the cap if `amplitude_max` exceeded a byte (not expected). Pure, deterministic, no errors.


## `virtual_rig.rs`

### `fn civ_slot(&mut self, other: bool) -> &mut Hz` [@ANCHOR: radioctl:radioctl_virtual_civ_slot]

Picks the frequency cell of the simulated radio a CI-V command acts on. `&mut self`; `other` is false for the selected VFO and true for the other one. It returns a mutable reference to `freq_a` or `freq_b` in the state, depending on whether VFO B is the selected one (`civ_b_selected`) and on `other`.
This is how the commands that address the selected VFO and the ones that address the unselected VFO share code. No errors; changes nothing by itself, the caller writes through the reference.

### `fn civ_set_freq(&mut self, params: &CivParams, other: bool, bcd: &[u8]) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_set_freq]

Sets a VFO frequency of the virtual radio from a CI-V frequency field. `&mut self`; `params` is the model's `CivParams` (its frequency field length, 4 to 6 bytes); `other` chooses the selected or the unselected VFO; `bcd` is the frequency bytes, borrowed.
The field is decoded with `decode_bcd_freq` and must be exactly the model's field length, else the reply is `CivReply::Ng` and nothing changes. On success the chosen VFO is set and `CivReply::Ok` returned. Simulated state only; no hardware and no transmitter.
