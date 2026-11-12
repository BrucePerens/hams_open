# Data structures of the radioctl tools

As a developer changing `daemons/radioctl/tools`, I want the data structures of `bench_verify.py` and `wire_to_transcript.py` explained in full (ADR 0113): what each is for, its fields, who builds it and how it is used. Each entry sits under the anchor on the definition in code; the tests cite the same anchors. Neither tool transmits by default and neither is part of the daemon.

### `Check` [@ANCHOR: radioctl:radioctl_bench_verify_check]

**Purpose and location.** One check of the bench-verification runbook (`docs/BENCH_VERIFICATION.md`): a row of its index table plus the text of its `#### ID. title` section. `daemons/radioctl/tools/bench_verify.py`, a dataclass built by `load_checks`.

| Field | Type | Meaning |
|---|---|---|
| `id` | `str` | The check id, a capital letter and digits such as `A5` or `C1`. |
| `title` | `str` | Its title. |
| `rig` | `str` | The index column naming the rigs the check applies to. |
| `phase` | `str` | `A`, `B` or `C`; phase C checks transmit. |
| `minutes` | `str` | The runbook's time estimate. |
| `present` | `str` | The index's "present" column (kept as text; what it records is defined by the runbook). |
| `source` | `str` | The index's source column. |
| `body` | `str` | The section text: the steps printed for the operator (default empty until the sections are read). |

**Property.** `transmits` is true for phase `C`. **Lifetime.** Built once per run from the runbook text; never changed after `load_checks` returns; not serialized.

### `Result` [@ANCHOR: radioctl:radioctl_bench_verify_result]

**Purpose and location.** The outcome of one check, recorded by the runner and written into the report and into `verified.toml.proposed`. `daemons/radioctl/tools/bench_verify.py`, a dataclass.

| Field | Type | Meaning |
|---|---|---|
| `check` | `str` | The check id. |
| `verdict` | `str` | `PASS`, `FAIL`, `NA`, `SKIPPED` or `REFUSED` (a transmit step that a safety precondition refused). |
| `detail` | `str` | A one-line reason or measurement. |
| `wire` | `list[str]` | The rigctl lines (with times) the check exchanged, for the report; empty by default. |

**Lifetime.** One per check, created by `Runner.record` and kept in `Runner.results`; the report renders them in order. Plain data, no locking.

### `Env` [@ANCHOR: radioctl:radioctl_bench_verify_env]

**Purpose and location.** Everything the tool does to the outside world, gathered in one class so tests replace it: running programs, starting radioctld, reading files, asking the operator, looking for tools and reading the clock. `daemons/radioctl/tools/bench_verify.py`.

**Definition.** `class Env` with no fields and the methods `run(argv, timeout)` (exit code and combined output; 127 and the reason when the program cannot start), `spawn(argv)` (a long-running process whose output is discarded; the caller stops it), `read_text(path)` (`''` when missing), `ask(prompt)`, `say(text)`, `which(name)` and `now()` (UTC). The test double `FakeEnv` overrides them.

**Safety.** The production `Env` touches the machine; it never transmits by itself, the checks decide that through radioctld and its gate. Not shared between threads.

### `Rigctl` [@ANCHOR: radioctl:radioctl_bench_verify_rigctl]

**Purpose and location.** A client of the rigctld text protocol that radioctld speaks: one line out, the reply lines back. `daemons/radioctl/tools/bench_verify.py`; returned by `Daemon.start`.

| Field | Type | Meaning |
|---|---|---|
| `sock` | `socket` | The TCP connection to the daemon (loopback by default, 3 s timeout). |
| `buf` | `bytes` | Received bytes not yet consumed as reply lines. |
| `log` | `list[str]` | Every command sent and line received with the monotonic time, copied into `Result.wire`. |

**Class constant.** `LINES` maps a command letter (`f`, `m`, `t`, `s`, `v`, `l`, `j`, `z`) to how many reply lines a get returns; any other command is read as a set whose reply is `RPRT n`. A command goes through `cmd(line)`. One connection, used by one thread.

### `Daemon` [@ANCHOR: radioctl:radioctl_bench_verify_daemon]

**Purpose and location.** One radioctld started for one phase of the verification and always stopped again. `daemons/radioctl/tools/bench_verify.py`.

| Field | Type | Meaning |
|---|---|---|
| `env` | `Env` | The environment used to run and spawn programs. |
| `proc` | `Popen` or `None` | The running daemon, `None` before `start` and after `stop`. |
| `port` | `int` | A free TCP port found for this daemon. |
| `cfg` | `Path` | The generated config: `bench-tx.toml` for a transmit phase, `bench-rx.toml` otherwise, written into the output directory. |
| `binary` | `str` | The radioctld program to run (`Options.radioctld`). |

**Behaviour.** `start` first runs the binary with `--check` on the generated config and refuses to go on if that fails, then starts it, waits up to `wait_s` seconds for the port and returns a `Rigctl`. The receive phase configures no transmit; a transmit phase carries `Options.max_transmit_s` as the daemon's own transmit limit. The config is generated from the options by `daemon_config` and holds no secret.

### `Options` [@ANCHOR: radioctl:radioctl_bench_verify_options]

**Purpose and location.** The command-line options of `bench_verify.py verify`, parsed once. `daemons/radioctl/tools/bench_verify.py`, a dataclass.

| Field | Type | Default | Meaning |
|---|---|---|---|
| `rig` | `str` | required | The rig description id (for example `icom-ic-7300`). |
| `port` | `str` | `virtual` | A `/dev/serial/by-id/...` path, `usb:VID:PID[:SERIAL]`, a pty or `virtual`; a bare `ttyUSBn` is refused by `port_refusal`. |
| `baud` | `int` | 19200 | The link speed. |
| `out_dir` | `Path` | current directory | Where the report and the proposal are written. |
| `only` | `list[str]` | empty | Run only these check ids. |
| `auto_only` | `bool` | false | Run only the automated checks. |
| `transmit` | `bool` | false | Allow transmit checks; still needs the typed `DUMMY LOAD ATTACHED` confirmation and every `transmit_failures` precondition. |
| `max_transmit_s` | `int` | 5 | Longest bounded key, seconds. |
| `no_upload` | `bool` | false | Pass `--no-upload` to the self-test (a testing convenience only). |
| `radioctld`, `selftest` | `str` | `radioctld`, `radioctl-selftest` | The programs used. |
| `rigs_dir` | `Path` | the crate's `rigs` directory | Where the descriptions are read. |

### `Runner` [@ANCHOR: radioctl:radioctl_bench_verify_runner]

**Purpose and location.** Runs the checks of one rig in order and collects the results. `daemons/radioctl/tools/bench_verify.py`.

| Field | Type | Meaning |
|---|---|---|
| `env`, `opts`, `checks` | `Env`, `Options`, `list[Check]` | What it runs against, with which options, and which checks. |
| `results` | `dict[str, Result]` | The result of each finished check by id. |
| `order` | `list[str]` | The ids in the order they finished. |
| `wire` | `list[str]` | All rigctl lines exchanged, in order, for the report. |
| `caps` | `set[str]` | The capabilities the rig's description declares (read from the description when the run starts). |
| `typed` | `str` or `None` | What the operator typed at the dummy-load confirmation, asked for once, on the first transmit check. |

**Behaviour.** `run_all` goes through the checks, `run_one` runs one (an automated check, or the printed steps with the operator's answer), `record` stores a result. A transmit check is a dry run unless `Options.transmit` is set, the confirmation text matches and `transmit_failures(opts, results, typed)` is empty (it lists unmet preconditions such as the earlier checks that must have passed); otherwise it is refused with the reason as a `REFUSED` result. The runner never edits a description or `verified.toml`.

### `Frame` [@ANCHOR: radioctl:radioctl_wire_frame]

**Purpose and location.** One line of a `bench_console --wire` capture: a read or write on the link. `daemons/radioctl/tools/wire_to_transcript.py`, a dataclass built by `parse_wire` from lines `SECONDS tx|rx HEX`.

| Field | Type | Meaning |
|---|---|---|
| `time` | `float` | Seconds on the capture's clock. |
| `direction` | `str` | `tx` (written to the rig) or `rx` (read from it). |
| `data` | `bytes` | The bytes, decoded from the hexadecimal. |

### `Command` [@ANCHOR: radioctl:radioctl_wire_command]

**Purpose and location.** One console command from the `--log` file, paired later with its result. `daemons/radioctl/tools/wire_to_transcript.py`, a dataclass built by `parse_log` from the console's `SECONDS > <command>` lines.

| Field | Type | Meaning |
|---|---|---|
| `time` | `float` | When the command was entered. |
| `text` | `str` | The command text, for example `setfreq a 7074000`. |
| `done` | `float` or `None` | When its `=` result line was printed; `None` until one is seen. |
| `result` | `str` | The result text (`ok`, `Frequency(A, Hz(14074000))`, an error) from the `=` line; empty until seen. |

### `Exchange` [@ANCHOR: radioctl:radioctl_wire_exchange]

**Purpose and location.** One `tx` frame and the `rx` bytes that followed it before the next `tx`. `daemons/radioctl/tools/wire_to_transcript.py`, a dataclass built by `exchanges` from the frames.

| Field | Type | Meaning |
|---|---|---|
| `time` | `float` | When the `tx` frame was written. |
| `tx` | `bytes` | The bytes written. |
| `rx` | `bytes` | The bytes read before the next `tx` (empty if none). |
| `rx_time` | `float` | When the last of those bytes arrived (0.0 if none). |
| `owner` | `Command` or `None` | The console command this exchange is the request of, set when commands are paired with exchanges; `None` for the actor's own polls. |

**Use.** The converter pairs each command with the last exchange answered before its result was printed and started after the command line, then writes the `[[step]]` transcript `conformance::replay` reads. Unowned exchanges and unsolicited `rx` bytes are listed as comments, not turned into steps. Redaction (`--redact HEX`) replaces the given bytes with zeros in every frame before they are written.
