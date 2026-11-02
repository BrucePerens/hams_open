#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Guided bench verification of one radio: `bench_verify.py verify <rig-id>`.

The checks, their order, steps and what each unlocks are those of `docs/BENCH_VERIFICATION.md` (this tool
reads that file; it keeps no second copy). For a rig the tool

* detects what is attached (reusing `radioctl-selftest --list`, which opens no port, and the sound
  cards in `/proc/asound/cards`) and prints it;
* walks the checks of phases A, B and C: it runs the ones it can automate (the gate check, the discovery
  list, the read-only self-test, reads through radioctld, a bounded PTT key and the watchdog) and for the
  rest prints the runbook steps and records the operator's answer (pass, fail, skip);
* **never transmits by default.** A transmit check is a dry run (it prints what it would do) unless
  `--transmit` is given AND the operator types `DUMMY LOAD ATTACHED` AND every safety precondition holds
  (see `transmit_failures`); if one fails the transmit step is refused and the reason is reported;
* writes `BENCH-REPORT-<rig>-<date>.txt` in the layout of `radioctl-selftest`'s report and
  `verified.toml.proposed` (a proposal only: it never edits `rigs/reports/verified.toml` or a description).

The tool's own report and proposal are never uploaded. The `radioctl-selftest` step follows the self-test's
default (Bruce, 2026-10-31): it uploads its report to hams.com automatically, with no prompt. `--no-upload`
is passed through only as a testing convenience (virtual-rig unit tests, offline benches).

Usage:
    bench_verify.py list
    bench_verify.py detect
    bench_verify.py verify RIG_ID [--port P] [--baud N] [--out-dir D] [--only A5,B2] [--auto-only]
                                  [--transmit] [--max-transmit-s N] [--no-upload]
"""
from __future__ import annotations

import argparse
import datetime
import re
import shutil
import socket
import subprocess
import sys
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

TOOL_DIR = Path(__file__).resolve().parent
RADIOCTL_DIR = TOOL_DIR.parent
RUNBOOK = RADIOCTL_DIR / "docs" / "BENCH_VERIFICATION.md"
CONFIRM_PHRASE = "DUMMY LOAD ATTACHED"
MAX_TRANSMIT_CAP_S = 10
# What a transmit check needs to have passed in this run first (runbook phase A and the read-only phase).
TRANSMIT_PRECONDITIONS = ("A1", "A3", "A4", "A5", "B1", "B2")
BARE_TTY = re.compile(r"^/dev/tty(USB|ACM)\d+$")
INDEX_ID = re.compile(r"^[A-Z]\d+$")
SECTION = re.compile(r"^#### ([A-Z]\d+)\. (.+)$")


@dataclass
class Check:
    """One row of the runbook index plus its section text."""

    id: str
    title: str
    rig: str
    phase: str
    minutes: str
    present: str
    source: str
    body: str = ""

    @property
    def transmits(self) -> bool:
        return self.phase == "C"


@dataclass
class Result:
    """The outcome of one check: PASS, FAIL, NA, SKIPPED or REFUSED, with a one-line detail."""

    check: str
    verdict: str
    detail: str
    wire: list[str] = field(default_factory=list)


# [@ANCHOR: radioctl_bench_load_checks]
def load_checks(text: str) -> list[Check]:
    """Reads the index table and the `#### ID. title` sections of the runbook text."""
    checks: dict[str, Check] = {}
    order: list[str] = []
    for line in text.splitlines():
        cells = re.split(r"\s*\|\s*", line.strip()) if line.startswith("| ") else []
        if len(cells) != 9 or not INDEX_ID.match(cells[1]) or not re.fullmatch(r"[A-Z]", cells[4]):
            continue
        cid, title, rig, phase, minutes, present, source = cells[1:8]
        checks[cid] = Check(cid, title, rig, phase, minutes, present, source)
        order.append(cid)
    current: str | None = None
    bodies: dict[str, list[str]] = {}
    for line in text.splitlines():
        s = SECTION.match(line)
        if s:
            current = s.group(1)
            bodies[current] = []
        elif line.startswith("#"):
            current = None
        elif current:
            bodies[current].append(line)
    for cid, lines in bodies.items():
        if cid in checks:
            checks[cid].body = "\n".join(lines).strip()
    return [checks[c] for c in order]


# [@ANCHOR: radioctl_bench_port_refusal]
def port_refusal(port: str) -> str | None:
    """Why a port may not be used, or None. A bare ttyUSBn/ttyACMn is refused: on the dev box ttyUSB0 is the
    SainSmart relay board and the ATM6301 supply shares the radio cables' CH340 ids, so only a by-id path,
    `usb:VID:PID[:SERIAL]`, a pty or `virtual` names a radio."""
    if BARE_TTY.match(port):
        return f"{port} is a bare tty name; use /dev/serial/by-id/... or usb:VID:PID (a CH340 board may sit on it)"
    if port == "virtual" or port.startswith(("/dev/serial/by-id/", "/dev/pts/")):
        return None
    if re.fullmatch(r"usb:[0-9a-fA-F]{4}:[0-9a-fA-F]{4}(:\S+)?", port):
        return None
    return f"{port!r} is not a by-id path, usb:VID:PID, a pty or 'virtual'"


class Env:
    """Everything the tool does to the outside world, so tests can replace it."""

    # [@ANCHOR: radioctl_bench_env_run]
    def run(self, argv: list[str], timeout: float = 60) -> tuple[int, str]:
        """Runs a program and returns (exit code, combined output); (127, reason) if it cannot start."""
        try:
            p = subprocess.run(argv, capture_output=True, text=True, timeout=timeout, check=False)
        except (OSError, subprocess.TimeoutExpired) as e:
            return 127, str(e)
        return p.returncode, p.stdout + p.stderr

    # [@ANCHOR: radioctl_bench_env_spawn]
    def spawn(self, argv: list[str]) -> subprocess.Popen:
        """Starts a long-running program (radioctld); the caller stops it."""
        return subprocess.Popen(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    # [@ANCHOR: radioctl_bench_env_read_text]
    def read_text(self, path: str) -> str:
        """Reads a text file, or '' if it is not there."""
        try:
            return Path(path).read_text()
        except OSError:
            return ""

    # [@ANCHOR: radioctl_bench_env_ask]
    def ask(self, prompt: str) -> str:
        """Asks the operator one line."""
        return input(prompt)

    def say(self, text: str = "") -> None:
        print(text, flush=True)

    def which(self, name: str) -> str | None:
        return shutil.which(name)

    def now(self) -> datetime.datetime:
        return datetime.datetime.now(datetime.timezone.utc)


# [@ANCHOR: radioctl_bench_detect]
def detect(env: Env, selftest: str = "radioctl-selftest") -> dict:
    """What is attached: `radioctl-selftest --list` text (no port is opened by it), the USB ids and
    by-id/pty-safe port names found in it, the sound cards, and which tools are installed."""
    rc, listing = env.run([selftest, "--list"], 20)
    ports = sorted(set(re.findall(r"/dev/(?:serial/by-id/\S+|tty(?:USB|ACM)\d+)", listing)))
    usb_ids = sorted(set(re.findall(r"\b([0-9a-f]{4}:[0-9a-f]{4})\b", listing)))
    cards = [ln.strip() for ln in env.read_text("/proc/asound/cards").splitlines() if re.match(r"\s*\d+ \[", ln)]
    tools = {t: bool(env.which(t)) for t in ("radioctld", "radioctl-selftest", "bench_console", "rigctl", "nc")}
    return {"selftest_rc": rc, "listing": listing, "ports": ports, "usb_ids": usb_ids, "audio": cards, "tools": tools}


# [@ANCHOR: radioctl_bench_daemon_config]
def daemon_config(rig_id: str, port: str, baud: int, listen_port: int, transmit: bool, max_transmit_s: int) -> str:
    """The radioctld config for one rig: the gate is open (`transmit = true`) only when asked, with a short limit."""
    if transmit and not 1 <= max_transmit_s <= MAX_TRANSMIT_CAP_S:
        raise ValueError(f"max_transmit_s must be 1 to {MAX_TRANSMIT_CAP_S} for a bench transmit")
    lines = ["[server]", 'allow = ["127.0.0.0/8"]', "", "[[rig]]", f'name = "{rig_id}"',
             f'description = "rigs/{rig_id}.toml"', f'listen = "127.0.0.1:{listen_port}"',
             f"transmit = {'true' if transmit else 'false'}"]
    if transmit:
        lines.append(f"max_transmit_s = {max_transmit_s}")
    lines += ["", "[rig.transport]"]
    if port == "virtual":
        lines.append('kind = "virtual"')
    else:
        lines.append('kind = "serial"')
        m = re.fullmatch(r"usb:([0-9a-fA-F]{4}):([0-9a-fA-F]{4})(?::(\S+))?", port)
        if m:
            ser = f', serial = "{m.group(3)}"' if m.group(3) else ""
            lines.append(f"usb = {{ vid = 0x{m.group(1)}, pid = 0x{m.group(2)}{ser} }}")
        else:
            lines.append(f'path = "{port}"')
        lines.append(f"baud = {baud}")
        if port.startswith("/dev/pts/"):
            lines.append('lines = "unmanaged"')
    return "\n".join(lines) + "\n"


class Rigctl:
    """A client of the rigctld text protocol (radioctld speaks it); one line out, the reply lines back."""

    LINES = {"f": 1, "m": 2, "t": 1, "s": 1, "v": 1, "l": 1, "j": 1, "z": 1}

    def __init__(self, port: int, host: str = "127.0.0.1", timeout: float = 3.0):
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.buf = b""
        self.log: list[str] = []

    # [@ANCHOR: radioctl_bench_rigctl_cmd]
    def cmd(self, line: str) -> list[str]:
        """Sends one command and returns its reply lines (a get: its values; a set: `RPRT n`)."""
        self.sock.sendall(line.encode() + b"\n")
        self.log.append(f"{time.monotonic():.3f} tx {line}")
        want = self.LINES.get(line.split()[0].lstrip("\\"), 0)
        out: list[str] = []
        while True:
            text = self._line()
            if text is None:
                break
            out.append(text)
            self.log.append(f"{time.monotonic():.3f} rx {text}")
            if text.startswith("RPRT") or (want and len(out) >= want):
                break
        return out

    def _line(self) -> str | None:
        while b"\n" not in self.buf:
            try:
                chunk = self.sock.recv(4096)
            except OSError:
                return None
            if not chunk:
                return None
            self.buf += chunk
        text, self.buf = self.buf.split(b"\n", 1)
        return text.decode(errors="replace").strip()

    def close(self) -> None:
        self.sock.close()


# [@ANCHOR: radioctl_bench_free_port]
def free_port() -> int:
    """A loopback TCP port nobody is listening on."""
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class Daemon:
    """One radioctld started for one phase and always stopped again."""

    def __init__(self, env: Env, cfg_dir: Path, args: "Options", transmit: bool):
        self.env = env
        self.proc: subprocess.Popen | None = None
        self.port = free_port()
        self.cfg = cfg_dir / ("bench-tx.toml" if transmit else "bench-rx.toml")
        self.cfg.write_text(daemon_config(args.rig, args.port, args.baud, self.port, transmit, args.max_transmit_s))
        self.binary = args.radioctld

    # [@ANCHOR: radioctl_bench_daemon_start]
    def start(self, wait_s: float = 8.0) -> Rigctl:
        """Starts radioctld on the generated config (checking it first) and connects."""
        rc, out = self.env.run([self.binary, "--config", str(self.cfg), "--check"], 20)
        if rc != 0:
            raise RuntimeError(f"radioctld --check failed: {out.strip()[:200]}")
        self.proc = self.env.spawn([self.binary, "--config", str(self.cfg)])
        end = time.monotonic() + wait_s
        while time.monotonic() < end:
            try:
                return Rigctl(self.port)
            except OSError:
                time.sleep(0.1)
        self.stop()
        raise RuntimeError("radioctld did not start listening")

    # [@ANCHOR: radioctl_bench_daemon_stop]
    def stop(self) -> None:
        """Terminates what start() spawned; kills it if it does not exit."""
        if self.proc is None:
            return
        self.proc.terminate()
        try:
            self.proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            self.proc.wait()
        self.proc = None


@dataclass
class Options:
    rig: str
    port: str = "virtual"
    baud: int = 19200
    out_dir: Path = Path(".")
    only: list[str] = field(default_factory=list)
    auto_only: bool = False
    transmit: bool = False
    max_transmit_s: int = 5
    no_upload: bool = False  # testing convenience only; the self-test uploads by default
    radioctld: str = "radioctld"
    selftest: str = "radioctl-selftest"
    rigs_dir: Path = RADIOCTL_DIR / "rigs"


# [@ANCHOR: radioctl_bench_transmit_failures]
def transmit_failures(opts: Options, results: dict[str, Result], typed: str | None) -> list[str]:
    """Every reason a transmit step must be refused now. Empty means all preconditions hold."""
    out: list[str] = []
    if not opts.transmit:
        out.append("--transmit was not given (dry run)")
    if typed != CONFIRM_PHRASE:
        out.append(f"the phrase {CONFIRM_PHRASE!r} was not typed")
    for cid in TRANSMIT_PRECONDITIONS:
        r = results.get(cid)
        if r is None or r.verdict != "PASS":
            out.append(f"precondition {cid} has not passed in this run")
    if not 1 <= opts.max_transmit_s <= MAX_TRANSMIT_CAP_S:
        out.append(f"max_transmit_s {opts.max_transmit_s} is outside 1 to {MAX_TRANSMIT_CAP_S}")
    refusal = port_refusal(opts.port)
    if refusal:
        out.append(refusal)
    return out


# [@ANCHOR: radioctl_bench_cap_set]
def description_caps(rigs_dir: Path, rig: str) -> tuple[str, set[str]]:
    """The display name and capability set of the rig description, or raises FileNotFoundError."""
    data = tomllib.loads((rigs_dir / f"{rig}.toml").read_text())
    return data.get("rig", {}).get("name", rig), set(data.get("capabilities", []))


class Runner:
    """Runs the checks of one rig in order and collects results."""

    def __init__(self, env: Env, opts: Options, checks: list[Check]):
        self.env, self.opts, self.checks = env, opts, checks
        self.results: dict[str, Result] = {}
        self.order: list[str] = []
        self.wire: list[str] = []
        self.caps: set[str] = set()
        self.typed: str | None = None

    def record(self, r: Result) -> None:
        self.results[r.check] = r
        self.order.append(r.check)
        self.wire += r.wire
        self.env.say(f"{r.verdict:<8} {r.check}: {r.detail}")

    # [@ANCHOR: radioctl_bench_run_all]
    def run_all(self) -> list[Result]:
        """Phases A, B and C (and E, manual) for this rig, in runbook order."""
        self.env.say(f"bench verify {self.opts.rig} on {self.opts.port}")
        refusal = port_refusal(self.opts.port)
        if refusal:
            raise SystemExit(f"refused: {refusal}")
        _, self.caps = description_caps(self.opts.rigs_dir, self.opts.rig)
        for c in self.checks:
            if c.phase not in "ABCE" or (self.opts.only and c.id not in self.opts.only):
                continue
            if c.rig not in ("any", "none") and not self._rig_matches(c):
                continue
            self.record(self.run_one(c))
        return [self.results[i] for i in self.order]

    def _rig_matches(self, c: Check) -> bool:
        tag = {"IC": "icom-ic-7300", "F63": "flex-6300", "KX3": "elecraft-kx3"}.get(c.rig, "")
        return self.opts.rig == tag

    # [@ANCHOR: radioctl_bench_run_one]
    def run_one(self, c: Check) -> Result:
        """One check: transmit checks are gated, automated ones run, the rest are asked of the operator."""
        self.env.say(f"\n== {c.id} {c.title} ({c.minutes} min, present: {c.present or 'no'})")
        if c.transmits:
            return self._transmit(c)
        auto = AUTOMATED.get(c.id)
        if auto:
            try:
                return auto(self, c)
            except (OSError, RuntimeError, ValueError) as e:
                return Result(c.id, "FAIL", f"{e}")
        return self._manual(c)

    def _manual(self, c: Check) -> Result:
        if self.opts.auto_only:
            return Result(c.id, "SKIPPED", "manual check, --auto-only")
        self.env.say(c.body or c.title)
        ans = self.env.ask(f"{c.id}: result [p]ass / [f]ail / [s]kip? ").strip().lower()[:1]
        verdict = {"p": "PASS", "f": "FAIL"}.get(ans, "SKIPPED")
        note = "operator answered" if verdict != "SKIPPED" else "operator skipped"
        if verdict == "PASS":
            note += " pass (manual, not machine-checked)"
        return Result(c.id, verdict, note)

    def _transmit(self, c: Check) -> Result:
        self.env.say(c.body or c.title)
        if not self.opts.transmit:
            return Result(c.id, "SKIPPED", "dry run: transmit not requested; the steps above were not executed")
        if self.typed is None:
            self.typed = self.env.ask(f"Type {CONFIRM_PHRASE} to confirm a dummy load is on the antenna port: ").strip()
        failures = transmit_failures(self.opts, self.results, self.typed)
        if failures:
            return Result(c.id, "REFUSED", "; ".join(failures))
        auto = AUTOMATED.get(c.id)
        if auto:
            try:
                return auto(self, c)
            except (OSError, RuntimeError, ValueError) as e:
                return Result(c.id, "FAIL", f"{e}")
        return self._manual(c)

    def with_daemon(self, transmit: bool, fn):
        """Runs fn(Rigctl) against a fresh radioctld and always stops the daemon."""
        self.opts.out_dir.mkdir(parents=True, exist_ok=True)
        d = Daemon(self.env, self.opts.out_dir, self.opts, transmit)
        try:
            client = d.start()
            try:
                return fn(client)
            finally:
                client.close()
        finally:
            d.stop()


# [@ANCHOR: radioctl_bench_check_a2]
def auto_a2(r: Runner, c: Check) -> Result:
    """A2 (partly): the port is a safe name; `id -nG` lists dialout for a path port."""
    p = r.opts.port
    if p.startswith("/dev/serial/by-id/"):
        rc, groups = r.env.run(["id", "-nG"], 5)
        if "dialout" not in groups.split():
            return Result(c.id, "FAIL", "this account is not in dialout")
    return Result(c.id, "PASS", f"port {p} is an acceptable name")


# [@ANCHOR: radioctl_bench_check_a5]
def auto_a5(r: Runner, c: Check) -> Result:
    """A5: with the gate closed, T 1 is refused with RPRT -9 and the radio is not reported keyed."""
    def go(cl: Rigctl) -> Result:
        refused = cl.cmd("T 1")
        state = cl.cmd("t")
        ok = refused == ["RPRT -9"] and state[:1] == ["0"]
        return Result(c.id, "PASS" if ok else "FAIL", f"T 1 -> {refused}, t -> {state}", cl.log[:])
    return r.with_daemon(False, go)


# [@ANCHOR: radioctl_bench_check_a6]
def auto_a6(r: Runner, c: Check) -> Result:
    """A6: radioctld --check on the generated config and the selftest listing."""
    cfg = r.opts.out_dir
    cfg.mkdir(parents=True, exist_ok=True)
    path = cfg / "bench-check.toml"
    path.write_text(daemon_config(r.opts.rig, r.opts.port, r.opts.baud, free_port(), False, r.opts.max_transmit_s))
    rc, out = r.env.run([r.opts.radioctld, "--config", str(path), "--check"], 20)
    info = detect(r.env, r.opts.selftest)
    ok = rc == 0 and info["selftest_rc"] == 0
    return Result(c.id, "PASS" if ok else "FAIL", f"--check rc {rc}; ports {info['ports']}; audio {len(info['audio'])}; {out.strip()[:80]}")


# [@ANCHOR: radioctl_bench_check_b1]
def auto_b1(r: Runner, c: Check) -> Result:
    """B1: the read-only self-test; it uploads automatically (its default) unless --no-upload was given for testing."""
    argv = [r.opts.selftest, "--description", r.opts.rig, "--port", r.opts.port, "--baud", str(r.opts.baud),
            "--out-dir", str(r.opts.out_dir)]
    if r.opts.no_upload:
        argv.append("--no-upload")
    rc, out = r.env.run(argv, 180)
    return Result(c.id, "PASS" if rc == 0 else "FAIL", f"radioctl-selftest exit {rc}; {out.strip().splitlines()[-1][:100] if out.strip() else ''}")


# [@ANCHOR: radioctl_bench_check_b2]
def auto_b2(r: Runner, c: Check) -> Result:
    """B2/B14 reads: frequency, mode, ptt state through radioctld (nothing is set)."""
    def go(cl: Rigctl) -> Result:
        f, m, t = cl.cmd("f"), cl.cmd("m"), cl.cmd("t")
        ok = bool(f and f[0].isdigit()) and len(m) == 2 and t[:1] == ["0"]
        return Result(c.id, "PASS" if ok else "FAIL", f"f {f} m {m} t {t}", cl.log[:])
    return r.with_daemon(False, go)


# [@ANCHOR: radioctl_bench_check_c1]
def auto_c1(r: Runner, c: Check) -> Result:
    """C1: a held connection keys with T 1, reads keyed, unkeys with T 0 (gate open, limited lease)."""
    def go(cl: Rigctl) -> Result:
        on = cl.cmd("T 1")
        time.sleep(0.5)
        keyed = cl.cmd("t")
        off = cl.cmd("T 0")
        after = cl.cmd("t")
        ok = on == ["RPRT 0"] and keyed[:1] == ["1"] and off == ["RPRT 0"] and after[:1] == ["0"]
        return Result(c.id, "PASS" if ok else "FAIL", f"T 1 {on} t {keyed} T 0 {off} t {after}", cl.log[:])
    return r.with_daemon(True, go)


# [@ANCHOR: radioctl_bench_check_c2]
def auto_c2(r: Runner, c: Check) -> Result:
    """C2: key and wait past max_transmit_s; the daemon must have unkeyed by itself."""
    def go(cl: Rigctl) -> Result:
        on = cl.cmd("T 1")
        time.sleep(r.opts.max_transmit_s + 1.5)
        state = cl.cmd("t")
        if state[:1] != ["0"]:
            cl.cmd("T 0")
            return Result(c.id, "FAIL", f"still keyed after {r.opts.max_transmit_s + 1.5:.1f} s", cl.log[:])
        return Result(c.id, "PASS" if on == ["RPRT 0"] else "FAIL", f"T 1 {on}; unkeyed by the watchdog", cl.log[:])
    return r.with_daemon(True, go)


AUTOMATED = {"A2": auto_a2, "A5": auto_a5, "A6": auto_a6, "B1": auto_b1, "B2": auto_b2, "B14": auto_b2,
             "C1": auto_c1, "C2": auto_c2}


# [@ANCHOR: radioctl_bench_render_report]
def render_report(opts: Options, name: str, results: list[Result], started: str, wire: list[str]) -> str:
    """The report in the layout of radioctl-selftest's: header, one line per check, summary, wire log."""
    n = lambda v: sum(1 for r in results if r.verdict == v)  # noqa: E731
    lines = ["radioctl bench verification report (bench_verify.py; local file, never uploaded by this tool)",
             f"system: {sys.platform}", f"started: {started}", f"description: {opts.rig} ({name})",
             f"target: {opts.port} at {opts.baud} baud",
             f"transmit requested: {'yes' if opts.transmit else 'no (dry run)'}", ""]
    word = {"PASS": "PASS", "FAIL": "DID NOT PASS", "NA": "N/A", "SKIPPED": "SKIPPED (dry run or skipped)", "REFUSED": "REFUSED"}
    for r in results:
        lines.append(f"{word.get(r.verdict, r.verdict)} {r.check}: {r.detail}")
    lines += ["", f"summary: {n('PASS')} pass, {n('FAIL')} did not pass, {n('REFUSED')} refused, {n('SKIPPED')} skipped",
              "", "wire log (monotonic seconds, tx = sent to radioctld, rx = received):"] + wire
    return "\n".join(lines) + "\n"


# [@ANCHOR: radioctl_bench_verified_proposal]
def verified_proposal(opts: Options, name: str, results: list[Result], date: str) -> str:
    """A `verified.toml.proposed`: comments naming what passed, and a [[report]] skeleton with placeholders.
    It is never merged into rigs/reports/verified.toml by this tool."""
    passed = [r.check for r in results if r.verdict == "PASS"]
    tx = any(r.verdict == "PASS" and r.check.startswith("C") for r in results)
    return (f"# PROPOSAL from bench_verify.py, not applied. Passed checks: {', '.join(passed) or 'none'}.\n"
            "# A maintainer flips the facts those checks exercise (runbook, 'Unlocks') to hardware-verified in\n"
            f"# rigs/{opts.rig}.toml and then adds this entry to rigs/reports/verified.toml.\n"
            "[[report]]\n"
            f'rig = "{opts.rig}"\nradio = "{name}"\nfirmware = "FILL IN FROM THE RADIO MENU"\ndate = "{date}"\n'
            'reporter = "FILL IN"\nissue = "FILL IN"\n'
            f"transmit_tested = {'true' if tx else 'false'}\n")


# [@ANCHOR: radioctl_bench_verify]
def verify(env: Env, opts: Options, runbook_text: str | None = None) -> int:
    """The `verify` command: run, write the two files, return 0 if no check failed."""
    text = runbook_text if runbook_text is not None else RUNBOOK.read_text()
    runner = Runner(env, opts, load_checks(text))
    started = env.now().strftime("%Y-%m-%dT%H:%M:%SZ")
    results = runner.run_all()
    name, _ = description_caps(opts.rigs_dir, opts.rig)
    date = started[:10]
    opts.out_dir.mkdir(parents=True, exist_ok=True)
    report = opts.out_dir / f"BENCH-REPORT-{opts.rig}-{date}.txt"
    report.write_text(render_report(opts, name, results, started, runner.wire))
    (opts.out_dir / "verified.toml.proposed").write_text(verified_proposal(opts, name, results, date))
    env.say(f"\nreport: {report}\nproposal: {opts.out_dir / 'verified.toml.proposed'} (this tool uploads nothing)")
    return 1 if any(r.verdict == "FAIL" for r in results) else 0


def main(argv: list[str] | None = None, env: Env | None = None) -> int:
    env = env or Env()
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list")
    sub.add_parser("detect")
    v = sub.add_parser("verify")
    v.add_argument("rig")
    v.add_argument("--port", default="virtual")
    v.add_argument("--baud", type=int, default=19200)
    v.add_argument("--out-dir", type=Path, default=Path("."))
    v.add_argument("--only", default="")
    v.add_argument("--auto-only", action="store_true")
    v.add_argument("--transmit", action="store_true")
    v.add_argument("--max-transmit-s", type=int, default=5)
    v.add_argument("--no-upload", action="store_true", help="testing convenience: pass --no-upload to radioctl-selftest (virtual rigs, offline benches)")
    v.add_argument("--radioctld", default="radioctld")
    v.add_argument("--selftest", default="radioctl-selftest")
    a = ap.parse_args(argv)
    if a.cmd == "list":
        for c in load_checks(RUNBOOK.read_text()):
            env.say(f"{c.id:<4} {c.phase} {c.rig:<4} {c.minutes:>3} min  {c.title}")
        return 0
    if a.cmd == "detect":
        env.say(repr(detect(env)))
        return 0
    opts = Options(a.rig, a.port, a.baud, a.out_dir, [s for s in a.only.split(",") if s], a.auto_only,
                   a.transmit, a.max_transmit_s, a.no_upload, a.radioctld, a.selftest)
    return verify(env, opts)


if __name__ == "__main__":
    sys.exit(main())
