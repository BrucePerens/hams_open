# The Bench Scope, the Bench Supply and the VNA's Sweep Tools: What Each Function Takes, Does and Returns

As an operator with a Siglent SDS1000X-E oscilloscope (written against an SDS1202X-E, firmware 1.3.27) on the bench LAN, an Abestop ATM6301 supply on a USB serial adapter, and a DG8SAQ VNWA 3E vector network analyser on a USB hub, I want an AI model to read waveforms, set the scope, drive the supply within caps I configured, sweep the VNA and look at a stored sweep in the time domain, without it being able to exceed those caps or leave the supply on. This story covers the function-level anchors of `daemons/bench_mcp/src/scope.rs`, `scope_more.rs`, `psu.rs`, `vna.rs` and `vna_time.rs`: what each function is for, what it takes, what it returns, which errors it gives and what it does to the instrument. The tool table, the tier check, the instrument lock and the config are in `src/main.rs` and `src/config.rs` and have their own entries; the shared line helpers (`Link::query`, `number`, `block`, `raw_until_idle`) are documented with `src/link.rs`.

How a call reaches these functions (from `call` in `src/main.rs`, read 2026-11-11): the tool must be offered for the instrument's kind; the instrument's configured tier must be at least the tool's tier (`read_only` < `read_config` < `full`, otherwise the call returns `needs tier ...` before anything is sent); then the per-instrument lock is taken (one conversation at a time per scope socket or serial port, so two calls never interleave their SCPI). For the scope and the supply a link is opened with `connect` and the function runs on it. The VNA functions run after `vna::install_power_hook()` and `ensure_present`. `bench_vna_time_domain` is dispatched before any VNA driver is chosen and reads a file only.

None of these functions keys an amateur-radio transmitter. The scope and the supply are bench instruments; the VNA's sweep and power-cycle put a low-level test signal on the unit's own TX port (the README gives about -17 dBm at 0 dB), which is why those tools need tier `read_config`. The transmit lock, the band check and the transmit gate of the radio daemons do not apply to anything here. The only function that can drive an external load is `output_on` in `psu.rs`, and its protections (caps, stored-setpoint check, confirmation, timed switch-off) are described in its entry.

Tests that cite these anchors are in the `#[cfg(test)]` modules at the foot of each source file and are named in the entries.

## The scope: reading and setting (`src/scope.rs`)

### `bench_mcp_scope_ch` [@ANCHOR: bench_mcp_scope_ch]

Reads which scope channel a tool call names. Every scope tool that acts on one channel (`bench_measure`, `bench_capture`, `bench_scope_spectrum`, the cursors, channel and set tools) calls it first, so that a missing or wrong channel is refused before any SCPI is sent. The SDS1202X-E has two channels, so only 1 and 2 are accepted.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` (borrowed JSON object) | The tool arguments; the key `channel` is read. |

Returns `Ok(1)` or `Ok(2)` as `u8`. Errors: `channel must be 1 or 2` when the key is missing, null, not an unsigned whole number, or any other number (0, 3, 1.5, a string). There is no default channel. Pure function: no I/O, no lock, no side effect. Tests: `measurements_are_asked_per_channel_and_only_for_plain_item_names`, `a_capture_is_written_as_f32_volts_with_its_edges_and_a_stopped_scope_is_refused` and `settings_become_the_scopes_commands_and_bad_values_are_refused_before_sending` in `scope.rs`.

### `bench_mcp_scope_status` [@ANCHOR: bench_mcp_scope_status]

`status(s)` is the scope half of `bench_status` (tier `read_only`): one read-only look at what the scope is doing now.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | The open SCPI link to the scope (TCP port 5025 by default), mutably borrowed for the call. |

It queries `TRIG_MODE?`, `TDIV?`, `SARA?` and, for channels 1 and 2, `C<n>:VDIV?`, `C<n>:OFST?`, `C<n>:CPL?`, `C<n>:ATTN?`. Returns a JSON object: `trigger_mode` (the text after the last space or comma of the reply, for example `AUTO`, `NORM`, `SINGLE`, `STOP`), `acquisition` (`"stopped"` when the mode is `STOP`, otherwise `"running"`), `timebase_s_per_div` (seconds per division), `sample_rate_sa_s` (samples per second), and `channels`, a list of two objects with `channel`, `vdiv_v` (volts per division), `offset_v` (volts), `coupling` (`A1M`, `D1M` or `GND` as the scope reports) and `attenuation` (probe ratio). Errors: any link error (`send:`, `read:` with the OS text, a timeout after the link's 10 s default) and `<command>: not a number: <reply>` if the scope answers a numeric query with something that has no number. It sends no setting command, so it changes nothing on the scope. About eleven round trips; the instrument lock is held for the whole call. `scope::set` also calls it to show the result of a change. Test: `the_status_reads_the_trigger_the_timebase_and_both_channels`.

### `bench_mcp_scope_measure` [@ANCHOR: bench_mcp_scope_measure]

`measure(s, args)` is the scope half of `bench_measure` (tier `read_only`): the scope's own parameter measurements on one channel, optionally repeated and summarised.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `args.channel` | integer 1 or 2 | Required; see `bench_mcp_scope_ch`. |
| `args.items` | array of strings | Measurement names such as `PKPK`, `MEAN`, `FREQ`, `DUTY`. Default when absent: `PKPK`, `MEAN`, `FREQ`, `DUTY`. Entries that are not strings are silently dropped. Each name must consist only of ASCII upper-case letters. |
| `args.statistics` | integer 2 to 100, or null/absent | Number of repeated readings to summarise. Absent or null means one reading. |

With one reading each item is asked as `C<n>:PAVA? <ITEM>` and the result object maps the item name to the text after the last space or comma of the reply, a string that keeps its unit (for example `1.02V`) or `****` when the scope cannot measure. With `statistics` the same query is repeated that many times per item and the value is the object from `statistics` (see `bench_mcp_scope_statistics`). The scope's own statistics command (`PACU`) did not answer on the unit this was written against, so the repeat is done here. Errors: `channel must be 1 or 2`; `statistics must be a whole number of readings from 2 to 100`; `bad item` (an item with any character other than A to Z; an empty string passes this check vacuously and is sent as an empty item name); link errors. Items are asked in order, so a `bad item` or link error on a later item loses the results of earlier ones (nothing partial is returned) but the earlier queries were already sent. Read-only on the scope, lock held for the whole call; with 100 readings of several items the call is long (each query is a round trip). Tests: `measurements_are_asked_per_channel_and_only_for_plain_item_names`.

### `bench_mcp_scope_statistics` [@ANCHOR: bench_mcp_scope_statistics]

`statistics(v, asked)` (private) summarises repeated readings for `measure`.

| Argument | Type | Meaning |
|---|---|---|
| `v` | `&[f64]` | The readings that parsed as numbers (units and a trailing `V`, `Hz`, `s` and so on already stripped by `measure`). |
| `asked` | `usize` | How many readings were requested. |

Returns a JSON object. When `v` is empty: `readings` 0, `asked`, `failed` equal to `asked`, and a `note` explaining that the scope answers `****` when it cannot measure. Otherwise: `readings` (count), `asked`, `failed` (`asked` minus the count), `mean`, `min`, `max` and `std_dev`. The standard deviation is the population form (divided by the count, not count minus one). Values are in the unit of the measured item, which this function does not know. No errors, no I/O, no panic (the empty case is handled first). Test: `statistics_summarise_numeric_readings_and_count_the_failed_ones`.

### `scope_mcp_analyse` [@ANCHOR: scope_mcp_analyse]

`analyse(volts, sara)` (private) summarises a captured waveform for `capture`: extremes, mean, rising-edge count and an estimated frequency. (This id lacks the `bench_mcp_` prefix the others have; the code is the authority and the id is kept as it is.)

| Argument | Type | Meaning |
|---|---|---|
| `volts` | `&[f32]`, volts | The samples, in time order. Must not be empty: the function reads `volts[0]` and would panic on an empty slice. `read_waveform` never returns an empty list, so the caller is safe. |
| `sara` | `f64`, samples per second | The sample rate; only used for the frequency. |

Returns the private struct `Analysis { min: f32, max: f32, mean: f64, edges: u32, freq: Option<f64> }`. The midpoint is `(min + max) / 2`; the high threshold is the midpoint plus 10 percent of the span and the low threshold the midpoint minus 10 percent, so a hysteresis band of 20 percent of the span keeps noise on a flat level from counting as edges. The state starts high when the first sample is above the midpoint. A rising edge is counted each time the signal, while low, goes above the high threshold; the start state is not an edge. `freq` is `sara * (edges - 1) / (last_edge_index - first_edge_index)` when at least two rising edges were counted, otherwise `None`. A flat or noise-only trace has zero edges and `freq` of `None`. Pure function, no I/O. Test: `square_wave_edges_and_frequency`.

### `bench_mcp_scope_read_waveform` [@ANCHOR: bench_mcp_scope_read_waveform]

`read_waveform(s, c)` (crate-visible) reads the channel's waveform memory and converts it to volts. Called by `capture`, `cursors` and `math`. The caller has already checked that the scope is running, or that a frozen frame is wanted.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `c` | `u8`, 1 or 2 | The channel (already validated by `ch`; this function does not check it). |

It sends `WFSU SP,1,NP,0,FP,0` (every point, no decimation; without it the unit returned only a few samples, found 2026-10-28), queries `C<c>:VDIV?`, `C<c>:OFST?` and `SARA?`, raises the link timeout to 30 s, asks `C<c>:WF? DAT2` as a binary block, then restores the timeout to 10 s whether or not the read worked. Each byte is a signed 8-bit sample converted as `sample * vdiv / 25 - offset` (25 counts per division). Returns `(Vec<f32>, f64)`: the volts and the sample rate in samples per second. Errors: any link error, a bad block header or length (from `Link::block`), and `empty waveform` when the block holds no bytes. Side effect: the scope's waveform-setup (`WFSU`) is left at all points; it is not put back. The 8-bit resolution limits accuracy to about 1/25 of a division per count. Tests: `math_reads_both_channels_from_a_frozen_frame_and_puts_the_scope_back` and the capture test.

### `bench_mcp_scope_capture` [@ANCHOR: bench_mcp_scope_capture]

`capture(s, args, out)` is `bench_capture` (tier `read_only`): reads one channel's waveform memory, writes it to a file and returns a summary.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `args.channel` | integer 1 or 2 | Required. |
| `args.allow_stopped` | boolean | Default false. A stopped scope holds an old frozen frame, so the call is refused unless this is exactly `true`. |
| `out` | `&Path` | The output directory (`BENCH_MCP_OUT`, or `bench_mcp` under the temp directory). |

Returns an object with `channel`, `samples`, `sample_interval_s` (1 / sample rate), `acquisition_was_stopped`, `min_v`, `max_v`, `mean_v`, `pkpk_v`, `rising_edges`, `estimated_frequency_hz` (null when fewer than two rising edges), `file` (the path) and `file_format` (`little-endian f32 volts`). The file is named `ch<channel>_<unix seconds>.f32` and holds the samples as consecutive little-endian 32-bit floats in volts. Errors: `channel must be 1 or 2`; the stopped-scope refusal (`the scope is stopped, so its waveform memory is an old frozen frame. Set trigger_mode AUTO (bench_set) or pass allow_stopped=true.`); everything `read_waveform` can return; `write <path>: <os error>`. Because the name has one-second resolution, two captures of the same channel in the same second write the same file (the later one replaces the earlier). The call sets the scope's waveform setup to all points (see `read_waveform`) and otherwise only reads. `spectrum` calls this function and deletes the file afterwards. Test: `a_capture_is_written_as_f32_volts_with_its_edges_and_a_stopped_scope_is_refused`.

### `bench_mcp_scope_spectrum` [@ANCHOR: bench_mcp_scope_spectrum]

`spectrum(s, args, out)` is `bench_scope_spectrum` (tier `read_config`, because it may change the memory depth and the timebase): the spectrum of one channel's waveform, computed on this host with the FFT in `spectrum.rs`, and the strongest peaks. More time per division gives finer frequency resolution but fewer samples per carrier cycle.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `args.channel` | integer 1 or 2 | Required. |
| `args.window` | text | `rect` (also `rectangular`, `none`), `hanning` (default, also `hann`), `hamming` or `blackman`, case-insensitive. |
| `args.points` | integer, power of two, 16 to 1048576 | How many of the first samples are transformed. Default 131072. |
| `args.peaks` | integer | How many peaks to list; default 10, clamped to 1 to 50. |
| `args.range_db` | number, dB | Peaks weaker than this many dB below the strongest are dropped. Default 60. |
| `args.memory_depth` | text such as `14K`, `140K`, `1.4M`, `14M` | If given, sent as `MSIZ <value>` and the call waits 1 s. At most 6 characters, only digits, `.`, `K` and `M`; anything else is refused here, and the scope refuses a depth it does not have. |
| `args.timebase_s` | number, seconds per division, finite and above 0 | If given, sent as `TDIV <value>S` with the number in exponent notation, and the call waits 1.5 s. |
| `args.allow_stopped` | boolean | Passed to `capture`. |
| `out` | `&Path` | Output directory (the temporary capture file is created and removed here). |

Returns `channel`, `window`, `points`, `sample_rate_hz`, `bin_hz`, `span_hz` (half the sample rate), `enbw_bins` (the window's equivalent noise bandwidth in bins), a `note` and `peaks`, a list of objects with `freq_hz`, `vrms` (volts rms at the scope input), `dbv` and `dbm_50` (dBV plus 13.01, the level a 50 ohm source would read into 50 ohms; the scope input itself is 1 megohm). With 8-bit samples, lines more than about 45 dB below the strongest are not reliable (the note says so). Errors: the argument checks above (`points must be a power of two from 16 to 1048576`, `memory_depth must look like 14K, 140K, 1.4M or 14M`, `timebase_s must be positive`), everything `capture` returns, and `the scope returned N samples, fewer than points = P; lower points or raise the memory depth`. With the default 131072 points a scope on a small memory depth returns that error. Side effects: the memory depth and the timebase stay changed on the scope; the capture file is written and then removed (not removed when an error is returned after the write). Blocks for up to about 2.5 s of fixed waits plus the capture. The memory-depth check here is looser than the whitelist of `bench_mcp_scope_acquire`. Test: `the_spectrum_tool_finds_the_tone_and_its_harmonic_through_a_scope_link`; see also `bench_mcp_spectrum` in `spectrum.rs`.

### `bench_mcp_scope_screenshot` [@ANCHOR: bench_mcp_scope_screenshot]

`screenshot(s, out)` is `bench_screenshot` (tier `read_only`): saves the scope screen as an image file.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `out` | `&Path` | Output directory. |

Sends `SCDP` and reads the raw reply with `Link::raw_until_idle` (stops after 1.5 s of silence, at most 30 s overall). The extension is chosen from the first bytes: `bmp` for `BM`, `png` for the PNG signature, otherwise `bin`. The file is `screen_<unix seconds>.<ext>` in `out`. Returns `{"file": path, "bytes": n}`. Errors: link errors; `screenshot: only N bytes` when fewer than 100 bytes arrive (a reply that short is an error text, not an image); the file-write error text. It does not change the scope. A dump can take several seconds, with the instrument lock held. Test: `a_screenshot_is_saved_by_its_image_type_and_a_short_reply_is_an_error`.

### `bench_mcp_scope_set` [@ANCHOR: bench_mcp_scope_set]

`set(s, args)` is the scope half of `bench_set` (tier `read_config`): timebase, vertical settings, trigger mode and trigger type. Only the keys present are acted on, in this order.

| Argument | Type | Meaning |
|---|---|---|
| `args.timebase_s` | number | Sent as `TDIV <value>S` (exponent form, seconds per division). Not range-checked here; the scope decides. |
| `args.channel` | integer 1 or 2 | Required when any of the next four keys is given (and for `trigger_type`). |
| `args.vdiv_v` | number, volts per division | `C<c>:VDIV <value>V`. |
| `args.offset_v` | number, volts | `C<c>:OFST <value>V`. |
| `args.coupling` | `A1M`, `D1M` or `GND` | `C<c>:CPL <value>`; anything else is `bad coupling`. |
| `args.attenuation` | number | Probe ratio, `C<c>:ATTN <value>`. |
| `args.trigger_mode` | `AUTO`, `NORM`, `SINGLE`, `STOP` | `TRIG_MODE <value>`; anything else is `bad trigger_mode`. |
| `args.trigger_type` | `EDGE`, `SLEW`, `GLIT`, `INTV`, `RUNT`, `TV` | `TRSE <type>,SR,C<c>`. These are the types firmware 1.3.27 accepted when probed; pattern, window, drop-out and serial-bus triggers were not accepted with the tokens tried (licensed options or other names). Any other value is refused with a message listing them. |

Returns `{"sent": [the commands in order], "status": <status object>}` after a 500 ms pause, so the answer shows what the scope reports now. Errors: `channel must be 1 or 2` (when a channel-dependent key is present without a valid channel), `bad coupling`, `bad trigger_mode`, the `trigger_type` message, link errors. Commands already sent before an error stay in effect (no rollback). Numbers are not checked for being finite or in range by this function, so a non-finite or nonsensical value is passed through to the scope. Calling it with no recognised key sends nothing and just returns the status. Test: `settings_become_the_scopes_commands_and_bad_values_are_refused_before_sending`.

### `bench_mcp_scope_self_cal` [@ANCHOR: bench_mcp_scope_self_cal]

`self_cal(s, args)` is `bench_scope_self_cal` (tier `full`): starts the scope's self-calibration (`*CAL?`), which takes a minute or two and may reset the vertical settings. It needs every probe unplugged, so a person at the bench must confirm.

| Argument | Type | Meaning |
|---|---|---|
| `args.dry_run` | boolean, null or absent | `true` returns the plan and sends nothing; false, null or absent runs the real call; any other type is `dry_run must be true or false`. |
| `args.confirm_inputs_disconnected` | boolean | Must be exactly `true` for the real call, given only after the person at the bench has unplugged every probe. |

The dry run returns `{"dry_run": true, "sent": [], "would": [...], "note": ...}`. The real call raises the link timeout to 300 s, sends `*CAL?`, restores 10 s, and returns `{"cal_reply": <reply>, "note": ...}`; the note says 0 means the scope accepted the command, that calibration may keep running, and to wait before measuring. Errors: the dry-run type error; `refused: confirm_inputs_disconnected must be true, after the person at the bench has unplugged every probe`; link errors (the timeout is restored before the error is returned). The call blocks, with the instrument lock held, until the scope answers (up to 300 s). The scope may reset its vertical settings. Test: `self_cal_needs_the_bench_persons_confirmation_and_a_dry_run_sends_nothing`.

## The scope: channel math, cursors and settings (`src/scope_more.rs`)

These functions compute on the host from the waveform memory (the scope's own math and cursor commands were not probed) or set acquisition, channel and display values and read each back so the answer shows what the scope accepted.

### `bench_mcp_scope_math` [@ANCHOR: bench_mcp_scope_math]

`math(s, args, out)` is `bench_scope_math` (tier `read_config`, because it may stop and restart acquisition): channel 1 `op` channel 2, sample by sample.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `args.op` | `add`, `sub`, `mul`, `div` | Required; anything else is `op must be add, sub, mul or div (channel 1 op channel 2)`. |
| `out` | `&Path` | Output directory. |

It reads `TRIG_MODE?`. If the scope is not stopped it sends `TRIG_MODE STOP` so both channels come from the same frame, reads channel 1 then channel 2 with `read_waveform`, and puts the trigger mode back (`AUTO`, `NORM` or `SINGLE` only) even if a read failed. If the mode before was some other word the scope is left stopped. The two sample rates must agree within one part in a million, otherwise `the channels were read at different sample rates (A and B Sa/s)`. The result is written to `math_<op>_<unix seconds>.f32` as little-endian f32 and returned with `op` (for example `C1 sub C2`), `samples` (the shorter channel's count), `sample_interval_s`, `frame_frozen_for_the_reading`, `undefined_points`, `min`, `max`, `mean`, `rms`, `file` and `file_format` (volts; volts squared for `mul`; a ratio for `div`, 0 where channel 2 was under 1 microvolt). Errors: the op message, link errors, the sample-rate mismatch, the file-write error. Side effects: a running scope is stopped for the duration of both reads (a few seconds at most), then restarted; the waveform setup is left at all points. Test: `math_reads_both_channels_from_a_frozen_frame_and_puts_the_scope_back`.

### `bench_mcp_scope_combine` [@ANCHOR: bench_mcp_scope_combine]

`combine(op, a, b)` (crate-visible) does the arithmetic of `math`.

| Argument | Type | Meaning |
|---|---|---|
| `op` | `&str` | `add`, `sub`, `mul`, or anything else, which means divide (the caller has already validated the four names). |
| `a`, `b` | `&[f32]`, volts | The two channels' samples. |

Returns `(Vec<f32>, usize)`: the results (as many as the shorter input, because the pairs are zipped) and the number of divisions that were by a channel-2 value with magnitude under 1e-6 (1 microvolt), which give 0 instead of a huge or non-finite number. No errors, no panic, no I/O. Test: `math_reads_both_channels_from_a_frozen_frame_and_puts_the_scope_back`.

### `bench_mcp_scope_value_at` [@ANCHOR: bench_mcp_scope_value_at]

`value_at(v, sara, t)` (crate-visible) returns the waveform value at time `t` by linear interpolation.

| Argument | Type | Meaning |
|---|---|---|
| `v` | `&[f32]`, volts | The samples; must hold at least one (an empty slice would underflow `len() - 1`; the only caller passes a non-empty read). |
| `sara` | `f64`, samples per second | Sample rate. |
| `t` | `f64`, seconds | Time from the first sample (not from the trigger). |

Returns `Some(volts)` when `t * sara` is finite and between 0 and the last sample index inclusive, otherwise `None` (before the record, after it, or not finite). Between two samples the result is the weighted mean; at the last sample it is that sample. Pure function. Test: `cursors_interpolate_and_two_of_them_give_the_differences`.

### `bench_mcp_scope_cursors` [@ANCHOR: bench_mcp_scope_cursors]

`cursors(s, args)` is `bench_scope_cursors` (tier `read_only`): the value of a channel at up to eight times, and with exactly two times the differences and the frequency.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `args.channel` | integer 1 or 2 | Required. |
| `args.times_s` | array of 1 to 8 finite numbers, seconds | Required; measured from the first sample of the waveform memory. |
| `args.allow_stopped` | boolean | As for `capture`. |

Returns `channel`, `record_s` (length of the record), `sample_interval_s`, `cursors` (a list of `{time_s, volts}` where `volts` is null outside the record) and a `note`. With exactly two times and both inside the record it adds `delta_time_s` (second minus first), `delta_volts` (second minus first) and `frequency_hz` (1 / the absolute time difference, null when the difference is 0). Errors: `channel must be 1 or 2`; `times_s is required: one to eight times in seconds from the first sample`; `times_s must be numbers` (also for non-finite); `times_s must hold one to eight times`; the stopped-scope refusal; link errors. Does not change the scope other than the waveform setup of `read_waveform`. Test: `cursors_interpolate_and_two_of_them_give_the_differences`.

### `bench_mcp_scope_acquire` [@ANCHOR: bench_mcp_scope_acquire]

`acquire(s, args)` is `bench_scope_acquire` (tier `read_config`): acquisition mode and memory depth; with no argument it only reads.

| Argument | Type | Meaning |
|---|---|---|
| `args.mode` | `sampling`, `peak_detect`, `average`, `high_resolution` | Sent as `ACQW SAMPLING`, `PEAK_DETECT`, `AVERAGE` or `ERES`. Anything else: `mode must be sampling, peak_detect, average or high_resolution`. |
| `args.average_count` | integer, power of two, 4 to 1024; default 16 | Used only with `average`, sent as `AVGA <n>` before the mode. Otherwise `average_count must be a power of two from 4 to 1024`. |
| `args.memory_depth` | one of `7K`, `70K`, `700K`, `7M`, `14K`, `140K`, `1.4M`, `14M` | Sent as `MSIZ <value>`. The last four apply on the two-channel-off sheet, per the error text. Anything else is refused. |

Each command is sent, followed by 100 ms and a read-back query (`ACQW?`, `AVGA?`, `MSIZ?`), with a 3 s timeout per read-back. Returns `sent`, `readback` (each query with the scope's answer, `(empty reply)`, or `no answer to <query>: <error>`), a `note` that a setting the scope did not accept shows its old value or no answer and that these commands come from the SDS1000X-E programming guide and are not all confirmed on firmware 1.3.27, plus `sample_rate_sa_s`, `memory_depth` and `mode` read after the changes. Errors: the three argument messages and link errors on sends; a failed read-back is reported inside the result, not as an error. Commands already sent stay in effect if a later one fails. Test: `settings_send_the_documented_commands_read_them_back_and_refuse_bad_values_unsent`.

### `bench_mcp_scope_channel` [@ANCHOR: bench_mcp_scope_channel]

`channel(s, args)` is `bench_scope_channel` (tier `read_config`): bandwidth limit and inversion of a channel.

| Argument | Type | Meaning |
|---|---|---|
| `args.channel` | integer 1 or 2 | Required. |
| `args.bandwidth_limit` | boolean, null or absent | `true` sends `BWL C<c>,ON`, `false` `BWL C<c>,OFF`. |
| `args.invert` | boolean, null or absent | `C<c>:INVS ON` or `OFF`. |

Any other type is `<key> must be true or false`. Each command is followed by a 100 ms pause and a read-back (`BWL?`, `C<c>:INVS?`). Returns `sent`, `readback`, a `note` and, always, `bandwidth_limit` (`BWL?` read now; the query is not per channel in the code, so the reply is whatever the scope returns for it) and `invert` (`C<c>:INVS?`). With neither key the call is a read. Errors: `channel must be 1 or 2`, the type message, link errors on sends. Test: `settings_send_the_documented_commands_read_them_back_and_refuse_bad_values_unsent`.

### `bench_mcp_scope_display` [@ANCHOR: bench_mcp_scope_display]

`display(s, args)` is `bench_scope_display` (tier `read_config`): screen saver, grid style and intensities.

| Argument | Type | Meaning |
|---|---|---|
| `args.xy`, `args.dot_join`, `args.menu` | boolean | `XYDS`, `DTJN`, `MENU` `ON` or `OFF`, through `item_cmds`. |
| `args.persistence` | `OFF`, `INFINITE`, `1`, `5`, `10`, `30` (a number is accepted) | `PESU <value>`. |
| `args.screen_saver` | `OFF`, `1MIN`, `5MIN`, `10MIN`, `15MIN`, `30MIN`, `60MIN` | Sent as `SCSV <value>`. Other values: `screen_saver must be OFF, 1MIN, 5MIN, 10MIN, 15MIN, 30MIN or 60MIN`. |
| `args.grid` | `FULL`, `HALF`, `OFF` | `GRDS <value>`; otherwise `grid must be FULL, HALF or OFF`. |
| `args.trace_intensity`, `args.grid_intensity` | integer 0 to 100 | Sent together as `INTS GRID,<g>,TRACE,<t>`. Giving one without the other is `trace_intensity and grid_intensity go together`; out of range or not whole is `<key> must be a whole number from 0 to 100`. |

Returns the same shape as `acquire` (`sent`, `readback`, `note`) plus the current `screen_saver`, `grid` and `intensity` read back (`SCSV?`, `GRDS?`, `INTS?`). With no key it is a read. Errors as listed, plus link errors on sends. Test: `settings_send_the_documented_commands_read_them_back_and_refuse_bad_values_unsent`.

### `bench_mcp_scope_item_cmds` [@ANCHOR: bench_mcp_scope_item_cmds]

`item_cmds(args, items, c)` turns the arguments of a scope tool into the commands that set them, checking every value first. It is the shared part of the tools written from the programming guide: each tool lists its settings as `Item { arg, cmd, ty }` and this function does the rest.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The tool arguments; an item whose key is absent or null is skipped. |
| `items` | `&[Item]` | The table of settings: argument name, command header (a `{c}` in it becomes the channel) and the type of the value. |
| `c` | `u8` | The channel for `{c}`. |

Types: a JSON boolean becomes `ON` or `OFF`; a word must be one of the listed ones (any case, sent upper case; a whole number counts as its digits, so `5` is the word `5`); a number must lie in its range and is sent as `1E-3` followed by the unit; a whole number must lie in its range. Returns `(command, query)` pairs such as `("XYDS ON", "XYDS?")`, or the first error, `<arg> must be <what>` (for example `persistence must be one of OFF, INFINITE, 1, 5, 10, 30`). Nothing is sent here, so one bad value refuses the call before the scope sees any command. Pure: no I/O. Test: `the_display_acquire_and_channel_tools_work_against_the_fake_sds1000x_e` (a bad value among good ones sends nothing).

### `bench_mcp_scope_read_items` [@ANCHOR: bench_mcp_scope_read_items]

`read_items(s, items, c)` asks the scope for the current value of every setting in an item table and returns a JSON object from argument name to the scope's answer, so a tool shows what the scope holds after a change and not only what was sent.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `items` | `&[Item]` | The settings to read; each is asked as its command header followed by `?`. |
| `c` | `u8` | Channel for `{c}`. |

Each answer is the scope's reply text, `(empty reply)`, or `no answer to <query>: <error>` after a 3 s timeout (a setting this firmware does not know is reported that way, not as an error of the call). Reads only; it changes nothing on the scope. Test: the same as `item_cmds`, which checks the `more` object of the display tool.

## The scope: functions from the programming guide (`src/scope_guide.rs`)

These tools are written from the SDS1000X-E programming guide (EN02E) and tested against `src/fake_scope.rs`, a fake scope that answers as the guide shows. Each sends its commands, reads the settings back and returns what the scope reports, because the real unit's answers have not yet been compared with the guide. The commands and replies that need a lab check are listed in the to-do `bench-mcp-scope-remaining-functions`.

### `bench_mcp_scope_history` [@ANCHOR: bench_mcp_scope_history]

`history(s, args, out)` is `bench_scope_history` (tier `read_config`): history mode, frame list, frame select and frame read-out.

| Argument | Type | Meaning |
|---|---|---|
| `args.history`, `args.list` | boolean | `HSMD ON/OFF`, `HSLST ON/OFF` (the list works only while history is on). Other types: `<key> must be true or false`. |
| `args.frame` | integer 0 to 100000 | `FRAM <n>`. |
| `args.frames` | array of up to 20 integers 0 to 100000 | Each is selected (`FRAM <n>`, then 100 ms), its `FTIM?` read, and its waveform read like `capture` does. Anything else: `frames must be up to 20 frame numbers from 0 to 100000`. |
| `args.channel` | integer 1 or 2 | Required with `frames`. |
| `args.save` | boolean | Also saves each frame as little-endian f32 volts in `out`. |
| `out` | `&Path` | Directory for saved frames. |

Every value is checked before the first command is sent. Returns `sent`, `readback`, `note`, `settings` (read back `history`, `list`, `frame`) and `frames` (per frame: `frame`, `time`, `samples`, `sample_interval_s`, `min_v`, `max_v`, `pkpk_v`, `file`). The frame number the scope can reach is its own: a larger one is clamped or ignored by the scope and the read-back shows it. The recording of frames is not started here. Test: `history_turns_on_reads_frames_with_their_times_and_refuses_bad_frames_unsent` against the fake scope.

## The bench supply (`src/psu.rs`)

The Abestop ATM6301 (0 to 60 V, 0 to 10 A) is driven over its CH340 serial port, SCPI at 115200 8N1. The configured `max_volts` and `max_amps` cap every setpoint. The output is switched on only if the setpoints already stored in the supply are within those caps. Switching the output on is the `full` tier tool `bench_psu_output` and needs `dry_run` or `confirm_load_connected`; this process switches the output off again after `for_s` seconds (at most the configured `max_on_s`, default 600 s per the tool text), because the driver knows of no output timer in the supply. The data types in this file are `Limits` (`max_volts` volts, `max_amps` amperes, `max_on_s` seconds, taken from the instrument's config entry by `call`) and `Timer`, described in the entry of `bench_mcp_psu_arm_switch_off`.

### `bench_mcp_psu_is_off` [@ANCHOR: bench_mcp_psu_is_off]

`is_off(reply)` (private) decides whether the supply's answer to `OUTP?` means the output is off. It is the proof used by `safe_state`: the supply must say off itself; a missing or odd answer is never taken as off.

| Argument | Type | Meaning |
|---|---|---|
| `reply` | `&str` | The line the supply returned. |

Returns true when the trimmed, upper-cased reply is `OFF` or `0`; anything else (including `ON`, `1`, an empty line, an error text) is false. Its sibling `is_on` accepts `ON` or `1` and is used by `state`; a reply that is neither is shown as `off` by `state` but is not accepted as proof of off by `safe_state`. Pure function. Test: `output_reply_spellings`.

### `bench_mcp_psu_measure` [@ANCHOR: bench_mcp_psu_measure]

`measure(l)` is the supply half of `bench_measure` (tier `read_only`): what the supply is delivering now.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | Open serial link to the supply (`\r\n` line ending). |

Queries `MEAS:VOLT?`, `MEAS:CURR?`, `MEAS:POW?` and `MEAS:ALL:INFO?`. Returns `{"voltage_v", "current_a", "power_w", "info"}` where the first three are numbers (units stripped by `Link::number`) and `info` is the supply's own text line. Errors: link errors (the 10 s default timeout) and `<command>: not a number: <reply>`. Read-only; lock held. Test: `a_measurement_reads_volts_amps_watts_and_the_info_line`. The sibling `state` (not anchored) is the supply half of `bench_status` and adds the output state, setpoints, over-voltage and over-current limits and `SYST:ERR?`.

### `bench_mcp_psu_caps` [@ANCHOR: bench_mcp_psu_caps]

`in_range(name, v, max)` (private) enforces the configured cap on a setpoint before any command is sent.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` | `voltage_v` or `current_a`, used in the message. |
| `v` | `f64` | The requested value (volts or amperes). |
| `max` | `f64` | The cap from the server's config (`max_volts` or `max_amps`). |

Returns `Ok(v)` when `v` is finite and in `0.0..=max` (both ends allowed), otherwise `<name> <v> is outside 0..=<max> (the cap in the server's config)`. NaN and the infinities are refused. Pure function; it is the only check in `set`, so it is what keeps a setpoint below the caps. Tests: `caps_reject_out_of_range_and_non_finite`, `setpoints_are_sent_only_within_the_caps_and_something_must_be_given`.

### `bench_mcp_psu_set` [@ANCHOR: bench_mcp_psu_set]

`set(l, args, lim)` is the supply half of `bench_set` (tier `read_config`): voltage and current setpoints. It never changes the output state; a supply that is on keeps delivering at the new setpoint.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | Open serial link to the supply. |
| `args.voltage_v` | number, volts | Sent as `VOLT <v>` with three decimals, after the cap check. |
| `args.current_a` | number, amperes | Sent as `CURR <a>` with three decimals. |
| `lim` | `&Limits` | Provides the two caps. |

Returns `{"sent": [the commands], "state": <state object>}` after a 300 ms pause; the state is the measured and stored values, so the caller sees what the supply accepted (the supply may round or reject). Errors: the `bench_mcp_psu_caps` message, `give voltage_v and/or current_a` when neither is a number, link errors. Both values are checked against the caps before anything is sent, so a refused current leaves the voltage unchanged. A key that is present but not a number is an error (`<key> must be a number`); null counts as absent. Test: `setpoints_are_sent_only_within_the_caps_and_something_must_be_given`.

### `bench_mcp_psu_safe_state` [@ANCHOR: bench_mcp_psu_safe_state]

`safe_state(l, timer)` is `bench_psu_safe_state` (tier `read_config`), and is also what `bench_psu_output` with `on: false` and the timed switch-off use. Safe at any time; use it first when anything went wrong.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | Open serial link to the supply. |
| `timer` | `&Timer` | The supply's timer state; its generation counter is bumped first so an explicit off cancels a pending timed switch-off that would now have nothing to do. |

Up to three tries (`OFF_TRIES`): send `OUTP OFF`, wait 300 ms, query `OUTP?`, and succeed only if `is_off` accepts the reply. Returns `{"output": "off", "verified": true}`. If all tries fail it returns the error `SAFE STATE NOT CONFIRMED: the supply output may still be ON (<last cause>). Do this now: press the output button on the supply or switch off its mains. Tell the owner before using it again.` The error is deliberately loud and gives the physical remedy. A send error counts as a failed try and the next try goes on. Worst case about one second plus the link timeouts. Tests: `safe_state_reads_off_back_and_fails_loudly_when_the_supply_stays_on`, `an_explicit_off_cancels_the_pending_timer`.

### `bench_mcp_psu_on_seconds` [@ANCHOR: bench_mcp_psu_on_seconds]

`on_seconds(args, lim)` works out how long the output may stay on after one `bench_psu_output` call.

| Argument | Type | Meaning |
|---|---|---|
| `args.for_s` | number, seconds, 1 to `max_on_s`, or null/absent | Requested on-time. |
| `lim.max_on_s` | `f64`, seconds | The ceiling from the instrument's config entry. |

Returns `Ok(for_s)` for a value from 1 to `max_on_s` inclusive, and `Ok(max_on_s)` when `for_s` is absent or null, so the on-time is always bounded. Errors: `for_s must be a number of seconds`; `for_s <s> is outside 1..=<max_on_s> (max_on_s in the server's config)`, which also catches non-finite values. Pure function. Test: `the_time_limit_defaults_to_the_cap_and_never_exceeds_it`.

### `bench_mcp_psu_stored_setpoints` [@ANCHOR: bench_mcp_psu_stored_setpoints]

`check_stored_setpoints(l, lim)` (private) refuses to go on when the setpoints already stored in the supply exceed the caps (a value set from the front panel may have been left there, and the cap check in `set` never saw it).

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | Open serial link to the supply. |
| `lim` | `&Limits` | Provides `max_volts` and `max_amps`. |

Queries `VOLT?` and `CURR?`. Returns `Ok((volts, amps))` when both are finite and not above the caps. Errors: link or number-parse errors, and `refused: the supply is set to <v> V / <a> A, above the caps <max_v> V / <max_a> A. Set lower values with bench_set first.` The comparison is "above", so a stored value equal to the cap passes. Read-only. Used by `plan_on` and `output_on`. Test: `a_dry_run_reads_the_setpoints_and_switches_nothing`.

### `bench_mcp_psu_plan_on` [@ANCHOR: bench_mcp_psu_plan_on]

`plan_on(l, lim, for_s)` is the dry run of `bench_psu_output` with `on: true` and `dry_run: true`: it tells what would happen without switching or setting anything.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | Open serial link to the supply. |
| `lim` | `&Limits` | The caps. |
| `for_s` | `f64`, seconds | The on-time already computed by `on_seconds`. |

Reads the stored setpoints (`check_stored_setpoints`) and returns `{"dry_run": true, "sent": [], "would_be_refused": <null or the refusal text>, "stored_voltage_v", "stored_current_a" (null when refused), "would": ["OUTP ON", "OUTP OFF by this server after <for_s> s", "OUTP? (must read OFF)"], "note": ...}`. The note says nothing was switched and that the real call also needs `confirm_load_connected: true` from the person at the bench. A refusal by the caps is reported inside the result, not as an error; link errors while reading setpoints are also reported there as `would_be_refused` because the helper's error is captured. Reads only. Test: `a_dry_run_reads_the_setpoints_and_switches_nothing`.

### `bench_mcp_psu_output_on` [@ANCHOR: bench_mcp_psu_output_on]

`output_on(l, lim, for_s, timer, reopen)` switches the supply output on and arms the switch-off. This is the one function in this story that can drive an external load, which is why `bench_psu_output` is tier `full` and `psu_output` in `main.rs` has already required `confirm_load_connected: true` (or taken the `dry_run` path) before this is reached.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | Open serial link to the supply. |
| `lim` | `&Limits` | The caps. |
| `for_s` | `f64`, seconds | How long to leave the output on (from `on_seconds`, so within `max_on_s`). |
| `timer` | `&Timer` | The supply's timer state and instrument lock. |
| `reopen` | closure returning `Result<Link, String>`, `Send + 'static` | Gives the timer thread its own link to the supply (the real caller opens the configured serial port again). |

Order of work: check the stored setpoints (`check_stored_setpoints`; a refusal ends the call before any output command); increase the timer generation (so any earlier timer becomes void); send `OUTP ON`; start the switch-off thread (`arm_switch_off`); wait 500 ms; read the state. Returns the state object plus `off_after_s` (the on-time). Errors: the stored-setpoint refusal, link errors (a failed `OUTP ON` send returns before the timer is armed). It does not itself check the confirmation; `psu_output` does. The switch-off lives in this process: if the server process ends before the time is up, nothing switches the output off (the supply has no output timer the driver uses), so `bench_psu_safe_state` or the front-panel button is the remedy. Tests: `the_output_is_refused_over_the_caps_and_switched_off_by_the_timer`.

### `bench_mcp_psu_arm_switch_off` [@ANCHOR: bench_mcp_psu_arm_switch_off]

`arm_switch_off(generation, secs, timer, reopen)` (private) starts the background thread that puts the output off again. The data type `Timer { lock: Arc<Mutex<()>>, generation: Arc<AtomicU64> }` (`Clone`, `Default`) belongs to this entry: `lock` is the same mutex as the instrument lock taken by `call`, so a timed off and a tool call never talk to the serial port at once; `generation` is a counter that increases whenever an output command (`output_on`, `safe_state`) supersedes a pending switch-off. One `Timer` exists per instrument name (`psu_timer` in `main.rs`, kept in a process-wide map).

| Argument | Type | Meaning |
|---|---|---|
| `generation` | `u64` | The counter value this timer was armed under. |
| `secs` | `f64`, seconds | Delay before the off. |
| `timer` | `Timer` | Owned clone of the supply's timer state. |
| `reopen` | closure returning `Result<Link, String>` | Opens a fresh link. |

The thread sleeps `secs`, takes the instrument lock (waiting for any call in progress; a poisoned lock is used anyway), and returns without doing anything if the generation counter has moved on (an explicit off, or a newer on, took over). Otherwise it makes up to three attempts (`OFF_TRIES`), each opening a new link and running `safe_state` on it, 2 s apart. If all fail it writes `bench_mcp: the supply output could not be switched off after its time limit: <cause>` to standard error; nothing else reports it, so a failed timed off is only visible there and in the next `bench_status`. The thread is detached: not joined, not cancelled, gone when the process exits. No return value. Test: `the_output_is_refused_over_the_caps_and_switched_off_by_the_timer`.

## The VNWA's sweep, status and power cycle (`src/vna.rs`)

The VNWA 3E is driven through the `vnwa_open` crate: USB control transfers to the unit and the audio of its two codecs, no vendor software. Results are raw (uncalibrated) ratios: S21 from the RX channel and S11 from the reflection channel, in the vendor program's convention. A sweep needs tier `read_config` because it puts RF on the TX port. Constants: `MAX_POINTS` 401 (each point about 0.2 s) and `MAX_HZ` 1.3 GHz. The struct `SweepArgs { start_hz: f64, stop_hz: f64, points: usize, config: Config }` is what `sweep_args` returns: the checked frequencies in hertz, the number of points, and the `vnwa_open::sweep::Config` this sweep uses (the instrument's configured settings with any per-call overrides; fields include `if_hz`, `point_ms`, `rf_level`, `hub`, `retries`). It is built per call, owned by the caller and never shared.

### `bench_mcp_vna_number` [@ANCHOR: bench_mcp_vna_number]

`number(args, key)` (private) reads an optional numeric argument.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The tool arguments. |
| `key` | `&str` | Which key to read. |

Returns `Ok(None)` when the key is absent or null, `Ok(Some(x))` for a finite number, and `<key> must be a number` for anything else (a string, a boolean, NaN or infinity cannot be represented in JSON in any case). No side effects. Used by `sweep_args`. Test: `sweep_arguments_are_checked_and_override_the_instrument_settings`. (`vna_time.rs` has its own copy, `bench_vna_time_number`.)

### `bench_mcp_vna_sweep_args` [@ANCHOR: bench_mcp_vna_sweep_args]

`sweep_args(args, base)` checks the arguments of one `bench_vna_sweep` call and merges them over the instrument's configured settings. `main.rs` calls it before the unit is looked for, so a bad argument is refused without touching the hardware.

| Argument | Type | Meaning |
|---|---|---|
| `args.start_hz` | number, hertz, above 0 and at most 1.3e9 | Required. |
| `args.stop_hz` | number, hertz, same range | Default: `start_hz` (a single frequency). |
| `args.points` | whole number, 1 to 401 | Default 1. |
| `args.if_hz` | number, hertz | Measurement IF override; 48 kHz divided by it must be a whole number of at least 4 samples (12, 6, 4 kHz work); checked by `vnwa_open::sweep::check`. |
| `args.point_ms` | number, milliseconds | Time per point override, range checked by `check`. |
| `args.rf_level` | whole number, 0 to 16383 | The unit's 14-bit RF level code. |
| `base` | `&Config` | The configured settings, cloned and not modified. |

Returns `SweepArgs`. Errors: `start_hz is required`; `points <p> must be a whole number from 1 to 401`; `<name> <f> is outside 0 to 1300000000 Hz` (the lower end is exclusive: 0 is refused); `rf_level <x> must be a whole number from 0 to 16383`; the `number` message; any message from `vnwa_open::sweep::check` for the final merged settings (IF divisibility, point time, the analysis window). A reversed range (`stop_hz` below `start_hz`) is not refused here. Pure function, no I/O. Test: `sweep_arguments_are_checked_and_override_the_instrument_settings`.

### `bench_mcp_vna_csv` [@ANCHOR: bench_mcp_vna_csv]

`csv(points)` renders a sweep as CSV text. Also used by the NanoVNA driver (`nanovna.rs`) so both write one format, and `bench_vna_time_domain` reads it back.

| Argument | Type | Meaning |
|---|---|---|
| `points` | `&[Point]` | The measured points (`freq_hz`, complex `s21` and `s11`, plus per-point quality numbers that are not written). |

Returns a `String`: the header `freq_hz,s21_re,s21_im,s21_db,s21_deg,s11_re,s11_im,s11_db,s11_deg` and one line per point, newline terminated. Frequency is written as the float's default text; real and imaginary parts with 6 decimals, dB (20 log10 of the magnitude, so minus infinity for an exact zero) and degrees with 3. Pure function. Test: `the_csv_has_a_header_and_one_row_per_point`.

### `bench_mcp_vna_calibrated_csv` [@ANCHOR: bench_mcp_vna_calibrated_csv]

`calibrated_csv(points, corrected)` renders the calibrated values of a sweep as CSV text.

| Argument | Type | Meaning |
|---|---|---|
| `points` | `&[Point]` | Source of the frequencies. |
| `corrected` | `&[(Option<Complex>, Option<Complex>)]` | Per point, the calibrated S11 and S21; `None` where the calibration does not cover that part. |

Returns a `String`: header `freq_hz,s11_re,s11_im,s11_db,s11_deg,s21_re,s21_im,s21_db,s21_deg` then one line per point (zipped, so the shorter list wins). Each missing part is four empty cells. Note the column order is S11 first, unlike the raw CSV, which has S21 first: `bench_vna_time_domain` therefore refuses this file (it reads the raw columns by name). Pure function. Test: `the_csv_has_a_header_and_one_row_per_point`.

### `bench_vna_ensure_present` [@ANCHOR: bench_vna_ensure_present]

`ensure_present(tool, present)` makes the tools that need the VNWA fail early with a typed `device not present` error, before anything else is attempted.

| Argument | Type | Meaning |
|---|---|---|
| `tool` | `&str` | The tool name. |
| `present` | `&dyn Fn() -> bool` | Says whether the unit is on the USB bus (`vnwa_open::power::unit_present` in production; tests give a fake). |

Tools that need the unit: `bench_vna_sweep`, `bench_vna_generator_on`, `_off`, `_level`, `_am`, `_fm`, `bench_vna_meter`, `bench_vna_spectrum`, `bench_vna_calibrate_measure`. Power cycling, calibration building, identify and status do not. Returns `Ok(())` when the tool does not need it or the unit is present; otherwise `device not present: the VNWA (<vid>:<pid>) is not on the USB bus (not connected, or switched off at its hub port); <tool> was not attempted`. The lookup is by name in a fixed list, so a new tool must be added there. Only reads the bus. Test: `an_absent_vnwa_fails_its_tools_with_a_typed_error_and_leaves_the_rest_alone`.

### `bench_vna_status` [@ANCHOR: bench_vna_status]

`status(cfg, hw)` is `bench_status` for the VNWA: what is connected and the settings a sweep starts from. `hw` is a `Backend` (the trait that reaches the unit, the codecs and the sound server): `Hardware` in production, a simulated unit in the tests.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | The instrument's configured settings; codec names given here win over the ones found. |
| `hw` | `&dyn Backend` | Where the unit, the codec ids and the sound-server state come from. |

Returns a JSON object: `usb` ("present", "present but not answering" or "not found or no permission") with `usb_read` (the two bytes of a harmless read, request 0x3a, or the error), `instrument_monitoring` (firmware and temperature, null unless the read answered), the codec ids (`codec_s11`, `codec_s21`, null with `codecs_error` when none were found), `codecs_protected_from_sound_server` with `codecs_protection_detail` and `codecs_protection_warning`, the hub, the power cycles this server did, `if_hz`, `point_ms`, `rf_level`, and `calibrated: false`. It never fails for a missing unit; that is reported in the value. Reads only (a USB control read, the USB tree, the sound server); it transmits nothing. Test: `status_reports_the_unit_its_codecs_and_the_sound_server_without_a_real_unit`.

### `bench_vna_sweep` [@ANCHOR: bench_vna_sweep]

`sweep(cfg, args, out_dir, hw)` is `bench_vna_sweep` (tier `read_config`: it puts RF on the TX port). It checks the arguments (`sweep_args`), opens the unit and both codecs through `hw.open_vna`, measures the points, writes the raw CSV to `out_dir/vna_sweep_<unix seconds>.csv` and, when `calibration` and `cable` are given, a corrected `..._calibrated.csv` (refused with "cable is required with calibration" when `cable` is missing, and by the calibration check when the settings differ).

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | The instrument's settings; the call's arguments override them. |
| `args` | `&Value` | `start_hz`, `stop_hz`, `points`, optional settings, `calibration`, `cable`. |
| `out_dir` | `&Path` | Where the CSV files go. |
| `hw` | `&dyn Backend` | `Hardware` in production, a simulated unit in the tests. |

Returns the point count, the span, the S21 minimum and maximum, the rows (up to 51 points), the file paths, the recovery tally, the worst IF error and level change, the last recovered failures, and the unit's temperature after the sweep. A bad argument never opens the unit; a failed point is the error of the whole sweep (no number from a failed attempt). Test: `a_sweep_measures_the_points_and_writes_the_csv_with_a_simulated_unit`.

### `bench_vna_instrument_monitoring` [@ANCHOR: bench_vna_instrument_monitoring]

`instrument_monitoring(u)` reports the unit's firmware version and internal temperature sensor (the vendor's Instrument Monitoring), used inside `status`.

| Argument | Type | Meaning |
|---|---|---|
| `u` | `&vnwa_open::usb::Vnwa` | An open USB handle to the unit. |

Returns `{"firmware": "v<major>.<minor>", "temperature_counts": n, "temperature_note": ...}`. The temperature is in raw, uncalibrated counts that rise as the unit warms (the doc comment gives about 25 counts in the first minute after power-up, then slowly), so only changes mean anything. A failed firmware read gives `{"error": ...}`; a failed temperature read gives `{"firmware": ..., "temperature_error": ...}`; a failure is always reported in the value, never as a number. Performs USB control reads. Test: `the_vna_power_hook_runs_the_hub_modules_ladder_and_lists_every_failed_rung` cites it; unknown whether any test covers the successful read on hardware.

### `bench_vna_power_hook` [@ANCHOR: bench_vna_power_hook]

`install_power_hook()` makes `vnwa_open`'s own power cycles (the recovery inside a measurement, the generator-off recovery after a modulation) run the USB hub module's implementation, the same code as `bench_vna_power_cycle` and `bench_usb_port_power`. `call` runs it before every VNA tool (idempotent).

Takes no arguments, returns nothing. It calls `vnwa_open::power::set_cycle_hook` with the hook from `power_hook(ProcessRunner, "/sys")` and ignores the result, so a hook that is already set stays set. The helper `power_hook(run, sysfs)` is the testable part: it builds a closure that turns the hub port (`hub`, `port`) and the ladder of (off time, back timeout) pairs into the hub module's port reference and options, and runs `usb_hub::power::cycle_escalating` with `vnwa_open::power::unit_present` as the readiness test, returning an error when every rung fails. Side effects when the hook later runs: switches a USB hub port off and on through `uhubctl` (a subprocess) and sysfs, which cuts the unit's power for the off time. Nothing here transmits. Test: `the_vna_power_hook_runs_the_hub_modules_ladder_and_lists_every_failed_rung`.

### `bench_vna_power_cycle` [@ANCHOR: bench_vna_power_cycle]

`power_cycle(cfg, args)` is `bench_vna_power_cycle` (tier `read_config`): cold-resets the unit through its hub port, waits for it and its codecs, and by default checks it with a one-point sweep. Everything the unit holds is lost: a generator that was on is off afterwards.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | Must carry `hub` (`"hub"` and `"hub_port"` in the instrument's entry, for example `"3-2"` and 2). |
| `args.verify` | boolean, default true | Run the check sweep afterwards. |

Sequence: hub port off for 3 s then on with a 25 s wait for the unit (the first rung), and on failure a 12 s off time with a 45 s wait (the second rung), through `usb_hub::power::cycle_escalating`; then a fixed 8 s wait because the unit and its codecs are listed before they answer reliably; the counter `POWER_CYCLES` (a static `AtomicU32`, reported by `status`) is increased; the codecs are found again. With `verify`, a one-point sweep at 10 MHz is made, which puts RF at the unit's own level on the TX port. Returns `power_cycled`, `back_after_s`, `codec_s11`, `codec_s21`, `hub_power` (the hub module's report), a note and, with `verify`, `verified` (`freq_hz`, `s21_db`, `s21_deg`, `s11_db`). Errors: `no hub configured for this VNA: set "hub" and "hub_port" in its entry of the bench config (for example "3-2" and 2)`; `verify must be true or false`; errors from the hub cycle, from finding the codecs, and from the verification sweep. Takes the instrument lock and also a lock named for the hub (`hub:<id>`), always in that order, so two instances on one hub take turns. Takes 15 to 90 seconds or more. Test: `the_vna_power_hook_runs_the_hub_modules_ladder_and_lists_every_failed_rung`. The README's list names this id as `bench_mcp_vna_power_cycle`, which does not exist in the code.

## The time-domain tool (`src/vna_time.rs`)

`bench_vna_time_domain` (tier `read_config`, VNA instrument kinds) takes the time-domain transform and the gating of a sweep that `bench_vna_sweep` made earlier. It reads the sweep's CSV from the output directory, so it needs no unit and sends nothing to one; it works for a sweep made by either VNA driver. Constant `C` is the speed of light, 299792458 m/s.

### `bench_vna_time_text` [@ANCHOR: bench_vna_time_text]

`text(args, key, default)` (private) reads an optional text argument.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The tool arguments. |
| `key` | `&str` | The key. |
| `default` | `&str` | Returned when the key is absent or null. |

Returns the string, the default, or the error `<key> must be text`. Borrows from the arguments or the default. Pure. Test: `a_stored_sweep_gives_its_delay_a_distance_and_a_gated_response_and_bad_input_is_refused`.

### `bench_vna_time_number` [@ANCHOR: bench_vna_time_number]

`number(args, key)` (private) reads an optional finite number; same contract as `bench_mcp_vna_number` (absent or null gives `None`, a non-number gives `<key> must be a number`), kept as a separate copy in this module. Used for `time_points`, `t_max_s`, `velocity_factor`, `gate_start_s` and `gate_stop_s`. The caller supplies the default when it gets `None`. Pure function: no I/O, no lock. Test: `a_stored_sweep_gives_its_delay_a_distance_and_a_gated_response_and_bad_input_is_refused`.

### `bench_vna_time_read_sweep` [@ANCHOR: bench_vna_time_read_sweep]

`read_sweep(file, trace)` (private) loads the frequencies and one trace of a raw sweep CSV.

| Argument | Type | Meaning |
|---|---|---|
| `file` | `&Path` | The CSV (already confined to the output directory by `in_out_dir`). |
| `trace` | `&str` | `s11` or `s21`; the columns `<trace>_re` and `<trace>_im` are read. |

Returns `(Vec<f64>, Vec<Complex>)`: frequencies in hertz and the complex values converted to the standard phase convention with `vendor_convention` (the imaginary part's sign is flipped; the unit's convention is the conjugate). Errors: `<path>: <os error>`; `<path> has no <column> column; use the CSV that bench_vna_sweep wrote (not the calibrated one)`; `<path>: line <n> is not a sweep row` for a missing, non-numeric or non-finite cell. Columns are found by header name, so extra columns are fine. A blank line inside the file counts as a row and gives the line error. Test: the time-domain test.

### `bench_vna_time_file` [@ANCHOR: bench_vna_time_file]

`in_out_dir(out, name)` (private) turns the `file` argument into a path inside the output directory and refuses anything that would leave it.

| Argument | Type | Meaning |
|---|---|---|
| `out` | `&Path` | The output directory. |
| `name` | `&str` | A bare file name ending in `.csv`. |

Returns `out/name`. Errors: `file "<name>" must be the name of a .csv sweep file in the output directory (<dir>)` when the name has any directory part (`/`, `..` components, an absolute path), is empty, or does not end in `.csv`. This is the only guard that keeps a tool caller from reading arbitrary files. Pure (the file is not opened here). Test: the time-domain test, which includes bad names.

### `bench_vna_time_domain` [@ANCHOR: bench_vna_time_domain]

`time_domain(args, out)` computes the impulse or step response of S11 or S21 of a stored sweep and optionally gates it.

| Argument | Type | Meaning |
|---|---|---|
| `args.file` | text | Required: name of a CSV in the output directory that `bench_vna_sweep` wrote. |
| `args.trace` | `s11` (default) or `s21` | Which trace. |
| `args.window` | `rectangular`, `hamming` (default), `hanning`, `blackman`, `blackman_squared` (aliases in `vnwa_open::timedomain::Window::from_name`) | Frequency-domain window. |
| `args.mode` | `lowpass` (default) or `bandpass` | The transform mode (see `vnwa_open::timedomain`). |
| `args.response` | `impulse` (default) or `step` | Which response. |
| `args.time_points` | whole number 8 to 8192, default 512 | Points on the time axis. |
| `args.t_max_s` | seconds, above 0, default `0.5 / step` | Last time; the axis is evenly spaced from 0. |
| `args.velocity_factor` | 0.1 to 1 | If given, `distance_m` is computed. |
| `args.gate_start_s`, `args.gate_stop_s` | seconds | Both or neither: a gate applied to the response. |
| `args.gate_shape` | window name, default `rectangular` | Gate edge shape. |
| `out` | `&Path` | The output directory. |

The sweep must have at least four points; the step is `(last - first) / (n - 1)` hertz. Writes `<sweep name>_<trace>_time.csv` (`time_s,re,im,magnitude`) and returns `trace`, `window`, `mode`, `response`, `points`, `step_hz`, `time_resolution_s`, `unaliased_until_s` (1 / step), `peak_time_s`, `peak_magnitude`, `time_file` and a `note`. With `velocity_factor`, `distance_m = c * vf * peak_time / 2` for S11 (round trip) or `/ 1` for S21. With a gate it also writes `<name>_<trace>_gated.csv` (`freq_hz,re,im,db,deg`, in the unit's phase convention again, the same as `bench_vna_sweep` writes) and adds `gated_file` and `gate`. Errors: the messages of the helpers above; `trace must be s11 or s21`; `mode ... must be lowpass or bandpass`; `response ... must be impulse or step`; `the sweep has fewer than four points`; `time_points must be a whole number from 8 to 8192`; `t_max_s must be positive`; `velocity_factor must be from 0.1 to 1`; `gate_start_s and gate_stop_s go together`; errors from `vnwa_open::timedomain` (transform, gate); `no peak in the time response`; file-write errors. No instrument is touched; the instrument lock is still held while it runs. The delay is a round trip for a reflection, and a calibrated sweep is what gives true reflection sizes (the note says so). Test: `a_stored_sweep_gives_its_delay_a_distance_and_a_gated_response_and_bad_input_is_refused`.

### `bench_mcp_rigd_program` [@ANCHOR: bench_mcp_rigd_program]

`rigd::program()` names the executable that serves a managed rig: the value of the environment variable `BENCH_MCP_RADIOCTLD` when it is set and not empty, otherwise `radioctld` found on the search path. Unit tests, built with `cfg(test)`, use the radioctld built from this repository when the variable is not set, so they never depend on an installed copy.

Takes no arguments and returns an `OsString`; it cannot fail, does no I/O and starts nothing itself (the daemon spawn and the diagnostics call it). A wrong path only shows up when the spawn fails. Test: `the_rig_daemon_program_is_never_empty`.

### `summary` [@ANCHOR: bench_mcp_scope_more_summary]

`summary(v)` in `scope_more.rs` reduces a waveform to its minimum, maximum, mean and RMS value, used by `bench_scope_math` to describe the combined trace.

| Argument | Type | Meaning |
|---|---|---|
| `v` | `&[f32]`, volts | The samples; an empty slice is allowed and yields a mean and RMS of 0 because the divisor is held at 1. |

Returns `(min, max, mean, rms)` as `(f32, f32, f64, f64)`; sums run in `f64`. An empty slice gives `(f32::MAX, f32::MIN, 0, 0)`. Pure, no errors, no I/O. Test: `summary_gives_min_max_mean_and_rms`.

### `readback` [@ANCHOR: bench_mcp_scope_more_readback]

`readback(s, query)` asks the scope for one setting and returns what it answered as a JSON value, so a caller can see whether a setting was accepted. It lowers the link timeout to 3 seconds for the question and restores 10 seconds afterwards.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `query` | `&str` | A query command such as `TRIG_MODE?`. |

Returns the trimmed reply as a JSON string, `"(empty reply)"` for a blank answer, or `"no answer to <query>: <error>"` when the query fails; it never returns an error so one silent setting does not hide the others. Test: `apply_sends_each_command_and_reads_its_setting_back`.

### `apply` [@ANCHOR: bench_mcp_scope_more_apply]

`apply(s, cmds)` sends a list of scope commands one by one and reads each related setting back, so the answer shows what the scope really accepted rather than what was requested. It waits 100 milliseconds after each command before the readback.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&mut Link` | Open link to the scope. |
| `cmds` | `Vec<(String, String)>` | Pairs of (command to send, query that reads the setting). |

Returns `{sent, readback, note}`: the commands in order, a map from each query to its `readback` result, and a note that these commands come from the programming guide and are not all confirmed on firmware 1.3.27. Errors: a failed send returns its text and stops. Test: `apply_sends_each_command_and_reads_its_setting_back`.
