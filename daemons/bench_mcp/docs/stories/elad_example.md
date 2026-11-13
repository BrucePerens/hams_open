# The ELAD Receiver Tools Listen Without Transmitting, and the Template Instrument Shows a New Driver How to Behave

As the person (or the AI agent) running the bench, I want to read an ELAD FDM-S2, FDM-DUO or FDM-S1 receiver over USB (identify it, record I/Q, take a spectrum, measure a level, find signals, sweep a band) without any chance of transmitting or of two programs fighting over the one unit, and, as a contributor adding a new instrument, I want a small working driver to copy that already does the safe things (checks every number, never turns a bad reply into a value, puts the output off on every path). This story documents the two modules that do that: the ELAD driver (`daemons/bench_mcp/src/elad/mod.rs` and `measure.rs`, the receiver itself is driven by the `elad_usb` crate) and the template driver (`daemons/bench_mcp/src/example/mod.rs`). The tool pages for the agent are the manual pages `instruments/elad` and `instruments/example`; the guide for new drivers is `docs/EXTENDING.md`.

**Hardware note (ELAD loader).** An FDM-S1 or FDM-S2 streams nothing until ELAD's own loader has programmed its FPGA with an image for the wanted sample rate. This project neither includes nor runs that loader; the owner runs it and then sets `"fpga_loaded": true` in the instrument entry. Nothing in this story, and none of the tests, runs hardware or the loader: the tests use a fake unit (`src/elad/testkit.rs`) and the real-unit check is the ignored test `tests/hil_elad.rs`. An FDM-DUO needs no image.

**Safety summary.** Nothing in `src/elad/` can transmit, write CAT commands or write the receiver's EEPROM: it has no transmitter, so the transmit lock, band check and transmit gate do not apply to any ELAD entry; the safety that applies is the instrument's tier (the receiving tools are `read_only` or `read_config`, none is `full`), the single-owner USB lock, the capture directory and the limits in the config. The template instrument drives an imaginary signal source's output, not a radio; its one actuating tool is tier `full` with `dry_run`, `confirm`, a limit checked before anything is sent and a verified switch-off.

## The ELAD driver root: settings, tools, the USB side, opening a receiver

### `parse_config` [@ANCHOR: elad_config]

Reads the `elad` settings of an instrument entry when the config file is loaded: `fpga_loaded`, `capture_dir` and `max_capture_s`. It exists so a bad value stops the server at start instead of surfacing in the middle of a capture. The caller is the config reader (`crate::config`) for every entry with `"driver": "elad"`. The result is stored as `Instrument.elad` and passed to every tool.

| Key / argument | Type | Meaning, range, default |
|---|---|---|
| `c` (argument) | `&Value`, borrowed | The instrument entry as JSON. Unknown keys are not looked at here. |
| `fpga_loaded` | bool | The owner's statement that an FDM-S1/S2's FPGA image was loaded by other means. Default `false`. Anything but a JSON boolean is an error. |
| `capture_dir` | string | Absolute directory that the tools which write files (`capture`, `spectrum`, `sweep`) write into. Optional; default none, in which case those tools refuse. A relative path is an error. The directory is not created or checked for existence here. |
| `max_capture_s` | number | Longest a streaming call may run, seconds; 0.1 to 30, finite; default 5.0. |

Returns `Ok(Config)` with the defaults filled in. Errors (all `Err(String)`): `fpga_loaded must be true or false`, `capture_dir must be a string`, `capture_dir "<x>" must be an absolute path`, `max_capture_s must be a number`, `max_capture_s <x> must be 0.1 to 30 seconds`. Pure: no files, no hardware. `Config` itself is a plain value (`Clone`, `Debug`, `PartialEq`), set once at start and only read afterwards, so it needs no lock.

Example: `{"driver": "elad", "tier": "read_config", "capture_dir": "/var/tmp/bench_elad", "fpga_loaded": false, "max_capture_s": 5}`.

See also: test `elad_config` (`src/elad/tests.rs`); `target_samples` uses `max_capture_s`; `new_file` uses `capture_dir`.

### `Usb::open` [@ANCHOR: bench_mcp_elad_usb_open]

The real implementation of `Bench::open`: it takes the single-owner lock for the receiver, opens the USB device and hands both to `open_locked`. It is the only path by which production code touches an ELAD receiver (the tests substitute a fake `Bench`). Callers: `identify` and `open_tuned`.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `Usb` (unit struct) | Holds no state. |
| `found` | `&Found`, borrowed | One entry of `Bench::find`: bus, address, USB product id, model. |
| `opts` | `OpenOptions`, owned | `fpga` (`FpgaState::LoadedExternally` when the config says so, else `Unknown`) and `sample_rate_hz`. |

Returns `Ok(Radio)` with the lock held for the life of the radio, or `Err(String)`. The error is the lock's refusal (names the holder, see `elad_take_lock`), or "could not open the receiver (...)" with the advice to install `elad_usb/udev/70-elad-fdm.rules` or grant an ACL (the usual Linux permission problem), or the errors of `open_locked`. Blocking: the USB open and the control requests of `Radio::open` run on the calling thread. Side effects: creates a lock file in the lock directory and sends the read-only identification requests. Nothing is transmitted and no EEPROM write is sent. Not exercised by `cargo test` (it needs a device); the hardware run `tests/hil_elad.rs` covers it.

### `take_lock_in` [@ANCHOR: elad_take_lock]

Takes the single-owner lock for one receiver, so the relay, another bench call or any other program built on `elad_usb` never streams from the same unit at the same time. The refusal names the holder (here this program is `bench_mcp`), so the person knows what to stop.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path`, borrowed | Directory for lock files. Production passes `elad_usb::lock::lock_dir()` (`ELAD_LOCK_DIR`, else `XDG_RUNTIME_DIR`, else a private `elad-<uid>` directory in the temp directory); tests pass a temporary one. |
| `found` | `&Found`, borrowed | Gives the bus and address that name the lock file (`elad-<bus>-<address>.lock`). |

Returns `Ok(DeviceLock)`, held until dropped (the radio keeps it), or `Err(text)` carrying the `LockError` text: another holder, or an I/O problem with the directory. Side effect: creates or opens the lock file; the lock is released when the value is dropped. Safe to retry after the holder ends. Test: `elad_take_lock` (two takes on the same unit: the second is refused naming the holder).

### `open_locked` [@ANCHOR: elad_open_locked]

Builds a `Radio` over a transport that carries a lock, so the lock is released exactly when the radio is dropped, on every path including errors. It is split from `Usb::open` so the tests can run it over a fake transport.

| Argument | Type | Meaning |
|---|---|---|
| `lock` | `DeviceLock`, owned | The lock to keep until the radio is dropped. |
| `t` | `Box<dyn Transport>`, owned | The transport (the real USB one, or a fake). |
| `found` | `&Found`, borrowed | Gives the model that decides identification and capabilities. |
| `opts` | `OpenOptions`, owned | FPGA state and sample rate, passed to `Radio::open`. |

Returns `Ok(Radio<Box<dyn Transport>>)` or `Err(String)` from `Radio::open` (identification failed, rate not offered by the model and similar), made into the standard "next step" text of `next_step`. Side effects: `Radio::open` sends the identification requests (firmware, hardware version, EEPROM calibration). If it fails, the lock and transport are dropped, so the lock is freed. Test: `elad_open_locked`.

### `instrument` [@ANCHOR: bench_mcp_elad_instrument]

Picks which configured ELAD instrument a tool call is about. Called by `call_with` before the tier check.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | Every configured instrument of every driver; only those with `kind == Kind::Elad` are considered. |
| `args` | `&Value`, borrowed | The tool arguments; an `instrument` string selects by name. |

Returns a reference to the instrument. With an `instrument` argument: the one of that name, else `Err("no elad instrument named \"<n>\"")`. Without it: the only ELAD instrument, `Err("no elad instrument is configured")` for none, or `Err("name the instrument: a, b")` listing the names when there are several. A name that belongs to a different driver is "no elad instrument named". Pure; no side effects. Test: `bench_mcp_elad_instrument` (cited in `src/elad/tests.rs` together with `call_with`).

### `number` [@ANCHOR: elad_args]

The one place a numeric tool argument is read: it must be present (or defaulted), a number, finite and within a range, and the error names the cause of the limit. It exists so no tool accepts `NaN`, a string or an out-of-range value, and so every refusal explains itself.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool arguments object. |
| `key` | `&str` | Argument name. |
| `lo`, `hi` | `f64` | Inclusive range, in the argument's own unit. |
| `default` | `Option<f64>` | Used when the key is absent or JSON `null`; `None` makes the argument required. The default is range-checked too. |
| `why` | `&str` | Words for the error: what the limit is. |

Returns `Ok(value)`. Errors: `<key> is required and must be a number`; `<key> must be a number` (a string, bool, array); `<key> <v> is outside <lo>..=<hi> (<why>)` (also for non-finite values). Pure. Callers: nearly every function of the driver (`rate_of`, `target_samples`, `fft_len`, `open_tuned`, `plan`, `detect`, `level`). Test: `elad_args`.

### `flag` [@ANCHOR: elad_flag]

Reads a boolean tool argument that may be absent. `filters` and `attenuator` use it.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool arguments. |
| `key` | `&str` | Argument name. |
| `default` | `bool` | Returned when the key is absent or `null` (`filters` true, `attenuator` false). |

Returns `Ok(bool)`. Error: `<key> must be true or false` for anything that is not a JSON boolean (a string `"true"` is refused, so there is no guessing). Pure. Test: `elad_flag`.

### `new_file` [@ANCHOR: elad_new_file]

Turns the `name` argument into the path of a new file in `capture_dir`, refusing anything unsafe. It is called first by `capture`, `spectrum` and `sweep`, before the receiver is touched, so a bad name or a taken name wastes no capture.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config`, borrowed | Supplies `capture_dir`. |
| `args` | `&Value`, borrowed | The tool arguments; `name` is required. |

Returns `Ok(PathBuf)` = `capture_dir/name`. The name must be 1 to 64 characters of ASCII letters, digits, dot, dash and underscore and must not start with a dot, which also rules out path separators and `..`. Errors: `this tool writes a file and the config has no capture_dir`; `name is required and must be a string`; the name-rule message; `<path> already exists; choose another name, nothing was overwritten`. Side effects: none (it only checks that the path does not exist). The check and the later creation are not atomic, but the writers open with `create_new(true)`, so a file created in between is still never overwritten; that failure surfaces as the I/O error `<path>: <error>`. Test: `elad_new_file`.

### `rate_of` [@ANCHOR: elad_rate_arg]

Reads the `rate_hz` argument and checks it against the receiver that is actually plugged in, because the model decides which sample rates exist.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | `rate_hz`, optional, integer Hz; default 192000 (the lowest, the least USB traffic). |
| `found` | `&Found`, borrowed | The first receiver on the bus. |

Returns `Ok(u32)`. Errors: those of `number` (the argument must be a finite number within 1 to 1e9), or `rate_hz <r> is not one of the <model> rates [...]`. From `elad_usb`: FDM-S2 192000, 384000, 768000, 1536000, 3072000, 6144000; FDM-S1 and FDM-DUO 192000 to 3072000 (the DUO's rate is in fact set by the unit, see the tool text). The value is truncated to an integer, so `192000.9` means 192000. Pure. Test: `elad_rate_arg`. Remember an FDM-S1/S2 streams only at the rate its loaded FPGA image was made for.

### `open_tuned` [@ANCHOR: elad_open_tuned]

Opens the first receiver on the bus at the asked rate and applies frequency, filter and attenuator state; it starts nothing. It is the common first step of `acquire`, `tune` and `sweep`.

| Argument | Type | Meaning |
|---|---|---|
| `bench` | `&dyn Bench` | The USB side (real or fake). |
| `cfg` | `&Config`, borrowed | `fpga_loaded` decides the `FpgaState` given to the radio. |
| `args` | `&Value`, borrowed | `freq_hz` (required, 0 to 1e9 here, then the model's own range on tuning), `rate_hz`, `filters` (default true), `attenuator` (default false). |

Returns `Ok(Tuned)`. Errors: no receiver (`no ELAD receiver is on the USB bus` plus next step); the argument errors of `rate_of`, `number` and `flag`; the open errors (lock held, permission, identification); for a model without switchable front end (FDM-S1, FDM-DUO) `the <model> has no switchable filters or attenuator; leave filters and attenuator at their defaults` when `filters` is false or `attenuator` is true; and the radio's refusal when `freq_hz` is outside the model's range (FDM-S2 and FDM-DUO 10 kHz to 54 MHz, FDM-S1 10 kHz to 30 MHz per `elad_usb`). The front-end state is recorded before the tune so the tune writes it and the receiver is never tuned at a gain the caller did not ask for. Side effects: takes the USB lock (kept by the `Tuned`), sends identification, front-end and tuning control requests. Receive only. Test: cited with `bench_mcp_elad_tune` in `src/elad/tests.rs`.

#### Structures `Acquired` and `Tuned` (no anchor of their own)

Two private structures carry the results between the helpers. `Acquired` is one finished acquisition and is created by `acquire`, read by `summary` and the tools. `Tuned` is a receiver opened and tuned but not streaming and is created by `open_tuned`, read by `acquire`, `tune` and `sweep`. They are not serialized, not shared between threads and live for one tool call.

| Struct | Field | Type | Meaning |
|---|---|---|---|
| `Acquired` | `samples` | `Vec<(f32, f32)>` | I/Q pairs, full scale 1.0, in arrival order; never empty. |
| | `rate_hz` | `u32` | The rate that was asked for and set. |
| | `elapsed_s` | `f64` | Seconds the reading loop took (includes start-up). |
| | `stats` | `elad_usb::radio::Stats` | Radio counters (samples, bytes, reads, timeouts, empty reads). |
| | `offset_db` | `f64` | EEPROM calibration offset for the current rate, filters and attenuator state. |
| | `model` | `&'static str` | Model name such as `FDM-S2`. |
| `Tuned` | `radio` | `Radio<Box<dyn Transport>>` | The open radio; it holds the single-owner lock. |
| | `found` | `Found` | The bus entry that was opened. |
| | `rate` | `u32` | The rate it was opened at. |

Invariants: `Tuned.radio` is not streaming; `Acquired.samples` has at least one pair.

### `start_stream` [@ANCHOR: elad_start_stream]

Starts the I/Q stream of an opened radio. An FDM-S1/S2 whose FPGA is not declared loaded is refused here, before anything is sent to start it.

| Argument | Type | Meaning |
|---|---|---|
| `radio` | `&mut Radio`, borrowed mutably | A radio from `open_tuned`. |

Returns `Ok(())`. Error: for `FpgaImageRequired`, a message that says nothing was sent beyond identification and tuning and points to the elad setup page and to `"fpga_loaded": true` (set it only after the image really was loaded, with the loader the owner runs); any other error gets the standard next-step text. After a successful or half-failed start the caller must call `radio.stop()` (the radio marks itself streaming first so a stop is always sent). Side effect: a USB control request that starts the FIFO. Receive only.

### `stream_samples` [@ANCHOR: elad_stream_samples]

Reads from a started stream until `target` samples or `deadline_s` seconds have passed.

| Argument | Type | Meaning |
|---|---|---|
| `radio` | `&mut Radio`, borrowed mutably | A started radio. |
| `target` | `usize` | Samples wanted; extra samples in the last block are dropped. |
| `deadline_s` | `f64` | Seconds to read at most. |

Returns `Ok((samples, elapsed_s))`; there may be fewer than `target` if the deadline hit (the caller decides whether that is enough; `capture` is not strict, `sweep` is). Bulk timeouts (500 ms each) are tolerated and counted by the radio. Errors: no sample at all (`the receiver delivered no samples before the deadline`), or a radio error; `NoSamples` is explained as probably a missing FPGA image or one lost at power-up or a rate change, and says the receiver was stopped. The caller stops the stream whatever the result. Blocking, on the calling thread, for up to the deadline plus one read timeout. Memory: pre-reserves at most about a million pairs. Test: `elad_stream_samples`.

### `acquire` [@ANCHOR: elad_acquire]

One complete acquisition: open, tune, start, read, stop. It is what `capture`, `spectrum`, `level` and `detect` build on.

| Argument | Type | Meaning |
|---|---|---|
| `bench` | `&dyn Bench` | The USB side. |
| `cfg` | `&Config`, borrowed | FPGA declaration. |
| `args` | `&Value`, borrowed | As `open_tuned`. |
| `target` | `usize` | Samples to read. |
| `deadline_s` | `f64` | Longest read time, seconds. |

Returns `Ok(Acquired)` with the radio's statistics and calibration offset. Errors: anything from `open_tuned`, `start_stream` (FPGA not declared: refused before streaming) and `stream_samples`. The stream is stopped on every path (`let _ = radio.stop()` runs before the result is looked at), and the radio, with the lock, is dropped on return. A receiver with the FPGA missing therefore reports silence as a missing image rather than hanging. Receive only. Tests: `elad_acquire` (three of them: normal, FPGA refused, silence).

### `target_samples` [@ANCHOR: elad_target_samples]

Works out how many samples a call will read for `seconds` at `rate`, capped by memory and by the config.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config`, borrowed | `max_capture_s` is the upper limit of `seconds`. |
| `args` | `&Value`, borrowed | `seconds`, 0.01 to `max_capture_s`. |
| `rate` | `u32` | The sample rate, Hz. |
| `default_s` | `f64` | Seconds when `seconds` is absent, itself capped at `max_capture_s` (`capture` passes 1.0). |

Returns `Ok((n, seconds))` with `n = max(1, floor(seconds x rate))`. Errors: the `number` errors for `seconds`, or `<s> s at <rate> Hz is <n> samples; at most 16000000 fit in one call (lower seconds or rate_hz)`. 16 million pairs is 128 MB of I/Q in memory; the cf32 file is 8 bytes per pair. Pure. Test: `elad_target_samples`.

### `summary` [@ANCHOR: elad_summary]

Builds the JSON object that every streaming tool returns first and then adds its own fields to. It tells the reader whether the capture can be trusted.

| Argument | Type | Meaning |
|---|---|---|
| `a` | `&Acquired`, borrowed | The acquisition. |

Returns a JSON object: `model`, `samples`, `rate_hz`, `measured_rate_hz` (samples divided by elapsed time; includes start-up time so it reads 10 to 15 % low), `elapsed_s`, `bulk_timeouts`, `bulk_empty_reads`, `bytes`, `rms` and `peak` (amplitude, full scale 1.0), `dc_i` and `dc_q` (DC offset), `clipped_samples` (at full scale) and `calibration_offset_db`. No errors; pure. Uses `dsp::stats`. Test: cited with `bench_mcp_elad_capture`.

## The ELAD tools

### `list` [@ANCHOR: bench_mcp_elad_list]

Tool `bench_elad_list` (tier `read_only`): lists the ELAD receivers on the USB bus. It opens nothing and sends nothing, so it is safe while the relay owns the unit.

| Argument | Type | Meaning |
|---|---|---|
| `bench` | `&dyn Bench` | The USB side. |
| `cfg` | `&Config`, borrowed | Used for `streams_under_this_config`. |

Returns `{"receivers": [...], "count": n}`; each row has `bus`, `address`, `usb_id` (`1721:<pid>`), `model`, `needs_fpga_image` and `streams_under_this_config` (true when no image is needed or `fpga_loaded` is set). An empty bus is `count` 0, not an error. Error: a USB enumeration failure with the next-step text. Test: `bench_mcp_elad_list`.

### `identify` [@ANCHOR: bench_mcp_elad_identify]

Tool `bench_elad_identify` (tier `read_only`): opens the first receiver and reads what it says about itself, sending only read requests; starts no stream and tunes nothing.

Arguments: `bench`, `cfg` as above and the optional `instrument`. It opens at 192000 Hz.

Returns `model`, `usb_id`, `firmware` ("major.minor" or null), `hardware` (or null), `serial`, `rate_correction_hz`, the calibration offsets `offset_global_db`, `offset_lp_db`, `offset_att_db`, `rx_range_hz` (low, high), `sample_rates_hz`, `needs_fpga_image` and `streams_under_this_config`. Errors: no receiver, lock held (names the holder), permission, identification failure. Takes and releases the USB lock within the call. Receive only; no EEPROM write. Verified on an FDM-S2 (firmware 3.0, hardware 1.3) per the README; other values unknown. Test: `bench_mcp_elad_identify`.

### `capture` [@ANCHOR: bench_mcp_elad_capture]

Tool `bench_elad_capture` (tier `read_config`): records `seconds` of I/Q at `freq_hz` to a new cf32 file and returns a summary.

| Argument | Type | Meaning |
|---|---|---|
| `freq_hz` | number, required | Centre frequency, Hz, within the model's range. |
| `seconds` | number, required in the schema | 0.01 to `max_capture_s`, and at most 16 million samples. |
| `name` | string, required | New file name in `capture_dir`; see `new_file`. |
| `rate_hz`, `filters`, `attenuator` | optional | As `rate_of`, `flag`. |

Returns `summary` plus `file`, `format` (`cf32 little-endian interleaved I,Q, full scale 1.0`), `file_bytes` and `centre_hz`. Errors: those of `new_file`, `target_samples`, `acquire`, and file I/O (`<path>: <error>`); the file is created only after the capture succeeded, with `create_new` so an existing file is never touched. The read deadline is `seconds + 3`. Blocking for about the capture time. Side effects: one file written; the receiver is opened, streamed, stopped and released. Receive only. Test: `bench_mcp_elad_capture`.

### `fft_len` [@ANCHOR: elad_fft_len]

Reads the `fft_len` argument for the spectrum tools: 256 to 65536, default 4096, and a power of two. Argument: `args` with optional `fft_len`. Returns `Ok(usize)`; errors are those of `number` or `fft_len <n> must be a power of two from 256 to 65536`. Pure. Used by `spectrum` and `detect`. Test: cited with `bench_mcp_elad_spectrum`.

### `spectrum` [@ANCHOR: bench_mcp_elad_spectrum]

Tool `bench_elad_spectrum` (tier `read_config`): a spectrum snapshot around `freq_hz` as a CSV in `capture_dir`, with the noise floor and the strongest peaks.

Arguments: `freq_hz` and `name` (required), `fft_len` (see above), `rate_hz`, `filters`, `attenuator`. It reads 16 FFT (fast Fourier transform) lengths of samples (at least two, at most 16 million; for example 65536 x 16 = 1048576) and averages them with Welch's method (Hann window, 50 % overlap, `dsp::welch`).

Returns `summary` plus `file`, `bins`, `bin_hz`, `noise_floor_dbfs` (the median bin) and up to five `peaks` (each `offset_hz`, `freq_hz`, `power_dbfs`). The CSV header is `offset_hz,freq_hz,power_dbfs`; power is dBFS relative to a full-scale complex tone. The peak call is `dsp::peaks(&sp, 5, 40.0, 3, 3)`: at most 5 peaks, at least 40 dB over the floor, with 3 as the minimum separation in bins and 3 as the DC bins skipped (argument order as in `dsp::peaks`). Errors: as `capture`, plus `dsp::welch` errors. Receive only. Test: `bench_mcp_elad_spectrum`.

### `level` [@ANCHOR: bench_mcp_elad_level]

Tool `bench_elad_level` (tier `read_config`): the signal level at `freq_hz`, read from a 4096-point spectrum averaged over 32 blocks of samples.

Arguments: `freq_hz` (required), `bandwidth_hz` (half-width to sum, 100 to 20000, default 1000), `rate_hz`, `filters`, `attenuator`.

Returns `summary` plus `band_half_width_hz`, `peak_bin_dbfs` (strongest bin within the half-width), `band_power_dbfs` (power sum of the bins within the half-width), `level_dbm_estimate_unverified` (peak bin plus the EEPROM calibration offset) and a `note`. The dBm figure is an estimate and has not been checked against a known generator. Errors: argument errors and those of `acquire` and `welch`. Receive only; the receiver is opened and released inside the call. Test: `bench_mcp_elad_level`.

### `call_with` [@ANCHOR: bench_mcp_elad_call_with]

Dispatches an ELAD tool: finds its `Spec`, picks the instrument, checks the tier and runs the function. `call` is the same with the real USB (`Usb`); `call_with` exists so tests run the entire path against a fake unit.

| Argument | Type | Meaning |
|---|---|---|
| `bench` | `&dyn Bench` | The USB side. |
| `insts` | `&[Instrument]` | All configured instruments. |
| `name` | `&str` | Tool name, e.g. `bench_elad_capture`. |
| `args` | `&Value` | Tool arguments. |

Returns the tool's JSON. Errors: `unknown tool <name>`; those of `instrument`; `<name> needs tier <Tier>; "<inst>" is configured at <Tier>` when the instrument is below the tool's tier (list and identify need `read_only`; the rest `read_config`; an instrument at `read_only` therefore cannot stream); and the tool's own. Pure apart from what the tool does. Test: `bench_mcp_elad_call_with`.

## The measuring tools (`src/elad/measure.rs`)

### `tune` [@ANCHOR: bench_mcp_elad_tune]

Tool `bench_elad_tune` (tier `read_config`): applies a frequency, filter and attenuator state and reports what was sent. It streams nothing, so it works on an FDM-S1/S2 whose FPGA image is not loaded.

Arguments: `freq_hz` (required), `rate_hz`, `filters`, `attenuator` (as `open_tuned`).

Returns `model`, `tuned_hz`, `tuning_word` (the 32-bit DDC word: the fraction of the ADC clock times 2^32; clock 61.44 MHz for FDM-S1, 122.88 MHz otherwise), `rate_hz`, `streaming` false, `front_end_switches`, `calibration_offset_db` and a `note` that the receiver keeps no tuning between calls, so this only proves it accepts the state. Errors: those of `open_tuned`. Side effects: lock taken and released; control requests sent. Receive only. Test: `bench_mcp_elad_tune`.

### `refined_offset_hz` [@ANCHOR: elad_refine_peak]

Refines a peak's frequency below one bin with a parabola through the log power of the peak bin and its two neighbours.

| Argument | Type | Meaning |
|---|---|---|
| `sp` | `&dsp::Spectrum`, borrowed | The spectrum (`power_db`, `bin_hz`). |
| `offset_hz` | `f64` | The peak's offset from the centre, Hz. |

Returns the refined offset, Hz. The correction is limited to half a bin. If the peak is at an edge of the spectrum (no neighbour) or the three powers are collinear (denominator near zero) the input offset is returned unchanged. No errors, pure. Test: cited with `bench_mcp_elad_detect`.

### `detect` [@ANCHOR: bench_mcp_elad_detect]

Tool `bench_elad_detect` (tier `read_config`): looks for signals around `freq_hz`.

| Argument | Type | Meaning |
|---|---|---|
| `freq_hz` | number, required | Centre frequency, Hz. |
| `threshold_db` | number | Margin over the noise floor, 3 to 60, default 10. |
| `fft_len` | integer | Power of two, 256 to 65536, default 4096. |
| `max_peaks` | integer | 1 to 20, default 5. |
| `rate_hz`, `filters`, `attenuator` | optional | As elsewhere. |

Returns `summary` plus `detected` (false when nothing stands out), `threshold_db`, `noise_floor_dbfs` (the median bin), `bin_hz`, `usable_half_span_hz` (80 % of half the rate) and `signals`, each `freq_hz` (refined), `offset_hz`, `power_dbfs` and `snr_db` (peak over the median bin, a one-bin bandwidth). Peaks outside the usable 80 % of the span and the 3 DC bins around the centre are ignored. Errors: argument errors and those of `acquire`. Receive only. Test: `bench_mcp_elad_detect` (two).

### `SweepPlan` and `plan` [@ANCHOR: elad_sweep_plan]

`plan` checks a sweep's arguments before the receiver is opened, and builds `SweepPlan { start, stop, step, centres }`.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config`, borrowed | `max_capture_s`. |
| `args` | `&Value`, borrowed | `start_hz`, `stop_hz` (Hz, 0 to 1e9, stop above start), `step_hz` (1 Hz to 80 % of the rate; default 40 % of the rate). |
| `rate` | `u32` | Sample rate. |

Fields: `start`, `stop` Hz; `step` Hz; `centres` the step centres, `start + step x i + step/2`, one per step. Errors: stop not above start; step out of range; more than 400 steps (`MAX_STEPS`; the message suggests raising the step or sweeping in parts); estimated running time over `6 x max_capture_s` seconds, where one step costs `(1024 x 8 + 4096) / rate + 0.02` seconds. Pure. Test: `elad_sweep_plan` (two).

### `sweep` [@ANCHOR: bench_mcp_elad_sweep]

Tool `bench_elad_sweep` (tier `read_config`): sweeps `start_hz` to `stop_hz` by retuning in steps and writes `freq_hz,power_dbfs` to a new CSV.

Arguments: `start_hz`, `stop_hz`, `name` (required); `step_hz`, `rate_hz`, `filters`, `attenuator`. Each step tunes to a centre, drops 4096 settling samples, reads 8192 samples, takes a 1024-point Welch spectrum, and keeps the bins within half a step of the centre. The last centre is clamped to the model's upper limit; a band outside the model's range is refused before the receiver is opened.

Returns `model`, `rate_hz`, `steps`, `step_hz`, `start_hz`, `stop_hz`, `points`, `elapsed_s`, `file`, `noise_floor_dbfs` (median of all points), `signals` (see `sweep_peaks`) and a `note` that a signal exactly at a step centre falls in the unreported centre bins; repeat with `start_hz` shifted by half a step. Errors: `plan` errors; a tune failure, a step that delivered too few samples, a stream error: all stop the sweep, stop the radio and return the error with no CSV written; file I/O. Blocking for the planned running time. One lock, one stream for the whole sweep. Receive only. Test: `bench_mcp_elad_sweep`.

### `sweep_peaks` [@ANCHOR: elad_sweep_peaks]

Finds the strongest local maxima of a sweep.

| Argument | Type | Meaning |
|---|---|---|
| `rows` | `&[(f64, f64, bool)]` | Points: frequency Hz, power dBFS, and whether the point lies within two bins of a step centre (excluded). |
| `floor` | `f64` | The noise floor, dBFS. |
| `margin_db` | `f64` | Minimum height over the floor (`sweep` passes 10). |
| `count` | `usize` | Most peaks to return (`sweep` passes 5). |

Returns a JSON array of `{freq_hz, power_dbfs, snr_db}`, strongest first, with candidates at least five points apart. A candidate is a point that is not a centre bin, is at least `floor + margin_db`, is not below its left neighbour and is above its right. Empty array when nothing qualifies. Pure, no errors. Test: `elad_sweep_peaks`.

## The template instrument (`src/example/mod.rs`)

The `example` driver is an imaginary SCPI-over-TCP signal source, "ACME EXAMPLE-1000". It has no hardware; it is the skeleton `tools/new_instrument.py` copies. It is listed only when an entry with `"driver": "example"` exists. Its tests run against a fake device on a local socket (`testkit.rs`).

### `parse_config` (example) [@ANCHOR: example_config]

Reads the `example` settings: `max_level_v` and `max_pulse_s`, the owner's limits; a bad value is refused when the config file is read, so the server never starts with a limit it cannot honour.

| Key | Type | Meaning, range, default |
|---|---|---|
| `max_level_v` | number | Highest output level any tool may set, volts; above 0 and at most the rating 5.0 V; default 1.0. |
| `max_pulse_s` | number | Longest time the output may be on in one call, seconds; 0.1 to 60; default 2.0. |

Returns `Ok(Config)`. Errors: `max_level_v must be a number`, `max_level_v <x> must be above 0 and at most the rating 5 V`, `max_pulse_s must be a number`, `max_pulse_s <x> must be 0.1 to 60 seconds`. The host and port are not read here: they belong to the generic instrument entry (the port default is unknown from this file). Pure. Test: `example_config`.

### `instrument` (example) [@ANCHOR: bench_mcp_example_instrument]

Chooses the example instrument a call is about; the same rule as the ELAD `instrument`: by the `instrument` argument, or the only one. Arguments `insts` (all instruments; only `Kind::Example` considered) and `args`. Errors: `no example instrument named "<n>"`, `no example instrument is configured`, `name the instrument: a, b`. Pure. Test: `bench_mcp_example_instrument`. Copy it unchanged into a new driver, changing the kind.

### `flag` (example) [@ANCHOR: bench_mcp_example_flag]

Reads a boolean argument that may be absent (false) but must not be anything else: `Ok(false)` for absent or `null`, `Ok(b)` for a JSON boolean, else `Err("<key> must be true or false")`. Arguments `args` and `key`. It is how `dry_run` and `confirm` are read, so `"confirm": "yes"` is refused and cannot count as consent. Pure. Test: `bench_mcp_example_flag`.

### `number` (example) [@ANCHOR: example_args]

Reads a number argument that must be present, finite and in `lo..=hi`; the error names the range and the cause of the limit (`why`). Arguments `args`, `key`, `lo`, `hi`, `why`. Returns `Ok(f64)`. Errors: `<key> is required and must be a number` (also for a string); `<key> <v> is outside <lo>..=<hi> (<why>)`. Unlike the ELAD version it has no default. Pure. Used by `measure` (`count`) and `pulse_args` (`level_v`, `pulse_s`). Test: `example_args`.

### `ask` [@ANCHOR: example_retry]

Sends a query and returns the reply, repeating it up to `tries` times in all.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | The line link to the instrument. |
| `cmd` | `&str` | The query, e.g. `*IDN?`. |
| `tries` | `u32` | Total attempts (callers pass 3, `READ_TRIES`). |

Each attempt sets the reply timeout to 2 s (`REPLY_TIMEOUT_S`), so no call waits long. An empty reply counts as a failure. Returns the non-empty reply, or after the last try `Err` naming the command, the number of tries, the timeout and the last cause, ending with the next step (check the cable, power, `bench_example_status`, power-cycle). Only for reads: repeating a query changes nothing, and commands that change the instrument (`OUTP ON`, `LEV`) are never retried this way. Blocking: up to `tries x 2` s. Test: `example_retry`.

### `finite` [@ANCHOR: example_readings]

Turns a reply into a number: a finite floating point value, with an optional trailing unit suffix of letters (`1.234V`). Arguments `cmd` (for the message) and `reply`. Returns `Ok(f64)`. Anything else, a dash, `NaN`, `inf`, a half line, is `Err("<cmd>: the reply \"<reply>\" is not a finite number" + next step)`. A measurement tool must never turn an instrument or link fault into a zero. Pure. Test: `example_readings`.

### `on_off` [@ANCHOR: bench_mcp_example_on_off]

Reads the output state: `ON` or `1` is true, `OFF` or `0` is false, case-insensitive, trimmed; any other word is `Err("OUTP?: the reply \"<r>\" is neither ON nor OFF" + next step)`. Argument `reply`. The error always says `OUTP?` since that is its only query. Pure. It makes a safe-state check trustworthy: only a real OFF counts as off. Test: `bench_mcp_example_on_off`.

### `open` [@ANCHOR: example_identity]

Opens the link and refuses an address that does not answer as the expected model, before any other command is sent.

| Argument | Type | Meaning |
|---|---|---|
| `connect` | `&Connector` | How to reach the instrument: `tcp` in production, a fake in tests. |
| `inst` | `&Instrument` | The configured entry (host, port). |

Returns `Ok((Link, parts))` where `parts` is the `*IDN?` reply split at commas: make, model, serial, firmware. The reply must have at least four fields and the second must be `EXAMPLE-1000` (`MODEL`). Otherwise `Err("<host>:<port> answered *IDN? with ... not an EXAMPLE-1000. Nothing was sent to it. Next: check the host and port in the bench config.")`; connect or no-reply errors come from `connect` and `ask`. Side effect: a TCP connection (5 s connect timeout in `Link::tcp`) and one query. Test: `example_identity`.

### `status` [@ANCHOR: bench_mcp_example_status]

Tool `bench_example_status` (tier `read_only`): identity, whether the output is on and the level it is set to; changes nothing. Arguments: `connect`, `inst`, optional `instrument`. Returns `make`, `model`, `serial`, `firmware`, `output` (`"on"` or `"off"`), `level_v` (volts) and `limits` (`max_level_v`, `max_pulse_s`). Errors: from `open`, `ask`, `on_off`, `finite`. Test: `bench_mcp_example_status`.

### `measure` [@ANCHOR: bench_mcp_example_measure]

Tool `bench_example_measure` (tier `read_only`): the voltage at the meter input, the mean of `count` readings. `count` is an integer 1 to 10, default 1. Returns `voltage_v` (mean, volts), `readings` and `spread_v` (highest minus lowest). Any reading that is not a finite number is an error, never a zero (`finite`); each query is retried by `ask`. Errors: `count` out of range or not a number, and those of `open`, `ask`, `finite`. Blocking for the number of readings. Test: `bench_mcp_example_measure`.

### `ensure_off` [@ANCHOR: example_safe_state]

Switches the output off and proves it. Up to 3 times (`OFF_TRIES`): send `OUTP OFF`, query `OUTP?`, and succeed only when the instrument itself says OFF.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | An open link. |

Returns `Ok(())`. Otherwise `Err("SAFE STATE NOT CONFIRMED: the output may still be ON (<last cause>). Do this now: switch the instrument's output off at its front panel or pull its power. Do not use the instrument again until the owner has looked at it.")`. Unlike `ask`, `OUTP OFF` is repeated; that is safe, as it is idempotent. Blocking up to about 3 x (2 + 2) s. Used by `safe_state` and by every path of `pulse`. Test: `example_safe_state`.

### `safe_state` [@ANCHOR: bench_mcp_example_safe_state]

Tool `bench_example_safe_state` (tier `read_config`): opens the link (with the identity check) and runs `ensure_off`. Returns `{"output": "off", "verified": true}` or the errors of `open` / `ensure_off`. Safe at any time and the first thing to run when something went wrong. Test: `bench_mcp_example_safe_state`.

### `Pulse` and `pulse_args` [@ANCHOR: example_pulse_args]

`Pulse { level_v, pulse_s }` is the checked arguments of a pulse: public, created by `pulse_args`, read by `pulse` and `run_pulse`; plain values, one call's life, no serialization.

| Field | Type | Meaning |
|---|---|---|
| `level_v` | `f64` | Output level, volts, 0 to `cfg.max_level_v`. |
| `pulse_s` | `f64` | On time, seconds, 0.1 (`MIN_PULSE_S`) to `cfg.max_pulse_s`. |

`pulse_args(cfg, args)` checks both with `number`, naming the config cap as the cause, before anything is sent. Errors: `level_v` / `pulse_s` missing, not a number, or out of range. Pure. Test: `example_pulse_args`.

### `pulse` [@ANCHOR: example_pulse]

Tool `bench_example_pulse` (tier `full`): drives the output at `level_v` for `pulse_s` seconds, then switches it off and verifies. It is the model of an actuating tool.

| Argument | Type | Meaning |
|---|---|---|
| `level_v`, `pulse_s` | number, required | See `Pulse`. |
| `confirm` | bool | Must be true, from the person at the bench. |
| `dry_run` | bool | Return the plan and send nothing. |

Order: arguments are checked (`pulse_args`); with `dry_run` it returns `{"dry_run": true, "sent": [], "would": [...]}` and sends nothing, not even a connection; without `confirm` it refuses, saying what it would do; then it connects, checks identity, and refuses to start if the output is already ON (use `safe_state`). Then `run_pulse` sets a time-out on the instrument (`pulse_s + 1`) so the instrument itself switches off even if the server dies, sets the level, turns on and sleeps; then `ensure_off` runs on every path. Returns `{"level_v","pulse_s","output":"off","verified_off":true}`; a failed pulse with a verified off is an error saying so; an unverifiable off is the loud SAFE STATE NOT CONFIRMED error. Blocking for about the pulse time; a transmitter does not exist here, but the equivalent controls (tier, `confirm`, limits, verified off) apply as for a real actuating tool. Test: `example_pulse`.

### `run_pulse` [@ANCHOR: bench_mcp_example_run_pulse]

Sends the three commands of a pulse and waits: `OUTP:TMO <pulse_s+1>`, `LEV <level_v>`, `OUTP ON`, then sleeps `pulse_s` on the calling thread. Arguments `l` (`&mut Link`) and `p` (`&Pulse`). Returns `Ok(())` or the link's send error. It does not switch off: the caller (`pulse`) always runs `ensure_off` afterwards. Commands are sent once and never retried. Test: `bench_mcp_example_run_pulse`.

### `call_with` (example) [@ANCHOR: bench_mcp_example_call_with]

Dispatches an example tool; `call` is the same with `tcp`. Arguments: `connect` (`&Connector`), `insts`, `name`, `args`. Errors: `unknown tool`, those of `instrument`, `<name> needs tier <T>; "<inst>" is configured at <T>`, and the tool's. `Connector` is `dyn Fn(&Instrument) -> Result<Link, String>`; `tcp` builds `Link::tcp(host, port)`. Pure apart from the tool. Test: `bench_mcp_example_call_with`.
