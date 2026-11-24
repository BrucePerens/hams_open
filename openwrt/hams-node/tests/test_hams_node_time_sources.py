# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the gps, rtc and wwv clock sources of hams-node-time (fake uci, fake device files)."""

import os
import pathlib
import subprocess
import tempfile
import time
import unittest

LIB = pathlib.Path(__file__).resolve().parent.parent / "files" / "usr" / "libexec" / "hams-time"
UCI = '#!/bin/sh\nkey=${3#hams_node_time.main.}\ngrep "^$key=" "$FAKE/uci" | cut -d= -f2-\n'
RMC = "$GPRMC,120000.00,A,4000.0,N,07500.0,W,0.0,0.0,241126,,,A*00\r\n"  # 2026-11-24 12:00:00 UTC
RMC_EPOCH = 1795521600


class SourceTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()
        (self.root / "bin" / "uci").write_text(UCI)
        (self.root / "bin" / "uci").chmod(0o755)

    def run_source(self, name, options):
        (self.root / "uci").write_text("".join("%s=%s\n" % kv for kv in options.items()))
        env = dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), FAKE=str(self.root))
        return subprocess.run(["sh", str(LIB / name)], env=env, capture_output=True, text=True, timeout=30)

    def file(self, name, text):
        path = self.root / name
        path.write_text(text)
        return str(path)

    def test_gps_reads_the_first_valid_rmc_sentence(self):
        dev = self.file("nmea", "$GPGGA,1,2,3\r\n" + RMC)
        result = self.run_source("gps", {"gps_device": dev})
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.split()[0], str(RMC_EPOCH))

    def test_gps_without_a_fix_or_a_device_fails(self):
        self.assertNotEqual(self.run_source("gps", {"gps_device": self.file("n", RMC.replace(",A,4", ",V,4"))}).returncode, 0)
        self.assertNotEqual(self.run_source("gps", {"gps_device": str(self.root / "missing")}).returncode, 0)
        self.assertNotEqual(self.run_source("gps", {}).returncode, 0)
        self.assertNotEqual(self.run_source("gps", {"gps_device": self.file("g", "$GPRMC,12x000,A,,,,,,,2411xx\n")}).returncode, 0)

    def test_rtc_prints_the_rtc_seconds_and_refuses_an_unset_rtc(self):
        ok = self.run_source("rtc", {"rtc_file": self.file("r", "%d\n" % RMC_EPOCH)})
        self.assertEqual(ok.stdout.split(), [str(RMC_EPOCH), "60"])
        self.assertNotEqual(self.run_source("rtc", {"rtc_file": self.file("r0", "0\n")}).returncode, 0)
        self.assertNotEqual(self.run_source("rtc", {"rtc_file": str(self.root / "none")}).returncode, 0)

    def test_wwv_adds_the_age_of_the_decoder_file_and_refuses_a_stale_one(self):
        path = self.file("wwv", "%d 1\n" % RMC_EPOCH)
        os.utime(path, (time.time() - 30, time.time() - 30))
        result = self.run_source("wwv", {"wwv_file": path})
        self.assertEqual(result.returncode, 0, result.stderr)
        epoch, error = result.stdout.split()
        self.assertIn(int(epoch) - RMC_EPOCH, (30, 31, 32))
        self.assertEqual(error, "1")
        self.assertNotEqual(self.run_source("wwv", {"wwv_file": path, "wwv_max_age": "10"}).returncode, 0)
        self.assertNotEqual(self.run_source("wwv", {"wwv_file": str(self.root / "none")}).returncode, 0)


if __name__ == "__main__":
    unittest.main()
