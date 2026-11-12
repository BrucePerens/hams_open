# The Radio Tools Key the Transmitter Into an Attested Dummy Load, Release It on Every Path, Watch the Dial and Take the Rig's Audio

As the person (or the AI agent) running the bench, I want `bench_mcp` to transmit a short, low-power test into a dummy load that the owner has attested in the
config, to read the meters while it is keyed, to be certain the rig is un-keyed afterwards whatever went wrong, to tell me what the dial does for a few
seconds, and to take the rig's receive audio from a sound input of the `audio` module. This story documents four files of `daemons/bench_mcp/src/radioctl/`:
`tx.rs` (the one place where PTT is turned on, and every way it is turned off), `txtools.rs` (the two tools that transmit, `bench_radioctl_tx_key_test`
and `bench_radioctl_tx_power_sweep`, and the safe-state tool), `watch.rs` (`bench_radioctl_watch`) and `audio_in.rs` (the audio source of the receiver
tests and of the load check). The slice name `hamlib_d` is historical: these files were part of the Hamlib driver until the driver became `radioctl`, and every
anchor id now starts with `radioctl_`. The tool pages for the agent are in `docs/RADIOCTL_TOOLS.md`; the earlier slices are `hamlib_a.md` (dispatch, protocol,
watchdog process) and `hamlib_b.md` (attestation parsing, the load check).

This is the transmit path, so each entry says how the four controls apply to it. In short:

* **Tier.** The tool list and `call` in `mod.rs` refuse a tool above the instrument's tier: `bench_radioctl_watch` is `read_only`, `bench_radioctl_safe_state` is
  `read_config`, and `bench_radioctl_tx_key_test` and `bench_radioctl_tx_power_sweep` are `full`. The two keyed tools are also offered only for a rig whose config has a
  `transmit` block (`NEEDS_ATTESTATION`), and `txtools::attested` refuses again when it is absent.
* **Lock.** Every tool except `bench_radioctl_safe_state` (and the two station-table tools) runs while holding `crate::instrument_lock(<instrument name>)`, one mutex
  per instrument, so two calls on one rig never interleave. `safe_state` takes no lock on purpose, so that it works while a keyed interval is running and ends it.
  Nothing in `tx.rs` takes the lock itself; the keyed tools are called with it already held.
* **Band check.** The owner's `transmit.frequency_ranges` (`Transmit::range_of`, in `check_args`) and then the rig's own transmit ranges from `dump_state`
  (in `preflight`). Both are checked before anything is sent to the rig that changes it.
* **TX gate.** Two things are called a gate. In the code of this slice, `txtools::gate` is the load plausibility check: only `Verdict::Consistent` lets the test
  go on. Separately, the managed `radioctld` is started with `transmit = true` in its own configuration only for a rig that has the owner's attestation, which opens
  the daemon's own transmit gate (`rigd::config_text`); with it closed, the daemon reports no transmit range and nothing can be keyed (see
  `radioctl_tx_full_scale`).

**Tests never transmit.** The standing rule (`manual/concepts/extending/standing-rules.md`, rule 4) is that no test keys a real transmitter or drives a real
speaker. The tests that cite the anchors below talk to a simulated rig (`Sim` in `testkit.rs`, served by `FakeRadioctld`), whose "PTT" is a boolean in memory,
and run the watchdog (`guard::run`) as a thread (`ThreadSpawner`) because a test binary is not `bench_mcp`. `tests/radioctl_guard.rs` runs the real watchdog
process against a virtual `radioctld` rig started on a free localhost port. The operator alert is recorded by `RecordingAlert`, not played. A test that needed
real RF or sound would have to be `#[ignore]` and run by the owner; none of the entries below has one.

The software cannot know what is connected to the rig. The owner's attestation and the load check are the controls, and every keyed tool's result carries the
sentence `CANNOT_KNOW` saying so.

## Keying the rig and every way of releasing it (`tx.rs`)

### `log_event` [@ANCHOR: radioctl_tx_log]

Appends one JSON line to the keyed-interval log of a rig: the permanent record of every time the transmitter was keyed, refused, stopped or released. It is called by
`tx::key` (events `begin` and `end`), by `txtools::key_test` and `power_sweep` (`refused` when the load check says no, `stopped` when something failed after that),
by `startup_check` (`startup_release`) and by `safe_state` (`safe_state`).

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path`, borrowed | The rig's managed directory (`rigd::dir`); created if it is missing. |
| `rig` | `&str` | The instrument name; the file is `<safe_name(rig)>.keyed.log` (letters, digits, `-` and `_` kept, anything else becomes `_`). |
| `event` | `&str` | Event name. The names used are `begin`, `end`, `refused`, `stopped`, `startup_release`, `safe_state`. |
| `fields` | `serde_json::Value` | A JSON object whose keys are merged into the line; a non-object adds nothing. A key of the same name as one of the base keys replaces it. |

Each line is a JSON object with `time` (`YYYYMMDD-HHMMSS`, local time, from `audio::store::timestamp`), `date` (`YYYY-MM-DD`, local), `rig`, `event`, plus the merged
fields. The file is opened for append, created if needed, one `writeln!` per call, then `sync_all` so the line is on disk before the function returns. No return value.
Errors: none are reported; creating the directory, opening, writing and syncing are all best effort and a failure is dropped silently (so a full disk or a read-only
directory means the transmit is not logged, and nothing tells the caller; unknown whether any other check notices). It does not panic. Side effects: file append and
fsync, which blocks for the duration of the disk write. No lock: two threads logging to one rig could interleave only whole lines, because each is written by one call
(unknown whether the OS guarantees that for long lines); in practice the instrument lock serializes the keyed tools. It never touches the rig and never transmits.
See also: the test `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval` cites it; related anchors `radioctl_tx_key`, `radioctl_tx_startup_check`,
`radioctl_tx_safe_state`.

### `believable_swr` [@ANCHOR: radioctl_tx_swr_valid]

Decides whether a standing-wave-ratio reading from the rig is believable enough to record and to act on. A real SWR is at least 1.0; the virtual rig and some radios
answer a constant 0 or nothing on the SWR meter, and treating that as "SWR 0, perfect" would hide an open or shorted connection, so those readings are "not reported".

| Argument | Type | Meaning |
|---|---|---|
| `x` | `f64` | The number read from `get_level SWR`. |

Return value: `Some(x)` when `x` is finite and in `1.0..=100.0`; `None` otherwise (zero, negative, below 1, above 100, NaN, infinity). Errors: none, never panics. Pure
function: no I/O, no lock, no transmitter involvement. It is used by `read_meters` for the SWR level while keyed, and so by the low-power probe: `None` samples count as
"no usable SWR", and the probe refuses to go on when it never saw a usable reading. See also: test `an_swr_reading_is_believed_only_from_one_up`; related anchors
`radioctl_tx_read_meters`, `radioctl_tx_probe`.

### `Keyed::release` [@ANCHOR: radioctl_tx_release]

The normal way a keyed interval ends: sends `set_ptt 0` on the connection already in use, reads PTT back, and falls back to a fresh connection and to the watchdog when
that does not work. It is a method of the private guard object `Keyed` that `tx::key` makes just after arming the watchdog, and `key` always calls it exactly once on the
way out of the sampling loop, for every outcome except a panic (that is `Drop`, below).

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&mut Keyed` | Holds the `Keying` request (host, port, rig, directory, minimum power), the optional watchdog handle and the `released` flag. |
| `c` | `&mut Client` | The connection `key` used to key the rig; borrowed. |

Return value: `Ok(true)` when `get_ptt` read back `PTT 0`; `Ok(false)` when `set_ptt 0` was accepted but the rig cannot report PTT (the read-back came back "unsupported",
`RigError::unsupported`); `Err(text)` when the release was not confirmed. Sequence: first `set_ptt 0` then `get_ptt` on `c`; if either fails (a link error, a refusal, or PTT
still reading non-zero), `guard::unkey(host, port, None)` runs: up to eight attempts on new connections. On success `finish` is called (watchdog stood down, record removed).
On failure the watchdog handle is told `leave_to_guard` (the pipe closes without `done`, so the watchdog acts alone) and the error begins `RELEASE NOT CONFIRMED:
... The rig may be transmitting. Switch it off at the rig now ... call bench_radioctl_safe_state, and do not use the rig until the owner has looked at it.` The minimum power
setting is not sent by this fallback (`guard::unkey` is called with `None`); the power setting is restored by the caller on the normal path. Side effects: socket I/O to
`radioctld`, a possible sleep of 250 ms between attempts, the state file removal in `finish`. Blocking. Transmitter: this is the un-keying step; it needs no tier, lock or
gate of its own and must work whatever else failed. See also: tests `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`,
`a_normal_release_is_not_repeated_by_the_drop`; related anchors `radioctl_tx_keyed_released_flag`, `radioctl_tx_keyed_drop`, `radioctl_guard_unkey`.

### `Keyed::finish` [@ANCHOR: radioctl_tx_keyed_released_flag]

Marks a keyed interval as properly released and removes every trace that the rig may be keyed. It sets the `released` flag on the `Keyed` guard so that the `Drop` does not
repeat the release, removes the rig from the process-wide `ACTIVE` table (the table `release_prefix` and `is_active` read), deletes the state file
`<rig>.keyed.json`, and tells the watchdog to stand down (`Watch::stand_down`, which sends `done` and waits for the process; its result is ignored). Called only by
`Keyed::release` after a confirmed release. No arguments besides `&mut self`; no return value; no error is reported (file removal and stand-down failures are dropped).
The flag is the invariant: `released == true` means PTT was read back off (or could not be read but was accepted off) and nothing is left to undo. Concurrency: the
`ACTIVE` mutex is taken with a poison-tolerant helper, so a panic elsewhere does not block it. Never keys. See also: test `a_normal_release_is_not_repeated_by_the_drop`
(one `set_ptt 0`, not two, and the power setting left to the caller); related anchors `radioctl_tx_release`, `radioctl_tx_keyed_drop`.

### `Keyed::drop` [@ANCHOR: radioctl_tx_keyed_drop]

The last line of defence: when the `Keyed` guard goes out of scope without `finish` having run, the rig is un-keyed anyway. That happens on a panic while keyed (the unwind
runs `Drop`), or on an early return in `tx::key` that skipped `release`. If `released` is false it calls `guard::unkey(host, port, min_power)`, which opens new connections,
sends `set_ptt 0`, sets the rig's minimum power setting (best effort) and reads PTT back, with up to eight attempts; then it removes the rig from `ACTIVE`. The watchdog
handle is dropped with the guard, which closes its pipe without `done`, so the watchdog also un-keys the rig on its own. The result of `unkey` is ignored: a destructor
cannot report, and the watchdog is the next net. Blocking while it runs (up to roughly eight rounds of connecting and 250 ms sleeps). No arguments, no return. Note for
the reader: when `release` itself failed (`RELEASE NOT CONFIRMED`) `released` is still false, so this `Drop` also runs, and removes the rig from `ACTIVE` even though the rig
may still be keyed; the state file is left on disk in that case, which is what the next start of the server looks for (see `radioctl_tx_startup_check`). Transmitter: only
un-keys. See also: test `a_panic_while_keyed_still_releases_the_rig_and_sets_the_minimum_power`; related anchors `radioctl_tx_release`, `radioctl_tx_keyed_released_flag`.

### `read_meters` [@ANCHOR: radioctl_tx_read_meters]

Reads one set of meter values from the rig while it is keyed. Called once per sampling step by `tx::key`.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection to `radioctld`; borrowed. |
| `have` | `&mut [(&'static str, bool)]` | Table of the meters to try: `RFPOWER_METER_WATTS`, `RFPOWER_METER`, `SWR`, `ALC`, `TEMP_METER`, each with an "available" flag. A meter whose read is refused as unavailable has its flag cleared so it is not asked again in this interval. |
| `t` | `f64` | Seconds since the key-down; stored in the sample as `t_s`. |
| `full_scale_w` | `Option<f64>` | The rig's full-scale transmit power in watts at the test frequency, used to turn a fractional power meter (`RFPOWER_METER`) into watts; `None` leaves such a reading out. |

Return value: `Ok(Sample)` with the fields it could fill (`power_w`, `swr`, `alc`, `temp`, each `Option<f64>`; `None` is "not reported"). For each meter the command is
`get_level <NAME>`: SWR goes through `believable_swr`; ALC and temperature are stored as read, in whatever unit the rig gives; the watts meter is stored as watts; the
fraction meter becomes watts (`v * full_scale_w`) only if no watts meter already filled `power_w`. Errors: `Err(text)` when a read fails for any reason other than the
rig answering RPRT -11, -4 or -18 (those three mean "this meter is not offered" and clear the flag): the text is `reading the <NAME> meter while keyed failed: <error>`, and
the caller aborts the keyed interval on it. A non-numeric value also ends in `Err`. Blocking socket I/O, one round trip per available meter; no sleeping. No lock (the
caller holds the instrument lock); does not itself key or release anything. See also: test `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`;
related anchors `radioctl_tx_swr_valid`, `radioctl_tx_summary`.

### `key` [@ANCHOR: radioctl_tx_key]

Keys the rig for a planned time, samples the meters, and releases it on every path. This is the only function in the module that sends `set_ptt 1`. It is called by
`txtools::keyed`, which is used for the low-power probe, the main interval of the key test and each step of a power sweep.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | The connection to the rig's `radioctld`; borrowed; used to key, sample and release. |
| `k` | `&Keying` | The request. `rig` (instrument name), `host`, `port` (where radioctld listens), `dir` (managed directory), `spawner` (starts the watchdog), `min_power` (lowest RFPOWER setting, a fraction 0 to 1, or `None`), `purpose` (tool, reason, who attested, frequency Hz, mode, power W, kind `probe`, `test` or `sweep step`), `limit_swr` (abort above this SWR), `limit_power_w` (the owner's cap; abort when the meter reads more than 1.5 times it), `previous_rfpower` (the setting before the test, written to the state file for a later recovery). |
| `seconds` | `f64` | Planned keyed time; must be in `0.1..=60` (`attest::HARD_MAX_SECONDS`, fixed in the code, not configurable). |
| `every` | `Duration` | Time between meter readings (the first reading is taken at once, without a sleep). |
| `full_scale_w` | `Option<f64>` | Full-scale watts for the fractional power meter, passed to `read_meters`. |

Order of events: (1) the seconds are checked; (2) the watchdog is spawned with a deadline of `seconds + MARGIN` (3 s) and must report `armed`, else `key` returns its error and
the rig is never keyed; (3) the `Keyed` guard is created; (4) the state file `<rig>.keyed.json` is written (pid, host, port, minimum power, previous RFPOWER, start time and the
purpose; a write failure is ignored), the `begin` line is logged and the rig is entered in `ACTIVE`; (5) `set_ptt 1`, the first command that radiates, whatever it answers; if it
is refused the interval is "aborted" with `set_ptt 1 failed: ...` and the loop is skipped; (6) the loop: read meters, abort when the SWR is over `limit_swr` or the power is
over 1.5 times `limit_power_w`, keep the sample (also the one that caused the abort), check with `get_ptt` that the rig is still keyed (if something else un-keyed it, such as
`safe_state` or the operator, the interval is aborted with `the rig was un-keyed by something else`; an error from that read is ignored), then sleep `min(every, time left)`;
(7) `Keyed::release`; (8) the `end` line with `outcome` `completed`, `aborted` or `release_not_confirmed`, the keyed seconds, the reason and the abort reason.

Return value: `Ok(KeyedRun)`: `samples` (all `Sample`s in time order), `seconds_keyed` (measured from just before `set_ptt 1` to just after the loop, so slightly more than the plan),
`aborted` (`None` when the planned time ran out, else the reason) and `ptt_read_back` (`true` when PTT was read back off, `false` when the rig cannot report PTT). Errors, each a
`String`: the seconds are out of range (`... is outside the 0.1 to 60 s a keyed interval may last`), the watchdog could not be armed (the spawner's text), or the release was not
confirmed (the `RELEASE NOT CONFIRMED` text; the samples are lost in that case, but the log line records `release_not_confirmed`). A panic anywhere after the guard exists is handled
by `Keyed::drop`. Blocking: it sleeps on the calling thread for about `seconds`, holding the instrument lock the whole time (the caller took it). Side effects: a watchdog process,
the state file, two log lines, the keyed transmitter. Tier, lock, band check and TX gate: `key` checks none of them; they are all done before it is called (`call` for the tier and the
lock, `check_args` and `preflight` for the bands, `gate` for the load check, and the daemon's own transmit gate decides whether `set_ptt 1` can do anything). What `key` itself
guarantees is the hard duration limit, the armed watchdog, the abort limits and the release. See also: test
`a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`; related anchors `radioctl_tx_keyed`, `radioctl_guard_run`, `radioctl_tx_release`.

### `release_prefix` [@ANCHOR: radioctl_tx_release_all]

Stops anything this process has keyed whose rig name starts with a given prefix (an empty prefix means all rigs). It is called by `shutdown_prefix` in `mod.rs`, which `shutdown`
calls when the server stops, before the managed daemons are stopped: stopping `radioctld` first would make a release impossible.

| Argument | Type | Meaning |
|---|---|---|
| `prefix` | `&str` | Instrument-name prefix; `""` matches every rig in the `ACTIVE` table. |

Return value: `Vec<String>` of failures, each `<rig>: <error>`; empty when all were released. For each match in `ACTIVE` it calls `guard::unkey(host, port, min_power)` (up to eight
attempts with new connections; sets the minimum power and reads PTT back); on success the rig is removed from `ACTIVE`, on failure it stays there and its text is returned (the
caller prints it to standard error as `could not confirm the release of a keyed rig`). Only rigs this process keyed (the in-memory table) are covered; a state file left by another,
dead process is handled by `release_rig`, `startup_check` and `release_after_crash`. Errors: none returned as `Err`; nothing panics (the table's lock tolerates poisoning).
Blocking: network I/O and sleeps, up to about eight rounds per rig. It does not take the instrument lock, on purpose (it must work during a call that holds it). Transmitter: it only
un-keys. See also: test `stopping_the_server_releases_a_rig_that_is_keyed_before_the_daemons_are_stopped`; related anchors `radioctl_shutdown_all`, `radioctl_tx_release_rig`.

### `release_rig` [@ANCHOR: radioctl_tx_release_rig]

Releases one rig before its daemon is stopped on purpose (the `stop` and `restart` actions of `bench_radioctl_daemon`). The daemon is the only way to reach the rig, so a rig that may be
keyed must be un-keyed first, and the stop is refused when that cannot be confirmed.

| Argument | Type | Meaning |
|---|---|---|
| `rig` | `&str` | Instrument name. |
| `dir` | `&Path` | The rig's managed directory, where the state file `<rig>.keyed.json` is looked for. |

Return value: `Ok(true)` something was released; `Ok(false)` the rig was neither in `ACTIVE` nor recorded in a state file with a readable host and port, so nothing was done; `Err(text)`
when `guard::unkey` could not confirm the release: `the rig may be keyed and could not be released first (<error>). Stopping its daemon would leave it keyed. Next:
bench_radioctl_safe_state`, which the tool wraps as `not stopping the daemon: ...`. The record used is the in-memory entry first, otherwise the state file's `host`, `port` and
`min_power` fields (an unreadable or incomplete file counts as no record). On success the `ACTIVE` entry and the state file are removed. Blocking network I/O; no instrument lock of its
own (the daemon tool holds it). Transmitter: only un-keys, and sets the minimum power through `unkey`. See also: test
`stopping_the_daemon_of_a_rig_that_may_be_keyed_releases_it_first_or_refuses`; related anchors `radioctl_daemon_tool`, `radioctl_tx_release_all`.

### `release_after_crash` [@ANCHOR: radioctl_release_after_crash]

The cleanup for a rig that the previous `bench_mcp` server may have left keyed. `rigd.rs` calls it when it finds a leftover `radioctld` of a dead server (the owner process is
gone but the daemon lives) and is about to terminate that daemon.

| Argument | Type | Meaning |
|---|---|---|
| `_rig` | `&str` | The rig name; unused (the leading underscore) and only there for symmetry with the other release functions. |
| `port` | `u16` | The localhost port the leftover daemon listens on. |

Return value: `true` when `guard::unkey("127.0.0.1", port, None)` succeeded, `false` when it did not. No minimum power is set (the `None`). Errors: none; a failure is the `false`.
The one caller in `rigd.rs` ignores the return value and sends the leftover daemon SIGTERM either way, so a failed release does not stop the daemon from being killed (intended: the daemon belongs to a dead server and must go so that the rig can be managed again). Blocking: up to eight rounds of connect, `set_ptt 0`, read-back with 250 ms sleeps. No lock. Transmitter: only un-keys. See also: test
`releasing_after_a_crash_sends_ptt_off_to_the_daemon`; related anchors `radioctl_tx_startup_check`, `radioctl_guard_unkey`.

### `startup_check` [@ANCHOR: radioctl_tx_startup_check]

The first contact with a rig that has a transmit attestation, in this run of the server: asks the rig to stop transmitting and records whether a crash had left it keyed. It is called
by `open` in `mod.rs` for every call that opens a connection to such a rig, including a `read_only` status call and `bench_radioctl_safe_state`, because the question "did a crash
leave the rig transmitting" must be answered before anything else.

| Argument | Type | Meaning |
|---|---|---|
| `rig` | `&str` | Instrument name. |
| `dir` | `&Path` | Managed directory (state file, log). |
| `host`, `port` | `&str`, `u16` | Where `radioctld` listens. |

No return value. A process-wide list `CHECKED` makes it run once per rig per server run (the rig is added to the list before any work, so a failed attempt is not repeated until the
server restarts). Steps: read the state file `<rig>.keyed.json` if there is one (a crash left it); connect with a 2 s timeout and read `get_ptt` (`PTT` other than `0` means keyed;
unknown if the rig cannot say); the rig is "stuck" when there was a state file or PTT reads on; for a stuck rig, read `dump_state` and find the lowest RFPOWER setting
(`rfpower_min`); then `guard::unkey(host, port, min_power)` (sends `set_ptt 0`, the minimum power if known, reads PTT back; a rig that was not stuck gets the `set_ptt 0` only, "as a
precaution"; `T 0` to a receiving rig does nothing); log a `startup_release` line; delete the state file; and, only for a stuck rig, store a note (`stuck_ptt_found`, `ptt_before`
`on`/`off`/`unknown`, `state_file_found`, `released`, `error`, `what`) for `take_note`. Errors: none returned; a link failure just means the unknowns above, and a failed release is in the note
and the log (`released: false`, `error`). Blocking: up to a couple of seconds of connect time plus the `unkey` rounds. Lock: it runs inside `open`, so under the instrument lock for every
call but `safe_state`, which opens without it. Transmitter: never keys; it only un-keys. See also: test `the_first_contact_releases_a_rig_left_keyed_by_a_crash_and_says_so_in_the_result`;
related anchors `radioctl_open`, `radioctl_tx_take_note`, `radioctl_startup_note`, `radioctl_release_after_crash`.

### `take_note` [@ANCHOR: radioctl_tx_take_note]

Hands over, once, the note that `startup_check` left for a rig whose PTT was found stuck. `with_startup_note` in `mod.rs` calls it after every tool call and, if there is a note, adds
it to the result as `startup_release`, so the caller learns that a crash left the rig keyed. Argument: `rig` (`&str`, the instrument name). Return value: `Some(Value)` the JSON note,
removed from the process-wide table (so a second call returns `None`), or `None` when there is no note (the normal case, and also the case of a rig that was not stuck, because only a
stuck rig leaves a note). No errors; no I/O; the table lock tolerates poisoning. Never touches the rig. See also: test
`the_first_contact_releases_a_rig_left_keyed_by_a_crash_and_says_so_in_the_result`; related anchors `radioctl_tx_startup_check`, `radioctl_startup_note`.

## The tools that transmit and the safe state (`txtools.rs`)

### `AudioAlert::alert` [@ANCHOR: radioctl_tx_alert]

The production implementation of the `Alerter` trait: sounds an operator alert through the `audio` module's tool `bench_audio_alert`. It exists so that a person at the bench is warned
before the transmitter is keyed. Tests use a recording `Alerter` instead, because no test makes sound.

| Field / argument | Type | Meaning |
|---|---|---|
| `self.insts` | `&[Instrument]` | The whole instrument list of the bench config, for the call into the sound module. |
| `self.instrument` | `String` | The name of the audio instrument to play on (the rig's `audio.instrument` setting). |
| `message` | `&str` | The text to announce. |

The call made is `crate::audio::call(insts, "bench_audio_alert", {"instrument", "severity": "warning", "message", "rediscover": false})`. Return value: what that tool returned (a JSON
object; `operator_alert` reads its `audible`, `error` and `text_to_send` members); `Err(text)` when the sound module refuses or fails, for example an unknown audio instrument. It plays real sound through the owner's audio instrument and is therefore subject to the sound module's own rules (quiet hours, reserved devices) which are not
part of this slice. No lock of its own beyond what the audio module takes. Never touches the rig. See also: test
`the_operator_alert_goes_to_the_audio_instrument_and_an_unknown_one_is_an_error_without_sound`; related anchor `radioctl_tx_operator_alert`.

### `rfpower_min` [@ANCHOR: radioctl_tx_min_power]

Finds the lowest RFPOWER setting the rig accepts, from the `level_gran` line of `dump_state`. That value is the setting used by the low-power probe, by the watchdog and the other releases
("set the minimum power"), and a floor for every requested power.

| Argument | Type | Meaning |
|---|---|---|
| `dump` | `&[String]` | The lines of the `dump_state` reply. |

The function looks for a line starting `level_gran=`, splits it at `;`, takes the entry starting `12=` (the code treats level number 12 as RFPOWER), and reads its first two comma
separated numbers as `lo` and `hi`. Return value: `Some(lo)` when `hi > lo` and `0 <= lo < 1` (a fraction of full scale); `None` when the line or the entry is missing, a number does not
parse, the range is empty, or `lo` is outside 0 to 1. A `None` means the rig did not say, and callers treat that as a reason not to transmit (`preflight` refuses) or to skip setting the
minimum power (the safe-state paths). Pure function, no I/O, no lock, no panic. See also: test
`the_rig_is_read_first_and_a_split_xit_keyed_or_over_cap_rig_is_refused_without_a_byte_sent`; related anchors `radioctl_tx_preflight`, `radioctl_tx_safe_state`.

### `full_scale_mw` [@ANCHOR: radioctl_tx_full_scale]

Reads the rig's full-scale transmit power at a frequency, in milliwatts, so that watts can be turned into the RFPOWER setting (a fraction of full scale) and back. `radioctld` has no
power-conversion command, so the setting is taken as linear in watts. For a managed rig the daemon's `tx_power_mw` is set from the owner's `full_power_w`; for an existing `radioctld` it is
that daemon's own value. Private to `txtools.rs`.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection to `radioctld`. |
| `hz` | `f64` | The transmit frequency in Hz. |

It reads `dump_state`, parses the transmit ranges (`parse_dump_ranges`: low Hz, high Hz, lowest mW, highest mW) and returns the highest-power figure of the first range that holds `hz` and
has a positive one. Return value: `Ok(mW)`. Errors, each a `String`: a link failure (as given by the client), `dump_state` refused (`the rig's transmit power range cannot be read ...; without it
the power cap cannot be enforced, so nothing is sent`), or no range with a power limit holds the frequency (`the rig reports no transmit range ... (is the daemon's transmit gate open and its
tx_power_mw set?) ...`). That last text is the symptom of a closed daemon transmit gate: with no transmit range reported, the power cap cannot be enforced, so the tools refuse. Blocking: one
round trip (the `dump_state` reply, read to its `done` line). No lock beyond the caller's; never keys. See also: test
`the_power_setting_is_a_fraction_of_the_transmit_range_power_the_daemon_reports_and_without_one_nothing_is_sent`; related anchors `radioctl_tx_power_convert`, `radioctl_dump_ranges`.

### `watts_to_fraction` and `fraction_to_watts` [@ANCHOR: radioctl_tx_power_convert]

The two conversions between watts and the RFPOWER setting, both using `full_scale_mw` at the given frequency: `watts_to_fraction` returns `w * 1000 / full_scale_mw`, `fraction_to_watts` returns
`f * full_scale_mw / 1000`. The anchor is on `watts_to_fraction`; the second is its inverse and has none of its own. Visible to the rest of `radioctl` (`pub(super)`).

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection to `radioctld`; each call reads `dump_state` again (one round trip). |
| `w` / `f` | `f64` | Watts, or the setting as a fraction of full scale. No range check here; a result above 1.0 is possible and is not clamped by the function. |
| `hz` | `f64` | The transmit frequency, which selects the transmit range. |

Return value: `Ok(f64)`. Errors: those of `full_scale_mw` (no usable transmit range). Pure arithmetic after the read; no lock; never keys. The conversion is only as good as the owner's
`full_power_w` (or the existing daemon's `tx_power_mw`) and the assumption that the rig's power setting is linear in watts; the meters read while keyed are what shows a difference
(unknown how far real radios deviate). See also: same test as `radioctl_tx_full_scale`; related anchors `radioctl_tx_preflight`, `radioctl_tx_keyed`.

### `check_args` [@ANCHOR: radioctl_tx_args]

Checks the arguments of a keyed test against the owner's attestation, before the rig is touched. It is the band check, mode check, power cap and duration cap in one place, and it is
used by `key_test` and (for its highest step) by `power_sweep`. The result is `TestArgs`: `frequency_hz`, `mode` (upper case), `power_w`, `seconds`, `every_ms`, `dry_run`, `confirm`, `reason`.

| Argument (JSON key) | Type | Rules |
|---|---|---|
| `frequency_hz` | number, required | Hz; 1 to 1e12 and inside one of `transmit.frequency_ranges` (`Transmit::range_of`); there is no default range list. |
| `mode` | string, optional | Case-insensitive; must be in `MODES` and in `transmit.allowed_modes`; default is the first allowed mode. A value that is not text is refused (`mode must be text`). |
| `power_w` | number, required | 0.001 to 1000 W, and at most `transmit.max_power_w` (itself at most 200 W). |
| `seconds` | number, required | 0.1 to 60 s, and at most `transmit.max_seconds`. |
| `sample_every_ms` | number, optional, default 250 | 100 to 2000 ms; stored as `every_ms`. |
| `reason` | string, optional here | Trimmed; at most 120 bytes and no control characters. Empty is allowed here but the key test then refuses it later. |
| `dry_run`, `confirm_dummy_load` | boolean or null, optional | `null` or absent mean false; any other type is an error `<name> must be true or false`. |
| `tx` | `&Transmit` | The owner's attested block (borrowed). |

Return value: `Ok(TestArgs)` or `Err(text)`, the first failing check, in the order above, each text saying what to change; examples: `<hz> Hz is outside transmit.frequency_ranges, the owner's list
of frequencies for dummy-load tests (...)`, `mode <m> is not in transmit.allowed_modes (...)`, `power_w <w> is above transmit.max_power_w <cap> (the owner's cap)`, `seconds <s> is above
transmit.max_seconds <cap>`. Pure: no I/O, no lock, nothing is sent. A present but non-numeric `frequency_hz`, `power_w` or `seconds` is reported as "must be a number" (only a missing one is "is required"), and the `reason` length rule counts characters. Example: `{"frequency_hz": 7040000, "power_w": 5, "seconds": 2, "reason": "tuner check",
"dry_run": true}` with a 7 to 7.1 MHz range and a 20 W, 2 s cap gives a plan; `power_w: 50` gives the cap error. See also: test
`a_test_is_refused_above_the_power_cap_outside_the_ranges_in_a_mode_not_allowed_or_too_long`; related anchors `radioctl_attest_config`, `radioctl_tx_preflight`, `radioctl_bounded`.

### `preflight` [@ANCHOR: radioctl_tx_preflight]

Reads the rig and refuses what the attestation, or the rig's own state, does not allow. It changes nothing: every command is a read. Called by `key_test` and `power_sweep` after `check_args` and
before the dry-run answer, so even a `dry_run` is checked against the real rig.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection to `radioctld`. |
| `tx` | `&Transmit` | The attestation (power cap, `allow_lowering_power`). |
| `a` | `&TestArgs` | The already-checked arguments. |

Return value: `Ok(Rig)`, what the rig said before anything was changed: `f_min` (lowest RFPOWER setting, fraction), `f_req` (the setting for the requested watts), `f_prev` (the setting now),
`w_prev` (watts of the setting now), `w_full` (watts at full scale), `prev_freq` (Hz), `prev_mode` (mode name and passband Hz, or `None` when the rig does not say). `Err(text)` when any of these
fail; every text ends with, or states, that nothing was sent. The checks in order: `dump_state` readable; the frequency inside one of the rig's own transmit ranges (the second band check); PTT
already on (refused; who keyed it must be found out, `bench_radioctl_safe_state` releases it); split on (the transmit frequency would be the other VFO's, which is not checked); XIT not zero (it
shifts the transmit frequency); the lowest power setting known (`rfpower_min`); the setting for the requested power not below that lowest setting; the requested setting, converted by the rig's full
scale, not over 1.02 times the owner's cap; the current setting readable, and not over 1.02 times the cap unless `allow_lowering_power` is true (then the tool lowers it and restores it afterwards). A
rig that refuses `get_ptt`, `get_split_vfo` or `get_xit` as unsupported is let through on that check (the refusal is data, not an error); a link failure on any read is an error. Blocking; one round
trip per read; the instrument lock is held by the caller. Never keys. A request for more watts than the rig's full scale (setting above 1.0) is refused here with "Nothing was sent", even when it is under the owner's cap. See also: test
`the_rig_is_read_first_and_a_split_xit_keyed_or_over_cap_rig_is_refused_without_a_byte_sent`; related anchors `radioctl_tx_args`, `radioctl_tx_min_power`, `radioctl_tx_power_convert`.

### `set_verified` [@ANCHOR: radioctl_tx_set_verified]

Sends one setting command to the rig and reads the value back, so that nothing is assumed accepted.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection. |
| `cmd` | `&str` | The set command text, for example `set_freq 7040000` or `set_level RFPOWER 0.25`. |
| `get` | `&str` | The matching read command, for example `get_freq` or `get_level RFPOWER`. |
| `key` | `Option<&str>` | The name of the value in the reply (for `get_freq`, `Frequency`), `None` for a bare number. |
| `want` | `f64` | The value expected back. |
| `tol` | `f64` | Allowed difference, in the unit of the value: 1 Hz for frequency, 0.01 (of full scale) for power. |
| `what` | `&str` | A short phrase for the error text. |

Return value: `Ok(())`. Errors: `<what>: <error>` when the set is refused, `<what> read back: <error>` when the read fails, `<what>: the rig reads back <value>, not <want>` when the difference is more
than `tol`. Blocking: two round trips. No lock beyond the caller's. It is used to set frequency, to set the power for each keyed interval and to restore the power afterwards; it does not key. See also:
test `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`; related anchors `radioctl_tx_restore`, `radioctl_tx_keyed`.

### `restore` [@ANCHOR: radioctl_tx_restore]

Puts the rig's power setting, mode and frequency back to what `preflight` read, after a test. It is called by `key_test` and `power_sweep` on every path after the load check and the operator alert have passed, whether the
test succeeded or not. Arguments: `c` (`&mut Client`) and `rig` (`&Rig`, the values read before). Return value: a `Vec<String>` of what could not be restored (empty when everything was): `power
setting <f_prev>` (`set_level RFPOWER` did not read back within 0.01), `mode <name>` (`set_mode <name> <passband>` was refused) or `frequency <hz> Hz` (`set_freq` refused). The order is power
first (the most important after a transmit), then mode, then frequency. Mode and frequency are not read back; a rig whose mode was not readable at the start (`prev_mode` is `None`) is left in the mode
of the test without saying so. The result goes into the tool's result as `rig_settings_not_restored`. No error is raised, so a failed restore does not hide the outcome of the test. Never keys. See
also: test `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`; related anchors `radioctl_tx_preflight`, `radioctl_tx_set_verified`.

### `summary` [@ANCHOR: radioctl_tx_summary]

Condenses the samples of a keyed interval into the numbers a person reads. Argument: `samples: &[Sample]`. Return value: a JSON object `{samples, power_w, swr, alc, temperature, not_reported}`.
Each of the four meter entries is `{min, max, mean}` rounded to two decimals, computed over only the samples that carry the reading, or `null` when no sample did; `not_reported` lists the names of
the meters that are `null` (`power_w`, `swr`, `alc`, `temperature`), which tells the reader a missing meter and not a good reading. `samples` is the count of samples taken. Units are those of the
rig's readings: watts for power (when a power meter exists), a ratio for SWR, the rig's own unit for ALC and temperature (unknown which; the code does not convert). Pure function, no I/O, no lock,
never panics (an empty slice gives all `null`). Used in the result of `key_test` (`keyed.meters`) and of each sweep step. See also: test
`a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`; related anchor `radioctl_tx_read_meters`.

### `operator_alert` [@ANCHOR: radioctl_tx_operator_alert]

Sounds the alert the owner configured, before the transmitter is keyed, and refuses the transmit when it cannot be heard. Called by `key_test` and `power_sweep` after the load check and before
anything is set on the rig.

| Argument | Type | Meaning |
|---|---|---|
| `tx` | `&Transmit` | The attestation; the field `operator_alert` says whether the owner wants an alert. |
| `deps` | `&Deps` | Its `alert` member is the `Alerter` (production: `AudioAlert`), or `None`. |
| `a` | `&TestArgs` | Frequency, seconds, power and reason, for the message. |
| `rig` | `&str` | Used in the message when there is no reason. |

Return value: `Ok(None)` when the owner did not ask for an alert (nothing is played); `Ok(Some(reply))` when the alert was sounded and the reply says `audible: true`. Errors (all mean nothing was sent):
the alert is on but there is no audio instrument to alert with; the alert call failed (`the operator alert failed (...); the owner configured an alert, so nothing was sent`); the reply was not
audible (`... was not audible (<error or "no sound">); ... Text to send by another channel: <text>`). The message is `Transmitting into the dummy load on <hz> for <s> seconds at <w> watts: <reason or rig
name>`. Side effects: sound through the audio instrument (in production). Blocking for the duration of the alert. `operator_alert` is in the list of known keys, so a config that sets `transmit.operator_alert` is accepted (an earlier version left it out and rejected such a config as `not a known key`; the config test now parses it). See also: test `a_key_test_is_refused_without_confirmation_without_a_reason_without_a_baseline_and_when_the_alert_cannot_be_heard`;
related anchors `radioctl_tx_alert`, `radioctl_attest_config`.

### `gate` [@ANCHOR: radioctl_tx_gate]

The load plausibility check used as a gate: the rig is listened to in receive, and unless the result is `Verdict::Consistent` the transmit is refused. It is the TX gate of the transmit tools. Called by
`key_test` and `power_sweep` after `confirm_dummy_load` and `reason` have been checked and before the operator alert.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection to the rig. |
| `inst` | `&Instrument` | The rig, for its config, its attestation (`load` settings) and its name. |
| `deps` | `&Deps` | `deps.audio` is the rig's audio source for the check (an audio level is part of it); it may be `None`. |
| `a` | `&TestArgs` | Gives the transmit frequency. |

It calls `loadtools::gate` (`radioctl_load_gate`), which measures with the rig in receive (S-meter readings at the frequency and at reference frequencies, and the audio level if there is an audio source)
and compares with the owner's recorded baseline. Return value: `Ok({verdict, compared_with_baseline, reasons, readings})` when the verdict is `consistent_with_a_load`. `Err(text)` otherwise: `not
transmitting: the load plausibility check says <refuse|no_baseline>: <reasons>. Next: ...`, where the next step for `no_baseline` is to record a baseline with the dummy load connected
(`bench_radioctl_load_baseline`) and for `refuse` is to look at what is on the antenna port (a dummy load does not hear time stations); errors from the measurement itself (a link failure, no baseline
file readable) pass through unchanged. The caller logs a `refused` line for an `Err`. The check never keys; it blocks for the dwell times of the measurement, with the lock held. It is evidence, not
proof, that a dummy load is connected. See also: test `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`; related anchors `radioctl_load_gate`, `radioctl_verdict_word`.

### `keyed` [@ANCHOR: radioctl_tx_keyed]

One keyed interval with everything around it set: sets the power, builds the `Keying` request from the owner's attestation and calls `tx::key`. It is used for the low-power probe, for the main interval of
the key test, and for each step of a sweep.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Connection. |
| `inst` | `&Instrument` | The rig; its attestation must exist (`attested`). |
| `deps` | `&Deps` | Supplies the watchdog `spawner`. |
| `rig` | `&Rig` | The values read by `preflight`: `f_min` becomes the minimum power for every release, `f_prev` the previous setting recorded in the state file, `w_full` the full-scale watts for the meter. |
| `a` | `&TestArgs` | Frequency, mode and reason for the log; `every_ms` for the sampling interval. |
| `seconds` | `f64` | The keyed time of this interval. |
| `f` | `f64` | The power setting to use, a fraction of full scale. |
| `kind`, `tool` | `&str` | `probe`, `test` or `sweep step`, and the calling tool's name, for the log. |

Steps: the attestation is fetched (error `this rig has no transmit attestation ...`); the daemon address (`rigd::endpoint`) and the directory are found; the setting in watts is computed for the
log (`fraction_to_watts` of `f` capped at 1); the setting is sent and read back with `set_verified` (tolerance 0.01; a failure here means nothing was keyed); then `tx::key` is called with the SWR
limit set to `transmit.max_swr`, the power limit to `transmit.max_power_w`, `by` set to the attestor, and the meter full scale. Return value and errors are those of `tx::key`, plus the errors above.
Blocking for the whole interval. Transmitter: this is where the owner's caps become the abort limits of the keyed interval. The tier, lock and band checks were done by the callers; it does not
repeat them. See also: test `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`; related anchors `radioctl_tx_key`, `radioctl_tx_probe`, `radioctl_tx_sweep`.

### `probe` [@ANCHOR: radioctl_tx_probe]

The low-power probe: keys the rig for a moment at its lowest power setting and reads the SWR, because an open or shorted connection receives nothing, so the load check cannot see it and only a
reflected-power reading can. It runs after the load check, the alert and the settings, and before the main interval of the key test and the first step of a sweep.

| Argument | Type | Meaning |
|---|---|---|
| `c`, `inst`, `deps`, `rig`, `a` | as for `keyed` | |
| `tool` | `&str` | The calling tool, for the log. |

Return value: `Ok({"done": false, "note": ...})` when the owner turned the probe off (`transmit.swr_probe: false`); no key-down happens then. Otherwise it keys at `rig.f_min` for
`transmit.swr_probe_s` seconds (0.1 to 2, default 0.5) and returns `Ok({"done": true, "swr": <worst reading, 2 decimals>, "power_setting": f_min})`. Errors, each meaning nothing further was sent: the probe
was stopped by an abort (`the low-power probe was stopped: <why> ...`); no usable SWR was reported at all (`the rig reported no usable SWR ...`, with the note that the dummy rig and some radios report 0
or nothing and the owner may turn the probe off and accept the risk); the worst SWR is above `transmit.max_swr` (default 2.0, allowed 1 to 5) (`... the load is open, shorted or wrong`); or any error of
`keyed`. This keys the transmitter at minimum power for up to two seconds. Same tier, lock and gate position as the key test, which called it. See also: test
`a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval`; related anchors `radioctl_tx_swr_valid`, `radioctl_tx_keyed`, `radioctl_tx_key_test`.

### `key_test` [@ANCHOR: radioctl_tx_key_test]

The body of the MCP tool `bench_radioctl_tx_key_test`: transmit for a few seconds into the attested dummy load at a stated power and frequency, read the meters, and put the rig back.

Control points, in order. **Tier:** the tool is `full`; `call` refuses it for an instrument below that, and the tool is listed only for a rig with a `transmit` block. **Lock:** `call` holds the
instrument's lock for the whole call, so no other tool touches the rig meanwhile except `safe_state`, which can end the keyed interval. **Band check and caps:** `check_args` (owner's frequency ranges,
modes, power and time caps) then `preflight` (the rig's own transmit range, PTT, split, XIT, power setting). **Dry run:** if `dry_run` is true the function returns the plan and sends nothing to the
rig that changes it. **Confirmation:** otherwise `confirm_dummy_load` must be true (the person at the bench says the antenna port is on the dummy load; the refusal names the cabling the owner attested) and
`reason` must be non-empty. **TX gate:** the load check (`gate`; a `refused` log line is written if it says no). Then the operator alert (when configured), then frequency (read back to within 1 Hz) and
mode (read back), the low-power probe, and the main interval (`keyed` with `rig.f_req`); then `restore` runs whether or not that worked. If anything after the gate failed a `stopped` log line is written and
the error returned.

| Argument (JSON key) | Type | Meaning |
|---|---|---|
| `instrument` | string | The rig's name (resolved by `call`). |
| `frequency_hz`, `mode`, `power_w`, `seconds`, `sample_every_ms`, `reason`, `dry_run`, `confirm_dummy_load` | | As in `check_args`. |

Return value: with `dry_run`: `{dry_run: true, sent: [], plan, would: [...six steps], note}`. Otherwise `{plan, load_check, operator_alert, swr_probe, keyed: {seconds, aborted, ptt_read_back_off, meters,
samples (at most 80)}, rig_settings_not_restored, keyed_interval_log (path), note}`. `aborted` is `null` or the reason the interval ended early (an SWR over the limit, a power meter far over the cap, the rig
un-keyed by something else). Errors (`String`): every refusal named above, each saying that nothing was sent, plus the errors of the called functions. The tool never leaves the rig keyed: all paths end in
`Keyed::release`, `Keyed::drop`, the watchdog process, or, at the latest, the next start's `startup_check`. Example call: `{"instrument":"bench-rig","frequency_hz":7040000,"power_w":5,"seconds":2,
"reason":"K6BP checking tuner","confirm_dummy_load":true}`. In tests nothing is transmitted: the rig is the simulated one, and the sound of the alert is recorded. See also: tests
`a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval` and, for the refusals, `a_key_test_is_refused_without_confirmation_without_a_reason_without_a_baseline_and_when_the_alert_cannot_be_heard`;
related anchors `radioctl_tx_sweep`, `radioctl_tx_gate`, `radioctl_tx_safe_state`.

### `power_sweep` [@ANCHOR: radioctl_tx_sweep]

The body of the MCP tool `bench_radioctl_tx_power_sweep`: the key test repeated at up to eight rising power levels, each step its own keyed interval with its own watchdog, stopping at the first step that
is aborted. Tier `full`; the same lock, attestation, band check, `confirm_dummy_load`, `reason`, load check, alert and probe as `key_test`.

| Argument (JSON key) | Type | Meaning |
|---|---|---|
| `powers_w` | array of numbers, required | 1 to 8 positive watt levels, strictly rising (lowest first). |
| `dwell_s` | number, default 1 | Seconds keyed at each level, 0.1 to 60, and at most `transmit.max_seconds`; `dwell_s` times the number of steps must not exceed 60. |
| `frequency_hz`, `mode`, `sample_every_ms`, `reason`, `dry_run`, `confirm_dummy_load` | | As in `check_args`. |

The checks of a single test are made for the highest step (`power_w` is the last of `powers_w`, `seconds` is `dwell_s`), so the owner's power cap applies to the whole sweep. Each step converts watts to a
setting (`watts_to_fraction`), raises it to the rig's minimum setting if it would be lower, and runs `keyed` with kind `sweep step`; the loop stops when a step reports an abort. Return value: with
`dry_run`: `{dry_run, sent: [], plan, steps_w, dwell_s, total_keyed_seconds, note}`; else `{plan, load_check, operator_alert, swr_probe, steps: [{power_w, setting, meters, aborted,
ptt_read_back_off}], rig_settings_not_restored, keyed_interval_log, note}`. Errors: the argument errors (`powers_w is required ...`, `powers_w takes 1 to 8 power levels`, `powers_w must rise`, `... make <t> s
of keyed time; the most in one call is 60 s`) and all those of `key_test`. The total keyed time of a sweep is limited to 60 s, and `transmit.max_seconds` limits each step, not the total (intended: each step is one keyed interval; a sweep may therefore key longer in total than one test). A step below the rig's minimum setting is raised to it; its entry says so (`raised_to_minimum: true`) and `setting` is the setting actually used. Like the key test, the sweep reads the mode back after setting it and does not key a rig that kept another mode. In tests the rig is simulated. See also: test
`a_power_sweep_keys_each_step_separately_stops_at_an_abort_and_obeys_the_same_gates`; related anchors `radioctl_tx_key_test`, `radioctl_tx_keyed`.

### `safe_state` [@ANCHOR: radioctl_tx_safe_state]

The body of `bench_radioctl_safe_state`: put the rig into its safe state (PTT off, read back; power setting to the minimum unless told not to), and say what it found. It is the safe-state companion of the keyed tools.
Tier `read_config` (lower than the transmit tools, so that someone who may not transmit can still release a rig). It is the one rig tool that does not wait for the instrument lock, so it runs while a keyed interval
is in progress and ends it: `tx::key` notices the un-keyed rig on its next sampling step and aborts.

| Argument (JSON key) | Type | Meaning |
|---|---|---|
| `instrument` | string | The rig. |
| `lower_power` | boolean or null, default true | Set the RFPOWER setting to the rig's lowest after the release. Another type is an error `lower_power must be true or false`. |

Steps: read the minimum setting from `dump_state`, the PTT state before, and the current RFPOWER (each tolerating a rig refusal; a link failure is an error); then up to three tries of `set_ptt 0` and a
`get_ptt` read-back (`Ok(true)` when it reads 0, "verified false" when the rig cannot report PTT). If three tries fail, a fresh connection is made with `guard::unkey` (which also sets the minimum power; up to eight
attempts); if that also fails the error is `SAFE STATE NOT CONFIRMED: ... The rig may be transmitting. Do this now: switch the rig off at its front panel or pull its power or its antenna feed. Do not use it again until
the owner has looked at it.` After a verified release it lowers the power (only when asked, and only when a minimum is known) and writes a `safe_state` log line. Return value: `{keyed_interval_was_running_here,
ptt: "receive", verified, ptt_was ("on", "off", "unknown"), power_setting_before, power_set_to_minimum, power_lowered_ok, note}`; `verified: false` with a note to look at the rig is the case of a rig that cannot
report PTT. Via the fresh-connection path the result is `{ptt: "receive", verified: true, via: "a new connection"}`. On the fresh-connection path the power is set to the minimum only when `lower_power` is true. Blocking, a few round trips. Transmitter: only un-keys; no gate applies. See also: test
`the_safe_state_unkeys_reads_it_back_lowers_the_power_and_works_while_another_call_runs`; related anchors `radioctl_tx_release`, `radioctl_tx_startup_check`, `radioctl_guard_unkey`.

## Watching the dial (`watch.rs`)

### `poll` [@ANCHOR: radioctl_watch_poll]

One poll of the values `bench_radioctl_watch` follows. Private to `watch.rs`. Argument: `c` (`&mut Client`, the connection). Return value: `Ok(Vec<(name, text)>)` with these entries, in this order: `frequency_hz`
(`get_freq`, the number printed in Hz with no decimals), `mode` and `passband_hz` (`get_mode`; the passband entry is present only when the mode was read), `vfo` (`get_vfo`) and `ptt` (`get_ptt`:
`receive` for 0, `TRANSMIT` for any other value, `unknown` when the reply has no `PTT` value). A value that the rig refuses (any `RPRT` error other than a link failure) is the text `unavailable`; a field missing
from a reply is `?`. `Err(text)` only when the link to `radioctld` fails (`tolerate` turns a link error into `Err`). Blocking: four round trips. Read only: it never sets anything and never keys. Code note: when `get_mode` is unavailable the list has no `passband_hz` entry, so it is one entry shorter than a poll where the mode was read; `watch` therefore compares two polls by entry name, not by position, and a poll where the mode appears or disappears reports only the entries that changed. See also: test
`the_watch_reports_the_changes_of_the_dial_with_their_times`; related anchors `radioctl_watch`, `radioctl_tolerate`.

### `watch` [@ANCHOR: radioctl_watch]

The body of `bench_radioctl_watch`: what the rig's dial does for a few seconds, found by polling. A person turns the knob, or software tunes the rig, and the changes of frequency, mode, VFO and PTT come back with their
times. `radioctld` has no event channel on this protocol, so the tool polls. Tier `read_only`; runs under the instrument lock; never sets anything and never keys (it can report that PTT is on, as `TRANSMIT`).

| Argument (JSON key) | Type | Meaning |
|---|---|---|
| `seconds` | number, default 10 | Time to watch, 1 to 120. |
| `interval_ms` | number, default 500 | Pause between polls, 100 to 5000 ms. |
| `c` | `&mut Client` | Connection. |

Out of range or non-numeric values are errors from `bounded`. A first poll is taken at once (`initial`); then the function sleeps `interval_ms` and polls again until `seconds` have elapsed (so the last poll can come up
to one interval, plus the polling time, after `seconds`; the result's `seconds` is the real elapsed time). Each difference between a poll and the previous one is an event `{t_s, what, from, to}` (`t_s` seconds since
the start, two decimals). At most 200 events are kept; more set `events_truncated: true`. Return value: `{seconds, polls, interval_ms, initial: {name: text, ...}, events, events_truncated, note}`, the note stating
that a change which reverts between two polls is not seen and that `frequency_hz` is a text of the reading in Hz. Errors: the argument errors, and a link failure in any poll (the events found so far are then lost, because
the error is returned with `?`; intended: a rig that stops answering is an error with its next step, not a partial result). Blocking: sleeps on the calling thread for up to the whole time, holding the instrument lock, so a long watch delays other calls to the same rig.
See also: test `the_watch_reports_the_changes_of_the_dial_with_their_times`; related anchors `radioctl_watch_poll`, `radioctl_bounded`.

## The rig's receive audio (`audio_in.rs`)

### `PipeWireInput::capture` [@ANCHOR: radioctl_audio_capture]

Takes a few seconds of the rig's receive audio from a PipeWire input node. It is the production implementation of `AudioSource::capture`; the receiver test (`bench_radioctl_rx_check`) and the load check
(`radioctl_load_gate`) use it, and tests give `AudioSource` recordings instead. Reading audio cannot transmit; it does not touch the rig.

| Argument | Type | Meaning |
|---|---|---|
| `seconds` | `f64` | How long to record; must be within `0.1..=30`. |
| `self.node` | `String` | The PipeWire node name of the input, found by `resolve`; the stream names it as its target. |

Return value: `Ok(Vec<f32>)` mono samples at 48 000 Hz (`RATE`), exactly `seconds * 48000` of them (truncated to that count). Errors, each a `String`: `<s> s is outside the 0.1 to 30 s a capture may last`;
`Recorder::start` failed (`pw-record` could not be started); fewer samples than needed arrived within `seconds + 4` s, or `pw-record` ended early (the text carries what it reported). The stream sets
`node.dont-fallback=true node.dont-reconnect=true`, so a vanished node is an error and never a move to the system default input (standing rule 2 of the module). Blocking: starts a `pw-record` child
process and waits for the audio, holding whatever lock the caller holds (the instrument lock). Side effects: the child process is stopped when the function returns. See also: test
`a_capture_has_bounds_and_a_missing_node_is_an_error_not_a_fall_back_to_the_default_input`; related anchors `radioctl_audio_resolve`, `bench_mcp_audio_engine_recorder_start`.

### `resolve` [@ANCHOR: radioctl_audio_resolve]

Finds the audio input of a rig from the names in the config. The rig entry's `audio` setting (`AudioRef`: `instrument`, `input`) names an `audio` instrument of the same bench config and the alias (or node name
fragment) of an input of it; the two modules share those names, and the device search and the refusal of reserved devices are done by the sound module's own code.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` | All the instruments of the bench config, borrowed. |
| `rig` | `&Instrument` | The radioctl instrument. |

Return value: `Ok(None)` when the rig has no `audio` setting (receive-audio features then work without audio or say they cannot); `Ok(Some(PipeWireInput))` when the input is found. Errors: `rig <name> names the
audio instrument <x>, which is not in the bench config` when no instrument of kind `audio` has that name; the error of the device listing (`devices::list` over `/proc/asound`, with the audio
instrument's settings); or the error of `devices::resolve` (no input matches, several match, or only a reserved device matches: such a device is never returned). Blocking: lists devices (the sound
module's discovery through `Process`; which commands it runs is unknown from this file). No lock of its own; `call` has the rig's lock. Never touches the rig. See also: test
`a_rig_names_its_audio_input_by_the_names_of_the_audio_instrument`; related anchors `radioctl_audio_capture`, `bench_mcp_audio_devices_resolve`.

### `required` [@ANCHOR: radioctl_tx_required]

`required(args, key, missing, own)` chooses the error text for a transmit-test argument that `bounded` rejected. If the key is absent or JSON null, the caller did not supply the value, so the answer is the "is required" text passed in `missing`; if a value is present but wrong (out of range, wrong type) the answer is `own`, the message `bounded` itself produced, which names the actual problem.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The tool arguments object. |
| `key` | `&str` | The argument name looked up. |
| `missing` | `&str` | The text returned when the argument is absent or null. |
| `own` | `String` | The text returned when a value is present. |

Returns a `String`; pure, no errors, no I/O. Test: `a_missing_value_gets_the_is_required_text_and_a_wrong_one_keeps_its_own`.

### `set_mode_verified` [@ANCHOR: radioctl_tx_set_mode_verified]

`set_mode_verified(c, mode)` sets the rig mode with `set_mode <mode> 0` and then reads it back with `get_mode`; the transmit tools call it before keying, so a rig that ignored the request is never keyed on the wrong mode.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&mut Client` | Open radioctld connection to the rig. |
| `mode` | `&str` | Upper-case mode name already validated against the allowed list. |

Returns `Ok(())` when the reported `Mode` equals `mode` (compared upper-case). Errors: `setting the mode: <rig error>` when the set fails, the read error text when the read fails, and `the rig reports mode ..., not ...; nothing was sent` when the rig kept another mode. It never keys. Test: `a_mode_the_rig_reports_back_is_accepted_and_a_different_one_is_refused`.
