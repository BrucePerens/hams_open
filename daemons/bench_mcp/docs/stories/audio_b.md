# The Audio Tools Sound The Alert, Keep The Volume Safe, Measure What Was Recorded And Store The Files

As the person running a bench with a speaker and a microphone on the computer, I want the audio tools to alert me at the right loudness, never leave a
volume changed after they finish, tell me from a recording whether a tone was really heard, and keep the recordings in one folder that cleans itself.
Everything below is in `daemons/bench_mcp/src/audio/`: `alert.rs` (the operator alert), `safety.rs` (volume limits and the guard that puts the
volume back), `levels.rs` (volume and mute through `wpctl`), `analysis.rs` (level, spectrum and tone measurements), `heard.rs` (the "was the tone
heard" decision), `wav.rs` (reading and writing WAV files) and `store.rs` (the managed capture folder). The tools around them (`tools.rs`, `verify.rs`,
`discover.rs`, `speak.rs`) call these functions; the tool-level description is `docs/AUDIO_TOOLS.md`.

None of this transmits on a radio. The audio tools make sound in the room and listen to it; a `full`-tier audio instrument entry is what allows the
sound-making ones, and the volume cap and quiet hours below are the only gate they have. The band check and TX gate of the radio tools do not apply here.

## Alerts (`alert.rs`)

### `Severity::parse` [@ANCHOR: bench_mcp_audio_alert_severity_parse]

Turns the `severity` argument of `bench_audio_alert` into the enum `Severity` (`Info`, `Warning`, `Urgent`). It exists so a typo in a severity is refused
instead of silently becoming a quiet beep. Called once by `alert()` with the argument text, or `"info"` when the caller gave none.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&str`, borrowed | Exactly `info`, `warning` or `urgent`, lower case. No trimming, no case folding. |

Returns `Ok(Severity)`. Any other text, including an empty string or `URGENT`, returns `Err("severity \"...\" is not info, warning or urgent")`. No panic.
Pure function: no I/O, no locking. Tests: `the_patterns_grow_with_severity_and_stay_short` in `alert.rs` cites it. Related: `bench_mcp_audio_alert_severity_word`.

### `Severity::word` [@ANCHOR: bench_mcp_audio_alert_severity_word]

The upper-case label of a severity (`INFO`, `WARNING`, `URGENT`) as a `&'static str`. It is the prefix of the alert text, `[URGENT] message`, which the answer
always returns as `text_to_send` so the operator can be told by another channel, and it is the `severity` field of the answer. Takes `self` by value
(`Copy`). Never fails. Pure. Inverse of `Severity::parse` apart from the case.

### `Severity::volume_share` [@ANCHOR: bench_mcp_audio_alert_severity_volume_share]

The fraction of the volume limit an alert of this severity uses outside quiet hours: `Info` 0.6, `Warning` 0.8, `Urgent` 1.0. The share is a plain
`f64` multiplier on the limit (the limit is in the wpctl volume scale, see `bench_mcp_audio_levels_get`), so the result is never above the limit. Only
`alert_volume` calls it. Never fails, pure. Documented for the operator in `AUDIO_TOOLS.md` ("info 60 %, warning 80 %, urgent 100 % of the limit").

### `alert_volume` [@ANCHOR: audio_alert_volume]

Chooses the wpctl volume at which an alert plays. Called by `alert()` after `safety::limits` has worked out the limit for the output and the time of day.

| Argument | Type | Meaning |
|---|---|---|
| `sev` | `Severity` | The alert's severity. |
| `limit` | `f64`, wpctl scale (0 to 1, cube root of the amplitude factor) | `Limits::volume`: the output's cap, or the quiet volume in quiet hours for a non-urgent alert. |
| `quiet` | `bool` | Whether quiet hours apply now (`Limits::quiet`). |

Returns `limit` itself when `quiet` and the severity is not `Urgent`: in quiet hours info and warning use the quiet volume only. In every other case it
returns `limit * volume_share(sev)`: so an urgent alert in quiet hours gets 100 % of the cap (the limit passed in is already the cap because
`limits(.., urgent = true, ..)` skips the quiet volume), and outside quiet hours the shares of 60, 80 and 100 % apply. No errors, no side effects. The
result is later clamped again by `VolumeGuard::apply`, so it can never exceed the limit. Tests: `quiet_hours_give_the_lowest_volume_except_for_urgent` in `alert.rs`.

### `pattern` [@ANCHOR: bench_mcp_audio_alert_pattern]

Builds the tone pattern of a severity as mono `f32` samples at 48000 Hz (the constant `RATE`), full scale 1.0, each beep at -12 dBFS peak through
`tones::generate` (a sine with a 10 ms edge ramp). It is audio the operator can tell apart from across the room without reading anything.

| Severity | Pattern | Length |
|---|---|---|
| `Info` | one 880 Hz beep of 0.25 s | 0.25 s |
| `Warning` | two 880 Hz beeps of 0.2 s with a 0.12 s gap | 0.52 s |
| `Urgent` | three times: 1000 Hz 0.15 s, 1400 Hz 0.15 s, gap 0.08 s (a siren) | 1.14 s |

Takes the severity by value, returns a new `Vec<f32>` (owned). No errors, no I/O. `alert()` calls it twice: once to write the WAV and once to measure its
length for the player's time-out (`length + 5 s`). With `repeat` above 1 the whole pattern is played again, so the total is bounded by `MAX_ALERT_S` (see
`audio_alert_tool`).

### `alert` [@ANCHOR: audio_alert_tool]

The `bench_audio_alert` tool (`full` tier): plays a tone pattern by severity and/or speaks a message on the alert output, and in every case returns the
alert text for the caller to send by another channel, because the operator may be away. Called by the audio dispatcher (`audio/mod.rs`, `bench_audio_alert`);
when the output is `auto`, a stored discovery exists, the devices changed, the alert is not urgent and it is not quiet hours, the dispatcher listens again first
(`rediscover: false` turns that off).

| Argument (JSON key) | Type | Default | Meaning |
|---|---|---|---|
| `severity` | string | `info` | `info`, `warning` or `urgent` (see `Severity::parse`). |
| `message` | string | none | Text to speak and to put in `text_to_send`. Trimmed; an empty or blank message counts as none. |
| `tone` | bool | `true` | Play the severity's tone pattern. |
| `speak` | bool | `true` when a message was given, else `false` | Speak the message. |
| `repeat` | integer 1 to 5 | 1 | How many times the tone and speech are played, with a 0.5 s pause between. |
| `output` | string | config `alert_output` | A device name or alias; `auto` follows the stored discovery; never the system default. |
| `dry_run` | bool | `false` | Only plan: no sound, no volume change, no files, no rediscovery. |
| `text`, `engine`, `voice`, `language`, `rate_wpm` | | | Speech settings, handed to `speak::request` (the message becomes `text`). |

Errors returned as `Err` (before any sound is attempted, no `text_to_send`): invalid severity; `speak` true with no message; `tone` and `speak` both false;
`repeat` outside 1 to 5. After that point every failure is folded into an `Ok` answer, so the text is never lost: the output cannot be resolved or is
reserved, the speech request is refused, the WAV cannot be written, the volume cannot be set, the player fails, or the alert ran longer than `MAX_ALERT_S`
(30 s; checked after each repeat, not in the middle of a sound).

Answer: `{"text_to_send": "[URGENT] message", "audible": true, "sound": {...}, "note": ...}` on success, where `sound` has `output`, `alias`, `severity`,
`repeat`, `tone`, `spoken`, `engine`, `volume_used`, `volume_limit`, `quiet_hours`, `seconds`, `speech_seconds`, `volume_restored` and `mute_restored`; or
`{"text_to_send": ..., "audible": false, "error": "...", "note": "No sound was made..."}`. Without a message the text is `[SEVERITY] alert`. With `dry_run` the
answer is the plan from `tools::dry_plan` plus `text_to_send`, or `{"dry_run": true, "would_fail": "...", "text_to_send": ...}`.

Side effects and order: chooses the output (`tools::pick`), computes the limits (`safety::limits`, with `urgent` for an urgent alert so it may use the
cap in quiet hours) and the volume (`alert_volume`); writes the pattern as `tone-<time>-alert.wav` in the capture folder (owner-only directory); then
`play_guarded` sets the volume, unmutes the output, plays each repeat through `pw-play` (waiting at most the pattern length plus 5 s) and speaks the
message, and puts the volume and mute back whatever happened (see `audio_safety_guard`). The tone file is deleted afterwards. Blocking and synchronous:
the call returns when the sound has finished; there is no lock of its own. There is no radio transmission.

Tests: `a_dry_run_of_an_alert_returns_the_text_and_the_plan_and_makes_no_sound` in `alert.rs`. Related: `audio_alert_volume`,
`bench_mcp_audio_alert_pattern`, `bench_mcp_audio_tools_play_guarded`, `audio_safety_limits`.

## Level Statistics And Spectrum (`analysis.rs`)

These are pure functions on mono `f32` samples with full scale 1.0, no hardware. Amplitudes are calibrated so a sine of amplitude A reads
`20 log10(A)` dBFS (peak, not RMS) at its frequency. The constant `FLOOR_DB` (-200 dBFS) stands for "nothing". Callers: `verify.rs`, `events.rs`,
`discover.rs`, `tools.rs`, `heard.rs` and the Hamlib receive checks (`hamlib/rxcheck.rs`, `hamlib/detect.rs`).

### `db` [@ANCHOR: bench_mcp_audio_analysis_db]

Converts a linear amplitude (full scale 1.0) to dBFS: `20 log10(amplitude)`. An amplitude at or below 1e-10, including zero and any negative number,
returns `FLOOR_DB` (-200.0), so a silent signal does not give minus infinity. `amplitude` is an `f64`, no unit. A NaN input is not caught (NaN is not
`<= 1e-10`) and returns NaN. Pure, no errors. 1.0 gives 0.0, 0.5 gives about -6.02.

### `rms` [@ANCHOR: bench_mcp_audio_analysis_rms]

The root mean square of a slice of samples (`&[f32]`, borrowed), summed in `f64`. An empty slice returns 0.0. The result is linear (not dB): a full-scale
sine is about 0.707, so `db(rms(s))` of that is about -3 dBFS. Used by `stats` and by the callers that need a level of a block (event detection,
receive checks). Pure, no errors.

### `stats` [@ANCHOR: bench_mcp_audio_analysis_stats]

Summarises a recording into a `Stats` value, used by recording and verification tools and by discovery to judge whether an input is alive.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&[f32]` | Mono samples, full scale 1.0. |
| `rate` | `u32`, Hz | Sample rate; sets the 20 ms window (`rate * 0.02` samples, at least 1). |
| `silence_dbfs` | `f64`, dBFS | A window is quiet when its peak-equivalent level (RMS times the square root of 2) is under this. |

`Stats` fields: `samples` (count), `peak_dbfs` (largest absolute sample through `db`), `rms_dbfs` (plain RMS in dBFS, so a full-scale sine reads -3 dBFS),
`clipped` (samples at or beyond 0.999 of full scale), `silence_fraction` (0 to 1, the share of 20 ms windows that are quiet; a short last window counts as
one window; 1.0 for an empty input), `all_zero` (every sample is exactly 0.0: a muted, disconnected or dead input, not a quiet room; true for an empty
input) and `dc_offset` (the mean sample, 0.0 for an empty input). No errors and no side effects; cost is a few passes over the samples.

### `fft` [@ANCHOR: bench_mcp_audio_analysis_fft]

Private in-place radix-2 forward FFT (bit-reversal permutation, then butterflies with the twiddle factor advanced by a rotation per step). It computes
the unnormalised transform `X[k] = sum x[n] e^(-2 pi i k n / N)` of the complex signal given as `re` and `im` (`&mut [f64]`, same length), overwriting
both. The length must be a power of two: this function does not check it (the caller `spectrum` does); another length gives meaningless output, and
slices of different lengths panic on an out-of-range index. Length 0 and 1 are no-ops. Pure, no allocation; only `spectrum` calls it.

### `Spectrum::range` [@ANCHOR: bench_mcp_audio_analysis_spectrum_range]

Private helper of `Spectrum`: the half-open range of bin indices that lie between `lo_hz` and `hi_hz` inclusive. The low end is rounded up and never below bin 1
(the DC bin is left out); the high end is rounded down plus one and cut to the number of bins. If `hi_hz` is below `lo_hz`, or the band is outside the
spectrum, the range is empty (`lo..lo`), which callers treat as "no data". Negative or NaN frequencies fall back to bin 1. Used by `peaks`, `median_db`
and `similarity`. Pure, no errors.

### `spectrum` [@ANCHOR: bench_mcp_audio_analysis_spectrum]

Welch's averaged amplitude spectrum: the signal is cut into blocks of `fft_len` samples starting every `fft_len / 2` samples (50 % overlap), each
multiplied by a Hann window, transformed with `fft`, and the power of all blocks is averaged. The result is scaled so a sine of amplitude A reads A at a
bin centre (up to about 1.4 dB less between bins, the Hann scalloping).

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&[f32]`, borrowed | Mono samples, full scale 1.0. |
| `rate` | `u32`, Hz | Sample rate; the bin width is `rate / fft_len`. |
| `fft_len` | `usize` | Block length: a power of two, at least 64 (4096 is typical at 48 kHz, about 11.7 Hz per bin). |

Returns `Spectrum { bin_hz, amp }` where `amp` has `fft_len / 2 + 1` linear amplitudes from 0 Hz to Nyquist (owned `Vec`). Error: `Err("fft_len N must be a power
of two of at least 64")`. A signal shorter than one block is zero-padded and gives one block (so its amplitudes are lower than the true level); an empty signal gives
all zeros. Samples after the last whole block are not used. Cost is O(n log fft_len) with a few vectors allocated per block. Pure, no side effects.

### `peaks` [@ANCHOR: bench_mcp_audio_analysis_peaks]

Finds the strongest spectral lines in a band, for the dominant-frequency and beep-frequency checks.

| Argument | Type | Meaning |
|---|---|---|
| `sp` | `&Spectrum` | From `spectrum`. |
| `lo_hz`, `hi_hz` | `f64`, Hz | Band to search (see `Spectrum::range`: the DC bin is never searched). |
| `count` | `usize` | The most peaks to return. 0 does not mean none: the stop test never fires, so every peak that qualifies is returned. |
| `min_db` | `f64`, dB | A bin only counts when it is at least this far above the median level of the band. |

A bin is a peak when it is higher than the bin before and not lower than the bin after, and at least `min_db` above the median of the band's dB levels. Each
peak's frequency and level are refined by parabolic interpolation over the three dB values (`Peak { hz, dbfs }`). Peaks are sorted loudest first and a peak
within 3 bins of a louder kept one is dropped, so the skirt of one tone is not reported as another. Returns an empty `Vec` when the band holds fewer than
three bins. Neighbours of an edge bin may lie just outside the band. No errors, pure.

### `median_db` [@ANCHOR: bench_mcp_audio_analysis_median_db]

The median spectrum level in dBFS over a band: a noise-floor estimate that a few tones do not move. Takes the spectrum and `lo_hz`, `hi_hz` (Hz). Sorts the bins'
dB values and returns the one at index `len / 2` (the upper median for an even count). An empty band returns `FLOOR_DB` (-200). Pure, no errors. Used by the
verify tool and the receive check to compare a signal with the floor.

### `tone_amp` [@ANCHOR: bench_mcp_audio_analysis_tone_amp]

The amplitude of one exact frequency in a block of samples: a single-frequency Fourier sum with a Hann window (no FFT bins, so no scalloping), scaled so a
sine of amplitude A at `hz` reads A.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&[f32]` | The block, mono, full scale 1.0. |
| `rate` | `u32`, Hz | Sample rate. |
| `hz` | `f64`, Hz | The frequency to measure; should be under half the rate (not checked). |

Returns a linear amplitude (`f64`); a block shorter than 8 samples returns 0.0. Cost is one sine and cosine per sample. Pure, no errors. The workhorse of
`hear_tone`, of beep timing in event detection and of the loopback test.

### `similarity` [@ANCHOR: bench_mcp_audio_analysis_similarity]

How alike two spectra are: the Pearson correlation of their dB values over a frequency band. 1.0 means the same shape, about 0 unrelated, negative inverted.
Used by `bench_audio_verify` to compare a recording with a stored `ref-` reference (default threshold 0.8).

| Argument | Type | Meaning |
|---|---|---|
| `a`, `b` | `&Spectrum` | Must have the same `bin_hz` (within 1e-9), i.e. the same rate and `fft_len`. |
| `lo_hz`, `hi_hz` | `f64`, Hz | The band compared. |

Levels are floored at -120 dB so a hole in one spectrum does not dominate. Returns 0.0 (not an error) when the bin widths differ, when fewer than 3 bins are in
the band, or when either spectrum is flat over the band (no variation to correlate). Otherwise a value from -1.0 to 1.0. Pure.

## The Tone-Heard Decision (`heard.rs`)

### `tracks` [@ANCHOR: bench_mcp_audio_heard_tracks]

Private helper that follows the level of one frequency through a recording: it slides a 30 ms window in 10 ms hops from sample `from` to sample `to` (cut to the
recording's length) and returns `(start_time_s, amplitude)` pairs, one per window, using `tone_amp`. The time is the window's start in seconds from the start of
the recording (`hear_tone` adds half a window when it reports onset and offset). Arguments: `s` samples, `rate` Hz, `hz` the frequency, `from` and `to` sample
indexes (usize). A range shorter than one window returns an empty `Vec`. Pure, no errors; called only by `hear_tone`.

### `hear_tone` [@ANCHOR: audio_heard_tone]

Decides whether a tone of one frequency was heard in a recording, and why not if it was not. Used by the loopback test (`tools.rs`, 12 dB needed), by the
discovery of working outputs and inputs (`discover.rs`) and by sound verification.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&[f32]`, mono, 1.0 full scale | The recording: noise first, then the tone. |
| `rate` | `u32`, Hz | Sample rate. |
| `hz` | `f64`, Hz | The tone's frequency (under half the rate, not checked). |
| `expect_from_s` | `f64`, seconds | When the tone is expected to start; everything before is the noise reference. At least 60 ms of lead is needed. |
| `min_snr_db` | `f64`, dB | The least level over the pre-tone noise that counts as heard. |

Returns a `Heard` value (never an error): `heard`, `hz`, `level_dbfs` (the strongest level of the tone, peak amplitude), `noise_dbfs`, `snr_db`,
`over_neighbours_db` (how far above the louder of the frequencies 1.19 times higher and lower, about a quarter octave, measured at the loudest window), `onset_s`
and `offset_s` (first and last time the track is within 6 dB of its peak, from the start of the recording, `None` when not measured), `input_all_zero` and a
plain-language `reason`. `Heard::json()` gives the JSON form with levels rounded to 0.1 dB and times to 1 ms.

The decision, in order: all samples exactly zero is "muted, disconnected or dead" and never passes; a recording shorter than the lead plus two windows, or a lead under two
windows, is "too short"; the noise reference is the 75th percentile (index `len * 3 / 4` of the sorted values, not the median) of the pre-tone track at the
tone's frequency and is never taken below -96 dBFS, so a digitally clean input does not give huge ratios; a level under -100 dBFS is "nothing at N Hz"; a ratio
under `min_snr_db` is "not enough over the noise"; under 10 dB over the neighbouring frequencies is "broadband sound, not the tone"; otherwise heard. Pure and
read-only; cost about `len / 480` windows at 48 kHz, each a `tone_amp` call. Tests: `a_clear_tone_over_room_noise_is_heard_with_its_times` and the two tests after it in `heard.rs` (noise only, another frequency, barely over the noise, a broadband burst).

## Volume And Mute (`levels.rs`)

The volume scale is `wpctl`'s: 0.0 up to 1.0 and above, the cube root of the signal amplitude factor, so 0.5 is about -18 dB. The config's volume cap and
quiet volume are in the same scale. A `Level` value is `{ volume: f64, muted: bool }`. Each function takes a `&dyn Run` (the injectable command runner;
real runs start `wpctl`, tests give a scripted one) and the PipeWire node id (`u32`, the object id from `wpctl status`, which changes when PipeWire restarts, so
it is looked up fresh from the device list). These change the live sound system of this computer; there is no radio involved.

### `parse_get_volume` [@ANCHOR: audio_levels_parse]

Parses the text of `wpctl get-volume`: `Volume: 0.52` or `Volume: 0.40 [MUTED]`, surrounding whitespace allowed. Argument `s: &str` (borrowed). Returns `Ok(Level)` with
`muted` true when any word after the number is `[MUTED]`. Errors (as `String`): text that does not start with `Volume:` gives `unexpected wpctl output "..."`, and a missing or
non-numeric number gives `no volume number in "..."`. An empty string is an error. Pure; the volume is whatever wpctl reports and may be above 1.0.

### `get` [@ANCHOR: bench_mcp_audio_levels_get]

Reads a node's volume and mute state: runs `wpctl get-volume <node_id>` through `run` and parses it with `parse_get_volume`. Returns `Ok(Level)`. Errors: the runner's own
error (for example `wpctl` not installed) passes through; a non-zero exit gives `wpctl get-volume N: <trimmed output>` (for example "Failed to find node"); unparsable output gives the
`parse_get_volume` errors. Read-only, no state changed. Called by the guard, by dry-run planning, by the levels and status tools and by discovery.

### `set_volume` [@ANCHOR: bench_mcp_audio_levels_set_volume]

Sets a node's volume: runs `wpctl set-volume <node_id> <volume>` with the volume written to 3 decimals (`volume: f64`, wpctl scale). This function does not cap or check
the value (wpctl accepts values above 1.0, which can be loud): every audible caller goes through `VolumeGuard` or the levels tool, which apply the limits first.
Returns `Ok(())` or `Err("wpctl set-volume N: <trimmed output>")` on a non-zero exit, or the runner's error. Side effect: the live volume of the device changes, and stays
changed unless a guard puts it back.

### `set_mute` [@ANCHOR: bench_mcp_audio_levels_set_mute]

Mutes or unmutes a node: runs `wpctl set-mute <node_id> 1` (mute true) or `0`. Returns `Ok(())` or `Err("wpctl set-mute N: <trimmed output>")`, or the runner's error. Side effect:
the live mute state changes until set again or restored by a guard. Unmuting is only done by callers that were asked to make sound (`unmute` in `VolumeGuard::apply`) or by the
levels tool on request.

## Limits And The Volume Guard (`safety.rs`)

### `limits` [@ANCHOR: audio_safety_limits]

The highest volume allowed now for audible output on a device. Returns `Limits { volume: f64, quiet: bool }` (wpctl scale). `cap = cfg.cap_for(dev)` is the device's own cap
never above the global `volume_cap` (default 0.5); `quiet` is true when the config's quiet hours contain `minute`. When quiet and not `urgent` the volume is the lower of
the cap and `quiet_volume` (default 0.15); otherwise it is the cap. Urgent requests therefore still use the cap in quiet hours, and `quiet` still reports true.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | The audio config: caps, quiet volume and quiet hours. |
| `dev` | `&Device` | The output; selects a per-port cap by alias. |
| `urgent` | `bool` | Allows the cap in quiet hours. |
| `minute` | `u32`, 0 to 1439 | Minute of the local day, supplied by the caller (`Ctx::minute`) so tests control the clock. |

No errors, no side effects. Every tool that makes sound calls this first; it is the single place the volume cap and quiet hours are decided.

### `VolumeGuard::apply` [@ANCHOR: audio_safety_guard]

Sets a device's volume for the length of an operation and remembers what it was, so the volume and mute can be put back. Returns the guard (which must be kept for as long as
sound plays, and restores on `restore()` or when dropped) together with an `Applied { before: Level, volume, muted }` describing what is now in effect.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Run`, borrowed for the guard's life | The command runner. |
| `dev` | `&Device` | The output. |
| `lim` | `&Limits` | From `limits`; the target never exceeds `lim.volume`. |
| `want` | `Want` | `Keep` leaves the volume but lowers it to the limit if above; `Level(v)` sets `v` or the limit, whichever is lower. |
| `unmute` | `bool` | Unmute the device if it is muted (for sound that must be heard). |

Errors: a reserved device (`dev.reserved` holds the reason, for example a headset in use) gives `"<node> is reserved: <why>"` and nothing is changed; reading the level or setting the
volume or mute fails with the `levels` error. If the unmute fails after the volume was changed, the guard restores the volume before returning the error; if setting the volume itself
fails, the guard is dropped and puts back what it had. A volume within 0.004 of the target is not rewritten. A guard that changed nothing is marked done, so restoring it does
nothing. Side effects: up to two `wpctl` calls; no files, network or radio. The guard borrows `run` and is single-threaded (not shared). `VolumeGuard::set` raises
the volume in later stages and the original state is still restored. Tests: `the_guard_lowers_to_the_cap_unmutes_and_restores_both` in `safety.rs`.

### `VolumeGuard::restore` [@ANCHOR: bench_mcp_audio_safety_volumeguard_restore]

Puts the device back to the volume and mute it had when `apply` recorded them. Idempotent: after the first call (or when `apply` changed nothing) later calls return `Ok(())`
without touching the system. The guard is marked done before the commands run, so a failing restore is not retried by `Drop`: the volume is set first, then the mute, and if
the volume command fails the mute is not attempted and the error is returned. Callers (`play_guarded`) call it explicitly so they can report the error; it is the only
place the original level is written back. Takes `&mut self`; returns `Result<(), String>` with the `levels::set_volume` or `levels::set_mute` error.

### `VolumeGuard` drop [@ANCHOR: bench_mcp_audio_safety_volumeguard_drop]

The destructor of `VolumeGuard`: calls `restore()` so the volume and mute go back on every exit path, including an early `?` return or a panic that unwinds, so a tool never leaves
a speaker loud or a microphone muted. It cannot return an error: a failure is printed to standard error as `audio: could not restore the volume of node N: <error>` (the
server's log) and otherwise ignored; it never panics. It does nothing when the guard was already restored. It cannot help when the whole process is killed with no
unwinding; there is no other recovery. Tests: `the_guard_lowers_to_the_cap_unmutes_and_restores_both` in `safety.rs` (it lets the guard drop).

## The Managed Capture Folder (`store.rs`)

Recordings and generated tones go in one folder, `capture_dir` of the audio config (default `<BENCH_MCP_OUT>/audio`), created owner-only. Recordings may contain speech, so they stay on
this computer and no tool uploads them. The files are named `<prefix>-<YYYYMMDD-HHMMSS>[-<label>].wav`, kept by the retention rule below, and named back by callers only through
`managed_file`.

### `timestamp` [@ANCHOR: bench_mcp_audio_store_timestamp]

The current local time as `YYYYMMDD-HHMMSS` (for example `20261111-143005`), through `libc::time` and `localtime_r`, so it follows the computer's time zone and has second resolution.
No arguments, no errors (if the C library fails the fields stay zero and the text is meaningless but still returned). Used for capture file names and for the time of the TX log
and state records in `hamlib/tx.rs`. Contains an `unsafe` block that only writes to its own local variables. Thread-safe (`localtime_r`).

### `slug` [@ANCHOR: bench_mcp_audio_store_slug]

Makes text safe to use inside a file name: every character that is not an ASCII letter or digit (spaces, dots, slashes, non-ASCII) becomes `-`, leading and trailing dashes are
removed, and at most 40 characters are kept (so the result can end in a dash after the cut). Runs of dashes are not merged. Takes `&str`, returns an owned `String`, which is
empty when nothing usable remains (callers drop an empty label). Pure, no errors. It prevents `..`, path separators and shell-special characters in names; used for labels in
`new_path` and for reference recording names (`ref-<slug>.wav`).

### `new_path` [@ANCHOR: bench_mcp_audio_store_new_path]

Makes sure the capture directory exists and returns a fresh file path in it: `<prefix>-<timestamp>[-<label>].wav`, with `-2`, `-3`, ... added before `.wav` while a file of that name
exists.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | The managed directory; created (with parents) if missing, with mode 0700. An existing directory keeps its mode. |
| `prefix` | `&str` | A fixed word such as `tone`, `rec` or `speech`; not sanitised, so callers pass constants. |
| `label` | `Option<&str>` | Caller text, passed through `slug`; dropped when empty after that. |

Returns `Ok(PathBuf)` (owned); error `"<dir>: <os error>"` when the directory cannot be created. The file itself is not created, so two callers asking in the same second before either
writes can get the same path: the tools run one audio operation at a time, and nothing else guards it. Side effect: the directory is created.

### `managed_file` [@ANCHOR: audio_store_managed]

Accepts a file name given by a caller (a relative name is looked up in the managed directory, an absolute path is taken as is) and returns its canonical path only if it is a `.wav`
file inside that directory. It stops a tool argument from reading any other file on the computer.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | The managed directory. |
| `name` | `&str` | A file name or path from the caller. |

Errors (`String`): the file or the directory does not exist or cannot be resolved (`"<name>: <os error>"`); the resolved path is not under the resolved directory
(`"<name> is outside the managed audio directory <dir>"`, which also catches `..` and symbolic links pointing out); the extension is not exactly `wav`
(`"<name> is not a .wav file"`). Reads the file system (canonicalisation) only. Used by the play tool and by the verify tool for references. Test: `only_wav_files_inside_the_directory_are_accepted` in `store.rs`.

### `prune` [@ANCHOR: audio_store_retention]

The retention rule for the managed directory: deletes `.wav` files older than `days`, then the oldest ones until no more than `max_files` remain, and returns how many it removed.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | The managed directory (not searched recursively). |
| `max_files` | `usize` | Most files to keep (config `retention_files`, default 50). |
| `days` | `u64` | Oldest age kept, by modification time (config `retention_days`, default 14). |

Files whose name starts with `ref-` (reference recordings stored by verify) are never counted or deleted. Other files are sorted oldest first. A file that cannot be removed
is skipped and not counted. An unreadable or missing directory gives 0, not an error. Side effect: files are deleted; no locking, so two processes pruning at once only race to delete the same
files. Called after recordings and speech (`speak.rs`, `verify.rs`) and by the record tool (`tools.rs`). Test: `the_oldest_files_beyond_the_count_and_the_age_are_removed` in `store.rs`.

## WAV Files (`wav.rs`)

A `Wav` value is `{ rate: u32 (Hz), channels: u16, samples: Vec<f32> }` with interleaved samples, full scale 1.0. Reading accepts integer PCM of 16, 24 and 32 bits and 32-bit float; writing
produces 16-bit PCM. Used for recordings, generated tones, speech output and references.

### `Wav::mono` [@ANCHOR: bench_mcp_audio_wav_wav_mono]

The channels averaged into one: a mono file is returned as a plain clone, otherwise each whole frame of `channels` samples is summed and divided by the channel count, and a trailing partial frame is
dropped. Takes `&self`, returns an owned `Vec<f32>`; a channel count of 0 is treated as 1. No errors, pure. Every analysis function expects mono, so verification calls it on references and recordings.

### `parse` [@ANCHOR: audio_wav]

Reads a WAV file from bytes into a `Wav`. `b: &[u8]` is the whole file. It needs a `RIFF`/`WAVE` header, then walks the chunks (an odd-sized chunk is padded by one byte, unknown chunks are skipped,
a chunk longer than the file is cut at the end of the file; the last `fmt ` and `data` chunks win). Format tag 1 or 0xFFFE (extensible; its sub-format is not examined) with 16, 24 or 32 bits is
scaled to full scale 1.0 (16-bit divides by 32768, 24-bit by 8388608, 32-bit by 2147483648); tag 3 with 32 bits is read as float unchanged (not clamped). Incomplete trailing samples are ignored.
Errors (`String`): not a RIFF/WAVE file or under 12 bytes; no `fmt ` (16 bytes at least) or `data` chunk; zero channels or zero sample rate; any other tag or bit depth (`unsupported WAV format tag T with B bits`).
Pure. `read(path)` wraps it with the file read and prefixes errors with the path. Tests: `wav.rs` round-trip, float and 24-bit, and garbage tests.

### `encode` [@ANCHOR: bench_mcp_audio_wav_encode]

Builds the bytes of a 16-bit PCM WAV (a 44-byte header with `fmt ` format 1, then one `data` chunk) from a `Wav`: each sample is clamped to -1.0 to 1.0, multiplied by 32767 and rounded, so values beyond full
scale clip and a round trip through `parse` is within one 16-bit step. `w: &Wav` borrowed; returns the owned `Vec<u8>`. It does not check that the channel count and sample count agree, and the sizes are 32-bit
numbers, so it is meant for recordings far under 4 GiB (the tools cap recordings at tens of seconds). No errors, no I/O; `write(path, &wav)` writes the bytes with `std::fs::write` (error
`"<path>: <os error>"`) and callers use `store::new_path` for the path.
