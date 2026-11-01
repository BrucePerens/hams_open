#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for wire_to_transcript.py on a capture-looking sample (formats of `bench_console`)."""
import tomllib
import unittest

import wire_to_transcript as w

# What `bench_console --log` and `--wire` write, with the timing of a real 19200 baud link: a
# poll runs while each command waits its turn, and the radio sent one unsolicited frame.
LOG = """\
    0.000 bench_console: icom-ic-7300 on /dev/ttyUSB0 at 19200 baud (receive only)
    0.510 > freq a
    0.560 = Frequency(A, Hz(14074000)) (50 ms)
    0.561 event source=Api Frequency(A): None -> Frequency(A, Hz(14074000))
    1.200 > setfreq a 7074000
    1.290 = ok (90 ms)
    1.400 > mode
    1.455 = Mode(Usb) (55 ms)
    1.500 > level af-gain
    1.560 = Level(AfGain, 128) (60 ms)
    2.000 > quit
    2.001 final stats: RigStats { commands_ok: 3 }
"""
WIRE = """\
    0.050 rx fe fe e0 94 00 00 40 07 14 00 fd
    0.100 tx fe fe 94 e0 25 01 fd
    0.130 rx fe fe e0 94 25 01 00 40 07 14 00 fd
    0.480 tx fe fe 94 e0 15 02 fd
    0.505 rx fe fe e0 94 15 02 00 30 fd
    0.515 tx fe fe 94 e0 03 fd
    0.555 rx fe fe e0 94 03 00 40 07 14 00 fd
    1.210 tx fe fe 94 e0 05 00 40 07 07 00 fd
    1.285 rx fe fe e0 94 fb fd
    1.402 tx fe fe 94 e0 15 02 fd
    1.408 rx fe fe e0 94 15 02 00 30 fd
    1.410 tx fe fe 94 e0 26 00 fd
    1.450 rx fe fe e0 94 26 00 01 00 01 fd
    1.510 tx fe fe 94 e0 14 01 fd
    1.555 rx fe fe e0 94 14 01 01 28 fd
"""


class WireToTranscriptTests(unittest.TestCase):
    # Tests [@ANCHOR: radioctl_wire_parse_wire]
    def test_the_wire_lines_are_parsed_and_a_bad_line_is_an_error(self):
        frames = w.parse_wire(WIRE)
        self.assertEqual(len(frames), 15)
        self.assertEqual((frames[1].direction, frames[1].data[:2]), ("tx", b"\xfe\xfe"))
        with self.assertRaises(ValueError):
            w.parse_wire("0.1 up fe fd\n")

    # Tests [@ANCHOR: radioctl_wire_parse_log]
    def test_a_command_gets_the_result_line_that_ended_it_and_events_are_ignored(self):
        commands = w.parse_log(LOG)
        self.assertEqual([c.text for c in commands], ["freq a", "setfreq a 7074000", "mode", "level af-gain", "quit"])
        self.assertEqual(commands[0].result, "Frequency(A, Hz(14074000))")
        self.assertEqual(commands[1].result, "ok")
        self.assertIsNone(commands[4].done)

    # Tests [@ANCHOR: radioctl_wire_exchanges]
    def test_a_tx_frame_takes_the_rx_bytes_after_it_and_rx_before_any_tx_is_stray(self):
        pairs, stray = w.exchanges(w.parse_wire("0.0 rx aa\n0.1 tx fe fd\n0.2 rx 01\n0.3 rx 02\n0.4 tx 05\n"))
        self.assertEqual([s.data for s in stray], [b"\xaa"])
        self.assertEqual((pairs[0].tx, pairs[0].rx, pairs[0].rx_time), (b"\xfe\xfd", b"\x01\x02", 0.3))
        self.assertEqual(pairs[1].rx, b"")

    # Tests [@ANCHOR: radioctl_wire_op_of]
    def test_console_commands_become_ops_and_unknown_ones_do_not(self):
        self.assertEqual(w.op_of("freq"), '{ kind = "get-frequency", vfo = "a" }')
        self.assertEqual(w.op_of("freq b"), '{ kind = "get-frequency", vfo = "b" }')
        self.assertEqual(w.op_of("setfreq b 14025000"), '{ kind = "set-frequency", vfo = "b", hz = 14025000 }')
        self.assertEqual(w.op_of("setmode CW"), '{ kind = "set-mode", mode = "cw" }')
        self.assertEqual(w.op_of("setritoffset -1200"), '{ kind = "set-rit-offset", hz = -1200 }')
        for unknown in ["", "sleep 1", "setfreq a x", "scope on", "setmode a b"]:
            self.assertIsNone(w.op_of(unknown), unknown)

    # Tests [@ANCHOR: radioctl_wire_expect_of]
    def test_console_results_become_expectations_and_unknown_ones_do_not(self):
        self.assertEqual(w.expect_of("ok"), '{ kind = "ack" }')
        self.assertEqual(w.expect_of("Frequency(B, Hz(7074000))"), '{ kind = "frequency", vfo = "b", hz = 7074000 }')
        self.assertEqual(w.expect_of("Mode(Usb)"), '{ kind = "mode", mode = "usb" }')
        self.assertEqual(w.expect_of("Signal(30)"), '{ kind = "signal", value = 30 }')
        self.assertEqual(w.expect_of("Split(true)"), '{ kind = "split", on = true }')
        self.assertIsNone(w.expect_of("Level(AfGain, 128)"))

    # Tests [@ANCHOR: radioctl_wire_redact]
    def test_redaction_zeroes_every_occurrence_of_the_bytes(self):
        self.assertEqual(w.redact(b"\x01\xde\xad\x02\xde\xad", [b"\xde\xad"]), b"\x01\x00\x00\x02\x00\x00")
        self.assertEqual(w.redact(b"abc", [b""]), b"abc")

    # Tests [@ANCHOR: radioctl_wire_toml_text]
    def test_ascii_frames_become_escaped_text_and_binary_frames_do_not(self):
        self.assertEqual(w.toml_text(b"GF\r\n"), '"GF\\r\\n"')
        self.assertEqual(w.toml_text(b'a"b\\'), '"a\\"b\\\\"')
        self.assertIsNone(w.toml_text(b"\xfe\xfe\x94"))
        self.assertIsNone(w.toml_text(b""))

    # Tests [@ANCHOR: radioctl_wire_convert]
    def test_the_sample_converts_to_the_steps_the_commands_ran_and_polls_are_comments(self):
        text = w.convert("icom-ic-7300", WIRE, LOG)
        doc = tomllib.loads(text)
        self.assertEqual((doc["schema"], doc["rig"], doc["origin"]), (1, "icom-ic-7300", "capture"))
        steps = doc["step"]
        self.assertEqual(
            [(s["tx"], s["rx"]) for s in steps],
            [
                ("fe fe 94 e0 03 fd", "fe fe e0 94 03 00 40 07 14 00 fd"),
                ("fe fe 94 e0 05 00 40 07 07 00 fd", "fe fe e0 94 fb fd"),
                ("fe fe 94 e0 26 00 fd", "fe fe e0 94 26 00 01 00 01 fd"),
            ],
        )
        self.assertEqual(steps[0]["expect"], {"kind": "frequency", "vfo": "a", "hz": 14074000})
        self.assertEqual(steps[1]["op"], {"kind": "set-frequency", "vfo": "a", "hz": 7074000})
        self.assertEqual(steps[1]["expect"], {"kind": "ack"})
        self.assertIn("# check: 2 exchanges ran while 'mode' did", text)
        self.assertIn("# not converted (unknown command): > level af-gain = Level(AfGain, 128)", text)
        self.assertIn("# unattributed exchange at 0.100 s", text)
        self.assertIn("# unsolicited rx at 0.050 s: fe fe e0 94 00 00 40 07 14 00 fd", text)

    # Tests [@ANCHOR: radioctl_wire_convert]
    def test_redaction_applies_to_steps_and_comments_and_nothing_to_convert_is_an_error(self):
        text = w.convert("icom-ic-7300", WIRE, LOG, [bytes.fromhex("4007")])
        self.assertNotIn("40 07", text)
        self.assertIn("03 00 00 00 14 00 fd", text)
        with self.assertRaises(ValueError):
            w.convert("icom-ic-7300", WIRE, "    0.0 > quit\n")

    # Tests [@ANCHOR: radioctl_wire_convert]
    def test_an_ascii_rig_gets_text_steps(self):
        text = w.convert(
            "kenwood-ts-2000",
            "0.10 tx 46 41 3b\n0.14 rx 46 41 30 30 30 30 37 30 37 34 30 30 30 3b\n",
            "0.09 > freq a\n0.15 = Frequency(A, Hz(7074000)) (5 ms)\n",
        )
        step = tomllib.loads(text)["step"][0]
        self.assertEqual((step["tx_text"], step["rx_text"]), ("FA;", "FA00007074000;"))

    # Tests [@ANCHOR: radioctl_wire_main]
    def test_main_writes_the_file_and_reports_a_missing_input(self):
        import tempfile
        from pathlib import Path

        with tempfile.TemporaryDirectory() as tmp:
            wire, log, out = Path(tmp, "w.txt"), Path(tmp, "c.log"), Path(tmp, "t.toml")
            wire.write_text(WIRE)
            log.write_text(LOG)
            argv = ["--rig", "icom-ic-7300", "--wire", str(wire), "--log", str(log), "--out", str(out)]
            self.assertEqual(w.main(argv), 0)
            self.assertEqual(len(tomllib.loads(out.read_text())["step"]), 3)
            self.assertEqual(w.main(["--rig", "x", "--wire", str(Path(tmp, "no")), "--log", str(log)]), 1)


if __name__ == "__main__":
    unittest.main()
