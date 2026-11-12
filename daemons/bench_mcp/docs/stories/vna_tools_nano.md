# The Extra VNA Tools and the NanoVNA Driver: Generator, Meter, Spectrum, Calibration Files and a Text-Shell Instrument

As the person (or the AI agent) running the bench, I want `bench_mcp` to drive a vector network analyser beyond a plain sweep: switch the VNWA 3E's CW generator
(with AM and FM) on and off, read its frequency meter and spectrum analyzer, measure calibration standards and build and apply an open/short/load/thru
calibration, and I want a NanoVNA-H to answer the same `bench_vna_*` sweep and calibration tools through its USB text shell. This story documents every anchor
of two source files: `daemons/bench_mcp/src/vna_tools.rs` (the VNWA's separate tools and the instrument-independent calibration file handling) and
`daemons/bench_mcp/src/nanovna.rs` (the `nanovna` driver). The plain sweep and the VNWA's power cycle are in `src/vna.rs` and are not covered here.

Facts that apply to every entry. The server (`main.rs`) finds the instrument, checks its tier (every `bench_vna_*` tool in these files is declared at tier
`read_config`), holds the per-instrument lock (`bench_mcp_main_instrument_lock`) for the whole call and runs the call on a blocking thread, so two calls to one
VNA never overlap and every function below may block. Files go to the output directory (`bench_mcp_main_out_dir`, the `BENCH_MCP_OUT` environment value or its
default). Nothing here is a licensed transmitter: the VNWA's generator, its sweep and its spectrum tracking generator put a low-level signal on the unit's TX
port (about -17 dBm at 0 dB attenuation, per the tool text), and the NanoVNA sources its own stimulus. None of the functions below applies a band check or a
transmit gate of its own; the only control they pass through is the `read_config` tier checked by the server. Errors are `Err(String)` (the VNWA tools and
`vna_tools.rs` helpers) or the typed `Error` of `nanovna.rs`, which converts to a `String` through `Display`. Argument-parsing functions run before the unit is
opened wherever the code says so, so a bad request never touches the hardware.

## VNWA tools: argument helpers (`vna_tools.rs`)

### `num` [@ANCHOR: bench_vna_tools_num]

Reads one optional numeric argument from a tool call's JSON arguments, so every tool in this file reports a wrong type the same way. Private helper; called by
`level_db`, `freq_hz`, `mod_args`, `meter` and `spectrum`.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool's JSON object. |
| `key` | `&str`, borrowed | The argument name to look up. |

Returns `Ok(None)` when the key is absent or JSON `null`, `Ok(Some(x))` for a finite number (integers and floats alike, as `f64`). Error: `Err("<key> must be a
number")` for a string, boolean, array or object, and for a number that is not finite (JSON cannot express NaN or infinity, so this is a guard only).
No side effects, no locks, idempotent. See also: `bench_vna_tools_level_db`, `bench_vna_tools_freq_hz`; the README calls this group "the VNA tools parse their
numeric, level and modulation arguments"; tests cite it in the `vna_tools` test module.

### `level_db` [@ANCHOR: bench_vna_tools_level_db]

Reads and validates the required `level_db` argument of the generator tools: the generator output as a negative attenuation in dB, 0 being the unit's full
output. Private helper called by `generator_on` and `generator_level`.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool arguments; `level_db` must be present. |

Returns `Ok(level_db)` in dB, between -50 and 0 inclusive. Errors: `level_db is required (0 to -50: a negative attenuation)` when absent or null; `level_db must
be a number` for a wrong type; the text of `vnwa_open::generator::level_for_attenuation` (`generator attenuation <x> dB is outside -50 to 0 dB`) outside the
range. That function also maps the dB value to the unit's 14-bit amplitude word, `round(16383 x 10^(dB/20))`; this helper only uses it as the range check and
returns the dB value. No hardware is touched. See also: `bench_vna_tools_generator_on`, `bench_vna_tools_generator_level`, `vnwa_open_generator_level`.

### `freq_hz` [@ANCHOR: bench_vna_tools_freq_hz]

Reads and validates the required `freq_hz` argument (the generator, the meter and the AM/FM tools): a frequency in hertz greater than 0 and no more than 1.3e9.
Private helper.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool arguments; `freq_hz` must be present. |

Returns `Ok(f)` in Hz. Errors: `freq_hz is required`; `freq_hz must be a number`; `freq_hz <f> is outside 0 to 1.3e9` (zero, negative or above 1.3 GHz). The
limit is the VNWA's receive range and is wider than what the generator can make: the generator's own limit (the 360 MHz DDS clock, with the image above 180 MHz)
is enforced later by `vnwa_open`, so a `freq_hz` that passes here can still be refused by `bench_vna_generator_on`. No hardware is touched. See also:
`bench_vna_tools_generator_on`, `bench_vna_tools_meter`, `bench_vna_tools_mod_args`.

### `mod_args` [@ANCHOR: bench_vna_tools_mod_args]

Reads and validates the arguments common to AM and FM: the carrier, the carrier level and the modulating frequency. Private helper for `generator_am` and
`generator_fm`.

| Argument (key) | Type | Default | Valid range |
|---|---|---|---|
| `freq_hz` | number, Hz | required | greater than 0, at most 1.3e9 (see `freq_hz`) |
| `level_db` | number, dB | -20 | -50 to 0 inclusive (note: optional here, required for plain CW) |
| `mod_hz` | number, Hz | 500 | `MIN_MOD_HZ` 1 to `MAX_MOD_HZ` 3000 of `vnwa_open::generator` |

Returns `Ok((freq_hz, level_db, mod_hz))`. Errors: those of `freq_hz`; `level_db <x> is outside -50 to 0`; `mod_hz <x> is outside 1 to 3000`; `<key> must be a number`.
No hardware is touched. The depth (AM) and the deviation (FM) are checked by the callers through `vnwa_open`.

## VNWA tools: generator (`vna_tools.rs`)

### `generator_on` [@ANCHOR: bench_vna_tools_generator_on]

The tool `bench_vna_generator_on`: switches the VNWA's CW generator on at one frequency and level and leaves it on until `bench_vna_generator_off`. It is the
only tool that leaves RF on the TX port after it returns. Called by `main.rs` dispatch for a `vnwa_open` instrument.

| Argument (key) | Type | Meaning |
|---|---|---|
| `freq_hz` | number, Hz, required | Carrier, greater than 0 and at most 1.3e9 here; `vnwa_open` refuses what the DDS cannot represent (not below 360 MHz). Above 180 MHz the output is the DDS image, not measured on the unit (per the tool text). |
| `level_db` | number, dB, required | -50 to 0; 0 dB is about -17 dBm; verified on a scope from 0 to -40 dB (per the result text). |

Returns JSON `{"generator":"on","freq_hz","level_db","note"}`. Errors: the argument errors of `freq_hz` and `level_db` (raised before the USB device is opened);
`Vnwa::open()` failures (unit absent or busy, the text comes from `vnwa_open`); errors from `vnwa_open::generator::on`. Blocking USB control transfers; takes
the instrument lock through the server; the generator state lives in the unit, not in this process. Do not run a sweep or the meter while it is on (tool
text). Not idempotent in effect only in that it re-programs the unit; repeating the same call is safe. Transmit safety: tier `read_config`, no band check, no
separate transmit gate; the output is a bench signal of about -17 dBm at most. See also: `bench_vna_tools_generator_level`, `bench_vna_modulation_stop`.

### `generator_off` (anchor `bench_vna_modulation_stop`) [@ANCHOR: bench_vna_modulation_stop]

The tool `bench_vna_generator_off`, and the recovery that ends a stuck AM or FM modulation. The anchor sits inside the function, on the `match` that decides what a
failed switch-off means; the doc comment above the function states the policy. Note: `generator_off` itself has no anchor of its own; this entry covers the whole
function.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&vnwa_open::sweep::Config`, borrowed | The instrument's VNA settings (called from `main.rs` as `&inst.vna`); only `cfg.hub` (the optional power-switching hub and port) is read. |

Behaviour: opens the unit and asks `vnwa_open::generator::off`. If that succeeds, returns `{"generator":"off"}`. If it fails and the unit no longer answers
(`generator::answers` is false, the signature of a unit that is modulating: its microcontroller then refuses the request), the USB handle is dropped and
`generator::end_modulation()` does a USB reset of the control chip, with no power change (measured on the unit, 2026-10-29); success returns
`ended_modulation_by_usb_reset: true`. If the reset fails and a `hub` is configured, `vnwa_open::power::cold_reset_after_modulation(&hub)` power cycles the unit
through the hub (the longer ladder meant for modulation lock-ups), the unit is reopened and must answer, and the result is `recovered_by_power_cycle: true`
(the unit then lost all its settings). If a failure leaves the unit answering, the original error is returned unchanged.

Errors: `Vnwa::open()` failure; the original `off` error when the unit still answers; with no hub, a message that the unit refuses the switch-off and a USB reset
did not help, telling the operator to power the USB port off and on and to set `hub` and `hub_port` in the instrument's bench-config entry; a hub power-cycle
error; `the unit was power cycled but still does not answer`. Blocking, takes about 3 s for the USB reset (per the tool text) and longer for a power cycle;
the power cycle disturbs everything on that hub port, so the hub lock applies as for any hub-power action. Idempotent: calling it when the generator is already
off succeeds. This function only ever reduces RF output. See also: `bench_vna_tools_generator_am`, `bench_vna_tools_generator_fm`.

### `generator_level` [@ANCHOR: bench_vna_tools_generator_level]

The tool `bench_vna_generator_level`: changes the output level of a generator that is already on, without changing its frequency.

| Argument (key) | Type | Meaning |
|---|---|---|
| `level_db` | number, dB, required | -50 to 0, as in `level_db`. |

Returns `{"generator":"level set","level_db"}`. Errors: argument errors from `level_db` (before the unit is opened); `Vnwa::open()` failure; failure of
`vnwa_open::generator::set_attenuation`. Blocking USB transfer; idempotent. Whether it does anything useful while the generator is off or while modulating is
unknown from this code (the call is sent either way). Transmit safety as for `generator_on`: tier `read_config`, no band check or gate in this function.

### `generator_am` [@ANCHOR: bench_vna_tools_generator_am]

The tool `bench_vna_generator_am`: starts the generator and amplitude-modulates it. The unit's microcontroller then modulates in real time, refuses the
switch-off request and must be USB reset; `bench_vna_generator_off` does that. Nothing else may use the unit while it modulates.

| Argument (key) | Type | Default | Meaning |
|---|---|---|---|
| `freq_hz` | number, Hz | required | Carrier (see `mod_args`). |
| `level_db` | number, dB | -20 | Carrier level, -50 to 0. |
| `mod_hz` | number, Hz | 500 | Modulating frequency, 1 to 3000. |
| `depth_pct` | number, percent | 100 | Modulation depth; the accepted range is decided by `vnwa_open::generator::am_fields`, unknown here. |

Returns JSON with `generator: "AM on"` and the parameters, a `verified` text (depth within 3 percentage points and frequency within 5 Hz on a scope at 10 MHz,
500 to 3000 Hz, 10 to 100 %) and a `warning`. Errors: the argument errors of `mod_args`; `am_fields` rejection (done before the unit is touched); USB open, `on`
and `modulate` failures. Blocking; leaves the unit modulating on success. Transmit safety as for `generator_on`: tier `read_config`, no band check, no gate.
See also: `bench_vna_modulation_stop`, `bench_vna_tools_generator_fm`.

### `generator_fm` [@ANCHOR: bench_vna_tools_generator_fm]

The tool `bench_vna_generator_fm`: starts the generator and frequency-modulates it, with the same stuck-until-reset behaviour as AM.

| Argument (key) | Type | Default | Meaning |
|---|---|---|---|
| `freq_hz`, `level_db`, `mod_hz` | as for `mod_args` | | Carrier, level (default -20 dB) and modulating frequency (default 500 Hz, 1 to 3000). |
| `deviation_hz` | number, Hz | required | Peak deviation; validated by `vnwa_open::generator::fm_fields` (the largest accepted is `MAX_FM_DEVIATION_HZ`, 300 kHz in `vnwa_open`). |

Returns JSON with `generator: "FM on"`, the parameters, a `verified` text (deviation within 3 % on a scope, at 1 and 2 kHz modulation and 10 and 20 kHz
deviation) and a `warning`. Errors: `mod_args` errors; `deviation_hz is required`; `fm_fields` rejection (before the unit is touched); USB open, `on` and
`modulate` failures. Blocking; leaves the unit modulating. Transmit safety as for `generator_on`. See also: `bench_vna_modulation_stop`.

## VNWA tools: meter and spectrum analyzer (`vna_tools.rs`)

### `meter` [@ANCHOR: bench_vna_tools_meter]

The tool `bench_vna_meter`: measures how far the signal at the receive port is from a chosen centre frequency, using the unit as a frequency meter. It
does not generate a signal; the unit's TX DDS is parked at the centre for the measurement.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config`, borrowed | The instrument's VNA settings; the call opens the unit with these but forces `if_hz` to 4000. |
| `freq_hz` | number, Hz, required | Centre frequency (greater than 0, at most 1.3e9). |
| `point_ms` | number, ms, default 60 | Measuring time, 0.2 to 100; otherwise `point_ms must be 0.2 to 100`. |

Returns JSON: `center_hz`, `tx_dds_hz`, the offset of the RX signal from the centre (`offset_hz`, with the per-codec offsets `offset_codec1_hz` and
`offset_codec2_hz`), `stderr_hz`, `frequency_hz` (centre plus offset), `measurement_s`, a `recovery` object (re-measures, transfer resends, power cycles,
audio overrun counts) and a `note`. The note is explicit that the offset is valid within +/-25 Hz, that a TX-to-RX loopback reads 0, and that the offset sign
(higher RX frequency is positive) and the absolute accuracy against a standard are NOT verified (`sign_verified` and `absolute_accuracy_verified` are `false`
in the result); the reference is the unit's 12 MHz TCXO, about 0.3 ppm per the vendor. `measurement_s` is the library's seconds multiplied by 31/28; the reason for
that factor is unknown from this code. Errors: argument errors; `Vna::open` and `meter` failures from `vnwa_open`. Blocking for about the measuring time; do not
run it while the generator is on (tool text). The recovery counters show what the library did to ride through USB and audio glitches. Transmit safety: tier
`read_config`; no band check or gate in this function.

### `spectrum` [@ANCHOR: bench_vna_spectrum]

The tool `bench_vna_spectrum`: a spectrum analyzer built from the VNWA's receiver. At each of N frequencies it measures the audio power within a band around
the unit's local oscillator, in uncalibrated codec counts, and writes the points to a CSV.

| Argument (key) | Type | Default | Meaning |
|---|---|---|---|
| `cfg` | `&Config` | | Instrument settings; the call forces `if_hz` to `vnwa_open::spectrum::SA_IF_HZ` (12 kHz) and `point_ms` to the argument. |
| `start_hz`, `stop_hz` | number, Hz | required | The range; `vnwa_open::spectrum::spectrum_plan` requires 12001 Hz to 1.3 GHz, `stop` not below `start`. |
| `points` | whole number | required | 2 to 2000, else `points <x> must be a whole number from 2 to 2000`. |
| `band_hz` | number, Hz | 5000 | Audio band measured at each point; `spectrum_plan` accepts 200 to 20000 Hz. |
| `point_ms` | number, ms | 20 | 5 to 100, else `point_ms must be 5 to 100`. |
| `tracking_db` | number, dB | off | If given (0 to -50) the tracking generator is on at that level: the TX runs 12 kHz below the LO, so a loopback appears as a line at 12 kHz (use `band_hz` above 12000). |
| `out_dir` | `&Path`, borrowed | | Where the CSV goes (passed by the server). |

Returns JSON: `points`, the range, `band_hz`, `point_ms`, `tracking_generator` (bool), `noise_floor_counts` (the median of the point values), `peaks` (up to 10
local maxima above 3 times the floor, strongest first, each with `freq_hz`, `rms_counts` and `db_over_floor`), `rows` (every point when there are 51 or fewer,
otherwise empty), `file` (`vna_spectrum_<unix seconds>.csv` with columns `freq_hz,rms_counts,db_rel_floor`), a `recovery` object and a `note`. The note says the
result is UNCALIBRATED, that a point is two-sided (audio is real, so LO+d and LO-d fall together), that ghost lines and subharmonic responses are not
characterised, that the vendor says to use it below 100 MHz, that a known 15 kHz-band tone read 8771 counts rms to 0.03 % and that the noise floor is about 2 counts.
Errors: argument errors, `spectrum_plan` errors (before the unit is opened), `Vna::open` and `spectrum` failures, and a file write failure
(`<path>: <io error>`). Blocking for about `points x point_ms` plus overhead. Writes one file. Transmit safety: tier `read_config`; the optional tracking
generator is a low-level bench signal and no band check or gate applies in this function.

## VNWA tools and shared: calibration files (`vna_tools.rs`)

### `name_ok` [@ANCHOR: bench_vna_tools_name_ok]

Validates a calibration session name, because the name becomes part of a file name (`cal_<session>_<standard>.csv`, `cal_<session>.cal`) and must not contain a
path separator. Private helper.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&str`, borrowed | The session name. |

Returns `Ok(())` for 1 to 40 characters, each an ASCII letter, digit, `-` or `_`; otherwise `Err("session must be 1 to 40 letters, digits, - or _")`. Pure; no
side effects. See also: `bench_vna_tools_measure_names`, `bench_mcp_vna_cal_build`.

### `parse_meta` [@ANCHOR: bench_vna_tools_parse_meta]

Reads the settings line at the top of a stored calibration standard file back into a `vnwa_open::cal::Setup`. Private helper of `read_standard`.

| Argument | Type | Meaning |
|---|---|---|
| `line` | `&str`, borrowed | The first line of a standard file, written by `meta_setup` as `# if_hz=<n>;point_ms=<n>;rf_level=<n>;cable=<text>`. |

The `Setup` fields are `if_hz` (Hz), `point_ms` (ms per point), `rf_level` (integer level word) and `cable` (free text naming what is between the ports and the
reference plane; newlines are replaced by spaces when written). Returns `Ok(Setup)`; keys not present keep `Setup::default()`. The line is split into at most four
parts on `;`, so a cable text may itself contain `;` but must come last. Errors: `missing settings line` (no `# ` prefix), `bad settings line` (a part without
`=`), `bad if_hz`, `bad point_ms`, `bad rf_level`, `unknown setting <key>`. Pure. See also: `bench_vna_tools_store_standard`, `bench_vna_tools_read_standard`.

### `measure_names` [@ANCHOR: bench_vna_tools_measure_names]

Checks the three text arguments of a calibration-measure call, for both the VNWA and the NanoVNA. Public; called by `calibrate_measure` here and by the
NanoVNA's `calibrate_measure`.

| Argument (key) | Type | Meaning |
|---|---|---|
| `session` | string, required | A name from `name_ok`; ties together the standards measured for one calibration. |
| `standard` | string, required | `open`, `short`, `load` or `thru`. |
| `cable` | string, required | Free text saying what is between the ports and the reference plane; it is stored and compared when the calibration is applied. |

Returns `Ok((session, standard, cable))` as owned strings. Errors: `session is required` (or the `name_ok` message), `standard is required`, `standard must be
open, short, load or thru`, `cable is required: say what is between the ports and the reference plane (it is stored and compared)`. Pure. See also:
`bench_vna_tools_store_standard`.

### `store_standard` [@ANCHOR: bench_vna_tools_store_standard]

Writes the measured points of one calibration standard to a file for `calibrate_build`, with a settings line so a mismatch is caught later. Instrument
independent. Public.

| Argument | Type | Meaning |
|---|---|---|
| `setup` | `&Setup`, borrowed | The settings the points were measured at (IF, ms per point, RF level, cable). |
| `pts` | `&[Point]`, borrowed | The measured points; at least 2. |
| `out` | `&Path`, borrowed | Output directory. |
| `session`, `standard` | `&str` | Already checked by `measure_names`. |

File format (`<out>/cal_<session>_<standard>.csv`, written whole, overwriting any earlier one): line 1 the `# if_hz=...` settings line, then one row per point
`freq_hz,s11_re,s11_im,s21_re,s21_im` in Rust `{:e}` exponent notation. Returns `{"session","standard","points","file","note"}`. Errors: `a calibration needs at least 2
points`; a write failure (`<path>: <io error>`). Writes one file; not atomic. See also: `bench_vna_tools_read_standard`, `bench_vna_tools_parse_meta`.

### `calibrate_measure` [@ANCHOR: bench_vna_tools_calibrate_measure]

The tool `bench_vna_calibrate_measure` for the VNWA: sweeps once and stores the points as one calibration standard.

| Argument (key) | Type | Meaning |
|---|---|---|
| `session`, `standard`, `cable` | strings | Checked by `measure_names`. |
| `start_hz`, `stop_hz`, `points`, `if_hz`, `point_ms`, `rf_level` | as for `bench_vna_sweep` | Parsed by `vna::sweep_args` over `cfg`; `points` must be at least 2 here. |
| `cfg`, `out` | borrowed | Instrument settings and output directory. |

Returns what `store_standard` returns. Errors: name errors, sweep-argument errors, `a calibration needs at least 2 points`, `Vna::open` and sweep failures,
write failure. Blocking for the sweep duration; the VNWA's own sweep emits its low-level stimulus on the TX port (tier `read_config`, no band check or gate
here). Open, short, load (and thru for S21) must be measured with the same start, stop, points, settings and cable. See also:
`bench_nanovna_calibrate_measure` (the NanoVNA version), `bench_mcp_vna_cal_build`.

### `read_standard` [@ANCHOR: bench_vna_tools_read_standard]

Reads a standard file written by `store_standard`. Private helper of `calibrate_build`.

| Argument | Type | Meaning |
|---|---|---|
| `file` | `&Path`, borrowed | The `cal_<session>_<standard>.csv` file. |

Returns `Ok((Setup, freqs, s11, s21))`: the settings from line 1 (see `parse_meta`), the frequencies in Hz, and the S11 and S21 values as `Complex { re, im }`.
Errors: read failure (`<path>: <io error>`); `empty file`; `parse_meta` errors; `bad number in "<line>"` and `bad row "<line>"` (a row without exactly 5 comma
separated numbers; an empty line in the body is such an error). Pure apart from reading the file.

### `calibrate_build` [@ANCHOR: bench_mcp_vna_cal_build]

The tool `bench_vna_calibrate_build`: combines the standards measured for one session into a calibration file. It touches no instrument, so it works with the
unit absent. Used with both the VNWA and the NanoVNA.

| Argument (key) | Type | Meaning |
|---|---|---|
| `session` | string, required | Session name (`name_ok`). |
| `kit_file` | string, optional | Path of a calibration-kit definition text; absent means `KitDefinition::default()`. |
| `created` | string, optional | A label stored in the file; default `unix <seconds>`. |
| `out` | `&Path`, borrowed | Directory holding the standard files and receiving the result. |

Looks for `cal_<session>_{open,short,load,thru}.csv` in `out`; at least one must exist (`no standards measured for this session`). All present standards must carry
the same settings and frequencies as the first found, else `<name> was measured at other settings, cable or frequencies than the others`. It calls
`Calibration::build` with the S11 of open, short and load (one-port) and the S21 of thru; a standard left out simply leaves that part uncalibrated. Writes
`cal_<session>.cal` (text from `Calibration::to_text`, format owned by `vnwa_open`). Returns `file`, `start_hz`, `stop_hz`, `points`, `s11_calibrated`,
`s21_calibrated` (true when the corresponding part could be built), the settings and cable, and a note that a calibration is applied with `bench_vna_sweep
calibration=<file>` and the same cable text and is refused at another IF, time per point, RF level or cable. Other errors: session and kit read or parse errors,
read errors, `Calibration::build` errors, write failure. Writes one file. See also: `bench_mcp_vna_cal_apply`, `bench_vna_tools_read_standard`.

### `apply_calibration` [@ANCHOR: bench_mcp_vna_cal_apply]

Corrects sweep points with a calibration file for the VNWA, building the `Setup` from the instrument's current configuration. Public; called by the VNWA
sweep (`vna.rs`) when a `calibration` argument is given.

| Argument | Type | Meaning |
|---|---|---|
| `file` | `&str`, borrowed | Path of a `.cal` file from `calibrate_build`. |
| `cfg` | `&Config`, borrowed | Gives `if_hz`, `point_ms` and `rf_level` of this sweep. |
| `cable` | `&str`, borrowed | The cable text given when the standards were measured. |
| `points` | `&[Point]`, borrowed | The measured points. |

Returns the loaded `Calibration` and one `(Option<Complex>, Option<Complex>)` per point: the corrected S11 and S21, `None` for a part the calibration does not
cover (the `Corrected` type alias). Errors are those of `apply_calibration_setup`. See also: `bench_vna_tools_apply_setup`.

### `apply_calibration_setup` [@ANCHOR: bench_vna_tools_apply_setup]

`apply_calibration` for any instrument: loads the calibration file, refuses it unless the given `Setup` equals the one it was made at, and corrects each point
with the same open/short/load/thru arithmetic, which does not depend on the instrument. The NanoVNA's sweep uses it directly.

| Argument | Type | Meaning |
|---|---|---|
| `file` | `&str`, borrowed | Path of the `.cal` file. |
| `setup` | `&Setup`, borrowed | The settings of the sweep being corrected. |
| `points` | `&[Point]`, borrowed | The points to correct. |

Returns `Ok((Calibration, Vec<(Option<Complex>, Option<Complex>)>))`. Errors: `<file>: <io error>` on read failure; `Calibration::from_text` parse errors;
`Calibration::check_setup` mismatch text (it lists each of IF, time per point, RF level and cable that differs); `Calibration::correct` errors. Read only; no
instrument access; idempotent. Corrected values for a point are computed independently. See also: `bench_mcp_vna_cal_apply`, `bench_nanovna_sweep_tool`.

## NanoVNA driver: errors, settings and the shell protocol (`nanovna.rs`)

### `Error` Display [@ANCHOR: bench_nanovna_error_display]

The `Display` implementation of the driver's `Error` enum, giving each failure its own wording so a caller can tell them apart. The enum is `NotSupported`,
`NotPresent`, `Timeout`, `Protocol`, `UnknownCommand`, `Args` and `Io`, each holding a `String`; it derives `Debug` and `PartialEq`. A `From<Error> for String`
uses this wording.

| Variant | Meaning | Text produced |
|---|---|---|
| `NotSupported(s)` | The instrument cannot do it or this driver does not | `not supported by this instrument (nanovna): <s>` |
| `NotPresent(s)` | Serial port missing, closed or failing | `device not present: <s>` |
| `Timeout(s)` | No prompt within the time | `nanovna timeout: <s>` |
| `Protocol(s)` | Reply is not what the shell protocol says | `nanovna protocol error: <s>` |
| `UnknownCommand(s)` | The shell answered `<name> ?` | `nanovna firmware has no command "<s>"` |
| `Args(s)` | Bad tool argument | `<s>` unchanged |
| `Io(s)` | Other I/O failure, including a failure to restore the sweep range | `nanovna i/o error: <s>` |

Pure formatting. `NotPresent` deliberately shares the prefix `device not present` with the VNWA's check, so callers treat both alike.

### `Config` and `parse_config` [@ANCHOR: bench_nanovna_parse_config]

Reads the `nanovna` keys of one instrument entry of the bench config into a `Config` and refuses unusable values when the config loads. The result is kept in
`Instrument.nanovna` and not changed afterwards. Called through the `module_config!` macro in `config.rs` when an entry's `driver` is `nanovna`.

| Field (config key) | Type | Default | Meaning and check |
|---|---|---|---|
| `serial` | `Option<String>` | none, but required | The serial device; use the `/dev/serial/by-id/...` name. A missing key is an error. |
| `min_hz` | `Option<f64>` | board default (50 kHz) | Overrides the lowest sweep frequency; finite and greater than 0. |
| `max_hz` | `Option<f64>` | 900 MHz (1.5 GHz on an H4) | Overrides the highest sweep frequency; finite and greater than 0. |
| `max_points` | `Option<usize>` | 101 (401 on an H4) | Overrides the point limit; a whole number, greater than 0 and at most 100000. |
| `settle_ms` | `f64` | 12.0 | Wait per point after the sweep is set, before the data is read (the unit sweeps continuously); 0 to 1000. |
| `use_scan` | `bool` | true | Use `scan` when the firmware lists it; false always uses `sweep` and `data`. |

Argument `c`: `&Value`, borrowed. Errors (all `Err(String)`): `a nanovna needs "serial" (the /dev/serial/by-id path)`; `<key> must be a positive number`;
`max_points must be a whole number`; `settle_ms must be 0 to 1000`; `use_scan must be true or false`. `Config` derives `Clone`, `Debug` and `PartialEq`; it is
plain data, not shared mutably. Whether the serial path exists is not checked here (see `open_shell`). Pure.

### `parse_reply` [@ANCHOR: bench_nanovna_parse_reply]

Turns the raw bytes of one shell reply into text lines, removing the echo of the command and the prompt, and classifying the two failure shapes. Called by
`Shell::command_within`.

| Argument | Type | Meaning |
|---|---|---|
| `cmd` | `&str`, borrowed | The command that was sent (used to recognise the echo and the `name ?` answer). |
| `raw` | `&[u8]`, borrowed | Everything read, normally ending in the prompt `ch> ` (`PROMPT`). |

Steps: strip a trailing `PROMPT`; any byte other than CR, LF, TAB or printable ASCII (0x20 to 0x7e) is an error; split on `\n`, trim `\r`, drop blank lines at both
ends; drop the first line if it equals the command (the echo); if the first remaining line is `<first word of cmd> ?` the firmware lacks the command. Returns
`Ok(lines)` (possibly empty). Errors: `Error::Protocol("non-text byte 0x.. in the reply to <cmd>")`; `Error::UnknownCommand(<first word>)`. Pure.

### `Shell::command_within` [@ANCHOR: bench_nanovna_command]

Sends one command to the NanoVNA shell and returns the reply lines. It is the only place that reads or writes the serial port. `Shell<P: Port>` holds the port
and a public `timeout` (default 3 s); `Shell::new(port)` builds it; `Shell::command(cmd)` is `command_within` with that default.

| Argument | Type | Meaning |
|---|---|---|
| `cmd` | `&str` | The command text without line end; the shell is sent `cmd` followed by CR. |
| `timeout` | `Duration` | How long to wait for the prompt `ch> `; large for `scan` (see `measure`). |

Steps: set the port timeout to 1 ms and read and discard anything left from an earlier timeout or garbage; write the command (a write that would block is retried
every 5 ms for up to 3 s; a write that never drains is `Timeout`); then read in pieces of up to 50 ms until the bytes end with the prompt or the deadline passes; pass
the bytes to `parse_reply`. Returns the lines. Errors: `NotPresent` (port closed, zero-byte write, I/O error), `Timeout` (`the port did not take ...` or `no prompt after
<cmd> within <n> ms (received <n> bytes: <first 60 chars>)`), and those of `parse_reply`. Blocking and synchronous on the calling thread; replies may arrive in any
number of pieces. After a `Timeout` the next call flushes the late bytes first. Not retried here.

### `Identity::limits` [@ANCHOR: bench_nanovna_limits]

Gives the range a unit can sweep. Method of `Identity`, which holds what `version`, `info` and `help` said: `version: String`, `board: Option<String>` (from the
`Board:` line of `info`), `info: Vec<String>` (all lines) and `commands: Vec<String>` (the words `help` lists; `Identity::has(cmd)` tests membership).
`Limits` is `{ min_hz: f64, max_hz: f64, max_points: usize }`.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `&Identity` | The identified unit. |
| `cfg` | `&Config` | Its `min_hz`, `max_hz` and `max_points` override the defaults. |

Defaults: `min_hz` 50 kHz; if the board name contains `H4`, `max_hz` 1.5 GHz and 401 points, otherwise 900 MHz and 101 points. The code comment cites public
documentation; the H4 top frequency of 1.5 GHz is the value in the code and has not been checked against the unit here (the README says the NanoVNA is not
verified on hardware). Returns `Limits`. Pure; cannot fail.

### `identify` [@ANCHOR: bench_nanovna_identify]

Asks the unit what it is: sends `version`, `info` and `help` and builds an `Identity`. Called at the start of each tool call.

| Argument | Type | Meaning |
|---|---|---|
| `sh` | `&mut Shell<P>` | The open shell (mutably borrowed for the conversation). |

Returns `Ok(Identity)`: `version` is the first non-blank line of the `version` reply trimmed; `board` the trimmed text after `Board:` in `info` if non-empty; `commands`
the words of `help`, with a `Commands:` prefix removed, split on spaces and commas. A firmware without `info` or `help` (answers `<cmd> ?`) yields empty lists and
is still identified by its version. Errors: `Protocol("version gave no text")`; any other `Error` from `command` (timeouts, port problems, garbage). Three round
trips, each up to 3 s. Read-only on the unit.

### `parse_sweep_query` [@ANCHOR: bench_nanovna_parse_sweep_query]

Reads the reply of `sweep` with no arguments, which prints the unit's current range as `start stop points`, so the range can be put back after a measurement.

| Argument | Type | Meaning |
|---|---|---|
| `lines` | `&[String]`, borrowed | The reply lines from `command("sweep")`; only the first is used. |

Returns `Ok((start_hz, stop_hz, points))` as `(f64, f64, usize)`; the point count is parsed as a float and cast, so a negative or fractional value saturates or
truncates without an error. Errors: `Protocol("sweep query should be 'start stop points', got ...")` when the first line does not have exactly 3 words;
`Protocol("<what>: <text> is not a number")` for a non-numeric or non-finite word. Pure.

### `parse_columns` [@ANCHOR: bench_nanovna_parse_columns]

Parses the numeric lines of a reply: each non-blank line must have exactly `per_line` whitespace-separated numbers. Used for `frequencies` (1 per line), `data 0`
and `data 1` (2: real and imaginary part) and `scan` (5: frequency, S11 re and im, S21 re and im).

| Argument | Type | Meaning |
|---|---|---|
| `lines` | `&[String]`, borrowed | The reply lines. |
| `per_line` | `usize` | Required numbers per line. |
| `what` | `&str` | Name for error messages (`scan`, `data 0`, ...). |

Returns `Ok(Vec<Vec<f64>>)`, one inner vector per non-blank line. Errors: `Protocol("<what>: expected <n> numbers per line, got <line>")`; `Protocol("<what>: <text>
is not a number")` (also for NaN and infinity). Pure.

### `check_range` [@ANCHOR: bench_nanovna_check_range]

Checks the sweep arguments of a call against the unit's limits before anything is sent.

| Argument | Type | Meaning |
|---|---|---|
| `start`, `stop` | `f64`, Hz | Both must lie within `lim.min_hz` to `lim.max_hz` inclusive, and `stop` must be above `start`. |
| `points` | `usize` | 2 to `lim.max_points`. |
| `lim` | `&Limits` | From `Identity::limits`. |

Returns `Ok(())`. Errors, all `Error::Args`: `start_hz <f> is outside this unit's <min> to <max> Hz` (same for `stop_hz`), `stop_hz must be above start_hz (the NanoVNA
sweeps a band)`, `points <n> must be a whole number from 2 to <max> (this unit's limit)`. A NaN frequency fails the range test. Pure.

## NanoVNA driver: measuring and tools (`nanovna.rs`)

### `measure` [@ANCHOR: bench_nanovna_measure]

Sweeps `start..stop` with `points` points and returns S11 and S21 as the unit reports them, with whatever calibration the unit itself has active. The unit's
calibration slots, flash and settings are never written; the sweep range it changes is held in RAM only and put back.

| Argument | Type | Meaning |
|---|---|---|
| `sh` | `&mut Shell<P>` | The shell. |
| `id` | `&Identity` | From `identify`; supplies the limits and whether `scan` is listed. |
| `cfg` | `&Config` | `use_scan`, `settle_ms` and limit overrides. |
| `start`, `stop` | `f64`, Hz | Rounded to whole hertz when sent. |
| `points` | `usize` | Number of points. |

Returns `Ok(Vec<Point>)` (`vnwa_open::sweep::Point`, with `if_error_hz` and `level_change` set to 0, which this driver does not measure). Order of work: `check_range`
first; if `use_scan` is true and `help` listed `scan`, send `scan <start> <stop> <n> 7` with a wait of 10 s plus 60 ms per point and parse with `scan_points`. If
the shell answers `name ?` or the reply is a protocol error, fall back to the plain sweep; any other error is returned. The plain sweep reads the current range
with `sweep`, runs `sweep_and_read`, then restores the range with `sweep <old>` and `resume` whatever happened (best effort, the first error is the one reported;
if only the restore fails the result is `Error::Io("measured, but restoring the sweep range failed: ...")`). Blocking, with long waits (hundreds of ms to
seconds). The scan path does not change the range. The mask `7` in the scan command is a constant in the code; its bit meaning is not documented there (unknown).
The NanoVNA's stimulus is its own and no transmit gate or band check applies.

### `scan_points` [@ANCHOR: bench_nanovna_scan_points]

Turns the reply of the `scan` command into points. Private helper of `measure`.

| Argument | Type | Meaning |
|---|---|---|
| `lines` | `&[String]` | Reply lines, each `freq s11_re s11_im s21_re s21_im`. |
| `n` | `usize` | The number of points asked for. |

Returns `Ok(Vec<Point>)` with `freq_hz`, `s11` and `s21`. Errors: the `parse_columns` errors with `what = "scan"`; `Protocol("scan returned <k> points, <n> asked")`.
Pure.

### `sweep_and_read` [@ANCHOR: bench_nanovna_sweep_and_read]

The fallback measurement for firmware without `scan`, or when `use_scan` is false: set the range, let the unit sweep, pause it and read the data. Private helper of
`measure`; does not restore the range (its caller does).

| Argument | Type | Meaning |
|---|---|---|
| `sh` | `&mut Shell<P>` | The shell. |
| `cfg` | `&Config` | `settle_ms`. |
| `(a, b, n)` | `(u64, u64, usize)` | Start Hz, stop Hz, point count. |

Sequence: `sweep a b n`; `resume`; sleep 300 ms plus `settle_ms x n` (so the unit completes at least one full sweep); `pause`; `frequencies`; `data 0` (S11); `data 1`
(S21). Returns `Ok(Vec<Point>)`, one per point, with frequencies from the unit. Errors: any `Error` from `command`; `Protocol("asked for <n> points, got <f>
frequencies, <a> S11 and <b> S21")` when a count differs. Blocks the thread for the settle sleep; leaves the unit paused after an error (the caller's restore runs
`resume`).

### `open_shell` [@ANCHOR: bench_nanovna_open_shell]

Opens the NanoVNA's serial port and wraps it in a `Shell`. Private; called once per tool call, so the port is open only during a call.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | Its `serial` path (an empty string if unset, which does not exist). |

Returns `Ok(Shell<Serial>)`. The port is opened at 115200 baud (USB CDC ignores the rate; the code calls this conventional). Errors: `NotPresent("<path> does not
exist (unplugged?)")`; `Io(<error>)` from `Serial::open`. Opens a device file; the port closes when the `Shell` is dropped. No locking of its own (the server's
instrument lock covers it).

### `call` [@ANCHOR: bench_nanovna_call]

The entry point for every `bench_vna_*` tool on a `nanovna` instrument, called by `main.rs` dispatch.

| Argument | Type | Meaning |
|---|---|---|
| `inst` | `&Instrument`, borrowed | The instrument entry: `inst.nanovna` settings, `inst.vna.hub` for the power cycle, `inst.name`. |
| `name` | `&str` | The tool name. |
| `args` | `&Value` | The tool's JSON arguments. |
| `out_dir` | `&Path` | Where files are written. |

Routing: `bench_identify` and `bench_status` open the shell, identify, and return the identity (`idn`, `version`, `board`, `info`, `commands`, `scan`, the limits, the hub
and a `calibrated` string saying it is unknown); `bench_vna_sweep` goes to `sweep_tool`; `bench_vna_calibrate_measure` to `calibrate_measure`; `bench_vna_calibrate_build`
to `vna_tools::calibrate_build` (the same code as the VNWA); `bench_vna_power_cycle` to `power_cycle`; the generator, AM, FM, meter and spectrum tools return
`Error::NotSupported` with the text that the NanoVNA shell has no controllable generator and the VNWA should be used. Any other name is `<name> does not apply to
<instrument name>`. Returns `Ok(Value)` or `Err(String)`. Blocking. Every path that touches the unit opens the port for that call only. Nothing here keys a transmitter
and no band check or gate applies.

### `sweep_args` [@ANCHOR: bench_nanovna_sweep_args]

Parses and checks the sweep arguments of a NanoVNA tool call.

| Argument (key) | Type | Meaning |
|---|---|---|
| `start_hz` | number, Hz, required | Start. |
| `stop_hz` | number, Hz | Stop; default is `start_hz` (see the note below). |
| `points` | whole number | Default is 101, or the unit's limit if lower; 0 and fractions are refused. |
| `if_hz`, `point_ms`, `rf_level` | any non-null value | The VNWA's settings; refused with `Error::NotSupported("<k> is a VNWA setting; the NanoVNA sets its own IF and level")`. |
| `id`, `cfg` | `&Identity`, `&Config` | For the limits. |

Returns `Ok((start, stop, points))`. Errors: `Error::Args` for `<k> must be a number`, `start_hz is required`, `points <p> must be a whole number`, and every `check_range`
message. Pure. Oddity worth knowing: because `stop_hz` defaults to `start_hz` and `check_range` requires `stop_hz` above `start_hz`, a call with no `stop_hz` is
always refused; a single-point NanoVNA sweep is therefore not possible through this function.

### `sweep_tool` [@ANCHOR: bench_nanovna_sweep_tool]

The tool `bench_vna_sweep` for a NanoVNA: sweeps, writes a CSV, and optionally corrects it with a stored calibration.

| Argument (key) | Type | Meaning |
|---|---|---|
| `start_hz`, `stop_hz`, `points` | | As in `sweep_args`. |
| `calibration` | string, optional | A `.cal` file from `bench_vna_calibrate_build`. |
| `cable` | string | Required when `calibration` is given: the text given when the standards were measured. |

Steps: open the shell, identify, `sweep_args`, `measure`, write `vna_sweep_<unix seconds>.csv` (via `vna::csv`). With `calibration`, apply it with
`vna_tools::apply_calibration_setup` using `Setup { cable, ..Default }` (the IF, time per point and RF level of the default `Setup`, which are the same defaults
that `calibrate_measure` stored) and write `vna_sweep_<stamp>_calibrated.csv`. Returns `points`, `start_hz`, `stop_hz`, `driver: "nanovna"`, `calibrated`, `calibrated_file`, `file`,
`rows` (S11 and S21 as dB and degrees for each point when 51 or fewer, else empty) and a `note`. Errors: all of the above steps, `cable is required with calibration (...)`,
write failures. Writes one or two files. The unit's range is restored after the read.

### `calibrate_measure` (NanoVNA) [@ANCHOR: bench_nanovna_calibrate_measure]

The tool `bench_vna_calibrate_measure` for a NanoVNA: measures one standard and stores it.

| Argument (key) | Type | Meaning |
|---|---|---|
| `session`, `standard`, `cable` | strings | Checked by `vna_tools::measure_names`. |
| `start_hz`, `stop_hz`, `points` | | As in `sweep_args`. |

Opens the shell, identifies, measures, and stores the points with `vna_tools::store_standard`, using a default `Setup` with the cable text. Returns what `store_standard`
returns. Errors: those of `measure_names`, `sweep_args`, `measure` and `store_standard`. Blocking; writes one file. The data is as the unit reports it, so any calibration
active on the unit itself is part of what is stored. See also: `bench_vna_tools_calibrate_measure`, `bench_mcp_vna_cal_build`.

### `power_cycle` [@ANCHOR: bench_nanovna_power_cycle]

The tool `bench_vna_power_cycle` for a NanoVNA: switches the unit's USB hub port off and on to restart it.

| Argument | Type | Meaning |
|---|---|---|
| `inst` | `&Instrument`, borrowed | Needs `inst.vna.hub` (hub and port) and `inst.nanovna.serial` (to detect the unit coming back). |

Behaviour: with no hub configured, `NotSupported("no hub configured for this NanoVNA: set "hub" and "hub_port" in its entry of the bench config")`. Otherwise one
cycle of `usb_hub::power::cycle_escalating` with a 3 s off time and a 25 s wait, run through `uhubctl` (`ProcessRunner`) and `/sys`, with a ready check that the serial path
reappears. Returns `{"power_cycled": true, "hub_power": <report>, "note"}`; the note says the sweep range and any unsaved settings are back to power-on values.
Errors: the `NotSupported` above and any error of the hub power module. Blocking for tens of seconds; it takes the instrument lock and the hub lock (the server holds
both for this tool); it removes power from the hub port, so anything else on that port is also reset. Not retried.
