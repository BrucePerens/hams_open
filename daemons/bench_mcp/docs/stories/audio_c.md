# The Bench Can Make Sound, Speak, Listen and Judge What It Heard, Without Ever Being Too Loud

As the owner of a bench with a sound card, I want an agent to be able to play a test tone, speak a message, record a room or a rig's speaker, and
ask "did the device make the sound it should have", with the volume held under a cap and put back afterwards and no reserved device ever touched, so
I can trust the sound tools in a room where I am sitting. This story is the third part (c) of the audio driver, in `daemons/bench_mcp/src/audio/`:
`tones.rs` (generated signals), `tools.rs` (device choice, record, meter, play, loopback, levels, volume, safe state), `verify.rs` (the verification
and event tools) and `speak.rs` (speech). The other audio files (devices, engine, safety, discovery, alert, analysis) are documented in their own
stories. The tool tiers: `read_only` for levels and synthesizers; `read_config` for record, meter, verify, events and safe state (they listen);
`full` for play, speak, loopback and set volume (they make noise). None of these tools keys a transmitter: they use only the sound devices of
the host through PipeWire, and a device reserved for another driver (the VNA codecs) is refused by name. Where a tool makes sound the output's
volume is limited by the cap and quiet hours (`safety::limits`) and restored by the volume guard.

## Generated test signals (`tones.rs`)

### `audio_tones` [@ANCHOR: audio_tones]

`tones::generate(t, rate, seconds, level_dbfs) -> Vec<f32>` makes the samples of one test signal, in memory, with no I/O. It is called by
`tools::source` (the play tool) and `tools::loopback`, and by tests. The tone is a `Tone`: `Sine{hz}`, `Sweep{from_hz,to_hz}` (logarithmic: the
frequency is `from_hz * e^(k t)`, and the phase is its integral), `Dual{hz1,hz2}` (two sines, each at half amplitude so the sum peaks at the
level) or `Noise` (a fixed-seed xorshift generator, so the same call gives the same noise).

| Argument | Type | Meaning |
| --- | --- | --- |
| `t` | `Tone` (copied) | what to make |
| `rate` | `u32`, Hz | sample rate; callers pass 48000 |
| `seconds` | `f64` | length; the count is `round(seconds*rate)`; a sweep uses it as the sweep time (never below 1e-9) |
| `level_dbfs` | `f64`, dBFS | peak amplitude, `10^(db/20)`; callers keep it within -90 to -1 with `tones::level` |

Returns mono `f32` samples in -1 to 1. Each end has a 10 ms raised-cosine fade (shorter, to half the length, for very short signals, at least one sample) so the
signal starts and stops without a click. Special values: zero seconds returns an empty vector. No errors and no panic for the argument ranges the tools allow
(a sweep with `from_hz` of 0 would divide by zero, which `tones::parse` prevents by its 20 Hz minimum). Pure and thread-safe. Test: `audio_tones` in `tones.rs`.

### `bench_mcp_audio_tones_hz` [@ANCHOR: bench_mcp_audio_tones_hz]

`hz(args, k, default, rate) -> Result<f64, String>` (private) reads one frequency argument from a tool's JSON. It exists so that no tone is made outside the
range a speaker and the sample rate can carry. Called only by `tones::parse`.

| Argument | Meaning |
| --- | --- |
| `args` | the tool arguments (borrowed) |
| `k` | the key to read, `freq_hz` or `freq2_hz` |
| `default` | `Some(hz)` when the key may be absent; `None` makes it required |
| `rate` | the sample rate in Hz; the top allowed is 45 % of it (21600 Hz at 48000) |

Returns the value in Hz. Errors: the key is missing and there is no default (`"<k> is required for this tone"`); the value is outside 20 Hz to
0.45 x rate (the message names the key, the value and the range). A non-number value counts as missing. Pure. Test: `bench_mcp_audio_tones_parse` and `_hz` in the `tones.rs` tests.

### `bench_mcp_audio_tones_parse` [@ANCHOR: bench_mcp_audio_tones_parse]

`tones::parse(args, rate) -> Result<Tone, String>` turns the tool arguments of `bench_audio_play` into a `Tone`. Called by `tools::source`.

| Argument `tone` | Result | Frequencies read (defaults) |
| --- | --- | --- |
| absent or `sine` | `Sine` | `freq_hz` (1000) |
| `sweep` | `Sweep` | `freq_hz` start (200), `freq2_hz` end (4000) |
| `two_tone` | `Dual` | `freq_hz` (700), `freq2_hz` (1900) |
| `noise` | `Noise` | none |

Errors: an unknown tone name (`tone "x" is not sine, sweep, two_tone or noise`), or any frequency outside the range of `hz` (20 Hz to 0.45 x rate). Pure,
no side effects. The sound level is separate (`tones::level`). Tests: `tones.rs`, `tools.rs` play tests.

### `bench_mcp_audio_tones_level` [@ANCHOR: bench_mcp_audio_tones_level]

`tones::level(args, default) -> Result<f64, String>` reads `level_dbfs` from the tool arguments (a number, dBFS; `default` when absent: -12 for play, -20 for loopback)
and refuses anything outside -90 to `MAX_LEVEL_DBFS` (-1) dBFS. The ceiling of -1 dBFS keeps a generated tone below digital clipping; it is not the safety
limit (the volume cap in `safety::limits` is). Error: `level_dbfs <v> is outside -90 to -1 dBFS`. Pure. Tests: `tones.rs`, loopback and play tests in `tools.rs`.

## Device choice, recording, playing, volume (`tools.rs`)

### `bench_mcp_audio_tools_bounded` [@ANCHOR: bench_mcp_audio_tools_bounded]

`bounded(args, k, default, lo, hi) -> Result<f64, String>` (private, also a copy in `verify.rs`) reads a number argument with a default and refuses a value outside
`lo..=hi` or not finite. It is how every duration, rate and interval in the sound tools is limited before anything is opened. Errors: `<k> <v> is outside <lo> to <hi>`.
A key that is present but not a number counts as absent and gets the default. Pure. Used by record, meter, source, loopback and the sample-rate helper (8000 to 96000 Hz).

### `audio_select_auto` [@ANCHOR: audio_select_auto]

`tools::pick(ctx, args, key, dir) -> Result<&Device, String>` chooses the device a tool uses; the `auto` branch is the anchored part. Order: the device named in
`args[key]` (resolved by alias, node name or unique fragment, `devices::resolve`); else the configured `alert_output` (outputs) or `verify_input` (inputs) when it is not
`auto`; else, for `auto`, the device the last discovery run found by listening (`ctx.discovery`), but only if it is still present, of the right direction and not reserved.
The system default device is never used, because it may be the user's headset or a reserved device. Errors: the resolver's own (unknown, ambiguous, reserved, wrong
direction); or, when nothing is named, configured or found, a message that says to name one with the key, set it in the config or run `bench_audio_discover`.
Borrowed return value, no I/O. Called by record, meter, play, loopback, verify, events and speak. Tests: `audio_select_auto` in `tools.rs`; the README lists it under discovery.

### `bench_mcp_audio_tools_take` [@ANCHOR: bench_mcp_audio_tools_take]

`take(rec, rate, seconds, trigger_dbfs, max_wait_s) -> Result<Taken, String>` (private) collects `seconds` of samples from a running `Recorder`. It first waits up to 5 s for the first
sample. With no trigger it waits for `seconds*rate` samples (timeout `seconds+5` s) and returns the first that many, `triggered: None`. With a trigger it scans the stream in
10 ms frames; at the first frame whose absolute peak is at or above `trigger_dbfs` it starts 0.1 s earlier (pre-roll), waits for the full length and returns that, `triggered:
Some(true)` and the time it waited. If `max_wait_s` passes without a trigger it returns everything heard, `triggered: Some(false)`, `waited_s = max_wait_s`.
Errors: the recorder's timeouts (no stream data). Blocks the calling thread (sleeps 10 ms between scans); it does not own the recorder, the caller drops it. Tests: `tools.rs` record tests.

### `audio_record_tool` [@ANCHOR: audio_record_tool]

`tools::record(ctx, args)` is `bench_audio_record` (tier `read_config`): record an input to a WAV file in the managed capture directory and report its levels.

| Argument | Type, unit, range | Default |
| --- | --- | --- |
| `input` | device name; with `monitor_of_output: true` an OUTPUT is named and its monitor is recorded | configured or discovered |
| `seconds` | 0.1 to `max_record_s` | 3 |
| `sample_rate_hz` | 8000 to 96000 | 48000 |
| `start_on_threshold_dbfs` | -120 to 0, start when the level reaches it | none |
| `max_wait_s` | 0.1 to 120, how long to wait for the threshold | 10 |
| `silence_threshold_dbfs` | level counted as silence | -60 |
| `label` | name part of the file | the device alias |

Returns JSON: `input`, `alias`, `monitor_of_output`, `sample_rate_hz`, `seconds`, `levels` (peak, RMS, clipped samples, silence fraction, all-zero, DC offset), `note` (the room may
be heard; the file stays in the capture directory and nothing uploads it), `file`, `retention_removed`; `warning` when every sample is exactly zero (muted, unplugged or dead).
If a threshold was set and not reached: `triggered: false`, `result` text and no file; if reached, `triggered: true` and `trigger_after_s`. Errors: a bad argument, a device that is
reserved or not found, the recorder failing to start or timing out, a file write failure. Side effects: one `pw-record` subprocess for the duration, one WAV file (new name,
owner-only directory), then old files are pruned by `retention_files` and `retention_days`. It never makes sound and never touches the volume. Test: `tests/hil_audio.rs`, unit tests in `tools.rs`.

### `bench_mcp_audio_tools_meter` [@ANCHOR: bench_mcp_audio_tools_meter]

`tools::meter(ctx, args)` is `bench_audio_meter` (`read_config`): level readings over time, nothing saved. Arguments: `input` (as pick), `seconds` 0.2 to 30 (default 3), `interval_ms` 50 to 2000 (default
250). It records at 48000 Hz for the whole time, splits the samples into whole intervals (a short last piece is dropped) and gives for each the time `t_s`, `peak_dbfs` and `rms_dbfs`.
Returns `readings`, `overall` (the same statistics as record, silence threshold -60 dBFS), `interval_ms`, `input` and the room-hearing `note`. Errors: bad arguments, device errors, recorder timeout.
Blocks for the whole listening time; no file, no sound. Test: `bench_mcp_audio_tools_meter` in `tools.rs`.

### `bench_mcp_audio_tools_source` [@ANCHOR: bench_mcp_audio_tools_source]

`source(ctx, args) -> Result<Source, String>` (private) prepares the sound for the play tool: a stored file or a generated tone. With `file` it resolves the name inside the managed capture
directory only (`store::managed_file`: no path outside it), reads the WAV and refuses one longer than `max_play_s`. Without it, it makes a tone from `tone`, `freq_hz`, `freq2_hz` (`tones::parse`),
`seconds` (0.05 to `max_play_s`, default 1) and `level_dbfs` (default -12), writes a 48000 Hz mono WAV in the capture directory and marks it `temporary`. `Source` holds `path`,
`seconds`, `temporary` (the caller deletes it) and a `description` for the answer. Errors: file not in the directory or unreadable, too long, bad tone or range, write failure.
Side effect: a file. The caller (`play`) removes a temporary file after playing or on a dry run. Test: `bench_mcp_audio_tools_source`.

### `bench_mcp_audio_tools_dry_plan` [@ANCHOR: bench_mcp_audio_tools_dry_plan]

`dry_plan(ctx, dev, lim, want, would) -> Result<Value, String>` is the answer to `dry_run: true` for every tool that would make sound (play, speak, loopback, alert). It changes nothing: no sound, volume or mute.
It refuses a reserved device (`<node> is reserved: <why>`), reads the output's current volume and mute with `wpctl` (`levels::get`), and reports what the volume would be: the current one (`Want::Keep`) or the
requested one (`Want::Level`), in both cases not above `lim.volume`. Returns `dry_run: true`, `output`, `alias`, `would` (the caller's own description of the action), `volume_now`, `muted_now`,
`volume_would_be`, `volume_limit`, `quiet_hours` and a note. Errors: reserved device; failure to read the level. Side effect: one read-only `wpctl` call. Test: `bench_mcp_audio_tools_dry_plan`.

### `bench_mcp_audio_tools_play_guarded` [@ANCHOR: bench_mcp_audio_tools_play_guarded]

`play_guarded(ctx, dev, lim, want, unmute, sound) -> Result<(Applied, Duration), String>` is the one place sound is made under the volume guard. It applies the volume (`VolumeGuard::apply`: refuses
a reserved device; sets the volume to the wanted level or the current one, never above `lim.volume`; unmutes only if `unmute`), runs the closure `sound` (which makes the noise and returns how long it
took), and then always restores the volume and mute, whether or not the sound succeeded. If both the sound and the restore fail, the sound's error is returned first; a restore failure alone is an
error too (the sound was made but the state could not be put back). Returns what was applied (volume used, the state before, whether still muted) and the time taken. Blocks for the sound.
`Drop` of the guard restores too, so a panic inside `sound` still puts the volume back. Used by play and speak. Test: `bench_mcp_audio_tools_play_guarded`.

### `audio_play_tool` [@ANCHOR: audio_play_tool]

`tools::play(ctx, args)` is `bench_audio_play` (tier `full`): plays a generated tone or a managed WAV on one output, in the volume cap and quiet hours.

| Argument | Meaning, range | Default |
| --- | --- | --- |
| `output` | device (never the system default; see `audio_select_auto`) | configured or discovered |
| `volume` | 0 to 1 wpctl volume, then limited to the cap or quiet volume; outside 0 to 1 is an error | keep the current, lowered to the limit if above |
| `file` | a WAV name in the capture directory | none |
| `tone`, `freq_hz`, `freq2_hz`, `seconds`, `level_dbfs` | as `tones::parse`, `seconds` 0.05 to `max_play_s`, `level_dbfs` default -12 | sine 1000 Hz, 1 s |
| `unmute` | unmute for the playback and re-mute after | false |
| `dry_run` | answer with `dry_plan` and make no sound | false |

Returns: `output`, `alias`, `played`, `seconds`, `took_s`, `volume_used`, `volume_limit`, `quiet_hours`, `volume_restored`, `mute_restored`; `warning` if the output is muted so nothing was audible. Errors: bad
arguments, reserved or unknown device, file problems, `pw-play` failing, restore failing. The stream is started with fallback and reconnect disabled, so a missing node fails instead of playing on the default
device. Side effects: a `pw-play` subprocess (waited up to the length plus 5 s, killed when dropped), a temporary WAV removed afterwards, volume and mute changed and restored. Test: `tests/hil_audio.rs`, `audio_play_tool` in `tools.rs`.

### `audio_loopback_tool` [@ANCHOR: audio_loopback_tool]

`tools::loopback(ctx, args)` is `bench_audio_loopback` (`full`): plays a tone on one output while recording one input and says whether the input heard it.

| Argument | Range | Default |
| --- | --- | --- |
| `output`, `input` | devices | as pick |
| `freq_hz` | 100 to 12000 | 1000 |
| `seconds` | 0.2 to min(`max_play_s`, 5) | 0.5 |
| `level_dbfs` | -90 to -1 | -20 |
| `volume` | 0 to 1 | the limit itself (so the tone is as loud as allowed) |
| `urgent` | true gets the cap instead of the quiet-hours volume | false |
| `dry_run` | plan only | false |

It starts the recorder, waits 0.5 s of lead (the noise before), starts the playback, waits for it and 0.4 s of tail, stops, and judges with `heard::hear_tone` (a 12 dB margin). Returns `result`
(heard or not, level, reason), `noise_before`, `verdict`, `tone_hz`, volume figures and `latency_ms_indicative` with a note that it includes process start-up and buffering and is not the device latency; `input_state` "dead: exact
zeros" for an all-zero input. Errors: bad arguments, devices, recorder or playback failure, restore failure. Side effects: the output is unmuted and restored (the guard); a temporary WAV; two subprocesses. Test: `tests/hil_audio.rs`.

### `bench_mcp_audio_tools_levels_get` [@ANCHOR: bench_mcp_audio_tools_levels_get]

`tools::levels_get(ctx, args)` is `bench_audio_levels` (`read_only`): volume, mute, gain and cap. Arguments: `direction` (`input` or `output`; any other word is an error) and `device` (one device by name;
direction defaults to output for the lookup). With no device it reads every non-reserved device, filtered by direction. Each row: `name`, `alias`, `direction`, `volume` (wpctl scale), `muted`, `gain_db` (the
volume cubed, in dB), `volume_cap` (for outputs, `Config::cap_for`; null for inputs). The result also says the volume scale: the signal amplitude factor is the cube of the wpctl volume. Reserved devices are
skipped in the list (not even asked) and refused when named. Errors: bad direction, unknown device, `wpctl` failure. Side effect: one `wpctl get-volume` per device; nothing is changed. Test: `bench_mcp_audio_tools_levels_get`.

### `bench_mcp_audio_tools_set_volume_plan` [@ANCHOR: bench_mcp_audio_tools_set_volume_plan]

`set_volume_plan(dev, before, vol, mute) -> Value` (private) is the answer to `dry_run: true` of `bench_audio_set_volume`: `dry_run: true`, the device, `now` (volume, muted), `would` (the requested volume and mute, null for
the one not asked) and the note "nothing was changed". Pure, no errors. It does not clamp: the caller has already refused a volume above the cap, so the planned value is always allowed (a mute-only request is planned
without a cap check). Called twice in `set_volume`, once after the cap check and once for a mute-only request. Test: `bench_mcp_audio_tools_set_volume_plan`.

### `bench_mcp_audio_tools_safe_state` [@ANCHOR: bench_mcp_audio_tools_safe_state]

`tools::safe_state(ctx, _args)` is `bench_audio_safe_state` (tier `read_config`, safe at any time; use it first when sound went wrong). It makes no sound and never mutes, unmutes or touches a reserved device. Steps: stop every
`pw-play` stream this server started (by `engine::PLAY_SIGNATURE`, the exact command line with the no-fallback options, so nothing else matches; run as `pkill -f`, which is part of the server's own tool, not a
shell used by an agent), then for each non-reserved output lower it to its cap if it is above (tolerance 0.004) and read it back. Returns `verified: true`, `streams` (text: stopped or none playing),
`lowered_to_cap` (node names) and `outputs` (name, alias, volume capped, cap). Errors, all prefixed `SAFE STATE NOT CONFIRMED`: the process lookup could not run; an output stayed above its cap after being set, with the advice to
lower it by hand and tell the owner; or a level read/set failed. No `transmit` of any kind. Test: `bench_mcp_audio_tools_safe_state`.

### `audio_set_volume_tool` [@ANCHOR: audio_set_volume_tool]

`tools::set_volume(ctx, args)` is `bench_audio_set_volume` (`full`): the volume and mute of one device, and an output never above its cap. Arguments: `device` (required), `direction` (`input`, else output), `volume` (0 to the cap for an output, 0 to 1 for
an input), `mute` (bool), `dry_run`. At least one of volume and mute must be given. Returns `before` and `after` (volume, muted) and the name and alias; with `dry_run` the `set_volume_plan`. Errors: `device is required`, `give
volume, mute, or both`, a volume outside 0 to the cap (the message names the cap), reserved or unknown device, `wpctl` failure. Unlike play this is a lasting change: nothing restores it. Side effects: `wpctl set-volume` and
`set-mute` on the device. Test: `tests/hil_audio.rs`, `audio_set_volume_tool` in `tools.rs`.

## Verification and events (`verify.rs`)

### `bench_mcp_audio_verify_bounded` [@ANCHOR: bench_mcp_audio_verify_bounded]

`verify::bounded(args, k, default, lo, hi) -> Result<f64, String>` (private) is the same limit check as the private one in `tools.rs` (a second copy; the code does not say why it is not shared): a number argument with a default, refused when not finite or outside `lo..=hi` (`<k> <v> is outside <lo> to <hi>`).
Used for `min_over_noise_db`, `min_similarity`, `calibrate_s`, `window_s`, `delay_s`, `sample_rate_hz` and the event parameters. Pure. Test: with the expectations tests.

### `bench_mcp_audio_verify_expectations` [@ANCHOR: bench_mcp_audio_verify_expectations]

`verify::expectations(args) -> Result<Expect, String>` reads what the caller expects of the sound into an `Expect`.

| Argument -> field | Meaning, unit | Default |
| --- | --- | --- |
| `min_over_noise_db` -> `min_over_noise_db` | dB over the noise floor that counts as sound, 3 to 80 | 10 |
| `min_peak_dbfs` | loudest peak at least this, dBFS | none |
| `expect_hz` -> `hz`, `tolerance_hz` | dominant frequency and its tolerance (Hz) | tolerance 5 % of the pitch, 0 with no pitch |
| `freq_min_hz`, `freq_max_hz` | dominant frequency range | none |
| `min_duration_s`, `max_duration_s` | longest event bounds | none |
| `expect_beeps`, `min_beeps` | exact or minimum event count | none |
| `expect_silence` | true: nothing may rise over the floor | false |
| `min_similarity` | spectral similarity to the reference, 0 to 1 | 0.8 |

Error: only the two bounded fields can fail. Pure; the type is `Clone, Default`. Used by `verify`, judged by `judge`. Test: `bench_mcp_audio_verify_expectations`.

### `audio_verify_judge` [@ANCHOR: audio_verify_judge]

`verify::judge(e, ev) -> (bool, Vec<Check>, String)` decides pass or fail from `Evidence` (input all zero, noise floor and threshold in dBFS, the detected `Event`s, the dominant frequency, the similarity) against the `Expect`. It returns the verdict, one
`Check` (name, ok, detail) for each thing asked, and a one-line reason: every detail when all passed, `name: detail` of each failed check otherwise. The answer is therefore the evidence, never a bare boolean.
Rules: an all-zero input fails at once with the single check `input_alive` ("nothing can be concluded"). With `expect_silence` the check is `silence` (no events). Otherwise `sound_present` (at least one event). When sound is present and silence is not expected the optional
checks run: `level`, `frequency` (within tolerance; a broadband sound with no dominant tone fails), `frequency_range`, `min_duration`, `max_duration` (the longest event). `beep_count` (exactly), `min_beeps` and `reference_similarity` (at least `min_similarity`) run whenever asked.
Pure, no I/O. Test: `audio_verify_judge` (many cases in `verify.rs`).

### `bench_mcp_audio_verify_run_action` [@ANCHOR: bench_mcp_audio_verify_run_action]

`run_action(argv, timeout) -> Value` (private) runs a verify action and waits. `argv` is a program and arguments from the config's `verify_actions` (a list, no shell, so no quoting or injection), run with stdin and stdout closed and stderr piped. It polls every 10 ms. Returns
`{"ran": true, "exit": code, "took_s"}`; on timeout the child is killed and waited for and the answer has `timed_out: true`; when the program cannot start `{"ran": false, "error"}`. `exit` is null if the child was killed by a signal. The caller adds `action` (the name) and
`started_s_after_calibration`. It never fails with an Err: the failure is the report. The timeout used by `verify` is the window length (at least 1 s) plus 5 s. The action can be anything the owner configured, so what it does (for example a command that starts a generator) is the owner's; it is the only subprocess here that is not an audio program. Test: `bench_mcp_audio_verify_run_action`.

### `audio_verify_tool` [@ANCHOR: audio_verify_tool]

`verify::verify(ctx, args)` is `bench_audio_verify` (`read_config`): did the device under control make the sound the caller expected? It records, measures the noise floor, optionally runs an action while recording, and judges.

| Argument | Meaning, range | Default |
| --- | --- | --- |
| `input` | the listening input | configured or discovered |
| `calibrate_s` | noise-floor time before the action, 0.5 to 10 | 1.5 |
| `window_s` | listening time after, 0.5 to `max_record_s - calibrate_s` (at least 1) | min(5, the maximum) |
| `delay_s` | wait after calibration before the action, 0 to `window_s` | 0 |
| `action` | a name in `verify_actions`; an unknown name lists the configured ones | none (another tool, in parallel, makes the sound) |
| `reference` / `save_reference` | a stored reference WAV to compare with (its rate is used) / save the window as `ref-<name>.wav` | none |
| `sample_rate_hz` | 8000 to 96000 | 48000 |
| the expectations | see `bench_mcp_audio_verify_expectations` | |

The noise floor is the 90th percentile of the calibration frame levels (skipping the first 0.1 s start-up transient, or half of the calibration if shorter); with an expected pitch the events are tracked at that frequency, which hears a tone under broadband noise. Events are detected in the window; the dominant
frequency and the spectrum peaks are taken from the active samples only (at least 256 samples, else no spectrum); similarity with the reference is the spectral similarity 100 Hz to 0.9 x Nyquist, 0 when the bin widths differ or there is nothing to compare.
Returns `pass`, `reason`, `checks`, `evidence` (noise floor, threshold, calibration and window statistics, events, dominant frequency, median spectrum, peaks, reference similarity), `action` (the `run_action` result), `delay_s`, the note, and `file` (the whole recording, kept and pruned under retention) or
`saved_reference`. An all-zero input is reported by `judge`. Errors: bad arguments, no such action, reference unreadable or outside the managed directory, recorder failure, `reference:` spectrum failure, file write failure. Makes no sound itself; the action may. Test: `tests/hil_audio.rs`, `audio_verify_judge`.

### `audio_events_tool` [@ANCHOR: audio_events_tool]

`verify::events(ctx, args)` is `bench_audio_events` (`read_config`): listen for N seconds and list the sound events (beeps, clicks). Arguments: `input`, `seconds` 0.5 to `max_record_s` (default 5), `sample_rate_hz` 8000 to 96000, `min_over_noise_db` 3 to 80 (10), `threshold_dbfs` (a fixed frame RMS instead of floor plus margin),
`merge_gap_ms` 0 to 2000 (40), `min_event_ms` 1 to 5000 (5), `track_hz` (follow one tone), `decode_morse` (bool). The first 0.1 s is skipped for the start-up transient and event times are corrected by that offset. Returns `event_count`, `noise_floor_dbfs`, `threshold_dbfs`, `events` (onset, offset, levels, pitch), `levels` of the whole recording, the room note, `mean_interval_s` for two or more events,
`morse` (text, symbols, dot length, wpm, or null when nothing decodes) and a `warning` for an all-zero input. Errors: bad arguments, device or recorder failure. Nothing is saved and no sound is made. Test: `audio_events_tool` end to end in `tests/hil_audio.rs`.

## Speech (`speak.rs`)

### `bench_mcp_audio_speak_engine_name` [@ANCHOR: bench_mcp_audio_speak_engine_name]

`Engine::name(self) -> &'static str` gives the word for a speech engine as the tool's JSON shows it and the config accepts it: `speech-dispatcher`, `espeak-ng` or `piper` (the enum `Engine` has the variants `SpeechDispatcher`, `EspeakNg`, `Piper`). Pure, never fails, no side effects. Used in the answers of speak, the dry-run plan and the synthesizers report. Test: `bench_mcp_audio_speak_engine_name`.

### `audio_speak_choose` [@ANCHOR: audio_speak_choose]

`speak::choose(want, found, target, cfg_default) -> Result<Engine, String>` decides which engine speaks. `want` is `auto` or an engine name (`auto` first takes `cfg_default`, the config's `tts_engine`); `found` says what is installed (`Found`: espeak-ng, spd-say, the speech-dispatcher server socket, Piper and its model); `target` is the output device.

| want | Result |
| --- | --- |
| `speech-dispatcher` | needs `spd-say` and a target that is the system default and not reserved, since it plays only through its own default path; otherwise an error that says to use espeak-ng or piper |
| `espeak-ng` | needs `espeak-ng` |
| `piper` | needs `piper` and a configured `piper_model` |
| `auto` | speech-dispatcher if installed, its server running and the target is the usable default; else espeak-ng; else Piper; else an error that says to install espeak-ng and that nothing was installed |
| anything else | `engine "x" is not auto, speech-dispatcher, espeak-ng or piper` |

Errors say what is missing and what to install; this tool never installs anything. Pure. Tests: `audio_speak_choose`.

### `audio_speak_voices` [@ANCHOR: audio_speak_voices]

`speak::parse_voices(text) -> Vec<Voice>` parses the output of `espeak-ng --voices`: it skips the header line and takes from each line with at least four whitespace-separated fields the language (field 2), gender (field 3, as written, e.g. `M` or `--`) and name (field 4). Shorter lines are ignored; it never fails. `Voice{language, name, gender}` strings. Pure. Used by `synthesizers`. Test: `audio_speak_voices` on a recording of the real command (`tests/audio_fixtures`).

### `bench_mcp_audio_speak_safe_code` [@ANCHOR: bench_mcp_audio_speak_safe_code]

`speak::safe_code(s) -> Result<&str, String>` limits a voice or language code, which is given to a program as an argument. The code must be 1 to 40 bytes, not start with `-` (so it can never be read as an option), and use only ASCII letters, digits, `_`, `+` and `-` (so `en-us`, `en+f3`, `de_DE` pass). Error: `"x" is not a voice or language code (letters, digits, _ + - only)`. Returns the same string, borrowed. Pure. Called by `request` and when the config is read (`tts_voice`, `tts_language`). Test: `bench_mcp_audio_speak_safe_code`.

### `bench_mcp_audio_speak_spd_args` [@ANCHOR: bench_mcp_audio_speak_spd_args]

`speak::spd_args(lang, voice, rate_wpm) -> Vec<String>` builds the command line of `spd-say`: `--wait` (return when speaking ends), `-e` (read the text from stdin), `-l <lang>`, `-r <rate>` and, when a voice other than the language was asked for, `-t <voice>`. The rate is `spd_rate(rate_wpm)`: speech-dispatcher's -100 to 100 scale, `round((wpm-160)/1.4)` clamped, so 160 wpm is its default 0 and the 80 to 450 wpm range maps inside the clamp. The text is never in the arguments. The caller checks both codes with `safe_code`. Pure. The espeak-ng and Piper command lines (`espeak_args`, `piper_args`) are built the same way with the output file path. Test: `bench_mcp_audio_speak_spd_args`.

### `bench_mcp_audio_speak_run_with_stdin` [@ANCHOR: bench_mcp_audio_speak_run_with_stdin]

`run_with_stdin(program, args, input, limit) -> Result<(), String>` (private) starts a program with `input` written to its standard input (the text never appears on the command line, so no quoting problem and no text in the process list), output discarded and stderr piped. It then polls every 10 ms. Success only when it exits with status 0. Errors: cannot start; cannot write the text; non-zero exit (the message has the status and the program's stderr, trimmed); not finished in `limit` (the child is killed and waited for, `did not finish within N s`). Blocks the calling thread up to the limit. Used for `espeak-ng` and `piper` (limit 30 s) and `spd-say` (40 s). Test: `bench_mcp_audio_speak_run_with_stdin`.

### `bench_mcp_audio_speak_request` [@ANCHOR: bench_mcp_audio_speak_request]

`speak::request(ctx, args, target) -> Result<Request, String>` validates a speaking request into a `Request{text, engine, voice, language, rate_wpm}` before any sound is made.

| Argument | Rule | Default |
| --- | --- | --- |
| `text` | required, trimmed, 1 to `MAX_TEXT` (600) characters | none |
| `language` | `safe_code` | config `tts_language` (`en-us`) |
| `voice` | `safe_code` | config `tts_voice`, else the language |
| `rate_wpm` | 80 to 450 | config `tts_rate` (160) |
| `engine` | see `audio_speak_choose` | `auto` |

Errors: `text is required`, `text must be 1 to 600 characters`, a bad code, `rate_wpm must be 80 to 450`, and the engine choice errors. It looks at which programs are on the path and whether the speech-dispatcher socket exists (`detect`), and nothing else. No side effects. Test: `bench_mcp_audio_speak_request`.

### `bench_mcp_audio_speak_synthesize` [@ANCHOR: bench_mcp_audio_speak_synthesize]

`speak::synthesize(ctx, r, out) -> Result<f64, String>` writes the request's speech to the WAV file `out` with espeak-ng (`--stdin -v voice -s wpm -w out`) or Piper (`--model <piper_model> --output_file out`), with the text on stdin and a 30 s limit, then reads the file back and returns its length in seconds. Errors: speech-dispatcher (it cannot write a file); Piper without `piper_model`; the program failing or timing out; the file unreadable; the speech longer than `MAX_SPEECH_S` (30 s). The caller owns the file and removes it or keeps it. Side effects: a subprocess and one file. It does not play and does not touch the volume. Test: `bench_mcp_audio_speak_synthesize`.

### `bench_mcp_audio_speak_say` [@ANCHOR: bench_mcp_audio_speak_say]

`speak::say(ctx, dev, r) -> Result<f64, String>` makes the sound of a request on `dev`; the caller holds the volume guard (it is only called inside `play_guarded`). With speech-dispatcher it runs `spd-say` (40 s limit) and returns the elapsed time, since speech-dispatcher plays itself on the default output (which `choose` allowed only for a usable default). With espeak-ng or Piper it synthesizes to a temporary WAV in the capture directory, plays it on the chosen node with `Playing` (no fallback to another device, stream volume 1.0, waited for the length plus 5 s) and returns the speech length; the temporary file is removed whatever the result. Errors: any of `synthesize` or the playback. The audible volume is the output's, set by the guard. Test: `bench_mcp_audio_speak_say`.

### `bench_mcp_audio_speak_synthesizers` [@ANCHOR: bench_mcp_audio_speak_synthesizers]

`speak::synthesizers(ctx, _args)` is `bench_audio_synthesizers` (`read_only`): reports what speech can be made, and installs nothing. It looks for the programs and the speech-dispatcher socket, and, if espeak-ng is there, runs `espeak-ng --voices` through the runner (a failed run gives an empty list). Returns `engines` (each with installed, note; speech-dispatcher with `server_running`; Piper with its `model`), `configured_engine`, `default_voice`, `default_language`, `default_rate_wpm`, `not_installed` (what to install with `apt install`) and `espeak_voices` (language, voice, gender). Errors: only a failure of the runner itself. No sound. Test: `bench_mcp_audio_speak_synthesizers`.

### `audio_speak_tool` [@ANCHOR: audio_speak_tool]

`speak::speak(ctx, args)` is `bench_audio_speak` (tier `full`): speaks `text` on an output, or writes it to a file.

| Argument | Meaning | Default |
| --- | --- | --- |
| `text`, `voice`, `language`, `rate_wpm`, `engine` | as `request` | see there |
| `output` | device (never the system default unless configured or discovered) | alert output or discovered |
| `volume` | 0 to 1, limited to the cap or the quiet volume | keep, lowered if above |
| `unmute` | unmute for the speech | false |
| `wav_file` | a label: write the speech to a WAV in the capture directory instead of playing | none |
| `dry_run` | the plan: engine, voice, language, rate, character count, volume, nothing spoken | false |

With `wav_file` nothing is played and no volume is touched: the file is made (`synthesize`), old files are pruned, and the answer has `engine`, `voice`, `wav_file`, `seconds` and `audible: false`; speech-dispatcher gives an error here. Otherwise the speech runs inside `play_guarded`: the volume is applied and restored. Returns `output`, `alias`, `engine`, `voice`, `language`, `rate_wpm`, `seconds`, `volume_used`, `volume_limit`, `quiet_hours`, `volume_restored`, `mute_restored`, and a `warning` when muted. Errors: all of `pick`, `request`, volume outside 0 to 1, reserved device, synthesis or playback failure, restore failure. The quiet-hours and cap limits are never urgent here. Test: `audio_speak_tool` (speaking to a WAV and to a virtual output), `tests/hil_audio.rs`.
