# radioctl function anchors, batch 36

Function anchors for the `radioctld` service and the Yaesu protocol code in `radioctl-protocols` (newcat engine, legacy five-byte engine and the two virtual-rig interpreters), written to the ADR 0113 standard. Each anchor names a function and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Anything that can key a transmitter says how the gate applies: the daemon checks its transmit gate and the one-owner rule (`key`, `send_morse`) before it asks the rig, and the rig layer refuses a key-on when the band or transmit frequency check fails; the protocol engines only encode and never decide. The virtual rigs have no hardware, so a key-on there only flips a state flag.

## `crates/radioctld/src/service.rs`

### `fn served_level(&self, name: &str) -> Result<LevelInfo, i32>` [@ANCHOR: radioctl:radioctld_service_served_level]

Resolves a Hamlib level name (`AF`, `RFPOWER`, `SWR`, ...) to the entry of the served-level table that tells `get_level` and `set_level` where the value comes from and whether the client sees a 0 to 1 fraction or the rig's own integer. `name` is borrowed, case as the client sent it. The table is built from the rig description by `served_levels`, so a rig only serves what its description can read.

Returns the `LevelInfo` by value. Errors are Hamlib codes: `EINVAL` (-1) when the name is not a level Hamlib knows, `ENAVAIL` (-11) when Hamlib knows it but this daemon never serves it or this rig's description does not. Pure lookup: no I/O, no locks, no side effects, safe to repeat.

### `async fn get_level(&self, name: &str) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_get_level]

Answers the Hamlib `l` command. It calls `served_level`, then reads from the source the entry names: a radio level (through the cache-or-ask `read`), a transmit power or SWR meter, a raw or calibrated S-meter, or a derived value. A radio level that is a fraction is returned as `(raw - min) / (max - min)` using `level_range`; a non-fraction level (attenuator dB, preamp step) is the raw integer. Meters go through `meter_cal` with the calibration for this rig id, and power in watts needs a power-meter calibration in the description.

Returns `Body::Fields` with one unlabeled value, floats printed without trailing zeros (six decimals at most). Errors: the `served_level` codes; `EIO` when the rig answers with the wrong kind of observation or a strength cannot be computed; `ENAVAIL` when a range or calibration is missing; and whatever `read` maps from a rig error (timeout, link down, queue full). Async; takes no lock of its own. Read only: it never keys the rig.

### `async fn meter(&self, meter: Meter) -> Result<u16, i32>` [@ANCHOR: radioctl:radioctld_service_meter]

Reads one transmitter meter (`Meter` names SWR, ALC, compression, supply voltage, drain current and the like) as the raw unscaled number the engine reports, in the family's own units; callers apply `meter_cal` to turn it into a Hamlib value. It goes through `read`, so a cached value younger than the configured maximum age is returned without touching the radio.

Returns `Ok(raw)`; `EIO` if the answer is not a `Meter` observation for the same meter; other codes come from `read`. Async. Reading a meter never keys the transmitter: on an unkeyed rig the transmit meters simply read near zero. No side effects beyond a cache refresh.

### `async fn set_level(&self, name: &str, value: f64) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_set_level]

Answers Hamlib `L`: sets a numeric level on the rig. `name` is a level name; `value` is a 0 to 1 fraction when the served entry is a fraction (it must lie in 0..=1) or a whole number (rounded to nearest) otherwise. The fraction is scaled onto the model's `(min, max)` from `level_range` and rounded to the nearest step, except transmit power (`RFPOWER`), which is rounded down, with a tolerance of 0.0001 so the six-decimal text a client prints (5/15 as 0.333333) keeps its step; a request is never exceeded.

Only levels read from the radio can be set: meters and derived values answer `ENAVAIL`. Errors: `EINVAL` for an unknown name, a fraction outside 0..=1, a result outside the model range, or a front-end value (attenuator dB, preamp step) the model has no step for; `ENAVAIL` when not served or no range; otherwise the mapped rig error. On success `Body::Done`. It sends `Op::SetLevel` through `set`; changing RF power does not key the transmitter, and the rig layer still gates any key-on.

### `async fn get_func(&self, name: &str) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_get_func]

Answers Hamlib `u`: reads a function switch. Only RIT and XIT are served; `func_lookup` classifies the name. The state comes from the cache or the rig (`Field::Rit` or `Field::Xit`) through `read`.

Returns `Body::Fields` with one value, `1` when the observation says on, `0` otherwise (including an observation of another kind, which is not an error here). Errors: `EINVAL` for a name Hamlib does not know, `ENAVAIL` for a known function this daemon does not serve or a rig without the switch, plus mapped rig errors. Read only; no side effects.

### `async fn set_func(&self, name: &str, on: bool) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_set_func]

Answers Hamlib `U`: switches RIT or XIT on or off. `name` picks the function, `on` is the new state, both borrowed or copied. It sends `Op::SetRit(on)` or `Op::SetXit(on)` through `set`, so the engine decides the wire form (for example an FT-891 or FT-450 has no transmit clarifier and the engine answers unsupported, which arrives as `ENAVAIL`).

Returns `Body::Done` on success. Errors: `EINVAL` for an unknown name, `ENAVAIL` for an unserved function, plus mapped rig errors. Idempotent: setting the same state twice is harmless. It does not change the clarifier offset (see `set_rit_offset`) and cannot key the transmitter.

### `async fn vfo_info(&self, vfo: Vfo) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_vfo_info]

Answers Hamlib `\get_vfo_info` for the VFO the command selected (`vfo`, `Vfo::A` or `Vfo::B`). It collects the frequency of that VFO in hertz, the mode and passband width, and the split flag, by calling `frequency`, `mode_and_width` and `split_vfo`.

Returns `Body::Fields` labeled `Freq`, `Mode` (the Hamlib mode name), `Width` (hertz, as `mode_and_width` reports), `Split` (`0` or `1`, defaulting to `0` when the split answer has no value) and `SatMode`, which is always `0` because satellite mode is not modeled. Errors: any code from the three reads, and `EIO` if `split_vfo` does not answer with fields. Read only; several reads may hit the rig if the cache is stale.

### `pub async fn execute(&self, client: ClientId, command: &Command, vfo: Option<VfoSel>) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_service_execute]

The one entry point from the protocol server: runs a parsed rigctld `Command` for the connection `client` and returns the reply body, or the negative Hamlib error code (`RPRT n`) the server will print. `vfo` is the VFO the client named, or `None` for the current one; `resolve_vfo` turns it into `Vfo::A` or `Vfo::B` (`ENAVAIL` for an absent selection). The method counts every command in `counters.commands`, then dispatches each `Command` variant to a helper.

Gating: `SetPtt(0)` always unkeys (`unkey`), open to every client; any other `SetPtt` goes to `key(client)` and `SendMorse` to `send_morse`, both of which check the transmit gate, take ownership of the transmitter for `client`, and are then subject to the rig layer band and transmit-frequency checks. Unsupported commands (`SetPowerStat`, `SendCmd`, locking the rig) answer `ENAVAIL`; `Quit` answers `Body::Close`. Async, takes the short `current` and owner locks only briefly, never across an await.

## `crates/radioctl-protocols/src/virtual_rig.rs`

### `fn yaesu_command(&mut self, text: &str) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_yaesu_command]

The newcat command interpreter of a virtual Yaesu rig. `text` is one tokenized command without its `;` (a two-letter name plus arguments); the virtual rig's feed loop calls it for each token and answers `?;` itself for junk. It matches `FA`, `FB`, `MD0`, `TX`, `FT`, `ST`, `CF` (FT-891 and FT-710 forms), `RT`, `XT`, `RC`, `RU`/`RD`, `SM0`, `RM5`, `IF`, `ID`, `AI`, `RA`/`PA` and the level commands, plus fixed answers Hamlib needs (`PS`, `SH`, `NA`, `VS`, `RF`). A read returns the reply bytes; a set updates the shared `RigState` and returns nothing, as the real radio is silent; anything unrecognised or out of range returns `?;`.

The model id for `ID` comes from the wire parameters; model id 670 (the FT-991 family) uses shorter `SH` and `RM5` tails and does not answer `VS`, `ST` or `RF`, which the FTDX10 and FTDX101 do. `TX1` only sets the virtual PTT flag: no RF, no gate. Not modeled: VFO selection and roofing filter settings are accepted and ignored. Synchronous, no I/O, mutates only the virtual state.

### `fn yaesu_cf(&self) -> bool` [@ANCHOR: radioctl:radioctl_virtual_yaesu_cf]

Tells the virtual Yaesu interpreter whether this model switches its receive clarifier with the `CF` command (description value `yaesu.clarifier = "cf-receive"`, the FT-891) instead of `RT`/`XT`. It inspects the wire parameters only and returns false for any non-Yaesu wire. `yaesu_command` uses it to guard the `CF0;` read and the `CF010;`/`CF000;` sets, so a model without the form still answers `?;`. The two-switch form of the FT-710 has its own sibling test (`yaesu_cf_rx_tx`, `"cf-rx-tx"`) with the `CF000` read and `CF001` offset. Pure, no side effects.

### `fn yaesu_digits(&self) -> usize` [@ANCHOR: radioctl:radioctl_virtual_yaesu_digits]

Returns how many digits the virtual rig writes and accepts in the frequency fields of `FA`, `FB` and the frequency part of `IF`: the description's `yaesu.frequency_digits`, which is 9 for most models and 8 for the older ones (FT-450, FT-950, FT-2000, FTDX1200, FTDX3000, FTDX5000, FTDX9000). A non-Yaesu wire falls back to 9. The count is used for zero padding on reads and for the exact length check on sets. Pure, no side effects.

### `fn yaesu_set_frequency(&mut self, vfo_a: bool, digits: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_yaesu_set_frequency]

Handles `FA`/`FB` followed by digits. `vfo_a` selects VFO A (true) or B (false); `digits` is the borrowed argument text and must be exactly `yaesu_digits()` decimal digits. On success it stores the value in hertz in `freq_a` or `freq_b` and returns `None` (the radio sends no answer to a set). A wrong length or a non-digit returns `Some("?;")`. No range check beyond the field width, no band or gate concern, since a virtual rig only holds state.

### `fn yaesu_if(&self) -> String` [@ANCHOR: radioctl:radioctl_virtual_yaesu_if]

Builds the answer to a read of `IF;`: `IF001`, the VFO A frequency zero padded to `yaesu_digits()` digits, a sign and four digits of clarifier offset in hertz, the RIT and XIT flags as `0` or `1`, the mode code character from the description (`0` when the mode has no code), and the fixed tail `00000;`. With 9 digits the string is 28 characters; with 8 digits it is 27. Read only, no side effects. The real engine side of the same format is `decode_if`.

### `fn yaesu_move_offset(&mut self, up: bool, digits: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_yaesu_move_offset]

Handles `RU` (up, `up` true) and `RD`: moves the stored clarifier offset by `digits`, which must be exactly four decimal digits of hertz. The result is stored in `rit_offset` and returns `None`. If the digits are malformed, or the new offset would pass 9999 Hz in either direction, nothing changes and it returns `Some("?;")`. The step is added to the existing offset (it is not absolute); `RC` resets the offset to zero. State only, no I/O.

### `fn yaesu_level(&mut self, name: &str, arg: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_yaesu_level]

Reads or sets a newcat level on the virtual rig: `AG` (AF gain) and `RG` (RF gain) carry a leading `0` before three digits, `MG` (mic gain), `KS` (keyer speed) and `PC` (RF power) are three digits alone. `name` is the two letter command, `arg` the rest. An empty `arg` (or `0` for AG and RG) reads: the stored value is returned as `NAME` + prefix + three digits + `;`, `0` when never set. A set must be three digits, inside the `(min, max)` that `yaesu::level_range` gives for this model; it is stored and `None` returned. Anything else gives `Some("?;")`. State only; setting `PC` does not key anything.

## `crates/radioctl-protocols/src/yaesu.rs`

### `fn level_wire(level: Level) -> Option<LevelWire>` [@ANCHOR: radioctl:radioctl_yaesu_level_wire]

Maps a `Level` to the newcat command that carries it, with its default valid range, as a `LevelWire` (fields: `name`, the two letter command; `leading_zero`, true when a `0` main receiver digit precedes the number; `min` and `max`, the default limits). Table: AF gain `AG` 0..255 with leading zero, RF gain `RG` 0..255 with leading zero, mic gain `MG` 0..100, keyer speed `KS` 4..60, RF power `PC` 5..100. Returns `None` for levels the newcat engine handles elsewhere or not at all (monitor, attenuator, preamp, dither, preselector, antenna). Per-model ranges override the defaults through `level_range`. Pure.

### `pub fn level_range(desc: &RigDescription, level: Level) -> Option<(u16, u16)>` [@ANCHOR: radioctl:radioctl_yaesu_level_range]

Returns the inclusive `(min, max)` raw values of a level for the described model, or `None` when newcat has no command for that level. `desc` is borrowed. If the description's `[yaesu]` table lists a range for the level it wins; otherwise the default from `level_wire` applies. The shipped descriptions use it for, among others, the FT-891 `RG` (0 to 30) and the 0 to 255 `MG`/`PC` models; these numbers come from the CAT books and are unverified against hardware. Used by the engine (`SetLevel` range check), the virtual rig and the daemon (`get_level`/`set_level` scaling). Pure.

### `fn decode_level(name: &str, digits: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_yaesu_decode_level]

Decodes one level answer into `Observation::Level(level, value)`. `name` is the two letter command (`AG`, `RG`, `MG`, `KS`, `PC`) and `digits` the argument text after it, for example `0128` for an AF gain of 128. A command with the leading zero must start with `0`, which is stripped; the remaining number must be exactly three digits and fit a `u16`. Returns `None` (the caller reports the reply as unrecognised) for an unknown name, a missing leading zero, the wrong digit count or a non-digit. Does not range check against the model. Pure.

### `pub fn parse_clarifier(text: &str) -> Option<i16>` [@ANCHOR: radioctl:radioctl_yaesu_parse_clarifier]

Parses the five characters of `IF` P3 (and of `CF001`): a sign, `+` or `-`, then exactly four decimal digits, giving the clarifier offset in hertz as a signed `i16`. `text` is borrowed. Returns `None` if the sign is missing or wrong, the digit count is not four, or a digit is not decimal. The value is not limited to 9999 by this function beyond the four digits. Pure; used by `decode_if`, the `CF` decoder and the virtual rig's tests.

### `pub fn new(desc: RigDescription) -> Result<YaesuEngine, RigError>` [@ANCHOR: radioctl:radioctl_yaesu_engine_new]

Builds a newcat engine; takes ownership of the description. It requires `desc.family == Family::YaesuNewcat` and a `[yaesu]` parameter block; otherwise it returns `RigError` of kind `Unsupported` ("not a Yaesu newcat description"). The new engine has a fresh tokenizer and the clarifier switches assumed off (`clar` is `(false, false)`, later learned from replies). No I/O. Call once per rig; the engine is single-owner state, driven by the rig actor, never shared across threads.

### `fn decode_front_end(&self, name: &str, digits: &str) -> Option<Observation>` [@ANCHOR: radioctl:radioctl_yaesu_engine_decode_front_end]

Decodes an `RA` (attenuator) or `PA` (preamp) answer. `digits` must be a single `0` (main receiver; fixed zero on the FT-991A) followed by exactly one digit, the wire step. The step is mapped to the level value (dB for the attenuator, step number for the preamp) with `attenuation::value_of` and the description. Returns `Observation::Level(Attenuator or Preamp, value)`, or `None` when the format is wrong or the description has no such step, so the answer is reported as unrecognised, never guessed. Pure.

### `fn decode(&self, text: &str) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_yaesu_engine_decode]

Turns one complete newcat reply (`text`, without the `;`) into `Inbound` items. `?` becomes `Inbound::Nak`. `IF...` goes to `decode_if`. On a `cf-rx-tx` model, `CF` with an eight character body decodes the two clarifier switches or the offset. Otherwise it matches `FA`, `FB` (digit count must equal `digits()`), `MD0`, `TX`, `FT`, `ST` (only on models whose description has `split_st`), `RT`, `XT`, `CF` (receive form), `SM0`, `RM5`, `ID`, `RA`/`PA` and the levels.

A split reading yields two observations: `Split` and the derived `TxVfo` (B when split, else A). A reply that cannot be decoded becomes `Inbound::Unrecognised` with the text and `;` restored, never an error. Pure; no I/O.

### `fn digits(&self) -> usize` [@ANCHOR: radioctl:radioctl_yaesu_engine_digits]

Number of digits in the frequency fields of `FA`, `FB` and P2 of `IF` for the described model: the description's `yaesu.frequency_digits`, 9 by default and 8 for the FT-450, FT-950, FT-2000, FTDX1200, FTDX3000, FTDX5000 and FTDX9000. It sets the width of `SetFrequency` commands, the length checks of the decoder, and the IF length (`digits + 16` without `IF` and `;`). A frequency that does not fit this many digits is refused in `encode`. Pure.

### `fn cf_receive(&self) -> bool` [@ANCHOR: radioctl:radioctl_yaesu_engine_cf_receive]

True when the model's clarifier is switched with the `CF` command alone (`yaesu.clarifier = "cf-receive"`, the FT-891): `CF010;` on, `CF000;` off, `CF0;` reads, and only the receive clarifier exists. The other styles are separate private checks: `rt_receive` (`"rt-receive"`, FT-450 and FT-450D) keeps `RT` with no `XT`, and `cf_rx_tx` (`"cf-rx-tx"`, FT-710) uses one `CF` for both switches and the offset. When this is true, `encode` refuses XIT with `Unsupported`. Reads the description only; pure.

### `fn split_st(&self) -> bool` [@ANCHOR: radioctl:radioctl_yaesu_engine_split_st]

True when the model's split is the `ST` command (`yaesu.split = "st"`, the `split_st` flag: FT-891, FT-710, FTDX10, FTDX101D and MP): `ST1;` on, `ST0;` off, answers 0, 1 or 2 (2 meaning on with a 5 kHz shift). False means the older `FT2;`/`FT3;` form, read with `FT;`. It chooses the split read and set commands in `encode` (`split_read`) and whether `decode` accepts an `ST` reply. Reads the description only; pure.

### `fn decode_if(&self, body: &str) -> Option<Vec<Inbound>>` [@ANCHOR: radioctl:radioctl_yaesu_engine_decode_if]

Decodes the `IF` answer, passed without `IF` and `;`. `body` must be ASCII and exactly `digits() + 16` characters (25 with a 9-digit frequency, 24 with 8). The fields: characters 0 to 2 channel (ignored), the VFO A frequency, five characters of clarifier offset (via `parse_clarifier`), the RIT flag, the XIT flag, then the mode character mapped by `code_mode`. Returns five observations in order: `Frequency(A)`, `Mode`, `Rit`, `Xit`, `RitOffset`. Returns `None` on a wrong length, non-ASCII text, a bad number, a flag other than 0 or 1, or an unknown mode code, and the caller then reports the reply as unrecognised. Pure.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_yaesu_engine_encode]

Part of `ProtocolEngine`: turns one `Op` into the bytes to send (`Request`: `bytes` and the `ReplyKind` to wait for). It first checks that the op's required capability is in the description, else `Unsupported`. Reads map to `FA;`, `FB;`, `MD0;`, `TX;`, `SM0;`, `RM5;`, `ID;`, `IF;` and the level and clarifier reads; sets map to `FA`/`FB` + zero padded digits, `MD0` + code, `TX0;`/`TX1;`, split, RIT, XIT and level commands. The clarifier varies by model (`CF` forms, `RT`/`XT`), the offset is sent as `RC;` plus `RU`/`RD`, and a changed switch updates the remembered `clar` pair.

Errors, all `Unsupported`: frequency too wide for the field, unknown mode, level out of the guides' range, offset beyond 9999 Hz, XIT on a receive-only clarifier model, level or op not implemented, and Morse (`SendCw`/`StopCw`), since the guides document no text-to-Morse command or stop. Keying: `SetPtt(true)` encodes `TX1;`. The engine does not decide whether transmitting is allowed; the gate, band and frequency checks happen in the rig layer before `encode` is called. Not a stop path: unkeying (`TX0;`) is.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_yaesu_engine_feed]

Part of `ProtocolEngine`: takes raw bytes read from the serial link (`bytes`, borrowed) and the monotonic time of arrival (`now`), appends them to the tokenizer, and decodes each complete command with `decode`. Partial commands wait in the tokenizer for the next call; text it cannot frame comes out as `Inbound::Unrecognised`. Afterwards it records every `Rit` and `Xit` observation in `clar`, so a later set of the other switch on a `cf-rx-tx` model resends the right state. Never fails and never blocks: returns an empty vector when no complete command has arrived. Called from the rig actor only.

## `crates/radioctl-protocols/src/virtual_rig.rs` (legacy five-byte)

### `fn legacy_block(&mut self, block: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_virtual_legacy_block]

Interprets one complete five-byte block (four parameter bytes and an opcode) sent to a virtual FT-817, FT-857 or FT-897. The feed loop cuts the stream into blocks of five and drops a partial block after the frame timeout. If the description has a block-protocol table, it delegates to `legacy_model_block`. Otherwise the opcode decides: `03` returns the frequency (BCD) and mode byte, `E7` one S-meter byte, `F7` one byte of power nibble, split bit (0 is on) and PTT bit (clear transmits); `01`, `07`, PTT on and off, split on and off, and clarifier on, off and frequency change state and return nothing; an unknown opcode is silent, as the radio is. A clarifier frequency with a non-BCD nibble or over 9990 Hz is ignored. PTT here only sets a state flag; no RF.

## `crates/radioctl-protocols/src/yaesu_legacy.rs`

### `pub fn encode_bcd_frequency(hz: Hz) -> Option<[u8; 4]>` [@ANCHOR: radioctl:radioctl_ylegacy_encode_bcd_frequency]

Encodes a frequency as four packed BCD bytes, two decimal digits per byte, most significant first, in units of 10 Hz (so 439.700 MHz is `43 97 00 00`). `hz` is a `Hz(u64)` value. Returns `None` when the frequency is not an exact multiple of 10 Hz or is above 999,999,990 Hz (the largest eight digits hold), so it never rounds silently. Pure; the inverse is `decode_bcd_frequency`.

### `pub fn encode_frequency(hz: Hz, high_band_flag: bool) -> Option<[u8; 4]>` [@ANCHOR: radioctl:radioctl_ylegacy_encode_frequency]

Encodes the four frequency bytes of a command. Without `high_band_flag` it is `encode_bcd_frequency`. With `high_band_flag` (the FT-736R description) a frequency from 1.2 GHz up to but excluding 1.3 GHz, and a multiple of 10 Hz, is sent as the 10 Hz count of the part above 1.2 GHz with the first nibble forced to `C`, standing for the digits 12. Everything else uses the plain form. Returns `None` for a frequency that does not fit. The `C` form is from Hamlib's source and unverified against a manual (none was read). Pure.

### `pub fn decode_frequency(bytes: &[u8], high_band_flag: bool) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_ylegacy_decode_frequency]

The inverse of `encode_frequency`. `bytes` is a borrowed slice that must be four bytes. With `high_band_flag` and a first nibble of `C`, the nibble is cleared, the rest decoded as BCD, and 1.2 GHz added. Otherwise it is the plain BCD decode. Returns `None` when the length is not four or any nibble is above 9 (apart from the `C` case). Pure; used by the virtual rig and the engine's frequency read.

### `pub fn decode_bcd_frequency(bytes: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_ylegacy_decode_bcd_frequency]

Decodes four packed BCD bytes of 10 Hz units into hertz (`43 97 00 00` gives 439,700,000 Hz). `bytes` must have exactly four elements, each nibble 0 to 9; otherwise `None` (a wrong length or a non-digit nibble, for example noise on the line). The largest result is 999,999,990 Hz. Pure, no side effects; inverse of `encode_bcd_frequency`.

### `pub fn new(desc: RigDescription) -> Result<YaesuLegacyEngine, RigError>` [@ANCHOR: radioctl:radioctl_ylegacy_engine_new]

Builds a legacy five-byte engine and takes ownership of the description. It requires `Family::YaesuLegacy` and a `yaesu_legacy` parameter block, else `RigError` of kind `Unsupported` ("not a Yaesu legacy description"). The engine starts with nothing pending, an empty receive buffer and no echo expectation. These radios send nothing for a setting and answer a read with a fixed number of unframed bytes, so the engine remembers which answer it waits for. No I/O; single owner, driven by the rig actor.

### `fn decode_frequency_mode(&self, bytes: &[u8]) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_ylegacy_decode_frequency_mode]

Decodes the five-byte answer to opcode `03`: four BCD frequency bytes (10 Hz units) and one mode byte. It returns two observations, `Frequency(A, hz)` and `Mode`, the mode taken from the description's mode code table. If the slice is not five bytes, a nibble is not a digit, or the mode code is unknown, it returns one `Inbound::Unrecognised` carrying the bytes. Only VFO A is reported: these rigs have no second VFO readout, so the transmit VFO is deliberately not reported (see the module note on split and the active transmit frequency). Pure.
