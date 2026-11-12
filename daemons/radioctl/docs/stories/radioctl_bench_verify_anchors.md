# radioctl function anchors, bench verify tool

Function anchors for `tools/bench_verify.py`, the guided bench runner that works through the bench runbook against one attached rig: it detects what is attached, runs the automated checks over a generated `radioctld` config, asks the operator for the manual ones, and writes a report and a `verified.toml.proposed`. Transmit checks (phase C) are gated: they run only when the operator opened the gate and typed the confirmation, with a short transmit limit, and `transmit_failures` lists every reason they must be refused. Nothing here transmits on its own, and the unit tests use fake environments, no hardware.

## `tools/bench_verify.py`

### `def load_checks(text: str) -> list[Check]` [@ANCHOR: radioctl:radioctl_bench_load_checks]

Reads the index table and the `#### ID. title` sections of the runbook text. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def port_refusal(port: str) -> str | None` [@ANCHOR: radioctl:radioctl_bench_port_refusal]

Why a port may not be used, or None. A bare ttyUSBn/ttyACMn is refused: on the dev box ttyUSB0 is the SainSmart relay board and the ATM6301 supply shares the radio cables' CH340 ids, so only a by-id path, `usb:VID:PID[:SERIAL]`, a pty or `virtual` names a radio. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def run(self, argv: list[str], timeout: float = 60) -> tuple[int, str]` [@ANCHOR: radioctl:radioctl_bench_env_run]

Runs a program and returns (exit code, combined output); (127, reason) if it cannot start. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def spawn(self, argv: list[str]) -> subprocess.Popen` [@ANCHOR: radioctl:radioctl_bench_env_spawn]

Starts a long-running program (radioctld); the caller stops it. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def read_text(self, path: str) -> str` [@ANCHOR: radioctl:radioctl_bench_env_read_text]

Reads a text file, or '' if it is not there. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def ask(self, prompt: str) -> str` [@ANCHOR: radioctl:radioctl_bench_env_ask]

Asks the operator one line. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def detect(env: Env, selftest: str = "radioctl-selftest") -> dict` [@ANCHOR: radioctl:radioctl_bench_detect]

What is attached: `radioctl-selftest --list` text (no port is opened by it), the USB ids and by-id/pty-safe port names found in it, the sound cards, and which tools are installed. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def daemon_config(rig_id: str, port: str, baud: int, listen_port: int, transmit: bool, max_transmit_s: int) -> str` [@ANCHOR: radioctl:radioctl_bench_daemon_config]

The radioctld config for one rig: the gate is open (`transmit = true`) only when asked, with a short limit. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def cmd(self, line: str) -> list[str]` [@ANCHOR: radioctl:radioctl_bench_rigctl_cmd]

Sends one command and returns its reply lines (a get: its values; a set: `RPRT n`). Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def free_port() -> int` [@ANCHOR: radioctl:radioctl_bench_free_port]

A loopback TCP port nobody is listening on. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def start(self, wait_s: float = 8.0) -> Rigctl` [@ANCHOR: radioctl:radioctl_bench_daemon_start]

Starts radioctld on the generated config (checking it first) and connects. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def stop(self) -> None` [@ANCHOR: radioctl:radioctl_bench_daemon_stop]

Terminates what start() spawned; kills it if it does not exit. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def transmit_failures(opts: Options, results: dict[str, Result], typed: str | None) -> list[str]` [@ANCHOR: radioctl:radioctl_bench_transmit_failures]

Every reason a transmit step must be refused now. Empty means all preconditions hold. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def description_caps(rigs_dir: Path, rig: str) -> tuple[str, set[str]]` [@ANCHOR: radioctl:radioctl_bench_cap_set]

The display name and capability set of the rig description, or raises FileNotFoundError. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def run_all(self) -> list[Result]` [@ANCHOR: radioctl:radioctl_bench_run_all]

Phases A, B and C (and E, manual) for this rig, in runbook order. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def run_one(self, c: Check) -> Result` [@ANCHOR: radioctl:radioctl_bench_run_one]

One check: transmit checks are gated, automated ones run, the rest are asked of the operator. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def auto_a2(r: Runner, c: Check) -> Result` [@ANCHOR: radioctl:radioctl_bench_check_a2]

A2 (partly): the port is a safe name; `id -nG` lists dialout for a path port. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def auto_a5(r: Runner, c: Check) -> Result` [@ANCHOR: radioctl:radioctl_bench_check_a5]

A5: with the gate closed, T 1 is refused with RPRT -9 and the radio is not reported keyed. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def auto_a6(r: Runner, c: Check) -> Result` [@ANCHOR: radioctl:radioctl_bench_check_a6]

A6: radioctld --check on the generated config and the selftest listing. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def auto_b1(r: Runner, c: Check) -> Result` [@ANCHOR: radioctl:radioctl_bench_check_b1]

B1: the read-only self-test; it uploads automatically (its default) unless --no-upload was given for testing. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def auto_b2(r: Runner, c: Check) -> Result` [@ANCHOR: radioctl:radioctl_bench_check_b2]

B2/B14 reads: frequency, mode, ptt state through radioctld (nothing is set). Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def auto_c1(r: Runner, c: Check) -> Result` [@ANCHOR: radioctl:radioctl_bench_check_c1]

C1: a held connection keys with T 1, reads keyed, unkeys with T 0 (gate open, limited lease). Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def auto_c2(r: Runner, c: Check) -> Result` [@ANCHOR: radioctl:radioctl_bench_check_c2]

C2: key and wait past max_transmit_s; the daemon must have unkeyed by itself. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def render_report(opts: Options, name: str, results: list[Result], started: str, wire: list[str]) -> str` [@ANCHOR: radioctl:radioctl_bench_render_report]

The report in the layout of radioctl-selftest's: header, one line per check, summary, wire log. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def verified_proposal(opts: Options, name: str, results: list[Result], date: str) -> str` [@ANCHOR: radioctl:radioctl_bench_verified_proposal]

A `verified.toml.proposed`: comments naming what passed, and a [[report]] skeleton with placeholders. It is never merged into rigs/reports/verified.toml by this tool. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.

### `def verify(env: Env, opts: Options, runbook_text: str | None = None) -> int` [@ANCHOR: radioctl:radioctl_bench_verify]

The `verify` command: run, write the two files, return 0 if no check failed. Arguments and return value are as in the signature. Reads no hardware itself except through the injected environment; nothing here keys a transmitter unless the gated transmit checks are run.
