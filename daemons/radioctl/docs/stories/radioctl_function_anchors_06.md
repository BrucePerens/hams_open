# radioctl function anchors, batch 06

Function anchors for the `radioctl-core` power model and rig-file validators, the `radioctld` power and meter conversions, the Hamlib capability-matrix tool and a few small name tables, written to the ADR 0113 standard. Each anchor is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Nothing here keys a transmitter: the power and meter functions only convert or display readings the rig already produced, and `Capability::transmits` is the predicate the transmit gate uses to decide which capabilities are gated.

## `crates/radioctl-core/src/power.rs`

### `pub fn dbm(self) -> f64` [@ANCHOR: radioctl:radioctl_power_dbm]

Returns a `Power` as decibels relative to one milliwatt, computed as `10 * log10(watts) + 30`. It takes `self` by value (`Power` is `Copy`) and never fails. Zero watts gives negative infinity because `log10(0)` is negative infinity in IEEE arithmetic; callers that print the value must handle that special case. It is pure and is the inverse of `Power::from_dbm`, used when a display or log wants a logarithmic figure.

### `pub fn with_offset_db(self, offset_db: f64) -> Option<Power>` [@ANCHOR: radioctl:radioctl_power_offset_db]

Corrects a power measured behind a directional coupler or attenuator back to the power at the transmitter. `offset_db` is the loss in decibels; a positive value means the meter reads lower than the line, so the result is `watts * 10^(offset_db / 10)`. The ratio is applied before any display conversion. Returns `None` when the result is not finite and non-negative, for example a NaN offset or one so large the product overflows. Pure; the value is not clamped.

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_power_unit_name]

Gives the configuration spelling of a `PowerUnit`: `w` for watts, `dbm`, `dbw` and `dbmv50` for decibels relative to one milliwatt, one watt and one millivolt across 50 ohms. These are the strings used in station configuration and by `from_name`, so the two round-trip exactly. The match is exhaustive over the enum, so there is no error or fallback value. Pure and allocation free.

### `pub fn from_name(name: &str) -> Option<PowerUnit>` [@ANCHOR: radioctl:radioctl_power_unit_from_name]

Looks up a `PowerUnit` by its configuration name by comparing `name` against `name()` of each variant. The comparison is exact and case sensitive: `w`, `dbm`, `dbw` and `dbmv50` are accepted, anything else (including `W` or `dBm`) returns `None`, and the caller decides how to report the bad configuration value. The string is borrowed and not retained. Pure.

### `pub fn to_power(self, value: f64, offset_db: f64) -> Option<Power>` [@ANCHOR: radioctl:radioctl_power_unit_to_power]

Converts one meter figure into watts at the transmitter, the single entry point every power source uses so downstream code only sees watts. `value` is in this unit (watts, dBm, dBW or dBmV across 50 ohms); `offset_db` is the coupler and attenuator loss in decibels, positive when the meter reads low. The figure is first turned into watts with the matching `Power::from_*` constructor, then `with_offset_db` is applied. Returns `None` if the figure is NaN or infinite, a negative watt value, or the offset makes the result non-finite. Pure.

### `pub fn label(&self) -> &'static str` [@ANCHOR: radioctl:radioctl_power_origin_label]

Names the kind of source behind a power reading, for telemetry: `rig` for the radio's own meter and `external` for a wattmeter attached to the shack. The name of the external meter (the `String` inside `PowerOrigin::External`) is deliberately not part of the label, so the vocabulary stays two words. Borrows `self`, never fails, and returns a static string.

### `fn read(&self) -> Option<PowerReading>` [@ANCHOR: radioctl:radioctl_power_reader_read]

The one method of the `RfPowerReader` trait, implemented by anything that can supply transmit power: the rig's meter or an external wattmeter. It is synchronous and takes `&self`, and implementors are `Send + Sync`, so it may be called from the rig actor, a station task or a test. It returns the latest `PowerReading`, or `None` when there is no reading or the last one has outlived its `valid_for`. A stale reading must read as no power, never the last number. A source that needs I/O keeps its latest reading and returns it here, so the call does not block.

### `pub fn new(points: Vec<(f64, f64)>) -> Option<PowerCal>` [@ANCHOR: radioctl:radioctl_power_cal_new]

Builds a power-meter calibration from `(raw, watts)` points, taking ownership of the vector. It requires at least two points, strictly ascending raw values, watts non-negative and non-decreasing, and every number finite. Any violation returns `None`; there is no panic. Equal watts on adjacent points are allowed (a flat step) but equal or falling raw values are not. The rig-file loader uses it to validate the `[power_meter]` block, and the points come from a manual and are marked unverified until measured.

### `pub fn watts(&self, raw: f64) -> Power` [@ANCHOR: radioctl:radioctl_power_cal_watts]

Maps a raw meter deflection to watts by linear interpolation between the calibration points. A raw value at or below the first point gives the first point's watts; at or above the last gives the last point's watts (flat, never extrapolated); in between it interpolates on the surrounding segment. The result is clamped to be non-negative. `raw` is a float in the meter's own scale (0 to 255 for CI-V). Borrows the calibration, infallible because `new` guarantees at least two points. Pure.

### `pub fn full_scale(&self) -> Power` [@ANCHOR: radioctl:radioctl_power_cal_full_scale]

Returns the watts of the last calibration point, which is the most the meter can report because `watts` is flat beyond it. It is attached to a `PowerReading` as `scale` so a display can range its bar graph, and `power_fraction_cal` divides by it. Borrows the calibration and cannot fail, since a valid calibration has at least two points. Zero is possible only for an all-zero table.

### `pub fn rig_reading(raw: u16, cal: Option<&PowerCal>) -> PowerReading` [@ANCHOR: radioctl:radioctl_power_rig_reading]

Turns a raw deflection of the rig's own power meter into a `PowerReading`. `raw` is capped at 255 first. With a calibration the forward figure is `cal.watts(raw)` in real watts, `relative` is false and `scale` is the calibration's full scale. Without one the forward figure is `raw / 255 * 100` as a 0 to 100 bar, `relative` is true and `scale` is 100, so watts are never reported from an uncalibrated meter. Reflected power is `None`; origin is `Rig`; the reading is good for 1500 ms (`RIG_READING_VALID_FOR`). Pure.

### `pub fn display_unit(watts: f64, previous: Option<DisplayUnit>) -> DisplayUnit` [@ANCHOR: radioctl:radioctl_power_display_unit]

Chooses milliwatts, watts or kilowatts for showing a power. `watts` is the figure; `previous` is the unit shown last time, or `None` on first display. Switching up happens at exactly 1 W and 1000 W. Switching down needs the value to drop 5 percent below the threshold (below 0.95 W or 950 W), so a reading wobbling around a threshold does not flip units. With no previous unit the plain thresholds apply. Pure, no errors; NaN falls into milliwatts because every comparison is false.

### `pub fn format_power(power: Option<Power>, previous: Option<DisplayUnit>) -> (String, DisplayUnit)` [@ANCHOR: radioctl:radioctl_power_format]

Formats a power for a status line and returns the text plus the unit used, which the caller passes back as `previous` next time. It picks the unit with `display_unit`, then keeps three significant digits: below 10 gets two decimals, below 100 one, else none (`5.00 mW`, `12.5 W`, `100 W`), and kilowatts always show two decimals (`1.50 kW`). A rounding carry to the next decade drops a decimal. `None` yields `--` and returns `previous` (watts if there was none). Pure.

## `crates/radioctl-core/src/rigfile.rs`

### `fn build_power_meter( section: Option<&PowerMeterSection>, ) -> Result<Option<PowerMeterParams>, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_power_meter]

Validates the optional `[power_meter]` table of a rig description file. With no section it returns `Ok(None)`: the rig simply has no watt calibration and its meter will read as a relative bar. With a section it copies the `raw_to_milliwatts` points (raw as `u16`, power as `u32` milliwatts) into `PowerMeterParams` and checks that `cal()` can form a calibration. If not, it returns a `RigError` saying the table needs at least two points ascending in raw value and not falling in power. It reads only its argument; the caller, `parse`, also demands a manual citation for the points via the facts table.

### `fn build_elecraft(section: &ElecraftSection) -> Result<ElecraftParams, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_elecraft]

Builds the Elecraft family parameters from the `[elecraft]` block: the model id (copied as given) and the per-model `levels` table, which is validated by `build_level_ranges` under the block name `elecraft`. Any error from the level table is returned unchanged. It does not decide whether an `rf-power` range exists: `parse` refuses an Elecraft description that claims the levels capability without one, so no default power range stands in for the model's own. Pure; borrows the section.

### `fn build_level_ranges( block: &str, levels: &BTreeMap<String, (u16, u16)>, ) -> Result<Vec<(Level, u16, u16)>, RigError>` [@ANCHOR: radioctl:radioctl_core_rigfile_build_level_ranges]

Turns a family block's `levels` table (used for `elecraft.levels` and `yaesu.levels`) into `(Level, min, max)` triples. `block` only labels error messages. Each key must name one of the numeric levels: af-gain, rf-gain, mic-gain, monitor-level, keyer-speed or rf-power; other level names (such as attenuator) are refused. Each value is `[min, max]` with `min <= max <= 999`, the limit of three wire digits. The first violation returns a `RigError` naming the block and key. Output order follows the sorted map. Pure.

## `crates/radioctld/src/meter_cal.rs`

### `pub fn power_fraction(cal: &MeterCal, raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_power_fraction]

Serves an uncalibrated `RFPOWER_METER` as a 0 to 1 fraction for a raw 0 to 255 deflection. It interpolates the model's printed `power` points (the default model reads the deflection over 255) and clamps the result to 0 and 1. This is a relative bar, not a measurement, and the scale is an unverified estimate (`CALIBRATION_VERIFIED` is false). A rig with watt calibration points uses `power_fraction_cal` instead. Pure; deflections above 255 are treated as 255.

### `pub fn power_fraction_cal(cal: &radioctl_core::power::PowerCal, raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_power_fraction_cal]

Serves `RFPOWER_METER` for a rig that has watt calibration points. It converts the raw deflection to watts with `cal.watts` and divides by the calibration's full scale (the last point), clamping to 0 and 1. If full scale is zero (an all-zero table) it returns 0.0 rather than divide by zero. `raw` is `u16` and is not capped here, but the calibration is flat beyond its last point. Pure, no errors.

## `crates/radioctld/src/service.rs`

### `async fn power_raw(&self) -> Result<u16, i32>` [@ANCHOR: radioctl:radioctld_service_power_raw]

Reads the engine's raw power value through the rig actor with `Op::GetPower`, in the family's own scale: a CI-V 0 to 255 deflection, Yaesu `RM5`, or Elecraft `PO` watts. It is a read-only query and never keys the rig. Awaits the actor, so it may wait on the serial port and its timeouts. Errors are `i32` rigctl codes: whatever `read` returns, or `EIO` when the observation is of a different kind than a power value.

### `pub async fn power_reading(&self) -> Option<radioctl_core::power::PowerReading>` [@ANCHOR: radioctl:radioctld_service_power_reading]

Public entry for the rig's own transmit-power reading, for stations that publish telemetry. It reads the raw value with `power_raw` and converts it with `rig_reading` using the description's `power_meter` calibration when present: watts with a calibration, otherwise a relative 0 to 100 bar. Any read error returns `None` (no power, not an old number). Async and read-only, it never keys the rig, and blocks only as long as the rig query takes.

## `crates/radioctl-core/src/units.rs`

### `pub fn name(self) -> &'static str` [@ANCHOR: radioctl:radioctl_core_level_name]

Returns the file-format name of a `Level`, used in rig description and transcript files: af-gain, rf-gain, mic-gain, monitor-level, keyer-speed, rf-power, attenuator, preamp, dither, preselector and antenna (hyphenated lower case). It searches the `LEVEL_NAMES` table; the fallback `unknown` cannot occur while the table covers every variant. Pure, and the inverse of `from_name`.

### `pub fn from_name(name: &str) -> Option<Level>` [@ANCHOR: radioctl:radioctl_core_level_from_name]

Finds the `Level` with a given file-format name, the parse side of `Level::name`. The match is exact and case sensitive against the `LEVEL_NAMES` table. Unknown text (`volume`, `AF-GAIN`) returns `None`; callers such as the rig-file and transcript loaders turn that into an error naming the bad value. The input is borrowed. Pure.

### `pub fn as_mhz(self) -> f64` [@ANCHOR: radioctl:radioctl_core_hz_as_mhz]

Converts a frequency in integer hertz to megahertz as a float, dividing by 1,000,000. It is for display only: frequencies are kept as integer hertz everywhere else so no rounding creeps into tuning. Takes `Hz` by value (it is `Copy`), cannot fail, and the result may lose precision above about 2^53 Hz, far beyond any radio.

## `crates/radioctl-core/src/transcript.rs`

### `fn step_bytes(hex: &str, text: &Option<String>, index: usize) -> Result<Vec<u8>, RigError>` [@ANCHOR: radioctl:radioctl_core_transcript_step_bytes]

Decodes one direction (send or expect) of a transcript step into bytes. A direction is given either as hex in `hex` or as ASCII in `text`, never both. If `text` is set and `hex` is non-blank it fails with a `RigError` naming the step `index`; if only `text` is set its UTF-8 bytes are used; otherwise `hex` goes through `parse_hex`, which refuses an odd digit count or non-hex characters. `index` is used only in the message. Pure.

## `tools/hamlib_caps_matrix.py`

The Python tool below turns the program output of `rigctl` into radioctl's capability matrix and the generated differences document. It reads no Hamlib source.

### `def parse_rig_list(text: str) -> list[dict]` [@ANCHOR: radioctl:radioctl_caps_parse_rig_list]

Parses the output of `rigctl -l` into one dict per model with `model_id` (int), `mfg`, `model`, `backend_version`, `status` and `macro`. Column positions are found from the header line (`Rig #`, `Mfg`, `Model`, `Version`, `Status`, `Macro`), so a change in padding does not break it. Blank lines are skipped. Raises `IndexError` or `ValueError` if the text is empty, the header lacks a column, or the id is not an integer. Pure; no process is run here.

### `def parse_dump(text: str) -> dict` [@ANCHOR: radioctl:radioctl_caps_parse_dump]

Parses one `rigctl --dump-caps` report into the `dump` fields: identity (model, manufacturer, backend version and status, rig and port type), VFO (variable frequency oscillator, the tuning control) list and operations, modes, get and set functions, levels and parameters (with min, max and step), preamp, attenuator, CTCSS (continuous tone-coded squelch system) and DCS counts, memory banks, spectrum data, serial settings, write and timeout timing, RIT/XIT limits and the `Can` and `Has` Y/N/E flags. Absent keys become empty strings, lists or zero; it does not raise on a missing field. Pure, reads only its argument.

### `def parse_ranges(lines: list[str]) -> dict` [@ANCHOR: radioctl:radioctl_caps_parse_ranges]

Summarises the TX and RX (receive) frequency range blocks of a dump. For each of `TX` and `RX` it returns `count` of ranges, lowest and highest hertz (`min_hz`, `max_hz`, `None` when there are none), and for TX also `max_power_w`, the highest `High power` figure in watts seen. Range headers like `TX ranges #1 for ...` select the section; lines outside a section are ignored. Pure and tolerant: unparsable lines are skipped, not raised.

### `def classify_family(mfg: str, model: str, port_type: str) -> dict` [@ANCHOR: radioctl:radioctl_caps_classify_family]

Derives a protocol family hint such as `icom_civ`, `elecraft_ascii`, `kenwood_ascii`, `yaesu_newcat_ascii`, `flexradio_network` or `virtual`, from manufacturer, model name and port type. It returns `{"family": ..., "basis": ...}`; the first matching rule wins and `basis` says which one fired, so a reader can see how weak the evidence is (a port-type-only match is the weakest). It is a planning hint, not Hamlib output and not a statement of the wire protocol. Pure; no error.

### `def vfo_model(vfo_list: list[str]) -> str` [@ANCHOR: radioctl:radioctl_caps_vfo_model]

Classifies the VFO arrangement from the VFO names a dump prints: `main_sub` when both Main and Sub are present, else `a_b` when VFOA and VFOB are, else `single` when VFOA or a bare VFO is present, else `none`. The list is not mutated. Order of tests matters: Main/Sub wins over A/B. Pure, no errors.

### `def quirk_hints(dump: dict) -> list[str]` [@ANCHOR: radioctl:radioctl_caps_quirk_hints]

Derives timing and behaviour hints from parsed dump fields only, as tag strings: inter_byte_write_delay, post_write_delay, long_timeout (1000 ms or more), many_retries (5 or more), flow_control_*, two_stop_bits, seven_data_bits, async_data_capable, transceive_flag_E, no_ptt_readback, no_frequency_readback, backend_alpha or backend_beta, and backend_self_reported_gaps. Missing timing or serial blocks count as empty. Reads the dict keys `timing`, `serial`, `async_data`, `can`, `backend_status` and `backend_warnings` and raises `KeyError` if one is absent. Pure.

### `def build_entry(row: dict, dump: dict | None, error: str) -> dict` [@ANCHOR: radioctl:radioctl_caps_build_entry]

Assembles one matrix entry for a model: `rig_list` (the list row), `dump_error` (the error text or `None`), `dump` and `derived`. When `dump` is `None` (the dump failed) both `dump` and `derived` are `None`. Otherwise `derived` holds `family`, `family_basis`, `vfo_model`, `has_scope` and `quirk_hints`, computed from the row and dump. Pure; the row and dump are stored by reference, not copied.

### `def build_matrix(jobs: int) -> dict` [@ANCHOR: radioctl:radioctl_caps_build_matrix]

Runs `rigctl --version` and `rigctl -l`, then `rigctl -m N --dump-caps` for every listed model on a thread pool of `jobs` workers, and returns the whole matrix: `schema_version`, a `provenance` block (source, rigctl version, commands, which fields are parsed or derived) and `models`. It spawns subprocesses with a 60 second timeout each and so can take minutes for a full Hamlib; a model whose dump fails becomes an entry with `dump` `None`. It does no radio I/O and cannot transmit. A missing `rigctl` raises `FileNotFoundError`.

### `def render_markdown(matrix: dict) -> str` [@ANCHOR: radioctl:radioctl_caps_render_markdown]

Renders the generated HAMLIB_DIFFERENCES document from a matrix: a heading, a provenance note saying no Hamlib source was read, counts of models listed, dumped and not dumpable, then sections for backend status, port type, derived protocol families, manufacturers, timing, VFO models, features and quirk hints, a list of the models whose dump failed, and a per-model table. The output is deterministic for a given matrix (sorted groups) so `--check` can compare it with the committed file. Returns a string with no trailing newline; `main` adds one. Pure.

### `def main(argv: list[str]) -> int` [@ANCHOR: radioctl:radioctl_caps_main]

Command-line entry point. Options: `--out-dir DIR` (required), `--jobs N` (threads, default 4) and `--check`. It builds the matrix and renders `capability_matrix.json` and `HAMLIB_DIFFERENCES.md`. Without `--check` it creates the directory, writes both files and prints `wrote PATH`, returning 0. With `--check` it writes nothing, compares with the existing files and prints `DRIFT: ...` and returns 1 when one differs or is missing. It runs `rigctl` subprocesses, so it needs Hamlib installed; it does not touch a radio.

## `crates/radioctl-core/src/capability.rs`

### `pub fn transmits(self) -> bool` [@ANCHOR: radioctl:radioctl_core_capability_transmits]

Says whether using a capability can put RF (radio frequency) on the air. It is true only for `SetPtt` and `SendCw`; reads, frequency and mode changes, levels and meters return false. This is the predicate the transmit gate relies on when deciding which operations need transmit authority, so adding a capability that can key the transmitter must be reflected here. Takes `self` by value (`Copy`), pure and infallible.
