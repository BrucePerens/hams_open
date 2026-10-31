# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Bruce Perens and the hams.com contributors.
"""Host-side tests for the FX2 station I/O tool: GET_INFO parsing, Intel HEX handling, the
RAM loader's safety rules, the simulator, and (when SDCC is installed) the compiled image.

Run:  python3 -m unittest discover -s hardware/fx2_station_io/tests -t hardware/fx2_station_io
"""
import os
import shutil
import subprocess
import sys
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(ROOT, "tools"))

import hams_fx2 as h  # noqa: E402
from fx2_simulator import SIM_INFO, SimulatedFx2  # noqa: E402


def build_image():
    """Build with make when SDCC exists; return the parsed image or None."""
    if not shutil.which("sdcc"):
        return None
    subprocess.run(["make", "-C", ROOT], check=True, capture_output=True)
    with open(os.path.join(ROOT, "build", "station_io.ihx"), encoding="ascii") as fh:
        return h.parse_ihx(fh.read())


class ParseGetInfoTests(unittest.TestCase):
    # Tests [@ANCHOR: HAMS_FX2_parse_get_info]
    def test_parses_simulator_record(self):
        info = h.parse_get_info(SIM_INFO)
        self.assertEqual((info.proto_major, info.proto_minor), (0, 1))
        self.assertEqual(info.device_type, 1)
        self.assertEqual(info.fw_version, (0, 1, 0))
        self.assertEqual(info.safe_state_kind, 0)
        self.assertEqual(info.hw_timeout_ms, 0)
        self.assertEqual(info.drivable_a, 0x03)
        self.assertEqual(info.endpoints, [(0, 1)])
        self.assertEqual(info.capability_names(), ["gpio_cmd", "msos20", "webusb", "safe_on_reset"])

    def test_ignores_unknown_trailing_fields(self):
        longer = bytearray(SIM_INFO) + b"\xAA\xBB"
        longer[0] = len(longer)
        self.assertEqual(h.parse_get_info(bytes(longer)).endpoints, [(0, 1)])

    def test_refuses_unknown_major_version(self):
        bad = bytearray(SIM_INFO)
        bad[1] = 1
        with self.assertRaises(h.Fx2Error):
            h.parse_get_info(bytes(bad))

    def test_refuses_truncated_and_inconsistent_records(self):
        with self.assertRaises(h.Fx2Error):
            h.parse_get_info(SIM_INFO[:10])
        bad = bytearray(SIM_INFO)
        bad[20] = 9          # endpoint count past the record
        with self.assertRaises(h.Fx2Error):
            h.parse_get_info(bytes(bad))
        bad = bytearray(SIM_INFO)
        bad[0] = 99          # length byte longer than the data
        with self.assertRaises(h.Fx2Error):
            h.parse_get_info(bytes(bad))


class IntelHexTests(unittest.TestCase):
    # Tests [@ANCHOR: HAMS_FX2_parse_ihx]
    def test_parses_records(self):
        img = h.parse_ihx(":03000000020040BB\n:00000001FF\n")
        self.assertEqual(img, {0: 2, 1: 0, 2: 0x40})

    def test_rejects_bad_checksum_and_missing_eof(self):
        with self.assertRaises(h.Fx2Error):
            h.parse_ihx(":03000000020040BC\n:00000001FF\n")
        with self.assertRaises(h.Fx2Error):
            h.parse_ihx(":03000000020040BB\n")
        with self.assertRaises(h.Fx2Error):
            h.parse_ihx(":020000040000FA\n:00000001FF\n")   # extended address record


class ChunkTests(unittest.TestCase):
    # Tests [@ANCHOR: HAMS_FX2_image_chunks]
    def test_gap_fill_and_split(self):
        chunks = h.image_chunks({0: 1, 4: 2, 0x2000: 3})
        self.assertEqual(chunks, [(0, bytes([1, 0, 0, 0, 2])), (0x2000, bytes([3]))])

    def test_refuses_outside_code_ram(self):
        with self.assertRaises(h.Fx2Error):
            h.image_chunks({0x4000: 1})
        with self.assertRaises(h.Fx2Error):
            h.image_chunks({0xE600: 1})
        with self.assertRaises(h.Fx2Error):
            h.image_chunks({})


class LoaderTests(unittest.TestCase):
    # Tests [@ANCHOR: HAMS_FX2_load_ram]
    def test_load_holds_resets_writes_verifies_releases_only_via_a0(self):
        sim = SimulatedFx2()
        n = h.load_ram(sim, {0: 2, 1: 0, 2: 0x40, 0x2300: 23})
        self.assertGreater(n, 0)
        self.assertTrue(sim.running)
        writes = [e for e in sim.log if e[0] == "out"]
        self.assertTrue(all(e[1] == 0x40 and e[2] == 0xA0 for e in writes))
        self.assertEqual(writes[0][3:], (0xE600, 0, b"\x01"))     # reset first
        self.assertEqual(writes[-1][3:], (0xE600, 0, b"\x00"))    # release last
        self.assertTrue(all(e[3] < 0x4000 for e in writes[1:-1]))  # code RAM only
        self.assertEqual(sim.eeprom_writes, 0)

    def test_readback_mismatch_leaves_cpu_in_reset(self):
        sim = SimulatedFx2()
        real = sim.ctrl_in

        def corrupt(*a):
            return b"\xFF" * a[4]
        sim.ctrl_in = corrupt
        with self.assertRaises(h.Fx2Error):
            h.load_ram(sim, {0: 2, 1: 0, 2: 0x40})
        sim.ctrl_in = real
        self.assertTrue(sim.cpu_reset)
        self.assertFalse(sim.running)


class GpioTests(unittest.TestCase):
    # Tests [@ANCHOR: HAMS_FX2_gpio]
    def setUp(self):
        self.sim = SimulatedFx2()
        h.load_ram(self.sim, {0: 2, 1: 0, 2: 0x40})

    # Tests [@ANCHOR: HAMS_FX2_read_info]
    def test_read_info_through_transport(self):
        self.assertEqual(h.read_info(self.sim).device_type, 1)

    def test_drive_and_release(self):
        h.gpio_set(self.sim, 3, 1)
        st = h.gpio_get(self.sim)
        self.assertEqual((st["oea"], st["pa"] & 3), (3, 1))
        h.gpio_set(self.sim, 0, 0)
        st = h.gpio_get(self.sim)
        self.assertEqual(st["oea"], 0)
        self.assertEqual(st["pb"], 0xFF)          # other ports stay inputs

    def test_refuses_pins_other_than_pa0_pa1(self):
        for oe, lv in ((4, 0), (0, 4), (0x80, 0x80)):
            with self.assertRaises(h.Fx2Error):
                h.gpio_set(self.sim, oe, lv)
        self.assertEqual(self.sim.oea, 0)


class TickTests(unittest.TestCase):
    # Tests [@ANCHOR: HAMS_FX2_tick]
    def test_tick_advances_at_one_millisecond_per_count(self):
        sim = SimulatedFx2()
        h.load_ram(sim, {0: 2, 1: 0, 2: 0x40})
        a = h.read_tick(sim)
        time.sleep(0.05)
        b = h.read_tick(sim)
        self.assertGreater(b.seconds - a.seconds, 0.04)
        self.assertLess(b.seconds - a.seconds, 0.5)
        self.assertLess(b.sub, 4000)
        self.assertEqual(b.loop_max_us, 0x120 * 0.25)

    def test_short_reply_refused(self):
        class Short(SimulatedFx2):
            def ctrl_in(self, *a):
                return b"\x00" * 3
        with self.assertRaises(h.Fx2Error):
            h.read_tick(Short())


@unittest.skipUnless(shutil.which("sdcc"), "SDCC not installed")
class CompiledImageTests(unittest.TestCase):
    # Tests [@ANCHOR: HAMS_FX2_parse_get_info]
    def test_image_record_matches_simulator_and_parser(self):
        image = build_image()
        length = image[h.INFO_ADDR]
        record = bytes(image[h.INFO_ADDR + i] for i in range(length))
        self.assertEqual(record, SIM_INFO)
        self.assertEqual(h.parse_get_info(record).device_type, 1)

    # Tests [@ANCHOR: HAMS_FX2_image_chunks]
    def test_image_fits_code_ram_and_descriptors_are_word_aligned(self):
        image = build_image()
        self.assertLess(max(image), h.CODE_RAM_END)
        for addr in (0x2000, 0x2020, 0x2030, 0x2050, 0x2070, 0x2080, 0x20A0, 0x20D0, 0x2110,
                     0x2200, h.INFO_ADDR):
            self.assertEqual(addr % 2, 0)
        # Descriptor spot checks: device descriptor VID/PID, BOS and MS OS 2.0 lengths.
        dev = bytes(image[0x2000 + i] for i in range(18))
        self.assertEqual(dev[0:2], bytes([18, 1]))
        self.assertEqual(dev[8:12], bytes([0x09, 0x12, 0x01, 0x00]))   # 1209:0001
        bos = bytes(image[0x20D0 + i] for i in range(57))
        self.assertEqual((bos[0], bos[1], bos[2], bos[4]), (5, 0x0F, 57, 2))
        msos = bytes(image[0x2110 + i] for i in range(162))
        self.assertEqual(msos[6], 162)
        self.assertEqual(msos[8:10], bytes([20, 0]))
        self.assertEqual(msos[28:30], bytes([132, 0]))
        self.assertEqual(10 + 20 + 132, 162)
        # MS OS 2.0 set length in the BOS capability matches the set.
        self.assertEqual(bos[5 + 24 + 24], 162)


if __name__ == "__main__":
    unittest.main()
