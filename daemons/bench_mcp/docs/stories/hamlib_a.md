# The radio rigs: the front door, receiver control, the connection test, time-station detection and the transmit watchdog

As the owner of a bench with a radio on it, I want an AI assistant to be able to find my rig, read what it hears, retune it, scan a band and tell me
why it does not answer, without ever being able to key the transmitter by accident, and with a watchdog that un-keys the rig if a transmit test goes
wrong. This story covers the first half of the `radioctl` driver of `daemons/bench_mcp` (the files `src/radioctl/mod.rs`, `control.rs`, `detect.rs`,
`diag.rs` and `guard.rs`). The driver was called `hamlib` until pull request #1568 replaced Hamlib's `rigctld` by `radioctld` (the program of
`daemons/radioctl`, which speaks the same line protocol over a TCP socket); the tools are now `bench_radioctl_*` and every anchor id starts with
`radioctl_`. This page describes the code as it is now. The users are the MCP (Model Context Protocol) tool dispatcher (`call`), the other files of the driver (`status.rs`,
`tx.rs`, `txtools.rs`, `loadtools.rs`, `rxcheck.rs`, `rigd.rs`), and the people who extend them. The rest of the driver (the transmit path, the load
check, the station table, the daemon manager) is documented in its own story files. The tool list and the safety model are in
`docs/RADIOCTL_TOOLS.md`.

The words used below: a *rig* is one configured instrument of kind `radioctl`; *radioctld* is the daemon that holds the serial port (or a virtual rig)
and answers text commands such as `get_freq` or `set_ptt 1`; a *tier* is one of `read_only` < `read_config` < `full` (`config::Tier`), set per instrument
in the bench config and required per tool.

## The front door: configuration, choosing the rig, running a tool

### `radioctl_config` [@ANCHOR: radioctl_config]

`parse_config(c: &Value) -> Result<Config, String>` reads the `radioctl` settings out of one instrument entry of the bench config
(`BENCH_MCP_CONFIG`) and checks every value before any rig is touched. It is called by `config.rs` when the entry's driver is `radioctl`; the result
is stored as `Instrument::radioctl` and is read by every tool of the driver. There are two shapes of entry. A *managed* rig names a radioctl rig
description with `rig` and this server starts and stops a `radioctld` for it, bound to 127.0.0.1. An *existing* rig gives `radioctld_port` for a
`radioctld` that already runs. Giving both, or neither, is an error. The keys `driver`, `tier` and the instrument name belong to the caller, not to this
function.

| Key | Type | Meaning, range, default |
|---|---|---|
| `rig` | string | Managed only. A rig description id (file `<id>.toml` in the rigs directory): 1 to 80 characters of letters, digits, `-`, `_`. |
| `device` | string | Managed only, optional. A serial device path: starts with `/dev/`, no `..`, at most 120 characters of letters, digits and `_ . / : -`, and it must pass `serial_guard::check_reserved_open` (the reserved-port list). Absent or null: a virtual rig made from the description. |
| `speed` | integer | Managed only. Baud rate, one of 1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600. Required with `device`; refused without it. |
| `rigs_dir` | string | Managed only, optional. Absolute path, no `..`, of the rig description files. Default: the `RADIOCTL_RIGS` environment variable (looked up later by `rigd::rigs_dir`). The path is not checked for existence here. |
| `listen_port` | integer | Managed only, optional. The port the daemon listens on, 1024 to 65535. Absent: the manager chooses. |
| `full_power_w` | number | Managed only, optional. The rig's full-scale RF output in watts from its manual, 0.1 to 2000. The transmit tools turn the owner's power cap into a power setting with it. Required when `transmit` is given. |
| `radioctld_port` | integer | Existing only. 1 to 65535. |
| `radioctld_host` | string | Existing only. Default `127.0.0.1`. Anything but `127.0.0.1`, `localhost` or `::1` is refused unless `allow_remote_radioctld` is true; the characters are limited to letters, digits and `_ . / : -`. |
| `allow_remote_radioctld` | boolean | Existing only. Default false. The owner accepts that radioctld has no authentication. |
| `reply_timeout_s` | integer | Both. How long one command may take, 1 to 30 seconds. Default 3. |
| `rx_ranges_hz` | list of `[low, high]` | Both. The owner's receive limits in Hz, each finite with `0 <= low < high`. Default empty (the rig's own ranges apply alone). |
| `managed_dir` | string | Both. Absolute path, no `..`, where daemon logs, keyed-interval state and logs and baselines are kept. Default: a `radioctl` directory under the bench output directory (`rigd::dir`). |
| `audio` | object | Both. `{instrument, input}`: the name of an `audio` instrument of the bench config and the alias or name fragment of its input that carries the rig's receive audio. Both parts must be non-blank. |
| `transmit` | object | Both. The owner's dummy-load attestation and limits; parsed by `attest::parse_transmit` (see the attestation story). Its error is returned prefixed `transmit: `. Without it no tool transmits. |

The result is `Config { source, reply_timeout_s, rx_ranges_hz, managed_dir, audio, transmit }`. `source` is `Source::Managed(Managed { rig, device, speed, port,
rigs_dir, full_power_w })` or `Source::Existing { host, port }`. `Config::default()` is an existing rig at `127.0.0.1:4532`, 3 s timeout, no ranges, no
audio, no transmit; `parse_config` starts from it, but a rig or a port is always required so the default source is never the result.

Errors (all `Err(String)`, each says what to change): the old Hamlib keys `model` and `set_conf` are rejected with a message that points at `rig`; both or
neither of `rig` and `radioctld_port`; a bad `rig`, `device`, `speed`, `rigs_dir`, `listen_port`, `full_power_w`, `radioctld_port`, `radioctld_host`,
`reply_timeout_s`, `rx_ranges_hz`, `managed_dir` or `audio`; a `device` without `speed` or the reverse; a remote host without the explicit permission; a managed
rig with `transmit` but no `full_power_w`; and `transmit.operator_alert` without `audio` (the alert is played through the audio instrument).

Concurrency and side effects: a pure function of its argument, except that `check_reserved_open` consults the reserved-serial list. It opens nothing, starts
no process and never talks to a rig. A `Config` is immutable after parsing and is shared by reference. This function does not decide whether transmitting
is possible with an existing radioctld that has no `full_power_w` key (the key is only read for a managed rig); the transmit tools make that decision.
Tests: `tests.rs` (`// Tests [@ANCHOR: radioctl_config]`). Related: `radioctl_rigs`, `radioctl_open`, `rigd::dir`, `rigd::rigs_dir`.

### `radioctl_safe_name` [@ANCHOR: radioctl_safe_name]

`safe_name(s: &str) -> String` turns an instrument or rig name into text that is safe inside a file name. Every character that is not an ASCII letter, a digit,
`-` or `_` becomes `_` (one `_` per character, so a two-byte letter becomes one `_`); nothing is trimmed or shortened. It is used wherever a per-rig file is
named: `<name>.radioctld.log` (the daemon log), the keyed-interval state file and log, the watchdog log and the baselines. The argument is borrowed; the result
is a new string; the function cannot fail and has no side effects. Two different names can map to the same text (`a b` and `a.b` both give `a_b`); the bench
config's instrument names are unique, and the owner is expected to avoid names that differ only in punctuation. An empty name gives an empty string.
Tests: `tests.rs`. Related: `rigd::dir`, `radioctl_diagnose` (reads the log), `tx::state_file`.

### `radioctl_instrument` [@ANCHOR: radioctl_instrument]

`instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String>` picks the rig a tool call is about. It looks only at the
instruments of kind `radioctl`. When `args["instrument"]` is a string, the rig with that exact name is returned. Otherwise, if exactly one radioctl rig is
configured it is returned. It does not look at the tier; the caller compares it.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` | Every configured instrument of every driver; borrowed, the result borrows from it. |
| `args` | `&Value` | The tool arguments; only the optional string `instrument` is read. A non-string `instrument` is ignored. |

Errors: `no radioctl instrument named "<n>"` (name given, none matches, or it names an instrument of another driver); `no radioctl instrument is configured`;
`name the rig: a, b, c` when several are configured and none was named. Pure, no locks, no I/O. Called by `call` and by `rigs`-style listings.
Tests: `tests.rs`. Related: `radioctl_call`, `radioctl_rigs`.

### `radioctl_open` [@ANCHOR: radioctl_open]

`open(inst: &Instrument) -> Result<Client, String>` gives a tool a connection to the rig's `radioctld`, starting the managed daemon first if it is not running.
It is called at the start of almost every rig tool (status, get, watch, set, memory, scan, rx_check, the load tools, the transmit tools, safe state), inside
`call` and, for those tools, after the instrument lock is held (safe state is the exception, see `radioctl_call`).

Steps: (1) `rigd::endpoint` returns the address: the configured host and port for an existing rig, or `127.0.0.1` and the daemon's port for a managed rig, starting
the daemon on demand (a leftover daemon of a dead server is stopped first, a daemon that failed four times in ten minutes is not restarted by itself, a
missing or unusable serial device is reported before the start). (2) If the rig's config has a `transmit` attestation, `tx::startup_check` runs once per rig
name per run of this server: it sends `set_ptt 0` (un-key; a receiving rig ignores it), reads the PTT (push-to-talk) state and any state file a crashed server left behind,
and if it finds the rig keyed it also sets the lowest power setting and leaves a note for `radioctl_startup_note`. This happens on the first call of any tier,
even a `read_only` status call, because the question "did a crash leave the rig transmitting" must be answered before anything else; it never keys.
(3) `Client::connect` opens the TCP connection with `reply_timeout_s` as the reply limit.

Returns a `Client` (owned by the caller, one connection, reopened by itself after a link failure; a read is retried once, a write never). Errors are text: the
daemon could not be started or reached, or the connect failed. Blocking: starting the daemon waits for it to be ready (up to ten seconds, `rigd`); the
transmit-startup check connects with a 2 second limit. Side effects: may start a `radioctld` process and write its log, may write the keyed-interval log and
remove a stale state file. It takes no lock itself; the caller holds the instrument lock. Tests: `tests.rs`. Related: `radioctl_call`, `radioctl_startup_note`,
`radioctl_endpoint`, `radioctl_tx_startup_check`.

### `radioctl_call` [@ANCHOR: radioctl_call]

`call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String>` is the entry point the bench_mcp dispatcher uses for every tool whose name starts
with `bench_radioctl_` (`main.rs` registers it with the prefix and `tool_defs`). It finds the tool in `SPECS`, enforces the tier, serialises calls on a rig
and routes to the function that does the work. `args` is the tool's JSON object (see each tool's `props` in `SPECS` for the keys); the result is the tool's JSON
value, or an error text that says what was wrong and, where there is one, what to do next.

Routing and checks, in order:

1. An unknown `name` is `unknown tool <name>`.
2. `bench_radioctl_models` needs no rig: it only needs some radioctl rig configured at or above the tool's tier (`read_only`). It lists the rig description files
   of the first managed rig's rigs directory, or of `RADIOCTL_RIGS` when only existing rigs are configured.
3. `bench_radioctl_rigs` goes to `rigs` (no lock, starts nothing).
4. `bench_radioctl_stations` and `bench_radioctl_station_set` choose the rig with `radioctl_instrument`, check its tier, and work on the station table files; they do not
   contact the rig and take no instrument lock.
5. Every other tool: choose the rig, then `<name> needs tier X; "<rig>" is configured at Y` unless the instrument's tier is at least the tool's tier (`Tier`
   orders `ReadOnly < ReadConfig < Full`).
6. `bench_radioctl_safe_state` is run now, without taking the instrument lock: it must work while a keyed interval is running and end it. The lock is deliberately
   not taken for this one tool.
7. For all the rest the per-instrument lock (`instrument_lock(name)`, one call at a time per rig, because a serial port has a single conversation) is taken and held
   until the tool returns; a lock poisoned by a panic is recovered, not an error. The tool then runs: status, watch, get, set, memory, scan, rx_check, load_check,
   load_baseline, tx_key_test, tx_power_sweep, diagnose, daemon. Anything else is `<name> does not apply to "<rig>"`.
8. The result passes through `radioctl_startup_note`.

How this relates to the transmitter. Tiers: the transmit tools (`bench_radioctl_tx_key_test`, `bench_radioctl_tx_power_sweep`) and the baseline recorder are `full`;
`bench_radioctl_set`, `_memory`, `_scan`, `_rx_check`, `_load_check`, `_safe_state`, `_station_set` and `_daemon` are `read_config`; the rest are `read_only`. The four tools in
`NEEDS_ATTESTATION` (load_check, load_baseline, tx_key_test, tx_power_sweep) are not even listed by `tool_defs` for a rig without a `transmit` block; `call` does not
refuse them by name, and the refusal when called anyway is in `txtools` (`no tool transmits with it`; the load tools were not re-checked for this page). The lock is the one above and is held for the whole keyed interval. `call` itself does not apply
the owner's band list, the power and time caps, `confirm_dummy_load`, the load-plausibility gate or the low-power probe: those are applied inside
`txtools::key_test` and `txtools::power_sweep` (anchors `radioctl_tx_args`, `radioctl_tx_gate`, `radioctl_tx_preflight`), after the tier check and with the lock held,
and the watchdog (`radioctl_guard_spawn`) is armed before the first `set_ptt 1`. For the transmit tools `call` passes the real process spawner
(`guard::ProcessSpawner`), the optional operator-alert audio and the rig's audio source. The receiver-control tools never key (see `radioctl_set`).

Errors are `Err(String)`. Blocking: yes, a call can last as long as the tool (a scan up to about 2 minutes, a keyed interval up to 60 seconds plus checks); a second call
on the same rig waits for the lock. Tests: `tests.rs` (tier refusal, routing). Related: `radioctl_open`, `radioctl_instrument`, `radioctl_startup_note`,
`radioctl_rigs`, `radioctl_daemon_tool`, `radioctl_shutdown_all`.

### `radioctl_startup_note` [@ANCHOR: radioctl_startup_note]

`with_startup_note(inst: &Instrument, r: Result<Value, String>) -> Result<Value, String>` adds the finding of the first-contact PTT check to the result of the tool
that triggered it. On `Ok` it asks `tx::take_note(rig)` for the note (the note is removed from its store when asked, so it is delivered once); if there is a note
and the result is a JSON object, the note is inserted under the key `startup_release`. The note holds `stuck_ptt_found`, `ptt_before` (`on`, `off`, `unknown`),
`state_file_found`, `released`, `error` and a sentence saying that the rig was left keyed and was released and set to minimum power. A note exists only when
the check found the rig keyed or found a crash record; a rig that was not transmitting leaves no note (the release is still logged in the keyed-interval log).
An `Err` result passes through unchanged and the note stays for the next successful call. If the result is `Ok` but not a JSON object, the note is taken and dropped.
No locks beyond the note store's own mutex; no I/O. Tests: `tests.rs`. Related: `radioctl_open`, `radioctl_tx_startup_check`, `radioctl_tx_take_note`.

### `radioctl_daemon_tool` [@ANCHOR: radioctl_daemon_tool]

`radioctld_action(inst, args)` is the tool `bench_radioctl_daemon` (tier `read_config`): manage the `radioctld` this server runs for a rig. `args["action"]` is required
and is one of `status`, `start`, `stop`, `restart` (anything else: `action "x" is not status, start, stop or restart`; absent: `action is required: ...`).

| Action | What it does |
|---|---|
| `status` | `rigd::status`: whether the daemon runs, its port and pid, starts in the last 10 minutes, the last exit; for an existing rig, host and port with `managed: false`. Starts nothing. |
| `start` | `rigd::start`: starts the daemon (forgetting earlier failures so the restart limit begins again). For an existing rig: `uses an existing radioctld; this server does not start or stop it`. |
| `stop` / `restart` | First `tx::release_rig`: a rig this process has keyed, or one a state file says was keyed, is un-keyed and read back, and the stop is refused with `not stopping the daemon: ...` when that cannot be confirmed (once the daemon is gone there is no way left to release the rig). Then `rigd::stop`; `restart` then calls `rigd::start`. |

The result is the JSON of the underlying call. The daemon listens on 127.0.0.1 only. The instrument lock is held by `call`. Side effects: starts or stops a process,
may send `set_ptt 0` (through `release_rig`) to a rig that may be keyed. It never keys. Tests: `tests.rs`. Related: `radioctl_call`, `radioctl_shutdown_all`,
`radioctl_tx_release_rig`, `radioctl_daemon_status`.

### `radioctl_rigs` [@ANCHOR: radioctl_rigs]

`rigs(insts: &[Instrument], args: &Value) -> Result<Value, String>` is the tool `bench_radioctl_rigs` (tier `read_only`): every configured rig, whether its daemon
runs and whether the rig answers now. It starts nothing and never keys. `args["instrument"]` (optional string) limits the list to one rig.

For each radioctl rig it reads `rigd::status`, works out an address (the configured one for an existing rig; `127.0.0.1` and the port for a managed rig whose daemon runs;
none otherwise) and, if there is one, connects with the rig's reply timeout and sends `get_freq`. Result: `{"rigs": [{"name", "tier", "radioctld": <status>, "rig": ...}]}`
where `rig` is `{"answers": true, "frequency_hz": f}`, `{"answers": false, "error": text}`, or the text `not started: the daemon starts at the first call that needs the rig`.
The tool description also promises "whether transmitting is configured", but the code does not put that in the result. Errors: `no radioctl instrument named "n"` or `no radioctl instrument is
configured` when nothing matches. It does not take the instrument lock (`call` routes it before the lock), so its single `get_freq` read can interleave with a running
tool; it is a read and changes nothing. One short TCP conversation per rig, so the cost grows with the rig count and with the reply timeout of an unreachable rig.
Tests: `tests.rs`. Related: `radioctl_call`, `radioctl_instrument`, `radioctl_daemon_status`.

### `radioctl_shutdown_all` [@ANCHOR: radioctl_shutdown_all]

`shutdown_prefix(prefix: &str)` stops what this module started, for the rigs whose instrument name starts with `prefix` (the empty prefix means all of them;
`shutdown()` is the same with the empty prefix, called when the server stops, see the two calls in `main.rs`). Order matters and is the point of the function: first
`tx::release_prefix(prefix)` un-keys every rig this process has recorded as possibly keyed (`guard::unkey`, with the rig's minimum power), and every failure is printed to
standard error as `bench_mcp radioctl: could not confirm the release of a keyed rig: ...`; only then `rigd::shutdown_prefix` kills the managed daemons, because a stopped daemon
makes a release impossible. Returns nothing; failures are reported, not raised, since the server is exiting. It takes no instrument lock (the server is going down; a
running keyed interval is ended by its own release or by its watchdog). Side effects: network commands to the rigs, process kills of the daemons this server started
(never anyone else's). Tests: `tests.rs`. Related: `radioctl_tx_release_all`, `radioctl_shutdown`, `radioctl_daemon_tool`.

## Receiver control (`control.rs`, tier `read_config`)

Nothing in this file keys the transmitter. The tools refuse, before sending anything, every level and function that sets what the transmitter does (RF (radio frequency) power,
microphone gain, key speed, compression, VOX: voice-operated transmit, tuner, tone, monitor, break-in, stored morse and voice messages: the list `TRANSMIT_SIDE`), they refuse to act while the
rig reports PTT on, and they send only these commands: `set_vfo`, `set_freq`, `set_mode`, `set_split_vfo`, `set_rit`, `set_xit`, `set_level`, `set_func`, `set_mem` and reads.
There is no band check for transmitting here because nothing here can transmit; the receive-range check (`radioctl_freq_guard`) is the only frequency check, and the
transmit lock, tier and gate (`full` tier, attestation, owner's ranges, load check, watchdog) belong to the transmit tools. `split` and `xit` change which frequency a
later transmission would use; they are allowed because they are receiver-side settings the owner may need, and they are not range-checked.

### `radioctl_rx_ranges` [@ANCHOR: radioctl_rx_ranges]

`rx_ranges(c: &mut Client, cfg: &Config) -> Result<(Vec<(f64, f64)>, String), String>` returns the receive ranges in force for a rig and a text saying where the limit came from.
It sends `dump_state` (a read: one retry) through `Client`, ignores a rig-side refusal of it (then the rig contributes no ranges), parses the receive list with
`parse_dump_ranges` and narrows it by `cfg.rx_ranges_hz` with `combine_ranges`. Arguments: `c` the open connection (mutably borrowed, used for one command), `cfg` the rig's config (read
only). Return: pairs `(low_hz, high_hz)` and the source text (see `combine_ranges`). An empty list means no range check. Error: a link failure while reading (`tolerate` turns a link
error into `Err`, a rig refusal into data). Called by `set`, `memory`, `scan`, `rxcheck.rs` and `loadtools.rs`. Blocking: one round trip. No lock beyond the instrument lock `call` holds.
Tests: `tests.rs`. Related: `radioctl_dump_ranges`, `radioctl_combine_ranges`, `radioctl_freq_guard`, `radioctl_tolerate`.

### `radioctl_dump_ranges` [@ANCHOR: radioctl_dump_ranges]

`parse_dump_ranges(lines: &[String]) -> (Vec<(f64, f64)>, Vec<TxRange>)` reads the frequency lists out of the lines of a `dump_state` reply. `TxRange` is
`(low_hz, high_hz, lowest_power_mw, highest_power_mw)` (`f64`s). The reply starts with three single-value lines (protocol version, model, region), which are skipped; then
the receive list, ended by an all-zero line, then the transmit list, ended by a second all-zero line. Each list line has seven whitespace-separated fields: low and high
frequency in Hz, modes mask, lowest power, highest power, VFO (variable frequency oscillator, the tuning control) mask, antenna mask. The parser takes fields 0 and 1 as Hz; for the transmit list it takes fields 3 and 4 as
power (a field that is not a number becomes `-1.0`, as radioctld writes `-1 -1` for receive lines). The first line that does not have seven fields, or whose first two fields are
not numbers, ends the parse silently, so a malformed or truncated reply gives the ranges found so far, possibly none. Returns `(receive, transmit)`; both may be empty. A pure function; no
errors and no panics. Receive ranges feed `rx_ranges`; the transmit list is used by the transmit path to check the owner's frequency against the rig's own transmit range before
keying (the owner's list is applied first, in `txtools::check_args`). Tests: the unit test in `control.rs` (`dump_state_ranges_are_read_until_the_zero_lines`) and `tests.rs`.
Related: `radioctl_rx_ranges`, `radioctl_dump_state`.

### `radioctl_combine_ranges` [@ANCHOR: radioctl_combine_ranges]

`combine_ranges(rig: &[(f64, f64)], owner: &[(f64, f64)]) -> (Vec<(f64, f64)>, String)` combines the rig's reported receive ranges with the owner's `rx_ranges_hz`: the
owner's limits narrow the rig's, never widen them. Both arguments are borrowed lists of `(low_hz, high_hz)`. The four cases:

| Rig reports | Owner sets | Result | Source text |
|---|---|---|---|
| nothing | nothing | empty list | `the rig reports no receive range and the config sets none: frequencies are not range-checked` |
| ranges | nothing | the rig's | `the rig's reported receive range` |
| nothing | ranges | the owner's | `rx_ranges_hz in the bench config` |
| ranges | ranges | every non-empty overlap of every rig range with every owner range (`max(low)` to `min(high)` where it is below) | `the rig's reported receive range narrowed by rx_ranges_hz in the bench config` |

Pure, no errors. Note, as the code stands: when both lists exist but nothing overlaps, the result is an empty list, and an empty list means "no range check" to `check_freq`, so such a
configuration accepts every frequency instead of none. The owner's `rx_ranges_hz` should therefore overlap the rig's range; the case is not caught by `parse_config`, which does not
know the rig's range. Tests: `tests.rs`. Related: `radioctl_rx_ranges`, `radioctl_freq_guard`.

### `radioctl_freq_guard` [@ANCHOR: radioctl_freq_guard]

`check_freq(f: f64, ranges: &[(f64, f64)], source: &str) -> Result<(), String>` refuses a receive frequency outside the ranges in force. `f` is the frequency in Hz; `ranges` the
list from `rx_ranges` (`(low, high)`, both ends inclusive); `source` the text that names the origin of the limit. `Ok(())` when `ranges` is empty or `f` lies in any range. Errors: a
frequency that is not finite or not positive (`frequency NaN Hz is not a positive finite number`), or one outside every range, in the words `frequency 7100000 Hz is outside the receive
range (a to b Hz, ...); source: <source>`. The message always names the limit and where it came from, so the owner can tell a rig limit from their own. Pure, no I/O. Called by `set`
(the new frequency), `memory` (a warning only) and `scan` (start and stop). It is a receive-side check; it is not the transmit band check. Tests: `tests.rs`. Related:
`radioctl_rx_ranges`, `radioctl_combine_ranges`, `radioctl_tx_args` (the transmit band check).

### `radioctl_ptt_is_on` [@ANCHOR: radioctl_ptt_is_on]

`ptt_is_on(c: &mut Client) -> Result<bool, String>` asks the rig whether it is transmitting, so that the control tools can refuse to work on a keyed rig. It sends `get_ptt` (a read, one retry)
and returns true when the reply's `PTT` value is anything but `0`. A rig that refuses the command (it cannot report PTT) or a reply without a `PTT` value counts as not keyed: false. A link
failure is `Err(text)`. Private to `control.rs`; used by `set`, `memory` and `scan`, each of which stops with a message that nothing was changed. Blocking: one round trip. The check is made once,
at the start of the tool, not between the steps of a scan. Tests: `tests.rs`. Related: `radioctl_set`, `radioctl_memory`, `radioctl_scan`, `radioctl_tolerate`.

### `radioctl_set_plan` [@ANCHOR: radioctl_set_plan]

`plan(args: &Value) -> Result<Plan, String>` checks every argument of `bench_radioctl_set` and turns them into a `Plan` before anything is sent to the rig; one bad value stops the whole call.
`Plan` is private: `freq: Option<f64>`, `mode: Option<(String, f64)>` (name, passband Hz), `vfo: Option<String>`, `split: Option<(bool, Option<String>)>`, `rit`, `xit: Option<f64>`,
`levels: Vec<(String, f64)>`, `funcs: Vec<(String, bool)>`. An absent or null key means "leave alone".

| Argument | Type, range | Rule |
|---|---|---|
| `frequency_hz` | number, 1 to 1e12 | a non-number or out-of-range value is an error (`<key> must be a number`, `<key> <v> is outside lo to hi`). The receive-range check comes later, in `set`. |
| `mode` | string | case-insensitive name from `MODES` (37 names such as `AM CW USB LSB FM PKTUSB`); stored upper-case. Otherwise `mode "X" is not a rig mode name (...)`. |
| `passband_hz` | number, 0 to 250000 | 0 means the rig's default; needs `mode` in the same call. |
| `vfo` | string | one of `VFOA VFOB VFOC currVFO MEM Main Sub MainA MainB SubA SubB`, case-insensitive, stored in the spelling of the list. |
| `split` | object | `enabled` (bool, required); `tx_vfo` optional, same list as `vfo`. |
| `levels` | object name to number | the name (case-insensitive) must be in `SETTABLE_LEVELS`: AF, RF, SQL, APF, NR, NB, BAL are 0 to 1; IF, PBT_IN, PBT_OUT are -10000 to 10000; CWPITCH 300 to 1000; NOTCHF 0 to 10000; AGC (automatic gain control) 0 to 6; ATT 0 to 60 dB; PREAMP 0 to 30 dB. |
| `functions` | object name to bool | the name must be in `SETTABLE_FUNCS`: NB NB2 NR ANF APF AIP MN LOCK MUTE SQL AFLT ANL FAGC. |
| `rit_hz`, `xit_hz` | number, -9999 to 9999 | |

A level or function in `TRANSMIT_SIDE` (RFPOWER MICGAIN KEYSPD COMP VOXGAIN VOXDELAY ANTIVOX TUNER VOX TONE TSQL MON SBKIN FBKIN BKIN_DLYMS SEND_MORSE SEND_VOICE_MEM) is refused with a message that
says it sets what the transmitter does and that only the transmit tools of an attested rig change transmit settings; this check comes before the "not settable here" check so the reason is the
right one. Validation order: frequency, passband, mode, vfo, split, levels, functions, rit, xit. A call that sets nothing is `nothing to set: give frequency_hz, mode, vfo, split, rit_hz, xit_hz,
levels or functions`. Pure, no I/O. Tests: `tests.rs`. Related: `radioctl_set`, `radioctl_arg_check`, `radioctl_bounded`.

### `radioctl_apply` [@ANCHOR: radioctl_apply]

`apply<T>(done: &[Value], what: &str, r: Result<T, RigError>) -> Result<(), String>` converts the outcome of one `set_*` command into the error text of `set`. `done` is the list of changes already
made in this call (each has a `set` field naming it), `what` names the step (`set frequency`), and `r` the command's result. A success discards the reply and gives `Ok(())`. A failure gives
`<what> failed: <rig error> (already applied before it: a, b)` or `... (already applied before it: nothing)`. The point is honesty: `set` is not atomic, a rig error in the middle leaves the
earlier changes in place, and the message lists exactly which. The read-backs after each change do not go through `apply`; they use `map_err`. Pure apart from formatting. Tests: `tests.rs`.
Related: `radioctl_set`.

### `radioctl_set` [@ANCHOR: radioctl_set]

`set(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String>` is the tool `bench_radioctl_set` (tier `read_config`, instrument lock held by `call`): change the receiver and
read each change back. Arguments are those of `radioctl_set_plan`.

Steps: (1) `plan` checks everything. (2) If a frequency is given, `rx_ranges` and `check_freq` refuse one outside the receive limits. (3) `ptt_is_on`: when the rig reports PTT on, the call is refused with
`Nothing was changed` and the advice to find out who keyed it (`bench_radioctl_safe_state` releases it where the transmit tools exist). (4) The changes, in this fixed order: VFO (`set_vfo`), frequency
(`set_freq`, rounded to whole Hz, then `get_freq`), mode (`set_mode M W`, then `get_mode`; a different mode read back adds a `warning`), split (`set_split_vfo <0|1> <tx_vfo>`; with no `tx_vfo` the
transmit VFO is sent as `VFOB`; read back with `get_split_vfo`), RIT (receiver incremental tuning) and XIT (`set_rit`, `set_xit`, read back), each level (`set_level`, read back with `get_level`) and each function
(`set_func`, read back with `get_func`). Every name sent is first checked by `check_arg` (letters, digits and `_ . + - / :`, at most 64 characters), so an argument cannot carry a second command.

Returns `{"changes": [{"set", "requested", "read_back", ...}]}` in the order applied. A read-back that differs from the request is reported, not an error (except that a differing mode adds a warning). Errors: every
plan error; a frequency outside the limits; PTT on; a rig refusal or link failure at a step (`apply`, naming what was already applied); a read-back that fails. The call is not atomic and is not rolled back.
Blocking: one round trip per command and per read-back, no sleeps. Transmitter: never keyed; no tier higher than `read_config` is needed, the transmit-side names are refused, and no watchdog is involved because
no command here can set PTT or the power. Tests: `tests.rs`. Related: `radioctl_set_plan`, `radioctl_apply`, `radioctl_ptt_is_on`, `radioctl_rx_ranges`, `radioctl_freq_guard`.

### `radioctl_memory` [@ANCHOR: radioctl_memory]

`memory(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String>` is the tool `bench_radioctl_memory` (tier `read_config`): recall memory channel `channel` and report the frequency and mode the rig then shows.
`channel` is required, a whole number from 0 to 99999 (a missing, non-numeric or out-of-range value gives `channel is required: a whole number from 0 to 99999`; a fraction gives `channel must be a whole number`).
PTT on refuses the call (`no channel was selected`). Then `set_vfo MEM` is tried and a rig-side refusal is only noted (some backends recall by channel number alone), then `set_mem <n>` (a refusal is an
error `select channel N: ...`), then the frequency (`get_freq`) and, tolerated, the mode and passband are read. If the frequency lies outside the receive limits the result gets a `warning`
quoting `check_freq`'s text. Returns `{"channel", "frequency_hz", "mode": {"mode","passband_hz"} or null, "note": text or null, "warning"?}`. As the tool description says, the current radioctld serves no memory channels
(RPRT -4), so with it the call ends in the `select channel` error; if `set_vfo MEM` had been accepted just before, the code does not switch the VFO back (unknown whether radioctld accepts it). Never keys; same
safety as `radioctl_set`. Tests: `tests.rs`. Related: `radioctl_ptt_is_on`, `radioctl_rx_ranges`, `radioctl_freq_guard`.

### `radioctl_scan` [@ANCHOR: radioctl_scan]

`scan(c: &mut Client, cfg: &Config, args: &Value) -> Result<Value, String>` is the tool `bench_radioctl_scan` (tier `read_config`): step the receiver across a range, dwell at each frequency, list the signal strength and squelch state,
rank the strongest, and put the rig back. The instrument lock is held for the whole scan.

| Argument | Type, range | Meaning |
|---|---|---|
| `start_hz`, `stop_hz` | number, 1 to 1e12, required | first and last frequency, Hz; `stop_hz` below `start_hz` is an error. |
| `step_hz` | number, 1 to 1e9, required | step in Hz. |
| `dwell_ms` | number, 20 to 5000, default 200 | settle time at each frequency. |
| `top` | integer, 1 to 100, default 10 | how many of the strongest to rank. |
| `stop_when_above_db` | number, -120 to 60, optional | end the scan at the first reading stronger than this many dB relative to S9. |
| `mode` | string, optional | a name from `MODES`; default: the rig's current mode (the scan then changes no mode). |

Limits: the number of steps is `floor((stop - start) / step) + 1` and may not exceed 400; the planned time `steps * (dwell + 60 ms)` may not exceed 120 s. Both refusals say how to change the call. Start and stop are checked against the
receive ranges with `check_freq` (the frequencies between them are not checked, so a range list with a gap is only caught at its ends). PTT on refuses the scan; PTT is read once, before it starts. The rig's frequency (`get_freq`; a failure is an error) and mode
(tolerated) are saved first, and `get_dcd` is probed to see whether the rig reports squelch. `scan_steps` does the stepping. Whatever the outcome, the saved frequency and mode are then sent back (`set_freq`, `set_mode`) and the result says
whether that worked; the VFO is not changed by a scan so it is not restored. If `scan_steps` failed, its error is returned after the restore.

Returns `{"steps": n, "stopped_early": bool, "points": [...], "strongest": [top points, strongest first], "restored": {"frequency": bool, "mode": bool, "frequency_hz": saved}}`; each point is
`{"frequency_hz", "strength_db_rel_s9", "s_units", "squelch_open"?}`. Blocking: sleeps `dwell_ms` per step on the calling thread while holding the lock (up to about two minutes). Never keys. Tests: `tests.rs`.
Related: `radioctl_scan_steps`, `radioctl_freq_guard`, `radioctl_s_units`, `radioctl_bounded`.

### `radioctl_scan_steps` [@ANCHOR: radioctl_scan_steps]

`scan_steps(c, start, step, steps, dwell_ms, mode, has_dcd, stop_above) -> Result<(Vec<Value>, bool), String>` is the loop inside `scan`; the caller has validated everything and saved the rig's state.
Arguments: `start` Hz and `step` Hz (`f64`), `steps` count (`usize`, at most 400 by the caller), `dwell_ms` milliseconds, `mode: Option<&str>` (sent once as `set_mode M 0` before the first step), `has_dcd` (whether
to read the squelch), `stop_above: Option<f64>` dB relative to S9. For step `i` it sends `set_freq <start + step * i>`, sleeps the dwell, reads `get_level STRENGTH` and, with `has_dcd`, `get_dcd`, and adds the point.
At the start of each step, if more than 120 s have passed since the call began (`SCAN_MAX_S`), it returns what it has with `stopped_early = true`; it also stops early, after recording the point, when the reading is
above `stop_above`. Returns `(points, stopped_early)`. Errors are the first command or reading that failed (`set 7010000 Hz: ...`, `signal strength at ... Hz: ...`); the points so far are lost in that case, and the caller
restores the rig. Blocking and sleeping on the calling thread; no lock of its own. Never keys. Tests: `tests.rs`. Related: `radioctl_scan`, `radioctl_s_units`.

## The connection test (`diag.rs`, tier `read_only`)

The tool `bench_radioctl_diagnose` answers "why does my rig not answer". It reads the configuration and the system. It never opens the serial port and never starts the daemon: opening a serial port can change the
DTR (data terminal ready) and RTS (request to send) lines, and some interfaces key the transmitter from them. It does talk to a daemon that is already running (one `get_freq`).

### `radioctl_device_holders` [@ANCHOR: radioctl_device_holders]

`holders(target: &Path) -> Vec<String>` finds the processes that have a device or file open. It resolves `target` with `canonicalize` (a path that does not resolve, such as a missing device, gives an empty list), reads
`/proc`, and for every numeric entry other than this process looks through `/proc/<pid>/fd` for a link to the same real path. Each hit is the text `<pid> <command name>` (from `comm`). Processes whose `fd` directory the user cannot
read (other users' processes, vanished processes) are silently skipped, so a holder owned by another user may not be seen. The argument is borrowed. Cost: one directory listing per process; no locks; read only. Used by
`diagnose` for the "device busy" check (with the daemon's own pid filtered out by the caller). Tests: `tests.rs`. Related: `radioctl_diagnose`.

### `radioctl_lines_note` [@ANCHOR: radioctl_lines_note]

`lines_note(cfg: &Config) -> Value` is the check line about the DTR and RTS control lines. It reads nothing from the system. For a managed rig it says that `radioctld` opens the port 8N1 with both lines inactive and never changes
them (nothing to configure); for an existing radioctld it says that daemon owns the port and its own settings apply. The result is the common check object `{"check": "DTR and RTS lines", "ok": null, "detail": text, "next": ""}`: `ok` is
null because the line is information, not a pass or a fail. Private; called by `diagnose` for managed rigs only. Pure. Tests: `tests.rs`. Related: `radioctl_diagnose`.

### `radioctl_diagnose` [@ANCHOR: radioctl_diagnose]

`diagnose(run: &dyn Run, name: &str, cfg: &Config) -> Result<Value, String>` is the tool `bench_radioctl_diagnose` (tier `read_only`, instrument lock held by `call`). `run` runs a program and gives back success and output
(`audio::Process` in production, a script in tests); `name` is the instrument name; `cfg` the rig's config. It returns `{"rig": name, "checks": [...], "failing": n, "note": ...}` and, as written, never an `Err`.
Each check is `{"check", "ok": true|false|null, "detail", "next"}` where `next` is the advice for a failing check; `failing` counts the checks whose `ok` is false.

The checks, in order: `radioctld program` (runs `<program> --help`; the program is found by `rigd::program`, the search path or `BENCH_MCP_RADIOCTLD`; false with the build advice when it cannot be run);
for a managed rig `rig description` (the file `<rigs_dir>/<rig>.toml` is found and has a name; the rigs directory comes from `rigs_dir` or `RADIOCTL_RIGS`), and, when a `device` is configured, `device exists`
(`metadata`), `device access` (read and write permission by `rigd::check_device`, with the group id and mode), `device busy` (other processes holding it open, found by `holders`; the daemon's own pid is not counted) and the DTR/RTS
note; for an existing rig one `source` line saying the serial port is not this server's to check; then `radioctld running` (a managed daemon that is not running is reported false with `bench_radioctl_daemon start (this opens the
serial port)` as the next step; the tool never starts it) or `the rig answers` (a connection and `get_freq`, with the time it took, false with the cable, power, baud rate and model advice); and, for a managed rig whose log exists, the last six lines of
`<name>.radioctld.log` as one `radioctld log` line. When the device does not exist the busy check still runs and reports that nothing holds it.

Transmitter: nothing here can key; the only rig traffic is one `get_freq` read to a daemon that already runs. Side effects: a short process run (`--help`), reads of `/proc` and of files. Tests: `tests.rs`. Related:
`radioctl_device_holders`, `radioctl_lines_note`, `radioctl_device_check`, `radioctl_daemon_status`, `radioctl_safe_name`.

## Time-station detection (`detect.rs`)

Finding a real signal in the audio of a receiver, used by the receiver test and by the load plausibility check (a dummy load must not hear WWV: the NIST time-signal station). All functions take mono samples `&[f32]` at full scale 1.0 and a sample rate in Hz, and
are pure. A time station (WWV, WWVH) sends a 1 kHz (WWV) or 1.2 kHz (WWVH) tick every second and, at set minutes, 440, 500 and 600 Hz tones; the module does not rely on the schedule. Mains hum (60 or 50 Hz and its harmonics) is
recognised as a comb of peaks and is not a signal; the harmonics that coincide with a station tone are left out of the tone check. The constants `TIME_TONES_HZ` = 440, 500, 600, 1000, 1200 and `TICK_HZ` = 1000, 1200 are shared by all of them. Levels are in dBFS
(`audio::analysis::db`: 20 log10 of the amplitude, -200 for silence).

### `radioctl_detect_hum` [@ANCHOR: radioctl_detect_hum]

`mains_comb(sp: &Spectrum) -> Option<f64>` says whether the spectrum shows mains hum. It takes the peaks between 40 and 1500 Hz (up to 30, at least 10 dB over the median of that range) and counts, for 60 Hz and for 50 Hz,
how many of the first ten harmonics have a peak within 3 Hz. When either count is at least three, the answer is `Some(60.0)` if the 60 Hz count is at least the 50 Hz count (a tie is 60), else `Some(50.0)`; otherwise `None`. `sp` is the Welch amplitude spectrum
(`audio::analysis::spectrum`, `bin_hz` and linear amplitudes per bin), borrowed. No errors. Called by `analyze`; its answer is passed to the tone and beat checks so hum is not mistaken for a signal. Tests: `detect.rs` (hum with and without a tone).
Related: `radioctl_detect_analyze`, `radioctl_detect_tones`.

### `radioctl_detect_tones` [@ANCHOR: radioctl_detect_tones]

`tone_hits(sp: &Spectrum, mains: Option<f64>) -> Vec<ToneHit>` (private) finds narrow steady peaks at the five station tones. A tone within 6 Hz of a mains harmonic (`mains` from `mains_comb`) is skipped. For the others it measures the peak level within two bins of the tone,
the floor (the median level of the bins 40 to 300 Hz away from it, leaving out every station tone's neighbourhood) and the skirt (the larger level 35 Hz above and below). It reports a hit when the peak is above -85 dBFS (so a silent input's numerical noise does not count), at least
15 dB over the floor and at least 10 dB over the skirt. `ToneHit { hz, level_dbfs, over_floor_db }`; results are in the order of the constant. No errors; a tone with no floor bins is skipped. Tests: `detect.rs` (tones in noise, none in silence). Related:
`radioctl_detect_hum`, `radioctl_detect_analyze`.

### `radioctl_detect_ticks` [@ANCHOR: radioctl_detect_ticks]

`tick_train(s: &[f32], rate: u32, hz: f64) -> Option<TickHit>` looks for the once-per-second tick of a time station at `hz`. It measures the amplitude at `hz` (`tone_amp`, 10 ms Hann windows every 2.5 ms), folds the resulting envelope on one second (400 slots), and takes the
loudest slot as the tick phase. It needs at least 6 whole seconds of audio and returns `None` below that, when the loudest slot is below -85 dBFS, when the peak is less than 8 dB over the median slot, or when fewer than 60 % of the seconds have their own loudest slot within 3 slots (7.5 ms) of the folded
one. A hit is `TickHit { hz, phase_s (where in the second the tick falls), contrast_db (peak over median), consistent (fraction of seconds that agree, 0 to 1) }`. This shows a station well below the level at which the S-meter moves. Cost: about 400 window analyses per second of audio, so 20 s of audio costs
8000; run on the calling thread. No errors. Tests: `detect.rs` (clean and noisy tick trains). Related: `radioctl_detect_analyze`, `bench_mcp_audio_analysis_tone_amp`.

### `radioctl_detect_beats` [@ANCHOR: radioctl_detect_beats]

`beat_notes(sp: &Spectrum, mains: Option<f64>) -> Vec<f64>` (private) lists steady narrow tones that are neither hum nor station tones: the beat note of a carrier heard in a single-sideband mode, or a receiver birdie. It takes the peaks between 150 Hz and 3.5 kHz (up to 8, at least 25 dB over the median) and
keeps those above -85 dBFS, not within 6 Hz of a mains harmonic, more than 12 Hz from every station tone, and at least 10 dB above the level 40 Hz away on both sides. The result is the frequencies in Hz, loudest first, possibly empty. No errors. Tests: `detect.rs` (a carrier beat, silence). Related:
`radioctl_detect_analyze`, `radioctl_detect_hum`.

### `radioctl_detect_analyze` [@ANCHOR: radioctl_detect_analyze]

`analyze(s: &[f32], rate: u32) -> Result<Findings, String>` runs all the detectors on a block of receiver audio. `s` is mono samples at full scale 1.0, `rate` the sample rate in Hz (both borrowed or copied; nothing is kept). `Findings` is returned:

| Field | Type | Meaning |
|---|---|---|
| `all_zero` | bool | every sample is exactly 0.0 (a dead input). |
| `rms_dbfs` | f64 | RMS level of the block in dBFS. |
| `tones` | `Vec<ToneHit>` | station tones found (`radioctl_detect_tones`). |
| `ticks` | `Vec<TickHit>` | tick trains found at 1000 and 1200 Hz (`radioctl_detect_ticks`). |
| `beats_hz` | `Vec<f64>` | steady narrow peaks that are not hum or station tones. |
| `mains_hum_hz` | `Option<f64>` | 60 or 50 when hum is recognised. |

`Findings::time_station()` is true when any tone or tick was found; `Findings::json()` gives the report form (levels rounded to 0.1 dB, `time_station_heard`, `beat_notes_hz` rounded to whole Hz). When the block is all zeros or shorter than 8192 samples the detectors are not run and
only `all_zero` and `rms_dbfs` are filled. Otherwise a Welch spectrum with an 8192-point FFT (fast Fourier transform) is computed. The `Err` is the spectrum's, which can only be an invalid FFT length, a constant here; whether other inputs (such as a rate of 0) can fail is unknown. Callers: the receiver test and the load check. Pure and
CPU only, a fraction of a second for ten seconds of audio. Tests: `detect.rs`. Related: `radioctl_detect_hum`, `radioctl_detect_tones`, `radioctl_detect_ticks`, `radioctl_detect_beats`.

## The transmit watchdog (`guard.rs`)

A separate process that un-keys the rig when the transmit tool cannot. Before a transmit tool keys the rig it starts `bench_mcp --radioctl-guard <host> <port> <deadline_ms> <min_power|none>` (this same program run again; `main.rs` hands the process to `guard::main` when its first argument is
`--radioctl-guard`) and waits until the guard says it is armed. The guard has its own connection to the rig's radioctld and the tool's end of a pipe on its standard input. It un-keys the rig and then sets the power to the minimum when any of these happens: the deadline passes; the pipe
closes without the word `done` (the server crashed, was killed or panicked); or the tool sends `abort`. The tool sends `done` only after it has un-keyed the rig and read the PTT state back. The guard is in its own process group, so a Ctrl-C aimed at the server does not take it down first.
Not covered: the guard being killed together with the server, radioctld or the computer dying, and a rig that ignores `set_ptt 0`; the next start of the module checks for a stuck PTT (`radioctl_open`). The watchdog is only a safety action: it has no tier, no band list and no transmit gate of its own because it can
only un-key and lower power; the tier, the lock, the owner's frequency, power and time limits, the load-plausibility gate and the confirmation are applied by the transmit tools before they spawn it, and the keyed interval's own `deadline` is its planned seconds plus `MARGIN` (3 s), with the planned seconds limited to 0.1 to 60.

### `radioctl_guard_unkey` [@ANCHOR: radioctl_guard_unkey]

`unkey(host: &str, port: u16, min_power: Option<f64>) -> Result<bool, String>` makes a rig stop transmitting and proves it. `host` and `port` are the rig's radioctld (borrowed; a fresh connection is opened, with a 2 s reply limit, so it works even when the connection of the tool is dead); `min_power` is the RFPOWER level to set afterwards
as a fraction 0 to 1 of the rig's scale, or `None` to leave the power alone. Up to 8 attempts, with a 250 ms pause between them: connect, `set_ptt 0`, if `min_power` is given `set_level RFPOWER <p>` (best effort, its failure is ignored because the PTT is what matters), then `get_ptt`. Results: `Ok(true)` when PTT reads back `0`; `Ok(false)` when the rig cannot report PTT
(a refusal such as "not available") after `set_ptt 0` was accepted, which is all there is to go on; `Err("could not un-key the rig after 8 attempts (<last error>)")` otherwise. Note the order: PTT off first, then the power. The write is not retried within one connection, but each attempt is a new one. Blocking: up to about 8 times (2 s + 250 ms) in the worst case. It takes
no instrument lock on purpose: it is called by the watchdog, by `tx::Keyed::release` as a fallback, by `release_prefix` and `release_rig` (server stop and daemon stop), by the first-contact check, and by `bench_radioctl_safe_state`, all of which must work while a keyed interval holds the lock. It never keys. Tests: `tests.rs`. Related: `radioctl_guard_run`, `radioctl_tx_release`,
`radioctl_tx_release_all`, `radioctl_tx_startup_check`.

### `radioctl_guard_run` [@ANCHOR: radioctl_guard_run]

`run(host, port, deadline, min_power, words, armed, report) -> i32` is the watchdog's loop. `host: &str`, `port: u16` the radioctld; `deadline: Duration` from now until it acts alone; `min_power: Option<f64>` as for `unkey`; `words: &mpsc::Receiver<Word>` the lines from the tool's end of the pipe
(`Word::Line(text)` or `Word::Closed`); `armed: &mut dyn Write` where it writes `armed` and flushes when ready; `report: &mut dyn Write` for its messages (in the real process the standard error, which goes to the watchdog log).

Steps: the deadline clock starts first; the guard proves the way to the rig works with one `get_freq` (a read) before saying it is ready, because a guard that cannot reach the rig must not let the tool key; on failure it writes `guard: cannot reach radioctld at host:port: ...` and returns 3 without writing `armed`. Then it writes `armed` and waits for a word until the
deadline. `done` returns 0 at once and touches nothing; `abort` un-keys; any other line is ignored; a closed pipe or a dropped sender un-keys (this is the crash case); the deadline passing un-keys. Un-keying is `unkey` with the same minimum power; it writes `guard: rig un-keyed` and returns 0, or `guard: FAILED: ... The rig may be transmitting: switch it off at the rig now.` and returns 2.
Exit codes: 0 done or un-keyed (including a rig that could not report PTT but accepted `set_ptt 0`), 2 could not un-key, 3 could not reach radioctld. The function blocks until one of these; in production it is the main thread of the guard process, in the tests a thread. Tests: `tests.rs` (in a thread), `tests/radioctl_guard.rs` (the real process). Related: `radioctl_guard_main`,
`radioctl_guard_unkey`, `radioctl_guard_spawn`, `radioctl_guard_stand_down`.

### `radioctl_guard_main` [@ANCHOR: radioctl_guard_main]

`main(args: &[String]) -> i32` is the process entry of the watchdog: the command line after the flag, `<host> <port> <deadline_ms> <min_power|none>`. The caller (`main.rs`) passes the exit code to `std::process::exit`. Arguments: `host` text; `port` a number; `deadline_ms` an integer from 500 to 120000; `min_power` a number from 0 to 1 or
the word `none`. Any other count or value prints `guard: <reason>` to standard error and returns 64 (including `usage: --radioctl-guard <host> <port> <deadline_ms> <min_power|none>`). Otherwise it starts a thread that reads standard input line by line and sends each line (and, at the end of input or a read error, `Word::Closed`) into a channel, and calls `run` with the real standard output (for `armed`)
and standard error (for the report). The guard has no config and no tier: everything it knows comes from the command line the tool built. The program is `bench_mcp` itself, so it never runs a tool and never touches the MCP server. Tests: `tests.rs`, `tests/radioctl_guard.rs` (the real process against a virtual rig). Related: `radioctl_guard_run`, `radioctl_guard_spawn`.

### `radioctl_guard_spawn` [@ANCHOR: radioctl_guard_spawn]

`ProcessSpawner::spawn(host: &str, port: u16, deadline: Duration, min_power: Option<f64>, log: &Path) -> Result<Box<dyn Watch>, String>` (the production implementation of the `Spawner` trait) starts the watchdog process and returns the handle the tool keeps while the rig is keyed. It starts the current executable (`current_exe`) with the guard flag and the four arguments (the deadline in
milliseconds, the minimum power as a number or `none`), in its own process group, with a pipe on standard input and on standard output and standard error appended to the file `log` (created if absent; the managed directory's watchdog log). It waits up to 5 seconds for the line `armed` on the guard's standard output. If the line does not come, the child is killed and waited for and the result is `the transmit watchdog did
not report armed (...); not keying. Next: bench_radioctl_diagnose, and check the watchdog log in the managed directory.` Other errors: the executable cannot be found, the log cannot be opened, the process cannot be started, no output pipe. Success is a `ProcessWatch { child, stdin }` (`Watch`).

Transmitter: this is the arming step. `tx::key` calls it before the first `set_ptt 1`, with `deadline = planned seconds + MARGIN`, and returns without keying when it fails, so a rig is never keyed without a watchdog. It is called with the instrument lock held, after the tier, the attestation, the owner's frequency, mode, power and time limits, the load check gate and the confirmation have been checked by the transmit tools. The tests of the `tx` module use a thread spawner
that runs the same `run`; `tests/radioctl_guard.rs` runs the real process. Related: `radioctl_guard_run`, `radioctl_guard_stand_down`, `radioctl_tx_key`.

### `radioctl_guard_stand_down` [@ANCHOR: radioctl_guard_stand_down]

`ProcessWatch::stand_down(self: Box<Self>) -> Result<(), String>` (the `Watch` trait's method) tells the watchdog the tool has finished safely. It writes `done` and a newline to the guard's standard input and then closes the pipe, and for up to 3 seconds polls (every 20 ms) for the child to exit. An exit of any status is `Ok(())` (the status is not examined; the guard returns 0 for `done`). If the process is still there after 3 seconds it is killed and waited for and the result is
`Err("the watchdog did not exit after done; it was killed")`. The method consumes the handle. It is called by `tx::Keyed::finish` after the release has been read back (`set_ptt 0` then `get_ptt` showing `0`, or the rig cannot report PTT), and the caller ignores the `Err` because the rig is already safe at that point. The sibling `leave_to_guard`, called when the release could not be confirmed, closes the pipe without `done` so the guard un-keys the rig itself, and waits for the
child in a background thread. The test comment of `tests/radioctl_guard.rs` notes that radioctld un-keys a rig when the client that keyed it disconnects. Tests: `tests/radioctl_e2e.rs`. Related: `radioctl_guard_run`, `radioctl_guard_spawn`, `radioctl_tx_keyed_released_flag`.
