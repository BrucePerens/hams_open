# The Glasgow Driver's Core, Its Process Runner and Its Installer Keep the Ports Safe and the Install Pinned

As the person (or the AI agent) at the bench with a Glasgow Interface Explorer, I want the `bench_glasgow_*` tools to power a port only when I have said so, to read the
device back before they trust it, to switch everything off when anything disagrees or a timer runs out, and to run a `glasgow` program that is exactly the one the server
was tested with, so that a probe on a target board never sees a voltage nobody approved. This story covers the core of the driver and its two support files in
`daemons/bench_mcp`: `src/glasgow/mod.rs` (the config, the per-instrument session record, the device lock, the safe state, the status and list tools, the port-set tool,
the watchdog and the dispatcher), `src/glasgow/runner.rs` (how the pinned `glasgow` command is started and ended), `glasgow/install_glasgow.py` (builds the pinned,
isolated Python environment) and `glasgow/pin_requirements.py` (regenerates the hashed requirements list). The applet tools (I2C, SPI, UART, JTAG, SWD, logic, GPIO,
pattern) are in `src/glasgow/ops.rs`, the output parsers are in `src/glasgow/parse.rs`, and the logic-analyzer tools built on top are in `docs/stories/gla.md`.

The design in one paragraph: every tool call arrives through `call_with`, which checks the tool name and the instrument's tier and then runs one function. A function that
could drive a pin first needs a port set by `bench_glasgow_port_set`, which records the voltage and a deadline (`max_on_s`) in a process-wide session table and starts a
watchdog thread. Every command goes through a `Runner`, so the tests put a simulated device behind the same code; the real runner starts the pinned `glasgow` as a child and
ends it by SIGINT, then SIGTERM, never SIGKILL. A failure that might leave something powered goes through `fail_closed`, which runs `glasgow safe`, reads every port back and
proves 0 V, or tells the person to unplug the USB cable.

Nothing here transmits on the air. The "transmit" risk of this driver is a voltage on a target board: the safety rules that stand in for the lock, band check and TX gate
are the tier (`bench_glasgow_port_set` is `full`, `bench_glasgow_safe_state` and `bench_glasgow_prepare` are `read_config`, the rest `read_only`), the per-instrument
mutex taken by the server's dispatcher (`bench_mcp_main_instrument_lock`, `src/main.rs`) for the whole call, the `flock` of the install's lock file, the `confirm` flag,
`max_volts`, `max_trip_a` and `max_on_s` of the config, and the readback after every change. There is no band check and no radio TX gate because no radio is involved.
Real-hardware status (which tools were measured on a revision C3 and which are fixtures only) is on the manual pages `manual/instruments/glasgow`.

Conventions in the entries: voltages are volts (`f64`), currents amperes, durations seconds unless a name says otherwise. A "port" is `A` or `B` (eight pins each, `A0` to
`A7`, `B0` to `B7`). A "recorded" voltage is what this server's session table holds, not what the device reports. "Parked" means the `control-gpio` applet is loaded with
no pin driven (all pins are inputs; with the rail on the pull resistors are released too).

## The configuration and the install location

### `glasgow_config` [@ANCHOR: glasgow_config]

Reads the driver-specific keys of an instrument entry whose `driver` is `glasgow` and returns a `Config`. It is called once for each such entry when the config file is
read, so a bad value stops the server at start-up with a message that names the key, never in the middle of a call. It also validates two keys that are not stored in
`Config` because the generic instrument entry owns them (`serial` and `max_volts`).

```rust
pub fn parse_config(c: &Value) -> Result<Config, String>
```

| Key (JSON) | Type | Meaning, range, default |
|---|---|---|
| `venv` | string, absolute path | The install directory made by `install_glasgow.py` (it holds `venv/` and `cache/`). A relative path is refused. Default: absent, which means `~/.local/share/bench_mcp/glasgow-70cb38c9` (see `install_dir`). |
| `max_capture_s` | number | The longest capture or listen of one call, seconds. 1 to 120, default 30. |
| `max_capture_mb` | number | The largest capture file, megabytes (truncated to an integer); the capture is stopped when it grows past this. 1 to 512, default 64. |
| `op_timeout_s` | number | The longest an applet call may take, seconds (truncated to an integer), including a first-use gateware build. 10 to 300, default 120. |
| `build_timeout_s` | number | The longest `bench_glasgow_prepare` may take, seconds. 30 to 1800, default 600. |
| `max_trip_a` | number | The highest supply current trip a tool may ask for, amperes. 0.01 to 0.325 (the hardware's range ends at 0.325), default 0.1. |
| `full_speed_fallback` | boolean | Default false. When true and the device links at 12 Mb/s, the data-streaming applets run anyway through the launcher `glasgow_fs.py` (64-byte packets, about 1 MiB/s) instead of being refused; every result then says `full_speed_fallback: true`. |
| `serial` | string | Checked only: must pass `serial_ok` (the form `C3-20240904T183337Z`). Not stored here; the instrument entry keeps it. |
| `max_volts` | number | Checked only: a value above 5.5 is refused (the highest any revision allows). Not stored here; the instrument entry keeps it (default 5.5). |

Returns `Ok(Config)` with every unset key at its default. Errors, all `Err(String)`: a key of the wrong type (`venv must be a path`, `<key> must be a number`,
`full_speed_fallback must be true or false`, `serial must be a string`); a relative `venv`; a number that is not finite or outside its range (`<key> <x> must be <lo> to
<hi>`); a malformed `serial`; `max_volts` above 5.5. A `max_volts` that is not a number is ignored here (the generic instrument parser handles it). Unknown keys are ignored.
No side effects: it reads only its argument. It is pure and safe to call from any thread. See also: `config_has_defaults_and_refuses_bad_values` in `src/glasgow/tests.rs`,
`Config` (the struct this fills; every field is in the table above), `glasgow_serial_ok` (`src/glasgow/parse.rs`).

Example entry: `"glasgow": {"driver": "glasgow", "tier": "full", "serial": "SN-0000", "max_volts": 3.6, "max_on_s": 300, "max_trip_a": 0.1, "max_capture_s": 30}`.

### `glasgow_install_dir` [@ANCHOR: glasgow_install_dir]

Returns the directory in which this instrument's pinned Glasgow install lives: the config's `venv` when it is set, otherwise `$HOME/.local/share/bench_mcp/glasgow-70cb38c9`
(the last part is the first eight characters of the pinned commit, `PINNED_COMMIT`). It is called by `real_ctx` (to build the call context) and by `list` (to report the
install). Nothing is created or checked here.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config`, borrowed | The parsed `glasgow` settings; only `venv` is read. |

Returns a new `PathBuf`. If `HOME` is not set the default path starts at `/nonexistent`, so a later `cannot start ...` error names a path that plainly does not exist
instead of a surprising directory. No errors, no panics, no locks, no files touched. See also: `install_dir_is_configured_or_the_pinned_default` in
`src/glasgow/tests.rs`, `install_paths` (the program and cache inside it), `install_main` (the script that creates it).

### `glasgow_real_ctx` [@ANCHOR: glasgow_real_ctx]

Builds the `Ctx` (the "everything a call touches outside the process" bundle) that production uses: the real runner, the real sysfs, the real capture directory and the lock
file. `call` builds one for every tool call; the tests build their own `Ctx` with a simulated device and call `call_with` instead.

| Argument | Type | Meaning |
|---|---|---|
| `inst` | `&Instrument`, borrowed | The configured instrument; its `glasgow` settings give the install directory. |

Returns a `Ctx` with: `runner` a `RealRunner` for `<install>/venv/bin/glasgow` with the cache `<install>/cache`; `sysfs` `/sys/bus/usb/devices`; `out_dir` the capture
directory from `crate::out_dir()` (the `BENCH_MCP_OUT` environment variable or its default, see `bench_mcp_main_out_dir`); `lock_path` `<install>/glasgow.lock`. It creates
nothing on disk and starts no process; errors cannot occur here (a missing install shows up later when the runner tries to start the program). Not blocking, no locks.
See also: `the_serial_is_passed_on_and_the_real_context_lives_in_the_install` in `src/glasgow/tests.rs`, `install_dir`, `install_paths`, `RealRunner`.

## The session record, the device lock and the command wrapper

### `glasgow_sessions` [@ANCHOR: glasgow_sessions]

The process-wide table of what this server has powered. It is the server's own memory of "which ports a person set, to what voltage, and until when"; every pin tool takes
the voltage from it instead of from its own arguments, `status` shows it, the watchdog reads its deadlines, and `shutdown` walks it. It lives in `src/glasgow/mod.rs`.

```rust
fn sessions() -> &'static Mutex<HashMap<String, Session>>   // key: the instrument's name
struct Session { ports: BTreeMap<char, PortRec>, runner: Arc<dyn Runner>, serial: Option<String>, timeout: Duration }
struct PortRec { volts: f64, deadline: Instant }
```

| Field | Type | Meaning |
|---|---|---|
| map key | `String` | The instrument name from the config; one entry per instrument that has at least one powered port. |
| `Session.ports` | `BTreeMap<char, PortRec>` | One record per powered port, key `'A'` or `'B'`. |
| `PortRec.volts` | `f64`, volts | The voltage a person set with `bench_glasgow_port_set`; compared with the device readback by the applet tools. Never 0 (setting 0 removes the record). |
| `PortRec.deadline` | `Instant` | When the port must be off: the moment of the set plus the instrument's `max_on_s`. The watchdog switches the ports off at or after it. |
| `Session.runner` | `Arc<dyn Runner>` | The way to reach the device, kept so `shutdown` can run `glasgow safe` without a call context. Refreshed on every set. |
| `Session.serial` | `Option<String>` | The instrument's configured serial, for the `--serial` option of the shutdown command. |
| `Session.timeout` | `Duration` | Set to the `max_on_s` window when the session is created. Unknown purpose: nothing reads it (`shutdown` discards it), so it looks unused. |

Invariants: an instrument has an entry only while it has at least one record; `port_set` removes the entry when its last record goes; `safe` removes the entry whatever the
outcome. Concurrency: one `Mutex` guards the whole table, taken only for short, non-blocking closures through `with_session`; a poisoned mutex is recovered, not propagated,
because the table is still consistent after a panic elsewhere. It is not persisted: after a restart the table is empty, and a port left powered by the old process is
unknown to the new one (the new server's `status` shows `outputs_enabled` from the device, and `bench_glasgow_safe_state` switches it off). Created by `port_set`, read by
`status`, `shutdown` and the watchdog, cleared by `safe` and `shutdown`. See also: `a_port_is_set_with_the_limit_first_and_read_back` in `src/glasgow/tests.rs`.

### `glasgow_base_args` [@ANCHOR: glasgow_base_args]

Gives the leading command-line words that choose the device: `["--serial", "<serial>"]` when the instrument's entry names a serial, else an empty list (the `glasgow`
program then uses the only device attached). Every command of the driver starts with these words, so a bench with two Glasgow units drives the right one.

| Argument | Type | Meaning |
|---|---|---|
| `inst` | `&Instrument`, borrowed | The configured instrument; only `serial` is read. |

Returns a new `Vec<String>`; the caller appends the subcommand and its arguments. No errors, no side effects, no locks. Note that the serial was validated when the config was
read (`parse_config`), so it cannot carry an option-like value. See also: `the_serial_is_passed_on_and_the_real_context_lives_in_the_install` in `src/glasgow/tests.rs`,
`read_ports`, `safe`.

### `glasgow_device_lock` [@ANCHOR: glasgow_device_lock]

Takes an exclusive `flock` on the install's lock file and returns a guard that holds it. The in-process mutex of the server (taken by the dispatcher per instrument) keeps two
calls of this server apart; this lock additionally keeps out a second `bench_mcp` process or a stray `glasgow` command of the same user, so only one session talks to the
device at a time. It is taken by `list`, `status`, `safe_state`, `port_set`, the watchdog and the applet tools in `ops.rs`.

```rust
fn device_lock(path: &Path) -> Result<DeviceLock, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&Path`, borrowed | The lock file, `<install>/glasgow.lock`. Its directory is created if missing; the file is created if missing and never truncated. |

Returns a `DeviceLock` that owns the open file; the lock is released when the value is dropped (the file closes). Errors: the file cannot be created or opened (the message is
the path and the OS error); the lock is still held by someone else after about two seconds (`another process holds <path> ... One session at a time: wait for it or end it,
then retry`). It retries every 50 ms for up to 2 s because a process that is just letting go, or a forked child that has not yet executed, is not a second session. It
blocks the calling thread for that time at most. It uses `flock(2)` with `LOCK_EX | LOCK_NB` (an `unsafe` call on a descriptor the function owns). Lock order: the dispatcher's
per-instrument mutex first, then this file lock, then the session-table mutex only inside short closures; the watchdog follows the same order. Not re-entrant: a function that
holds the guard must not call another function that takes it (the applet functions that need the device while `port_set` or `safe_state` holds it use the lower-level `run`
instead). No transmitter or band rule applies; it protects the USB device. See also: `list_reports_the_device_the_link_and_the_install` in `src/glasgow/tests.rs`,
`glasgow_install_main` (which creates the install the lock file lives in).

### `glasgow_run` [@ANCHOR: glasgow_run]

The one function through which every `glasgow` command of the driver runs. It hands a `Req` to the context's runner and turns the two abnormal outcomes into errors the caller
can show a person: a child that would not end, and a child that was ended because the time ran out.

```rust
fn run(ctx: &Ctx, req: Req, what: &str) -> Result<Out, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | The runner to use (real or simulated). |
| `req` | `Req`, owned | The arguments after the program name, the time limit, and the optional `stop_after`, `watch`, `nice` and `full_speed` settings (see `runner.rs`). |
| `what` | `&str`, borrowed | A short phrase for the error message, for example `glasgow voltage`. |

Returns `Ok(Out)` for any child that ended by itself or that the runner ended on purpose (`stopped`, so a capture cut by its size limit is still a success); the exit code is in
`Out.code` and the caller must check it. Errors: the runner could not start the program (its own message, which says to run the installer or fix `venv`); the child was `stuck`
(still running after SIGINT and SIGTERM; the message names its pid and says to end it by hand and then call `bench_glasgow_safe_state`); the child `timed_out` (the message says
the command did not finish within the limit and was interrupted, shows the last three lines of its stderr, and points to `bench_glasgow_list` then `bench_glasgow_safe_state`).
It blocks the calling thread for as long as the command runs (up to the request's timeout plus up to about 20 s for the SIGINT, SIGTERM ladder). It takes no lock of its own;
callers hold the device lock. A command that timed out may have left a port powered or a pin driven: the callers that can have done so run `fail_closed` or `park` afterwards.
See also: `status_reads_the_ports_and_fails_loudly_when_it_cannot` in `src/glasgow/tests.rs`, `RealRunner` (`glasgow_real_runner`).

### `glasgow_read_ports` [@ANCHOR: glasgow_read_ports]

Runs `glasgow voltage` (with the instrument's `--serial`) and parses its table into one `PortRow` per port, together with the warnings and errors of the program's log. It is the
"readback" that every claim about the device rests on: `status`, `safe` (to prove 0 V) and `port_set` (before and after the change) all use it.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner and paths. |
| `inst` | `&Instrument`, borrowed | The instrument (for `--serial`). |

Returns `Ok((rows, problems))`: `rows` is `Vec<PortRow>` (port letter, supply set point `vsupply_v`, limit `vlimit_v`, measured `vsense_v`, supply current `isupply_a`, trip
`ialert_a`, and the `short` and `fault` flags, all volts or amperes), `problems` the log lines worth passing on (`parse::log_problems`). The command has a 30 s time limit.
Errors: everything `run` can return; a non-zero exit (`glasgow voltage failed (<last three stderr lines>)`, with the added hint to install
`glasgow/70-glasgow-bench.rules` when stderr contains `missing permissions`, which means the user cannot open the USB device); a table that does not parse (a short or unreadable
table is an error, never a guess; the parser's text is wrapped with the "Next:" advice). It does not take the device lock itself; the caller must hold it. It changes nothing on
the device. See also: `status_reads_the_ports_and_fails_loudly_when_it_cannot` in `src/glasgow/tests.rs`, `glasgow_parse_voltage_table` (`src/glasgow/parse.rs`).

## Safe state, list and status

### `glasgow_safe` [@ANCHOR: glasgow_safe]

Runs `glasgow safe`, which switches off every port's voltage regulator and every pin driver, and then reads every port back to prove that all are at 0 V. It is the one
function that is allowed to say "the device is safe", and it is the base of `fail_closed`, `safe_state`, the watchdog and the shutdown path.

```rust
pub fn safe(ctx: &Ctx, inst: &Instrument) -> Result<Value, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner and paths. |
| `inst` | `&Instrument`, borrowed | The instrument; its session record is removed. |

Returns `Ok` with JSON `{"outputs_enabled": false, "verified": true, "ports": [{"port", "vsupply_v", "vsense_v"}, ...]}` only when `glasgow safe` exited 0 and every port's
supply reads at most 0.05 V (the absolute value). Errors: every failure path returns a message that starts with `SAFE STATE NOT CONFIRMED: a port may still be powered or a pin
driven. Do this now: unplug the Glasgow's USB cable (every output goes off), and do not use it again until the owner has looked at the target.` followed in brackets by the
cause: the command could not run or timed out (`run`'s error), `glasgow safe` exited non-zero (exit code and last three stderr lines), the readback failed, or a port still
reads above 0.05 V (the port and its voltage). Side effects: the instrument's session record is cleared FIRST, before the command runs and whatever happens afterwards, so a
failure never leaves the server believing a port is powered at a set voltage; the watchdog thread of a cleared session finds nothing due and ends. The caller must hold the
device lock (and the dispatcher's instrument mutex). Time limit 30 s for each of the two commands. Safe to call at any time and repeatedly (idempotent). See also:
`a_readback_that_disagrees_switches_everything_off` in `src/glasgow/tests.rs`, `safe_state`, `fail_closed`, `shutdown`.

### `glasgow_list` [@ANCHOR: glasgow_list]

The tool `bench_glasgow_list` (tier `read_only`): reports which Glasgow devices are attached, their hardware revision, the USB link speed, the pinned software version and
whether the install and its gateware cache exist. It changes nothing and drives nothing, and is the first thing to call when anything looks wrong.

```rust
pub fn list(ctx: &Ctx, inst: &Instrument) -> Result<Value, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner, sysfs root and lock path. |
| `inst` | `&Instrument`, borrowed | The instrument (install directory, optional serial). |

Returns JSON with: `installed` (the program file exists), `install_dir`, `pinned_glasgow_commit` (`PINNED_COMMIT`), `install` (the content of `INSTALLED.json` or null),
`gateware_cache_present` (the directory `cache/GlasgowEmbedded/bitstreams` exists), `usb` (the kernel's view of the 20b7:9db1 device with its speed, from `parse::usb_device`; null when
not found), `devices` (an array of `{serial, revision, port_voltage_range_v}` from `glasgow list`, or null when the install is absent), `log_problems` and `warning`. The
`warning` is set (a text about the full-speed 12 Mb/s link that cannot use 512-byte bulk endpoints, with advice on cable and port) only when `usb.high_speed` is exactly
false; otherwise null. When the install is absent only the file-system facts are returned and no process is started. Errors: the device lock or `glasgow list` fails; more than
one device is listed while the entry has no `serial` (`N Glasgow devices are connected (...); set "serial" in this instrument's config entry`). It takes the device lock and
runs one 30 s command. See also: `list_reports_the_device_the_link_and_the_install` in `src/glasgow/tests.rs`, `install_dir`, `glasgow_usb_device` (`src/glasgow/parse.rs`).

### `glasgow_status` [@ANCHOR: glasgow_status]

The tool `bench_glasgow_status` (tier `read_only`): the voltage of each port as the device reports it, plus what this server recorded. It only reads.

```rust
pub fn status(ctx: &Ctx, inst: &Instrument) -> Result<Value, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner and paths. |
| `inst` | `&Instrument`, borrowed | The instrument. |

Returns JSON: `outputs_enabled` (true when any port's supply reads above 0.05 V, the device's own truth, not the server's), `pins` (a sentence saying pins are inputs unless a
tool is running, or that no port is powered), `ports` (one object per port: `port`, `supply_set_v`, `limit_v`, `measured_v`, `current_a`, `short`, `current_trip_a`, `fault`),
`recorded_by_this_server` (for each recorded port `port`, `set_v` and `seconds_left` until its deadline, or null when no record exists), `log_problems`, and `limits`
(`max_volts` and `max_on_s` of the instrument and `max_trip_a` of the glasgow config). Errors: the device lock cannot be had, or `read_ports` fails (see its entry). A mismatch
between `outputs_enabled` and `recorded_by_this_server` (for example after a server restart) is a reason to call `bench_glasgow_safe_state`. See also:
`status_reads_the_ports_and_fails_loudly_when_it_cannot` in `src/glasgow/tests.rs`, `read_ports`, `sessions`.

### `glasgow_safe_state` [@ANCHOR: glasgow_safe_state]

The tool `bench_glasgow_safe_state` (tier `read_config`): the person's or agent's "make it safe now" button. It takes the device lock and calls `safe`, so it proves 0 V on every
port or says what to do by hand. The server also runs the same `glasgow safe` when it stops (`shutdown`).

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner and paths. |
| `inst` | `&Instrument`, borrowed | The instrument. |

Returns what `safe` returns (`outputs_enabled: false`, `verified: true` and the port voltages) or its error. Additionally an error when the device lock is held elsewhere (see
`device_lock`). The tool forgets what was set (the session record is cleared). It is safe at any time and is the first call after any failure. See also:
`safe_state_proves_zero_volts_or_says_what_to_do` in `src/glasgow/tests.rs`, `safe`.

## Argument checking helpers

### `glasgow_flag` [@ANCHOR: glasgow_flag]

Reads a boolean argument such as `confirm` or `dry_run` from a tool's JSON arguments, strictly. An absent key and a JSON `null` both mean false; the booleans mean what they
say; anything else (a string `"true"`, the number 1) is refused, so a driving tool is never confirmed by a loosely typed value.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool's argument object. |
| `key` | `&str`, borrowed | The key to read. |

Returns `Ok(bool)`. Error: `<key> must be true or false` for a value of any other type. Pure, no side effects. See also: `flags_and_numbers_are_strict` in
`src/glasgow/tests.rs`, `number`, `port_set` (the main caller).

### `glasgow_number` [@ANCHOR: glasgow_number]

Reads a numeric argument that must lie in a closed range, and names the source of the range in the error so the caller knows which limit to change.

```rust
pub(crate) fn number(args: &Value, key: &str, lo: f64, hi: f64, why: &str, default: Option<f64>) -> Result<f64, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool's argument object. |
| `key` | `&str`, borrowed | The key to read. |
| `lo`, `hi` | `f64` | The inclusive range, in the unit of the argument. |
| `why` | `&str`, borrowed | Where the range comes from, inserted in the error (for example `the max_volts of this instrument's config`). |
| `default` | `Option<f64>` | Used when the key is absent or null; `None` makes the argument required. |

Returns `Ok(value)`. Errors: the key is absent or null with no default, or is not a JSON number (`<key> is required and must be a number`); the value is not finite or is outside
`lo..=hi` (`<key> <v> is outside <lo>..=<hi> (<why>)`). The default is also range-checked. Pure. See also: `flags_and_numbers_are_strict` in `src/glasgow/tests.rs`, `flag`,
`port_set`, and the applet tools in `src/glasgow/ops.rs`.

### `glasgow_revision` [@ANCHOR: glasgow_revision]

Finds the hardware revision of the device and the I/O voltage range that revision allows, which `port_set` needs before it sends anything. The revision comes from the
serial: the configured one, or, when none is configured, the only device `glasgow list` shows.

```rust
pub(crate) fn revision(ctx: &Ctx, inst: &Instrument) -> Result<(String, f64, f64), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner. |
| `inst` | `&Instrument`, borrowed | The instrument; its `serial` is used when present. |

Returns `(revision, lowest_v, highest_v)`, for example `("C3", 1.65, 5.0)`; revisions starting with `D` give 0.9 to 5.5 V, the others 1.65 to 5.0 V (from
`parse::revision_limits`). Errors: with no configured serial, `glasgow list` fails or times out (`run`'s errors); no device is listed (`no Glasgow device is listed`); several are
listed (`several Glasgow devices are connected; set "serial" in the config entry`); the serial is not a Glasgow serial. When a serial is configured, no command runs at all. With
no configured serial it runs `glasgow list` (read-only, 30 s) and does not take the device lock itself. See also: `a_port_is_set_with_the_limit_first_and_read_back` in
`src/glasgow/tests.rs`, `glasgow_revision_limits` and `glasgow_serial_ok` (`src/glasgow/parse.rs`).

### `glasgow_fail_closed` [@ANCHOR: glasgow_fail_closed]

Called by a tool when something went wrong after it may have powered a port or driven a pin (a failed readback, a changed voltage, a failed park). It runs `safe`, which switches
everything off and proves 0 V, and returns the original error text extended with the outcome, so the person reads what failed and what state the device is in.

```rust
pub(crate) fn fail_closed(ctx: &Ctx, inst: &Instrument, why: String) -> String
```

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Runner and paths. |
| `inst` | `&Instrument`, borrowed | The instrument. |
| `why` | `String`, owned | The original error, without a final full stop. |

Returns the message to use as the tool's `Err`: `<why>. The ports were switched off and read back at 0 V; the recorded settings were cleared.` when `safe` succeeded, or
`<why>. SAFE STATE NOT CONFIRMED: ... unplug the Glasgow's USB cable ...` when it did not. It never returns a success; the caller returns the string as an error. Side effects:
those of `safe` (command runs, session record cleared). The caller must already hold the device lock. See also: `a_readback_that_disagrees_switches_everything_off` in
`src/glasgow/tests.rs`, `safe`, `port_set`, `park` in `src/glasgow/ops.rs`.

## Setting a port

### `glasgow_port_set` [@ANCHOR: glasgow_port_set]

The tool `bench_glasgow_port_set` (tier `full`): sets one port's I/O voltage, or switches it off with 0. It is the only way a port gets powered and so the gate for every pin tool:
the applet tools refuse a port that was not set here and use exactly the voltage recorded here. It can put a voltage on a target board, which is the safety concern of this
driver (there is no radio, so no band check or TX gate).

```rust
pub fn port_set(ctx: &Ctx, inst: &Instrument, args: &Value) -> Result<Value, String>
```

| Argument (JSON) | Type | Meaning |
|---|---|---|
| `port` | string, required | `"A"` or `"B"`. |
| `volts_v` | number, required | The I/O voltage, volts. 0 switches the port off. Otherwise at least the revision's lowest and at most its highest (1.65 to 5.0 V for revisions A to C, 0.9 to 5.5 V for D) and never above the instrument's `max_volts`. |
| `trip_current_a` | number, optional | The supply current trip, amperes. 0 to the smaller of the config's `max_trip_a` and 0.325; default 0.1, or `max_trip_a` when that is smaller. |
| `confirm` | boolean | Must be true for any `volts_v` other than 0 (the person at the bench has said the port may be powered at that voltage). Switching off needs no confirm. |
| `dry_run` | boolean | True returns the plan and sends nothing. |
| `instrument` | string, optional | Chosen by the dispatcher (`instrument`), not read here. |

Returns on success JSON with `port`, `set_v`, `limit_v` and `measured_v` (read back), `trip_current_a`, `outputs_enabled`, `pins` (`inputs (parked)`), `switches_off_in_s`
(the instrument's `max_on_s`, or null when switched off) and `ports` (all rows). A dry run returns `{"dry_run": true, "sent": [], "would": [the four glasgow commands], "revision",
"note"}`. Errors: a missing or other `port`; a bad `volts_v` or `trip_current_a` (`number`'s errors); a non-boolean flag; the revision cannot be found (`revision`); a voltage
outside the revision's range; `volts_v` non-zero without `confirm` (the message tells how to use it); `max_on_s` unusable as a time; the device lock; the park, readback or any
`glasgow voltage-limit` or `glasgow voltage` command failing. Any failure after the rail may have been touched, and any disagreement of the readback, returns `fail_closed`'s
message (ports off, 0 V proved, session cleared).

Order of work, which is the safety argument: (1) arguments and range are checked and the plan is built before any device access; (2) the device lock is taken; (3) the deadline
(now plus `max_on_s`) is computed BEFORE anything energizes, so a later failure cannot leave a rail without a timer; (4) the port's pins are parked as inputs while the rail is
still off, so nothing a previous gateware drove is connected when the rail comes up; (5) the ports are read; (6) when RAISING (or equal) the hardware limit register is set first
and the supply second, so the limit is never below the supply; when LOWERING the supply is set first and the limit second; (7) the ports are read again and the result is
accepted only when the supply is within the larger of 0.15 V and 5 % of the setting, the limit (for a non-zero setting) is within 0.05 V, and the port shows neither fault nor
short; (8) the session record is stored (removed for 0) and, for a non-zero setting, a watchdog thread is started.

Concurrency: runs under the dispatcher's per-instrument mutex and the install's file lock; takes the session-table mutex only inside closures. Side effects: the device's supply
and limit change, `control-gpio` is loaded (a gateware with all pins inputs), the session table changes and a thread is spawned. Setting the same port again is allowed; the new
deadline replaces the old and the older watchdog finds nothing due. Setting one port to 0 does not touch the other port. Not idempotent in time (each call renews the
`max_on_s` window). For a dry run with no configured `serial`, `revision` still runs the read-only `glasgow list` to learn the board revision and so the voltage range; `sent: []` means that no command that changes anything was sent (intended: the plan needs the limits). See also: `a_port_is_set_with_the_limit_first_and_read_back` in `src/glasgow/tests.rs`, `sessions`, `spawn_watchdog`, `fail_closed`, `park`.

Example, `bench_glasgow_port_set` with `{"port": "A", "volts_v": 3.3, "trip_current_a": 0.05, "confirm": true}` powers port A at 3.3 V with a 50 mA trip and returns after the
readback shows the supply and limit at 3.3 V; `{"port": "A", "volts_v": 0}` switches it off.

### `glasgow_watchdog` [@ANCHOR: glasgow_watchdog]

Starts the thread that keeps the promise "a powered port goes off by itself after `max_on_s`". It is called by `port_set` after a non-zero set, and it works without any further
call from a person.

```rust
fn spawn_watchdog(inst: Instrument, lock_path: PathBuf, runner: Arc<dyn Runner>, sysfs: PathBuf, out_dir: PathBuf, deadline: Instant)
```

| Argument | Type | Meaning |
|---|---|---|
| `inst` | `Instrument`, owned (cloned) | The instrument to switch off. |
| `lock_path` | `PathBuf`, owned | The install's lock file. |
| `runner` | `Arc<dyn Runner>`, shared | The way to reach the device. |
| `sysfs`, `out_dir` | `PathBuf`, owned | Copied into the `Ctx` the thread builds for `safe`. |
| `deadline` | `Instant` | When to act. |

Returns nothing; the detached thread sleeps in steps of at most 200 ms until the deadline, then (1) takes the per-instrument mutex (`crate::instrument_lock`), so it never runs
in the middle of a tool call, (2) looks in the session table whether any port of the instrument has a deadline that has passed, and only if so (3) takes the device lock and runs
`safe`. If no record exists, or every deadline is still in the future (the port was set again, so a newer watchdog owns the new deadline), it does nothing and ends. Failures
(the lock cannot be had, `safe` cannot confirm) are written to standard error as `bench_mcp glasgow: the max_on_s watchdog could not ...` and the thread ends; no retry is made,
so a person must call `bench_glasgow_safe_state` (the server's own log is the only report). Because it waits for the instrument mutex, a long-running tool call delays the
switch-off until the call ends. Lock order: instrument mutex, then device file lock, then the session mutex. Each thread acts once. See also:
`the_max_on_s_watchdog_switches_the_ports_off_by_itself` in `src/glasgow/tests.rs`, `port_set`, `safe`, `sessions`.

### `glasgow_shutdown` [@ANCHOR: glasgow_shutdown]

Switches off every Glasgow this process powered. `shutdown()` is the production entry (it calls this with the empty prefix) and is called by the server when it stops (twice in
`src/main.rs`, on the stop paths); `shutdown_prefix` exists so the tests can limit the sweep to the instrument names of their own test.

```rust
pub fn shutdown()                                // all instruments
pub fn shutdown_prefix(prefix: &str)             // instruments whose name starts with prefix
```

| Argument | Type | Meaning |
|---|---|---|
| `prefix` | `&str`, borrowed | Only session entries whose instrument name starts with it are handled; `""` handles all. |

Returns nothing. For each matching session entry it runs `glasgow [--serial S] safe` through the runner kept in the entry (30 s limit), with no readback and no device lock
(the process is ending), and then removes the entry whatever happened. A failure or a non-zero exit is written to standard error as `could not confirm 'glasgow safe' for <name> at
shutdown: ... Unplug the Glasgow.`; the function never panics on a failure and never blocks longer than the commands. It is a last effort, not a proof: unlike `safe` it does not
read the voltages back. It only covers ports this process powered; a port left on by an earlier process is not in the table. Not meant to be called concurrently with tool
calls. See also: `shutdown_switches_off_what_this_process_powered` in `src/glasgow/tests.rs`, `sessions`, `safe`.

## Choosing the instrument, listing and dispatching the tools

### `glasgow_instrument` [@ANCHOR: glasgow_instrument]

Picks which configured Glasgow a call is for. A bench may configure several instruments of kind `glasgow` (for example two units, each with its own serial); the call names one,
or names none when exactly one exists.

```rust
fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | Every configured instrument of every kind; only `Kind::Glasgow` ones are candidates. |
| `args` | `&Value`, borrowed | The tool arguments; the optional string `instrument` is read. |

Returns a reference into `insts`. Errors: an `instrument` name that matches no glasgow instrument (`no glasgow instrument named "<n>"`; an instrument of another kind with that
name is not found); no `instrument` given and none configured (`no glasgow instrument is configured`); none given and several configured (`name the instrument: a, b, ...`). Pure,
no side effects. See also: `tools_are_listed_by_tier_and_the_instrument_is_chosen_exactly` in `src/glasgow/tests.rs`, `call_with`.

### `glasgow_tool_defs` [@ANCHOR: glasgow_tool_defs]

Builds the list of MCP tool definitions this driver offers for the current config. The server calls it when a client asks for the tool list. A tool is listed only when at least one
configured glasgow instrument is at or above the tool's tier, so a `read_only` instrument shows no driving tool.

```rust
pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool>
```

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | Every configured instrument. |

Returns one `rmcp::model::Tool` per matching entry of `SPECS` (the six core tools: `bench_glasgow_list`, `_status`, `_applets`, `_safe_state`, `_port_set`, `_prepare`) followed by
those of `ops::SPECS` (the applet tools). Each has the name, the description prefixed with its tier in brackets (for example `[Full]`), and a JSON schema object built from the
tool's `props` text (a JSON object body held as a string) and `required` list. The `props` text is parsed with `unwrap`, so a malformed `props` string in a `Spec` would panic here;
the tests list every tool, which keeps that from reaching a release. Pure and cheap; no device access. See also:
`tools_are_listed_by_tier_and_the_instrument_is_chosen_exactly` in `src/glasgow/tests.rs`, `call_with`.

### `glasgow_call_with` [@ANCHOR: glasgow_call_with]

The dispatcher of the driver: finds the tool, checks the tier, picks the instrument and runs the tool's function. `call(insts, name, args)` (the function the server registers, not
anchored itself) builds the production context with `real_ctx` and calls this; the tests call it with a simulated device, so the whole path (tier check, argument checks, command
words) runs without hardware.

```rust
pub fn call_with(ctx: &Ctx, insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | The way to reach the device. |
| `insts` | `&[Instrument]`, borrowed | Every configured instrument. |
| `name` | `&str`, borrowed | The full tool name such as `bench_glasgow_port_set`. |
| `args` | `&Value`, borrowed | The tool's JSON arguments. |

Returns the tool's JSON result. Errors: `unknown tool <name>` when the name is in neither `SPECS` nor `ops::SPECS`; the errors of `instrument`; `<name> needs tier <Tier>; "<instrument>" is
configured at <Tier>` when the instrument's tier is below the tool's (`ReadOnly` < `ReadConfig` < `Full`): this happens BEFORE any command runs; and the tool function's own errors. The
tool names `bench_glasgow_list`, `_status`, `_applets` (static; no device), `_safe_state`, `_port_set` and `_prepare` go to `list`, `status`, `ops::applets`, `safe_state`, `port_set`
and `ops::prepare`; every other name goes to `ops::call`. The server's dispatcher holds the instrument's mutex around the whole call (`src/main.rs`), so calls to one instrument are
serialised. It is blocking and may take as long as the command time limits (30 s for the core tools, `op_timeout_s` for applets). See also:
`a_tool_above_the_tier_or_unknown_is_refused_before_any_command` in `src/glasgow/tests.rs`, `tool_defs`, `instrument`.

## Starting and ending the `glasgow` program

### `glasgow_runner_command` [@ANCHOR: glasgow_runner_command]

Builds the operating-system command for one `Req`: which program, which arguments, which environment, and how its streams are connected. It is private to `RealRunner` and is
called by `RealRunner::run`.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `&RealRunner` | `program` (the pinned `glasgow`) and `cache_dir`. |
| `req` | `&Req`, borrowed | `args`, `nice` and `full_speed` are used here (the timeouts and the watch are used by `run`). |

Returns a `std::process::Command`, not yet started. The program is `self.program`; when `req.full_speed` is set it is instead the install's Python (`python` next to the `glasgow`
file) running that launcher script (the full-speed fallback launcher, which then behaves as `glasgow`). With `req.nice` the command is `nice -n 10` followed by the program (bitstream
builds use every core for about ten seconds). The environment gets `XDG_CACHE_HOME` set to the install's cache directory (so the bitstream cache is the pinned install's, never the
user's own `~/.cache`), `PYTHONNOUSERSITE=1` (the user's site-packages cannot change the install), and `NO_COLOR=1` (parsers see plain text); the rest of the environment is
inherited. Standard input is null and both output streams are pipes. No shell is involved, so no argument is interpreted by one. No errors here; a missing program shows when `run`
spawns it. See also: `the_real_runner_captures_output_stops_with_sigint_and_never_kills` in `src/glasgow/tests.rs`, `RealRunner::run`, `Req`.

### `glasgow_real_runner` [@ANCHOR: glasgow_real_runner]

Runs one command to completion and returns its output, ending it on a time limit by the gentlest means. It is the production implementation of the `Runner` trait (the tests use
the simulated device in `src/glasgow/testkit.rs`). It is the only place in the driver that signals a process.

```rust
fn run(&self, req: &Req) -> Result<Out, String>
```

| Field | Type | Meaning |
|---|---|---|
| `req.args` | `Vec<String>` | Arguments after the program name. |
| `req.timeout` | `Duration` | The whole call ends after this long whatever happens. |
| `req.stop_after` | `Option<Duration>` | For a capture that runs until told to stop: SIGINT after this long; the exit then counts as a success (`Out.stopped`). |
| `req.watch` | `Option<(PathBuf, u64)>` | A file the program writes and a size in bytes; SIGINT is sent when the file grows past the size (a capture of a busy bus must not fill the disk). |
| `req.nice` | `bool` | Run at low priority. |
| `req.full_speed` | `Option<PathBuf>` | The launcher script for the full-speed fallback. |

Returns `Ok(Out)` with `code` (the exit code, or None when killed by a signal), `stdout` and `stderr` (UTF-8 with replacement), and three flags: `timed_out` (this runner ended it
because `timeout` passed), `stopped` (this runner ended it on purpose, by `stop_after` or the size limit; the output is complete up to there), `stuck` (still running after SIGINT and
SIGTERM). Errors: the program cannot be started (`cannot start <path>: <os error>. Next: run daemons/bench_mcp/glasgow/install_glasgow.py (see manual instruments/glasgow/setup) or
fix the config's "venv".`); the pid or the pipes cannot be handled; waiting on the child fails.

Process ladder: it checks the child every 50 ms; when `stop_after` or the size limit is reached it sends SIGINT (what Ctrl+C does: Glasgow closes its files and stops cleanly) and
marks `stopped`; otherwise at `timeout` it sends SIGINT and marks `timed_out`; 10 s after SIGINT, if the child is still there, SIGTERM; 10 s after SIGTERM, if still there, it gives
up and returns `stuck` with a message that names the pid, says the child was left running, says "no SIGKILL is ever sent", and tells a person to look at it and then call
`bench_glasgow_safe_state`. SIGKILL is never sent, and the only pid ever signalled is the one this function started (an `unsafe kill(2)` with that pid while it is not yet
reaped). Two helper threads read stdout and stderr so a full pipe cannot stall the child; for a stuck child they are not waited for. Blocking: the calling thread sleeps up to
the timeout plus about 20 s. The caller holds the device lock. See also: `the_real_runner_captures_output_stops_with_sigint_and_never_kills` in `src/glasgow/tests.rs`, `run`
(the wrapper that turns `timed_out` and `stuck` into errors), `RealRunner::command`.

### `glasgow_install_paths` [@ANCHOR: glasgow_install_paths]

Says where, inside an install directory, the `glasgow` program and the gateware cache are.

```rust
pub fn install_paths(dir: &Path) -> (PathBuf, PathBuf)
```

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path`, borrowed | An install directory (from `install_dir`). |

Returns `(<dir>/venv/bin/glasgow, <dir>/cache)`: the program the runner starts and the directory handed to it as `XDG_CACHE_HOME` (the built bitstreams end up under
`cache/GlasgowEmbedded/bitstreams`). Pure string work; the files need not exist. It must match the layout `install_glasgow.py` creates (a venv at `DEST/venv` and `DEST/cache`).
See also: `install_dir_is_configured_or_the_pinned_default` in `src/glasgow/tests.rs`, `install_dir`, `real_ctx`, `list`.

## The installer

### `glasgow_install_sha256` [@ANCHOR: glasgow_install_sha256]

Computes the SHA-256 digest of a file, as a lower-case hexadecimal string. The installer uses it twice: to check the downloaded archive against the pinned `ARCHIVE_SHA256`, and to
record the digest of `requirements.txt` in `INSTALLED.json`. It is in `glasgow/install_glasgow.py`.

```python
def sha256(path) -> str
```

| Argument | Type | Meaning |
|---|---|---|
| `path` | path-like | The file to read in binary mode. |

Returns 64 hexadecimal characters. It reads in 1 MiB blocks, so a large file costs no memory. Raises the usual `OSError` if the file cannot be opened or read (not caught). No side
effects. See also: `test_sha256_of_a_file` in `glasgow/test_glasgow_tools.py`, `copy_bounded`, `main`.

### `glasgow_install_copy_bounded` [@ANCHOR: glasgow_install_copy_bounded]

Copies the source archive from an open binary stream (the download, or the file a person passed with `--archive`) into a private file, refusing a stream that is longer than the
expected archive. The caller then hashes and extracts the COPY, so a later change to the original, or an endless download, cannot get past the check.

```python
def copy_bounded(src, dst_path) -> None
```

| Argument | Type | Meaning |
|---|---|---|
| `src` | open binary file-like (has `read(n)`) | The stream to copy; not closed here. |
| `dst_path` | path-like | The file to create or overwrite (`DEST/glasgow.tar.gz`). |

Returns nothing. It copies in 1 MiB blocks and stops with `SystemExit("REFUSED: the archive is larger than <limit> bytes (<ARCHIVE_BYTES> expected). Nothing from it was
installed.")` when more than `ARCHIVE_BYTES + 1 MiB` bytes (51 826 741 plus 1 048 576) have been read; the partial file stays on disk until `main` removes the destination. A
shorter stream is accepted here, because the hash check later decides. Blocks on the stream's reads (a download has its own 300 s timeout in `main`). The only side effect is the
destination file. See also: `test_the_archive_is_copied_into_dest_and_the_copy_is_what_is_extracted_and_a_huge_one_is_refused` in `glasgow/test_glasgow_tools.py`, `main`.

### `glasgow_install_run` [@ANCHOR: glasgow_install_run]

Runs one command of the installation, printing it first. A failing command stops the installation. It is in `glasgow/install_glasgow.py` (not the same function as the Rust `run`).

```python
def run(cmd, **kw) -> None
```

| Argument | Type | Meaning |
|---|---|---|
| `cmd` | list | The program and its arguments, run without a shell. |
| `kw` | keyword arguments | Passed to `subprocess.run` (for example `env=` for the build version). |

Prints `+ <command words>` to standard output (flushed) and runs the command with `check=True`. Raises `subprocess.CalledProcessError` when the command exits non-zero or `OSError`
when it cannot start; neither is caught, so the script ends with a Python traceback and a non-zero status. It waits for the command (no timeout). It is used for the venv creation,
the hashed requirements install and the install of Glasgow itself. See also: `test_run_runs_a_command_and_raises_on_failure` in `glasgow/test_glasgow_tools.py`, `main`.

### `glasgow_install_main` [@ANCHOR: glasgow_install_main]

The installer script's entry point: builds the pinned, isolated environment the driver runs. It is run once by a person (`nice -n 10 python3 -I install_glasgow.py --dest
~/.local/share/bench_mcp/glasgow-70cb38c9`), never by the server. It touches only `DEST`; it never changes another venv, the system Python or `/etc`.

```python
def main(argv=None) -> None
```

| Argument | Type | Meaning |
|---|---|---|
| `--dest` | path, required | The directory to create. It must not exist; an existing one is refused with `<dest> exists; remove it first (this script never reuses or modifies an existing environment)`. `~` is expanded. |
| `--python` | path, default the running interpreter | A Python 3.13 or newer to build the venv with. |
| `--archive` | path, optional | A copy of Glasgow's source archive already on disk; it is copied into `DEST` and the copy is checked and installed. Without it the archive is downloaded from `https://codeload.github.com/GlasgowEmbedded/glasgow/tar.gz/<commit>` with a 300 s timeout. |
| `argv` | list or None | The arguments (None: `sys.argv`), so the tests can call it. |

Steps, in order: refuse an existing `DEST`; lower its own priority with `os.nice(10)` (ignored if not allowed); create `DEST`; copy the archive into `DEST/glasgow.tar.gz` through
`copy_bounded`; compute its sha256 and REFUSE unless it equals `ARCHIVE_SHA256` (`d20f1910...8139`; the message says GitHub does not promise stable archive bytes and a person must
re-verify the commit and update the pin); create the venv at `DEST/venv`; `pip install --require-hashes --only-binary=:all: -r requirements.txt` (every package at an exact version,
a file whose hash is not listed is refused); extract the archive into `DEST/src` with the `data` tar filter (no links outside, no special files); install Glasgow from
`DEST/src/glasgow-<commit>/software` with `--no-deps --no-build-isolation` and `PDM_BUILD_SCM_VERSION` set to the pinned version `0.1.dev0+g70cb38c`; create `DEST/cache`; run
`glasgow --version`; write `DEST/INSTALLED.json` (the commit, the program's reported version, the archive hash, the requirements file's hash, and the Python version, as JSON) and
print `installed: glasgow <version> at <dest>`.

Failure: an oversized archive or a hash mismatch removes `DEST` completely (no partial install) and exits with a message that starts `REFUSED`; a later step that fails (a pip
error, a missing Python 3.13) leaves `DEST` in place and raises, and the person removes `DEST` before trying again. The script exits 0 only after a complete install. Constants at
the top: `COMMIT` (`70cb38c9b03471acd2eabb76c82308891bec2ca5`, the same as `PINNED_COMMIT` in `mod.rs`: unknown whether anything checks that the two agree), `ARCHIVE_URL`,
`ARCHIVE_SHA256`, `ARCHIVE_BYTES` (51 826 741) and `VERSION`. It needs network access to GitHub and to PyPI (through pip). It builds nothing on the device and drives no hardware.
See also: `test_the_archive_is_copied_into_dest_and_the_copy_is_what_is_extracted_and_a_huge_one_is_refused`, `test_an_archive_with_the_wrong_hash_is_refused_before_anything_is_installed`,
`test_an_existing_destination_is_never_reused` and `test_the_pin_names_the_commit_and_the_requirements_are_hashed` in `glasgow/test_glasgow_tools.py`, `sha256`, `copy_bounded`,
`run` (installer), `install_paths`.

## The requirements pin generator

### `glasgow_pin_hashes` [@ANCHOR: glasgow_pin_hashes]

Asks PyPI which files exist for one package version and returns the sha256 of each Linux wheel and source archive, sorted. It is the data source of `requirements.txt`: with these
hashes `pip install --require-hashes` accepts any of the listed files and nothing else. It is in `glasgow/pin_requirements.py`, which a person runs when bumping a pin; the server
never runs it.

```python
def hashes(name, version) -> list[str]
```

| Argument | Type | Meaning |
|---|---|---|
| `name` | str | The PyPI project name, as written in `PINS`. |
| `version` | str | The exact version. |

Reads `https://pypi.org/pypi/<name>/<version>/json` (60 s timeout) and keeps the entries whose `packagetype` is `bdist_wheel` or `sdist` and whose file name (lower-cased) contains
none of `win`, `macosx`, `ppc64`, `s390x`, `armv7`, `i686`, `riscv`, `musllinux`, `freebsd` (so only the platforms the bench uses stay). Returns the sorted list of the `sha256` digests.
Errors: the network or JSON failure raises (`urllib` or `json` errors, not caught); no file is left after filtering (`<name> <version>: no usable files`, `SystemExit`). The only
side effect is the network request. Note that the filter is a substring test, so a file whose name merely contains one of those words (for example in a project name) is dropped too.
See also: `test_only_linux_files_are_kept_and_sorted` in `glasgow/test_glasgow_tools.py`, `main` (pin generator).

### `glasgow_pin_main` [@ANCHOR: glasgow_pin_main]

Prints the whole `requirements.txt` to standard output. It is run by a person as `python3 pin_requirements.py > requirements.txt` after changing the `PINS` list (the exact versions,
measured on 2026-11-08 according to the comment in the file; `glasgow/DEPENDENCIES.md` records them with their licences). Glasgow itself is not in the list; it is installed from the
source archive by `install_main`.

```python
def main() -> None
```

No arguments. Prints a header comment line, then for each package of `PINS` in case-insensitive name order one block of the form `name==version \` followed by one
`    --hash=sha256:<digest>` line for each digest from `hashes`, joined with backslash continuations. Any failure of `hashes` stops the script part-way, so the redirect leaves a
truncated file: the person must not commit it (the file is only valid after a complete, successful run). The header's date is fixed text in the code. Network requests: one per
package, in sequence. No other side effects. See also: `test_main_prints_every_pin_with_its_hashes` in `glasgow/test_glasgow_tools.py`, `hashes`, `main` (installer), which reads the
file this prints.
