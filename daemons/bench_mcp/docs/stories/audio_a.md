# The Audio Tools Find the Right Sound Card, Make Noise Only Within Limits and Learn by Listening What Works

As the person (or the AI agent) running the bench, I want `bench_mcp` to list the sound cards PipeWire knows, record from one and play to
another by name, never touch the sound cards inside the VNWA, never play louder than a configured cap or in quiet hours, and find out by
listening which output really makes sound and which input really hears, so a test can say "the alarm beeped three times" instead of "the
command returned". This story documents the core of that: the module root and the instrument's `audio` settings
(`daemons/bench_mcp/src/audio/mod.rs`), the PipeWire streams (`engine.rs`), the device list and name resolution (`devices.rs`), sound-event
detection and the Morse decoder (`events.rs`) and audio path discovery (`discover.rs`). The tool pages for the agent are in
`docs/AUDIO_TOOLS.md`. None of these functions keys a transmitter: they drive sound cards through `pw-dump`, `pw-record`, `pw-play` and
`wpctl`, never a radio's PTT; the safety that applies is the tier of the `audio` instrument, the volume cap, the quiet hours and the reserved
list, and every entry says which of them it touches.

## The module root: running programs, the `audio` settings, picking the instrument, dispatching a tool

### `Process::run` [@ANCHOR: bench_mcp_audio_process_run]

The production implementation of the `Run` trait: it runs one program with its arguments, waits for it to end and returns whether it
succeeded together with everything it printed. All the short-lived helper programs of the audio tools (`pw-dump`, `wpctl`, `espeak-ng`, `spd-say`
and so on) go through this one function, so a test can script the `Run` trait instead and a production call is the same code path. Callers are
`devices::list`, the level and volume code and the speech code.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `Process` (unit struct) | Holds no state. |
| `program` | `&str`, borrowed | Program name looked up on `PATH`, or an absolute path. Never passed through a shell. |
| `args` | `&[String]`, borrowed | Arguments, one element each; no quoting or expansion happens. |

Return value: `Ok((success, text))` where `success` is the exit status (`true` for zero) and `text` is stdout followed by stderr, both decoded
with lossy UTF-8 (invalid bytes become U+FFFD). A non-zero exit is not an error here: the caller decides. `Err(message)` when the program
cannot be started: `"<program> is not installed (not found on PATH)"` for a missing program, `"<program>: <io error>"` for any other spawn failure.

Side effects and timing: it blocks the calling thread until the program exits; there is no timeout, so a hung `pw-dump` hangs the tool call.
Stdin is inherited from `Command::output` defaults (null). It holds no lock. See also: the tests of `mod.rs` cite it through the scripted runner.

### `QuietHours::contains` [@ANCHOR: bench_mcp_audio_quiethours_contains]

Says whether a minute of the local day falls inside the configured quiet hours. Quiet hours make the audible tools drop to the quiet volume (or
refuse, for discovery), so this predicate is evaluated for every sound-making call, by `safety::limits`, `status`, `wants_rediscovery` and
`discover::run`.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `QuietHours` | `start` and `end`, minutes since local midnight, 0 to 1439. |
| `minute` | `u32` | The minute of the local day to test, normally from `local_minute_of_day`. |

Return value: `true` when `minute` is in the span. The span includes `start` and excludes `end`. When `start <= end` it is the plain range
`start..end`; when `start > end` the span crosses midnight and is `minute >= start || minute < end` (the default 22:00 to 07:00 is
1320 and 420). `start == end` cannot be built through `parse_config` (it is refused); built by hand it is an empty span. No errors, no panic.
Pure function, no locks, no side effects. Tested through `audio_config` and quiet-hour tests in `mod.rs`.

### `Config::alias_of` [@ANCHOR: bench_mcp_audio_config_alias_of]

Finds the config alias (the short name an operator wrote in `outputs` or `inputs`) that names a device. The result fills `Device::alias`, which
is what `devices::resolve` matches first and what `bench_audio_devices` prints in its `alias` field.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `Config` | The parsed `audio` entry. |
| `d` | `&devices::Device`, borrowed | The device. Its `dir` picks the map: `outputs` for an output, `inputs` for an input. |

Return value: `Some(alias)` for the first alias whose matcher fits, `None` for no match (the device is then used by node name only). A matcher
fits when it equals the node name ignoring case, or when the lower-cased node name or the lower-cased description contains the lower-cased
matcher. The aliases are walked in `BTreeMap` order, that is alphabetically, so when two aliases fit one device the alphabetically first wins.
An empty matcher cannot occur (`ports` refuses it). No errors, no panic, pure, no locks.

### `Config::cap_for` [@ANCHOR: bench_mcp_audio_config_cap_for]

The highest `wpctl` volume that any tool may set or play at on one device. It is the safety ceiling used by `safety::limits`,
`bench_audio_set_volume`, the volume guard and `bench_audio_devices` (field `volume_cap`).

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `Config` | Supplies `volume_cap` (global, default 0.5) and the alias tables. |
| `d` | `&devices::Device`, borrowed | The device; its `alias` and `dir` select an alias entry. |

Return value: `f64` on the `wpctl` scale (0 to 1; the signal amplitude factor is the cube of it, so 0.5 is about -18 dB). When the device has an
alias whose entry carries its own `volume_cap`, the result is the smaller of that and the global cap; the global cap can therefore never be
exceeded by an alias. Without an alias or without an own cap the result is the global cap. A device with an alias but looked up in the wrong map
(never the case for a device built by `devices::parse`) falls back to the global cap. No errors, no panic, pure, no locks.
Tested in the cap tests of `mod.rs`.

### `hhmm` [@ANCHOR: bench_mcp_audio_hhmm]

Parses a clock time such as `"22:00"` into minutes since midnight, for `quiet_hours.start` and `quiet_hours.end`. Private to `mod.rs`; called
only by `parse_config`.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&str`, borrowed | Text `H:MM` or `HH:MM`. The part before the colon is the hour 0 to 23, the part after is the minute 0 to 59. |

Return value: `Ok(minutes)` in 0 to 1439 (`h * 60 + m`). Errors (the text goes to the config error): `time must look like "22:00"` when there is no
colon, `bad hour` or `bad minute` when a part is not an unsigned integer, and `"<s>" is not a time of day` when the hour is over 23 or the minute
over 59. A single-digit minute such as `7:5` is accepted as 7:05. Pure, no side effects, no locks.

### `ports` [@ANCHOR: bench_mcp_audio_ports]

Reads the `outputs` or `inputs` object of the `audio` entry into a map from alias to `Port` (the matcher and an optional own volume cap). Private
to `mod.rs`; called twice by `parse_config`.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&serde_json::Value`, borrowed | The whole `audio` entry. |
| `key` | `&str` | `"outputs"` or `"inputs"`. |

Each member of the object is `alias: "fragment"` (the string is the matcher) or `alias: {"match": "fragment", "volume_cap": 0.3}`. The matcher is a
node name or a fragment of the node name or description (see `alias_of`). `volume_cap`, when a number, must be above 0 and at most 1; a value that is
not a number is ignored (the alias then has no own cap).

Return value: `Ok(map)`; the map is empty when the key is absent. Errors, each naming the key and alias: `<key> must be an object`; `<key>: "<alias>" is not
a usable alias` for an empty or whitespace alias or the reserved word `auto`; `<key>.<alias>: no "match"` for an object without a string `match`;
`<key>.<alias> must be a string or an object`; `<key>.<alias>: match is empty`; `<key>.<alias>: volume_cap must be above 0 and at most 1`. The result
is a `BTreeMap`, so iteration (and the order of `status` output) is alphabetical by alias. Pure, no side effects, no locks.

### `parse_config` [@ANCHOR: audio_config]

Turns the `audio` settings of a bench config instrument entry (`"driver": "audio"`) into a `Config`, and refuses a bad entry when the config file is
read, before any tool runs. Called by the config reader in `crate::config` for every audio instrument; every other part of `audio` reads the
resulting `Config`.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&serde_json::Value`, borrowed | The instrument's JSON object. A value that is not an object yields all defaults. |

Settings read, with the default (the default is the value of `Config::default()`):

| Key | Type, range | Default | Notes |
|---|---|---|---|
| `outputs`, `inputs` | object alias to string or `{match, volume_cap}` | empty | See `ports`. |
| `alert_output` | `"auto"` or an alias of `outputs` | `"auto"` | Any other name is an error. |
| `verify_input` | `"auto"` or an alias of `inputs` | `"auto"` | Any other name is an error. |
| `volume_cap` | number, above 0 to 1 (wpctl scale) | 0.5 | The ceiling for every tool. |
| `quiet_volume` | number, above 0 to 1 | 0.15 | Silently lowered to `volume_cap` when higher. |
| `quiet_hours` | `{"start":"22:00","end":"07:00"}` or `false` | 22:00 to 07:00 | `false` switches quiet hours off; start equal to end is an error; any other non-object value is an error. |
| `capture_dir` | path string | none; `Config::capture_dir()` gives `<bench out dir>/audio` | Where recordings, references and the discovery file live. |
| `retention_files`, `retention_days` | unsigned integers | 50, 14 | A value that is not an unsigned integer is ignored. |
| `max_record_s` | 1 to 600 seconds | 30 | Out of range is an error. |
| `max_play_s` | 0.1 to 60 seconds | 10 | Out of range is an error. |
| `tts_engine` | `auto`, `speech-dispatcher`, `espeak-ng`, `piper` | `auto` | Anything else is an error. |
| `tts_voice`, `tts_language` | string accepted by `speak::safe_code` | none, `en-us` | Rejected text is an error naming the key. |
| `tts_rate` | integer 80 to 450 words per minute | 160 | Out of range is an error. |
| `piper_model` | path string | none | Not validated here. |
| `verify_actions` | object name to non-empty list of strings (program, then arguments) | empty | The only commands `bench_audio_verify` runs; no shell. |
| `reserved` | list of non-empty strings | `["08bb:29c0"]` | Lower-cased and appended; `08bb:29c0` is always present; duplicates dropped. |

Return value: `Ok(Config)`; `Err(message)` for the first problem found, in the order of the table (alias tables first). Messages name the key
(for example `alert_output "x" is neither "auto" nor an alias in outputs`, `max_record_s must be 1 to 600`,
`verify_actions.<name> must be a non-empty list of strings`, `reserved entries must be non-empty strings`). No panic. Pure: it reads no file and
starts no program. The `Config` is a plain owned value, cloned freely, read-only after parsing, with no locks.
Invariants after success: `0 < quiet_volume <= volume_cap <= 1`; the reserved list contains `08bb:29c0`; an alias named by `alert_output` or
`verify_input` exists. Tested in `mod.rs` (`Tests [@ANCHOR: audio_config]`).

### `local_minute_of_day` [@ANCHOR: bench_mcp_audio_local_minute_of_day]

The clock the quiet hours are judged by: minutes since local midnight now, from the C library's `time` and `localtime_r`, so the system time zone
and daylight saving apply. Called by `call` (the production entry point) once per tool call and passed down as `minute` so one call sees one time.

No arguments. Return value: `u32` in 0 to 1439 (`tm_hour * 60 + tm_min`). No errors: if `localtime_r` failed the zeroed `tm` would give 0 (midnight),
which the code does not detect. The function uses `unsafe` for the two libc calls, which write only to local variables; `localtime_r` is the
re-entrant variant, so it is safe from several threads. No locks, no files. Tested with a range check in `mod.rs`.

### `instrument` [@ANCHOR: bench_mcp_audio_instrument]

Chooses which configured `audio` instrument a tool call is for, from the optional `instrument` argument every audio tool accepts. Private to
`mod.rs`; called once by `call_with`.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | Every instrument in the bench config; only those with `kind == Kind::Audio` count. |
| `args` | `&serde_json::Value`, borrowed | The tool arguments; `instrument` is read as a string. |

Return value: `Ok(&Instrument)` borrowed from `insts`. With `instrument` given, the audio instrument of that name. Without it, the only audio
instrument. Errors: `no audio instrument named "<n>"` (also when the name belongs to a non-audio instrument), `no audio instrument is configured`, and
`name the instrument: a, b` when several exist and none was named (the list shows the names). Pure; no locks.

### `list_devices` [@ANCHOR: bench_mcp_audio_list_devices]

Implements the tool `bench_audio_devices` (tier `read_only`): the device list as JSON with the config alias, caps, and, unless told not to, each
device's volume and mute. It opens no audio stream. Private to `mod.rs`, called from `call_with`.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Run` | Program runner (`pw-dump`, `wpctl`). |
| `asound` | `&Path` | The `/proc/asound` directory (tests give a fixture). |
| `cfg` | `&Config` | The instrument's settings (reserved list, caps). |
| `args` | `&Value` | Tool arguments below. |

Tool arguments: `device` (string: alias, node name or fragment; one device; a reserved device is refused here), `direction` (`output` or
`input`; with `device` it says which side, default `output`), `include_reserved` (boolean, default true; false hides the reserved devices), `levels`
(boolean, default true; false skips reading the volume), `instrument`.

Return value: `{"devices": [...], "volume_scale": "..."}`. Each entry is `Device::json()` plus `volume_cap` (`cap_for`) and, when levels are read, `volume`,
`muted` and `gain_db`, or `levels_error` when `wpctl` failed for that device. Reserved devices are listed (when not hidden) but their volume is
never read: nothing touches them. Errors: any error of `devices::list` (`pw-dump` missing or failing), of `devices::resolve` for `device`
(no match, several matches, only reserved matches), and `direction "<x>" is not output or input`. Side effects: runs `pw-dump` once and `wpctl` once
per listed non-reserved device; no sound, no change. Blocks the caller for those programs.

### `call_with` [@ANCHOR: bench_mcp_audio_call_with]

The dispatcher of every audio tool: it checks the tool name and the tier, builds the context and runs the tool. `call` is the production wrapper
that passes `Process`, `/proc/asound` and the real clock; tests call `call_with` with a scripted runner, a fixture directory and a chosen minute, so
tests and production take the same path.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Run` | Program runner. |
| `asound` | `&Path` | The `/proc/asound` directory. |
| `minute` | `u32` | Minute of the local day, used for quiet hours throughout the call. |
| `insts` | `&[Instrument]` | All configured instruments. |
| `name` | `&str` | The tool name, one of the `SPECS` (for example `bench_audio_play`). |
| `args` | `&Value` | The tool arguments (each tool's schema is its `SPECS` entry). |

Return value: `Ok(Value)` with the tool's JSON result. Errors: `unknown tool <name>`; the errors of `instrument`; `<name> needs tier <T>; "<inst>" is
configured at <T2>` when the instrument's tier is below the tool's (tiers: `read_only` for devices, status, levels, synthesizers and discovery;
`read_config` for record, meter, verify, events and safe state, which hear the room; `full` for play, set volume, loopback, speak, alert and discover,
which make noise); `devices::list` errors; and anything the tool returns. The tier is checked before any program runs, so a refused call changes
nothing. `bench_audio_devices` and `bench_audio_status` are answered first without reading the stored discovery.

Flow for the others: list the devices once, load the stored discovery from the capture directory, build the `tools::Ctx` and run the tool.
`bench_audio_alert` without `dry_run` first asks `wants_rediscovery`; when true it runs `discover::run`, lists the devices again, reloads the
result and alerts on what works now, adding `rediscovery` (`{ran, audible_seconds, alert_output}` or `{ran: false, error}`) to the answer. A failed
rediscovery does not stop the alert.

Side effects: the sound-making tools play, record, change `wpctl` volumes and restore them; the discovery tool writes the capture directory. This
function takes no lock of its own; any serialization between concurrent calls on one instrument happens outside this file (unknown from this code).
No transmitter is involved. Tests cite it in `mod.rs`.

### `wants_rediscovery` [@ANCHOR: audio_discover_auto_rerun]

Decides whether an alert should first listen again, because the sound devices changed since the last discovery (a USB card plugged in or a port
changed state) and the old choice of alert output may no longer be right. Private to `mod.rs`; called from `call_with` only for `bench_audio_alert`.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&tools::Ctx`, borrowed | The configuration, the device list, the minute and the stored discovery. |
| `args` | `&Value`, borrowed | The alert's arguments: `severity`, `output`, `rediscover`. |

Return value: `true` only when all of these hold: the config's `alert_output` is `auto`; the call names no `output`; the call does not pass
`rediscover: false`; the severity is not `urgent` (an urgent alert must not wait for a discovery); it is not quiet hours (discovery makes test tones);
and a stored discovery exists whose `changes(devices)` is not empty. No errors, pure, no locks. Idempotent: the answer depends only on the inputs.
Tested in `mod.rs` (`Tests [@ANCHOR: audio_discover_auto_rerun]`); see also `audio_discover_stale` and `audio_discover_tool`.

## The PipeWire streams: `pw-record` into memory and `pw-play` of a file

The callers resolve the node name through `devices::resolve`, which never returns a reserved device, so nothing in `engine.rs` re-checks the reserved
list. Audible volume limits are applied by the callers (the volume guard) before a stream starts.

### `NO_FALLBACK` [@ANCHOR: audio_engine_no_fallback]

A constant string of PipeWire stream properties, `node.dont-fallback=true node.dont-reconnect=true`, passed with `-P` to `pw-play` and (in the
same words) to `pw-record`. Without them, when the target node has vanished (an unplugged USB card), PipeWire silently connects the stream to the
default device, which could be a reserved VNWA card, or a headset the operator wears. With them the stream fails with "defined target not found",
which the callers report.

Location: private `const` in `engine.rs`. Type: `&str`, one string of two `key=value` properties separated by a space, taken by PipeWire as one
property list. Its companion `PLAY_SIGNATURE` (public) is the start of the whole `pw-play` command line, `pw-play -P <NO_FALLBACK>`, which
`bench_audio_safe_state` uses to find playbacks this server started; the two strings must stay in step (a test cites the anchor). Immutable,
no concurrency concern. A test in `engine.rs` pins that the playback command line starts with `PLAY_SIGNATURE`.

### `Recorder::start` [@ANCHOR: bench_mcp_audio_engine_recorder_start]

Starts `pw-record` for one node and collects the samples in memory on two background threads. The recording runs until `stop`, `shutdown` or drop.

| Argument | Type | Meaning |
|---|---|---|
| `node` | `&str`, borrowed | PipeWire node name (persistent), from `devices::resolve`. |
| `rate` | `u32`, Hz | Sample rate requested from PipeWire, for example 48000. |
| `monitor` | `bool` | `false`: `node` is an input. `true`: `node` is an output (a sink) and what it plays is recorded (`stream.capture.sink=true`). |

The command is `pw-record -P <props> --raw --target <node> --rate <rate> --channels 1 --format f32 --latency 40ms -`: mono, 32-bit float
little-endian on stdout, the no-fallback properties included.

Return value: `Ok(Recorder)`. Errors: `pw-record is not installed (not found on PATH)`, `pw-record: <io error>` for another spawn failure, and
`pw-record has no stdout` (cannot happen with piped output). A stream that cannot connect is not an error here: the process starts, ends early,
and `wait_samples` reports its stderr.

Concurrency: spawns a child process and two threads. One reader copies stdout into a shared `Vec<f32>` (a `Mutex`, whole four-byte samples only,
a partial sample is carried to the next read; a poisoned lock is recovered); the other collects stderr into a string. The buffer grows without
a bound while recording: the caller limits the duration. Opens an audio capture stream, so a microphone input hears the room; the caller documents
that, and no file is written by this function. Dropping the `Recorder` stops it. Tested in `engine.rs`: a missing target fails and never falls back to the default device.

### `Recorder::wait_samples` [@ANCHOR: bench_mcp_audio_engine_recorder_wait_samples]

Blocks until the recording holds at least `n` samples, so a caller knows the stream is live before it plays a tone.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `Recorder` | Needs `&mut` to poll the child process. |
| `n` | `usize` | Samples wanted (one channel, so samples equal frames). |
| `patience` | `Duration` | Longest wait. |

Return value: `Ok(())` once `len() >= n`. Errors: `pw-record ended early (<status>): <stderr>` when the process exited before enough samples (it first sleeps 50 ms
so the stderr thread can finish; the stderr text, for example "defined target not found", is the useful part); `pw-record delivered <got> of <n> samples
in <s> s` when `patience` ran out. Polls every 5 ms. The process is not killed on an error; the caller drops the recorder. Locks the buffer
briefly on each poll. Discovery uses `wait_samples(0.8 s of samples, 6 s)` on every candidate input.

### `Recorder::shutdown` [@ANCHOR: bench_mcp_audio_engine_recorder_shutdown]

Ends the recording: kills `pw-record`, reaps it and joins both reader threads. Private; called by `stop` (which then returns the samples) and by `Drop`.

No arguments beyond `&mut self`; no return value; no errors (a kill or wait failing because the child already exited is ignored). Idempotent:
a second call finds the child gone and the reader list empty. It blocks until the readers have seen end-of-file, which follows the kill at once.
It guarantees that no `pw-record` of this server outlives the recorder, including when the tool call panics or returns early (through `Drop`).
Takes the process, not the buffer lock. No test cites it directly; it runs in every recorder test through `stop` and drop.

### `Playing::start` [@ANCHOR: bench_mcp_audio_engine_playing_start]

Starts `pw-play` playing one WAV file on one node and returns at once with a handle.

| Argument | Type | Meaning |
|---|---|---|
| `node` | `&str`, borrowed | Output node name from `devices::resolve`. |
| `path` | `&Path`, borrowed | The WAV file; a managed capture-directory file or a generated tone. |
| `stream_volume` | `f64` | The stream volume factor passed as `--volume` with three decimals; 1.0 keeps the file's own level. The device volume limit is set separately with `wpctl`. |

Command: `pw-play -P <NO_FALLBACK> --target <node> --volume <v> <path>`, stdout null, stderr piped (read only if it fails).
Return value: `Ok(Playing)` with `started` (an `Instant`, public) set to now. Errors: `pw-play is not installed (not found on PATH)` and `pw-play: <io error>`.
This makes sound in the room as soon as the stream connects, so the callers have already applied the volume cap and quiet hours (`safety::limits`)
and hold a volume guard that restores the volume. A missing file or a missing target is reported by `wait` from `pw-play`'s exit status.

### `Playing::wait` [@ANCHOR: bench_mcp_audio_engine_playing_wait]

Waits for a playback to end, with a deadline, and consumes the handle.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `Playing`, by value | Consumed; its drop then finds the child gone. |
| `limit` | `Duration` | Longest run time, measured from `started`. |

Return value: `Ok(Duration)`, the time from start to exit, when `pw-play` exits with success. Errors: `pw-play failed (<status>): <stderr>` for a non-zero
exit (stderr read at that point); `pw-play: <io error>` when polling fails; `pw-play did not finish within <s> s and was stopped` after `limit`, in which case
the process is killed and reaped. Polls every 5 ms, blocking the calling thread. Callers pass the clip length plus a margin, so a stuck stream
cannot hold the tool or leave sound running.

### `Playing::drop` [@ANCHOR: bench_mcp_audio_engine_playing_drop]

The `Drop` implementation of `Playing`: when a handle is dropped while `pw-play` still runs (a tool call returned early on an error, or panicked), the
process is killed and reaped, so no sound outlives the call. When the child has already ended it does nothing. No arguments, no return, no
errors (kill and wait results are ignored). Idempotent. It is the second line of defence after `wait`'s deadline and the reason a failed volume
restore is not accompanied by a still-playing stream. `bench_audio_safe_state` covers the case where the server itself died. Tested in `engine.rs`.

## The device list: what PipeWire reports, what is reserved, and how a name finds one device

### `Dir::word` [@ANCHOR: bench_mcp_audio_devices_dir_word]

The direction of a device as the word used in the JSON and in error messages. `Dir` is a two-valued enum: `Output` (a PipeWire `Audio/Sink`, a
playback device) and `Input` (an `Audio/Source`, a capture device). `word(self)` returns `"output"` or `"input"` (`&'static str`). Pure, no errors, no locks. It appears in
`Device::json` (`direction`), in the fingerprint (`output|node|availability`) and in the resolve errors (`no input device matches ...`). Note that
`Dir as u8` (Output 0, Input 1) is the primary sort key of the device list. Tested in `devices.rs`.

### `Device::transport` [@ANCHOR: bench_mcp_audio_devices_device_transport]

Classifies a device by how its audio gets there: `"bluetooth"` (node name starts with `bluez_` or bus is `bluetooth`), `"network"` (node name contains `raop`, an
AirPlay sink, or starts with `tunnel.`, a PulseAudio tunnel) or `"alsa"` (everything else, the local cards). Discovery uses it to test only local ALSA
devices by default, because Bluetooth and network outputs would play tones elsewhere in the house, and `fingerprint` ignores non-ALSA devices so
a phone that comes and goes does not make the stored discovery look stale. `&self` only; returns `&'static str`; pure; no errors; no locks.
Also reported in the `transport` field of `bench_audio_devices`. Tested in `devices.rs`.

### `rates_of` [@ANCHOR: bench_mcp_audio_devices_rates_of]

Collects the sample rates in a PipeWire `EnumFormat` `rate` property. Private; used by `parse`.

| Argument | Type | Meaning |
|---|---|---|
| `v` | `&Value` | The `rate` value: a number, an object, or an array of either. |
| `out` | `&mut Vec<u64>` | Appended to. |

A number is one rate (Hz). An object (a PipeWire range or choice) contributes its `default`, `min` and `max` members when they are unsigned
integers. An array is walked element by element. Any other value (a string, null) adds nothing. No errors, no panic; the order is the order found and
the duplicates are not removed here (`parse` sorts and removes duplicates afterwards). Pure apart from the append.

### `formats_of` [@ANCHOR: bench_mcp_audio_devices_formats_of]

Collects the sample format names (for example `S16LE`, `F32LE`) from the `format` property of an `EnumFormat` entry. Private; used by `parse`.

| Argument | Type | Meaning |
|---|---|---|
| `v` | `&Value` | A string (one format), an object (the values are walked, whatever the keys), or an array. |
| `out` | `&mut Vec<String>` | Appended to. |

Numbers, booleans and null are ignored. No errors, no panic. The result keeps the order found; `parse` calls `dedup`, which removes only
neighbouring duplicates, so a format that appears twice apart stays twice (cosmetic: the list is evidence for a human, no tool decides on it). Pure apart from the append.

### `reserved_reason_in` [@ANCHOR: audio_reserved_by_topology]

Decides whether a device is reserved (never opened by any audio tool because another driver owns it) and gives the reason text. `reserved_reason`
calls it with the real `/sys`; the sysfs root is an argument so a test can give a fixture tree.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | Its `reserved` list: `vid:pid` strings (exactly nine characters with a colon) and lower-case node-name fragments. |
| `d` | `&Device` | The device; its `usb_id`, `usb_path` and `node_name` are read. |
| `sys` | `&Path` | The sysfs root, `/sys` in production. |

Return value: `Some(reason)` or `None` (usable). An entry that is a `vid:pid` matches when `usb_id` is equal; any other entry matches when the lower-cased
node name contains it. The reason reads `matches the reserved entry "<r>": another driver (the VNWA) owns this sound card and a second opener breaks its sweeps`.
The topology rule applies to the default entry `08bb:29c0`, the generic PCM2900C codec that many cheap USB sound dongles share with the VNWA:
when the device has a USB path and that path exists in sysfs (`bus/usb/devices/<path>`), the card is the VNWA's only if `vnwa_open::reserved::is_vnwa_codec`
finds the VNWA control chip (20a0:4118) beside it under the same hub; if it does not, the entry is skipped and the card is an ordinary sound card.
When the path is unknown or cannot be found in sysfs, the card stays reserved (fail closed). A user-added entry never gets the topology
exception. Pure apart from reading sysfs; no errors, no locks. Tested in `devices.rs` with fixture sysfs trees.

### `parse` [@ANCHOR: audio_devices_parse]

Turns the JSON of `pw-dump` into the list of audio devices, one `Device` per `Audio/Sink` or `Audio/Source` node. A pure function of its inputs apart from
reading `cardN/usbid` files, so it is tested on dumps saved from the dev box.

| Argument | Type | Meaning |
|---|---|---|
| `dump` | `&str` | The complete `pw-dump` output, a JSON array of objects. |
| `asound` | `&Path` | The `/proc/asound` directory. The `usbid` file of each card is read in preference to PipeWire's own vendor and product properties, because a power cycle of the VNA renumbers the cards and the kernel's answer is current. |
| `cfg` | `&Config` | For the reserved list and the aliases. |

Return value: `Ok(Vec<Device>)` sorted by direction (outputs first) and then node name. Other node kinds (streams, midi, video) are skipped.
Errors: `pw-dump output: <json error>` for unparsable text, and `a node has no node.name`.

The `Device` fields (all owned; a `Clone`, produced fresh on each call, never mutated afterwards):

| Field | Type | Meaning and source |
|---|---|---|
| `id` | `u32` | The PipeWire object id; changes on re-plug, so do not store it (use `node_name`). 0 if missing. |
| `node_name` | `String` | `node.name`, the persistent name; the identity everywhere. |
| `description` | `String` | `node.description`, else the node name. |
| `dir` | `Dir` | From `media.class`. |
| `card`, `alsa_device` | `Option<u32>` | ALSA card (`alsa.card` or `api.alsa.pcm.card`) and device number. |
| `alsa_path` | `Option<String>` | `api.alsa.path`, for example `hw:1`. |
| `bus` | `String` | The parent device's `device.bus` (`usb`, `pci`, `bluetooth`, ...), `unknown` if absent. |
| `usb_path` | `Option<String>` | The hub-tool style path (`3-2.2.2`) found in the parent's `device.sysfs.path`; used to link to the `bench_usb_*` tools. |
| `usb_id` | `Option<String>` | `vid:pid` in lower case: the kernel's `usbid` file when the card number is known, else PipeWire's USB vendor/product properties. |
| `port_type`, `availability` | `Option<String>` | From the parent's `Route` parameter for this device and direction: the port type (`hdmi`, `headphones`, ...) and `yes`, `no` or `unknown`. A `no` (unplugged jack or HDMI with no monitor) is supporting evidence, never proof. |
| `is_default` | `bool` | Named by the `default.audio.sink` / `default.audio.source` metadata. |
| `state` | `String` | PipeWire node state (`running`, `idle`, `suspended`), `unknown` if absent. |
| `channels` | `Option<u64>` | `audio.channels`, overridden by an `EnumFormat` `channels`. |
| `rates`, `formats` | `Vec<u64>`, `Vec<String>` | Sorted, de-duplicated rates; formats as described under `formats_of`. |
| `reserved` | `Option<String>` | The reason from `reserved_reason_in`, `None` when usable. |
| `alias` | `Option<String>` | From `Config::alias_of`. |

Locking, files: reads a few small files, no stream is opened and nothing is changed. Tested in `devices.rs` (`Tests [@ANCHOR: audio_devices_parse]`).

### `list` [@ANCHOR: bench_mcp_audio_devices_list]

Reads the live device list: runs `pw-dump` through the given runner and hands the text to `parse`.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Run` | The program runner. |
| `asound` | `&Path` | `/proc/asound`. |
| `cfg` | `&Config` | Reserved list and aliases. |

Return value: `Ok(Vec<Device>)`. Errors: the runner's error when `pw-dump` is not installed (`pw-dump is not installed (not found on PATH)`); `pw-dump failed
(is PipeWire running for this user?): <output>` for a non-zero exit (typically no PipeWire session for this user); and the `parse` errors. Blocks while
`pw-dump` runs, no timeout. Opens no audio stream; no sound. Called once per tool call by `call_with` and again by an alert after a rediscovery. A
device that disappears between listing and use is caught later by the no-fallback stream properties.

### `resolve` [@ANCHOR: bench_mcp_audio_devices_resolve]

Finds one device in a list from what a person or agent typed: an alias, a node name or a fragment. It is the gate through which every tool that
opens a stream gets its device, and the place that guarantees a reserved device is never returned.

| Argument | Type | Meaning |
|---|---|---|
| `devs` | `&[Device]`, borrowed | The device list. |
| `want` | `&str` | Alias or node name (exact, ignoring case), or a fragment of the node name or description (ignoring case). |
| `dir` | `Dir` | Only devices of this direction are considered. |

Return value: `Ok(&Device)` borrowed from `devs`. An exact match (alias or node name) that is unique wins; otherwise the candidates are all the devices
whose node name or description contains the text. Reserved devices are removed from the candidates. Errors: `"<want>" names only reserved devices (<names>) and the
audio tools never open them: <reason>` when every candidate is reserved; `no <direction> device matches "<want>"; bench_audio_devices lists them`
for none; `"<want>" matches N <direction> devices (<names>); use a more specific name or an alias` for several. An ambiguous name is never guessed.
Pure; no locks; no side effects. Tested in `devices.rs` (reserved, ambiguous, alias and fragment cases).

## Sound events: when it sounded, how loud, what pitch, and what the Morse said

Everything here is pure arithmetic on a mono `f32` sample slice (full scale is 1.0), used by `bench_audio_events` and `bench_audio_verify` and
tested on synthetic recordings (`analysis::synth`).

### `tone_levels` [@ANCHOR: bench_mcp_audio_events_tone_levels]

The level of one frequency, over time: the amplitude of a single tone in dBFS (the peak amplitude of a sine at that frequency; a full-scale sine reads about 0 dBFS) in
windows of 20 ms starting every `frame_ms`. It finds a beep far below the broadband noise of a room, because noise outside the one frequency is rejected.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&[f32]` | The samples. |
| `rate` | `u32`, Hz | The sample rate. |
| `hz` | `f64`, Hz | The tone to follow. |
| `frame_ms` | `f64`, ms | The hop between windows; at least one sample. |

Return value: one `f64` dBFS per window (`analysis::tone_amp`, Hann window at the exact frequency, converted with `db`; `-200` is the floor for silence). The window starts at
sample `i * hop` and is kept while it fits whole inside `s`, so a signal shorter than 20 ms gives an empty list. A window under 8 samples (a rate
under 400 Hz) gives the floor. No errors, no panic. Pure. A window starts at its frame, so the sound is centred 10 ms later; `detect` adds that shift to event times.

### `levels_for` [@ANCHOR: bench_mcp_audio_events_levels_for]

Chooses which level series event detection works on. `p.tone_hz` set: `tone_levels` for that frequency. Otherwise `frame_levels`, the RMS of consecutive
frames of `p.frame_ms` (a short last frame is dropped). Arguments: `s: &[f32]`, `rate: u32` Hz, `p: &Params` (borrowed). Return value: `Vec<f64>` of dBFS,
one per frame, index `i` covering sample `i * frame_samples` onward. No errors, no panic, pure. Called by `detect` and by the verification code
that needs the same series.

### `percentile` [@ANCHOR: bench_mcp_audio_events_percentile]

The value at a given position of the sorted list, used to estimate the noise reference (the 20th percentile of broadband frame levels, the median of
tone levels).

| Argument | Type | Meaning |
|---|---|---|
| `v` | `&[f64]` | The values; not modified (a sorted copy is made). |
| `p` | `f64` | The fraction from 0.0 (lowest) to 1.0 (highest). |

Return value: the element at index `round((len - 1) * p)` of the ascending order (`total_cmp`, so NaN sorts to an end without a panic); `-200 dBFS` (`FLOOR_DB`) for an empty slice.
A `p` over 1.0 would index past the end and panic: callers pass constants 0.2 and 0.5. No other errors. Pure, no locks. Not an interpolating percentile.

### `detect` [@ANCHOR: audio_events_detect]

Finds the sound events in a recording: where each starts and stops, how loud, and the pitch when there is a clear one. Used for relays clicking, a
rotor motor, an alarm, beeps and keyed CW.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&[f32]` | The mono samples. |
| `rate` | `u32`, Hz | The sample rate. |
| `p` | `&Params` | The settings below. |

`Params` (default in brackets): `frame_ms` (5 ms; the time resolution), `margin_db` (10; how far over the noise a frame must be), `threshold_dbfs` (none; a fixed RMS threshold that replaces noise plus margin),
`noise_dbfs` (none; a separately measured noise reference, for example from a calibration window), `merge_gap_ms` (40; silence shorter than this between two
sounds joins them), `min_event_ms` (5; shorter sounds are dropped) and `tone_hz` (none; follow one frequency).

Return value: `Detection { noise_dbfs, threshold_dbfs, events }`. Method: the level series from `levels_for`; the noise is `noise_dbfs`, else the 20th percentile of the levels (the median when
tracking a tone, because narrowband levels fluctuate more); the threshold is `threshold_dbfs`, else the noise (never taken below -96 dBFS, the 16-bit floor, so
digital silence does not turn every frame into an event) plus `margin_db`. A sound starts at a frame at or above the threshold and ends at the first frame more than 3 dB below it
(hysteresis); a sound still going at the end is closed at the end. Sounds closer than the merge gap are joined; those shorter than `min_event_ms` (at least 20 ms when tracking a tone, the window
length) are dropped. Each `Event`: `onset_s` and `offset_s` (seconds from the start, shifted 10 ms for a tone), `peak_dbfs` (largest absolute sample in the span), `rms_dbfs`, and `hz`,
the strongest spectral peak between 100 Hz and 90 % of Nyquist and at least 15 dB over the median of that range (on at most the first 16384 samples of the
span), `None` for clicks and noise. `Event::json` rounds times to 1 ms and levels and pitch to 0.1.

No errors, no panic (an empty or short slice gives no events; `frame_ms` of 0 acts as one sample). Pure, no locks. Tested in `events.rs`
(`Tests [@ANCHOR: audio_events_detect]`).

### `decode_morse` [@ANCHOR: audio_morse]

A simple Morse decoder for keyed tones found by `detect`, so a test can verify that a keyer or beacon sent the expected text.

| Argument | Type | Meaning |
|---|---|---|
| `events` | `&[Event]` | The marks in time order (each event is one key-down). |

Return value: `None` for no events; otherwise `Morse { text, dot_s, wpm, symbols }`: the decoded text (trimmed), the dot length in seconds, the speed in words per
minute (`1.2 / dot_s`, the PARIS standard) and the symbols as dots and dashes with spaces between letters and ` / ` between words. Method: the mark lengths are split at the midpoint
between the shortest and the longest when the longest is more than twice the shortest (shorter are dots, longer are dashes); otherwise every mark is a dot, so a message of only dashes of one length reads as
dots. The dot length is the mean of the dots. A gap over 2 dots ends a letter, a gap over 5 dots ends a word. A letter not in the table (A-Z, 0-9 and `. , ? / = + -`) is decoded as `?`.
No errors, no panic; a mark of zero length gives a dot length of 0 and an infinite speed. Pure. Limits: no farnsworth timing, no prosigns, speed assumed
constant. Tested in `events.rs` (`Tests [@ANCHOR: audio_morse]`).

## Audio path discovery: finding out by listening what works

### `conclude` [@ANCHOR: audio_discover_conclude]

The decision logic of discovery, kept pure so it is tested on made-up observations: from what each output played and what each input heard it decides which
inputs are real microphones, which outputs really make sound, which output and input `auto` should pick, and whether the operator must check by ear.
The principle: a speaker is present only if a microphone heard it; a microphone is present only if it heard a known tone or has the noise floor of a live capture.
An output heard by nothing is "no evidence of sound", not a failure.

| Argument | Type | Meaning |
|---|---|---|
| `outs` | `&[OutState]` | One per output tested: `name`, `is_default`, `is_hdmi`, `availability` (port evidence), `freq` (its test tone, Hz), `max_volume` (highest wpctl volume tried), `error`. |
| `ins` | `&[InState]` | One per input: `name`, `is_default`, `error` (could not record), `all_zero` (exact digital zeros), `muted`, `noise_rms_dbfs`. |
| `obs` | `&[Obs]` | One per output and input pair: `output`, `input`, `freq`, `volume`, `heard`, `level_dbfs`, `snr_db`, `reason`. |

Return value, `Conclusion`: `inputs` and `outputs` (JSON per device), `pairs` (loopback pairs), `alert_output` / `verify_input` (`Option<String>`) with their
reasons, and `operator_check` (`Option<String>`).

* Input `status`: `unavailable` (error), `microphone_heard_tone`, `dead` (all zeros: "muted in the mixer" when `muted`, else "muted, disconnected or no capture hardware"), `microphone_live_noise_floor` (no tone but a live noise floor).
* Output `status`: `error`, `sound_heard` (with the inputs that heard it), `cannot_verify_no_microphone` (no live input exists) and `no_evidence_of_sound`; it also carries `port_evidence` (the driver's jack state, supporting only) and `shared_frequency_with` (other outputs given the same tone, when more than 8 outputs reuse a frequency).
* `pairs` are all heard observations, strongest signal-to-noise first, each with `crosstalk_suspect` when its level is over 15 dB under the output's best pair.
* `alert_output` is the first heard output after ordering non-HDMI before HDMI, then the system default first, then the best signal-to-noise; with none heard it is `None` with the reason (no live microphone, or none heard).
* `verify_input` is the first live input ordered heard first, then default, then best signal-to-noise; `None` if every input recorded only zeros or failed.
* `operator_check` is a sentence asking the operator to listen when no microphone works, or when no output was heard; `None` otherwise.

No errors, no panic, no I/O, no locks. Tested in `discover.rs` with synthetic observations (`Tests [@ANCHOR: audio_discover_conclude]`).

### `ladder` [@ANCHOR: audio_discover_ladder]

The volume steps discovery tries on one output: it starts quiet and rises only while nothing is heard, so loud sound is never made unnecessarily.

| Argument | Type | Meaning |
|---|---|---|
| `quiet` | `f64`, wpctl scale | The configured quiet volume. |
| `limit` | `f64`, wpctl scale | The highest allowed now (`safety::limits().volume`: the cap, or the quiet volume in quiet hours). |

Return value: a short ascending `Vec<f64>`, each value rounded to three decimals and never above `limit`. The start is the lower of `quiet` and `limit`; when the room above it is under 0.03 the answer is just
`[limit]`; otherwise it is `[start, geometric mean, limit]`, the middle step left out when either gap would be under 0.03. Example: quiet 0.15, limit 0.5 gives `[0.15, 0.274, 0.5]`; quiet 0.15, limit 0.15
gives `[0.15]`. No errors, no panic (inputs are config-validated), pure.

### `fingerprint` [@ANCHOR: bench_mcp_audio_discover_fingerprint]

A stable description of the candidate devices, stored with a discovery so that a later call can see that a device came, went or changed state.

Argument: `devs: &[Device]`. Return value: a sorted `Vec<String>` with one `"<direction>|<node name>|<availability>"` entry (`-` for unknown availability) per device that is not reserved and
whose transport is `alsa` (Bluetooth and network devices that come and go do not invalidate a discovery). A jack being plugged in changes the availability text and so the fingerprint. No errors; pure; no
locks. Compared by `Stored::changes`. Serialized in the discovery file as the `fingerprint` array.

### `Stored` and `load` [@ANCHOR: bench_mcp_audio_discover_load]

`Stored` is the in-memory form of the last discovery result, read from the file `audio_discovery.json` in the capture directory by `load`. It is the
structure through which `auto` selection and staleness work.

| Field | Type | Meaning |
|---|---|---|
| `at_epoch` | `u64`, seconds since 1970 | When the run finished (0 if missing). |
| `fingerprint` | `Vec<String>` | The `fingerprint` of the devices at that time. |
| `alert_output` | `Option<String>` | `selection.alert_output`: the node name `auto` uses for audible output, `None` if none was found. |
| `verify_input` | `Option<String>` | `selection.verify_input`: the node name `auto` uses to listen. |
| `raw` | `serde_json::Value` | The whole stored JSON, returned by `bench_audio_discovery`. |

`load(dir: &Path)` returns `Option<Stored>`: `None` when the file is missing, unreadable or not JSON (a missing or damaged file means "no discovery yet", not an error; missing keys
inside a readable file take empty or zero values). Reads one file; no lock. The file format: see `run` for the JSON keys and `save` for the way it is written.
Callers get one `Stored` per tool call, borrowed through `Ctx::discovery`; it is a snapshot, not updated while the call runs.

### `Stored::changes` [@ANCHOR: audio_discover_stale]

Says what changed in the sound devices since the stored discovery: the difference between the stored fingerprint and the fingerprint of the devices now.

Arguments: `&self`, `devs: &[Device]`. Return value: `Vec<String>`, empty when the discovery is fresh; entries `new or changed: <fingerprint line>` for lines now present and not stored,
and `gone or changed: <line>` for stored lines no longer present. A changed port state shows as one of each. Pure; no errors; no locks. Used by `bench_audio_discovery` (`stale`, `changes_since`) and by
`wants_rediscovery` to decide that an alert should listen again. Tested in `discover.rs`.

### `save` [@ANCHOR: bench_mcp_audio_discover_save]

Writes the discovery result to `audio_discovery.json` in the capture directory. Private; called at the end of a full (not partial) `run`.

Arguments: `dir: &Path` (capture directory), `v: &Value` (the result). Return value: `Ok(PathBuf)` of the file, reported as `stored_in`. It makes the directory with owner-only
permissions (through `store::new_path`, whose returned file name is not used), writes the pretty-printed JSON in one `write` (not through a temporary file, so a crash mid-write could leave a
damaged file, which `load` then treats as no discovery) and sets the file mode to 0600 (a failure of that last step is ignored). Errors: the directory error or `<path>: <io error>`, or the serialization error. The
last writer wins when two runs overlap. Tested in `discover.rs`.

### `run` [@ANCHOR: audio_discover_tool]

The tool `bench_audio_discover` (tier `full`): it makes short quiet tones in the room and listens with every candidate input to find out what really works. The
caller has checked the tier. Result and method are as the tool description in `AUDIO_TOOLS.md` says; this entry is the exact behaviour.

| Argument (JSON) | Type | Meaning |
|---|---|---|
| `urgent` | bool, default false | Allows the run in quiet hours; the volume is then limited to the quiet volume. |
| `include_bluetooth` | bool, default false | Also test Bluetooth and network outputs and inputs (not only local ALSA devices). |
| `dry_run` | bool, default false | Return the plan, make no sound; allowed in quiet hours. |
| `output`, `input` | string | Limit the test to one device each (alias, node name or fragment; reserved are refused). Such a run is partial: reported, not stored, and does not change the selection. A named device is tested whatever its transport. |

Return value: with `dry_run`, `{dry_run, plan (per output: frequency_hz and volume_steps), inputs, tone_seconds, quiet_hours_now}`. Otherwise `{at_epoch, fingerprint, urgent,
quiet_hours_at_run, audible_seconds, include_bluetooth, inputs, outputs, loopback_pairs, observations, selection {alert_output, alert_output_reason, verify_input, verify_input_reason},
operator_check, method}` plus `stored_in` (the file) or, for a partial run, `partial` (the explanation). This JSON is the discovery file format; `Stored` reads `at_epoch`, `fingerprint` and `selection`.

Errors: `there is no output device to test`; `quiet hours are in effect: audible discovery is not run now. Run it outside quiet hours, or pass urgent: true ...` (checked before anything is played); the
`devices::resolve` errors for `output` / `input`; the capture-directory error; a failure to start or restore the volume of an output, `Playing` errors, or a file error, which end the loop (the
error is returned after the recorders are stopped; the volume is restored first).

Behaviour in time: every candidate input starts a `Recorder` at 48 kHz at once and each is given up to 6 s to deliver 0.8 s of samples (an input that cannot is marked unavailable, not fatal). Each output in turn gets its own tone
(1000, 1300, 1700, 2200, 2900, 3700, 4700, 6000 Hz by position, repeating after eight), a 0.4 s sine at -12 dBFS written to a temporary WAV file in the capture directory that is deleted at the end. For each volume of the
`ladder` (from the quiet volume to the limit of `safety::limits`, never above the cap, urgent not applied) a `VolumeGuard` sets the volume and unmutes, waits 250 ms, plays the tone with `Playing`, waits 0.5 s and asks `heard::hear_tone`
for each input (heard when its level at that frequency is 12 dB over the same frequency in the 0.4 s before it and over the neighbouring frequencies); the output's steps stop at the first step that any input hears. The guard
restores the original volume and mute after each output. Total sound made is `audible_seconds` (0.4 s per step). Then each input's recording is stopped and summarised (all zero, noise level, mute state through `wpctl`) and
`conclude` decides.

Side effects: audible tones in the room, `wpctl` volume and mute changes (restored), temporary files, the stored result. The volume cap (the device's own cap included) and quiet hours bound the sound; reserved devices (the VNWA codecs) are never touched, and no transmitter or radio is involved. The call blocks for the whole run, seconds per output.
Tested in `discover.rs` with scripted runners (`Tests [@ANCHOR: audio_discover_conclude]`, `audio_discover_ladder`); the audible run itself needs the machine's sound hardware.

### `report` [@ANCHOR: bench_mcp_audio_discover_report]

The tool `bench_audio_discovery` (tier `read_only`): shows the stored discovery, whether it is stale and what `auto` would pick now. It makes no sound and changes nothing.

Arguments: `ctx: &Ctx` (borrowed: the device list and the stored result), `_args` (unused). Return value: without a stored result `{"stored": false, "note": "..."}`; otherwise `{stored: true, stale (bool), changes_since
(list from `Stored::changes`), age_s (seconds since the run, 0 if the clock went backwards), result (the stored JSON), auto_selects_now {alert_output, verify_input}}`. The two `auto_selects_now` values are what `tools::pick` returns for no
named device now (the configured alert output or verify input, else the discovered one, and `null` when nothing is usable). No errors (a failure to pick is shown as `null`). Pure apart from reading the clock; no locks.
Tested in `discover.rs` (`Tests [@ANCHOR: bench_mcp_audio_discover_report]`).
