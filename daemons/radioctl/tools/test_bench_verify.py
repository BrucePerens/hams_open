#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for bench_verify.py. Only virtual rigs are used: an in-process rigctld-style server that
behaves like radioctld (gate, watchdog, unkey on disconnect), a fake Env that spawns it, and a pty to
check that a pty is an acceptable port name. No hardware, no real radioctld, nothing transmits."""
import os
import socket
import tempfile
import threading
import tomllib
import unittest
from unittest import mock
from pathlib import Path

import bench_verify as b

LISTING = "ports:\n  /dev/serial/by-id/usb-Silicon_Labs_CP210x-if00  10c4:ea60\n  /dev/ttyUSB0  1a86:7523\n"


class VirtualRig:
    """A radioctld stand-in on a loopback port: T 1 is refused unless the gate is open, a keyed rig is
    unkeyed by the watchdog (when enabled) and when its connection closes."""

    def __init__(self, port, gate, max_s, watchdog=True):
        self.gate, self.max_s, self.watchdog = gate, max_s, watchdog
        self.keyed = False
        self.key_on_count = 0
        self.srv = socket.create_server(("127.0.0.1", port))
        self.srv.settimeout(0.2)
        self.stopped = False
        self.threads = [threading.Thread(target=self.accept, daemon=True)]
        self.threads[0].start()

    def accept(self):
        while not self.stopped:
            try:
                conn, _ = self.srv.accept()
            except OSError:
                continue
            t = threading.Thread(target=self.serve, args=(conn,), daemon=True)
            t.start()

    def serve(self, conn):
        f = conn.makefile("rwb")
        try:
            for raw in f:
                line = raw.decode().strip()
                if line == "f":
                    out = "14074000"
                elif line == "m":
                    out = "USB\n2400"
                elif line == "t":
                    out = "1" if self.keyed else "0"
                elif line == "T 1":
                    if not self.gate:
                        out = "RPRT -9"
                    else:
                        self.keyed = True
                        self.key_on_count += 1
                        if self.watchdog:
                            threading.Timer(self.max_s, self.unkey).start()
                        out = "RPRT 0"
                elif line == "T 0":
                    self.keyed = False
                    out = "RPRT 0"
                else:
                    out = "RPRT -11"
                f.write((out + "\n").encode())
                f.flush()
        except OSError:
            pass
        finally:
            self.unkey()
            f.close()
            conn.close()

    def unkey(self):
        self.keyed = False

    def close(self):
        self.stopped = True
        self.srv.close()


class FakeProc:
    def __init__(self, rig):
        self.rig, self.terminated = rig, False

    def terminate(self):
        self.terminated = True
        self.rig.close()

    def wait(self, timeout=None):
        return 0

    def kill(self):
        self.terminate()


class FakeEnv(b.Env):
    def __init__(self, answers=None, watchdog=True):
        self.answers = answers or {}
        self.said, self.runs, self.rigs, self.procs, self.prompts = [], [], [], [], []
        self.watchdog = watchdog
        self.typed = b.CONFIRM_PHRASE
        self.groups = "ai dialout"

    def run(self, argv, timeout=60):
        self.runs.append(argv)
        if argv[:1] == ["id"]:
            return 0, self.groups
        if argv[1:2] == ["--list"]:
            return 0, LISTING
        return 0, "ok"

    def spawn(self, argv):
        cfg = tomllib.loads(Path(argv[2]).read_text())["rig"][0]
        rig = VirtualRig(int(cfg["listen"].split(":")[1]), cfg["transmit"], cfg.get("max_transmit_s", 180), self.watchdog)
        self.rigs.append(rig)
        proc = FakeProc(rig)
        self.procs.append(proc)
        return proc

    def read_text(self, path):
        return "0 [PCH            ]: HDA-Intel - HDA Intel PCH\n1 [CODEC          ]: USB-Audio - USB Audio CODEC\n"

    def ask(self, prompt):
        self.prompts.append(prompt)
        if "DUMMY LOAD" in prompt:
            return self.typed
        for key, val in self.answers.items():
            if prompt.startswith(key):
                return val
        return "p"

    def say(self, text=""):
        self.said.append(text)

    def which(self, name):
        return "/usr/bin/" + name

    def now(self):
        import datetime
        return datetime.datetime(2026, 11, 7, 12, 0, tzinfo=datetime.timezone.utc)


def opts(tmp, **kw):
    return b.Options("icom-ic-7300", out_dir=Path(tmp), max_transmit_s=1, **kw)


class BenchVerifyTests(unittest.TestCase):
    # Tests [@ANCHOR: radioctl_bench_load_checks]
    def test_the_runbook_index_and_sections_are_read(self):
        checks = b.load_checks(b.RUNBOOK.read_text())
        ids = [c.id for c in checks]
        for need in ("A1", "A5", "B1", "C1", "C2", "C10", "D1", "E4"):
            self.assertIn(need, ids)
        self.assertEqual(len(ids), len(set(ids)))
        for c in checks:
            if c.phase in "ABCE":
                self.assertTrue(c.body, f"{c.id} has no #### section in the runbook")
        self.assertTrue(next(c for c in checks if c.id == "C1").transmits)
        self.assertFalse(next(c for c in checks if c.id == "B1").transmits)

    # Tests [@ANCHOR: radioctl_bench_port_refusal]
    def test_only_named_ports_and_a_pty_are_accepted(self):
        self.assertIsNotNone(b.port_refusal("/dev/ttyUSB0"))
        self.assertIsNotNone(b.port_refusal("/dev/ttyACM1"))
        self.assertIsNotNone(b.port_refusal("COM3"))
        self.assertIsNone(b.port_refusal("/dev/serial/by-id/usb-X-if00"))
        self.assertIsNone(b.port_refusal("usb:10c4:ea60"))
        self.assertIsNone(b.port_refusal("usb:10c4:ea60:A123"))
        self.assertIsNone(b.port_refusal("virtual"))
        master, slave = os.openpty()
        try:
            self.assertIsNone(b.port_refusal(os.ttyname(slave)))
        finally:
            os.close(master)
            os.close(slave)

    # Tests [@ANCHOR: radioctl_bench_daemon_config]
    def test_the_config_opens_the_gate_only_when_asked_and_only_briefly(self):
        closed = tomllib.loads(b.daemon_config("icom-ic-7300", "usb:10c4:ea60", 19200, 4999, False, 5))["rig"][0]
        self.assertFalse(closed["transmit"])
        self.assertNotIn("max_transmit_s", closed)
        self.assertEqual(closed["transport"]["usb"]["vid"], 0x10C4)
        opened = tomllib.loads(b.daemon_config("icom-ic-7300", "usb:10c4:ea60:A1", 19200, 4999, True, 5))["rig"][0]
        self.assertTrue(opened["transmit"])
        self.assertEqual(opened["max_transmit_s"], 5)
        self.assertEqual(opened["transport"]["usb"]["serial"], "A1")
        with self.assertRaises(ValueError):
            b.daemon_config("icom-ic-7300", "virtual", 19200, 4999, True, 180)
        pty = tomllib.loads(b.daemon_config("icom-ic-7300", "/dev/pts/3", 19200, 4999, False, 5))["rig"][0]
        self.assertEqual(pty["transport"]["lines"], "unmanaged")
        self.assertEqual(pty["transport"]["path"], "/dev/pts/3")

    # Tests [@ANCHOR: radioctl_bench_transmit_failures]
    def test_every_missing_precondition_is_named(self):
        o = b.Options("icom-ic-7300", port="/dev/ttyUSB0", transmit=False, max_transmit_s=99)
        why = " | ".join(b.transmit_failures(o, {}, None))
        for word in ("--transmit", "DUMMY LOAD", "A5", "B2", "max_transmit_s", "bare tty"):
            self.assertIn(word, why)
        ok = {c: b.Result(c, "PASS", "") for c in b.TRANSMIT_PRECONDITIONS}
        good = b.Options("icom-ic-7300", port="usb:10c4:ea60", transmit=True, max_transmit_s=5)
        self.assertEqual(b.transmit_failures(good, ok, b.CONFIRM_PHRASE), [])
        self.assertTrue(b.transmit_failures(good, ok, "yes"))

    # Tests [@ANCHOR: radioctl_bench_detect]
    def test_detect_reuses_the_selftest_listing_and_sound_cards(self):
        info = b.detect(FakeEnv())
        self.assertEqual(info["ports"], ["/dev/serial/by-id/usb-Silicon_Labs_CP210x-if00", "/dev/ttyUSB0"])
        self.assertIn("10c4:ea60", info["usb_ids"])
        self.assertEqual(len(info["audio"]), 2)
        self.assertTrue(info["tools"]["radioctld"])

    # Tests [@ANCHOR: radioctl_bench_verify] [@ANCHOR: radioctl_bench_run_all] [@ANCHOR: radioctl_bench_run_one]
    # Tests [@ANCHOR: radioctl_bench_check_a5] [@ANCHOR: radioctl_bench_check_b2] [@ANCHOR: radioctl_bench_check_a6]
    # Tests [@ANCHOR: radioctl_bench_check_a2] [@ANCHOR: radioctl_bench_check_b1]
    def test_a_default_run_never_transmits_and_writes_both_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv()
            self.assertEqual(b.verify(env, opts(tmp, port="virtual")), 0)
            self.assertEqual(sum(r.key_on_count for r in env.rigs), 0)  # A5's T 1 was refused at the gate
            self.assertTrue(all(not r.gate for r in env.rigs))
            self.assertFalse(Path(tmp, "bench-tx.toml").exists())
            report = Path(tmp, "BENCH-REPORT-icom-ic-7300-2026-11-07.txt").read_text()
            self.assertIn("PASS A5", report)
            self.assertIn("SKIPPED (dry run or skipped) C1", report)
            self.assertIn("tx T 1", report)
            self.assertIn("rx RPRT -9", report)
            proposal = Path(tmp, "verified.toml.proposed").read_text()
            self.assertIn("transmit_tested = false", proposal)
            self.assertIn("FILL IN", proposal)
            self.assertTrue(tomllib.loads(proposal)["report"][0]["rig"] == "icom-ic-7300")
            self.assertTrue(all(p.terminated for p in env.procs))
            selftests = [a for a in env.runs if a[0] == "radioctl-selftest" and "--description" in a]
            self.assertTrue(selftests and all("--no-upload" not in a for a in selftests))

    # Tests [@ANCHOR: radioctl_bench_check_b1]
    def test_no_upload_is_passed_to_the_selftest_only_as_a_testing_convenience(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv()
            b.verify(env, opts(tmp, port="virtual", only=["B1"], no_upload=True))
            argv = [a for a in env.runs if "--description" in a][0]
            self.assertIn("--no-upload", argv)

    # Tests [@ANCHOR: radioctl_bench_check_c1] [@ANCHOR: radioctl_bench_check_c2] [@ANCHOR: radioctl_bench_daemon_start]
    # Tests [@ANCHOR: radioctl_bench_daemon_stop] [@ANCHOR: radioctl_bench_rigctl_cmd] [@ANCHOR: radioctl_bench_free_port]
    def test_a_confirmed_transmit_run_keys_briefly_and_the_watchdog_unkeys(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv()
            self.assertEqual(b.verify(env, opts(tmp, port="virtual", transmit=True)), 0)
            report = Path(tmp, "BENCH-REPORT-icom-ic-7300-2026-11-07.txt").read_text()
            self.assertIn("PASS C1", report)
            self.assertIn("PASS C2", report)
            keyed = [r for r in env.rigs if r.gate]
            self.assertEqual(len(keyed), 2)
            self.assertTrue(all(r.key_on_count == 1 and not r.keyed for r in keyed))
            self.assertTrue(all(p.terminated for p in env.procs))
            self.assertIn("transmit_tested = true", Path(tmp, "verified.toml.proposed").read_text())

    def test_a_wrong_phrase_refuses_every_transmit_and_never_opens_the_gate(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv()
            env.typed = "yes it is"
            b.verify(env, opts(tmp, port="virtual", transmit=True))
            report = Path(tmp, "BENCH-REPORT-icom-ic-7300-2026-11-07.txt").read_text()
            self.assertIn("REFUSED C1", report)
            self.assertIn("REFUSED C2", report)
            self.assertFalse(any(r.gate for r in env.rigs))
            self.assertFalse(Path(tmp, "bench-tx.toml").exists())

    def test_a_failed_safety_precondition_refuses_the_transmit_steps(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv(answers={"A4": "f"})
            b.verify(env, opts(tmp, port="virtual", transmit=True))
            report = Path(tmp, "BENCH-REPORT-icom-ic-7300-2026-11-07.txt").read_text()
            self.assertIn("REFUSED C1: precondition A4 has not passed", report)
            self.assertFalse(any(r.gate for r in env.rigs))

    def test_auto_only_leaves_manual_preconditions_unmet_so_nothing_transmits(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv()
            b.verify(env, opts(tmp, port="virtual", transmit=True, auto_only=True))
            report = Path(tmp, "BENCH-REPORT-icom-ic-7300-2026-11-07.txt").read_text()
            self.assertIn("REFUSED C1", report)
            self.assertFalse(any(r.gate for r in env.rigs))

    def test_a_watchdog_that_does_not_unkey_fails_and_the_tool_unkeys(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv(watchdog=False)
            self.assertEqual(b.verify(env, opts(tmp, port="virtual", transmit=True)), 1)
            report = Path(tmp, "BENCH-REPORT-icom-ic-7300-2026-11-07.txt").read_text()
            self.assertIn("DID NOT PASS C2: still keyed", report)
            self.assertTrue(all(not r.keyed for r in env.rigs))

    # Tests [@ANCHOR: radioctl_bench_check_a2]
    def test_a_path_port_needs_dialout(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv()
            env.groups = "ai"
            r = b.Runner(env, opts(tmp, port="/dev/serial/by-id/usb-X-if00"), [])
            self.assertEqual(b.auto_a2(r, b.Check("A2", "", "any", "A", "", "", "")).verdict, "FAIL")

    def test_a_bare_tty_stops_the_run_before_anything_is_opened(self):
        with tempfile.TemporaryDirectory() as tmp:
            env = FakeEnv()
            with self.assertRaises(SystemExit):
                b.verify(env, opts(tmp, port="/dev/ttyUSB0"))
            self.assertEqual(env.procs, [])

    # Tests [@ANCHOR: radioctl_bench_env_run] [@ANCHOR: radioctl_bench_env_read_text] [@ANCHOR: radioctl_bench_env_spawn]
    # Tests [@ANCHOR: radioctl_bench_env_ask]
    def test_the_real_env_runs_reads_and_spawns_without_touching_hardware(self):
        e = b.Env()
        self.assertEqual(e.run(["true"])[0], 0)
        self.assertEqual(e.run(["/nonexistent-program-xyz"])[0], 127)
        self.assertEqual(e.read_text("/nonexistent-file-xyz"), "")
        p = e.spawn(["sleep", "30"])
        p.terminate()
        self.assertIsNotNone(p.wait(timeout=5))
        with mock.patch("builtins.input", return_value="x"):
            self.assertEqual(e.ask("?"), "x")

    # Tests [@ANCHOR: radioctl_bench_cap_set] [@ANCHOR: radioctl_bench_render_report] [@ANCHOR: radioctl_bench_verified_proposal]
    def test_the_description_is_read_and_the_report_has_the_selftest_layout(self):
        name, caps = b.description_caps(b.RADIOCTL_DIR / "rigs", "icom-ic-7300")
        self.assertEqual(name, "Icom IC-7300")
        self.assertIn("set-ptt", caps)
        text = b.render_report(b.Options("icom-ic-7300"), name, [b.Result("A5", "PASS", "x"), b.Result("C1", "FAIL", "y")],
                               "2026-11-07T12:00:00Z", ["0.1 tx f"])
        self.assertIn("description: icom-ic-7300 (Icom IC-7300)", text)
        self.assertIn("summary: 1 pass, 1 did not pass, 0 refused, 0 skipped", text)
        self.assertIn("wire log", text)

    def test_the_command_line_lists_and_detects(self):
        env = FakeEnv()
        self.assertEqual(b.main(["list"], env), 0)
        self.assertTrue(any(line.startswith("A5") for line in env.said))
        self.assertEqual(b.main(["detect"], env), 0)


if __name__ == "__main__":
    unittest.main()
