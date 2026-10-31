# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Bruce Perens and the hams.com contributors.
"""Behaviour tests for the function-map engine, the ptt channel with its firmware time-out, the
heartbeat protocol and lead/lag sequencing. One body of tests runs twice: against the software
model (virtual clock) and, when HAMS_FX2_PORT names the board's bus-port path (for example 3-1) and
the process may open it, against the real CY7C68013A running the compiled image. Only PA0 and PA1
are ever driven.

Real board:  sudo env PYTHONDONTWRITEBYTECODE=1 HAMS_FX2_PORT=3-1 python3 -B tests/test_fx2_ptt.py
"""
import os
import shutil
import subprocess
import sys
import time
import unittest

import usb.util

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, os.path.join(ROOT, "tools"))

import hams_fx2 as h  # noqa: E402
from fx2_simulator import SimulatedFx2  # noqa: E402

TOL = 3.0          # milliseconds: tick granularity (1 ms) plus main-loop latency (about 0.3 ms)
MAX_ON = 1000
HB = 600
MIN_OFF = 300
PA0, PA1 = h.PIN_PA0, h.PIN_PA1


class BoardTransport(h.UsbTransport):
    """The real board: bus reset through the kernel, suspend through runtime power management."""

    def __init__(self, dev, port):
        super().__init__(dev)
        self.port = port

    def bus_reset(self):
        self.dev.reset()
        time.sleep(0.6)

    def suspend(self):
        usb.util.dispose_resources(self.dev)          # an open usbfs handle keeps the device awake
        base = f"/sys/bus/usb/devices/{self.port}/power"
        with open(f"{base}/runtime_status", encoding="ascii") as fh:
            before = fh.read().strip()
        with open(f"{base}/autosuspend_delay_ms", "w", encoding="ascii") as fh:
            fh.write("300")
        with open(f"{base}/control", "w", encoding="ascii") as fh:
            fh.write("auto")
        deadline = time.monotonic() + 5.0
        status = before
        while time.monotonic() < deadline:
            with open(f"{base}/runtime_status", encoding="ascii") as fh:
                status = fh.read().strip()
            if status == "suspended":
                break
            time.sleep(0.1)
        with open(f"{base}/control", "w", encoding="ascii") as fh:
            fh.write("on")
        if status != "suspended":
            raise AssertionError(f"the board did not suspend (runtime_status {status})")


class PttBehaviour:
    """The tests; subclasses supply self.t (a Transport with sleep, bus_reset and suspend)."""

    def load_ptt(self, max_on=MAX_ON, hb=HB, min_off=MIN_OFF, **extra):
        h.load_map(self.t, [h.PinSpec(PA0, h.FUNC_PTT, max_on_ms=max_on, min_off_ms=min_off, **extra)],
                   heartbeat_ms=hb)
        self.hb = h.Heartbeater(self.t)
        return self.hb.beat()

    def pins(self):
        return h.gpio_get(self.t)

    def pa0_driven(self):
        st = self.pins()
        return (st["oea"] & 1) == 1 and (st["pa"] & 1) == 1

    def pa0_low(self):
        st = self.pins()
        return (st["oea"] & 1) == 1 and (st["pa"] & 1) == 0

    def key_down(self, mask=1):
        h.key(self.t, PA0, True)
        self.hb.commanded = mask

    def hold(self, seconds, step=0.2):
        """Keep the heartbeat going for the given time and return the last status."""
        end = seconds
        st = None
        while end > 1e-9:
            d = min(step, end)
            self.t.sleep(d)
            st = self.hb.beat()
            end -= d
        return st

    def edges(self):
        _, newest_first = h.get_edges(self.t)
        return list(reversed(newest_first))

    def gap(self, a, b):
        return (b - a) % 65536

    def setUp(self):
        h.load_map(self.t, [h.PinSpec(PA0, h.FUNC_PTT)])        # a known state to start from
        self.hb = h.Heartbeater(self.t)

    def tearDown(self):
        self.t.ctrl_out(0x40, h.REQ_MAP_BEGIN, 0, 0)            # releases both pins to inputs

    # ------------------------------------------------------------------ identity
    # Tests [@ANCHOR: HAMS_FX2_get_info_ptt]
    def test_get_info_reports_timeout_heartbeat_and_safe_state(self):
        info = h.read_info(self.t)
        self.assertEqual((info.proto_major, info.proto_minor), (0, 2))
        self.assertIn("hw_timeout", info.capability_names())
        self.assertIn("heartbeat", info.capability_names())
        self.assertEqual(info.safe_state_kind, 1)
        self.assertEqual((info.hw_timeout_ms, info.heartbeat_ms), (5000, 2000))
        self.assertEqual(info.drivable_a, 0x03)

    # ------------------------------------------------------------------ map validation
    # Tests [@ANCHOR: HAMS_FX2_function_map]
    def refused(self, pins, code, **kw):
        with self.assertRaises(h.Fx2Error) as cm:
            h.load_map(self.t, pins, **kw)
        self.assertIn(code, str(cm.exception))
        st = h.get_status(self.t)
        self.assertFalse(st.map_valid)
        self.assertEqual(h.MAP_ERRORS[st.map_error], code)
        self.assertEqual(self.pins()["oea"] & 3, 0, "a refused map must leave both pins released")

    def test_map_refuses_two_functions_on_one_pin(self):
        self.refused([h.PinSpec(PA0, h.FUNC_PTT), h.PinSpec(PA0, h.FUNC_POWER)],
                     "pin_has_two_functions")

    def test_map_refuses_undefined_safe_state(self):
        self.refused([h.PinSpec(PA0, h.FUNC_PTT, safe_defined=False)], "safe_state_undefined")
        self.refused([h.PinSpec(PA0, h.FUNC_POWER, safe_defined=False)], "safe_state_undefined")

    def test_map_refuses_pins_the_board_does_not_allow(self):
        for pin in (0x02, 0x07, 0x10, 0x17, 0x20, 0x27):        # PA2, PA7, PB0, PB7, PD0, PD7
            self.refused([h.PinSpec(pin, h.FUNC_PTT)], "pin_not_allowed")

    def test_map_refuses_safe_state_equal_to_active_level(self):
        self.refused([h.PinSpec(PA0, h.FUNC_PTT, safe=h.SAFE_HIGH, active_high=True)],
                     "safe_state_is_active_level")
        self.refused([h.PinSpec(PA0, h.FUNC_POWER, safe=h.SAFE_LOW, active_high=False)],
                     "safe_state_is_active_level")

    def test_map_refuses_ptt_without_timeout_and_heartbeat_zero(self):
        self.refused([h.PinSpec(PA0, h.FUNC_PTT, max_on_ms=0)], "no_timeout")
        self.refused([h.PinSpec(PA0, h.FUNC_PTT)], "no_heartbeat", heartbeat_ms=0)

    def test_map_refuses_bad_keyline_arrangements(self):
        self.refused([h.PinSpec(PA0, h.FUNC_PTT, partner=PA1), h.PinSpec(PA1, h.FUNC_POWER)],
                     "bad_partner")
        self.refused([h.PinSpec(PA0, h.FUNC_PTT), h.PinSpec(PA1, h.FUNC_KEYLINE)], "bad_partner")
        self.refused([h.PinSpec(PA0, h.FUNC_PTT, partner=PA0)], "bad_partner")
        self.refused([h.PinSpec(PA0, h.FUNC_PTT, lead_ms=50)], "lead_lag_without_keyline")

    def test_map_refuses_parameters_on_non_ptt_pins_and_steps_out_of_order(self):
        self.refused([h.PinSpec(PA0, h.FUNC_POWER, max_on_ms=100)], "bad_parameter")
        self.t.ctrl_out(0x40, h.REQ_MAP_BEGIN, 0, 0)
        with self.assertRaises(h.Stalled):
            self.t.ctrl_out(0x40, h.REQ_MAP_PIN, PA0 | (h.FUNC_PTT << 8), 0)     # no safe state
        self.assertEqual(h.MAP_ERRORS[h.get_status(self.t).map_error], "safe_state_undefined")

    def test_map_accepts_a_valid_map_and_drives_the_safe_state(self):
        st = self.load_ptt()
        self.assertTrue(st.map_valid)
        self.assertTrue(self.pa0_low(), "the safe state (driven low) is applied at commit")
        self.assertEqual(st.keyed, 0)

    def test_direct_gpio_set_is_refused_while_a_map_is_active(self):
        self.load_ptt()
        with self.assertRaises(h.Stalled):
            h.gpio_set(self.t, 3, 3)
        self.assertTrue(self.pa0_low())

    # ------------------------------------------------------------------ key-down rules
    # Tests [@ANCHOR: HAMS_FX2_ptt_channel]
    def test_key_down_refused_without_map_and_without_heartbeat(self):
        self.t.ctrl_out(0x40, h.REQ_MAP_BEGIN, 0, 0)               # a map is being loaded, none active
        with self.assertRaises(h.Stalled):
            h.key(self.t, PA0, True)
        self.assertEqual(h.REFUSALS[h.get_status(self.t).refusal], "no_map")
        self.load_ptt()
        self.t.sleep(0.01)
        # load_ptt sent the first heartbeat; a lapse of it needs HB ms, so refuse on a fresh map
        h.load_map(self.t, [h.PinSpec(PA0, h.FUNC_PTT, max_on_ms=MAX_ON)], heartbeat_ms=HB)
        with self.assertRaises(h.Stalled):
            h.key(self.t, PA0, True)
        self.assertEqual(h.REFUSALS[h.get_status(self.t).refusal], "no_heartbeat")
        self.assertTrue(self.pa0_low())

    def test_key_down_raises_the_line_and_key_up_lowers_it_and_logs_the_host(self):
        self.load_ptt()
        base = h.get_status(self.t).release_count
        self.key_down()
        self.assertTrue(self.pa0_driven())
        st = self.hb.beat()
        self.assertEqual((st.keyed, st.active, st.state[0]), (1, 1, 2))
        h.key(self.t, PA0, False)
        self.hb.commanded = 0
        self.assertTrue(self.pa0_low())
        st = self.hb.beat()
        self.assertEqual((st.keyed, st.last_reason), (0, h.REL_HOST))
        self.assertEqual(st.release_count, (base + 1) & 0xFF)

    def test_keying_a_keyline_or_an_unmapped_pin_is_refused(self):
        self.load_ptt()
        with self.assertRaises(h.Stalled):
            h.key(self.t, PA1, True)
        self.assertEqual(h.REFUSALS[h.get_status(self.t).refusal], "bad_channel")

    # ------------------------------------------------------------------ hardware time-out
    # Tests [@ANCHOR: HAMS_FX2_ptt_timeout]
    def test_line_held_past_the_timeout_is_released_by_the_firmware(self):
        self.load_ptt()
        base = h.get_status(self.t).release_count
        self.key_down()
        self.assertTrue(self.pa0_driven())
        st = self.hold(MAX_ON / 2000.0)
        self.assertTrue(self.pa0_driven(), "still keyed before the time-out")
        self.assertLess(st.remaining_ms[0], MAX_ON)
        self.t.sleep(MAX_ON / 1000.0 / 2 + 0.25)                    # no host action: no beats, below HB
        st = h.get_status(self.t)
        self.assertTrue(self.pa0_low(), "the firmware released the line on its own")
        self.assertEqual((st.keyed, st.active, st.last_reason), (0, 0, h.REL_TIMEOUT))
        self.assertEqual(st.release_count, (base + 1) & 0xFF)
        count, events = h.get_events(self.t)
        self.assertEqual(events[0][:2], (h.REL_TIMEOUT, 0))
        rise, fall = self.edges()[-2:]
        self.assertEqual((rise[1], fall[1]), (1, 0))
        on_ms = self.gap(rise[2], fall[2])
        self.assertTrue(MAX_ON - TOL <= on_ms <= MAX_ON + TOL, f"on-time {on_ms} ms")

    def test_a_host_that_keeps_commanding_the_line_is_told_it_is_out_of_sync(self):
        self.load_ptt()
        self.key_down()
        self.hold(MAX_ON / 1000.0 + 0.4)                            # beats keep saying "keyed"
        st = h.get_status(self.t)
        self.assertEqual(st.last_reason, h.REL_TIMEOUT, "the first release reason is kept")
        self.assertTrue(st.out_of_sync)
        self.assertFalse(st.hb_alive)
        self.hb.commanded = 0
        st = self.hb.beat()
        self.assertTrue(st.hb_alive)
        self.assertFalse(st.out_of_sync)

    def test_minimum_off_time_is_enforced(self):
        self.load_ptt()
        self.key_down()
        self.hold(MAX_ON / 1000.0 + 0.1)                             # the time-out fires at MAX_ON
        self.hb.commanded = 0
        self.hb.beat()                                               # re-arm after the release
        with self.assertRaises(h.Stalled):
            h.key(self.t, PA0, True)
        self.assertEqual(h.REFUSALS[h.get_status(self.t).refusal], "minimum_off_time")
        self.assertTrue(self.pa0_low())
        self.hold(MIN_OFF / 1000.0 + 0.1)
        self.key_down()
        self.assertTrue(self.pa0_driven())

    # ------------------------------------------------------------------ heartbeat
    # Tests [@ANCHOR: HAMS_FX2_heartbeat]
    def test_heartbeat_answers_with_the_boards_own_state(self):
        st = self.load_ptt()
        self.assertTrue(st.hb_alive)
        self.assertEqual(st.last_seq, 1)
        self.assertLessEqual(st.hb_remaining_ms, HB)
        self.assertGreater(st.hb_remaining_ms, HB - 100)
        self.assertEqual(st.remaining_ms, [0xFFFF, 0xFFFF])

    def test_heartbeat_lapse_releases_the_line(self):
        self.load_ptt()
        base = h.get_status(self.t).release_count
        self.key_down()
        beat = self.hb.beat()
        self.t.sleep(HB / 1000.0 + 0.2)                              # the host stops sending
        st = h.get_status(self.t)
        self.assertTrue(self.pa0_low())
        self.assertEqual((st.last_reason, st.hb_alive, st.keyed), (h.REL_HB_LAPSE, False, 0))
        self.assertEqual(st.release_count, (base + 1) & 0xFF)
        fall = self.edges()[-1]
        self.assertEqual(fall[1], 0)
        lapse = self.gap(beat.ms, fall[2])
        self.assertTrue(HB - TOL <= lapse <= HB + TOL + 10, f"lapse released after {lapse} ms")

    def test_repeated_sequence_number_releases_and_disarms(self):
        self.load_ptt()
        self.key_down()
        st = self.hb.beat()
        self.assertTrue(self.pa0_driven())
        self.assertTrue(st.hb_alive)
        again = h.heartbeat(self.t, self.hb.seq, 1)                  # the same number a second time
        self.assertTrue(self.pa0_low())
        self.assertEqual((again.last_reason, again.hb_alive), (h.REL_HB_SEQUENCE, False))
        with self.assertRaises(h.Stalled):
            h.key(self.t, PA0, True)
        self.assertEqual(h.REFUSALS[h.get_status(self.t).refusal], "no_heartbeat")
        self.hb.commanded = 0
        self.assertTrue(self.hb.beat().hb_alive)

    def test_commanded_state_mismatch_releases(self):
        self.load_ptt()
        self.key_down()
        self.hb.beat()
        self.hb.commanded = 0                                        # host claims "not keyed"
        st = self.hb.beat()
        self.assertTrue(self.pa0_low())
        self.assertEqual((st.last_reason, st.hb_alive, st.out_of_sync), (h.REL_HB_MISMATCH, False, False))

    def test_host_claiming_keyed_while_the_board_is_not_marks_out_of_sync(self):
        self.load_ptt()
        self.hb.commanded = 1
        st = self.hb.beat()
        self.assertTrue(st.out_of_sync)
        self.assertFalse(st.hb_alive)
        self.assertTrue(self.pa0_low())

    # ------------------------------------------------------------------ USB events
    # Tests [@ANCHOR: HAMS_FX2_release_on_usb]
    def test_usb_bus_reset_releases_the_line(self):
        self.load_ptt()
        self.key_down()
        self.assertTrue(self.pa0_driven())
        self.t.bus_reset()
        st = h.get_status(self.t)
        self.assertTrue(self.pa0_low())
        self.assertEqual((st.last_reason, st.keyed, st.hb_alive), (h.REL_USB_RESET, 0, False))

    def test_usb_suspend_releases_the_line(self):
        self.load_ptt(max_on=20000, hb=20000)
        self.key_down()
        self.assertTrue(self.pa0_driven())
        self.t.suspend()
        st = h.get_status(self.t)
        self.assertTrue(self.pa0_low())
        self.assertEqual((st.last_reason, st.keyed), (h.REL_USB_SUSPEND, 0))

    def test_new_map_releases_everything_and_logs_it(self):
        self.load_ptt()
        self.key_down()
        h.load_map(self.t, [h.PinSpec(PA0, h.FUNC_PTT)])
        st = h.get_status(self.t)
        self.assertEqual((st.last_reason, st.keyed), (h.REL_MAP_CLEARED, 0))

    # ------------------------------------------------------------------ lead and lag
    # Tests [@ANCHOR: HAMS_FX2_lead_lag]
    def load_pair(self, lead=100, lag=80, max_on=MAX_ON):
        h.load_map(self.t, [h.PinSpec(PA0, h.FUNC_PTT, max_on_ms=max_on, lead_ms=lead, lag_ms=lag,
                                      partner=PA1),
                            h.PinSpec(PA1, h.FUNC_KEYLINE)], heartbeat_ms=HB)
        self.hb = h.Heartbeater(self.t)
        self.hb.beat()

    def test_lead_and_lag_sequence_the_two_lines(self):
        self.load_pair()
        h.key(self.t, PA0, True)
        self.hb.commanded = 3
        early = self.pins()                                          # coarse: a few ms after key-down
        self.assertEqual(early["oea"] & 3, 3)
        self.assertEqual(early["pa"] & 3, 2, "keyline (PA1) first, ptt (PA0) still low")
        self.t.sleep(0.2)
        self.assertEqual(self.pins()["pa"] & 3, 3, "both asserted after the lead time")
        self.assertEqual(self.hb.beat().state[0], 2)
        h.key(self.t, PA0, False)
        self.hb.commanded = 0
        self.assertEqual(self.pins()["pa"] & 3, 2, "ptt dropped, keyline held through the lag")
        self.assertEqual(self.hb.beat().state[0], 3)
        self.t.sleep(0.2)
        self.assertEqual(self.pins()["pa"] & 3, 0, "keyline dropped after the lag")
        # Exact figures from the firmware's own timestamps (250 ns resolution).
        ev = self.edges()
        k_up, p_up, p_dn, k_dn = [(c, lvl, ms) for c, lvl, ms in ev][-4:]
        self.assertEqual((k_up[0], k_up[1], p_up[0], p_up[1]), (1, 1, 0, 1))
        self.assertEqual((p_dn[0], p_dn[1], k_dn[0], k_dn[1]), (0, 0, 1, 0))
        lead = self.gap(k_up[2], p_up[2])
        lag = self.gap(p_dn[2], k_dn[2])
        self.assertTrue(100 - 1 <= lead <= 100 + TOL, f"lead {lead} ms")
        self.assertTrue(80 - 1 <= lag <= 80 + TOL, f"lag {lag} ms")

    def test_timeout_during_a_sequence_drops_both_lines_together(self):
        self.load_pair(max_on=400)
        h.key(self.t, PA0, True)
        self.hb.commanded = 3
        self.t.sleep(0.55)
        st = h.get_status(self.t)
        self.assertEqual(self.pins()["pa"] & 3, 0)
        self.assertEqual((st.last_reason, st.keyed, st.state), (h.REL_TIMEOUT, 0, [0, 0]))
        ev = self.edges()
        p_dn, k_dn = ev[-2], ev[-1]
        self.assertEqual((p_dn[0], k_dn[0]), (0, 1))
        self.assertLessEqual(self.gap(p_dn[2], k_dn[2]), 1.0)
        on_ms = self.gap([e for e in ev if e[0] == 1 and e[1] == 1][-1][2], k_dn[2])
        self.assertTrue(400 - TOL <= on_ms <= 400 + TOL, f"on-time {on_ms}")

    def test_lead_and_lag_figures_are_bounded_by_the_clock_not_the_host(self):
        # The same sequence repeated with the host busy-sleeping a different amount each time must
        # give the same lead and lag, because the firmware, not the host, times it.
        self.load_pair(lead=40, lag=25)
        leads = []
        for pause in (0.0, 0.037, 0.0113):
            h.key(self.t, PA0, True)
            self.hb.commanded = 3
            self.t.sleep(0.12 + pause)
            h.key(self.t, PA0, False)
            self.hb.commanded = 0
            self.t.sleep(0.08)
            self.hold(MIN_OFF / 1000.0 * 0.0 + 0.05)
            ev = self.edges()
            leads.append(self.gap(ev[-4][2], ev[-3][2]))
        for lead in leads:
            self.assertTrue(40 - 1 <= lead <= 40 + TOL, f"lead {lead}")


class SimulatedPtt(PttBehaviour, unittest.TestCase):
    def setUp(self):
        self.t = SimulatedFx2()
        h.load_ram(self.t, {0: 2, 1: 0, 2: 0x40})
        super().setUp()


@unittest.skipUnless(os.environ.get("HAMS_FX2_PORT"), "set HAMS_FX2_PORT=3-1 to test the real board")
class BoardPtt(PttBehaviour, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not shutil.which("sdcc"):
            raise unittest.SkipTest("SDCC not installed")
        subprocess.run(["make", "-C", ROOT], check=True, capture_output=True)
        cls.port = os.environ["HAMS_FX2_PORT"]
        rc = h.main(["--port", cls.port, "load", os.path.join(ROOT, "build", "station_io.ihx")])
        if rc:
            raise RuntimeError("load failed")
        time.sleep(1.0)

    def setUp(self):
        dev = h.find_device(h.HAMS_VID, h.HAMS_PID, self.port)
        self.assertIsNotNone(dev, "board not found")
        self.t = BoardTransport(dev, self.port)
        self.addCleanup(usb.util.dispose_resources, dev)     # a lingering handle keeps it from suspending
        super().setUp()


if __name__ == "__main__":
    unittest.main()
