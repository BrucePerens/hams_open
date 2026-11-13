# radioctl function anchors, batch 28

Anchors for the `radioctl-protocols` crate (ADR 0113 standard): the virtual rig's Icom CI-V and Kenwood command handlers, and the Icom CI-V and Kenwood engine encoders and decoders for levels, filter width, modes, meters, RIT/XIT, the front end (attenuator, preamplifier) and the Kenwood `IF` answer. Tests cite each anchor with `Tests [@ANCHOR: ...]`.
Transmit gate: nothing here opens the transmit gate. Virtual rig functions change only simulated state; the engine functions only build or read byte strings. Setting a level or a mode never keys the radio, and the gate is applied by the layer that sends PTT (push to talk) or send-cw.


## `virtual_rig.rs`

### `fn civ_front_end_get(&self, level: Level) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_front_end_get]

Answers a read of the front end on the virtual Icom radio: command `11` (attenuator, one BCD byte of decibels) or `16 02` (preamplifier step). `&self`; `level` is `Level::Attenuator` for the first and the preamplifier for the second.
It asks `attenuation::virtual_get` for the stored value according to the description. For the attenuator it replies `CivReply::Data(0x11, [BCD dB])`, for the preamplifier `Data(0x16, [0x02, step])`. A model that lacks the level gives `CivReply::Ng`. A value too large for a byte is replaced with 0 in the reply. Simulated state only; no hardware.

### `fn civ_front_end_set(&mut self, level: Level, byte: u8) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_front_end_set]

Handles a set of the attenuator (`11`) or preamplifier (`16 02`) on the virtual Icom radio. `&mut self`; `level` says which; `byte` is the data byte as received. For the attenuator the byte is two BCD digits of decibels and a non-BCD byte (a nibble above 9) is answered with `CivReply::Ng`; for the preamplifier it is the step number as a plain byte.
The value is passed to `attenuation::virtual_set`, which accepts only values the description lists (for example attenuation of 20 dB only if the model has that step). `CivReply::Ok` on success, `Ng` otherwise. Changes only the simulated levels.

### `fn civ_level_get(&self, sub: u8) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_level]

Answers a `14 <sub>` level read on the virtual Icom radio (the function is `civ_level_get`). `&self`; `sub` is the sub-command byte (AF 01, RF (radio frequency) 02, power 0A, mic 0B, keyer speed 0C, monitor 15). The level is found with `civ_level_of`; a sub-command that is unknown, or a level the description marks as absent, gives `CivReply::Ng`.
For the keyer speed the rig stores words per minute while the wire carries the 0 to 255 scale, so the value is converted with `keyer_wpm_to_raw`. The reply is `CivReply::Data(0x14, [sub, BCD value (2 bytes)])`. A level with no stored value is also `Ng`. Read-only; no hardware.

### `fn civ_level_set(&mut self, sub: u8, value: &[u8]) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_level_set]

Handles a `14 <sub>` level set on the virtual Icom radio. `&mut self`; `sub` is the sub-command byte and `value` is the BCD data (two bytes, `0000` to `0255`). It looks up the level with `civ_level_of` and decodes the value with `decode_bcd_meter`.
Refused with `CivReply::Ng`: unknown sub-command, value that is not valid BCD or is above 255, and a level the description lacks. For the keyer speed the raw value is converted to words per minute with `keyer_raw_to_wpm` (above 250 is refused). On success the value is stored in the simulated levels and `CivReply::Ok` returned.

### `fn civ_filter_get(&self) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_filter]

Answers the filter width read (`1A 03`) on the virtual Icom radio (the function is `civ_filter_get`). `&self`. The stored width in hertz is converted to an index with `filter_hz_to_index`, using the AM table when the simulated mode is AM, SAM, SAM-L or SAM-U and the SSB/CW/RTTY table otherwise.
Reply: `CivReply::Data(0x1A, [0x03, BCD index])`. If the stored width is not a width of that table (or does not fit in 32 bits) the answer is `CivReply::Ng`. Read-only, no hardware.

### `fn civ_filter_set(&mut self, byte: u8) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_filter_set]

Handles a filter width set (`1A 03 <index>`) on the virtual Icom radio. `&mut self`; `byte` is the BCD index (for example `0x15` is index 15). A byte with a nibble above 9 is refused with `CivReply::Ng`.
The index is converted to hertz with `filter_index_to_hz`; the table depends on the simulated mode (AM, SAM, SAM-L and SAM-U use the AM table). An index outside the table gives `Ng`; otherwise the stored `filter_width` changes and `CivReply::Ok` is returned. Because the table follows the mode, the same index means a different width after a mode change. Simulated state only.

### `fn scope_sweep(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_scope_sweep]

Gives the bytes the virtual radio sends on its own for one scope sweep of the Main receiver, at the VFO (variable frequency oscillator, the tuning control) A frequency, as unsolicited frames (built by `unsolicited_civ`, not answers to a request). `&mut self`; each call advances the sweep counter.
For a TS-890 it returns the Kenwood sweep instead (`ts890_sweep`). For an Icom CI-V model with a scope, each frame of the simulated scope's sweep is wrapped as command 27, sub-command `00`, in a CI-V envelope. The result is empty when the model has no scope, waveform output is off, or the scope is held. No hardware.

### `fn filter_scope_sweep(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_filter_scope_sweep]

Gives the bytes of the `##DD3` filter-scope frame that a TS-890S sends with its waveform output on LAN. `&mut self`. For a TS-890 it returns what `ts890_filter_sweep` builds; for any other model, or when output is off, it returns an empty vector.
It exists so the TS-890 engine's decoder for the filter scope can be tested without the radio. Simulated, no hardware, no errors.

### `fn civ_level_of(sub: u8) -> Option<Level>` [@ANCHOR: radioctl:radioctl_virtual_civ_level_of]

Maps a `14` sub-command byte to the level it carries: `01` AF gain, `02` RF gain, `0A` RF power, `0B` microphone gain, `0C` keyer speed, `15` monitor level. `sub` is by value. Any other byte gives `None`.
It is the virtual rig's side of the table; the engine's `level_sub` is the inverse and the two must agree, which the tests check by driving the engine against the virtual rig. Pure, no errors.


## `civ.rs`

### `fn level_sub(level: Level) -> Option<u8>` [@ANCHOR: radioctl:radioctl_civ_level_sub]

Maps a level to its `14` sub-command byte, for the levels the CI-V engine supports: AF gain `01`, RF gain `02`, RF power `0A`, mic gain `0B`, monitor level `15` and keyer speed `0C` (`KEYER_SUB`). `level` is by value.
Returns `None` for levels that are not carried under `14`: attenuator, preamplifier, dither, preselector and antenna (the first two have their own commands `11` and `16 02`; the rest are not implemented for CI-V). Callers turn `None` into an `Unsupported` error. Pure.

### `fn keyer_wpm_to_raw(wpm: u16) -> Option<u16>` [@ANCHOR: radioctl:radioctl_civ_keyer_wpm_to_raw]

Converts a keyer speed in words per minute to the raw value of CI-V `14 0C`. `wpm` is by value and must be 6 to 48 (`KEYER_WPM_MIN` is 6, the table has 43 steps). The raw values are not linear: the table `KEYER_RAW` runs 0, 7, 12, 19 up to 250, taken from the IC-7300 family's behaviour; the header says it is unverified on a radio.
Returns `None` for a speed below 6 or above 48. Pure, no I/O. The inverse is `keyer_raw_to_wpm`.

### `fn keyer_raw_to_wpm(raw: u16) -> Option<u16>` [@ANCHOR: radioctl:radioctl_civ_keyer_raw_to_wpm]

Converts a raw `14 0C` value to words per minute. `raw` is by value, 0 to 255 on the wire. It returns the slowest speed whose table value is not below `raw`, so a value between two steps rounds up to the faster one.
Returns `None` above 250, the top of the table, which the radio never sends. Result range 6 to 48. Pure, no I/O; the inverse is `keyer_wpm_to_raw`.

### `fn meter_sub(meter: Meter) -> u8` [@ANCHOR: radioctl:radioctl_civ_meter_sub]

Maps a raw meter to its CI-V `15` sub-command byte, following Hamlib's `icom_defs.h`: SWR (standing wave ratio) `12`, ALC (automatic level control) `13`, compression `14`, supply voltage (Vd) `15`, drain current (Id) `16`. `meter` is by value. Total, pure, no errors.
The inverse is `sub_meter`. Reading a meter never keys the radio; the transmit meters read zero while unkeyed.

### `fn sub_meter(sub: u8) -> Option<Meter>` [@ANCHOR: radioctl:radioctl_civ_sub_meter]

Maps a `15` sub-command byte back to the raw meter it carries, the inverse of `meter_sub`. `sub` is by value. It searches all meters (`Meter::ALL`) and returns the one whose sub-command equals `sub`, or `None` for an unknown byte (for example the S-meter `02`, which is a different command).
Used when decoding a reply so that the right observation is produced. Pure, no errors.

### `fn base_mode(mode: Mode) -> (Mode, bool)` [@ANCHOR: radioctl:radioctl_civ_base_mode]

Splits a mode into the sideband to send and the DATA flag. `mode` is by value. `Data` is USB with DATA on and `DataR` is LSB (lower sideband) with DATA on; every other mode is itself with DATA off. The result is `(Mode, bool)`.
Icom radios express a data mode as a sideband plus a flag carried by command `26`; radioctl has separate `Data` and `DataR` modes. Total, pure. The inverse is `mode_with_data`.

### `fn mode_with_data(base: Mode, data: bool) -> Option<Mode>` [@ANCHOR: radioctl:radioctl_civ_mode_with_data]

Combines a sideband and the DATA flag into a radioctl mode, the inverse of `base_mode`. `base` is the plain mode and `data` the DATA flag. USB with DATA is `Data`, LSB with DATA is `DataR`, and any mode with DATA off is itself.
Returns `None` for DATA on a mode with no data variant in radioctl's vocabulary (AM-D and FM-D exist on the radios but not here). Callers refuse or ignore the reply. Pure.

### `fn has_data_modes(&self) -> bool` [@ANCHOR: radioctl:radioctl_civ_engine_has_data_modes]

Says whether the description lists the `data` mode, which means the model is an Icom with USB plus DATA. `&self`. True when `mode_code(Mode::Data)` exists.
Such a model takes command `26` for the mode, which carries the DATA flag that `04` and `06` lack, so the engine uses `26` for reading and setting the mode. Pure, no errors.

### `fn decode_mode(&mut self, code: u8, data: bool, filter: Option<u8>) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_civ_engine_decode_mode]

Decodes a mode reply into an observation. `&mut self`; `code` is the radio's mode code byte from a `04`/`01` reply (`code filter`) or a `26 00` reply (`code data filter`); `data` is the DATA flag; `filter` is the filter number when the reply has one.
The code is translated to a plain mode through the description, then combined with DATA by `mode_with_data`. A code the description does not list, or DATA on a mode with no data variant, gives `None`. As a side effect a filter number of 1 to 3 is remembered in `filter_number` for the next `26 00` set. Returns `Some(Observation::Mode)`.


## `virtual_rig.rs`

### `fn meter(&self, meter: Meter) -> u16` [@ANCHOR: radioctl:radioctl_virtual_state_meter]

Reads a raw CI-V meter from the virtual radio's state. `&self`; `meter` is the meter wanted; the result is a 0 to 255 style raw value as `u16`. The supply voltage (`Vd`) reads its fixed value at any time; the other meters (SWR, ALC, compression, drain current) read their `KEYED_METERS` value only while the simulated radio is keyed and 0 otherwise.
So the SWR of an unkeyed virtual radio is 0, as a real radio with no transmitter running shows no reflection. Pure, read-only.

### `fn civ_mode_set(&mut self, code: u8, data: bool) -> CivReply` [@ANCHOR: radioctl:radioctl_virtual_civ_mode_set]

Sets the virtual radio's mode from a CI-V mode code and the DATA flag. `&mut self`; `code` is the mode byte, `data` the DATA flag (command `26` has it; `06` does not, so it lands on the plain mode, a choice the source marks as a bench question).
The code is looked up in the description and reduced with `base_mode`, then `mode_with_data` combines it. An unknown code, or DATA on a mode with no data variant (AM, FM), gives `CivReply::Ng` and leaves the mode alone. Otherwise the simulated mode changes and `CivReply::Ok` is returned.


## `civ.rs`

### `fn filter_table_is_am(&self) -> Result<bool, RigError>` [@ANCHOR: radioctl:radioctl_civ_engine_filter_table_is_am]

Chooses which filter index table applies, by the mode the engine last saw. `&self`; returns `Ok(true)` for AM, SAM, SAM-L and SAM-U, `Ok(false)` for the other modes with a filter width.
Errors are `Unsupported` `RigError`s: in FM and wide FM there is no filter width, and when no mode has been read yet the message says to read the mode before the filter width, because the SSB/CW/RTTY and AM index tables differ and guessing would give a wrong width. Pure.

### `fn encode_vfo_b_or_split(&self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_civ_engine_encode_vfo_b_split]

Encodes the operations on VFO B and on split for Icom radios. `&self`; `op` is the operation. Handled: `GetFrequency(B)` and `SetFrequency(B, hz)` as command `25 01` (the set carries the BCD frequency), and `GetSplit` and `SetSplit` as command `0F` with a flag byte.
Returns `None` for any other operation, so the caller tries the next group; `Some(Err)` when the frequency does not fit the model's field. Builds a request only; nothing is sent. Setting split does not by itself key the radio.

### `fn encode_main_sub(&self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_civ_engine_encode_main_sub]

Encodes the Main/Sub band ops of a dual-receiver Icom, by the description's `civ.main_sub` level. `SelectReceiver(Main)` is command `07` with `D0` and `Sub` is `D1`, on both levels (`select`: IC-9100, `full`: IC-7610); on `full` only, `GetReceiver` is `07 D2` and `SetDualWatch(false/true)` is `07 C0`/`C1` with `GetDualWatch` `07 C2`. Replies `07 D2 00/01` and `07 C2 00/01` decode in `decode` to `Receiver` and `DualWatch`. Every other receiver op (the transmit band included) and a level that lacks the command return an `Unsupported` error; a non-receiver op returns `None` so the other encoders try it. Reads the description only; pure, nothing is sent.

### `fn encode_rit_xit(&self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_civ_engine_encode_rit_xit]

Encodes RIT, XIT (transmitter incremental tuning) and RIT offset operations for Icom radios. `&self`; `op` is the operation. `GetRit` and `SetRit` use `21 01`, `GetXit` and `SetXit` use `21 02` (a flag byte for set), and `GetRitOffset` and `SetRitOffset` use `21 00`, the set carrying the 3 bytes from `encode_rit_offset`.
An offset beyond 9999 Hz is refused with an `Unsupported` `RigError`. Any other operation returns `None` so another encoder can handle it. Builds a request only.

### `fn encode_f8101(&mut self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_civ_engine_encode_f8101]

Encodes the few commands the IC-F8101 does differently: the frequency set for VFO A as `1A 35` with the 5-byte field, the mode read as `1A 34`, and the mode set as `1A 36 00 <code>`. `&mut self`; `op` is the operation.
A mode not in the description gives an `Unsupported` `RigError`; a set mode is remembered in the engine's `mode`. Any other operation returns `None`. The source says the facts are from Hamlib 4.6.2 (`icf8101.c`), the radio's own table was not read, and the commands are unverified on a radio. Builds requests only.

### `fn encode_front_end(&self, level: Level, value: u16) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_civ_engine_encode_front_end]

Encodes a set of the attenuator (`11`, one BCD byte of decibels) or the preamplifier (`16 02`, a step number). `&self`; `level` selects which and `value` is decibels or the step.
It first checks that the description lists the value (`attenuation::step_of`), so a value the model does not offer is refused here rather than sent. Attenuation above 99 does not fit one BCD byte and is refused. Errors are `Unsupported` `RigError`s. Builds a request only.

### `fn encode_levels_and_filter(&self, op: &Op) -> Option<Result<Request, RigError>>` [@ANCHOR: radioctl:radioctl_civ_engine_encode_levels_and_filter]

Encodes the level and filter-width operations for Icom radios. `&self`; `op` is the operation. Attenuator and preamplifier reads and sets go to their own commands (`11`, `16 02`); other levels go through `14 <sub>` with a BCD value 0 to 255; keyer speed is converted from words per minute (6 to 48) with `keyer_wpm_to_raw`.
A level the description lacks, a level not implemented for CI-V, a value above 255 and a keyer speed outside 6 to 48 are refused with `Unsupported` `RigError`s. The filter-width operations follow the same group; the remainder of the function was not read for this entry, check the source for them. Returns `None` for other operations. Builds requests only.


## `civ_scope.rs`

### `fn line_from(receiver: u8, partial: Partial) -> ScopeLine` [@ANCHOR: radioctl:radioctl_scope_line_from]

Builds a finished `ScopeLine` from the partial sweep the assembler collected. `receiver` is 0 or 1; `partial` is taken by value and holds the frame-1 header and all samples.
In the centred modes (`Center`, `ScrollCenter`) the header's two edges are centre and half-span, so low is centre minus half-span and high is centre plus half-span, both saturating at the integer limits. In the fixed modes the two edges are low and high as sent. The out-of-range flag and the samples are copied. Pure, no errors.

### `fn try_handle(&mut self, data: &[u8]) -> Result<Option<Vec<u8>>, ()>` [@ANCHOR: radioctl:radioctl_scope_sim_try_handle]

Does the work of the simulated scope's `handle`: runs one scope command and returns `Ok(Some(reply))` for a query, `Ok(None)` for an accepted set, or `Err(())` for a refusal. `&mut self`; `data` starts with the sub-command byte.
Display and output (`10`, `11`) are read with no data and set with one flag byte. Mode (`14`), span, hold, reference, speed and edges follow the same pattern per receiver. An empty command, a receiver number beyond the simulated receivers, an unknown sub-command or an invalid value gives `Err(())`. Only the simulated state changes. The sub-commands after mode were not read for this entry; see the source.


## `kenwood.rs`

### `fn decode_if(&self, body: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_kenwood_engine_decode_if]

Decodes the answer to the Kenwood `IF` (information) command. `&self`; `body` is the answer without `IF` and `;`, which must be 35 ASCII characters (an R-5000 answer of 30 is widened by inserting five zeros after the frequency). Fields read: frequency (11 digits), the RIT/XIT offset region (10 characters, `parse_if_offset`), RIT (receiver incremental tuning) flag, XIT flag, transmit flag and split flag.
Returns `Some(list of Inbound observations)` or `None` when the length, character set or any field is wrong. Pure. The transmit flag is only reported, nothing is keyed.

### `fn level_wire(level: Level) -> LevelWire` [@ANCHOR: radioctl:radioctl_kenwood_level_wire]

Describes how a level is written on the Kenwood wire. `level` is by value; the result is `LevelWire` with the two-letter command name, whether it carries a main/sub receiver digit, and the minimum and maximum values. AF gain `AG` 0 to 255 (with the digit), RF gain `RG` 0 to 255, mic gain `MG` 0 to 100, monitor `ML` 0 to 9, keyer speed `KS` 10 to 60, RF power `PC` 5 to 100.
The ranges are the intersection of the TS-2000 and TS-590 guides. Attenuator (`RA`) and preamplifier (`PA`) have their own forms and range 0 here; dither, preselector and antenna have no Kenwood command, and the encoder refuses them first. Pure.

### `fn decode_level(name: &str, digits: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_kenwood_decode_level]

Decodes a Kenwood level answer. `name` is the two-letter command (`AG`) and `digits` the rest (`0128` for AF gain 128, where the leading `0` is the main receiver digit). It finds the level whose wire name is `name` among AF gain, RF gain, mic gain, monitor level, keyer speed and RF power.
For levels with the main digit it strips a leading `0`; the number must then be exactly 3 digits. Returns `Some(Observation::Level(level, value))` or `None` for an unknown name, a missing digit or a non-numeric or wrong-length number. Pure.

### `fn parse_if_offset(region: &str) -> Option<i16>` [@ANCHOR: radioctl:radioctl_kenwood_parse_if_offset]

Reads the RIT/XIT offset from the 10 characters of an `IF` answer that hold its P2 and P3 fields. `region` is borrowed. The last `+` or `-` in the region is the sign, and 4 or 5 digits follow it to the end; the sign must be at position 4 or later.
Returns the signed offset in hertz, or `None` when there is no sign, the digit count is not 4 or 5 or the digits are not numeric. The two digit counts cover the TS-2000 style (5 digits) and the TS-590, TS-990 style (4 digits after five filler characters). Pure.


## `virtual_rig.rs`

### `fn kenwood_if(&self) -> String` [@ANCHOR: radioctl:radioctl_virtual_kenwood_if]

Builds the virtual Kenwood radio's `IF` answer as a `String`. `&self`. The RIT/XIT offset region depends on the model: TS-990 and TS-590S or SG (ID 021 and 023) put five spaces and a signed 4-digit offset, the TS-440 five zeros and a signed 4-digit offset, other models a 4-digit step and a signed 5-digit offset.
The frequency is that of the receive VFO (A when `kenwood_rx_vfo` is 0, else B) and the mode is the description's mode code as a character, `0` if unknown. The R-5000's answer lacks five characters after the frequency; how the function shortens its answer for that model is in the source beyond this entry (`unknown`, check it). The rest of the layout (flags and so on) is in the source beyond this entry. Read-only, simulated.

### `fn kenwood_select_vfo(&mut self, arg: &str, receive: bool) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_kenwood_select_vfo]

Handles the Kenwood commands that choose the receive VFO (`FR`) or the transmit VFO (`FT`) on the virtual radio. `&mut self`; `arg` is the argument text and `receive` is true for the receive command. An argument of `1` selects VFO B and anything else VFO A.
Afterwards the simulated split flag is set when the receive and transmit VFOs differ. Returns `None`, meaning there is no answer line. Selecting the transmit VFO only sets a simulated flag; no transmitter is keyed.

### `fn kenwood_move_offset(&mut self, up: bool, digits: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_kenwood_move_offset]

Handles the Kenwood commands that move the RIT/XIT offset up or down (`RU` and `RD`) on the virtual radio. `&mut self`; `up` gives the direction; `digits` must be exactly 5 digits, the amount in hertz.
The new offset is the current one plus or minus the amount and must stay within plus or minus 9999 Hz. A bad digit string or a result out of range returns `Some("?;")`, the Kenwood error answer, and leaves the offset unchanged. On success the offset is stored and `None` is returned (no answer line).
