# Glasgow logic analyzer and pattern generator tools (`bench_gla_*`)

Code: `src/gla/`. Manual: `manual/instruments/glasgow/logic-analyzer.md` and the pages `tools/bench_gla_*`. Tests: `src/gla/tests.rs`.
One-paragraph design: the Glasgow revision C `analyzer` applet streams every pin change with a 48 MHz timestamp and has no trigger
and no rate setting. `bench_gla_capture` asks the Glasgow module's `bench_glasgow_logic_capture` for a VCD, then does the trigger,
pre-trigger window, resampling, statistics, files and protocol decoding on the host. `bench_gla_pattern` plans a pattern here and has
`bench_glasgow_pattern` play it. The module never reaches the device itself: every safety rule (port voltage set first, tier, time
limit, confirm, park) is the Glasgow module's. All code except `tools::capture`, `tools::pattern_tool` and the entry points is pure
and runs without hardware.

Entries are grouped by file. Every function, data structure and constant set has one; the id is the `[@ANCHOR: ...]` in the code.
Times in the model are nanoseconds (`u64`) from the start of the capture; a "pin" is a bit position in a `u32` pin-state word
(bit n = captured pin n, in the order of the VCD's wires, which is the order of the `pins` argument).

## Contents

`gla_wave_struct`, `gla_sampled_struct`, `gla_wave_value_at`, `gla_wave_value_before`, `gla_wave_pin_index`, `gla_parse_vcd`,
`gla_write_vcd`, `gla_write_csv`, `gla_window`, `gla_resample`, `gla_describe`; `gla_summarize`; `gla_trigger_enum`,
`gla_trigger_parse`, `gla_trigger_find`, `gla_trigger_find_state`, `gla_trigger_apply`; `gla_crc32`, `gla_zip_store`,
`gla_rate_text`, `gla_write_sr`; `gla_decode_uart`, `gla_decode_i2c`, `gla_decode_spi`, `gla_decode_run`; `gla_pattern_plan_struct`,
`gla_pattern_quantize`, `gla_pattern_hold`, `gla_pattern_num`, `gla_pattern_plan`, `gla_pattern_backend_args`; `gla_backend_trait`,
`gla_ctx_struct`, `gla_pin_names`, `gla_auto_rate`, `gla_limits`, `gla_read_wave`, `gla_inside`, `gla_process`, `gla_analyze`,
`gla_capture`, `gla_pattern_tool`; `gla_tool_defs`, `gla_real_backend`, `gla_instrument`, `gla_call`.

## Waveform model (`src/gla/wave.rs`)

### `gla_wave_struct`

**Purpose.** A captured waveform: the common currency of every other function. Built by `parse_vcd` or the tests, cut by `window`,
read by the statistics, the trigger, the decoders and the writers. Lives for one tool call; never shared between threads.

```rust
pub struct Wave { pub names: Vec<String>, pub init: u32, pub edges: Vec<Edge>, pub end_ns: u64, pub origin_ns: u64, pub overrun_ns: Option<u64> }
pub struct Edge { pub t_ns: u64, pub value: u32 }
```

| Field | Type | Meaning |
|---|---|---|
| `names` | `Vec<String>` | Pin names in bit order, 1 to 16 (`MAX_PINS`). Names from the VCD (`pin[0]`) or the port pins (`A0`) after `bench_gla_capture` renames them. |
| `init` | `u32` | Pin states at time 0 (the VCD `$dumpvars` block). Set by `parse_vcd`, or by `window` to the state just before the window. |
| `edges` | `Vec<Edge>` | Changes, strictly increasing in `t_ns`; `value` holds all pins after the change; simultaneous changes are one entry; an entry equal to the previous state is never stored. A change at time 0 is allowed only in a window. |
| `end_ns` | `u64` | When the capture ended (at least the last change). Pulses are measured up to it. |
| `origin_ns` | `u64` | Where time 0 of this wave is in the original capture: 0 for a parsed file, the window start after `window`. Added back to reported times. |
| `overrun_ns` | `Option<u64>` | Time of the analyzer's FIFO-overrun marker (`x` in the VCD): samples after it are lost. |

**Invariants.** `edges` sorted and strictly increasing; `end_ns >= last edge`; bits above `names.len()` are zero. **Serialized form:**
VCD (`write_vcd`), CSV (`write_csv`), sigrok `.sr` (`write_sr`, via `Sampled`). **Created by** `parse_vcd`, `window`; **read by**
everything else. **Tests:** `vcd_is_parsed_with_initial_state_coalesced_changes_and_the_overrun_marker`.

### `gla_sampled_struct`

**Purpose.** A wave on a fixed time grid, the form the sigrok `.sr` file needs. Made by `resample`, consumed by `write_sr`.

| Field | Type | Meaning |
|---|---|---|
| `rate_hz` | `f64` | Sample rate, 1 to 48 000 000. Sample i is the pin state at `round(i * 1e9 / rate_hz)` ns. |
| `data` | `Vec<u32>` | One pin-state word per sample; at most `MAX_SAMPLES` (8 million). |
| `missed_pulses` | `u64` | Pulses (spans between two changes of one pin) with no sample instant inside: in the source, invisible at this rate. |

**Tests:** `resampling_follows_the_wave_and_counts_the_pulses_it_cannot_show`.

### `gla_wave_value_at`

**Purpose.** The pin states at a time, for decoders and the trigger. `fn value_at(&self, t_ns: u64) -> u32`: the state after the
last change at or before `t_ns` (`init` before the first). Binary search, O(log n), no side effects, never fails. **Tests:**
`value_lookups_and_pin_names_resolve`.

### `gla_wave_value_before`

**Purpose.** What a clocked receiver sees at an edge: the state just before change number `i`. `fn value_before(&self, i: usize) -> u32`
returns `init` for 0 and `edges[i-1].value` otherwise. `i` must be a valid index into `edges` or `edges.len()`; larger panics. Used
by the I2C (two-wire serial bus), UART and SPI (four-wire serial bus) decoders and the edge trigger. **Tests:** `value_lookups_and_pin_names_resolve`.

### `gla_wave_pin_index`

**Purpose.** Resolve a pin as the caller wrote it. `fn pin_index(&self, pin: &Value) -> Result<usize, String>`. `pin` is a JSON
string (a name in `names`, such as `"A0"`) or a number (a position, 0-based). **Errors:** a name not captured (the message lists the
captured pins), a position past the last pin, any other JSON type. **Tests:** `value_lookups_and_pin_names_resolve`.

### `gla_parse_vcd`

**Purpose.** Read the VCD of the Glasgow's `analyzer` applet (and any VCD of 1-bit wires) into a `Wave`.
`pub fn parse_vcd(text: &str) -> Result<Wave, String>`.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | The whole file. Borrowed. `$timescale` of s, ms, us, ns or ps is honoured; times are rounded to whole nanoseconds. |

**Returns** the wave; values at time 0 are `init`; `x` or `z` on any wire sets `overrun_ns` to the time it first appears (the
applet writes `x` on every wire at an overrun). **Errors:** no wires; a wire wider than 1 bit; more than 16 wires; an unknown unit; a
value other than 0, 1, x, z; a change naming an unknown id; time going backwards. **Special concerns:** pure, O(n); a multi-megabyte
file is read as text in memory (the backend caps the file at its `max_capture_mb`). **Tests:** `vcd_is_parsed_with_initial_state_coalesced_changes_and_the_overrun_marker`.

### `gla_write_vcd`

**Purpose.** The wave as VCD (1 ns timescale, scope `glasgow`, ids `!` onward), readable by GTKWave, PulseView and `parse_vcd`.
`pub fn write_vcd(w: &Wave) -> String`. A first change at exactly time 0 (possible in a window) is read back as part of the initial
state. **Tests:** `vcd_and_csv_written_from_a_wave_read_back_the_same`.

### `gla_write_csv`

**Purpose.** The wave as CSV for spreadsheets. `pub fn write_csv(w: &Wave) -> String`: header `time_ns,<names>`, a row for the state at
0, one row per change, and a last row at `end_ns` when it is later than the last change; each pin is `0` or `1`. **Tests:**
`vcd_and_csv_written_from_a_wave_read_back_the_same`.

### `gla_window`

**Purpose.** Cut and re-base a wave, for the trigger window. `pub fn window(w: &Wave, from_ns: u64, to_ns: u64) -> Wave`.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | Source; borrowed. |
| `from_ns`, `to_ns` | `u64` | Window bounds in the source's time; clamped to `0..=w.end_ns`, `to` at least `from`. |

**Returns** a wave whose time 0 is `from_ns`, `init` the state just before `from_ns` (so a change exactly at `from_ns` is kept, at
time 0), changes from `from_ns` to `to_ns` inclusive, `end_ns = to - from`, `origin_ns` increased by `from`, and the overrun marker
if it is inside. Pure. **Tests:** `a_window_is_rebased_and_keeps_where_it_came_from`.

### `gla_resample`

**Purpose.** Put the wave on a fixed grid (for `.sr`) and say what that cost. `pub fn resample(w: &Wave, rate_hz: f64) -> Result<Sampled, String>`.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | Source; borrowed. |
| `rate_hz` | `f64` | 1 to 48 000 000 (the analyzer's own timestamp clock); anything else, or NaN, is an error. |

**Returns** `Sampled` with `end_ns * rate + 1` samples and `missed_pulses`. **Errors:** rate out of range; more than
`MAX_SAMPLES` (8 000 000) samples ("lower sample_rate_hz or shorten the window"). **Cost:** O(samples + changes), 4 bytes per
sample in memory. **Tests:** `resampling_follows_the_wave_and_counts_the_pulses_it_cannot_show`.

### `gla_describe`

**Purpose.** The part of every answer that names a wave. `pub fn describe(w: &Wave) -> Value`: `{pins, duration_ns, changes, origin_ns,
overrun_at_ns}` (`overrun_at_ns` null when none). **Tests:** `describe_names_the_pins_the_length_and_an_overrun`.

## Statistics (`src/gla/stats.rs`)

### `gla_summarize`

**Purpose.** The per-pin report: edge counts, frequency, duty cycle, pulse widths, glitches. `pub fn summarize(w: &Wave, glitch_ns: u64) -> Value`.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The window to report on; borrowed. |
| `glitch_ns` | `u64` | A pulse shorter than this is a glitch. The tools default it to one sample period, at least 42 ns. |

**Returns** `{duration_ns, pins: {name: {first_level, last_level, edges, rising, falling, high_fraction, frequency_hz,
period_spread_ns, duty_cycle, high_pulse_ns {min,max}, low_pulse_ns {min,max}, glitches {threshold_ns, count, first[<=16]}}}}`. A
pulse is the span between two consecutive changes of one pin, so the first and last spans (cut by the ends) are not pulses. Frequency
is `1e9 / median rising-edge period`, null with fewer than two rising edges; `period_spread_ns` is longest minus shortest period (a
jitter figure); `duty_cycle` is the high time over the complete cycles between the first and last rising edge. Glitch times are in
the original capture's time. Pure, O(changes x pins). **Tests:** `a_square_wave_has_its_frequency_duty_edges_and_pulse_widths_reported`,
`glitches_below_the_threshold_are_counted_and_listed_and_a_flat_pin_has_no_frequency`.

## Trigger (`src/gla/trigger.rs`)

### `gla_trigger_enum`

**Purpose.** What starts the window. `enum Trigger { Immediate, Edge { pin, rising: Option<bool> }, Level { pin, level, min_ns }, Pattern { mask, want, min_ns } }`.
`pin` is a bit position; `rising` true, false or either (None); `Level` holds one pin at `level` for at least `min_ns` (0 means the
moment it is there); `Pattern` holds when `state & mask == want` for at least `min_ns`. Made by `trigger::parse`, used by `find` and
`apply`; a value, no sharing. **Tests:** `trigger_specs_are_parsed_and_bad_ones_refused`.

### `gla_trigger_parse`

**Purpose.** Read the `trigger` argument. `pub fn parse(w: &Wave, v: Option<&Value>) -> Result<Trigger, String>`. `v`: none or null or
`{"type":"none"}`; `{"type":"edge","pin":"A0","edge":"rise|fall|either"}`; `{"type":"level","pin":..,"level":0|1,"min_ns":n}`;
`{"type":"pattern","pattern":"1x0","min_ns":n}` with one character per captured pin, first pin first (0, 1, x for don't care).
**Errors:** unknown type; missing pin; unknown pin (`pin_index`); a bad edge or level; a pattern of the wrong length, with a bad
character, or only x. **Tests:** `trigger_specs_are_parsed_and_bad_ones_refused`.

### `gla_trigger_find`

**Purpose.** The time of the first trigger at or after a time. `pub fn find(w: &Wave, t: &Trigger, from_ns: u64) -> Option<u64>`.
Returns the time of the matching change (edge), or the start of the span on which a level or pattern condition held for `min_ns`;
`Immediate` returns `from_ns`; None when it never occurs. A level or pattern already true at `from_ns` fires there. Pure. **Tests:**
`edge_level_and_pattern_triggers_find_the_right_moment`.

### `gla_trigger_find_state`

**Purpose.** Shared search behind level and pattern triggers. `fn find_state(w, from_ns, min_ns, ok: impl Fn(u32) -> bool) -> Option<u64>`
(private): walks the spans of constant state from `from_ns` and returns the start of the first span where `ok(state)` is true for at
least `min_ns` (with `min_ns` 0: any non-empty span). **Tests:** `edge_level_and_pattern_triggers_find_the_right_moment`.

### `gla_trigger_apply`

**Purpose.** The pre-trigger buffer. `pub fn apply(w: &Wave, t: &Trigger, pre_ns: u64, post_ns: u64) -> Result<(Wave, u64), String>`: the
window from `pre_ns` before the first trigger to `post_ns` after it (clamped to the capture) and the trigger's time inside the
window (less than `pre_ns` when the trigger is early in the capture). **Errors:** the trigger never occurred (the message says so and
the capture length). The revision C analyzer has no hardware trigger: the device records continuously and this runs on the
recording. **Tests:** `the_window_keeps_the_pre_trigger_part_and_the_trigger_position`.

## Files (`src/gla/export.rs`)

### `gla_crc32`

**Purpose.** The checksum ZIP needs. `pub fn crc32(data: &[u8]) -> u32`: CRC-32/IEEE, bitwise (no table); `crc32(b"123456789") ==
0xCBF43926`. Pure. **Tests:** `crc32_matches_the_check_value_and_the_zip_lists_every_entry`.

### `gla_zip_store`

**Purpose.** A dependency-free ZIP writer for the `.sr` file. `pub fn zip_store(entries: &[(&str, Vec<u8>)]) -> Vec<u8>`: each entry
stored without compression with a CRC (cyclic redundancy check), then a central directory and the end record; DOS date fixed at 1980-01-01, so the bytes depend
only on the content. Limits: 4 GiB per entry and archive (no ZIP64); names up to 65 535 bytes, ASCII. **Tests:**
`crc32_matches_the_check_value_and_the_zip_lists_every_entry` (reads it back with its own local-header parser).

### `gla_rate_text`

**Purpose.** The sample rate as sigrok writes it. `fn rate_text(hz: f64) -> String` (private): `2 MHz`, `250 kHz`, else `1234 Hz`
(rounded to a whole hertz). **Tests:** `a_sigrok_session_file_has_version_metadata_and_packed_samples`.

### `gla_write_sr`

**Purpose.** The sigrok session file, opened by PulseView and sigrok-cli. `pub fn write_sr(s: &Sampled, names: &[String]) -> Vec<u8>`:
a ZIP with `version` (`2`), `metadata` (INI: `samplerate`, `total probes`, `probeN=<name>`, `unitsize`, `capturefile=logic-1`) and
`logic-1-N` chunks of 1 048 576 samples, packed little-endian, 1 byte per sample for up to 8 pins, else 2. Not compressed: 1 byte
per sample per 8 pins. **Tests:** `a_sigrok_session_file_has_version_metadata_and_packed_samples`.

## Decoders (`src/gla/decode.rs`)

All decoders work on the change list (exact to the capture's timestamps), return JSON, and list at most 4096 frames, transactions or
transfers (`MAX_OUT`; `count` is the number listed for UART and I2C, the number found for SPI; the error counts of the UART decoder are complete). Reported times are in the original capture's time.

### `gla_decode_uart`

**Purpose.** Asynchronous serial on one pin. `pub fn uart(w: &Wave, pin: usize, baud: f64, bits: u32, parity: &str, invert: bool) -> Result<Value, String>`.

| Argument | Type | Meaning |
|---|---|---|
| `pin` | `usize` | Bit position of the line. |
| `baud` | `f64` | 50 to 3 000 000. |
| `bits` | `u32` | Data bits 5 to 8, least significant first. |
| `parity` | `&str` | `none`, `even` or `odd`. |
| `invert` | `bool` | Idle low, start bit high. |

A frame starts at a change from idle to start level; the start level is checked again half a bit later (a shorter glitch counts as a
`false_start`); each bit is sampled in its middle; the first stop bit is checked (others are not). **Returns** `{frames[{t_ns, byte, hex,
framing_error, parity_error}], count, framing_errors, parity_errors, false_starts, text, last_frame_cut_by_capture_end}`; `text` shows
printable ASCII and `.` for the rest. **Errors:** arguments out of range. Baud is not detected: a wrong baud gives framing errors.
**Tests:** `uart_frames_are_decoded_with_text_parity_and_framing_errors`, `a_uart_glitch_is_not_a_frame_and_an_inverted_line_decodes`.

### `gla_decode_i2c`

**Purpose.** I2C on two pins. `pub fn i2c(w: &Wave, scl: usize, sda: usize) -> Result<Value, String>`. START is SDA falling while SCL
is high, STOP is SDA rising while SCL is high, data is sampled at each SCL rising edge, nine clocks per byte (the ninth is the
acknowledge, low is ACK). The first byte is the address (7-bit and R/W; a first byte `11110xx` is flagged `ten_bit` and not
interpreted further). **Returns** `{transactions[{start_ns, repeated_start, address, address_hex, read, address_ack, ten_bit, data[{hex,
ack}], stop, partial_bits}], count}`; a START with no STOP before the next START is a repeated start. **Errors:** `scl == sda`. No
clock-stretching or glitch filtering. **Tests:** `i2c_transactions_are_decoded_with_addresses_acks_repeated_starts_and_stops`.

### `gla_decode_spi`

**Purpose.** SPI. `pub fn spi(w, cs: Option<usize>, sck: usize, copi: Option<usize>, cipo: Option<usize>, mode: u8, bits: u32, msb_first: bool, cs_active_low: bool) -> Result<Value, String>`.

| Argument | Type | Meaning |
|---|---|---|
| `cs` | `Option<usize>` | Chip select; None: always selected, one transfer. |
| `sck` | `usize` | Clock. |
| `copi`, `cipo` | `Option<usize>` | Controller-out and peripheral-out lines; at least one. |
| `mode` | `u8` | 0 to 3 (CPOL, CPHA): modes 0 and 3 sample on the rising edge, 1 and 2 on the falling edge. |
| `bits` | `u32` | Word size 4 to 32. |
| `msb_first`, `cs_active_low` | `bool` | Bit order and chip-select polarity. |

Data is read from the state just before the sampling edge. **Returns** `{mode, bits, transfers[{start_ns, words, copi[hex], cipo[hex],
extra_bits}], count}` with one transfer per chip-select assertion. **Errors:** mode, bits or pins out of range. **Tests:**
`spi_words_are_decoded_in_every_mode_and_bit_order`.

### `gla_decode_run`

**Purpose.** Pick the decoder from the `decode` argument. `pub fn run(w: &Wave, spec: &Value) -> Result<Value, String>`. `spec.type` is
`uart` (`pin`, `baud` default 115200, `bits` 8, `parity` none, `invert` false), `i2c` (`scl`, `sda`) or `spi` (`sck`, optional `cs`,
`copi`, `cipo`, `mode` 0, `bits` 8, `msb_first` true, `cs_active_low` true). Pins by name or position. **Errors:** unknown type, missing
or unknown pin, and the decoder's own. **Tests:** `the_decode_argument_picks_the_decoder_and_reads_pins_by_name`.

## Pattern planner (`src/gla/pattern.rs`)

Constants (a set of related limits): `MIN_HOLD_MS` 0.5, `MAX_HOLD_MS` 5000, `MAX_STEPS` 256 per round, `MAX_REPEAT` 100 000. They are
the limits of `bench_glasgow_pattern`'s host timer and step list.

### `gla_pattern_plan_struct`

**Purpose.** A planned pattern. `struct Plan { steps: Vec<(u32, f64)>, repeat: u64, total_s: f64, achieved: Value }`: `steps` are (pin mask,
hold in ms), 0.1 ms resolution, played `repeat` times; `total_s` the sum; `achieved` the numbers that will really come out (for a
square wave `frequency_hz`, `duty`, `high_ms`, `low_ms`). Made by `plan`, turned into backend arguments by `backend_args`, reported
to the caller. **Tests:** `a_square_wave_is_planned_with_its_real_frequency_and_duty`.

### `gla_pattern_quantize`

`fn q(ms: f64) -> f64` (private): round to 0.1 ms. **Tests:** `a_square_wave_is_planned_with_its_real_frequency_and_duty`.

### `gla_pattern_hold`

**Purpose.** One hold checked against the host timer. `fn hold(ms: f64, what: &str) -> Result<f64, String>` (private): the quantized
hold, or an error naming `what`, the value and the 0.5 to 5000 ms range, and that faster edges need an FPGA-timed generator.
**Tests:** `a_square_wave_is_planned_with_its_real_frequency_and_duty`.

### `gla_pattern_num`

`fn num(spec, key, lo, hi, def: Option<f64>) -> Result<f64, String>` (private): a numeric field of the spec in range, or the default,
or an error naming the field and range. **Tests:** `a_square_wave_is_planned_with_its_real_frequency_and_duty`.

### `gla_pattern_plan`

**Purpose.** Turn a pattern description into steps and say what it will do. `pub fn plan(spec: &Value, npins: usize, max_total_s: f64) -> Result<Plan, String>`.

| Argument | Type | Meaning |
|---|---|---|
| `spec` | `&Value` | `type` `square`/`pwm` (`pin`, `frequency_hz` 0.1 to 2000, `duty` 0..1 default 0.5, `cycles` or `duration_s`), `pulses` (`pin`, `width_ms`, `gap_ms`, `count`), `bits` (`pin`, `bits` text of 0/1 up to 256, `bit_ms`, `repeat`), `steps` (`steps` [{value, hold_ms}], `repeat`). |
| `npins` | `usize` | How many pins the generator drives; `pin` is an index below it, other pins stay low. |
| `max_total_s` | `f64` | The longest run; the config's `max_capture_s`. |

**Errors:** unknown type; an out-of-range field; a high or low time that rounds outside 0.5 to 5000 ms (a square wave above about
1 kHz at 50 %); more than 256 steps after merging equal neighbours; a total over `max_total_s`. Pure: drives nothing. **Tests:**
`a_square_wave_is_planned_with_its_real_frequency_and_duty`, `pulse_trains_bit_patterns_and_raw_steps_are_planned`.

### `gla_pattern_backend_args`

`pub fn backend_args(p: &Plan, pins: &str) -> Value`: `{pins, steps[{value, hold_ms}], repeat}`, the arguments of
`bench_glasgow_pattern`; `confirm`, `dry_run` and `instrument` are added by the caller. **Tests:** `the_backend_arguments_carry_pins_steps_and_repeat`.

## Tools (`src/gla/tools.rs`)

### `gla_backend_trait`

**Purpose.** The one door to the Glasgow module. `trait Backend { fn call(&self, tool: &str, args: &Value) -> Result<Value, String>; }`.
Production: `Real`, which calls `glasgow::call` (all its rules apply); tests: a fake and the Glasgow module's simulated device.
Implementations may block for the length of a capture. **Tests:** `capture_asks_the_backend_with_only_its_own_arguments_and_analyses_the_file`.

### `gla_ctx_struct`

**Purpose.** What a call needs besides its arguments. `struct Ctx<'a> { backend: &'a dyn Backend, out_dir: PathBuf, max_capture_s: f64 }`:
`out_dir` is `BENCH_MCP_OUT` (files are written and read only there), `max_capture_s` the Glasgow instrument's cap on a capture or a
pattern. Built per call by `call`. **Tests:** `capture_asks_the_backend_with_only_its_own_arguments_and_analyses_the_file`.

### `gla_pin_names`

`pub fn pin_names(spec: &str) -> Result<Vec<String>, String>`: `A0:3` gives `A0 A1 A2 A3`, `A6:7,B0` gives `A6 A7 B0`. Errors: a part that
is not `A0`/`B3:5` (ports A and B, pins 0 to 7, range upward), or more than 16 pins. **Tests:** `pin_specs_become_names_and_wrong_ones_are_refused`.

### `gla_auto_rate`

**Purpose.** The default sample rate. `pub fn auto_rate(w: &Wave) -> f64`: the slowest of 1 kHz, 2 kHz, 5 kHz ... 20 MHz, 24 MHz, 48 MHz
that gives the shortest pulse of any pin at least 4 samples and keeps the sample count under 8 million; the fastest that fits when none
does; 1 kHz for a wave with no pulse. **Tests:** `the_automatic_rate_shows_the_shortest_pulse_with_four_samples`.

### `gla_limits`

**Purpose.** The link model behind `bench_gla_limits`. `pub fn limits(npins: u32, link_mbps: f64) -> Value`. A record is
(1, 2 or 3 delay bytes of 7 bits) + 1 event byte + `ceil(npins/8)` data bytes; the usable bulk rate is 0.91 MiB/s at 12 Mb/s (measured
with `glasgow run benchmark` sink mode on the unit; capture itself cannot run there) and 40 MB/s at 480 Mb/s (a model, not measured).
**Returns** `{model, pins, link_mbps, bulk_bytes_per_s, bulk_bytes_per_s_source, capture_works, note, timestamp_clock_hz, rows[{spacing,
bytes_per_edge, max_edges_per_s}], trigger, sample_rate}`. Pure. **Tests:** `the_limits_say_that_full_speed_cannot_capture_and_high_speed_is_edge_limited`.

### `gla_read_wave`

`fn read_wave(path: &Path) -> Result<Wave, String>` (private): read a file as text and `parse_vcd` it. Errors name the path.
**Tests:** `analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`.

### `gla_inside`

`fn inside(ctx: &Ctx, name: &str) -> Result<PathBuf, String>` (private): a plain file name joined to `out_dir`; a name that is empty,
has `/` or `\`, or starts with `.` is refused (no path traversal). **Tests:** `analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`.

### `gla_process`

**Purpose.** The analysis shared by capture and analyze. `pub fn process(ctx: &Ctx, w: &Wave, args: &Value, stem: &str) -> Result<Value, String>`.
Arguments read from `args`: `trigger`, `pre_trigger_ms`, `post_trigger_ms` (default: the rest) or `samples` with `sample_rate_hz`,
`sample_rate_hz` (default `auto_rate`), `glitch_ns` (default one sample period, at least 42 ns), `formats` (`vcd`, `csv`, `sr`; default
all; `[]` none), `decode`. Writes `<stem>.<ext>` into `out_dir` (overwrites). **Returns** `{trigger, window, full_capture, sampling,
summary, files, overrun, decode?}`. **Errors:** anything from the called functions; an unknown format; `samples` without a rate. Side
effects: files in `out_dir`. **Tests:** `analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`,
`a_capture_after_the_port_is_set_is_decoded_and_the_pins_are_parked`.

### `gla_analyze`

**Purpose.** `bench_gla_analyze`. `pub fn analyze(ctx: &Ctx, args: &Value) -> Result<Value, String>`: `args.file` is a `.vcd` name in `out_dir`;
the rest as `process`; output stem `<name>_analysis`; the answer adds `source`. Touches no device. **Tests:**
`analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`.

### `gla_capture`

**Purpose.** `bench_gla_capture`. `pub fn capture(ctx: &Ctx, args: &Value) -> Result<Value, String>`: checks `duration_s` (0.1 to
`max_capture_s`, default 2) and `pins` (default `A0:3`), calls the backend's `bench_glasgow_logic_capture` with only `pins`,
`duration_s`, `pull` and `instrument`, reads the VCD it wrote, names the pins after the port pins, then `process`. **Safety:** the
backend refuses when the port's voltage was not set by `bench_glasgow_port_set`, when the link is at full speed, and parks the pins
afterwards; this function cannot bypass that, and a `dry_run` answer of the backend is returned as is. Blocks for the capture
duration. **Tests:** `capture_asks_the_backend_with_only_its_own_arguments_and_analyses_the_file`,
`the_capture_and_the_pattern_refuse_until_the_port_voltage_is_set_through_the_backend`, `a_full_speed_link_stops_the_capture_in_the_backend_before_anything_runs`.

### `gla_pattern_tool`

**Purpose.** `bench_gla_pattern`. `pub fn pattern_tool(ctx: &Ctx, args: &Value) -> Result<Value, String>`: `pins` (1 to 8), `pattern` (or
the pattern fields at the top level), `confirm`, `dry_run`. Plans with `pattern::plan`, then calls `bench_glasgow_pattern` with the
steps and passes `confirm`, `dry_run` and `instrument` on. **Safety:** drives pins only through the backend, which requires the port
voltage to be set first, `confirm: true`, a run no longer than `max_capture_s`, and releases every pin at the end even on failure; a
plan error stops before the backend is called. **Returns** `{planned{steps, repeat, total_s, achieved, pins, timing}, backend}`. **Tests:**
`a_pattern_is_planned_here_confirmed_and_bounded_by_the_backend_and_loops_back_into_a_capture`,
`a_pattern_through_the_backend_needs_confirm_and_releases_the_pins`, `a_backend_refusal_ends_the_capture_and_the_pattern_with_its_own_message`.

## Entry points (`src/gla/mod.rs`)

### `gla_tool_defs`

`pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool>`: the `bench_gla_*` tools at or below the tier of a configured `glasgow`
instrument (`bench_gla_limits` and `_analyze` read_only; `_capture` and `_safe_state` read_config; `_pattern` full); every description
ends with its manual path and every schema has `instrument`. Empty without a Glasgow. **Tests:** `tools_are_listed_by_tier_the_instrument_is_found_and_the_tier_is_enforced`.

### `gla_real_backend`

`struct Real<'a>(&'a [Instrument])` implementing `Backend`: `call` is `glasgow::call(insts, tool, args)`, the Glasgow module's own
entry with its tier check and real device context. **Tests:** `the_production_backend_hands_the_call_to_the_glasgow_module`.

### `gla_instrument`

`fn glasgow_instrument(insts, args) -> Result<&Instrument, String>` (private): the instrument named by `args.instrument`, or the only
`glasgow` one; errors for none, several without a name, or an unknown name. **Tests:** `tools_are_listed_by_tier_the_instrument_is_found_and_the_tier_is_enforced`.

### `gla_call`

**Purpose.** The router of the `bench_gla_` prefix. `pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String>`:
unknown tool and tier errors come before anything else; builds the `Ctx` (`BENCH_MCP_OUT`, the instrument's `max_capture_s`), adds the
instrument name to the arguments and dispatches. `bench_gla_limits` reads the Glasgow's link speed from sysfs unless `link_mbps` is
given; `bench_gla_safe_state` runs `bench_glasgow_safe_state`. Called by the server for every `bench_gla_*` request. **Tests:**
`tools_are_listed_by_tier_the_instrument_is_found_and_the_tier_is_enforced`.
