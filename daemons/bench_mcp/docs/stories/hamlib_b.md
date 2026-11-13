# The Radio Tools Talk to radioctld, Know the Rigs, Read the Owner's Transmit Attestation and Check That the Dummy Load Looks Like One

As the person (or the AI agent) running the bench, I want `bench_mcp` to talk to a radio through the `radioctld` daemon of `daemons/radioctl`, to tell me which
rigs exist and what each can do, to refuse any transmit test unless the owner has attested a dummy load in the config, and to listen with the rig in receive
before a transmit test to see whether the path behaves like a dummy load. This story documents the client of the `radioctld` line protocol
(`daemons/bench_mcp/src/radioctl/proto.rs`), the rig list read from the rig description files (`models.rs`), the owner's `transmit` block (`attest.rs`),
the load plausibility check and its baseline files (`load.rs`) and the tools around it (`loadtools.rs`). The slice name `hamlib_b` is historical: these files were
the Hamlib driver until #1568 converted the driver to radioctl, and every anchor id now starts with `radioctl_`. The tool pages for the agent are in
`docs/RADIOCTL_TOOLS.md`.

None of the functions below keys the transmitter. The two tools that transmit (`bench_radioctl_tx_key_test`, `bench_radioctl_tx_power_sweep`, in `txtools.rs`) use
parts of this slice as their safeguards: the transmit block parsed by `parse_transmit` is the owner's attestation and its caps, and `loadtools::gate` is the
load check that runs first. Each entry says how the tier, the per-instrument lock, the owner's band check and the TX gate apply to it. In short: tiers are
`read_only` < `read_config` < `full` (the tool list and `call` in `mod.rs` refuse a tool above the instrument's tier); every tool except `bench_radioctl_safe_state`
and the two station-table tools runs while holding the instrument's lock (`crate::instrument_lock`, one mutex per instrument name), so two calls on one rig never
interleave; the band check is `Transmit::range_of` against the owner's `frequency_ranges`; and the TX gate is `Verdict::Consistent` from the load check.

## The client of radioctld's protocol

### `RigError` [@ANCHOR: radioctl_error_display]

The error type of the client, with its `Display` text, which is what a tool shows the caller. There are two kinds. `Link(String)` means the socket failed, timed out or
closed, or the reply was not in the protocol's form, so nothing is known about the rig. `Rig { code: i32, cmd: String }` means `radioctld` answered `RPRT <code>` with
a negative code: the rig or the backend refused or could not do the command. The anchor sits in `fmt`: a `Link` prints its message unchanged (the messages already end in
a `Next:` hint); a `Rig` prints `radioctld refused <cmd>: RPRT <code> (<code name>). <next step>`, with the name from `code_name` and the hint from `code_next_step`.
`RigError::unsupported()` is true for codes -1, -4, -11, -15, -17 and -18 (not offered, or the argument is out of the domain): callers report these as a fact about the
rig, not as an outage.

| Field | Type | Meaning |
|---|---|---|
| `Link(m)` | `String`, owned | Complete message including what to do next. |
| `Rig.code` | `i32` | The negative number after `RPRT`. |
| `Rig.cmd` | `String`, owned | The command text that was refused, as sent (without the `+\` prefix). |

Errors: none; formatting cannot fail. Side effects: none, pure. It has no lock and no transmitter involvement. See also: `return_codes_have_names_and_a_next_step` in
`proto.rs` cites it; related anchors `radioctl_error_codes`, `radioctl_code_next_step`.

### `code_name` [@ANCHOR: radioctl_error_codes]

Maps a return code of the protocol to a short English name, for the error text and for the tools that explain why a rig did not answer. The comment in the code says `radioctld`
itself answers 0, -1, -4, -5, -6, -8, -9, -11 and -14, but the table knows 0 to -22 so that a code from any backend has a name.

| Argument | Type | Meaning |
|---|---|---|
| `code` | `i32` | The number after `RPRT`; 0 and every negative code to -22 are named. |

Return value: `&'static str`: `OK` for 0, for example `the rig did not answer (timeout)` for -5, `not available on this rig` for -11, `rig not powered on` for -20,
`access denied` for -22, and `unknown code` for anything else (positive numbers and codes below -22). Errors: none, it never panics. Pure function, no I/O, no lock. The
transmit path does not use it, except through the text of an error it passes on. See also: `return_codes_have_names_and_a_next_step` (`proto.rs`).

### `code_next_step` [@ANCHOR: radioctl_code_next_step]

Gives the sentence that tells the caller what to do about a refused command, appended to the error text of `RigError::Rig`. Private to `proto.rs`; it exists so every refusal
ends with an action rather than a bare number.

| Argument | Type | Meaning |
|---|---|---|
| `code` | `i32` | The `RPRT` code. |

Return value: `&'static str`, one of five sentences. Codes -5, -6, -8, -10, -13 and -14 (timeouts, I/O and bus trouble): check the cable, power and baud rate, run
`bench_radioctl_diagnose`, power-cycle the rig's USB port with the hub tools, then `bench_radioctl_daemon restart`. Code -20: the rig is off, switch it on. Codes -4, -11 and
-18: the rig or backend does not offer the function; `bench_radioctl_models` shows what it offers. Code -2: check the model number and device settings. Every other code, including
-1 and -9: check the argument against `bench_radioctl_models`. No errors, no I/O, no lock. See also: `return_codes_have_names_and_a_next_step`.

### `Reply::number` [@ANCHOR: radioctl_reply_number]

Reads one finite number out of a `Reply`, which is the lines between the echo of a command and its `RPRT` line. Used for everything numeric the tools read from the rig: the
frequency (`get_freq`, key `Frequency`), levels such as `STRENGTH` and `AF` (bare line), the data carrier detect (`get_dcd`, key `DCD`), passbands and meters. It is also what
`Client::read_number` calls.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `Reply`, borrowed | `lines`: the reply body with the echo line and the `RPRT` line removed. |
| `key` | `Option<&str>`, borrowed | `Some(k)`: the value of the `k: value` line (key compared without case). `None`: the first line, trimmed, as a bare value. |

Return value: `Ok(f64)`, always finite. Only the first whitespace-separated word of the text is parsed, so `1 dB` reads as 1. Errors, all `RigError::Link`: the key or the bare
line is missing (`radioctld reply lacks <key>: <lines>`); the word does not parse or is NaN or infinite (`radioctld gave "<text>", which is not a finite number. Next:
bench_radioctl_daemon restart; if it repeats the rig is misreporting.`). Pure, no I/O, no lock. Nothing transmits. See also: the test `reply_helpers_read_keys_and_bare_values_and_refuse_garbage` in
`proto.rs` cites it.

### `check_arg` [@ANCHOR: radioctl_arg_check]

Decides whether a piece of caller-supplied text may be placed in a command to `radioctld`, so that no text from a caller can become a second command or a different one. The
client frames a command as one line, so a newline or any space could inject another command; this check is applied by the control and status tools (`control.rs`, `status.rs`) to names they paste into commands (a
VFO name, a level name, a function name).

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&str`, borrowed | The text to check. |

Allowed: 1 to 64 bytes, each an ASCII letter or digit or one of `_ . + - / :`. Return value: `Ok(s)`, the same slice, or `Err(String)` of the form `"<s>" is not a usable radioctld
argument (letters, digits and _ . + - / : only, up to 64 characters)`. Empty text, spaces, quotes, control characters, non-ASCII and anything over 64 bytes are refused. Pure; no
I/O, no lock. It is a syntax check only: whether a rig has the level is for the rig to say, and the transmit limits are not decided here. See also: the test `arguments_cannot_carry_a_second_command` in `proto.rs` cites it.

### `Client::connect` [@ANCHOR: radioctl_connect]

Creates the client for one `radioctld` and opens its TCP connection at once. Called by `mod.rs::open` for every tool call that talks to a rig, and by the managed-daemon code to
probe whether a daemon answers.

| Argument | Type | Meaning |
|---|---|---|
| `host` | `&str`, borrowed (copied) | Host name or address of the daemon. The config only allows the local machine unless `allow_remote_radioctld` is set. |
| `port` | `u16` | TCP port of the daemon's `rigctld`-style listener. |
| `timeout` | `Duration` | The bound for every later exchange (read and write timeout and overall deadline), from the rig's `reply_timeout_s`. |

Return value: `Ok(Client)` holding the open connection (a write half and a buffered read half of the same socket), or `Err(RigError::Link)` from `reopen` (see that entry: resolution
failure or connect refused or timed out after 3 s). No command is sent on connect, so connecting never changes the rig. The `Client` is not shared between threads; a tool call owns it
for its duration, under the instrument's lock. See also: `radioctl_reopen`; the scripted-fake tests in `tests.rs` cite it.

### `Client::reopen` [@ANCHOR: radioctl_reopen]

Drops any connection and makes a new one. It is private and is called by `connect` and by `command` when the previous failure left no connection.

Steps: clear `conn`; resolve `host:port` with `to_socket_addrs` and take the first address; `TcpStream::connect_timeout` with a fixed 3 seconds (independent of the exchange timeout);
set `TCP_NODELAY` (failure ignored); clone the socket for the buffered reader. Errors, all `RigError::Link`: the name does not resolve (`radioctld at host:port: <io error>`), it resolves
to nothing (`... no address`), or the connect fails (`cannot connect to radioctld at host:port (<error>). Next: bench_radioctl_daemon status, then start or restart; for an existing
radioctld check that it runs and the port in the config.`). The client is left without a connection after any error. Side effects: one TCP connection to the daemon; no data is written.
Blocks up to 3 seconds. No lock of its own. See also: `radioctl_connect`, `radioctl_command`.

### `Client::command` [@ANCHOR: radioctl_command]

Sends one command to `radioctld` and returns its parsed reply: the single entry point through which every get, set, key and unkey of the radio tools goes. No retry is made here.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `Client` | The connection; reopened when absent. |
| `cmd` | `&str`, borrowed | The command and its arguments, such as `set_freq 7005000` or `get_level STRENGTH`, without the leading `+\` and without a newline. |

Return value: `Ok(Reply)` when the final `RPRT` code is 0 or positive. Errors: `RigError::Link("refusing a command with control characters: ...")` when `cmd` holds any ASCII
control character (this is the second guard against injection, after `check_arg`); a `Link` error from `reopen` when no connection exists and one cannot be made; whatever `exchange`
reports. On any `Link` error the connection is dropped, so the next command starts clean; a `Rig` error (negative `RPRT`) leaves the connection open, since the link itself worked.
Side effects: network I/O to `radioctld`, bounded by the client timeout; for a `set_*`, `set_ptt` or similar command, the rig changes. Transmit: this function does not decide whether
a command may be sent; `set_ptt 1` goes through it like any command, so the safeguards (tier, instrument lock, `parse_transmit` caps, `range_of` band check, the load gate, the keyed-interval
watchdog of `guard.rs`) live in the callers (`txtools.rs`), and the plain control tools refuse transmit-side settings before calling it. It must be called with the instrument's lock held.
See also: `radioctl_exchange`, `radioctl_read_retry`.

### `Client::exchange` [@ANCHOR: radioctl_exchange]

Does the actual one-line write and reply read, in the protocol's "extended response" form. Private; called only by `command`.

Behaviour: sets the read and write timeouts, writes `+\<cmd>\n`, then reads lines until a terminator or the deadline (now plus the client timeout, not reset per line). The first line must be
the echo `<command word>:`, otherwise the reply is not this protocol. Following lines are collected (at most 4000) until `RPRT <n>`; `n < 0` returns `RigError::Rig { code: n, cmd }`,
`n >= 0` returns `Reply { lines }`. The one command with another ending is `dump_state`: its reply is free text ended by a line `done` with no `RPRT`, and that `done` line is included as the
last element of `lines`.

Errors, all `RigError::Link` with the text `<radioctld at host:port>: <what> after <timeout> s (command <word>). Next: bench_radioctl_diagnose; then bench_radioctl_daemon restart; check the
cable and power of the rig.`, where `<what>` is: `write failed`, `no complete reply` (deadline passed), `read failed`, `the connection was closed`, `an unexpected first line`, `bad RPRT line` or
`a reply of more than 4000 lines`. Caveat: the deadline is only checked between lines, so a daemon that stalls mid-line is stopped by the socket read timeout rather than the deadline; both
equal the configured timeout. Side effects: network I/O only. Transmit: none by itself. Lock: the caller holds the instrument's lock. See also: `radioctl_command`, `radioctl_dump_state`.

### `Client::read` [@ANCHOR: radioctl_read_retry]

A command that only reads, with one retry. Reads (`get_freq`, `get_level`, `get_ptt`, `dump_state`) are safe to repeat, so a flaky link or a rig that is slow gets a second try; a command that
changes the rig is never sent through this function.

| Argument | Type | Meaning |
|---|---|---|
| `cmd` | `&str`, borrowed | A read command. The function cannot check that it is one; callers must pass reads only. |

Return value: the `Reply` of the first or second attempt, or the second attempt's error. A retry happens only when the first attempt gave `RigError::Link(_)` or `RigError::Rig` with code -5
(rig timeout) or -6 (I/O error); any other result, including success and other refusals such as -11, is returned at once. Before the retry it sleeps 100 ms and drops the connection, so
the second attempt opens a fresh one. Worst case is therefore two timeouts plus 100 ms and a reconnect. Side effects: network I/O. Lock: caller holds the instrument's lock.
Transmit: reading PTT (push to talk) through here (`get_ptt`) is how the tools check that a rig is not keyed. See also: `radioctl_command`, `radioctl_dump_state`.

### `Client::dump_state` [@ANCHOR: radioctl_dump_state]

Reads the rig's limits and level ranges with the `dump_state` command, through `read`, so it gets the one retry. The reply has no `RPRT`; `exchange` ends it at the `done` line, and the lines are
returned for `control::parse_dump_ranges` to turn into receive and transmit frequency ranges.

Arguments: `&mut self` only. Return value: `Ok(Reply)` whose `lines` end with `done`, or the errors of `read`/`exchange` (link errors and rig refusals; a rig that does not serve it answers
`RPRT -11` or `-4`, which callers treat as "no ranges" via `status::tolerate`). A reply over 4000 lines is an error. Side effects: network I/O, no change to the rig. Lock: the caller's.
Transmit: the transmit-range data returned here is read by the transmit tools' preflight (the rig's own transmit ranges are checked in `txtools.rs` in addition to the owner's `frequency_ranges`); `load.rs` uses
the receive side through `rx_ranges`. See also: `radioctl_exchange`, `radioctl_rx_ranges`.

## The rigs radioctl knows

### `parse_description` [@ANCHOR: radioctl_models_parse]

Reads the identity and capability list of a rig description file (a TOML file of radioctl's `rigs/` directory) with a small line reader, not a TOML parser. It exists so the bench can list and describe
rigs without starting a daemon; the daemon validates a description completely when it starts, so errors here do not mean the daemon will accept the file.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str`, borrowed | The whole file. |

Return value: `Ok(Rig)` with `id`, `name`, `family` (all strings) and `capabilities` (list of strings). It reads: top-level `capabilities = [ "..." , ... ]` (possibly spanning lines until the closing
`]`) and, under the `[rig]` section, `id`, `name` and `family`, each taking the quoted text of the value. Everything after a `#` on a line is a comment. Other sections and keys are ignored; the quoted
strings of a value are concatenated for `id`, `name` and `family`. Error: `"no [rig] id and name in the description"` when `id` or `name` is empty; `family` and `capabilities` may be empty. Pure, no I/O,
no lock. Limits of the reader (read from the code): a `capabilities` array placed after a section header is not seen (only the top level counts), and a `#` inside a quoted string cuts the line. See also:
tests in `models.rs`; related anchors `radioctl_models_list`, `radioctl_models_description_name`.

### `description_name` [@ANCHOR: radioctl_models_description_name]

Returns the display name of the rig a description file describes. It serves `bench_radioctl_diagnose`, which shows the name of the configured rig's description next to the other facts of a connection test.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&Path`, borrowed | A description file. |

Return value: `Ok(name)` (the `[rig] name`), or `Err(String)` of the form `cannot read the rig description <path>: <io error>` when the file cannot be read, or `<path>: <parse error>` when `parse_description`
fails. Side effects: reads one file. No lock needed; no network; nothing transmits. See also: `radioctl_models_parse`, and the diagnose tool in `diag.rs`.

### `list` [@ANCHOR: radioctl_models_list]

Lists every rig description in a directory, sorted by id.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path`, borrowed | The rigs directory, from the config's `rigs_dir`, the `RADIOCTL_RIGS` variable or the default found by `rigd::rigs_dir`. |

Return value: `Ok(Vec<Rig>)`, sorted by `id`. Only files ending in `.toml` are read; a file that cannot be read or does not parse is skipped silently, so one bad file does not hide the others. Errors:
`cannot read the rig description directory <dir>: <io error>` when the directory cannot be opened; `no rig descriptions in <dir>` when nothing parsed. Side effects: reads the directory and each
file; no lock and no daemon needed. Nothing transmits. See also: `radioctl_models_tool` (the only caller in the tool path), tests in `models.rs`.

### `models::tool` [@ANCHOR: radioctl_models_tool]

The body of `bench_radioctl_models`: lists the rigs radioctl has descriptions for, or describes one. Tier: `read_only`; `mod.rs::call` checks that some radioctl instrument has at least that tier and then
runs it without opening a rig and without taking the instrument lock, because it reads files only.

| Argument (JSON `args`) | Type | Meaning |
|---|---|---|
| `dir` (function argument) | `&Path` | The rigs directory, found by the caller. |
| `rig` | string, optional | A description id such as `elecraft-kx3`: return what that description declares instead of the list. |
| `search` | string, optional | Words to find, case-insensitively, as a substring of `"<id> <name> <family>"`. |
| `limit` | integer, optional | 1 to 500, default 50 (also accepted as `null`). |
| `instrument` | string, optional | Present in the schema; not used here. |

Return value, with `rig`: `{rig, name, family, capabilities, note}`, where the note says this is what the description declares, not what a particular unit does, and that a rig answers -11 for what its firmware
lacks. Without `rig`: `{total_rigs, matching, shown, rigs: [{rig, name, family}]}` with `shown` the smaller of `matching` and `limit`. Errors (as `Err(String)`): the errors of `list`; an unknown `rig` gives `no rig description "<id>". Next: search the
list with bench_radioctl_models and the search argument.`; a `limit` that is not a whole number from 1 to 500 gives `limit must be a whole number from 1 to 500`. Transmit: none; it only reads descriptions.
See also: the `models.rs` tests that cite it; `docs/RADIOCTL_TOOLS.md`.

## The owner's transmit attestation

### `number` [@ANCHOR: radioctl_attest_number]

Reads one optional bounded number from a JSON object of the `transmit` block (or its `load_check` sub-block). It exists so every numeric limit of the attestation is range-checked in one way.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&Value`, borrowed | The JSON object. |
| `key` | `&str` | The key to read. |
| `lo`, `hi` | `f64` | Inclusive bounds. |

Return value: `Ok(None)` when the key is absent or `null` (the caller then applies its default or reports the key as required); `Ok(Some(x))` for a finite JSON number inside `lo..=hi`. Error: `<key> must be a number from <lo> to
<hi>` for a string, a boolean, an infinite or out-of-range number. Pure, no I/O, no lock. This is where the safe ranges are enforced: for example `max_power_w` 0.01 to 200 and `max_seconds` 0.1 to 60 (the hard limits `HARD_MAX_POWER_W`
and `HARD_MAX_SECONDS`, which no configuration can raise) are checked through it. See also: `radioctl_attest_config`; the test `a_transmit_block_needs_the_attestation_and_every_limit_and_nothing_defaults_wider` cites it.

### `parse_transmit` [@ANCHOR: radioctl_attest_config]

Reads the `transmit` block of a rig's config entry into a `Transmit` value. This block is the owner's attestation that the rig's antenna connector is on a dummy load, with their name and the date, and the limits they accept: a rig can transmit through
these tools only when the block exists and parses. The software cannot see what is connected; the attestation and these limits are the control, and the load check is evidence on top. It is called when the instrument config is read (`mod.rs`
config parsing; a bad block refuses the configuration with the message). Everything the safe operation needs is required, and nothing is defaulted that would widen what may be done. Licence compliance for any transmission is the owner's responsibility.

| Key in the JSON | Type | Required | Rules and default |
|---|---|---|---|
| `dummy_load` | boolean | yes | Must be exactly `true`. |
| `attested_by` | string | yes | Non-empty, up to 80 characters, no control characters. |
| `attested_on` | string | yes | A date `YYYY-MM-DD` (10 characters); a future date is refused (compared with the local date). |
| `cabling` | string | yes | Non-empty, up to 300 characters, no control characters; describes what is connected. |
| `max_power_w` | number | yes | 0.01 to 200 (watts, the cap for any keyed power). |
| `max_seconds` | number | yes | 0.1 to 60 (the cap for any keyed interval). |
| `frequency_ranges` | array of `[low, high]` Hz | yes, non-empty | Each pair: 10 kHz <= low <= high <= 6 GHz. There is no default. |
| `allowed_modes` | array of strings | yes, non-empty | Each, upper-cased, must be in `control::MODES`. |
| `max_swr` | number | no | 1 to 5, default 2.0. An SWR (standing wave ratio) above it at the low-power probe refuses the test. |
| `swr_probe` | boolean | no | Default true. |
| `swr_probe_s` | number | no | 0.1 to 2, default 0.5 seconds. |
| `allow_lowering_power` | boolean | no | Default false: may a tool lower the rig's power setting to the requested level (restored afterwards). |
| `operator_alert` | boolean | no | Default false, but see below. |
| `load_check` | object | no | `margin_db` 1 to 30 (default 6), `audio_margin_db` 1 to 30 (6), `max_spread_db` 1 to 30 (6), `readings` 3 to 10 (5), `dwell_ms` 50 to 2000 (150), `references` 2 to 12 (6), `af` 0.05 to 1 (0.5), `preamp_db` 0 to 60 (none). |

Return value: `Ok(Transmit)`; the struct holds `attested_by`, `attested_on`, `cabling`, `max_power_w`, `max_seconds`, `frequency_ranges: Vec<(f64, f64)>`, `allowed_modes: Vec<String>` (upper-case), `max_swr`, `swr_probe`, `swr_probe_s`,
`allow_lowering_power`, `operator_alert` and `load: LoadCheck`. Errors (all `Err(String)`, each naming the key): `transmit must be an object`; `transmit.dummy_load must be true: the owner attests that the rig's antenna port is on a dummy load`;
`transmit.<k> is required (text)`; over-long or control-character text; `transmit.attested_on must be a date like 2026-10-29`; `transmit.attested_on is in the future`; missing or out-of-range numbers (from `number`); the `frequency_ranges` and
`allowed_modes` messages above; `transmit.load_check must be an object`; `transmit.<k> must be true or false`; and `transmit.<k> is not a known key (<list>)` for any key not in the known list.

Odd result found while documenting: the known-key list (`dummy_load`, `attested_by`, `attested_on`, `cabling`, `max_power_w`, `max_seconds`, `frequency_ranges`, `allowed_modes`, `max_swr`, `swr_probe`, `swr_probe_s`, `allow_lowering_power`, `load_check`) does not
include `operator_alert`, although the function reads it and `mod.rs` and `txtools.rs` use it. As the code stands, a block that sets `operator_alert` is refused as an unknown key, so the operator alert cannot be switched on through this parser. Not fixed here (docs only).

Side effects: none except reading the local clock. No lock, no network. Transmit: it creates the limits the TX path later enforces: `Transmit::range_of(hz)` returns the index of the owner's range containing `hz` (the band check), the power and time caps bound
`bench_radioctl_tx_key_test` and the sweep, and `allowed_modes` bounds the mode. The transmit preflight in `txtools.rs` also checks the frequency against the rig's own transmit ranges from `dump_state`. See also: `radioctl_attest_number`, `radioctl_loadtools_prepare` (uses `range_of`),
`radioctl_tx_gate`.

## The load plausibility check

### `Verdict::word` [@ANCHOR: radioctl_verdict_word]

Names a verdict in the words that go into the tool reports and error messages. `Verdict` has three values: `Consistent` (the readings match the baseline: consistent with a dummy load, not proof), `Refuse` (something is above the baseline, a station was
heard or a measurement failed: do not transmit) and `NoBaseline` (no baseline for this frequency range: the readings are only reported).

Argument: `&self`. Return value: `&'static str`: `consistent_with_a_load`, `refuse` or `no_baseline`. Pure. These strings are part of the output format of `bench_radioctl_load_check` (`verdict`) and of the refusal text of the transmit tools; callers that
branch must compare the `Verdict`, not the text. See also: `radioctl_load_decide`; the test in `load.rs` cites it.

### `decide` [@ANCHOR: radioctl_load_decide]

The decision of the load check, made on readings alone: no rig, no files, no clock. `bench_radioctl_load_check`, the baseline recorder (to find out whether readings are fit) and the transmit tools' gate all use it.

| Argument | Type | Meaning |
|---|---|---|
| `base` | `Option<&Baseline>` | What was recorded when the owner attested the load, or `None`. |
| `now` | `&[Reading]` | What was just measured: first the transmit frequency (`id` `tx`), then the reference stations. |
| `th` | `&LoadCheck` | The thresholds of the attestation (`margin_db`, `audio_margin_db`, `max_spread_db`). |
| `settings` | `&Value` | The settings record of this measurement (AF, preamp, attenuator, readings, dwell, references, whether audio was used). |

Return value: `Decision { verdict, reasons, compared }`, `compared` being how many current readings had a baseline counterpart with the same `id`. Rules, in order. No readings: `Refuse`. For every reading, a non-finite S-meter mean or spread is a
reason; an audio record that is all zeros (muted, unplugged, dead) is a reason; a time station's ticks or tones heard in the audio is a reason (the path is antenna-like). With no baseline: `NoBaseline` when there are no reasons so far, `Refuse` when there are,
plus a reason saying a baseline must be recorded. With a baseline: different settings than at the baseline is a reason; then for each reading found in the baseline: a spread (the larger of now and baseline) over `max_spread_db` is a reason and the reading is
not compared further; mean minus baseline mean minus `margin_db` minus the spread above 0 is a reason; squelch open now but not at the baseline is a reason; audio rms more than `audio_margin_db` above the baseline (unless all zero) is a reason; a steady
note (beat) more than 15 Hz from every baseline note is a reason. Fewer than 2 of the current readings being found in the baseline is a reason (`only N of the M readings are in the baseline`). The verdict is `Consistent` only if there are no reasons, and then one reason is added saying this is consistent with a load and is not proof;
otherwise `Refuse`. A reading present now but absent in the baseline is skipped, not an error, but the compared-count rule catches a baseline that does not cover the set.

This is the TX gate's judgement: anything but `Consistent` stops a transmit test (`txtools::gate`, `radioctl_tx_gate`), which also means a missing baseline stops it. It is evidence, not proof: an open or unplugged connection hears nothing and looks good (the low-power SWR
probe exists for that), and the owner's attestation still gates transmitting. Pure function, no I/O, no lock. See also: `radioctl_verdict_word`, `radioctl_baseline_problems`; the tests in `load.rs` cite it (at the baseline, above it, unsteady, no baseline, other settings, a station heard, a dead input).

### `load_baseline` [@ANCHOR: radioctl_baseline_load]

Reads the stored baseline for one of the owner's frequency ranges. The baseline file of a rig is `<dir>/<safe_name(rig)>.baseline.json`, a JSON object `{"bands": {"<low>-<high>": {taken_on, attested_by, cabling, settings, readings[]}}}`, the band key being the range's low and
high in whole Hz (for example `7000000-7010000`). Each reading is stored in full (`id`, `frequency_hz`, `mode`, `mean_db`, `spread_db`, `squelch_open` and `audio_obs` with `rms_dbfs`, `all_zero`, `time_station`, `beats_hz`, `detail`), unrounded.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | The rig's managed directory (`rigd::dir`). |
| `rig` | `&str` | The instrument's name (made file-safe by `safe_name`). |
| `range` | `(f64, f64)` | The owner's frequency range holding the transmit frequency. |

Return value: `Ok(Some(Baseline))`; `Ok(None)` when the file does not exist or has no entry for that range; `Err(String)` when the file is not valid JSON (`<path>: <error>. Next: remove the file and record the baseline again.`) or when the band entry has no usable readings
(`<path>: the baseline for <key> is damaged. Next: record it again.`). Readings that cannot be parsed are dropped; if none survives it is the damaged error. Side effects: reads one file. No lock; concurrent calls only read. Nothing transmits. See also: `radioctl_baseline_save`, `radioctl_load_gate`
(the reader); the round-trip test in `load.rs` cites it.

### `save_baseline` [@ANCHOR: radioctl_baseline_save]

Stores the baseline for one frequency range in the rig's baseline file, keeping the other ranges already in it. Called only by `bench_radioctl_load_baseline`, after the readings passed `baseline_problems`.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | Directory, created if absent. |
| `rig` | `&str` | Instrument name. |
| `range` | `(f64, f64)` | The range, which becomes the band key (low-high in whole Hz). |
| `b` | `&Baseline` | The record: `taken_on`, `attested_by`, `cabling`, `settings`, `readings`. |

Return value: `Ok(())` or `Err(String)` of the form `<path>: <io error>` for the directory, the temporary file or the rename. The existing file is read and parsed; if it is missing or damaged it starts again from an empty `{"bands": {}}` (a damaged file is overwritten without comment).
The write goes to `<name>.baseline.json.tmp` and is renamed over the file, so a crash leaves the old file or the new one. Pretty-printed JSON. The two steps (read then write) are not locked against another writer; the tool call holds the instrument's lock, which is what makes it safe for one rig.
Transmit: none, but this file is what the TX gate compares with, so recording a baseline with a real antenna connected would make a later check pass wrongly; the tool therefore requires `confirm_dummy_load` at tier `full`. See also: `radioctl_baseline_load`, `radioctl_load_baseline_tool`.

### `mode_for` [@ANCHOR: radioctl_load_mode_for]

Picks the receive mode used for the reading at the transmit frequency itself: AM when the frequency is in the HF class (below 30 MHz, per `class_of`), FM otherwise (VHF and UHF). The transmit frequency has no station to say which mode to use, and the check only listens, so a plain demodulator
suits.

Argument: `hz: f64`. Return value: `"AM"` or `"FM"` (`&'static str`). Pure, no I/O, no errors. Note that a non-finite `hz` falls in the UHF branch of `class_of` and so gives `FM`; callers check the frequency before. This mode is the one that is set on the rig for the reading, and the rig's own mode is restored
afterwards. See also: `radioctl_load_measure`, which calls it for the `tx` reading.

### `read_original` [@ANCHOR: radioctl_load_read_original]

Records what the rig was set to, before the load check retunes it, so `restore` can put it back. Private to `load.rs`.

Argument: `c: &mut Client`. Return value: `Ok(Original { freq, mode: Option<(String, f64)>, levels: Vec<(&'static str, f64)> })`: the frequency in Hz (`get_freq`, key `Frequency`; required), the mode name and passband (`get_mode`; `None` if the rig does not offer it, or it cannot be parsed), and the levels `AF`, `PREAMP` and `ATT`
(`get_level`, each only if the rig answers). Errors: the frequency read failing returns the error text (`Err(String)`); for mode and the levels a link failure returns `Err`, while a rig refusal such as -11 is tolerated (`status::tolerate`: the value is simply absent). Reads are retried once (`Client::read`). Nothing is changed on the rig. Lock: the caller (`measure`) is under the
instrument's lock. See also: `radioctl_load_restore`, `radioctl_load_measure`.

### `restore` [@ANCHOR: radioctl_load_restore]

Puts the rig back after the load check: mode and passband, then frequency, then each level that was read. Called by `measure` on every path, success or failure.

Arguments: `c: &mut Client`, `o: &Original`. Return value: `Vec<String>` of what could not be restored (`mode <name>`, `frequency <Hz> Hz`, `level <name>`), empty when all went back. It never returns an error and it keeps going after a failure, so one rejected command does not leave the rest unrestored. Commands sent:
`set_mode <mode> <passband>`, `set_freq <Hz>`, `set_level <name> <value>`: receive settings only, no PTT. The result is surfaced to the caller as `rig_settings_not_restored` in the tool output, so a failure is reported rather than hidden. Side effects: changes the rig back. The check never touches a keyed rig, so this
cannot interfere with a transmission. Lock: the caller's. See also: `radioctl_load_read_original`, `radioctl_load_measure`.

### `read_point` [@ANCHOR: radioctl_load_read_point]

Measures one frequency with the rig in receive: tunes, waits, reads the S-meter several times, reads the squelch, and records audio if an input is configured. It is the unit of the check (one for the transmit frequency, one per reference station). Private.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | The rig connection. |
| `th` | `&LoadCheck` | Supplies `dwell_ms` and `readings`. |
| `id` | `&str` | `tx` or the station id; copied into the `Reading`. |
| `hz` | `f64` | Frequency to tune. |
| `mode` | `&str` | Mode name, set with passband 0 (the rig's default). |
| `audio` | `Option<&dyn AudioSource>` | The rig's audio input, or `None` for no audio evidence. |
| `audio_s` | `f64` | Seconds of audio to capture. |

Return value: `Ok(Reading { id, frequency_hz, mode, mean_db, spread_db, squelch_open, audio })`: mean of the `STRENGTH` readings (dB relative to S9), spread (highest minus lowest), the squelch from `get_dcd` (`None` if the rig has none) and the audio observation from `detect::analyze`. Sequence: `set_freq`, `set_mode`, sleep
`dwell_ms`, then `readings` S-meter reads 60 ms apart. Errors (`Err(String)`, each prefixed with the id): tuning or mode setting failed; the S-meter cannot be read (`refusing, because a check without a reading proves nothing`); the audio cannot be recorded (`refusing, because the audio evidence is missing`); the detection failed. A `get_dcd` rig refusal is tolerated.
Side effects: changes the rig's frequency and mode (receive); records audio from the sound card through the audio module; takes `dwell_ms + 60*(readings-1)` ms plus the capture time. Lock: the caller holds the instrument's lock. Transmit: it never sets PTT. See also: `radioctl_load_measure`, `radioctl_load_audio_seconds`.

### `audio_seconds` [@ANCHOR: radioctl_load_audio_seconds]

Chooses how long to record audio at a reference station: 8 seconds when its `audio` kind is `wwv` or `wwvh` (a time station, where the one-second tick rhythm needs several ticks to be recognised), otherwise 4 seconds.

Argument: `s: &Station`, borrowed (reads its `audio` field). Return value: `f64` seconds, 8.0 or 4.0. Pure, no errors. The transmit frequency's own reading always uses 4.0 (a fixed value in `measure`). The total listening time of a check is therefore roughly the sum over the references, up to 12 references at 8 seconds in the worst case, as `references` allows up to 12.
See also: `radioctl_load_measure`, `radioctl_load_read_point`.

### `measure` [@ANCHOR: radioctl_load_measure]

Does the whole listening part of the load check with the rig in receive: the transmit frequency and the reference frequencies. It is called by `loadtools::measure`, for the check and for the baseline recorder. The rig's frequency, mode, AF, preamp and attenuator are put back afterwards on every path. A keyed rig is never touched.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | The rig connection. |
| `tx` | `&Transmit` | The attestation; `tx.load` gives the thresholds, AF, preamp, readings, dwell, references. |
| `tx_hz` | `f64` | The transmit frequency being checked. |
| `table` | `&Table` | The station table, for the reference selection. |
| `covers` | `&dyn Fn(f64) -> bool` | True when the rig can receive that frequency (receive ranges in force). |
| `audio` | `Option<&dyn AudioSource>` | The audio input, or `None`. |

Return value: `Ok(Measured { readings, settings, stations_used, restore_failed })`. Steps: read `get_ptt`; if `PTT` is anything but `0` return `Err("the rig reports PTT on; the load check listens with the rig in receive and does nothing while it transmits")` (an error reading PTT is ignored, so a rig that does not answer `get_ptt` is still measured);
choose up to `load.references` stations with `Table::references` for this frequency, the hour and what the rig covers; `read_original`; set AF to `load.af`, the preamp if configured, and ATT 0 (failures ignored: a rig without them is listened to as it is, and the `settings` record says what was applied); `read_point` for `tx` (mode from `mode_for`, 4 s of audio) and then each station
(mode and audio length from the station); `restore` always. A measurement error is returned only after the restore has run. The reading `Err` comes before the restore failures, which are in `restore_failed`. Side effects: rig retuned and put back; audio recorded; takes seconds to a minute or more. Lock: the caller's; it does not take one itself.
Transmit: receive only. Tier: through `bench_radioctl_load_check` at `read_config` and `bench_radioctl_load_baseline` at `full`, and by the transmit tools at `full`. See also: `radioctl_loadtools_measure`, `radioctl_load_gate`.

### `note_heard` [@ANCHOR: radioctl_load_note_heard]

Records in the station table which time stations the check actually heard, so the table's `last_heard` dates come from this bench's own detections.

| Argument | Type | Meaning |
|---|---|---|
| `table` | `&mut Table` | Updated in memory (the caller saves it). |
| `m` | `&Measured` | A finished measurement; reading 0 (`tx`) is skipped and the rest are paired with `stations_used` in order. |

Return value: `Vec<String>` of the station ids marked. A station is marked heard only when its reading's audio shows a time station (`time_station`) and the station's `audio` kind is `wwv` or `wwvh`, so a carrier on a weather channel or an AM station is not recorded here. The date is today's local date (`now_local`). A failed `mark_heard` is skipped. Pure apart from the clock and the in-memory table; no I/O, no lock.
Note: hearing a time station through the supposed dummy load also makes `decide` refuse; the mark only records the fact for the station table. See also: `radioctl_loadtools_measure` (calls it and saves the table), `radioctl_stations_tool`.

## The tools around the check

### `stations` [@ANCHOR: radioctl_stations_tool]

The body of `bench_radioctl_stations`: shows the table of reference stations the load check listens for (the shipped WWV, WWVH, AM band samples and weather radio entries and the owner's own), with notes, the date each was last heard and whether that is recent. Tier `read_only`; `mod.rs::call` dispatches it without
the instrument lock because it only reads one file. Never keys.

| Argument (JSON) | Type | Meaning |
|---|---|---|
| `include_retired` | boolean, default false | Also list stations the owner retired. |
| `frequency_hz` | number, optional | Also show the references that would be used now for this transmit frequency (at most 12). |
| `instrument` | string, optional | Which rig; the table is per rig directory. |

Return value: `{today, stations: [...], table_file, saved, note}` where `saved` says whether the table file exists (otherwise the defaults are shown), plus with `frequency_hz` `references_for_frequency_hz`, `references_now` (`id`, `frequency_hz`, `mode`) and `local_hour`. Errors: the table load errors of `Table::load` (a damaged `stations.json` is an error, never a silent fall back
to the defaults). Side effects: reads `<dir>/stations.json`. See also: `radioctl_station_set_tool`, `radioctl_stations_load`.

### `station_set` [@ANCHOR: radioctl_station_set_tool]

The body of `bench_radioctl_station_set`: the owner's edits of the reference station table. Tier `read_config` (a tool above the instrument's tier is refused with a message naming both tiers). It is dispatched without the instrument lock, like `stations`, so two edits at once could overwrite each other, but the table is small and one owner edits it.

| Argument (JSON) | Type | Meaning |
|---|---|---|
| `action` | string, required | `add`, `retire`, `restore`, `remove` (only an entry the owner added) or `note`. |
| `id`, `name`, `frequency_hz`, `mode`, `audio`, `kind`, `note` | various | The fields the action needs, validated by `Table::apply` (frequency 10 kHz to 6 GHz, audio `wwv`, `wwvh`, `carrier`, `voice` or `none`, kind `local`, `time_station`, `am_band` or `weather`). |

Return value: `{done, stations_in_table, table_file}`, `done` being the description of the change. Errors: `action is required: add, retire, restore, remove or note`; the load error of a damaged table; the errors of `apply` (for example an id that already exists, an added station without a note, removing a shipped entry, a note over 600 characters, an unknown action); the write error of `save`. Side effects: rewrites `<dir>/stations.json`. Never
touches the rig or the transmitter. The shipped entries are retired, not removed, so they can be restored. See also: `radioctl_stations_tool`, `radioctl_stations_save`.

### `prepare` [@ANCHOR: radioctl_loadtools_prepare]

The common first step of the load check and the baseline recorder: finds the rig's transmit attestation and the owner's frequency range for the requested frequency. Private to `loadtools.rs`.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | The rig's radioctl settings. |
| `args` | `&Value` | Must hold `frequency_hz`. |

Return value: `Ok(Prepared { tx, range, dir })`: the `Transmit` block, the owner's `(low, high)` range that holds the frequency and the managed directory. Errors: `this rig has no transmit attestation in the bench config, so there is no dummy-load path to check. Nothing was done.`; `frequency_hz is required (Hz): the transmit frequency the check is for` (missing, not a number
or outside 1 Hz to 1e12 Hz, via `status::bounded`); `<f> Hz is outside the frequency ranges the owner listed in transmit.frequency_ranges (<list>); the check is only for those`. This is the band check of the load tools: the check and its baseline only exist for frequencies the owner has attested, by `Transmit::range_of`. Creates the managed directory if
absent (`rigd::dir`). No lock of its own. Nothing transmits. See also: `radioctl_attest_config`, `radioctl_load_gate`.

### `loadtools::measure` [@ANCHOR: radioctl_loadtools_measure]

Wraps `load::measure` with the table, the receive ranges and the "heard" bookkeeping. Private to `loadtools.rs`; used by the gate and the baseline recorder.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection. |
| `cfg` | `&Config` | Rig settings. |
| `p` | `&Prepared` | From `prepare`. |
| `f` | `f64` | The frequency in Hz. |
| `audio` | `Option<&dyn AudioSource>` | Audio input. |

Return value: `Ok((Measured, Table))`. Steps: load the station table; read the receive ranges in force (`rx_ranges`: the rig's own from `dump_state` narrowed by the owner's `rx_ranges_hz`); refuse the frequency if it is outside them (`check_freq`, naming the limit and its source, because the check is receive-only and the rig must be able to receive
there); run `load::measure` with a `covers` predicate built from the same ranges; record stations heard with `note_heard` and save the table when any were (a failed save is ignored). Errors: those of the pieces, as `Err(String)`. Side effects: rig retuned and restored; audio recorded; may rewrite `stations.json`. Lock: the caller's. Receive only. See also:
`radioctl_load_measure`, `radioctl_load_note_heard`.

### `gate` [@ANCHOR: radioctl_load_gate]

Runs the whole check and returns the decision with a report. The transmit tools call it as their gate (`txtools::gate`, anchor `radioctl_tx_gate`); `bench_radioctl_load_check` shows its report. It is therefore the only place the load check and the TX tools share.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection, with the instrument's lock held by the caller. |
| `cfg` | `&Config` | Rig settings. |
| `audio` | `Option<&dyn AudioSource>` | Audio input, or `None`. |
| `rig` | `&str` | Instrument name, for the baseline file. |
| `f` | `f64` | The transmit frequency to check, Hz. |

Return value: `Ok((Decision, Value))`. The JSON report has `verdict` (the word), `reasons`, `compared_with_baseline`, `readings` (rounded to 0.1 dB), `baseline` (`taken_on`, `attested_by`, `cabling`, or `null`), `audio` (a description of the input, or a note that none is configured), `stations_heard_now`, `rig_settings_not_restored` and a fixed note that
this is evidence, not proof. Steps: `prepare` (attestation and band check), `load_baseline` for the range, `measure`, `decide`, then the stations heard today among those used. Errors: those of `prepare`, `load_baseline` (a damaged baseline file) and `measure`; a refusal verdict is not an error here: it is `Ok` with `Verdict::Refuse` and the callers decide. The TX gate treats anything other than
`Consistent` as a refusal. Side effects: as `measure`; reads the baseline file. Never keys. See also: `radioctl_load_check_tool`, `radioctl_tx_gate`, `radioctl_load_decide`.

### `load_check` [@ANCHOR: radioctl_load_check_tool]

The body of `bench_radioctl_load_check`: the receive-only readings compared with the baseline, shown to the caller. Tier `read_config`; offered in the tool list only for a rig whose config has a `transmit` block; runs under the instrument's lock. Never keys.

| Argument (JSON) | Type | Meaning |
|---|---|---|
| `frequency_hz` | number, required | A frequency inside one of the owner's `transmit.frequency_ranges`. |
| `instrument` | string, optional | Which rig. |

Return value: the report of `gate` without the decision. A missing frequency becomes NaN and is refused by `prepare` as `frequency_hz is required`. Errors as `gate`. A `refuse` or `no_baseline` verdict is a successful answer with the reasons, not an error. Side effects: as `gate` (the rig is retuned and restored; the audio is recorded). Evidence only: passing this check does not authorise
anything; the TX tools run the gate again themselves. See also: `radioctl_load_gate`, `radioctl_baseline_problems`.

### `baseline_problems` [@ANCHOR: radioctl_baseline_problems]

Says what makes a set of readings unfit to be recorded as a baseline. A baseline taken with a station audible or with an unsteady S-meter would make later checks too lenient, so the recorder refuses those.

| Argument | Type | Meaning |
|---|---|---|
| `m` | `&Measured` | The readings just taken. |
| `max_spread_db` | `f64` | The largest acceptable spread in one reading (the attestation's `load_check.max_spread_db`). |

Return value: `Vec<String>`, empty when fit. It runs `decide` with no baseline and the default thresholds, keeps the reasons other than the one about the missing baseline (so non-numeric readings, a dead audio input and a time station heard show up), then adds `<id>: the readings vary by <x> dB, more than <max> dB: too unsteady to be a baseline` for each reading whose spread exceeds the limit. Pure.
See also: `radioctl_load_baseline_tool` (the only caller), `radioctl_load_decide`.

### `load_baseline` [@ANCHOR: radioctl_load_baseline_tool]

The body of `bench_radioctl_load_baseline`: records the readings through the dummy load as the baseline for the owner's frequency range holding `frequency_hz`. The owner attests that the load is connected; the tool cannot know. Tier `full` (the highest), because a baseline taken on a live antenna would make the TX gate pass later; offered only for a rig with a
`transmit` block; runs under the instrument's lock. It never keys.

| Argument (JSON) | Type | Meaning |
|---|---|---|
| `frequency_hz` | number, required | Inside one of the owner's `frequency_ranges` (checked by `prepare`). |
| `confirm_dummy_load` | boolean, default false | Must be `true`, and must come from the person at the bench: it states that the rig's antenna port is on the dummy load. |
| `dry_run` | boolean, default false | Return the plan and change nothing; listed first, so it needs no confirmation. |
| `instrument` | string, optional | Which rig. |

Return value: with `dry_run`: `{dry_run: true, sent: [], frequency_range_hz, rig, would: [...], attested_by, attested_on, cabling, note}`; the plan names the reference stations and the audio input. Without confirmation: error `recording a baseline states that the rig's antenna port is on the dummy load described as "<cabling>" in the config; call it with confirm_dummy_load: true only when the person at the bench has said so
(dry_run: true shows the plan)`. Confirmed: `{recorded: true, frequency_range_hz, taken_on, attested_by, cabling, readings, rig_settings_not_restored, audio, note}`. Errors: those of `prepare` and `measure`; `no baseline was recorded: <problems>` when `baseline_problems` is not empty (a station heard, unsteady readings, no audio signal, a non-number), which is the case when an antenna is connected; the write error of `save_baseline`. The record carries the attestation's name, the current local date
and the cabling text, and the settings of the check; a later check with other settings refuses until the baseline is recorded again. Side effects: rig retuned and restored; audio recorded; one baseline file written. Receive only. See also: `radioctl_baseline_save`, `radioctl_baseline_problems`, `radioctl_load_gate`.
