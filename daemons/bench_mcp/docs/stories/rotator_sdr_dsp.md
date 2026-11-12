# The Rotator, SDR and Signal-Processing Tools Move an Antenna Safely and Measure What a Receiver or a Scope Hears, With Nothing Here Able to Transmit

As the person (or the AI agent) running the bench, I want `bench_mcp` to point an antenna rotator without ever driving it outside the limits its owner set, to tune a
software defined radio (bladeRF, LimeSDR, RTL-SDR) and tell me what it hears, and to turn a scope waveform into a spectrum, all with arguments checked before any
hardware is touched. This story documents every anchor of four source files of `daemons/bench_mcp`: the rotator module (`src/rotator/mod.rs`: configuration, the limit
check, the move, the stop and the park), the SDR module (`src/sdr.rs`: configuration, argument checks, the SoapySDR capture, survey and coherence tools), the scope
spectrum maths (`src/spectrum.rs`, used by `bench_scope_spectrum` in `scope.rs`) and the I/Q signal processing (`src/dsp.rs`, used by the SDR tools). The configuration
examples and the tool tiers are in the README (sections "Rotators" and the SDR driver list).

Facts that apply to every entry. Nothing here can transmit radio frequency energy: the SDR tools are receive only, the spectrum and DSP code are pure functions, and the
rotator moves a motor, not a transmitter. The transmit gate, the band check and the `full` tier therefore do not apply to any function below, and no entry keys a
transmitter. The rotator is the one thing here that can do physical harm, so its tools are gated by tier (`bench_rotator_status` is `read_only`; point, stop and park are
`read_config`; the SDR tools are all `read_config`), by the owner's limits, and by a pre-flight check that runs before anything is sent. The server (`main.rs`) holds a
per-instrument lock (`bench_mcp_main_instrument_lock`) for the whole call, so two calls to one SDR never overlap; the rotator module takes the same lock itself, and the one
exception is the rotator stop, which does not wait for the lock (see `rotator_call`). Every call may block (rotator waits sleep in 500 ms steps, SDR captures stream for
seconds), so the server runs calls on a blocking thread.

## The rotator module: configuration, picking the rotator, reading its state

### `degrees` [@ANCHOR: rotator_degrees]

Reads one optional number of degrees from the instrument's JSON object and checks it against a range, so a limit or tolerance that is not a sane number is refused when the
configuration loads. It is a private helper of `parse_config`, called once for each of `min_az`, `max_az`, `min_el`, `max_el`, `tolerance_deg` and `max_wait_s`. Pure: no I/O.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&Value`, borrowed | The instrument's JSON object. |
| `key` | `&str`, borrowed | The key to read; it is also the name used in the error text. |
| `lo`, `hi` | `f64` | The inclusive range, in the unit of the key (degrees; seconds for `max_wait_s`). |

Returns `Ok(None)` when the key is absent or JSON `null` (the caller then uses the rotator's own value or its default), and `Ok(Some(x))` for a finite number inside
`lo..=hi`. Returns `Err("<key> must be a number from <lo> to <hi> degrees")` for anything else: a string, a boolean, a number outside the range. (The words "degrees" appear
even for `max_wait_s`; the text is only slightly wrong there.) No panic.

See also: `rotator_config` (the only caller); the config test in the `rotator` test module cites this anchor.

### `parse_config` [@ANCHOR: rotator_config]

Reads the `rotator` keys of one instrument entry of the bench config into a `Config` and refuses anything unusable, so a mistake is reported when the config loads, not when
a motor should move. Called through the `module_config!` macro of `config.rs` when an entry's `driver` is `rotator`; the result is kept in `Instrument.rotator` and never
changes. The rotator itself is not contacted here: the limits that the rotator reports are read later by `rotator_limits`.

`Config` (struct, `Clone, Debug, PartialEq`, with a `Default`):

| Field (config key) | Type | Default | Meaning and check |
|---|---|---|---|
| `radioctld_port` (`port`) | `u16` | none, required | The port of an already running `radioctld` with a `[[rotator]]` section. Integer 1 to 65535. |
| `radioctld_host` (`host`) | `String` | `127.0.0.1` | Host name or address of that `radioctld`. Non-empty and only ASCII letters, digits, `.`, `-` and `:`. Anything except `127.0.0.1`, `localhost` and `::1` is refused unless `allow_remote_radioctld` is `true`, because radioctld has no authentication. |
| `allow_remote_radioctld` | `bool` (read, not stored) | `false` | Owner's explicit acceptance of a non-local `radioctld`. |
| `reply_timeout_s` | `u64` | 3 | Seconds to wait for one reply. Whole number 1 to 30. |
| `min_az`, `max_az` | `Option<f64>` | `None` | Owner's azimuth limits in degrees, each -360 to 720. They narrow what the rotator reports; `None` means the rotator's own. |
| `min_el`, `max_el` | `Option<f64>` | `None` | Owner's elevation limits in degrees, each -90 to 180. |
| `tolerance_deg` | `f64` | 2.0 | How close, in degrees on each axis, the read-back position must be to the target for a move to count as arrived. 0.1 to 20. |
| `max_wait_s` | `f64` | 120.0 | The longest a call may wait for a move, seconds, 1 to 600 (`HARD_MAX_WAIT_S`). |

Argument: `c`, `&Value`, borrowed. Returns `Ok(Config)`.

Errors (`Err(String)`): the old settings `rotctld_port`, `rotctld_host` or `allow_remote_rotctld` present (they were for Hamlib's `rotctld` and are refused with the new name to use;
this check comes first); `radioctld_port` missing, not an integer or outside 1 to 65535; a host with bad characters or not a string; a non-local host without
`allow_remote_radioctld`; `reply_timeout_s` outside 1 to 30; any `degrees` failure; `min_az >= max_az` or `min_el >= max_el` when both of a pair are given.
Pure: no I/O. Example entry: `{"driver": "rotator", "tier": "read_config", "radioctld_port": 4533, "min_az": 0, "max_az": 360, "min_el": 0, "max_el": 90}`.

See also: `rotator_degrees`, `rotator_limits` (combines these with the rotator's own limits), `rotator_wait_arg` (uses `max_wait_s`), `rotator_point`, `rotator_park`.

### `instrument` [@ANCHOR: rotator_instrument]

Chooses which configured rotator a call is for. Called first by `rotator_call` for every tool. Signature `fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String>`:
`insts` is every instrument of the loaded config (borrowed), `args` the tool arguments (borrowed).

Only instruments of kind `rotator` are considered. If `args.instrument` is a string, the rotator of exactly that name is returned, else `Err("no rotator named \"<n>\"")`. A non-string
`instrument` is ignored as if absent. With no name: exactly one configured rotator is returned; none gives `Err("no rotator is configured")`; several give
`Err("name the rotator: a, b, ...")` listing the names. Pure: no I/O, no lock.

See also: `rotator_call`; the call test cites this anchor.

### `text` [@ANCHOR: rotator_error_text]

Turns an error of the link to `radioctld` into one line of text for the tool's reply, ending with the next action for a rotator (the shared radioctl client's own advice is about
rigs and would mislead). Used everywhere in this module through `.map_err(text)`. Signature `fn text(e: RigError) -> String`, `e` owned. Pure.

`RigError::Link(m)` (the socket failed, timed out, or the reply was malformed; nothing is known about the rotator): `m` is cut at its first ` Next:` (dropping the rig advice), trailing dots are
removed, and the text "Next: check that radioctld runs on the host and port in the config, the rotator's cable and power, and restart radioctld; then bench_rotator_stop if it may be moving." is
appended. `RigError::Rig { code, cmd }` (radioctld answered `RPRT <code>` with a negative code): the text is `radioctld refused <cmd>: RPRT <code> (<name>). Next: ...`, where the name comes from
`radioctl::proto::code_name` (-1 "invalid parameter", -17 "argument out of domain", -21 "limit exceeded", and so on). For codes -21, -1 and -17 the advice is "the position is outside what the rotator
accepts; bench_rotator_status shows its limits"; for every other code it is to check the rotator, cable and power and then use `bench_rotator_stop` if it may be moving. Never fails.

See also: `radioctl_error_codes` in the radioctl docs for the code names.

### `position` [@ANCHOR: rotator_position]

Reads the rotator's azimuth and elevation now. Called by `rotator_moving`, `rotator_wait`, `rotator_point` (to learn the current elevation and the position for the plan) and `rotator_park`.
Signature `fn position(c: &mut Client) -> Result<(f64, f64), String>`: `c` is a connected `radioctl::proto::Client`, borrowed mutably (one command at a time).

Sends `get_pos` through `Client::read`, which retries once over a fresh connection if the link failed (a read changes nothing, so a retry is safe), and takes the `Azimuth` and `Elevation` lines of the
reply as finite numbers. Returns `(azimuth_deg, elevation_deg)` in degrees as radioctld reports them (not clamped to the limits). Errors are `rotator_error_text`: a link failure or timeout,
`RPRT` refusal, or a reply that lacks either line or whose value is not a finite number. Blocks for at most the reply timeout, twice with the retry. Does not move anything.

See also: `rotator_moving`, `rotator_wait`.

### `limits` [@ANCHOR: rotator_limits]

Works out the limits inside which a move is allowed: what the rotator reports, narrowed by what the owner set. This is the safety envelope; `rotator_point` refuses to run without it. Public
signature `pub fn limits(c: &mut Client, cfg: &Config) -> Result<([f64; 4], String), String>`: `c` borrowed mutably, `cfg` borrowed.

Sends `dump_state` (a read, with the one retry). From the reply it takes `min_az`, `max_az`, `min_el`, `max_el` from `key=value` lines (radioctld's form), or from `Minimum Azimuth:`,
`Maximum Azimuth:`, `Minimum Elevation:`, `Maximum Elevation:` lines (a `rotctld` in the extended form); a value that does not parse counts as not reported. For each of the four, if both the rotator and
the owner give a value the narrower wins (the larger minimum, the smaller maximum); if only one gives one that value is used; if neither does, the call fails with "the rotator reports no limits and the
config sets none for it; set min_az, max_az, min_el and max_el in the config: a rotator is not moving without limits". Owner limits can therefore only narrow, never widen, the rotator's own.

Returns `([min_az, max_az, min_el, max_el], source)` in degrees, where `source` is a sentence naming where the numbers come from ("the rotator's reported limits", or "... narrowed by min_az, max_az,
min_el and max_el in the bench config" when the owner set any). Other errors are `rotator_error_text`. The result is not cached: it is read again for every move and status.

See also: `rotator_target_check`, `rotator_status` (turns a failure here into `limits: null`), `rotator_point`.

### `check_target` [@ANCHOR: rotator_target_check]

The pre-flight check of a move: refuses a target outside the limits before anything is sent. Public signature `pub fn check_target(az: f64, el: f64, lim: &[f64; 4], source: &str) -> Result<(), String>`;
`az` and `el` are the requested degrees, `lim` is `[min_az, max_az, min_el, max_el]` as returned by `rotator_limits` (borrowed), `source` the limits' description (borrowed, only used in the message).

Returns `Ok(())` when both numbers are finite and `min <= value <= max` on both axes (the limits themselves are allowed). Errors: "azimuth_deg and elevation_deg must be finite numbers" (NaN or infinity), or
"azimuth <az> / elevation <el> is outside the limits (azimuth a to b, elevation c to d degrees); source: <source>. Nothing was sent." Pure: no I/O, so it is safe to call from a dry run.

See also: `rotator_point`, `rotator_limits`.

### `moving` [@ANCHOR: rotator_moving]

Tests whether the rotator is moving, by reading the position twice 500 ms apart. Used by `rotator_status` and, to confirm the stop, by `rotator_stop`. Signature
`fn moving(c: &mut Client) -> Result<(bool, (f64, f64)), String>`, `c` borrowed mutably.

Returns `(is_moving, position)`: `is_moving` is true when azimuth or elevation changed by more than 0.05 degrees between the two reads, and `position` is the second read `(azimuth_deg, elevation_deg)`. Errors:
those of `rotator_position`. Blocks the calling thread for about 500 ms plus the two reads (`std::thread::sleep`). A rotator moving slower than 0.1 degrees per second would read as still, which is
unknown for a real controller; the stop therefore asks three times (see `rotator_stop`).

See also: `rotator_status`, `rotator_stop`, `rotator_wait` (the same 0.05 degree rule for a park).

### `status` [@ANCHOR: rotator_status]

The body of `bench_rotator_status` (tier `read_only`): reports the position, the limits, whether it is moving and the rotator's info text. Changes nothing. Signature
`fn status(c: &mut Client, cfg: &Config) -> Result<Value, String>`; both borrowed.

Returns JSON `{"azimuth_deg", "elevation_deg", "moving", "limits", "info"}`. `limits` is an object `{min_az, max_az, min_el, max_el, source}` or `null` when `rotator_limits` failed (status deliberately still
answers, so the owner can see the position of a rotator whose limits are not set; the error text is not shown). `info` is the `Info` line of `get_info`, or `null` when `get_info` fails or has none. `moving` and the position come
from `rotator_moving`, so the call takes about half a second. Errors only when the two position reads fail (`rotator_error_text`).

See also: `rotator_moving`, `rotator_limits`; the status test cites this anchor.

## The rotator module: waiting, moving, stopping, dispatch

### `wait_for` [@ANCHOR: rotator_wait]

Waits for a move to finish, by polling the position every 500 ms, and says whether it did. Used by `rotator_point` (with a target) and `rotator_park` (without). Signature
`fn wait_for(c: &mut Client, target: Option<(f64, f64)>, tol: f64, wait_s: f64) -> Result<(bool, (f64, f64), f64), String>`:

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | The radioctld connection, borrowed mutably. |
| `target` | `Option<(f64, f64)>` | `Some((az, el))` degrees to wait for; `None` to wait for the motion to stop (a park has no target the code knows). |
| `tol` | `f64` | Arrival tolerance in degrees on each axis (the config's `tolerance_deg`). |
| `wait_s` | `f64` | The most to wait, seconds; the callers have already checked it against `max_wait_s`. |

Returns `(done, position, elapsed_s)`. With a target, `done` is true when both axes are within `tol` of it. Without one, `done` is true when two reads 500 ms apart differ by less than 0.05 degrees on both axes.
`done` is false when `wait_s` ran out first. The position is the last one read; elapsed seconds are measured from the call. The first position is read before any deadline check, so a call with `wait_s` of 0 still reads once
(the callers only wait when `wait_s > 0`). Errors: a failed position read (`rotator_error_text`). Blocking, sleeps in 500 ms steps; the instrument lock is held by `rotator_call` for the whole wait, which is why the
stop does not take it. The doc comment above the function mentions a `done` argument that does not exist; it is stale.

See also: `rotator_wait_arg`, `rotator_point`, `rotator_park`.

### `wait_arg` [@ANCHOR: rotator_wait_arg]

Reads and checks the optional `wait_s` argument of `bench_rotator_point` and `bench_rotator_park`. Signature `fn wait_arg(args: &Value, cfg: &Config) -> Result<f64, String>`, both borrowed.

Returns `0.0` when `wait_s` is absent or `null` (do not wait; the position is still read back), otherwise the number of seconds. It must be a finite number from 0 to `cfg.max_wait_s` inclusive, else
`Err("wait_s must be a number from 0 to <max_wait_s> (max_wait_s in the config)")`. Pure.

See also: `rotator_config` (`max_wait_s`), `rotator_point`, `rotator_park`.

### `point` [@ANCHOR: rotator_point]

The body of `bench_rotator_point` (tier `read_config`, "ROTATOR MOVES A MOTOR"): points the antenna at an azimuth and elevation, inside the limits. Signature
`fn point(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String>`, all borrowed.

| Argument (key of `args`) | Type | Meaning |
|---|---|---|
| `azimuth_deg` | number, required | Target azimuth in degrees; must lie inside the limits. |
| `elevation_deg` | number, optional | Target elevation in degrees; omitted or `null` means the current elevation. |
| `wait_s` | number, optional | Seconds to wait for arrival, 0 to `max_wait_s`, default 0 (see `rotator_wait_arg`). |
| `dry_run` | boolean, optional | `true` returns the plan and sends nothing. Any other type is an error. |

The order is fixed and every step before the move can refuse it: read `azimuth_deg`, check `dry_run`, check `wait_s`, read the limits (`rotator_limits`; a rotator with no limits is not moved), read the current position,
resolve the elevation, run `rotator_target_check`. Only then, unless `dry_run`, the command `set_pos <az> <el>` is sent (a command, never retried, so a move is never sent twice), and, if `wait_s > 0`, `rotator_wait` waits
for arrival; the position is always read back at the end.

Returns for a real move `{"target", "position", "waited", "arrived", "waited_s", "tolerance_deg", "note"}`. `arrived` is true only when a wait was asked for and the position came within the tolerance; with no wait it
is false and `waited` is false (the move has only been commanded; the position is the read-back at that moment). When the wait ended without arrival the note says it may still be moving and names `bench_rotator_status` and
`bench_rotator_stop`. A dry run returns `{"dry_run": true, "sent": [], "would": [...], "position_now", "limits_source"}` and sends nothing. Errors: "azimuth_deg is required (degrees)", "dry_run must be true or false",
"elevation_deg must be a number", the `wait_s` error, the no-limits error, the outside-limits error (text "Nothing was sent."), and link errors (`rotator_error_text`).

Side effects: moves a motor, which can be unsafe for people, the antenna and the mast. It cannot key a transmitter. Safety measures that apply: the tool needs tier `read_config`; the owner's limits only narrow the rotator's;
the check happens before the command; `dry_run` exists; the call holds the instrument lock, so no second move starts during it; and `bench_rotator_stop` and `bench_rotator_park` are always available. A move wider than the owner's
limits is impossible; a move that the rotator's controller itself refuses is reported as an `RPRT` error.

See also: `rotator_target_check`, `rotator_limits`, `rotator_wait`, `rotator_stop`; the point tests cite this anchor.

### `stop` [@ANCHOR: rotator_stop]

The body of `bench_rotator_stop`, the rotator's safe state: stops the motion and proves it stopped. Safe at any time, intended as the first thing to use when anything goes wrong. Signature
`fn stop(c: &mut Client) -> Result<Value, String>`, `c` borrowed mutably. No arguments other than `instrument`.

Up to three times: send the command `stop`; if it was accepted, run `rotator_moving`; if the position no longer changes, return at once. If the position still changes, or `stop` failed, remember why and try again.
Returns `{"stopped": true, "moving": false, "azimuth_deg", "elevation_deg"}` on success. After three failures it returns `Err("STOP NOT CONFIRMED: <reason>. The rotator may still be moving. Do this now: remove power from the
rotator's controller, or use its own stop control. Do not use it again until the owner has looked at it.")`. Errors from `moving` itself (a failed read after an accepted stop) end the call at once with that error, without retrying.
Blocking, about half a second per attempt. The `stop` command is sent through `command` (no retry inside it), so the three attempts are the only repeats; stop is idempotent.

Locking: unlike the other three tools, `rotator_call` does not take the instrument lock for the stop, so a stop works while a point or park is waiting in another call.

See also: `rotator_call`, `rotator_moving`, `rotator_point`, `rotator_park`; the stop tests cite this anchor.

### `park` [@ANCHOR: rotator_park]

The body of `bench_rotator_park` (tier `read_config`): sends the rotator to its park position. Signature `fn park(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String>`, all borrowed.

Argument `wait_s` (number, optional, 0 to `max_wait_s`, default 0; `rotator_wait_arg`). The command `park` is sent (once, never retried). With `wait_s > 0` the call waits via `rotator_wait` with no target until the position
stops changing (0.05 degree rule) or the time runs out; otherwise it reads the position once. Returns `{"parked_command_sent": true, "motion_ended": <bool>, "position": {azimuth_deg, elevation_deg}, "waited_s"}`.
`motion_ended` is true only when a wait was requested and the motion ended; it does not say the park position was reached. Errors: the `wait_s` error and link errors (`rotator_error_text`), including an `RPRT` refusal when the
radioctld rotator model has no park. The park position itself is the rotator's own; the owner's limits are not checked against it (unknown whether radioctld applies them). It moves a motor and cannot transmit; use `bench_rotator_stop` to
end it.

See also: `rotator_wait`, `rotator_stop`.

### `call` [@ANCHOR: rotator_call]

The entry point of the module, called by the server (`main.rs`, `Module` with prefix `bench_rotator_`) for every `bench_rotator_*` tool. Public signature
`pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String>`: `insts` the configured instruments (borrowed), `name` the tool name, `args` its JSON arguments (borrowed).

Steps: find the tool in `SPECS` (`unknown tool <name>` otherwise), pick the rotator (`rotator_instrument`), refuse when the instrument's tier is below the tool's tier (`<name> needs tier ReadConfig; "<n>" is configured at ReadOnly`),
take the per-instrument lock from `crate::instrument_lock(name)` (`lock().unwrap_or_else(|e| e.into_inner())`, so a poisoned lock does not stop the safe state), connect to radioctld (`Client::connect` with the configured host, port and reply
timeout; a failure is `rotator_error_text`), and dispatch to `rotator_status`, `rotator_point`, `rotator_stop` or `rotator_park`. `bench_rotator_stop` is the only tool that skips the lock, because a stop must not wait for the move
that is running. A name in `SPECS` that the match does not handle gives "does not apply to". The returned `Value` is the tool's JSON; the server wraps it. Blocking: runs on the server's blocking thread. `tool_defs` (not
anchored) lists only the tools whose tier is at or below the tier of a configured rotator, with the tier in square brackets at the start of each description.

The module does not start `radioctld`: it must already run. Each call opens its own connection and drops it at the end.

See also: `rotator_instrument`, `rotator_config`; `bench_mcp_main_instrument_lock` in the main-module docs; the call test cites this anchor.

## The SDR module: configuration, limits and argument checks

### `check_config` [@ANCHOR: bench_mcp_sdr_check_config]

Checks the SDR settings of an instrument entry when the config loads. Public signature `pub fn check_config(c: &Config) -> Result<(), String>`; `c` borrowed. Called by `config.rs` (`sdr_config`, anchor `bench_mcp_config_sdr`) for the
drivers `bladerf`, `limesdr` and `rtlsdr`. The SDR `Config` is a struct of four fields:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `driver` | `String` | `bladerf` | The config file's driver name: `bladerf`, `limesdr` or `rtlsdr`. Set by the config loader from the entry's `driver`. |
| `serial` | `Option<String>` | `None` | Hexadecimal serial, or any prefix of it, to choose one of several boards. |
| `xb200` | `Option<String>` | `None` | bladeRF only: XB-200 transverter setting passed to SoapySDR: `50M`, `144M`, `222M`, `auto`, `auto1db`, `auto3db` or `custom`. |
| `direct_samp` | `Option<String>` | `None` | RTL-SDR only: direct sampling branch `"1"` (I) or `"2"` (Q), for 0.1 to 28.8 MHz without an upconverter. |

Errors: an unknown driver name (`unknown SDR driver "<d>"`); a serial that is empty or has a character that is not a hex digit (this also stops a serial from smuggling another SoapySDR argument such as `,driver=audio`);
`xb200` on a radio other than the bladeRF; an `xb200` value not in the list; `direct_samp` on a radio other than the RTL-SDR; a `direct_samp` value other than `"1"` or `"2"`. Returns `Ok(())` otherwise. Pure.

See also: `bench_mcp_sdr_args`, `bench_mcp_sdr_limits`; the config test cites this anchor.

### `soapy_args` [@ANCHOR: bench_mcp_sdr_args]

Builds the SoapySDR argument string that opens this radio. Public signature `pub fn soapy_args(c: &Config) -> String`; `c` borrowed. Used by `Radio::open` and by `identify` (which returns it as `soapy_args`).

The string starts with `driver=lime` (for `limesdr`), `driver=rtlsdr` (`rtlsdr`) or `driver=bladerf` (anything else), then, in this order and only when set, `,direct_samp=<x>`, `,xb200=<x>`, `,serial=<x>`. Example for a bladeRF with an
XB-200 and a serial: `driver=bladerf,xb200=auto1db,serial=ca3e`. Pure, never fails; it trusts `check_config` to have refused bad characters, so it must not be fed a `Config` that skipped that check.

See also: `bench_mcp_sdr_check_config`.

### `limits` [@ANCHOR: bench_mcp_sdr_limits]

The tuning and rate limits the tools enforce before the radio is touched. Private `fn limits(c: &Config) -> Limits`. `Limits` (private struct) has `min_hz`, `max_hz` (tuning range, Hz), `min_rate`, `max_rate` (sample rate, samples
per second), `max_gain` (dB) and `survey_rate` (the survey's default sample rate, Hz). Values by radio:

| Radio | Tuning | Sample rate | Max gain | Survey rate |
|---|---|---|---|---|
| RTL-SDR (tuner) | 24 MHz to 1766 MHz | 0.225 to 3.2 MS/s | 49.6 dB | 2.4 MS/s |
| RTL-SDR, `direct_samp` set | 0.1 MHz to 28.8 MHz | as above | 49.6 dB | 2.4 MS/s |
| LimeSDR | 30 MHz to 3.8 GHz | 0.25 to 30.72 MS/s | 70 dB | 10 MS/s |
| bladeRF, no XB-200 | 237.5 MHz to 3.8 GHz | 0.25 to 20 MS/s | 70 dB | 10 MS/s |
| bladeRF, `xb200` set | 9 kHz to 3.8 GHz | 0.25 to 20 MS/s | 70 dB | 10 MS/s |

These numbers are the tools' own rules, taken from the code; whether a particular unit can actually do each range is unknown from the code. Pure, never fails.
Called by `capture_args`, `survey_args` and `coherence`. The RTL-SDR also has a gap in its sample rates (0.3 to 0.9 MS/s) that `check_rate` (not anchored) enforces.

See also: `bench_mcp_sdr_in_range`, `bench_mcp_sdr_capture_args`.

### `number` [@ANCHOR: bench_mcp_sdr_number]

Reads an optional number argument. `fn number(args: &Value, key: &str) -> Result<Option<f64>, String>`, both borrowed. Returns `Ok(None)` when `key` is absent or `null`, `Ok(Some(x))` for a finite JSON number, and
`Err("<key> must be a number")` for anything else (string, boolean, non-finite). Pure. Used by every argument check of this module and by `bench_mcp_sdr_whole`.

### `whole` [@ANCHOR: bench_mcp_sdr_whole]

Reads an optional whole-number argument with a default and a range. `fn whole(args: &Value, key: &str, default: usize, min: usize, max: usize) -> Result<usize, String>`, `args` and `key` borrowed; `default`, `min` and `max` are counts
(samples, FFT points, repeats, channel numbers). Returns `default` when absent or `null`, the value when it has no fractional part and is within `min..=max` inclusive, otherwise
`Err("<key> <x> must be a whole number from <min> to <max>")`; a non-number is the `bench_mcp_sdr_number` error. Pure.

See also: `bench_mcp_sdr_number`; the limits tests cite this anchor.

### `in_range` [@ANCHOR: bench_mcp_sdr_in_range]

Checks that a number lies inside an inclusive range. `fn in_range(name: &str, x: f64, lo: f64, hi: f64) -> Result<f64, String>`: `name` is the argument name used in the message, `x` the value, `lo` and `hi` the bounds (same unit as `x`).
Returns `Ok(x)`, or `Err("<name> <x> is outside <lo> to <hi>")`; a NaN fails. Pure. Used for frequency, sample rate, gain and ppm. The caller always chooses the limit, so the bounds come from `bench_mcp_sdr_limits`.

### `antenna_arg` [@ANCHOR: bench_mcp_sdr_antenna_arg]

Reads the optional `antenna` argument, the receive input to use. `fn antenna_arg(args: &Value, c: &Config) -> Result<Option<String>, String>`, both borrowed. Absent or `null` gives `Ok(None)` (leave the driver's default, which is
`LNAL` on the LimeSDR). A string is upper-cased and must be one of `LNAH`, `LNAL`, `LNAW` (the `LIME_INPUTS` constants), and the radio must be a LimeSDR. Errors: "antenna must be a string"; "only the LimeSDR has a choice of receive
input; leave antenna out" (for any other driver, checked before the name); `antenna "<NAME>" must be one of LNAH, LNAL, LNAW`. Returns `Ok(Some(NAME))`. Pure. Used by `capture_args`, `survey_args` and `coherence`.

### `capture_args` [@ANCHOR: bench_mcp_sdr_capture_args]

Checks the arguments of `bench_sdr_capture` against the radio's limits before the radio is opened, and returns the checked `Capture`. `pub fn capture_args(args: &Value, c: &Config) -> Result<Capture, String>`, both borrowed.

| Argument (key) | Type | Default | Check |
|---|---|---|---|
| `freq_hz` | number | required | Centre frequency in Hz, inside `limits().min_hz..=max_hz`. |
| `sample_rate_hz` | number | 2e6 | Samples per second, inside the rate limits; for the RTL-SDR not between 0.3 and 0.9 MS/s. |
| `gain_db` | number | 30 | 0 to the radio's `max_gain`. |
| `samples` | integer | 262144 (`1 << 18`) | Complex samples per channel, 4096 to 16777216 (`MAX_SAMPLES`, `1 << 24`). |
| `channel` | integer | 0 | 0 or 1; 1 only on the LimeSDR. |
| `antenna` | string | none | `bench_mcp_sdr_antenna_arg`. |
| `ppm` | number | 0 | RTL-SDR only, -200 to 200: the dongle's crystal error in parts per million (helper `ppm_arg`, not anchored). |

`Capture` (public struct, `Debug, Clone, PartialEq`) holds `freq_hz`, `rate_hz`, `gain_db`, `samples`, `channels: Vec<usize>` (here one entry), `antenna: Option<String>` and `ppm: f64`. Errors are the texts of the helpers, plus
"freq_hz is required" and "this radio has one receive channel (0)". Pure, no hardware.

See also: `bench_mcp_sdr_limits`, `bench_mcp_sdr_survey_args`; the capture tests cite this anchor.

### `survey_args` [@ANCHOR: bench_mcp_sdr_survey_args]

Checks the arguments of `bench_sdr_survey` and returns the checked `Survey`. `pub fn survey_args(args: &Value, c: &Config) -> Result<Survey, String>`, both borrowed.

| Argument | Type | Default | Check |
|---|---|---|---|
| `start_hz` | number | required | Inside the tuning limits. |
| `stop_hz` | number | `start_hz + 1e6` | Inside the tuning limits and above `start_hz`. |
| `sample_rate_hz` | number | the radio's survey rate | As for a capture. |
| `gain_db` | number | 30 | 0 to the radio's maximum. |
| `fft_len` | integer | 4096 | 64 to 65536 and a power of two. |
| `samples_per_step` | integer | 262144 | `fft_len * 4` to 16777216. |
| `antenna`, `ppm` | | | As for a capture. |

`Survey` (public struct) holds the same values (`start_hz`, `stop_hz`, `rate_hz`, `gain_db`, `fft_len`, `samples_per_step`, `antenna`, `ppm`). Its method `steps()` is `ceil((stop - start) / (rate * USABLE))` and at least 1, where `USABLE` is 0.7: only 70 % of
each capture's bandwidth is kept because the radio's filters roll off the edges. The call is refused when `steps()` exceeds 400 (`MAX_STEPS`), with a message telling the caller to raise the sample rate or narrow the span. Pure; the hardware is not touched.

See also: `bench_mcp_sdr_capture_args`; the survey tests cite this anchor.

## The SDR module: the radio, the raw file and the coherence tool

### `continuous` [@ANCHOR: bench_mcp_sdr_continuous]

Decides whether a capture arrived at the sample rate, so no samples were lost. `pub fn continuous(capture_s: f64, real_s: f64) -> bool`: `capture_s` is the wall time the streaming took (seconds), `real_s` how long the signal lasts
(`(samples + 8192) / rate`, seconds). Returns `capture_s <= real_s * 1.04 + 0.1`: 4 % and 0.1 s of slack. A slow stream means samples were dropped. The reported overflow count is not trusted alone because a bladeRF with an FPGA newer than
libbladeRF knows reports an overrun about every 504 samples even when none are lost. `capture` subtracts 0.3 s for the RTL-SDR, which spends that long starting its USB transfers. The tune time (over a second on the LimeSDR) is not part of the streaming time.
Pure.

See also: `bench_sdr_capture` result field `continuous`; the continuous test cites this anchor.

### `write_iq` [@ANCHOR: bench_mcp_sdr_write_iq]

Saves captured samples as a raw file. `fn write_iq(path: &Path, samples: &[Iq]) -> Result<(), String>`: `path` the file to create (overwritten if it exists), `samples` borrowed. Format `.cf32`: for each sample the I value then the Q value, each a 32-bit
little-endian IEEE float, no header, so a file is `8 * samples` bytes; full scale is 1.0. The file is written in one `std::fs::write`; an error is `"<path>: <io error>"` (no partial-file cleanup). Called by `capture` with the name
`sdr_<driver>_<freq_hz>_<unix seconds>.cf32` in the server's output directory (`out_dir()`), which is the only place a file is created; `survey` writes a CSV instead. Blocking disk I/O; no secrets.

See also: the I/Q file test cites this anchor; `iq_format` in the capture reply.

### `coherence` [@ANCHOR: bench_mcp_sdr_coherence_args]

The body of `bench_sdr_coherence`, whose arguments are checked here before the radio is opened (hence the anchor name). `pub fn coherence(c: &Config, args: &Value) -> Result<Value, String>`: both borrowed. It measures the phase and gain between the two
receive channels of a radio that has two (the LimeSDR), after a fresh tune on every repeat.

| Argument | Type | Default | Check |
|---|---|---|---|
| `freq_hz` | number | required | Inside the tuning limits. |
| `sample_rate_hz` | number | 2e6 | Rate limits as for a capture. |
| `gain_db` | number | 30 | 0 to maximum. |
| `repeats` | integer | 4 | 1 to 10. |
| `antenna` | string | none | LimeSDR input, applied to both channels. |

Flow: check arguments, open the radio (`Radio::open`, which loads `libSoapySDR` with `dlopen` and calls `SoapySDRDevice_makeStrArgs`), refuse when the radio has fewer than two receive channels
(`the <driver> has <n> receive channel; coherence needs two`), then for each repeat capture 262144 samples on channels 0 and 1 together (`ppm` is not used here) and run `dsp::phase_between` with FFT length 4096 and 3 DC bins skipped. Returns
`{"freq_hz", "runs": [{tone_offset_hz, phase_deg, phase_std_within_capture_deg, gain_db, coherence, blocks}], "mean_phase_deg", "phase_std_across_retunes_deg", "note"}`. The mean and the across-retune spread are the circular mean and circular
standard deviation of the per-repeat phases, in degrees. The note says a splitter feeding one signal to both inputs is needed; without it the numbers mean nothing, and `coherence` near 1 is the check.

Errors: the argument texts above, "SoapySDR is not installed (libSoapySDR.so not found; ...)" when the library cannot be loaded, "cannot open the <driver> (<args>): <SoapySDR error>", SoapySDR setup and stream errors, "the radio delivered no samples (5 read timeouts
...)" or "capture took longer than 120 s" when the stream stalls, and `phase_between` errors. Receive only; it holds the instrument lock for the whole call (many seconds); the library is loaded once for the life of the process.

See also: `bench_mcp_sdr_phase_math`, `bench_mcp_sdr_limits`; the coherence test cites this anchor.

## The scope spectrum maths (`spectrum.rs`)

Used only by the scope tool `bench_scope_spectrum` (`scope.rs`, anchor `bench_mcp_scope_spectrum`), which fetches a waveform, converts it to volts and calls `spectrum` and `peaks` here. The scope's 8-bit samples limit the dynamic range to about 48 dB per bin before the window's
processing gain, so this is a check of the strong lines (the fundamental and its first harmonics), not a spectrum analyzer. Levels are in dBV rms at the scope input, and dBm into 50 ohms (dBV + 13.0103). Not to be confused with `dsp.rs`, which has its own `Spectrum` and `Peak` types for I/Q data.

### `Window::parse` [@ANCHOR: bench_mcp_spectrum_window_parse]

Turns the `window` argument of `bench_scope_spectrum` into a `Window` (enum with four variants: `Rect`, `Hanning`, `Hamming`, `Blackman`; `Clone, Copy, Debug, PartialEq`). `pub fn parse(s: &str) -> Result<Window, String>`, `s` borrowed. The comparison is
case-insensitive: `rect`, `rectangular` or `none` give `Rect`; `hann` or `hanning` give `Hanning`; `hamming` and `blackman` give themselves. Anything else is `Err("unknown window \"<s>\" (rect, hanning, hamming, blackman)")`. The scope tool's default is `"hanning"`. Pure.

### `Window::weight` [@ANCHOR: bench_mcp_spectrum_window_weight]

The weight of sample `k` of an `n`-point window. `fn weight(self, k: usize, n: usize) -> f64` (private; `k` is the sample index 0 to n-1, `n` the number of points). The form is periodic (angle `x = 2 pi k / n`, not `n - 1`), the right one for spectral analysis. `Rect` is 1;
`Hanning` is `0.5 - 0.5 cos x`; `Hamming` is `0.54 - 0.46 cos x`; `Blackman` is `0.42 - 0.5 cos x + 0.08 cos 2x`. Returns a value from about 0 to 1 (Hamming never reaches 0: 0.08 at the ends). Pure.

### `fft` [@ANCHOR: bench_mcp_spectrum_fft]

An in-place iterative radix-2 FFT (decimation in time: a bit-reversal reorder, then butterflies, with twiddle factors built by recurrence). `fn fft(re: &mut [f64], im: &mut [f64])`, private: the real and imaginary parts of the same signal, both borrowed mutably, replaced by the transform (the unnormalised
forward transform, `e^(-j...)`). `re.len()` must be a power of two and `im.len()` equal to it; this is not checked here and a wrong length gives wrong results or an index panic. The only caller, `spectrum`, checks the length first. The twiddle factors are built by a running product, so rounding error grows with the length; its size is unknown here. Pure. The same algorithm is repeated in `dsp.rs` (`bench_mcp_dsp_fft`).

### `spectrum` [@ANCHOR: bench_mcp_spectrum]

Computes the spectrum of a real waveform. `pub fn spectrum(x: &[f32], fs: f64, w: Window) -> Result<Spectrum, String>`: `x` the samples in volts (borrowed), `fs` the sample rate in hertz, `w` the window.

Result type `Spectrum`: `bin_hz` (`fs / n`, frequency step in Hz), `vrms` (`Vec<f64>`, the rms volts of each bin from DC through Nyquist, `n/2 + 1` entries) and `enbw_bins` (the window's equivalent noise bandwidth in bins, `n * sum(w^2) / sum(w)^2`: 1.0 for rect, about 1.5 for Hanning).
Steps: the mean is subtracted (so DC does not leak), the window is applied, the FFT is taken, and each bin's magnitude is scaled by `2 / sum(w) / sqrt(2)` so a sine of amplitude A reads A/sqrt(2) volts rms at its bin (the coherent gain correction). A sine falling between bins is spread over the neighbours and
reads low by the window's scalloping loss (see `peaks`).

Error: "<n> samples: the FFT needs a power of two, at least 16" when `n < 16` or not a power of two (the scope tool checks `points` first, so this is a second guard). Allocation: a few vectors of `n` doubles. Blocking CPU work, `O(n log n)`; the time at the largest size (1,048,576 points) is unknown. No hardware, no I/O.

See also: `bench_mcp_spectrum_peaks`, `bench_mcp_spectrum_fft`, `bench_mcp_spectrum_window_weight`; the spectrum test cites this anchor.

### `peaks` [@ANCHOR: bench_mcp_spectrum_peaks]

Lists the strongest spectral lines. `pub fn peaks(sp: &Spectrum, count: usize, min_below_max_db: f64) -> Vec<Peak>`: `sp` borrowed, `count` the most to return (the scope tool clamps its `peaks` argument to 1 to 50, default 10), `min_below_max_db` how far below the strongest line a peak may be and still be listed (the scope tool's `range_db`, default 60).

A peak is a bin from index 2 up (DC and the first bin are skipped) whose value is greater than its left neighbour and at least its right neighbour. Its frequency and level are refined by a parabola through the logs of the three bins: the offset `d = 0.5 (a - c) / (a - 2b + c)` (0 when the curve is flat) and the level `exp(b - 0.25 (a - c) d)`.
`Peak` (struct, `Clone, Debug, PartialEq`) has `freq_hz` (`(k + d) * bin_hz`), `vrms` (volts rms), `dbv` (`20 log10 vrms`) and `dbm_50` (`dbv + 13.0103`, the level into 50 ohms; 13.0103 is `10 log10(1000 / 50)`). Peaks are returned strongest first, cut to `count`. The level is exact for a tone centred on a bin and up to about
1.4 dB low for Hanning when the tone is between two bins (0 dB for rect). An empty list is returned when nothing qualifies. The function does not fail, but `sp.vrms` must have at least 4 entries, which `spectrum` guarantees. The doc comment of the function has a garbled sentence ("at least `min_below_max_db` above... that is"); its meaning is the one given here.

See also: `bench_mcp_spectrum`; the peaks test cites this anchor.

## The I/Q signal processing (`dsp.rs`)

Pure functions on complex baseband samples for the SDR tools; no hardware. A sample is `Iq = (f32, f32)`, I then Q, full scale 1.0. `Spectrum` (struct, `Debug, Clone`) is an averaged power spectrum: `bin_hz` (Hz per bin) and `power_db` (`Vec<f64>`, the power of each bin in dB relative to a full-scale complex tone, 1.0 amplitude = 0 dBFS),
with **bin 0 at -rate/2 and the DC bin at index `len / 2`**; its method `offset_hz(i)` returns `(i - len/2) * bin_hz`, the offset of a bin from the centre frequency. `Peak` (struct, `Debug, Clone, PartialEq`) has `offset_hz` and `power_db`. The levels are relative to full scale and depend on the radio's gain setting, not calibrated power.

### `fft` [@ANCHOR: bench_mcp_dsp_fft]

The in-place radix-2 FFT of this file: `fn fft(re: &mut [f64], im: &mut [f64])` (private): the same iterative bit-reversal and butterfly algorithm as `bench_mcp_spectrum_fft`, duplicated here for the complex input, forward transform, unnormalised. `re` and `im` are borrowed mutably and replaced by the result; the length must be a power of two and equal for both
(the caller `block_fft` takes it from the window, checked earlier by `bench_mcp_dsp_check_len`). No error return; a bad length would index out of range or give wrong numbers. Pure.

### `check_len` [@ANCHOR: bench_mcp_dsp_check_len]

Checks an FFT length. `fn check_len(fft_len: usize) -> Result<(), String>` (private). `Ok(())` when `fft_len` is a power of two of at least 16, otherwise `Err("fft_len <n> must be a power of two of at least 16")`. Called by `welch` and `phase_between`, so those never reach the FFT with a bad length. Pure. (The SDR argument check for a survey's `fft_len` is stricter: 64 to 65536.)

### `block_fft` [@ANCHOR: bench_mcp_dsp_block_fft]

The windowed FFT of one block of samples, with DC moved to the middle. `fn block_fft(samples: &[Iq], at: usize, w: &[f64]) -> (Vec<f64>, Vec<f64>)` (private): `samples` borrowed, `at` the index of the first sample of the block, `w` the window (its length is the block length). The caller must ensure `at + w.len() <= samples.len()`; there is no check, an overrun panics with an index error. Returns the real and
imaginary parts (`re`, `im`) of the transform of `samples[at..at + n] * w`, rotated left by `n / 2` so that index `n / 2` is zero frequency, bin 0 is -rate/2 and bin `n - 1` is the highest positive frequency. Allocates two vectors of `n` doubles. Pure.

### `welch` [@ANCHOR: bench_mcp_sdr_spectrum_math]

Welch's averaged power spectrum of a capture, the maths behind the capture and survey tools. `pub fn welch(samples: &[Iq], fft_len: usize, rate_hz: f64) -> Result<Spectrum, String>`: `samples` borrowed, `fft_len` the block size (power of two, 16 or more), `rate_hz` the sample rate in Hz (finite, above 0).

Method: Hann-windowed blocks of `fft_len` samples, each starting `fft_len / 2` after the previous (50 % overlap), their power spectra summed and scaled by `1 / (blocks * sum(w)^2)`, so a complex tone of amplitude A reads `20 log10(A)` dBFS at its bin; a tone between bins reads up to 1.4 dB low. Each bin is `10 log10` of the scaled power, floored at 1e-30 (-300 dB)
so silence does not give minus infinity. Returns `Spectrum` with `bin_hz = rate_hz / fft_len`. Errors: the `check_len` error; "<n> samples are fewer than one FFT block of <fft_len>"; "sample rate must be a positive number" (zero, negative, NaN or infinite). Cost `O(blocks * fft_len log fft_len)`; with a capture of 262144 samples at 4096 points that is about 127 blocks. Pure.

See also: `bench_mcp_dsp_noise_floor`, `bench_mcp_dsp_peaks`, `bench_mcp_sdr_phase_math`.

### `noise_floor_db` [@ANCHOR: bench_mcp_dsp_noise_floor]

Estimates the noise floor of a spectrum as its median power, which strong peaks do not move. `pub fn noise_floor_db(s: &Spectrum) -> f64`, `s` borrowed. Sorts a copy of `power_db` (`total_cmp`, so NaN sorts last without panicking) and returns the element at index `len / 2`, in dBFS. Panics if `power_db` is empty (indexing an empty vector); `welch` never returns an empty spectrum.
Used by `peaks` and by the capture and survey tools (`noise_floor_dbfs`, `median_floor_dbfs` in their replies). Allocates a copy; pure.

### `peaks` [@ANCHOR: bench_mcp_dsp_peaks]

Finds the strongest spectral peaks of an I/Q spectrum. `pub fn peaks(s: &Spectrum, count: usize, min_above_floor_db: f64, min_sep_bins: usize, skip_dc_bins: usize) -> Vec<Peak>`: `s` borrowed; `count` the most to return; `min_above_floor_db` how far above the median floor (`noise_floor_db`) a peak must be, in dB; `min_sep_bins` the least distance in bins between two returned peaks; `skip_dc_bins` how many bins on each side of the middle bin to ignore, because radios put a spike at the centre frequency
(the capture tool calls it with count 8, 15 dB above the floor, separation 8 bins and 3 DC bins skipped; the survey uses count 10, 10 dB, separation 4 and skip 0, because it replaces the DC spike itself).

A candidate is a bin (not the first or last) that is at least the left neighbour and greater than the right neighbour, at least `min_above_floor_db` over the floor and farther than `skip_dc_bins` from index `len / 2`. Candidates are sorted by power, strongest first, and taken greedily, skipping any within `min_sep_bins` of one already taken, until `count` is reached. Returns each as `Peak { offset_hz, power_db }`; the list may be empty. No interpolation: the frequency is the bin's. Never fails; a spectrum of fewer than 3 bins returns an empty list. Pure.

See also: `bench_mcp_dsp_noise_floor`; the peaks tests cite this anchor.

### `phase_between` [@ANCHOR: bench_mcp_sdr_phase_math]

The phase and amplitude relation of two simultaneous captures at their strongest common tone, the maths behind `bench_sdr_coherence`. `pub fn phase_between(a: &[Iq], b: &[Iq], fft_len: usize, rate_hz: f64, skip_dc_bins: usize) -> Result<PhaseReport, String>`: the channels `a` and `b` (borrowed; the shorter length is used, so they should be the same capture), `fft_len` the block size (power of two, 16 or more), `rate_hz` the sample rate, `skip_dc_bins` the DC bins to ignore.

The tone is the strongest bin of the `welch` spectrum of `a` (skipping the first, the last and the DC neighbourhood). Then, over consecutive Hann-windowed blocks without overlap, the cross spectrum A x conj(B) is taken at that bin; each block's angle is one phase sample. `PhaseReport` (struct, `Debug, Clone`):

| Field | Unit | Meaning |
|---|---|---|
| `tone_offset_hz` | Hz | Offset of the analysed tone from the centre frequency. |
| `phase_deg` | degrees, -180 to 180 | Circular mean of (phase of A minus phase of B) over the blocks. |
| `phase_std_deg` | degrees | Circular standard deviation, `sqrt(-2 ln R)` with R the mean resultant length: how stable the offset is. |
| `gain_db` | dB | `10 log10 (power of A / power of B)` at the tone: positive when A is stronger. |
| `coherence` | 0 to 1 | Magnitude-squared coherence at the tone; 1 means perfectly related. |
| `blocks` | count | Number of blocks averaged. |

Errors: the `check_len` error; "<n> samples are too few for a phase measurement (need <2 * fft_len>)"; the `welch` errors; "no bin to analyse". Pure; the caller (`coherence`) needs a signal on both inputs: with unrelated signals the coherence is near 0 and the phase means nothing. With one block, the standard deviation is 0 and coherence is 1 by construction, which is why at least two blocks are required.

See also: `bench_mcp_sdr_coherence_args`, `bench_mcp_sdr_spectrum_math`; the phase test cites this anchor.

### `stats` [@ANCHOR: bench_mcp_dsp_stats]

Basic statistics of a capture. `pub fn stats(samples: &[Iq]) -> (f64, f64, (f64, f64), usize)`: `samples` borrowed (an empty slice is treated as length 1 and gives zeros). Returns a tuple `(rms, peak, (dc_i, dc_q), clipped)`:
`rms` is `sqrt(mean(i^2 + q^2))`, the complex RMS amplitude, full scale 1.0 (the capture tool reports it as `rms_dbfs`, `10 log10(rms^2)`); `peak` is the largest absolute value of any I or Q component; `dc_i` and `dc_q` are the means of I and Q (the DC offset); `clipped` is the number of samples whose I or Q has magnitude 0.999 or more (counted once per sample), which signals the radio's front end was overdriven.
Never fails. One pass, `O(n)`. Pure.

See also: the stats test cites this anchor; the capture reply fields `rms_dbfs`, `peak_amplitude`, `dc_offset`, `clipped_samples`.
