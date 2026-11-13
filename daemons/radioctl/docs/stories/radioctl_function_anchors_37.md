# radioctl function anchors, batch 37

Function anchors for the Yaesu legacy block protocol (engine, description frequency forms, rig-file builders), the CI-V frequency field helpers, the shared front-end level (attenuator and preamplifier) module, and the Kenwood dialect and handheld helpers in `radioctl-core` and `radioctl-protocols`, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Anything that can key a transmitter says how the gate applies: the engines here only encode and decode bytes; the transmit gate, band check and frequency check are applied by the rig layer before an engine is asked to encode a key-on, and the virtual rigs have no hardware.

## `crates/radioctl-protocols/src/yaesu_legacy.rs`

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_ylegacy_engine_encode]

The `ProtocolEngine` encode of the legacy five-byte engine: turns one `Op` into a five-byte block (four parameters, then the opcode) and records which answer `feed` must count bytes for. `op` is borrowed. It first requires the op's capability in the description (`Unsupported` otherwise). A block-protocol model (a `block` table in the description) goes to `encode_block`, which also arms the echo check; the classic FT-817 family is handled inline: frequency set and read (`01`, `03`), mode (`07`), PTT (`08`/`88`), split, clarifier on/off and offset (`F5`), RX (receive) and TX status reads (`E7`, `F7`).

Returns the `Request` (bytes plus the `ReplyKind`). Errors, all `Unsupported`: a frequency not a multiple of 10 Hz or too large, a mode without a code, a clarifier offset over 9990 Hz or not a multiple of 10, VFO (variable frequency oscillator, the tuning control) B reads (no second VFO), Morse, and any other op. Keying: `SetPtt(true)` is opcode `08`; the engine does not decide whether transmitting is allowed, the rig layer does before calling. Side effect: it sets `pending` and clears the receive buffer, so call it once per request, in order.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_ylegacy_engine_feed]

The `ProtocolEngine` feed of the legacy engine. These radios send no framing: a read answers a fixed number of bytes, so the engine counts. `bytes` are newly received serial bytes (borrowed); `now` is the monotonic arrival time. If the buffer holds a partial answer older than `PARTIAL_FRAME_TIMEOUT` (500 ms), the stale bytes come out as `Inbound::Unrecognised` and the new bytes start afresh.

Order of work: if the model echoes commands (FT-767GX style), the first bytes are compared with the sent command and give `Ack` or `Nak`. Then it waits for the number of bytes the pending read needs (5 for frequency and mode, 1 for RX or TX status, the table length for a block read) and decodes: S-meter from the low nibble of the RX status; PTT (bit 7 clear means transmitting, Hamlib's reading), split (bit 5 clear means on) and the power nibble from the TX status. Bytes with nothing pending, and extra bytes, are reported as `Unrecognised`. Never blocks or fails.

### `fn fill(command: &LegacyCommand, params: &[u8]) -> [u8; 5]` [@ANCHOR: radioctl:radioctl_ylegacy_fill_command]

Builds the five bytes of a block-protocol command: starts from the command's fixed `template` and writes `params` into the slot starting at the command's `at` index (0 to 4, validated when the description is loaded). `command` and `params` are borrowed. A parameter byte that would fall past the end of the block is dropped silently, so `params` longer than the slot is truncated, not an error. Used for set-frequency (the encoded frequency bytes) and set-mode (one code byte). Pure; no I/O.

### `fn encode_block(&self, op: &Op) -> Result<(Request, Pending), RigError>` [@ANCHOR: radioctl:radioctl_ylegacy_encode_block]

Encoding for the block-protocol models (FT-747 to FT-1000MP, FT-100, some receivers), driven entirely by the description's `LegacyBlock` tables, so no model facts are in the code. `op` is borrowed. It supports frequency set on VFO A (the command's `form.encode`, then `fill`), mode set (`mode_code`), PTT and split (a fixed on and off block each), and reads of frequency, mode, PTT and split, each found by searching the description's status reads for one that carries that field.

Returns the request and the `Pending::Block(index)` to expect (or `Pending::Nothing`). The reply kind is `Ack` when the radio echoes commands, else none. Errors, all `Unsupported`: no such command in the description, a frequency the form cannot represent, an unknown mode, no status read that carries the field, any other op. Keying: PTT on is only the description's block; the gate lives above the engine. Pure, no I/O.

### `fn decode_block(&self, read: &LegacyRead, reply: &[u8]) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_ylegacy_decode_block]

Decodes one status reply of a block-protocol model, using the field offsets in the matching `LegacyRead` of the description. `read` is the table entry that was requested; `reply` is the bytes received (borrowed). It emits, when the entry lists them: `Frequency(A)` from the form at its offset, `Mode` from the byte masked and looked up in the description's `status_modes`, and `Ptt` and `Split` from a flag (offset, mask, and whether set means on).

A frequency or mode that cannot be decoded (bad BCD nibble, unknown mode code, reply too short) makes the whole reply one `Unrecognised` with the bytes, so nothing half-decoded is reported. A missing flag byte just omits that observation. Pure.

## `crates/radioctl-protocols/src/virtual_rig.rs`

### `fn legacy_model_block(&mut self, block: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_legacy_model_block]

The virtual-rig side of a block-protocol Yaesu model. `block` is one five-byte block received from the engine. Using the description's tables it: echoes the block first when the model echoes; applies PTT on/off and split on/off blocks to the virtual state; answers a status read (a block equal to a table `request`) with a reply of the table's length, zero filled, holding the VFO A frequency in the listed form and offset, the mode's code, and the PTT, split and VFO-mode flag bits; and applies set-frequency and set-mode blocks when their fixed bytes match the template. Anything unmatched changes nothing (silent, as the radio). Returns the reply bytes (possibly empty). Virtual: no RF (radio frequency), only state flags. It returns an empty vector if the description has no block table.

## `crates/radioctl-core/src/description.rs`

### `pub fn width(self) -> usize` [@ANCHOR: radioctl:radioctl_core_legacy_form_width]

Method of `LegacyFreqForm`: the number of bytes a frequency occupies in that form, 3 for the two three-byte binary forms (`Bin10HzMsb3`, `Bin10HzLsb3`) and 4 for all others. `self` is `Copy`. Used to slice a status reply at its field offset, to validate that a field lies inside its reply when a rig file is loaded, and to size the output of `encode`. Pure.

### `fn unit_millihz(self) -> u64` [@ANCHOR: radioctl:radioctl_core_legacy_form_unit]

Private method of `LegacyFreqForm`: the size of one count of the form in thousandths of a hertz, so fractional units stay integer. 1000 for the 1 Hz forms (`Bcd1HzMsb`, `Bin1HzMsb4`), 1250 for the 1.25 Hz form (`Bin125Msb4`), 625 for the 0.625 Hz form (`Bin0625Msb4`), 10000 (10 Hz) for all the rest. `encode` divides by it, `decode` multiplies by it. The FRG-8800 form does not use it. Pure.

### `pub fn encode(self, hz: Hz) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_core_legacy_form_encode]

Encodes a frequency `hz` (whole hertz, `Hz(u64)`) into the bytes of the form, `width()` long. The frequency is converted to counts of the form's unit; it must be an exact multiple (a multiple of 10 Hz for the 10 Hz forms, of 5 Hz for the 1.25 Hz and 0.625 Hz forms), or the result is `None` (never rounded). BCD forms write two decimal digits per byte; binary forms write the count big-endian; the least-significant-first forms (`Bcd10HzLsb`, `Bin10HzLsb3`) reverse the bytes. The count must fit the width (for BCD, 8 digits) or it returns `None`.

`Frg8800` is special: the frequency must be a multiple of 25 Hz and at most 999,999,975 Hz; the byte order is least significant first from the 100 Hz digit, and the low nibble of the first byte is a one-hot code of the 25 Hz step (1, 2, 4, 8 for 0, 25, 50, 75). Pure; the inverse is `decode`.

### `pub fn decode(self, bytes: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_core_legacy_form_decode]

The inverse of `encode`: the frequency in whole hertz from `bytes`, which must be exactly `width()` long. It returns `None` for a wrong length, a BCD nibble above 9, or, for `Frg8800`, a step nibble that is not one of 1, 2, 4 or 8. Reverses the byte order for the least-significant-first forms, accumulates the count, and multiplies by the unit; the 1.25 Hz and 0.625 Hz forms round down to a whole hertz, so a decode followed by an encode is exact only on values `encode` can produce. Pure; no side effects.

## `crates/radioctl-core/src/rigfile.rs`

### `fn legacy_form(name: &str) -> Result<LegacyFreqForm, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_legacy_form]

Maps the frequency-form name written in a rig description file to a `LegacyFreqForm`. The names are `bcd-10hz-msb`, `bcd-10hz-lsb`, `bcd-1hz-msb`, `bin-10hz-msb3`, `bin-10hz-lsb3`, `bin-10hz-msb4`, `bin-1hz-msb4`, `bin-1.25hz-msb4`, `bin-0.625hz-msb4` and `frg8800`, matched exactly (case sensitive). Anything else returns `RigError` of kind `Malformed` with a message naming the unknown form, which fails the rig file load. Pure; called by `build_legacy_block` for command forms and read frequency fields.

### `fn build_legacy_block(y: &YaesuLegacySection) -> Result<Option<LegacyBlock>, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_legacy_block]

Builds the block-protocol tables (`LegacyBlock`) of a yaesu-legacy description from its parsed rig-file section `y` (borrowed), and checks them. It converts the set-frequency and set-mode command entries, the PTT and split on/off pairs, every status read (request, length, frequency field, mode field, PTT, split and VFO-mode flags), the status mode codes and the echo flag.

Errors, all `Malformed`: a command slot starting past byte 4; an on block without its off block (PTT or split); a set-frequency command with no frequency form; a status field (frequency, mode, PTT, split) that lies outside its reply length; and anything `legacy_form` or the mode-code builder rejects. Returns `Ok(None)` when the section defines no block commands at all (the classic FT-817 family), `Ok(Some(block))` otherwise. Pure; run once at description load, so a bad file never reaches an engine.

## `crates/radioctl-core/src/description.rs` (level and CI-V parameters)

### `pub fn max_attenuation_db(&self) -> Option<u8>` [@ANCHOR: radioctl:radioctl_core_levelparams_max_attenuation]

Method of `LevelParams`: the largest receive attenuation, in decibels, that the model has. If the description gives a continuous range (`attenuator_any_up_to_db` above 0) that value is returned; otherwise the last entry of the ascending list `attenuator_db`. Returns `None` when neither is present, meaning the model has no attenuator. Used to decide whether the attenuator level exists at all (`step_of` refuses without it) and for range display. Pure.

### `pub fn attenuator_step(&self, db: u16) -> Option<u8>` [@ANCHOR: radioctl:radioctl_core_levelparams_attenuator_step]

Method of `LevelParams`: converts an attenuation in decibels, `db`, to the model's 1-based step number. `0` dB is step 0 (off) on any model. With a continuous range (`attenuator_any_up_to_db`), the step equals the decibels, up to the range limit. Otherwise the position of `db` in the list `attenuator_db` plus one. Returns `None` for a value with no step (7 dB on a 6/12/18 dB model, or beyond the range, or a step number that does not fit `u8`). The inverse is `attenuator_db_of`. Pure.

### `pub fn attenuator_db_of(&self, step: u8) -> Option<u16>` [@ANCHOR: radioctl:radioctl_core_levelparams_attenuator_db]

Method of `LevelParams`: the attenuation in decibels of the 1-based `step`. Step 0 is off and always 0 dB. With a continuous range the step is the decibels (up to the limit). Otherwise the `step`th entry of `attenuator_db` (index `step - 1`). Returns `None` for a step the model does not have. Used when decoding a rig reply (`attenuation::value_of`) so an unknown step is reported unrecognised instead of guessed. Pure.

### `pub fn lacks(&self, level: Level) -> bool` [@ANCHOR: radioctl:radioctl_core_levelparams_lacks]

Method of `LevelParams`: true when the description's `levels.absent` list names `level`, meaning the model's command table has no such level (of the plain `14` family: AF, RF, microphone, monitor, keyer, power). The CI-V engine and virtual rig refuse a lacking level. An empty list (the default) means the model has them all, so this returns false for everything. Pure; no side effects.

### `pub fn field_len(&self, hz: Hz) -> usize` [@ANCHOR: radioctl:radioctl_core_civparams_field_len]

Method of `CivParams`: the number of BCD bytes of the CI-V frequency field for `hz` on this model. The description's `frequency_bytes` is returned as is (4 for the eight-digit models such as IC-735, 5 for ten digits), except that the value 6 means the IC-905 rule: 5 bytes up to 5,850,000,000 Hz, 6 bytes above it. Used by `freq_bytes` and the virtual rig to size frequency fields. Pure.

## `crates/radioctl-protocols/src/civ.rs`

### `pub fn encode_bcd_freq_len(hz: Hz, len: usize) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_civ_encode_bcd_freq_len]

Encodes `hz` as `len` BCD bytes, least significant pair first, as CI-V does (4 for eight-digit models, 5 for ten, 6 for twelve). Each byte is two decimal digits. Returns `None` when the frequency has more digits than `2 * len`, never truncating. `len` 0 gives `Some(empty)` only for 0 Hz. Pure; allocates one vector. The decoder is `decode_bcd_freq`; the fixed five-byte form is `encode_bcd_freq`.

### `fn freq_bytes(&self, hz: Hz) -> Result<Vec<u8>, RigError>` [@ANCHOR: radioctl:radioctl_civ_engine_freq_bytes]

Method of the CI-V engine: the frequency as the BCD field of this model. Takes the field length from the description (`CivParams::field_len(hz)`, default 5 without CI-V parameters) and calls `encode_bcd_freq_len`. Returns `Unsupported` ("frequency has more digits than the model's field") when it does not fit, which reaches the caller as a refused set. Used by the frequency set encoders, including the IC-F8101 form. Pure.

## `crates/radioctl-protocols/src/virtual_rig.rs` (CI-V)

### `fn civ_freq_field(&mut self, params: &CivParams, other: bool) -> Option<Vec<u8>>` [@ANCHOR: radioctl:radioctl_virtual_civ_freq_field]

Virtual CI-V rig: the frequency of the selected VFO as the model's BCD field. `other` false picks the selected VFO, true the other one (`civ_slot`). The length comes from `params.field_len`, then `encode_bcd_freq_len`. Returns `None` if the stored frequency does not fit (cannot happen for values the virtual rig accepted). It needs `&mut self` only because `civ_slot` does. No I/O; state is read, not changed.

## `crates/radioctl-protocols/src/attenuation.rs`

### `pub fn is_front_end_level(level: Level) -> bool` [@ANCHOR: radioctl:radioctl_att_is_level]

True for the two receive front-end levels this module handles, `Level::Attenuator` (decibels of attenuation) and `Level::Preamp` (a step number), false for every other level. The daemon's `set_level` uses it to decide whether a value needs a step check against the model before it is sent. Pure; no description needed.

### `pub fn step_of(desc: &RigDescription, level: Level, value: u16) -> Result<u8, RigError>` [@ANCHOR: radioctl:radioctl_att_step]

Converts a level value to the wire step. For the attenuator `value` is decibels (0 is off) and the step is the 1-based step from `attenuator_step`; for the preamp `value` is the step itself, which must not exceed `levels.preamp_steps`. `desc` is borrowed.

Errors, all `RigError` kind `Unsupported`: the level is not a front-end level; the description lists no attenuator steps (no `max_attenuation_db`) or no preamp steps; the value has no step (e.g. 7 dB on 6/12/18 dB) or the preamp step is too high. Callers use it both to encode a set and, with value 0, to ask whether the model has the control at all. Pure.

### `pub fn value_of(desc: &RigDescription, level: Level, step: u8) -> Option<u16>` [@ANCHOR: radioctl:radioctl_att_value]

The inverse of `step_of`: the level value (dB for the attenuator, the step number for the preamp) of a wire `step` that the rig reported. Attenuator steps map through `attenuator_db_of` (0 is 0 dB); a preamp step is valid when it is within `preamp_steps` and the model has any. Returns `None` for a step the description does not have, or for a level that is not front-end, and the engine then reports the reply as unrecognised instead of guessing. Pure.

### `pub fn virtual_get(desc: &RigDescription, levels: &BTreeMap<Level, u16>, level: Level) -> Option<u16>` [@ANCHOR: radioctl:radioctl_att_virtual_get]

What a virtual rig answers to a read of a front-end level: the value stored in `levels` for `level`, 0 if never set. Returns `None` when the model has no such control (checked by asking `step_of` for value 0), which the virtual interpreters turn into a refusal. `levels` is the virtual rig's state map, borrowed read-only. Pure.

### `pub fn virtual_set(desc: &RigDescription, levels: &mut BTreeMap<Level, u16>, level: Level, value: u16) -> bool` [@ANCHOR: radioctl:radioctl_att_virtual_set]

Stores a front-end level into the virtual rig's state map. It first checks `step_of(desc, level, value)`; on any error (no control, no such step) nothing is stored and it returns `false`. Otherwise it inserts `value` (dB for the attenuator, step for the preamp) and returns `true`. `levels` is mutated; `desc` is borrowed. No I/O.

### `fn virtual_ascii(desc, levels, name: &str, arg: &str, second_field: bool) -> Option<String>` [@ANCHOR: radioctl:radioctl_att_virtual_ascii]

Shared body of the Kenwood-style and Elecraft virtual `RA` and `PA` handlers. `name` is `RA` (attenuator, two-digit step) or `PA` (preamp, one-digit step); `arg` is the text after the name (empty for a read); `second_field` true appends the always-zero second parameter Kenwood answers carry (`00` after the attenuator step, `0` after the preamp step). `levels` is mutated on a set.

A read returns `NAME` + zero padded step + tail + `;`. A set must be exactly the field width of ASCII digits and map to a step the model has (`value_of`); it stores the level and returns `None` (no answer). A model without the control, or a bad argument, returns `Some("?;")`. State only; no RF.

### `pub fn virtual_kenwood(desc, levels, name: &str, arg: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_att_virtual_kenwood]

The `RA` and `PA` commands of a virtual Kenwood-style rig: `RA;` answers `RAnn00;` (two-digit step, then the always-zero second field), `RAnn;` sets; `PA;` answers `PAn0;`, `PAn;` sets. A refusal, including a model without the control, is `Some("?;")`; a successful set answers `None`. It calls `virtual_ascii` with the second field on. `levels` is mutated; `desc` is borrowed.

### `pub fn virtual_elecraft(desc, levels, name: &str, arg: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_att_virtual_elecraft]

The `RA` and `PA` commands of a virtual Elecraft rig: `RA;` answers `RAnn;` and `PA;` answers `PAn;`, and the same forms set, per the Elecraft Programmer's Reference Rev. G5, page 22. A refusal is `Some("?;")`, a successful set `None`. It is `virtual_ascii` without the second field. `levels` is mutated on a set; `desc` is borrowed.

### `pub fn virtual_yaesu(desc, levels, name: &str, arg: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_att_virtual_yaesu]

The `RA` and `PA` commands of a virtual Yaesu newcat rig. `RA0;` answers `RA0n;`, the leading 0 being the main receiver, and `RA0n;` sets; `PA` is the same for the preamp. `arg` is the text after the two-letter name: `0` is a read, `0` plus one digit a set that must map to a step the model has. A refusal (model without the control, bad argument) is `Some("?;")`; a set answers `None`. `levels` is mutated on a set. State only.

## `crates/radioctl-core/src/description.rs` (dialects)

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_kenwood_dialect_name]

Method of `KenwoodDialect`: the name used in rig description files: `classic`, `ts890`, `ts440`, `ts450`, `ts990`, `r5000`, `trc80`. Returns a borrowed static string, so there is no allocation and no error case. The exact inverse is `from_name`: `from_name(d.name()) == Some(d)` for every variant. The names are part of the rig description file format, so changing one breaks every shipped description that uses it. Called when a description is written back or shown to a person; the loader uses `from_name`. Pure, no side effects.

### `pub fn from_name(name: &str) -> Option<KenwoodDialect>` [@ANCHOR: radioctl:radioctl_core_kenwood_dialect_from_name]

Method of `KenwoodDialect`: the dialect for a rig file name (`classic`, `ts890`, `ts440`, `ts450`, `ts990`, `r5000`, `trc80`), matched exactly and case sensitively. Returns `None` for anything else, including an empty string or different case; the rig file loader turns that into a load error that names the bad value, so a typo is caught when the description loads. The default dialect for a description that names none is `Classic`, chosen by the caller, not here. Pure, no allocation, no side effects.

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_hh_dialect_name]

Method of `HhDialect` (Kenwood handheld and mobile command sets): the name used in rig files: `th`, `tm710`, `th72`, `th74`. Returns a borrowed static string and is the inverse of `from_name`, so every variant round trips. The names belong to the rig description file format and must not change without migrating the shipped descriptions. `th` is the older FQ-command handhelds, `tm710` the TM-D710 and TM-V71 mobiles, `th72` the TH-D72 and `th74` the TH-D74. Pure, no error case, no side effects.

### `pub fn from_name(name: &str) -> Option<HhDialect>` [@ANCHOR: radioctl:radioctl_core_hh_dialect_from_name]

Method of `HhDialect`: the handheld dialect for a rig file name (`th`, `tm710`, `th72`, `th74`), exact and case sensitive; `None` otherwise (including an empty string or different case), which the rig file loader reports as an unknown dialect so the description fails to load instead of running with a wrong command set. It is the inverse of `name`. Pure, no allocation, no side effects.

## `crates/radioctl-protocols/src/kenwood_hh.rs`

### `pub fn fo_layout(dialect: HhDialect) -> Option<(usize, usize)>` [@ANCHOR: radioctl:radioctl_hh_fo_layout]

Gives the shape of the `FO` line (the frequency and mode report) of a handheld dialect as `(line length, mode column)`: TM-D710 and TM-V71 `(48, 47)`, TH-D72 `(52, 51)`, TH-D74 `(72, 31)`; columns are zero-based byte positions of the mode digit. Returns `None` for the `th` dialect, which uses the `FQ` command instead. The engine uses it to parse `FO` lines and to decide which command family to speak. Pure.

### `pub fn th_step_and_frequency(hz: u64) -> (u64, u8)` [@ANCHOR: radioctl:radioctl_hh_th_step_and_frequency]

For the `FQ` dialect, chooses the frequency actually sent and its step code for a requested `hz` (hertz). It rounds to the 5 kHz grid and to the 6.25 kHz grid and takes the nearer (a tie takes 6.25 kHz, step code 1; the 5 kHz grid is step code 0). From 470 MHz upward the frequency is rounded to 10 kHz with step code 4. Returns `(frequency in Hz, step code)`. Never fails; the caller sends the rounded frequency, so the radio may end up off the requested value by up to half a step. Pure.
