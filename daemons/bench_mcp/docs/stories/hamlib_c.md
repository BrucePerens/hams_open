# The radio rigs: the managed daemon, the receiver check, the reference-station table and the status tools

As the owner of a bench with a radio on it, I want the server to start and watch the one program that talks to my rig, to read everything the
receiver reports in one call, to test the receiver with its own audio, and to keep an honest list of reference stations that the dummy-load
check listens to, so that an AI assistant can look at the rig without ever being able to key it, and so that what the rig does not report is
said, not guessed. This story covers four files of the `radioctl` driver of `daemons/bench_mcp`: `src/radioctl/rigd.rs` (the managed
`radioctld`), `rxcheck.rs` (the receiver test with audio), `stations.rs` (the reference-station table) and `status.rs` (the read-only tools and
the helpers that every tool shares). The driver was called `hamlib` until pull request #1568 replaced Hamlib's `rigctld` by `radioctld` (the
program of `daemons/radioctl`, which speaks the same line protocol over a TCP socket), so the story file keeps the old name (`hamlib_c`) and
every anchor id starts with `radioctl_`. The users are the MCP (Model Context Protocol) tool dispatcher (`radioctl::call` in `mod.rs`), the other files of the driver
(`control.rs`, `watch.rs`, `diag.rs`, `tx.rs`, `txtools.rs`, `load.rs`, `loadtools.rs`) and the people who extend them. The front door, the
receiver control and the watchdog are in `hamlib_a.md`, the transmit path in `hamlib_b.md`. The tool list and the safety model are in
`docs/RADIOCTL_TOOLS.md`.

The words used below: a *rig* is one configured instrument of kind `radioctl`; *radioctld* is the daemon that holds the serial port (or a
virtual rig) and answers text commands such as `get_freq` or `set_ptt 1`; a *tier* is one of `read_only` < `read_config` < `full`
(`config::Tier`), set per instrument and required per tool; the *instrument lock* is the per-rig mutex `instrument_lock(name)` that
`radioctl::call` holds while a tool runs (every tool but the safe state, the rig list and the model list), so two tools never talk to one rig at
once. Nothing in these four files keys the transmitter. The one place where this code touches the transmit side at all is the managed daemon:
`rigd::config_text` gives the daemon its own transmit gate only for a rig that has the owner's dummy-load attestation (see `radioctl_daemon_args`).

## The managed daemon (`rigd.rs`)

For a rig configured with `rig` (a *managed* rig) this server starts one `radioctld`, bound to 127.0.0.1 on a free port, keeps its child handle,
restarts it on demand within a limit and stops it when the server stops. A small state file per rig records who owns the daemon, so that a
daemon left over by a killed server is found and stopped by the next one. All state is process-wide: a `Mutex<BTreeMap<rig name, Slot>>` (`STATE`,
child handle, port, start times, last exit text) and a `Mutex<BTreeSet>` (`REAPED`, rigs whose leftover has been looked for). Both locks recover from
poisoning. The constants are `MAX_STARTS = 4` starts per rig within `WINDOW` = 600 seconds without the owner asking again, and `READY_WAIT` = 10
seconds for a new daemon to accept connections. A rig configured with `radioctld_port` (an *existing* rig) is never started, stopped or signalled.

### `radioctl_managed_dir` [@ANCHOR: radioctl_managed_dir]

`dir(cfg: &Config) -> PathBuf` returns the directory where everything this driver keeps for a rig lives: the daemon's state file
(`<name>.radioctld.json`), its generated configuration (`<name>.radioctld.toml`), its log (`<name>.radioctld.log`), the keyed-interval state and log of the
transmit tools, the load-check baselines and the station table `stations.json`. It is called by every function of the driver that touches one of
those files (`rigd` itself, `diag`, `loadtools`, `txtools`, `mod.rs`).

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` (borrowed) | The rig's parsed `radioctl` settings. Only `managed_dir` is read. |

Return: `cfg.managed_dir` when the owner set it (an absolute path without `..`, checked by `parse_config`), else a directory named `radioctl` under
the bench output directory (`crate::out_dir()`, which is `BENCH_MCP_OUT` or `bench_mcp` under the system temporary directory). The directory is
created with `create_dir_all` on every call; a failure to create it is ignored, so the first file operation that follows reports the real error with
its path. It cannot fail and takes no lock. Because the default is a temporary directory, the station table, the baselines and the keyed-interval
state are lost when the system clears it unless the owner sets `managed_dir` (or `BENCH_MCP_OUT`). Tests: `tests.rs` (cited as `radioctl_managed_dir`).
Related: `radioctl_rigs_dir` (a different directory: the rig descriptions), `radioctl_safe_name` (file names), `radioctl_stations_load`.

### `radioctl_rigs_dir` [@ANCHOR: radioctl_rigs_dir]

`rigs_dir(m: &Managed) -> Result<PathBuf, String>` names the directory that holds the radioctl rig description files (`<rig>.toml`, the `rigs/`
directory of `daemons/radioctl`). It is called when a daemon is started (`radioctl_daemon_start`), by the model list (`bench_radioctl_models`, `mod.rs`)
and by the connection test (`diag.rs`).

| Argument | Type | Meaning |
|---|---|---|
| `m` | `&Managed` (borrowed) | The managed-rig settings. Only `rigs_dir` is read. |

Return: `m.rigs_dir` (the config key `rigs_dir`, an absolute path) when set; otherwise the environment variable `RADIOCTL_RIGS`, which is radioctld's
own variable, when it is set and not empty. The path is not checked for existence here. Error: when neither is set, the text `no rig description
directory: set rigs_dir in the rig's config entry or RADIOCTL_RIGS ... Next: bench_radioctl_models lists the rigs once it is set.` No locks, no files
and no network; it reads the process environment. Tests: `tests.rs` (cited as `radioctl_rigs_dir`). Related: `radioctl_daemon_start`, `radioctl_config`.

### `radioctl_daemon_args` [@ANCHOR: radioctl_daemon_args]

`config_text(name: &str, m: &Managed, port: u16, rigs: &Path, transmit: bool) -> String` builds the text of the TOML file that `radioctld --config`
reads for one managed rig. It is pure (it writes nothing; `radioctl_daemon_start` writes the file) and nothing in it comes from a tool argument: `name`,
`rig`, `device`, `speed` and `full_power_w` are the owner's config values, already validated by `parse_config`.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` | The instrument name; written through `safe_name` as the rig's `name`. |
| `m` | `&Managed` | `rig` (description id), `device` and `speed` (both present: a serial rig; otherwise a virtual rig made from the description), `full_power_w`. |
| `port` | `u16` | The localhost port the daemon listens on. |
| `rigs` | `&Path` | The rig description directory; the description is `<rigs>/<rig>.toml`, with `\` and `"` escaped for TOML. |
| `transmit` | `bool` | The daemon's own transmit gate. |

The text has a `[server]` table with `cache_max_age_ms = 0` (a read is never served from the daemon's cache, because scans and the load check need
what the rig says now), one `[[rig]]` with `listen = "127.0.0.1:<port>"` (the address is fixed in this function; there is no option to change it, so
the daemon is reachable only from this machine), `transmit = <bool>`, and `tx_power_mw = <full_power_w * 1000>` when the owner gave `full_power_w`
(the full-scale RF (radio frequency) output the transmit tools use to turn watts into a power setting), and a `[rig.transport]` table: `kind = "serial"` with `path` and `baud`, or
`kind = "virtual"`. **Transmit gate**: the caller `radioctl_daemon_ensure` passes `transmit = cfg.transmit.is_some()`, so the daemon's gate is open
only for a rig whose bench config carries the owner's dummy-load attestation (`transmit` block). The tier, the band and power checks, the load check and
the keyed-interval watchdog are done by this driver's tools before any key command (documented in `hamlib_b.md`), not by this function; what the
daemon itself does when its gate is open is defined in `daemons/radioctl` and is not described here (unknown from this file). The device path is
not checked here (`radioctl_device_check` does that before the start). Tests: `tests.rs` (`config_text` is called with a serial rig, a virtual rig with
`transmit` true and a rigs path that contains a quote; cited as `radioctl_daemon_args`).

### `radioctl_reap_stale` [@ANCHOR: radioctl_reap_stale]

`reap_stale(name: &str, dir: &Path) -> Result<Option<u16>, String>` stops the leftover `radioctld` of a server that died without stopping it. It
reads the state file `<dir>/<name>.radioctld.json` (`{"owner_pid", "radioctld_pid", "port"}`, written by `radioctl_daemon_start`). It is called once per
rig and server run by `radioctl_daemon_ensure` before the first start, and directly by tests.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` | The instrument name (through `safe_name`, the state file's name). |
| `dir` | `&Path` | The managed directory. |

Return: `Ok(None)` when there is no readable state file (nothing to do); otherwise `Ok(Some(port))` (the port from the file, if it had one) after the
leftover has been dealt with and the state file removed. A file that is not JSON is treated as empty (pids 0): it is removed and nothing is signalled.
Error: `rig "<n>" is managed by another bench_mcp process (pid N); two servers must not share a rig. Next: use that server, or stop it.` when
the owner pid is not this process, is alive (signal 0) and its `/proc/<pid>/cmdline` contains `bench_mcp`: a live owner is never touched. Otherwise,
if the daemon pid is above 1, alive and its command line contains `radioctld`, the function first calls `tx::release_after_crash(name, port)` (the
rig may have been left keyed; this unkeys it through the watchdog's `unkey`, ignoring the outcome), then sends SIGTERM, polls every 50 ms for up
to 3 seconds, and sends SIGKILL if it is still alive. The command-line check is what keeps the function from signalling an unrelated process that
reused the pid; it only ever signals a process that matches `radioctld`. Side effects: the state file is deleted; it blocks up to about 3 seconds.
Tests: `tests.rs` (cited as `radioctl_reap_stale`: a dead owner's daemon is stopped, a live owner is refused). Related:
`radioctl_daemon_ensure`, `radioctl_release_after_crash` (`tx.rs`), `radioctl_daemon_start` (writes the file).

### `radioctl_log_tail` [@ANCHOR: radioctl_log_tail]

`log_tail(path: &Path) -> String` returns the last eight lines of a text file, joined with ` | `, for an error message. It is private to `rigd.rs`,
used by `radioctl_daemon_start` to say why a new `radioctld` exited or never accepted a connection (the daemon's stdout and stderr go to the log
file `<name>.radioctld.log`). The argument is borrowed. A file that cannot be read gives an empty string, never an error. The whole file is read
into memory (the log is truncated at each start attempt, so it is small). No locks. Tests: `tests.rs` (cited as `radioctl_log_tail`).
Related: `bench_radioctl_diagnose` (`diag.rs`) reads the same log file for the connection test.

### `radioctl_device_check` [@ANCHOR: radioctl_device_check]

`check_device(path: &str) -> Result<(), String>` tells the owner that the rig's serial device is missing or not usable by this user, before a
daemon is started. The reason it exists: `radioctld` keeps running after it fails to open the rig, which from outside looks like a rig that does not
answer. It is called by `radioctl_daemon_start` for a managed rig whose device starts with `/dev/`, and by the connection test (`diag.rs`).

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | The device path from the config (already validated by `parse_config`). |

Return `Ok(())` when `metadata(path)` succeeds and `access(path, R_OK | W_OK)` says this user may read and write it. Errors, each ending with a
`Next:` step: the path cannot be examined (`the rig's device <path> cannot be used (<os error>)`, which suggests checking the cable and the path, `bench_usb_find`
and stable `/dev/serial/by-id` names); the path has a NUL byte; or this user may not read and write it (the text names the file kind, group id and
mode, and says to add the user to that group, `dialout` on Debian). The function only reads metadata and calls `access(2)`; it opens nothing, so it does
not disturb a rig that another program holds, and it never changes permissions. No locks. Tests: `tests.rs` (cited as `radioctl_device_check`:
`/dev/null` passes, a missing path and `/dev/mem` fail). Related: `radioctl_daemon_start`, `radioctl_diagnose`.

### `radioctl_daemon_start` [@ANCHOR: radioctl_daemon_start]

`spawn(name: &str, m: &Managed, dir: &Path, transmit: bool) -> Result<(Child, u16), String>` (private) starts one `radioctld` for a managed rig and
returns the child and its port once the daemon accepts connections. It is called only by `radioctl_daemon_ensure`, which holds the process-wide `STATE`
lock for the whole call.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` | The instrument name. |
| `m` | `&Managed` | The managed settings: `rig`, `device`, `rigs_dir`, `port` (the config's `listen_port`). |
| `dir` | `&Path` | The managed directory (`radioctl_managed_dir`). |
| `transmit` | `bool` | Passed to `config_text`: the daemon's transmit gate (open only for a rig with the owner's attestation). |

Steps. (1) A `/dev/` device goes through `check_device`. (2) The rig description `<rigs>/<rig>.toml` must be a file, else the error
`no rig description <path> for rig "<id>". Next: bench_radioctl_models lists the rigs; check rig and rigs_dir.` (3) Up to three attempts (one if the owner
fixed `listen_port`): choose the port (the fixed one, or a free one found by binding `127.0.0.1:0` and dropping the listener), create (truncate)
the log file, write `<name>.radioctld.toml` from `config_text`, and run `radioctld --config <file>` with stdin null and stdout and stderr to the log. The
program is `BENCH_MCP_RADIOCTLD` or `radioctld` from the search path (`program()`). (4) Wait up to `READY_WAIT` (10 s), polling every 50 ms: if the child
has exited, the attempt failed; if a TCP connect to `127.0.0.1:port` succeeds (200 ms timeout), wait 250 ms and check the child again (if another program
had taken the port, ours fails to bind and exits), then write the state file `{"owner_pid": <this pid>, "radioctld_pid": <child pid>, "port": <port>}` and
return. On timeout the child is killed and reaped. Errors: a missing program (`radioctld is not installed. Next: build it from daemons/radioctl ... or set
BENCH_MCP_RADIOCTLD`), any other spawn failure, file errors with their paths, and after the last attempt `<reason>; its log says: <log tail>. Next: run
bench_radioctl_diagnose (permissions on the device, a busy port, the model number and baud rate).` The function blocks for up to about 30 seconds
(three attempts of 10 s) in the worst case. The child is owned by this process (a `std::process::Child`; nothing detaches it). A serial device is opened
by the daemon, not here. Tests: `tests.rs` (cited as `radioctl_daemon_start`: managed daemons are started on a free localhost port against virtual
rigs). Related: `radioctl_daemon_args`, `radioctl_log_tail`, `radioctl_reap_stale`.

### `radioctl_daemon_ensure` [@ANCHOR: radioctl_daemon_ensure]

`ensure(name: &str, m: &Managed, cfg: &Config) -> Result<u16, String>` (private) returns the port of the rig's running daemon, starting it if needed,
within the restart limit. It is the single place that decides to start a daemon: `radioctl_endpoint` (every tool call that opens the rig) and
`radioctl_daemon_explicit_start` call it.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` | The instrument name; the key of the slot. |
| `m` | `&Managed` | The managed settings. |
| `cfg` | `&Config` | The rig's settings; `cfg.transmit.is_some()` decides the daemon's transmit gate and `managed_dir` the directory. |

Behaviour: the first time a rig name is seen by this server run (`REAPED`), `reap_stale` runs and its error, if any, is returned. Then, under the `STATE` lock,
if the slot has a child that is still running its port is returned; if the child has exited, `last_exit` records `exited with <status>` and the child is
dropped. Starts older than `WINDOW` are forgotten; if `MAX_STARTS` (4) remain, the error is `radioctld for "<n>" has been started 4 times in 10 minutes (last
exit: ...); not restarting it again by itself. Next: bench_radioctl_diagnose to find the cause, then bench_radioctl_daemon with action start.` Otherwise `spawn`
runs, and on success the start time, child and port are stored. Locking: the `STATE` mutex is held while the daemon starts, so a start of one rig (up to
seconds) blocks `status`, `stop` and `ensure` for every other rig of this server; the per-instrument lock of `call` already serialises the tools of one rig. Tests:
`tests.rs` (cited as `radioctl_daemon_ensure`: start on demand, stop, start again; the restart limit has no test of its own, unknown whether another test reaches it). Related: `radioctl_endpoint`.

### `radioctl_endpoint` [@ANCHOR: radioctl_endpoint]

`endpoint(name: &str, cfg: &Config) -> Result<(String, u16), String>` answers where the commands for this rig go. It is called by `radioctl_open` (every tool
that talks to a rig), by the transmit tools and by the safe state. For an *existing* rig (`Source::Existing`) it returns the configured host and port and
starts nothing. For a *managed* rig it calls `radioctl_daemon_ensure` and returns `("127.0.0.1", port)`, so using a managed rig starts its daemon on first
use. Errors are those of `ensure` (leftover owned by a live server, no description, device not usable, restart limit, startup failure). Arguments are
borrowed; the host is returned as an owned string. Tests: `tests.rs` (cited as `radioctl_endpoint`). Related: `radioctl_open`, `radioctl_daemon_status` (reports without starting).

### `radioctl_daemon_explicit_start` [@ANCHOR: radioctl_daemon_explicit_start]

`start(name: &str, cfg: &Config) -> Result<Value, String>` is the `start` action of `bench_radioctl_daemon` (tier `read_config`; also the second half of `restart`).
It clears the rig's list of start times, so the owner's explicit request begins a new restart window, then calls `radioctl_daemon_ensure` and returns the
`radioctl_daemon_status` object with an added `port` number. Error: `"<n>" uses an existing radioctld; this server does not start or stop it` for an existing
rig, plus the errors of `ensure`. A daemon that is already running is not restarted (its port is returned). The transmit gate of the daemon is the same as for an on-demand
start (`cfg.transmit.is_some()`); the tool does not key anything. The call runs under the instrument lock taken by `call`. Tests: `tests.rs`
(cited as `radioctl_daemon_explicit_start`). Related: `radioctl_daemon_stop`, `radioctl_daemon_tool` (`mod.rs`).

### `radioctl_daemon_stop` [@ANCHOR: radioctl_daemon_stop]

`stop(name: &str, cfg: &Config) -> Result<Value, String>` stops the managed daemon of a rig and removes its state file; it is never refused, and stopping a
daemon that is not running is not an error. It is called by the `stop` and `restart` actions of `bench_radioctl_daemon`, after the tool has asked
`tx::release_rig` to release a rig that may be keyed (this function does not do that itself: stopping the daemon of a keyed rig would leave it keyed, which is why
the tool unkeys first and refuses to stop the daemon when that fails). Result: `{"radioctld": "stopped", "was_running": bool}`; error for an existing rig
as in `start`. It calls `radioctl_kill_slot` (SIGTERM, up to 2 s, then kill) and deletes `<name>.radioctld.json`. Tests: `tests.rs`
(cited as `radioctl_daemon_stop`). Related: `radioctl_shutdown`, `radioctl_tx_release_rig`.

### `radioctl_kill_slot` [@ANCHOR: radioctl_kill_slot]

`kill_slot(name: &str) -> bool` ends the child process of one rig's slot and returns whether it was running. The child handle is taken out of the slot
(the slot, with its start history and last exit text, stays). If the child is running, it gets SIGTERM, then up to 2 seconds (polling every 50 ms) to
exit, then `Child::kill` (SIGKILL), and it is always reaped with `wait`, so no zombie is left. The signal goes only to the pid of our own live child, never to a
pid read from a file. It returns false when the rig has no slot, no child, or the child had already exited. Called by `radioctl_daemon_stop`,
`radioctl_shutdown` and by tests. It takes the `STATE` lock and blocks up to about 2 seconds with it held. It does not remove the state file. Tests:
`tests.rs` (cited as `radioctl_kill_slot`).

### `radioctl_shutdown` [@ANCHOR: radioctl_shutdown]

`shutdown_prefix(prefix: &str)` stops the managed daemons of every rig whose name starts with `prefix`; an empty prefix stops all of them. It collects the
matching names under the `STATE` lock, releases the lock, and calls `radioctl_kill_slot` for each, one after the other. It is called by `radioctl::shutdown_prefix` and
`shutdown_all` (`mod.rs`, when the server stops) and by tests that use a name prefix to stop only their own
rigs (so one test cannot stop another's daemons). It returns nothing and cannot fail. The state files are not removed (the next server run finds them
with a dead owner and removes them in `radioctl_reap_stale`). Tests: `tests.rs` (cited as `radioctl_shutdown`). Related: `radioctl_shutdown_all`.

### `radioctl_daemon_status` [@ANCHOR: radioctl_daemon_status]

`status(name: &str, cfg: &Config) -> Value` says what this server knows about a rig's daemon, without starting it and without contacting it. It is the
`status` action of `bench_radioctl_daemon`, an input of `bench_radioctl_rigs`, and of the connection test. For an existing rig: `{"source": "existing radioctld",
"host", "port", "managed": false}`. For a managed rig: `{"source": "managed radioctld", "managed": true, "rig": <description id>, "listens_on": "127.0.0.1"}`
plus, when the rig has a slot, `running` (the child is alive, checked with `try_wait`), `port` and `pid` (only when running), `starts_in_last_10_min`
(starts within `WINDOW`) and `last_exit` (text or null); a rig that has never been started has just `running: false`. It takes the `STATE` lock briefly and
changes nothing except that `try_wait` reaps an exited child. A running daemon is not proof that the rig answers; `bench_radioctl_rigs` and the connection test read
the rig. Tests: `tests.rs` (cited as `radioctl_daemon_status`).

## The receiver test with audio (`rxcheck.rs`)

`bench_radioctl_rx_check` (tier `read_config`) tunes the receiver, reads the S-meter and the squelch, records a few seconds of the rig's audio from
the sound input named in the rig's `audio` setting, and reports the signal strength against the audio level, the audio noise floor, the strongest
steady tone and whether a time station is in the audio. It never transmits: it sends no `set_ptt`, `send_morse` or any transmit-side command; it only
changes the frequency, the mode and the AF level and puts them back. It runs under the instrument lock, so it holds the rig for the length of the recording.

### `radioctl_rx_check` [@ANCHOR: radioctl_rx_check]

`rx_check(c: &mut Client, cfg: &Config, audio: Option<&dyn AudioSource>, args: &Value) -> Result<Value, String>` is the tool. `call` (`mod.rs`) resolves the
audio source with `audio_in::resolve` (None when the rig has no `audio` setting) and passes it with an open client.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | An open radioctld connection; the check uses reads (retried once) and writes (never retried). |
| `cfg` | `&Config` | The rig's settings; `rx_ranges_hz` and the rig's own ranges limit the frequency. |
| `audio` | `Option<&dyn AudioSource>` | The receiver's audio input (`capture(seconds)`, `rate()`, `describe()`); None: S-meter and squelch only. |
| `args.frequency_hz` | number, Hz | Required, 1 to 1e12 (then checked against the receive ranges). |
| `args.seconds` | number | Audio length, 3 to 20, default 5. |
| `args.expect_tone_hz` | number, Hz | Optional (null counts as absent), 100 to 5000; the tone the owner expects, to report the offset. |
| `args.mode` | string | Optional rig mode name from `MODES` (case-insensitive); default `AM` when the frequency is below 30 MHz (`class_of` is `hf`), else `FM`. |

Order of work: validate the arguments (`bounded`); take the receive ranges (`rx_ranges`: the rig's `dump_state` narrowed by the owner's `rx_ranges_hz`) and refuse a
frequency outside them with the source of the limit (`check_freq`); read `get_ptt` and refuse if the rig reports that it is transmitting (a rig that does not
report PTT (push to talk) is let through; a link failure is an error); remember the frequency (a rig error is an error), the mode with its passband and the AF level (both optional);
run `radioctl_rx_listen`; then **always** restore, even after a failure: `set_mode <mode> <passband>`, `set_freq`, `set_level AF`, collecting the names of those that failed.
The result of the listening is returned with an added `rig_settings_not_restored` (a list, empty when all went back); if the listening failed, its error is returned instead
(and the restoring has still been tried). Other errors: `frequency_hz is required (Hz)`, bound errors (`<key> <value> is outside <lo> to <hi>`), `mode is not a rig mode name`,
`the rig reports PTT on; the receiver check does nothing while it transmits`. Transmit side: no key command is sent; the tier is `read_config`, the transmit gate and
the dummy-load check are not involved, and the band check is the receive-range check above; the VFO (variable frequency oscillator, the tuning control), split, RIT (receiver incremental tuning) and XIT (transmitter incremental tuning) are neither read nor restored. Blocking: about 0.3 s settling, 5 meter reads
80 ms apart and then the recording (`seconds`), all with the instrument lock held. Example: `{"frequency_hz": 10e6, "seconds": 5, "expect_tone_hz": 1000}`
gives AM at 10 MHz with `signal`, `squelch_open` and an `audio` object. Tests: `tests.rs` (cited as `radioctl_rx_check`: a test of the receiver check against a rig with synthetic audio). Related: `radioctl_rx_listen`, `radioctl_rx_ranges`, `radioctl_bounded`, `radioctl_detect_analyze`.

### `radioctl_rx_listen` [@ANCHOR: radioctl_rx_listen]

`listen(c, f: f64, mode: &str, seconds: f64, expect: Option<f64>, audio: Option<&dyn AudioSource>) -> Result<Value, String>` (private) does the measuring for
`radioctl_rx_check` after the arguments are checked: `f` in Hz, `mode` a valid mode name, `seconds` 3 to 20, `expect` the optional expected tone in Hz. It
sets the frequency and the mode (passband 0, the rig's default; errors `tuning to <f> Hz: ...` and `setting mode <m>: ...`), sets the AF level to the load check's `af`
default 0.5 only when audio is available (a refusal is ignored), waits 300 ms, and reads `get_level STRENGTH` five times 80 ms apart (an unreadable S-meter is the
error `the S-meter cannot be read: ...`). The `signal` object has `mean_db_relative_to_s9`, `spread_db` (largest minus smallest), `s_units`, `text` and the five `readings`;
`squelch_open` is from `get_dcd` (null when the rig does not report it). Without audio the result has `audio: "none configured: ..."`. With audio it
records `seconds` of samples (`the rig's audio cannot be recorded: ...` on failure), runs the time-station detector (`detect::analyze`; its findings come out as `findings`), and
adds `source`, `rms_dbfs` (0.1 dB), `input_all_zero` (a dead input) and `findings`. When the input is not all zero and at least 8192 samples came, it also adds
`noise_floor_dbfs` (the median of the spectrum from 300 to 3000 Hz), `dominant_tone` (`hz`, `level_dbfs`: the loudest peak from 150 to 3500 Hz that is 20 dB above that
median; absent when none), `tone_offset_hz` and `tone_offset_note` (only with `expect`: the offset of the tone from the expected one, which is the receiver's tuning error when the tone is a carrier beat; the sound
card's clock adds its own tiny error) and `signal_vs_audio` (one sentence). Side effects: it changes the rig's frequency, mode and AF level and does not restore them
(`rx_check` does); it opens the sound input for the length of the recording. Tests: `tests.rs` (cited as `radioctl_rx_listen`). Related: `radioctl_s_units`,
`radioctl_s_text`, `radioctl_tolerate`, `radioctl_station_class`.

## The reference-station table (`stations.rs`)

The load plausibility check (`load.rs`, `loadtools.rs`; `hamlib_b.md`) listens, before a transmit test, to reference stations through the rig's supposed dummy-load path.
The table of those stations is this file. The module comment sets the rules: never assume a station is on the air; a station that is silent now is "not heard", never a failure,
and does not weaken the check, which refuses when any reference *is* heard through the load path and needs no reference to be heard. The shipped table lists carrier
frequencies (public facts) with notes in our own words; the owner reviews it and adds local stations. Until the owner changes it, the defaults apply; once changed, it is stored
as `stations.json` in the managed directory (`radioctl_managed_dir`).

The record is `Station` (`Clone`, `Debug`, `PartialEq`), one per row:

| Field | Type | Meaning, range, default, who sets it |
|---|---|---|
| `id` | string | The key. 1 to 40 bytes of `a-z`, `0-9`, `-`, `_`. Required. Unique in a table (`add` refuses a duplicate). |
| `name` | string | Display name, at most 80 bytes; default empty. |
| `frequency_hz` | f64, Hz | Carrier frequency, finite, 10 kHz to 6 GHz. Required. Its class (`hf`, `vhf`, `uhf`) decides for which transmit frequencies it is a reference. |
| `mode` | string | A rig mode name from `MODES`; default `AM`. The mode the check tunes the receiver to. |
| `audio` | string | What to expect in the audio: one of `AUDIO_KINDS` = `wwv` (1 kHz ticks, 440/500/600 Hz tones), `wwvh` (1.2 kHz ticks), `carrier`, `voice`, `none`; default `none`. Only `wwv` and `wwvh` can be recorded as heard. |
| `kind` | string | `time_station`, `am_band`, `weather` or `local` (default `local`). Orders the choice of references. The code does not check that the value is one of these. |
| `note` | string | Where the status was checked and when; at most 600 bytes; default empty. Required non-empty for an entry the owner adds. |
| `last_heard` | `Option<String>` | The date `YYYY-MM-DD` of the last detection on this bench, written by `radioctl_stations_heard`; none until heard. |
| `retired` | bool | A retired station is never used as a reference; default false. Set by the owner. |
| `user_added` | bool | True for an owner's entry (JSON key `added_by_owner`); only those can be removed and they are listed first. |

`Station::json(today)` is the form shown to a caller: all fields (with `added_by_owner`) plus `heard_recently`, true when the last-heard date is today or up to 30 days ago.
The same function with an empty date writes the file (the `heard_recently` key is removed there). Tool access: `bench_radioctl_stations` (tier `read_only`) shows the table;
`bench_radioctl_station_set` (tier `read_config`) edits it. A change to the table changes which stations the dummy-load check listens to before a
transmit test; the check itself is described in `hamlib_b.md`.

### `radioctl_station_class` [@ANCHOR: radioctl_station_class]

`class_of(hz: f64) -> &'static str` sorts a frequency into a band class: `"hf"` below 30 MHz, `"vhf"` from 30 MHz up to (not including) 300 MHz, `"uhf"` from 300 MHz up.
Pure; no errors, no side effects. A NaN falls through to `"uhf"` (both comparisons are false). The class decides which stored stations are candidates for a
transmit frequency (`radioctl_stations_select`: a station is a reference only when it has the same class as the transmit frequency), and the default mode of the receiver check
(`radioctl_rx_check`: AM for `hf`, FM otherwise). It is also available as `Station::class()`. The boundaries are conventional radio-band names and
are not a licence-band table (unknown whether the owner's band plan matches; nothing here checks a transmit band). Tests: `stations.rs` (cited as `radioctl_station_class`).

### `radioctl_station_days` [@ANCHOR: radioctl_station_days]

`days(date: &str) -> Option<i64>` converts a `YYYY-MM-DD` date to the number of days since 1970-01-01 (so that the age of a date is a difference of two results). Split
on `-`, parts that do not parse as integers are dropped, and anything other than exactly three numbers gives `None`; the month must be 1 to 12 and the day 1 to 31 (the days of the month
are not checked: `2026-02-31` is accepted and counts as a day in March). The year is any integer; the computation is the standard civil-date day count (1970-01-01 is 0). Pure. Callers: `heard_recently`, the
`last_heard` check in `radioctl_station_check`, and `attest.rs` (for the attestation's date). `None` is how a malformed date is detected; there is no error text. Tests: `stations.rs`
(cited as `radioctl_station_days`).

### `radioctl_stations_recent` [@ANCHOR: radioctl_stations_recent]

`heard_recently(last: Option<&str>, today: &str) -> bool` tells whether a station was last heard within the last 30 days (`HEARD_RECENTLY_DAYS`). It is true when both dates parse with
`radioctl_station_days` and `today - last` is from 0 to 30 days inclusive. It is false for no date, an unparsable date, a date in the future, and (as `Table::save` passes `""` for today) when today is unknown.
Pure; used by `Station::json` for the `heard_recently` field of `bench_radioctl_stations`, which tells the owner whether a station is worth keeping.
Arguments are borrowed. Tests: `stations.rs` (cited as `radioctl_stations_recent`). Related: `now_local` (the caller's source of today's local date).

### `radioctl_stations_defaults` [@ANCHOR: radioctl_stations_defaults]

`defaults() -> Vec<Station>` is the table that ships, used when no `stations.json` exists. It lists 28 entries, all with `last_heard` none, not retired and not owner-added: WWV (`wwv-2_5`, `wwv-5`,
`wwv-10`, `wwv-15`, `wwv-20`; AM; audio `wwv`; kind `time_station`), WWVH (`wwvh-2_5`, `wwvh-5`, `wwvh-10`, `wwvh-15`; AM; audio `wwvh`; kind `time_station`), twelve US medium-wave samples (`am-640`, 680, 710, 760, 810, 880, 1010,
1070, 1120, 1200, 1500, 1600 kHz; AM; audio `voice`; kind `am_band`) and the seven NOAA (National Oceanic and Atmospheric Administration) Weather Radio channels (`wx-162400` to `wx-162550`, 162.400 to 162.550 MHz in 25 kHz steps; FM; audio `voice`; kind `weather`). The notes say in the code's own
words where the status was checked (NIST's published station information, January 2026, with the warning that budget proposals threaten WWV and WWVH, so the owner is to check again) and that
the AM and weather entries are samples whose usefulness depends on the owner's location. The Canadian station CHU is deliberately not listed: a code comment says its shortwave
transmissions stopped on 2026-06-22 (this page cannot check that). The frequencies are rounded to whole Hz. No arguments, no errors; a fresh vector on every call. The owner is expected to review the list and to retire or replace
entries; the shipped entries can be retired but never removed. Tests: `stations.rs` (cited as `radioctl_stations_defaults`).

### `radioctl_station_from_json` [@ANCHOR: radioctl_station_from_json]

`from_json(v: &Value) -> Result<Station, String>` (private) builds a `Station` from one JSON object and validates it. It reads the keys listed in the table above: `id` (required string) and
`frequency_hz` (required number); optional `name` (default empty), `mode` (`AM`), `audio` (`none`), `kind` (`local`), `note`, `last_heard`, `retired` (false) and `added_by_owner` (false);
other keys are ignored and a value of the wrong type counts as absent for the optional keys. It ends with `radioctl_station_check`, so every station that comes from the file or from the owner's `add` has been through the same rules. Errors:
`a station needs id`, `a station needs frequency_hz`, and the errors of the check. It is used by `Table::load` (every row of the file) and by the `add` action of `radioctl_stations_edit`. No I/O. Tests: `stations.rs`
(cited as `radioctl_station_from_json`).

### `radioctl_station_check` [@ANCHOR: radioctl_station_check]

`check(s: &Station) -> Result<(), String>` is the validity rule for a table entry: the id is 1 to 40 bytes of `a-z`, `0-9`, `-`, `_`; the frequency is finite and from 10 000 Hz to 6e9 Hz; the mode is in `MODES`; the audio kind is in `AUDIO_KINDS`; the name is at
most 80 bytes and the note at most 600 bytes; a present `last_heard` parses with `radioctl_station_days`. The first failing rule gives an error naming the station and the rule (for instance `station x: frequency_hz must be 10 kHz to 6 GHz`, `station
id "A B" must be 1 to 40 characters of a-z, 0-9, - and _`, `last_heard must be YYYY-MM-DD`). The `kind` and the `note` text are not otherwise checked. Pure (no I/O). Called by `from_json`. Because `Table::load` runs it for every row,
one bad row makes the whole file unreadable (see `radioctl_stations_load`). Tests: `stations.rs` (cited as `radioctl_station_check`).

### `radioctl_stations_load` [@ANCHOR: radioctl_stations_load]

`Table::load(dir: &Path) -> Result<Table, String>` reads the station table from `<dir>/stations.json` (`dir` is the managed directory). The file's form is `{"stations": [ {id, name, frequency_hz, mode, audio, kind, note, last_heard, retired, added_by_owner}, ... ]}`.
When the file cannot be read (absent, or not readable) the result is the shipped `defaults()`. A damaged file is an error, never a silent fall back: `<path>: <json error>. Next: repair or remove the file; the defaults apply when it is absent.`, `<path>: no
stations list`, or `<path>: <the first bad row's error>`. Callers: `loadtools::stations`, `station_set`, the load check and baseline (`loadtools.rs`) and the transmit tools' load gate. A damaged table therefore makes the load check, and with it a transmit test, fail until the file is repaired; it never
makes the check weaker. Read-only, no lock beyond the instrument lock of the calling tool. Tests: `stations.rs` (cited as `radioctl_stations_load`). Related: `radioctl_stations_save`.

### `radioctl_stations_save` [@ANCHOR: radioctl_stations_save]

`Table::save(&self, dir: &Path) -> Result<(), String>` writes the table to `<dir>/stations.json` as pretty-printed JSON `{"stations": [...]}`, one object per station with the fields listed above and without `heard_recently`. The write is to `stations.json.tmp` in the same
directory, then `rename`, so a reader sees the old file or the new one, not half of one (no `fsync`). The directory is created when missing. Errors: `<path>: <io error>` for the directory, the temporary file or the rename. Callers: `bench_radioctl_station_set`
(after an owner edit; its error is returned) and the load check (after recording heard stations; there the error is ignored). Concurrency: two servers or two rigs that share one managed directory share one table, and nothing locks the file
between them; in one server the instrument lock covers one rig only. Tests: `stations.rs` (cited as `radioctl_stations_save`).

### `radioctl_stations_heard` [@ANCHOR: radioctl_stations_heard]

`Table::mark_heard(&mut self, id: &str, date: &str) -> Result<(), String>` records `last_heard = date` for the station `id` (a `YYYY-MM-DD` string, normally today's local date from `now_local`); the date is not validated here. Error: `no station "<id>" (bench_radioctl_stations lists them)`. It changes the
table in memory only; the caller saves it. The caller is `load::note_heard`, which records a station only when its audio kind is `wwv` or `wwvh` and the detector found the time station's ticks or tones in the audio. The record is what feeds `heard_recently` and so what the owner sees. A station that is not heard is never marked and
nothing is recorded against it (silence is not a failure). A date that is not `YYYY-MM-DD` would make the saved file fail `radioctl_station_check` on the next load. Tests: `stations.rs` (cited as `radioctl_stations_heard`).

### `radioctl_stations_select` [@ANCHOR: radioctl_stations_select]

`Table::references(&self, tx_hz: f64, local_hour: u32, covers: &dyn Fn(f64) -> bool, limit: usize) -> Vec<&Station>` chooses the reference stations the load check listens to for a transmit frequency. Callers: the load check (`load.rs`, with `limit` the check's `references`, default 6), the transmit tools'
preparation (`loadtools.rs`) and `bench_radioctl_stations` (with a `frequency_hz`, limit 12).

| Argument | Type | Meaning |
|---|---|---|
| `tx_hz` | f64, Hz | The transmit frequency; only its class (`radioctl_station_class`) is used. |
| `local_hour` | u32, 0 to 23 | The local hour. `7..19` is day, the rest is night. |
| `covers` | `&dyn Fn(f64) -> bool` | True for a frequency the rig can receive (the caller passes the receive ranges); stations the rig cannot receive are dropped. |
| `limit` | usize | At most this many stations are returned. |

Selection: not retired, same class as `tx_hz`, and `covers(station frequency)`. Order: owner-added entries first (they were chosen for this site), then time stations, then AM-band stations, then the rest (weather and local); inside a group, at night the lower
frequencies first, by day the higher first (HF (high frequency, 3 to 30 MHz) propagation); the sort is stable. The result borrows from the table; it is not modified. Pure, no errors; an empty result is possible (a VHF (very high frequency, 30 to 300 MHz) transmit frequency with the shipped table gives only the weather channels). The result
is a list of candidates only: the check needs no station to be heard. Tests: `stations.rs` (cited as `radioctl_stations_select`).

### `radioctl_stations_edit` [@ANCHOR: radioctl_stations_edit]

`Table::apply(&mut self, action: &str, args: &Value) -> Result<String, String>` makes one owner edit to the table in memory and returns a short sentence (`added <id>`, `retired <id>`, `restored <id>`, `removed <id>`, `note changed for <id>`). `bench_radioctl_station_set` (tier `read_config`, under the instrument lock) calls it, then
saves the table. `args.id` (string) is always required (`id is required`).

| Action | Other arguments | Effect and errors |
|---|---|---|
| `add` | `frequency_hz` (required), `name`, `mode`, `audio`, `kind` (default `local`), `note` (required, non-empty) | Adds a new entry marked `added_by_owner`. Errors: the id exists (`retire or remove it first`); no note (`an added station needs a note saying where its status was checked and when`); every `radioctl_station_check` rule. |
| `retire` / `restore` | optional `note` | Sets `retired`; a given note is put in front of the old one as `<new> \| <old>` and cut to 600 characters. Unknown id is an error. |
| `remove` | none | Only for an entry the owner added; a shipped entry is refused (`retire it instead (retiring keeps the record that it was checked)`). |
| `note` | `note` (required, at most 600 bytes) | Replaces the note. |

An unknown action is `action "<a>" is not add, retire, restore, remove or note`. The table is changed only when the action succeeds, and nothing is written to disk by this function. Retiring a station removes it from the references the
load check listens to; the table's rules say that this never weakens the check, which refuses when any reference that is still in use is heard. Tests: `stations.rs` (cited as `radioctl_stations_edit`).

## The status tools and shared helpers (`status.rs`)

`bench_radioctl_status` (everything the receiver reports, in one call) and `bench_radioctl_get` (one value, or several signal readings) are tier `read_only`: they send only `get_*` commands and
cannot change or key the rig. A function the rig lacks is reported as unavailable with the reason; a link failure ends the call with an error, so a value that was not just read is never
returned. The file also holds helpers that other files use (`s_units`, `s_text`, `tolerate`, `bounded`). The constants `STATUS_LEVELS` (23 level names, the receive ones first and then the transmit-side meters, which read zero
when the rig is not keyed), `STATUS_FUNCS` (15 function names) and the AGC (automatic gain control) code table (0 off, 1 superfast, 2 fast, 3 slow, 4 user, 5 medium, 6 auto) are the lists the status call walks.

### `radioctl_s_units` [@ANCHOR: radioctl_s_units]

`s_units(db_rel_s9: f64) -> f64` turns the S-meter level (radioctld's `STRENGTH`, in dB relative to S9) into S-units: S9 at 0 dB, 6 dB per S-unit below it, so -54 dB is S0; the result is not below 0, is rounded to 0.1, and is 9.0 for any reading at or above S9 (the part above S9 is in `s_text`, not here). Pure; a NaN gives 0
(as `max` ignores NaN). Used by `radioctl_s_text`, `radioctl_level_json`, `radioctl_signal_readings`, `radioctl_rx_listen` and the scan result in `control.rs`. The mapping is the convention of 6 dB per unit below S9; whether a given rig's
meter follows it is a property of the rig. Tests: `status.rs` (cited as `radioctl_s_units`).

### `radioctl_s_text` [@ANCHOR: radioctl_s_text]

`s_text(db_rel_s9: f64) -> String` writes the same reading as text for a person: `S9+20 dB` for 0.5 dB or more above S9 (the dB rounded to a whole number), otherwise `S<n>` with `n` the S-units rounded to a whole number (`S7`; a reading from 0 to 0.5 dB above S9 shows `S9`). Pure. Used in the `text` fields of the status, get, signal and receiver-check
results. Tests: `status.rs` (cited as `radioctl_s_text`). Related: `radioctl_s_units`.

### `radioctl_tolerate` [@ANCHOR: radioctl_tolerate]

`tolerate<T>(r: Result<T, RigError>) -> Read<T>` separates two kinds of failure, where `Read<T>` is `Result<Result<T, RigError>, String>`. A link failure (`RigError::Link`: the socket failed, timed out, closed, or the reply was not in the protocol's form) becomes the outer `Err(text)`: nothing is known about the rig and the tool must stop. A rig-side refusal
(`RigError::Rig`, an `RPRT <negative>` reply: the rig or backend refuses or lacks the function) is data, the inner `Err`, which the caller may report ("not available on this rig") and continue. Used by every tool that reads optional values: the status, the receiver check, the scan and watch, the load check and the transmit
preparation. The value is moved; no side effects. Tests: `tests.rs` (cited as `radioctl_tolerate`). Related: `RigError::unsupported` (which codes mean "the rig lacks the function").

### `radioctl_capability_query` [@ANCHOR: radioctl_capability_query]

`listed(c: &mut Client, cmd: &str) -> Result<Option<Vec<String>>, String>` asks the rig which names it supports through a list query such as `get_level ?` or `get_func ?` and returns them. `cmd` is a complete command of the owner's tool (a constant in this file), and `c` is borrowed mutably (the read is retried once
on a link failure). The reply lines are split on whitespace into names. `Ok(None)` means the rig refused the query: the caller then tries every name and treats "unsupported" answers as absent. A link failure is the outer error. `bench_radioctl_status` uses it to skip names the rig does not list, so that
it does not report a long list of "not available" for levels the rig simply lacks. Tests: `tests.rs` (cited as `radioctl_capability_query`).

### `radioctl_level_json` [@ANCHOR: radioctl_level_json]

`level_json(name: &str, v: f64) -> Value` (private) shapes one level reading for the status and get results. `STRENGTH` becomes `{"db_relative_to_s9": v, "s_units", "text"}`; `AGC` becomes `{"code": <rounded integer>, "name": <off|superfast|fast|slow|user|medium|auto or null for a code outside 0 to 6>}`; every other level is the number itself, in the unit the rig reports it
(a level from 0 to 1 for gains, dB, Hz or watts depending on the level; radioctld's description of the rig is the authority, not this function). Pure. Tests: `tests.rs` (cited as `radioctl_level_json`). Related: `radioctl_status`, `radioctl_get`.

### `radioctl_status` [@ANCHOR: radioctl_status]

`status(c: &mut Client, rig: &str) -> Result<Value, String>` is `bench_radioctl_status` (tier `read_only`): all the receiver reports, in one call, under the instrument lock. `rig` is the instrument name. It sends `get_freq`, `get_mode`, `get_vfo`, `get_split_vfo`, `get_rit`, `get_xit`, `get_ptt`, `get_dcd`, `get_powerstat`,
`get_info`, the level query `get_level ?`, then `get_level <name>` for each of the 23 `STATUS_LEVELS` that the rig lists (all of them when it does not say), the same for the 15 `STATUS_FUNCS` with `get_func`, and `get_mem`. Result object: `rig`, `frequency_hz`, `mode`, `passband_hz`, `vfo`, `split` (`enabled`, `tx_vfo`), `rit_hz`, `xit_hz`,
`ptt` (`receive`, `TRANSMIT` or `unknown`), `carrier_detect_or_squelch_open`, `power_on`, `info`, `levels` (name to `radioctl_level_json`), `functions` (name to boolean), `memory_channel` and `unavailable`, an object of what could not be read, with `not available on this rig` for a refusal that means the function is missing, otherwise
the rig's error text. A refusal is data; a link failure aborts with the error. `get_dcd`, `get_powerstat`, `get_info` and `get_mem` are left out silently when the rig refuses them; the others appear under `unavailable`. A keyed rig is shown as `TRANSMIT`: a status of a transmitting rig is allowed (it only reads). The call is about
50 commands; each read is retried once, and the reply timeout is the rig's `reply_timeout_s` (1 to 30 s, default 3) per command. Tests: `tests.rs` (cited as `radioctl_status`). Related: `radioctl_tolerate`, `radioctl_capability_query`.

### `radioctl_get` [@ANCHOR: radioctl_get]

`get(c: &mut Client, args: &Value) -> Result<Value, String>` is `bench_radioctl_get` (tier `read_only`): one value from the rig, or several signal readings.

| Argument | Type | Meaning |
|---|---|---|
| `args.item` | string, required | One of `frequency`, `mode`, `vfo`, `split`, `rit`, `xit`, `ptt`, `squelch`, `power`, `info`, `memory`, `channel`, `level`, `function`, `signal`. |
| `args.name` | string | Required for `level` and `function`: the name (for instance `AF`, `ATT`, `NB`), checked by `check_arg` (letters, digits and `_ . + - / :`, at most 64) and made upper case. |
| `args.channel` | integer | Required for `channel`, below 100 000. |
| `args.count`, `args.interval_ms` | numbers | For `signal` (see `radioctl_signal_readings`). |

Results are small objects (`{"frequency_hz": ...}`, `{"mode", "passband_hz"}`, `{"ptt": "receive"|"TRANSMIT"|"unknown"}`, `{"level", "value"}`, `{"function", "on"}`, `{"channel", "as_reported": [lines]}` and so on). Unlike `status`, any failure, including a refusal by the rig, is an error text for that item (`unknown item "x"` for a bad item). A
`memory` or `channel` read of a radioctld rig is refused by the daemon (`RPRT -4`; the README says it does not serve memory channels). Nothing is written; no key command exists in the list. Tests: `tests.rs` (cited as `radioctl_get`). Related: `radioctl_level_json`, `radioctl_bounded`.

### `radioctl_signal_readings` [@ANCHOR: radioctl_signal_readings]

`signal(c: &mut Client, args: &Value) -> Result<Value, String>` (private) is the `signal` item of `radioctl_get`: several readings of the signal strength. `args.count` is 1 to 50 (default 1) and `args.interval_ms` is 0 to 2000 ms (default 100), both through `radioctl_bounded`; the first reading is immediate and the others follow after `interval_ms`
(a sleep with the instrument lock held, so at most about 100 s). Each is `get_level STRENGTH` in dB relative to S9. The result: `readings` (the list), `mean_db_relative_to_s9`, `spread_db` (largest minus smallest), `s_units` and `text` of the mean. A rig error or a link failure at any reading is the error, and no partial result is returned. A spread
that is large relative to the signal says that the reading is not steady (the load check uses the same idea). Tests: `tests.rs` (cited as `radioctl_signal_readings`).

### `radioctl_bounded` [@ANCHOR: radioctl_bounded]

`bounded(args: &Value, key: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String>` reads a number argument and enforces its range; the one function the driver's tools use for numeric arguments. A missing key or JSON null gives `default`; a value that is not a number gives `<key> must be a number`; a value that is not finite or is outside `lo..=hi` gives
`<key> <v> is outside <lo> to <hi>`. A default is checked with the same rule: passing `f64::NAN` as the default makes the key required (the callers then replace the error with their own `... is required` text). Pure; no side effects. Used by the status, receiver check, scan, watch and transmit tools and the load tools. Tests: `status.rs` (cited as `radioctl_bounded`: defaults, bounds and
rejection of NaN and text) and `tests.rs`.
