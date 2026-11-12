# The Spectrum Analyzer's Sweeps, Markers, Screenshot and Tracking Generator, Entry by Entry

As an operator with a Siglent SSA3000X spectrum analyzer (with the tracking generator option) on the bench LAN, I want a model to set a frequency range, run one
sweep and get back a CSV, a summary and the strongest peaks; to place a marker; to take a screenshot; and to switch the tracking generator (TG) on and off, set
its level and measure a scalar S21 against a stored "through" reference, with the generator always proven off afterwards. This story covers
`daemons/bench_mcp/src/sa/sweep.rs` (the analyzer side: the range and request types, the trace, peaks, the CSV, the sweep, markers, the screenshot) and
`daemons/bench_mcp/src/sa/tg.rs` (the tracking generator tools and the reference store). The config entry, the shared helpers (`open`, `ask`, `ask_f64`,
`number`, `flag`, `on_off`, `options`), `bench_sa_status` and the dispatcher are in `sa_core.md`.

Nothing here transmits on the air. The tracking generator is a bench signal source with a rated output of -20 to 0 dBm into a cable between the TG OUTPUT and
the RF INPUT; it is handled at tier `read_config` (the tool table in `src/sa/mod.rs`; `bench_sa_screenshot` is `read_only`), with no confirm flag, band
check or TX gate. The safety rule instead is that the generator is "off unless a tool turned it on", and every path that turns it on also tries to turn it off
and read it back (`ensure_tg_off`). All the instrument calls of one analyzer run one at a time under the server's per-instrument lock (see `sa_core.md`); the
functions here take `&mut Link`, so they never share a connection.

## Types of the analyzer side (`sweep.rs`)

### `bench_mcp_sa_span` [@ANCHOR: bench_mcp_sa_span]

The frequency range of a sweep, declared in `src/sa/sweep.rs`: `pub struct Span` (`Clone`, `Copy`, `Debug`, `PartialEq`), a plain value built by `sweep_args` and
copied into a `SweepReq`. Never serialized; the range reaches the unit only as the `:SENSe:FREQuency:STARt` and `:STOP` commands in `sweep_inner`.

| Field | Type, unit | Meaning |
|---|---|---|
| `start_hz` | `f64`, Hz | Left edge of the trace (the first of its points). |
| `stop_hz` | `f64`, Hz | Right edge of the trace (the last point). |

Invariants (enforced by `sweep_args`, not by the type): `start_hz >= 0`, `stop_hz <= max_hz + 0.5` (the model's top frequency from `*IDN?`) and
`stop_hz - start_hz >= 100` Hz. The type has no default and no lock; it is `Copy` and owned by whoever holds the request. Tests citing it:
`sa::sweep::tests` (the sweep argument tests). See also `bench_mcp_sa_sweep_req`, `bench_mcp_sa_sweep_args`.

### `bench_mcp_sa_sweep_req` [@ANCHOR: bench_mcp_sa_sweep_req]

A checked sweep request: `pub struct SweepReq` in `src/sa/sweep.rs`, produced only by `sweep_args` (so every field has been range-checked) and consumed by
`run_sweep` / `sweep_inner`, and by `tg_sweep` for the TG tools. `None` in an optional field means "leave the instrument's own value". Not serialized; created
per call, owned by the tool function, borrowed (`&SweepReq`) by the sweep.

| Field | Type, unit | Range | Meaning |
|---|---|---|---|
| `span` | `Span`, Hz | see `Span` | The range to sweep. |
| `rbw_hz` | `Option<f64>`, Hz | 10 to 1e6 | Resolution bandwidth; when set, RBW auto is switched off first. |
| `vbw_hz` | `Option<f64>`, Hz | 1 to 1e6 | Video bandwidth; when set, VBW auto is switched off first. |
| `ref_level_dbm` | `Option<f64>`, dBm | -100 to 30 | Reference level of the display. |
| `attenuation_db` | `Option<u32>`, dB | 0 to 50, whole numbers | Input attenuator; when set, attenuation auto is switched off first. |
| `detector` | `Option<&'static str>` | `POSitive`, `NEGative`, `SAMPle`, `AVERage`, `NORMal` | The SCPI name of the trace 1 detector function. |
| `average` | `u32` | 1 to 999, default 1 | Sweeps averaged; 1 means a single sweep in write mode. |
| `continuous_after` | `bool` | default true | Put the unit back in continuous sweep when the sweep ends, on every path. |

Concurrency: none, it is a value. Tests citing it: `sa::sweep::tests` (argument checks). See also `bench_mcp_sa_sweep_args`, `bench_mcp_sa_run_sweep`.

### `bench_mcp_sa_trace` [@ANCHOR: bench_mcp_sa_trace]

One trace and the settings the unit itself reported for it: `pub struct Trace` in `src/sa/sweep.rs`. Built by `sweep_inner` from the unit's read-backs (never from
what was asked), kept by value by `bench_sa_sweep`, `Reference` (the through) and the S21 tool, and summarized by `trace_summary`. Not serialized as such; its
numbers are written to a CSV (`write_csv`) and a JSON summary.

| Field | Type, unit | Meaning |
|---|---|---|
| `start_hz`, `stop_hz` | `f64`, Hz | Range as the unit holds it (checked to within 1 Hz of the request). |
| `dbm` | `Vec<f64>`, dBm | The trace points, left to right; exactly `POINTS` = 751 from a real sweep (`Vec::new()` while the settings are being read). Point `i` is at `bin_hz(start, stop, n, i)`. |
| `rbw_hz`, `vbw_hz` | `f64`, Hz | Resolution and video bandwidth the unit reports. |
| `ref_level_dbm` | `f64`, dBm | Reference level the unit reports (0.0 in a `Trace` read from a reference file, which does not store it). |
| `attenuation_db` | `f64`, dB | Attenuation the unit reports. |
| `detector` | `String` | The detector name exactly as the unit returned it. |
| `sweep_time_s` | `f64`, s | Sweep time the unit reports (0.0 in a trace read from a reference file). |
| `average` | `u32` | Average count requested and set. |

Invariants: `start_hz < stop_hz`; a real trace has 751 finite points. Owned by the caller, no locking. Tests citing it: `sa::sweep::tests`, and the through tests
in `sa::tg::tests`. See also `bench_mcp_sa_run_sweep`, `bench_mcp_sa_trace_summary`, `bench_mcp_sa_reference`.

### `bench_mcp_sa_peak` [@ANCHOR: bench_mcp_sa_peak]

A single spectral peak: `pub struct Peak { freq_hz: f64, level_dbm: f64 }` in `src/sa/sweep.rs`. Produced by `find_peaks` (frequency from `bin_hz`, level the
trace point itself) and turned into `{"freq_hz","level_dbm"}` JSON objects by `trace_summary`. `freq_hz` is in Hz and lies between the trace's start and stop;
`level_dbm` is in dBm and is one of the trace's values. Not stored, no default, no concurrency. Tests citing it: `sa::sweep::tests` (peak finding). See also
`bench_mcp_sa_find_peaks`.

## Sweep arguments, parsing and files (`sweep.rs`)

### `bench_mcp_sa_sweep_args` [@ANCHOR: bench_mcp_sa_sweep_args]

`pub fn sweep_args(args: &Value, max_hz: f64) -> Result<SweepReq, String>` in `src/sa/sweep.rs`. Turns the JSON arguments of `bench_sa_sweep`,
`bench_sa_tg_normalize` and `bench_sa_tg_s21` into a `SweepReq` and checks every one before any command is sent to the unit, so a bad argument costs nothing on the
instrument. Called by `sweep`, `normalize` and `s21` right after `open` supplies `max_hz`.

| Argument | Type, unit | Range or values | Notes |
|---|---|---|---|
| `args` | borrowed JSON object | keys below | Absent or `null` keys count as not given. |
| `max_hz` | `f64`, Hz | model top frequency | From the identity (`open`); bounds start, stop, center and span. |
| `start_hz`, `stop_hz` | number, Hz | 0 to `max_hz` | Used together. |
| `center_hz`, `span_hz` | number, Hz | center 0 to `max_hz`; span 100 to `max_hz` | Used together; range is `center -/+ span/2`. |
| `rbw_hz`, `vbw_hz` | number, Hz | 10 to 1e6; 1 to 1e6 | Optional. |
| `ref_level_dbm` | number, dBm | -100 to 30 | Optional. |
| `attenuation_db` | whole number, dB | 0 to 50 | A fraction is "must be a whole number". |
| `detector` | string | `positive`, `negative`, `sample`, `average`, `normal` | Mapped to the SCPI names; any other string, or a non-string, is an error. |
| `average_count` | whole number | 1 to 999 | Default 1. |
| `continuous_after` | bool | default true | Anything but a boolean is an error. |

Returns `Ok(SweepReq)`. Errors (all `Err(String)`, never a panic): a number out of range or not a number (via `number`, which names the range and its source); a
non-boolean flag; "give start_hz and stop_hz, or center_hz and span_hz (not a mixture, not neither)"; a range with `start < 0`, `stop > max_hz + 0.5` or less than
100 Hz wide; a bad detector word. Pure: no I/O, no locking, no instrument contact. Example: `{"center_hz": 100e6, "span_hz": 10e6, "rbw_hz": 10000}` gives
95..105 MHz. Tests citing it: `sa::sweep::tests` (argument tests) and the sweep tests of `sa::tg::tests`. See also `bench_mcp_sa_number`, `bench_mcp_sa_flag`
(in `sa_core.md`), `bench_mcp_sa_span`, `bench_mcp_sa_sweep_req`.

### `bench_mcp_sa_parse_trace` [@ANCHOR: bench_mcp_sa_parse_trace]

`pub fn parse_trace(reply: &str) -> Result<Vec<f64>, String>` in `src/sa/sweep.rs`. Parses the comma separated ASCII trace the unit returns for `:TRACe:DATA? 1`
(after `:FORMat:TRACe:DATA ASCii`) into dBm values. Called only by `sweep_inner`.

`reply` is borrowed text: fields separated by commas, each trimmed; empty fields (such as a trailing comma or newline) are skipped. Returns the values in order.
Errors: a field that is not a finite number (named in the message, including `NaN` and `inf`), and fewer than two points ("the trace has only N points"). It does
not check that the count is 751; `sweep_inner` does that next. Pure, no I/O, no panic. Tests citing it: `sa::sweep::tests` (trace parsing). See also
`bench_mcp_sa_run_sweep`.

### `bench_mcp_sa_bin_hz` [@ANCHOR: bench_mcp_sa_bin_hz]

`pub fn bin_hz(t_start: f64, t_stop: f64, n: usize, i: usize) -> f64` in `src/sa/sweep.rs`: the frequency of point `i` of `n` points spread evenly from `t_start` to
`t_stop`, `t_start + (t_stop - t_start) * i / (n - 1)`, in Hz. Point 0 is exactly `t_start` and point `n-1` is exactly `t_stop`. Called by `find_peaks`, `write_csv`,
and the S21 tool (min and max positions); `trace_summary` uses the same bin width.

`n` must be at least 2: with `n = 1` the division is by zero (the result is not finite) and with `n = 0` the `usize` subtraction panics in a debug build. Every
caller passes a trace or column of at least two points (`parse_trace` and `write_csv` insist). `i` is not range checked (an `i` past the end extrapolates). Pure.
Tests citing it: `sa::sweep::tests` (bin frequency). See also `bench_mcp_sa_find_peaks`, `bench_mcp_sa_write_csv`.

### `bench_mcp_sa_find_peaks` [@ANCHOR: bench_mcp_sa_find_peaks]

`pub fn find_peaks(t: &Trace, count: usize, threshold_db: f64) -> Vec<Peak>` in `src/sa/sweep.rs`. Lists the strongest local maxima of a trace, so a model does not
have to read 751 points. Called by `sweep` (arguments `peaks`, default 5, 0 to 50, and `peak_threshold_db`, default 10, 0 to 200) and by `normalize` with count 0.

| Argument | Type, unit | Meaning |
|---|---|---|
| `t` | `&Trace`, borrowed | The trace; `t.dbm` must not be empty (the median is `sorted[len/2]`, which panics on an empty vector; every real trace has 751 points). |
| `count` | `usize` | Most peaks returned; 0 returns none. |
| `threshold_db` | `f64`, dB | A point is a candidate only when it is at least this far above the trace median. |

Rule: a candidate is `>=` its left neighbour and `>` its right neighbour (the ends have one neighbour), so a flat top counts once, at its right end. Candidates are
taken strongest first, and one is skipped when it lies within two points of an already taken, stronger peak. Returns peaks in order of falling level, each with its
frequency from `bin_hz` and its level. An all-flat trace gives one peak (its last point) when the threshold is 0 and none otherwise. Pure, no I/O. Tests citing it:
`sa::sweep::tests` (peak tests). See also `bench_mcp_sa_peak`, `bench_mcp_sa_bin_hz`.

### `bench_mcp_sa_new_path` [@ANCHOR: bench_mcp_sa_new_path]

`pub fn new_path(out: &Path, stem: &str, ext: &str) -> PathBuf` in `src/sa/sweep.rs`: builds `out/<stem>_<milliseconds>_<counter>.<ext>`, where the milliseconds are the
wall clock since the Unix epoch (0 if the clock is before it) and the counter is a process-wide `AtomicU64` (`FILE_COUNTER`, relaxed) incremented on every call. So
two calls in one process never get the same name, even in the same millisecond. It does not touch the disk: the caller creates the directory and the file.
Callers: `write_csv` (stem `sa_sweep`, `sa_tg_s21`, or `sa_tg_reference_<name>`) and `screenshot` (stem `sa_screen`, ext `bmp`). `stem` and `ext` are borrowed and
must not contain path separators (the callers pass fixed text or a name that `ref_name` already restricted to letters, digits, `-` and `_`). Two processes writing
to one directory in the same millisecond with the same counter could collide: unlikely, not guarded. Tests citing it: `sa::sweep::tests` (file names). See also
`bench_mcp_sa_write_csv`.

### `bench_mcp_sa_write_csv` [@ANCHOR: bench_mcp_sa_write_csv]

`pub fn write_csv(out, stem, comments, header, start, stop, cols) -> Result<PathBuf, String>` in `src/sa/sweep.rs`: writes the CSV that every sweep tool leaves in the
output directory (`BENCH_MCP_OUT`, via `crate::out_dir()`), and returns its path.

| Argument | Type | Meaning |
|---|---|---|
| `out` | `&Path` | Directory; created (with parents) if missing, a failure to create it is ignored and shows up as the write error. |
| `stem` | `&str` | File name stem, see `new_path`. |
| `comments` | `&[(String, String)]` | Written first as `# key=value` lines (empty for a plain sweep; the through reference stores its settings here). |
| `header` | `&str` | The column header line, for example `freq_hz,level_dbm` or `freq_hz,s21_db,measured_dbm`. |
| `start`, `stop` | `f64`, Hz | Frequency of the first and last row. |
| `cols` | `&[&[f64]]` | Value columns of equal length (at least two rows); one row per index. |

Format: first column `frequency` from `bin_hz` with one decimal, then each column with three decimals, comma separated, a newline per row. Errors: no columns or fewer
than two rows, or columns of different lengths ("the columns are empty or of different lengths"); a failed write ("write PATH: reason"). Side effect: one new file,
never overwriting an earlier one. Not locked (names are unique). Tests citing it: `sa::sweep::tests` (CSV). See also `bench_mcp_sa_new_path`,
`bench_mcp_sa_parse_reference` (reads the through file back).

## Running a sweep (`sweep.rs`)

### `bench_mcp_sa_run_sweep` [@ANCHOR: bench_mcp_sa_run_sweep]

`pub fn run_sweep(l: &mut Link, req: &SweepReq) -> Result<Trace, String>` in `src/sa/sweep.rs`. The one place a sweep is made: it sets the analyzer from `req`, runs one
sweep (or `average` sweeps) with continuous sweep off, and returns the `Trace` with the unit's own read-backs. Called by `sweep` and by `tg_sweep` (normalize and S21).
`l` is an open, identity-checked `Link` to the analyzer, borrowed mutably for the whole call; nothing else may use it meanwhile (the server's per-instrument lock).

Wrapper behaviour: it calls `sweep_inner`, then, when `req.continuous_after`, sends `:INITiate:CONTinuous ON` on every path (success or error, the result of that send is
ignored). Errors are those of `sweep_inner`. Side effects: SCPI traffic over the LAN socket; the unit's range, bandwidths, detector, trace mode and sweep count are
changed and stay changed (only continuous sweep is restored). It does not touch the tracking generator. No transmitter is involved. Tests citing it: `sa::sweep::tests`
(the sweep with a fake analyzer). See also `bench_mcp_sa_sweep_inner`, `bench_mcp_sa_tg_sweep`.

### `bench_mcp_sa_sweep_inner` [@ANCHOR: bench_mcp_sa_sweep_inner]

`fn sweep_inner(l: &mut Link, req: &SweepReq) -> Result<Trace, String>` (private) in `src/sa/sweep.rs`: the command sequence behind `run_sweep`. In order: continuous sweep
off, power unit dBm, logarithmic Y scale, ASCII trace format; start, stop, start again (so the unit's clamping order does not matter); RBW, VBW, attenuator (each with its
AUTO switched off first, only when given); the reference level AFTER the attenuator (the unit moved it when set first, found 2026-11-05); the detector; trace mode
`AVERage` with the count and a clear when `average > 1`, else `WRITe`; the sweep count.

Then it reads back and checks: start and stop within 1 Hz of the request (else "the analyzer holds A..B Hz ... it clamps a range it cannot make"); sweep time, and refuses
when `sweep time x average` exceeds 120 s (`MAX_SWEEP_S`, "Next: widen the RBW, narrow the span, or average fewer sweeps"), before the sweep starts; RBW, VBW, reference
level, attenuation and detector; a requested reference level or attenuation that differs by more than 0.05 from the read-back is an error naming the coupling. Then
`:INITiate:IMMediate`, and the wait for the end is `*OPC?` with a socket timeout of `ceil(total x 2 + 10)` seconds (an error names it and says to call `bench_sa_status`
or power-cycle). An answer other than `1` is an error. The trace is read with `:TRACe:DATA? 1` (read tries and reply timeout as in `sa_core.md`), parsed with
`parse_trace`, and must have exactly `POINTS` = 751 points, else "refusing to guess the frequency axis".

Errors: every `Link` failure and every check above, as `Err(String)` with a "Next:" step where the operator can act; no panic. It leaves the socket timeout at the sweep wait
value (later calls reset it: `ask` sets its own). Commands that change the unit are sent once, never repeated. Tests citing it: `sa::sweep::tests`. See also
`bench_mcp_sa_run_sweep`, `bench_mcp_sa_parse_trace`.

### `bench_mcp_sa_trace_summary` [@ANCHOR: bench_mcp_sa_trace_summary]

`pub fn trace_summary(t: &Trace, peaks: &[Peak]) -> Value` in `src/sa/sweep.rs`: the JSON a tool returns for a trace. Called by `sweep`, `normalize` (with no peaks) and `s21`
(which then adds its own `s21` object).

Fields of the object: `points` (count), `start_hz`, `stop_hz`, `bin_hz` (`(stop - start) / (points - 1)`), `rbw_hz`, `vbw_hz`, `ref_level_dbm`, `attenuation_db`, `detector`,
`sweep_time_s`, `average_count` (the unit's readbacks), `min_dbm`, `median_dbm`, `max_dbm` (from a sorted copy; the median is the element at `len/2`), and `peaks`, an array of
`{"freq_hz","level_dbm"}`. `t.dbm` must hold at least two points: an empty trace panics at the index and one point divides by zero (the callers always have 751). Pure, no I/O.
Tests citing it: `sa::sweep::tests`. See also `bench_mcp_sa_trace`, `bench_mcp_sa_peak`.

### `bench_mcp_sa_sweep` [@ANCHOR: bench_mcp_sa_sweep]

`pub fn sweep(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String>` in `src/sa/sweep.rs`: the tool `bench_sa_sweep` (tier `read_config`). One sweep with
the arguments checked first, written to a CSV, with a summary and peaks. Called by the dispatcher in `sa/mod.rs` with the instrument's own entry from the config.

| Argument | Meaning |
|---|---|
| `connect` | The connector that opens the link (a real TCP socket, or a fake in tests); borrowed. |
| `inst` | The configured instrument (host, port, `sa.max_tg_dbm`); borrowed. |
| `args` | The sweep arguments of `sweep_args`, plus `peaks` (0 to 50, default 5), `peak_threshold_db` (0 to 200, default 10) and `tg_off_after` (bool, default false). |

Steps: `open` (identity check; an address that is not an SSA3000X is sent nothing else), `sweep_args`, run the sweep, read the TG state (best effort, `.ok()`), and when `tg_off_after`
call `ensure_tg_off` (also when the sweep failed), combine both results with `finish`, find peaks, write `sa_sweep_*.csv` (`freq_hz,level_dbm`) in `BENCH_MCP_OUT`. Returns the
`trace_summary` plus `file` (path) and `tracking_generator`: `"off (verified)"` when `tg_off_after` succeeded, else `{"state":"on"|"off","level_dbm":..}` as read, or `null` when the state
could not be read. Errors: bad arguments, no usable identity, any sweep error, a failed switch-off (the SAFE STATE NOT CONFIRMED text, with the sweep's own error appended if both failed),
a CSV write error. The TG is never switched ON here; the optional switch-off is the only TG command. Example: `{"start_hz":1e6,"stop_hz":100e6,"rbw_hz":100000,"tg_off_after":true}`.
Tests citing it: `sa::sweep::tests` and `sa::tg::tests` (switch-off after a sweep). See also `bench_mcp_sa_run_sweep`, `bench_mcp_sa_finish`, `bench_mcp_sa_write_csv`.

### `bench_mcp_sa_marker` [@ANCHOR: bench_mcp_sa_marker]

`pub fn marker(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String>` in `src/sa/sweep.rs`: the tool `bench_sa_marker` (tier `read_config`). Switches a marker on trace 1
on, optionally searches or places it, and reads it back.

| Argument | Type, unit | Range | Meaning |
|---|---|---|---|
| `marker` | whole-valued number | 1 to 4, default 1 | Which marker. |
| `search` | string | `max`, `next`, `left`, `right` | Sent as `MAXimum`, `MAXimum:NEXT`, `MAXimum:LEFT`, `MAXimum:RIGHt`. |
| `freq_hz` | number, Hz | 0 to the model's top frequency | Place the marker at this frequency. |

Order of work: the `marker` and `search` arguments are checked before the connection is opened; `freq_hz` is checked after `open` (it needs `max_hz`). `search` and `freq_hz` together is an error
("give search or freq_hz, not both"). Then, on the unit: `STATe ON`, `MODE POSition`, `TRACe 1`, the search or the `X` set. Returns `{"marker", "trace": 1, "freq_hz", "level_dbm"}` read back
from the unit (`MARKer<m>:X?` and `:Y?`). Errors: bad arguments, identity, any I/O failure. The marker is left on. Reads the displayed trace as it is; it does not start a sweep, so in continuous
sweep the position can move between calls. No tracking generator access. Tests citing it: `sa::sweep::tests`. See also `bench_mcp_sa_number`.

### `bench_mcp_sa_screenshot` [@ANCHOR: bench_mcp_sa_screenshot]

`pub fn screenshot(connect: &Connector, inst: &Instrument) -> Result<Value, String>` in `src/sa/sweep.rs`: the tool `bench_sa_screenshot` (tier `read_only`, no arguments). Reads the screen with
`:HCOPy:SDUMp:DATA?` (through `Link::raw_until_idle` with a 1500 ms idle gap and a 40 s overall limit), which returns a headerless 24-bit BMP of 1024 x 600; checks it with `bmp_header`; and saves the first
`size` bytes as `sa_screen_<ms>_<n>.bmp` in `BENCH_MCP_OUT`.

Returns `{"file", "bytes", "width", "height", "format": "BMP"}`. Errors: identity failure; a reply that does not look like a BMP (from `bmp_header`); fewer bytes than the header declares ("the BMP says N
bytes and M arrived. Next: call it again (do not use USB)" - the USB connection cuts replies short); a failed write. Extra bytes beyond the declared size are dropped. It changes no instrument setting and
does not touch the tracking generator. Tests citing it: `sa::sweep::tests` (screenshot with a fake BMP). See also `bench_mcp_sa_bmp_header`, `bench_mcp_sa_new_path`.

### `bench_mcp_sa_bmp_header` [@ANCHOR: bench_mcp_sa_bmp_header]

`pub fn bmp_header(raw: &[u8]) -> Result<(u32, u32, usize), String>` in `src/sa/sweep.rs`: reads the size fields of a BMP file header. `raw` is the borrowed reply. Returns `(width, height, total_size)`: width
at offset 18 and height at offset 22 (little endian, `u32`), total file size at offset 2 (`usize`).

Errors (no panic): fewer than 54 bytes or not starting with `BM` ("N bytes that do not start like a BMP. Next: call bench_sa_status, then try again"); a declared size below 54, or a width or height of 0 or
above 4096 ("the BMP header is not plausible"). A top-down BMP stores a negative height, which reads as a huge `u32` and is also refused as implausible; the unit's screen dump is bottom-up. Pure. Tests
citing it: `sa::sweep::tests`. See also `bench_mcp_sa_screenshot`.

## The tracking generator tools (`tg.rs`)

### `bench_mcp_sa_tg_state` [@ANCHOR: bench_mcp_sa_tg_state]

`pub fn tg_state(l: &mut Link) -> Result<(bool, f64), String>` in `src/sa/tg.rs`: reads the generator's output state and level from the unit: `:OUTPut:STATe?` (through `ask`, 3 tries of 5 s,
then `on_off`, which accepts exactly `1`/`ON`/`0`/`OFF`) and `:SOURce:POWer?` (`ask_f64`). Returns `(output_on, level_dbm)`. Errors: no usable reply after the tries (with the "Next:" check of the cable and
analyzer), an unrecognised state word, a non-numeric level. Read-only: nothing is changed on the unit. `l` is the borrowed open link. Called by `sweep` (status in the result), `switch_on`, `tg_off`
and `tg_level`. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_on_off`, `bench_mcp_sa_ask` (in `sa_core.md`).

### `bench_mcp_sa_ensure_tg_off` [@ANCHOR: bench_mcp_sa_ensure_tg_off]

`pub fn ensure_tg_off(l: &mut Link) -> Result<(), String>` in `src/sa/tg.rs`: the safe-state routine. Up to `OFF_TRIES` (3) times it sets the reply timeout to `REPLY_TIMEOUT_S`, sends `:OUTPut:STATe OFF` and reads
`:OUTPut:STATe?`; it returns `Ok(())` only when the unit itself answers off. A send failure, a still-on answer or an unparseable answer is remembered and the loop tries again. (Here, unlike other writes, the repeat
is safe: switching off is idempotent.)

If all tries fail, the error starts with `SAFE STATE NOT CONFIRMED: the tracking generator may still be ON (<last reason>)` and tells the operator to press TG then TG off on the front panel or disconnect
what is on the TG OUTPUT, and not to rely on the analyzer until it reports off. Callers that turn the generator on (`switch_on`, `tg_sweep`) and `sweep` with `tg_off_after` and `bench_sa_tg_off` use it, and its
result is always passed on, never dropped. Side effects: the generator output goes off. Not a transmitter in the radio sense: a bench source of at most 0 dBm; no TX gate applies. Tests citing it:
`sa::tg::tests` (off verified, off failing). See also `bench_mcp_sa_finish`, `bench_mcp_sa_tg_off`.

### `bench_mcp_sa_finish` [@ANCHOR: bench_mcp_sa_finish]

`pub fn finish<T>(work: Result<T, String>, off: Option<Result<(), String>>) -> Result<T, String>` in `src/sa/tg.rs`: combines the result of some work with the result of the generator switch-off that
followed it. `off` is `None` when no switch-off was asked for. Table: work and no off, or off ok: the work's result unchanged; work ok but off failed: the off error (the data is dropped on purpose, so the
operator cannot miss the unconfirmed safe state); work failed and off failed: `"<work error>; <off error>"`; work failed and off ok: the work error. Generic over the success type (a `Trace`, a `Value`). Pure,
owns its arguments, no panic. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_ensure_tg_off`, `bench_mcp_sa_tg_sweep`.

### `bench_mcp_sa_level_arg` [@ANCHOR: bench_mcp_sa_level_arg]

`fn level_arg(inst: &Instrument, args: &Value) -> Result<Option<f64>, String>` (private) in `src/sa/tg.rs`: reads `tg_level_dbm` from the arguments. Accepts a finite number in `TG_RATED_MIN_DBM` (-20.0) to
`inst.sa.max_tg_dbm` (the config's cap; default and highest allowed 0.0 dBm); `None` when the key is absent or null. Errors: not a number; outside the range, with a message naming "the generator's range -20 to
0 dBm, and the config's max_tg_dbm". The cap is therefore enforced before the link is opened and before any command. Pure. Used by `tg_on`, `tg_level`, `normalize` and `s21`. Tests citing it: `sa::tg::tests`
(level limits). See also `bench_mcp_sa_number`, `bench_mcp_sa_limits` (in `sa_core.md`).

### `bench_mcp_sa_require_tg` [@ANCHOR: bench_mcp_sa_require_tg]

`fn require_tg(l: &mut Link) -> Result<(), String>` (private) in `src/sa/tg.rs`: checks that the analyzer lists the `TG` option in `:SYSTem:OPTions?` (through `options`, which splits on spaces and commas; the match
ignores case). Ok when present. Error otherwise: "this analyzer does not list the TG (tracking generator) option in :SYSTem:OPTions?" (and `options` can itself fail on a silent unit). Read-only. Called by `tg_on`,
`tg_level`, `normalize` and `s21` before any generator command; `tg_off` does not call it, so the switch-off can still be tried on a unit whose option list is unreadable. Tests citing it: `sa::tg::tests`.
See also `bench_mcp_sa_options` (in `sa_core.md`).

### `bench_mcp_sa_set_level` [@ANCHOR: bench_mcp_sa_set_level]

`fn set_level(l: &mut Link, dbm: f64) -> Result<(), String>` (private) in `src/sa/tg.rs`: sends `:SOURce:POWer <dbm>` and reads `:SOURce:POWer?` back; fails when the two differ by more than
`LEVEL_TOLERANCE_DB` (0.05 dB) ("the generator level reads back X dBm, not the Y dBm set"). The output state is not touched. `dbm` has already been checked by `level_arg`, so it is within -20 to the cap. Errors:
a send or read failure and the read-back mismatch. The write is sent once. Called by `switch_on` and `tg_level`. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_level_arg`, `bench_mcp_sa_tg_level`.

### `bench_mcp_sa_tg_on` [@ANCHOR: bench_mcp_sa_tg_on]

`pub fn tg_on(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String>` in `src/sa/tg.rs`: the tool `bench_sa_tg_on` (tier `read_config`, no confirm flag). Argument `tg_level_dbm` (optional, -20 to
`max_tg_dbm` dBm). Order: check the level (`level_arg`), open the link with the identity check, `require_tg`, then `switch_on`. Returns `{"tracking_generator":"on","level_dbm":<read back>}`. On any failure after the
open the generator is switched off again and its result is merged into the error (see `switch_on`). The generator output goes ON the unit's TG OUTPUT; nothing is radiated, and the tier, lock and band check
of the transmit tools do not apply. Side effect: the unit's output state and level change and stay changed until `bench_sa_tg_off` or a tool that switches it off. Tests citing it: `sa::tg::tests`.
See also `bench_mcp_sa_switch_on`, `bench_mcp_sa_tg_off`.

### `bench_mcp_sa_switch_on` [@ANCHOR: bench_mcp_sa_switch_on]

`fn switch_on(l: &mut Link, inst: &Instrument, level: Option<f64>) -> Result<Value, String>` (private) in `src/sa/tg.rs`: the one place the generator is switched on. With `level` it calls `set_level`
(set and verified); without it, it reads the present level and refuses when it is above `inst.sa.max_tg_dbm` ("give tg_level_dbm"). Then `:OUTPut:STATe ON` and `tg_state`, which must report on ("did not report on
after :OUTPut:STATe ON"). Returns `{"tracking_generator":"on","level_dbm":<level read back>}`. On any error in that sequence it calls `ensure_tg_off` and returns `finish(error, Some(off result))`, so a
failed turn-on never leaves the output on without saying so. Called by `tg_on` and `tg_sweep`. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_ensure_tg_off`, `bench_mcp_sa_finish`.

### `bench_mcp_sa_tg_off` [@ANCHOR: bench_mcp_sa_tg_off]

`pub fn tg_off(connect: &Connector, inst: &Instrument) -> Result<Value, String>` in `src/sa/tg.rs`: the tool `bench_sa_tg_off` (tier `read_config`, no arguments). Opens the link with the identity check, runs
`ensure_tg_off` (up to 3 tries, read back), then reads the level. Returns `{"tracking_generator":"off","verified":true,"level_dbm":<level>}`. Errors: identity failure, the SAFE STATE NOT CONFIRMED error,
or a failure reading the level (after the off was proven). It never needs the TG option check. Side effect: the generator output is off. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_ensure_tg_off`.

### `bench_mcp_sa_tg_level` [@ANCHOR: bench_mcp_sa_tg_level]

`pub fn tg_level(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String>` in `src/sa/tg.rs`: the tool `bench_sa_tg_level` (tier `read_config`). Sets `tg_level_dbm` (required here: "tg_level_dbm is
required" when absent; -20 to `max_tg_dbm` dBm) without changing whether the output is on. Order: `level_arg`, open, `require_tg`, `set_level` (read back within 0.05 dB), `tg_state`. Returns `{"tracking_generator":"on"|"off",
"level_dbm":<read back>}`. Errors as for those steps. Note that changing the level while the output is on changes the live signal. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_set_level`, `bench_mcp_sa_tg_on`.

## The through reference and S21 (`tg.rs`)

### `bench_mcp_sa_reference` [@ANCHOR: bench_mcp_sa_reference]

A through reference: `pub struct Reference` in `src/sa/tg.rs` (`Clone`, `Debug`, `PartialEq`): the trace taken with the TG cabled straight to the RF input, plus the TG level it was taken at. It is made by `normalize`,
kept in the in-memory store `refs()` (keyed `<instrument name>:<reference name>`), written to a CSV (`comments`), and read back from that CSV by `parse_reference`; `s21` subtracts it from a measurement.

| Field | Type, unit | Meaning |
|---|---|---|
| `tg_level_dbm` | `f64`, dBm | The generator level during the through; a measurement must use the same level (`check_same`). |
| `trace` | `Trace` | The through trace with its settings; from a file, `ref_level_dbm` and `sweep_time_s` are 0.0 because they are not stored. |

Invariants: the trace has 751 points and finite values. Lifetime: the in-memory copy lives until the server process ends; the CSV persists in the output directory. Serialized form: see `bench_mcp_sa_reference_comments`.
Concurrency: stored under the `refs()` mutex; values are cloned out. Tests citing it: `sa::tg::tests` (reference file). See also `bench_mcp_sa_parse_reference`, `bench_mcp_sa_check_same`.

### `bench_mcp_sa_refs` [@ANCHOR: bench_mcp_sa_refs]

`fn refs() -> &'static Mutex<HashMap<String, Reference>>` (private) in `src/sa/tg.rs`: the process-wide store of through references, a `static REFS: OnceLock<Mutex<HashMap<..>>>` created empty on first use. The key is
`"<inst.name>:<name>"`, so two analyzers (or two reference names) never share a slot. Written by `normalize` (insert, replacing a reference of the same key), read by `s21` (clone). Never cleared and never saved:
a restart of the server empties it, and the CSV or `reference_file` is the way back. Locking: `.lock().unwrap()` for the instants of an insert or lookup, never held across instrument I/O; a poisoned mutex (a panic
while held) would panic the next caller, which cannot happen in practice as the guarded code only inserts or clones. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_reference`, `bench_mcp_sa_normalize`.

### `bench_mcp_sa_ref_name` [@ANCHOR: bench_mcp_sa_ref_name]

`fn ref_name(v: Option<&str>) -> Result<String, String>` (private) in `src/sa/tg.rs`: validates a reference name. `None` becomes `"default"`. The name must be 1 to 40 characters, each an ASCII letter or digit, `-` or
`_`; anything else is the error "name must be 1 to 40 letters, digits, - or _". The restriction matters because the name goes into a file name (`sa_tg_reference_<name>_...csv`), so it can carry no path separator or dot.
Returns the owned name. Called by `normalize` (argument `name`) and `s21` (argument `reference`). Pure. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_new_path`.

### `bench_mcp_sa_reference_comments` [@ANCHOR: bench_mcp_sa_reference_comments]

`fn comments(r: &Reference) -> Vec<(String, String)>` (private) in `src/sa/tg.rs`: the `# key=value` header lines of a reference CSV, which `write_csv` writes above the column header. Keys, in order: `tool`
(`bench_sa_tg_normalize`), `tg_level_dbm`, `start_hz`, `stop_hz`, `rbw_hz`, `vbw_hz`, `attenuation_db`, `detector`, `average_count`: exactly the settings `check_same` compares, written with Rust's default `f64`
formatting. The reference level and the sweep time are not written. Example file head: `# tool=bench_sa_tg_normalize`, `# tg_level_dbm=-20`, `# start_hz=1000000`, ... then `freq_hz,level_dbm` and 751 rows.
Pure. Tests citing it: `sa::tg::tests` (reference file). See also `bench_mcp_sa_parse_reference`, `bench_mcp_sa_write_csv`.

### `bench_mcp_sa_parse_reference` [@ANCHOR: bench_mcp_sa_parse_reference]

`pub fn parse_reference(text: &str) -> Result<Reference, String>` in `src/sa/tg.rs`: reads a reference CSV written by `bench_sa_tg_normalize` (the `reference_file` argument of `bench_sa_tg_s21`). `text` is the
borrowed file contents. Lines starting `# ` containing `=` give the settings (keys of `comments`); a line starting `freq_hz` (the header) and blank lines are skipped; every other line is a data row whose second
comma separated field is the level.

Errors: a data row whose second field is missing or not a finite number ("bad reference row ..."); a missing or non-finite numeric setting ("the reference file has no usable `# KEY=` line"); no `# detector=` line;
a point count other than `POINTS` (751) ("has N points, not 751"). The frequency column of the rows is ignored (the frequencies come from `start_hz` and `stop_hz`). `ref_level_dbm` and `sweep_time_s` are set to 0.0.
`average_count` is cast with `as u32`. Pure (the caller reads the file). Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_reference_comments`, `bench_mcp_sa_s21`.

### `bench_mcp_sa_check_same` [@ANCHOR: bench_mcp_sa_check_same]

`pub fn check_same(r: &Reference, t: &Trace, tg_level_dbm: f64) -> Result<(), String>` in `src/sa/tg.rs`: refuses a measurement whose settings differ from those of the through reference, because a ratio of traces taken
with different settings is meaningless. Compares `start_hz`, `stop_hz`, `rbw_hz`, `vbw_hz`, `attenuation_db`, `average_count` and the TG level, numbers equal when within a relative 1e-6 (floor 1 as the scale), and the detector
name without regard to case. Returns `Ok(())` when all agree. Otherwise `Err` naming the FIRST differing field in that order, the reference's value and the sweep's, and "Next: repeat bench_sa_tg_normalize with the settings you
want, or leave those arguments out to use the reference's" (the detector message is similar). The reference level and sweep time are deliberately not compared. Pure. Called by `s21` after the sweep. Tests citing it:
`sa::tg::tests`. See also `bench_mcp_sa_with_defaults`.

### `bench_mcp_sa_with_defaults` [@ANCHOR: bench_mcp_sa_with_defaults]

`fn with_defaults(args: &Value, r: &Reference) -> Value` (private) in `src/sa/tg.rs`: returns a copy of the S21 arguments in which every setting the caller left out (absent or `null`) is filled from the reference, so
`bench_sa_tg_s21 {"reference":"default"}` repeats exactly the through's sweep. Filled: `start_hz` and `stop_hz` (only when none of `start_hz`, `stop_hz`, `center_hz`, `span_hz` is given, so a centre/span request is not
mixed with a start/stop), `rbw_hz`, `vbw_hz`, `attenuation_db`, `detector` (mapped back from the unit's name by its first three letters, anything else to `normal`), `average_count` and `tg_level_dbm`. A value the caller gave is kept,
and `check_same` then rejects it if it differs. Pure; the input is borrowed and not changed. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_check_same`, `bench_mcp_sa_s21`.

### `bench_mcp_sa_tg_sweep` [@ANCHOR: bench_mcp_sa_tg_sweep]

`fn tg_sweep(l, inst, req, level, leave_on) -> Result<Trace, String>` (private) in `src/sa/tg.rs`: TG on at `level`, one sweep, TG off again unless `leave_on`. `l` is the open, identity-checked link; `inst` supplies the cap;
`req` is the checked `SweepReq`; `level` is the checked level in dBm; `leave_on` keeps the output on afterwards. Order: `switch_on` (which already switches off on its own failure); if that worked `run_sweep`; then, unless `leave_on`,
`ensure_tg_off`, whatever happened before; the three results are merged by `finish`. So a failed sweep still turns the generator off, and a sweep whose switch-off could not be confirmed is an error, not data. Called by
`normalize` and `s21`. With `leave_on = true` and a failed sweep after a good turn-on, the generator stays on. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_switch_on`, `bench_mcp_sa_finish`.

### `bench_mcp_sa_normalize` [@ANCHOR: bench_mcp_sa_normalize]

`pub fn normalize(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String>` in `src/sa/tg.rs`: the tool `bench_sa_tg_normalize` (tier `read_config`). The operator cables the TG OUTPUT to the RF
INPUT; this sweeps that through with the TG on and stores it as the 0 dB reference.

| Argument | Type, unit | Meaning |
|---|---|---|
| `name` | string | Reference name, `ref_name` rules, default `default`. |
| `tg_level_dbm` | number, dBm | -20 to `max_tg_dbm`; default -20 (the rated minimum, which keeps the analyzer input safe). |
| `leave_tg_on` | bool, default false | Leave the output on afterwards. |
| sweep arguments | see `sweep_args` | Range required (start/stop or centre/span); others optional. |

Order: validate the name, level and flag, open (identity), `sweep_args`, `require_tg`, `tg_sweep`, write `sa_tg_reference_<name>_*.csv` (with the `comments` header), store the `Reference` under `<inst>:<name>`.
Returns the `trace_summary` (no peaks) plus `name`, `tg_level_dbm`, `file` and `tracking_generator` (`"on"` or `"off (verified)"`). Errors: any of those steps; a failed switch-off wins over the data. The reference is stored only after the
CSV was written. Replaces an earlier reference of the same name for that instrument. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_reference`, `bench_mcp_sa_s21`, `bench_mcp_sa_tg_sweep`.

### `bench_mcp_sa_s21` [@ANCHOR: bench_mcp_sa_s21]

`pub fn s21(connect: &Connector, inst: &Instrument, args: &Value) -> Result<Value, String>` in `src/sa/tg.rs`: the tool `bench_sa_tg_s21` (tier `read_config`): a scalar S21 sweep between the TG OUTPUT and the RF INPUT with the device under test
in between, relative to a through reference.

| Argument | Meaning |
|---|---|
| `reference` | Name of a reference stored by `bench_sa_tg_normalize` in this server run (key `<inst>:<name>`). |
| `reference_file` | Path of a reference CSV (read with `parse_reference`). Exclusive with `reference`. |
| `tg_level_dbm`, `leave_tg_on`, sweep arguments | As for `normalize`; with a reference, missing settings come from it (`with_defaults`). |

Steps: resolve the reference (an unknown name: "no reference named ... run bench_sa_tg_normalize first, or pass reference_file"; both given is an error; a file that cannot be read or parsed is an error), fill defaults, level
(default -20 dBm), open, `sweep_args`, `require_tg`, `tg_sweep`, then `check_same` when there is a reference, and the S21: measured minus reference per point in dB, or, with no reference, measured minus the TG level (uncalibrated: cable and
generator flatness included, said so in the result). Writes `sa_tg_s21_*.csv` (`freq_hz,s21_db,measured_dbm`). Returns the `trace_summary` plus `s21` (`relative_to`, `max_db`, `max_at_hz`, `min_db`, `min_at_hz`, `mean_db`), `tg_level_dbm`,
`file`, `tracking_generator`. The generator is on only during the sweep unless `leave_tg_on`; a failed switch-off is the error. Mismatched settings are an error only after the sweep, and the generator has been switched off by then. Example:
`{"reference":"default","tg_level_dbm":-20}`. Tests citing it: `sa::tg::tests`. See also `bench_mcp_sa_normalize`, `bench_mcp_sa_check_same`, `bench_mcp_sa_with_defaults`.
