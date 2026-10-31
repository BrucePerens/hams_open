# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Bruce Perens and the hams.com contributors.
"""A software model of the station I/O firmware's USB interface, for host-side tests.

It implements the Cypress 0xA0 firmware-load behaviour of the unprogrammed chip (RAM only,
CPUCS gate) and the vendor commands of the loaded image, independently of the C source, so
the host code can be exercised without a board. It is built from the protocol constants in
the README, not from the compiled image; tests/test_fx2_host.py compares it with the image.
"""
from __future__ import annotations

import time

from hams_fx2 import (CPUCS_ADDR, FLAG_ACTIVE_HIGH, FLAG_SAFE_DEFINED, FUNC_KEYLINE, FUNC_POWER,
                      FUNC_PTT, PARAM_LAG_MS, PARAM_LEAD_MS, PARAM_MAX_ON_MS, PARAM_MIN_OFF_MS,
                      PARAM_PARTNER, REL_HB_LAPSE, REL_HB_MISMATCH, REL_HB_SEQUENCE, REL_HOST,
                      REL_MAP_CLEARED, REL_TIMEOUT, REL_USB_RESET, REL_USB_SUSPEND,
                      REQ_FIRMWARE_LOAD, REQ_GET_EDGES, REQ_GET_EVENTS, REQ_GET_INFO,
                      REQ_GET_STATUS, REQ_GET_TICK, REQ_GPIO_GET, REQ_GPIO_SET, REQ_HEARTBEAT,
                      REQ_KEY, REQ_MAP_BEGIN, REQ_MAP_COMMIT, REQ_MAP_HEARTBEAT, REQ_MAP_PARAM,
                      REQ_MAP_PIN, STATUS_LEN, Fx2Error, LED_MASK, Stalled, Transport)

SIM_INFO = bytes([
    23, 0, 2, 0x01, 0x00, 0, 2, 0, 1,
    0x3F, 0, 0, 0,          # gpio_cmd | msos20 | webusb | hw_timeout | heartbeat | safe_on_reset
    1,                      # safe state per map
    0x88, 0x13,             # default maximum on-time 5000 ms
    0xD0, 0x07,             # default heartbeat lapse 2000 ms
    0x07, LED_MASK, 1, 0x00, 1,
])
BOARD_PINS = (0, 1)         # PA0 and PA1 are the only pins the board table allows as outputs


class _Chan:
    def __init__(self) -> None:
        self.func = 0
        self.safe = 0
        self.active_high = True
        self.partner = None
        self.owner = None
        self.max_on = 5000
        self.min_off = 0
        self.lead = 0
        self.lag = 0
        self.state = "idle"
        self.level = None           # 0, 1, 2 (released) or None before the map is committed
        self.t_key = 0
        self.t_phase = 0
        self.t_off = None


class SimulatedStationEngine:
    """The function-map engine written from the protocol description (virtual clock, 1 ms steps)."""

    def __init__(self) -> None:
        self.now = 0
        self.edges = []              # (channel, level, ms)
        self.events = []             # (reason, channel, ms)
        self.reset_all()

    def reset_all(self) -> None:
        self.ch = [_Chan(), _Chan()]
        self.map_state = "none"
        self.map_error = 0
        self.refusal = 0
        self.alive = False
        self.oos = False
        self.have_seq = False
        self.last_seq = 0
        self.hb_timeout = 2000
        self.hb_t = 0
        self.last_reason = 0
        self.last_channel = 0

    # --- pin and release plumbing
    def _set(self, c: int, level: int) -> None:
        if self.ch[c].level != level:
            self.ch[c].level = level
            self.edges.append((c, level, self.now))

    def _active(self, c: int, on: bool) -> None:
        self._set(c, int(self.ch[c].active_high == on))

    def _safe(self, c: int) -> None:
        self._set(c, 2 if self.ch[c].safe == 2 else self.ch[c].safe)

    def _primary(self, c: int) -> bool:
        return self.ch[c].func in (FUNC_PTT, FUNC_POWER)

    def keyed_mask(self) -> int:
        m = 0
        for c, ch in enumerate(self.ch):
            if self._primary(c) and ch.state in ("lead", "on"):
                m |= 1 << c
                if ch.partner is not None:
                    m |= 1 << ch.partner
        return m

    def active_mask(self) -> int:
        return sum(1 << c for c, ch in enumerate(self.ch)
                   if ch.func and ch.level in (0, 1) and (ch.level == 1) == ch.active_high)

    def _release_channel(self, c: int, reason: int) -> None:
        ch = self.ch[c]
        was_keyed = ch.state in ("lead", "on")
        self._safe(c)
        if ch.partner is not None:
            self._safe(ch.partner)
        ch.state = "idle"
        ch.t_off = self.now
        if was_keyed:
            self.events.append((reason, c, self.now))
            self.last_reason, self.last_channel = reason, c

    def release_all(self, reason: int) -> None:
        for c, ch in enumerate(self.ch):
            if self._primary(c) and ch.state != "idle":
                self._release_channel(c, reason)

    # --- map
    def _fail(self, code: int) -> bool:
        self.map_error = code
        return False

    def map_begin(self) -> bool:
        self.release_all(REL_MAP_CLEARED)
        for c, ch in enumerate(self.ch):
            if ch.level is not None:
                self._set(c, 2)
        self.ch = [_Chan(), _Chan()]
        self.map_state, self.alive, self.have_seq, self.oos = "loading", False, False, False
        self.hb_timeout = 2000
        self.map_error = 0
        return True

    def map_pin(self, pin: int, func: int, flags: int) -> bool:
        if self.map_state != "loading":
            return self._fail(10)
        if pin not in BOARD_PINS:
            return self._fail(1)
        if func not in (FUNC_PTT, FUNC_KEYLINE, FUNC_POWER):
            return self._fail(3)
        if self.ch[pin].func:
            return self._fail(2)
        safe = flags & 3
        if not flags & FLAG_SAFE_DEFINED or safe == 3:
            return self._fail(4)
        active_high = bool(flags & FLAG_ACTIVE_HIGH)
        if safe != 2 and safe == int(active_high):
            return self._fail(5)
        ch = self.ch[pin]
        ch.func, ch.safe, ch.active_high = func, safe, active_high
        return True

    def map_param(self, pin: int, param: int, value: int) -> bool:
        if self.map_state != "loading":
            return self._fail(10)
        if pin not in BOARD_PINS:
            return self._fail(1)
        ch = self.ch[pin]
        if ch.func != FUNC_PTT:
            return self._fail(6)
        if param == PARAM_MAX_ON_MS:
            ch.max_on = value
        elif param == PARAM_MIN_OFF_MS:
            ch.min_off = value
        elif param == PARAM_LEAD_MS:
            ch.lead = value
        elif param == PARAM_LAG_MS:
            ch.lag = value
        elif param == PARAM_PARTNER:
            if (value & 0xFF) not in BOARD_PINS or (value & 0xFF) == pin:
                return self._fail(7)
            ch.partner = value & 0xFF
        else:
            return self._fail(6)
        return True

    def map_heartbeat(self, ms: int) -> bool:
        if self.map_state != "loading":
            return self._fail(10)
        if ms == 0:
            return self._fail(9)
        self.hb_timeout = ms
        return True

    def map_commit(self) -> bool:
        if self.map_state != "loading":
            return self._fail(10)
        for ch in self.ch:
            ch.owner = None
        for c, ch in enumerate(self.ch):
            if ch.func == FUNC_PTT:
                if ch.max_on == 0:
                    return self._fail(8)
                if ch.partner is not None:
                    k = self.ch[ch.partner]
                    if k.func != FUNC_KEYLINE or k.owner is not None:
                        return self._fail(7)
                    k.owner = c
                elif ch.lead or ch.lag:
                    return self._fail(11)
            elif ch.func == FUNC_POWER:
                ch.partner = None
        if any(ch.func == FUNC_KEYLINE and ch.owner is None for ch in self.ch):
            return self._fail(7)
        if not any(ch.func for ch in self.ch):
            return self._fail(3)
        self.map_state = "valid"
        for c, ch in enumerate(self.ch):
            if ch.func:
                self._safe(c)
        return True

    # --- keying and heartbeat
    def _refuse(self, why: int) -> bool:
        self.refusal = why
        return False

    def key(self, pin: int, down: bool) -> bool:
        if self.map_state != "valid":
            return self._refuse(1)
        if pin not in BOARD_PINS or not self._primary(pin):
            return self._refuse(5)
        ch = self.ch[pin]
        if not down:
            if ch.state in ("lead", "on"):
                self._safe(pin)
                ch.t_off = self.now
                self.events.append((REL_HOST, pin, self.now))
                self.last_reason, self.last_channel = REL_HOST, pin
                if ch.partner is not None and ch.lag:
                    ch.state, ch.t_phase = "lag", self.now
                else:
                    if ch.partner is not None:
                        self._safe(ch.partner)
                    ch.state = "idle"
            return True
        if ch.state in ("lead", "on"):
            return True
        if not self.alive:
            return self._refuse(2)
        if ch.state == "lag":
            return self._refuse(4)
        if ch.t_off is not None and self.now - ch.t_off < ch.min_off:
            return self._refuse(3)
        ch.t_key = ch.t_phase = self.now
        if ch.partner is not None:
            self._active(ch.partner, True)
            if ch.lead:
                ch.state = "lead"
                return True
        self._active(pin, True)
        ch.state = "on"
        return True

    def heartbeat(self, seq: int, commanded: int) -> None:
        if self.map_state != "valid":
            return
        reason = 0
        if self.have_seq and seq == self.last_seq:
            reason = REL_HB_SEQUENCE
        elif commanded != self.keyed_mask():
            reason = REL_HB_MISMATCH
        self.last_seq, self.have_seq = seq, True
        if reason == 0:
            self.alive, self.oos, self.hb_t = True, False, self.now
        else:
            self.alive = False
            self.oos = bool(commanded & ~self.keyed_mask())
            self.release_all(reason)

    def step(self, ms: int) -> None:
        for _ in range(ms):
            self.now += 1
            self._tick()

    def _tick(self) -> None:
        if self.map_state != "valid":
            return
        for c, ch in enumerate(self.ch):
            if not self._primary(c):
                continue
            if ch.state == "lead" and self.now - ch.t_phase >= ch.lead:
                self._active(c, True)
                ch.state = "on"
            if ch.state in ("lead", "on") and ch.func == FUNC_PTT and self.now - ch.t_key >= ch.max_on:
                self._release_channel(c, REL_TIMEOUT)
            elif ch.state == "lag" and self.now - ch.t_phase >= ch.lag:
                self._safe(ch.partner)
                ch.state = "idle"
        if self.alive and self.now - self.hb_t >= self.hb_timeout:
            self.alive = False
            self.release_all(REL_HB_LAPSE)

    def bus_reset(self) -> None:
        self.release_all(REL_USB_RESET)
        self.alive = False

    def suspend(self) -> None:
        self.release_all(REL_USB_SUSPEND)
        self.alive = False

    # --- records
    def _remaining(self, c: int) -> int:
        ch = self.ch[c]
        if ch.func != FUNC_PTT or ch.state not in ("lead", "on"):
            return 0xFFFF
        return max(0, ch.max_on - (self.now - ch.t_key))

    def status(self) -> bytes:
        def u16(v: int) -> bytes:
            return (v & 0xFFFF).to_bytes(2, "little")

        flags = (self.map_state == "valid") | (self.alive << 1) | (self.oos << 2) \
            | ((self.map_state == "loading") << 3)
        hb_rem = 0xFFFF
        if self.alive:
            hb_rem = max(0, self.hb_timeout - (self.now - self.hb_t))
        d = bytes([STATUS_LEN, flags, self.keyed_mask(), self.active_mask(), self.last_reason,
                   self.last_channel, len(self.events) & 0xFF, self.map_error, self.refusal,
                   {"idle": 0, "lead": 1, "on": 2, "lag": 3}[self.ch[0].state],
                   {"idle": 0, "lead": 1, "on": 2, "lag": 3}[self.ch[1].state]])
        return d + u16(self.last_seq) + u16(self.now) + u16(self._remaining(0)) \
            + u16(self._remaining(1)) + u16(hb_rem) + u16(0x120)


class SimulatedFx2(Transport):
    """State: ROM (unprogrammed) or RUNNING; RAM; port A output-enable and latch."""

    def __init__(self) -> None:
        self.ram = bytearray(0x4000)
        self.cpu_reset = False
        self.running = False
        self.oea = 0
        self.ioa_latch = 0
        self.eeprom_writes = 0       # must stay zero: the tool never touches the EEPROM
        self.log = []
        self.engine = SimulatedStationEngine()
        self._t0 = time.monotonic()

    def ctrl_out(self, req_type, req, value, index, data=b""):
        self.log.append(("out", req_type, req, value, index, bytes(data)))
        if req_type == 0x40 and req == REQ_FIRMWARE_LOAD:
            if value == CPUCS_ADDR:
                self.cpu_reset = bool(data and data[0] & 1)
                self.running = not self.cpu_reset and self.ram_loaded()
                return
            if not self.cpu_reset:
                raise Fx2Error("simulator: RAM write while the CPU is running is not allowed")
            if value + len(data) > len(self.ram):
                raise Fx2Error("simulator: write outside code RAM")
            self.ram[value:value + len(data)] = data
            return
        if not self.running:
            raise Fx2Error("simulator: vendor request to an unprogrammed device")
        if req_type == 0x40 and req == REQ_GPIO_SET:
            if (value | index) & ~LED_MASK or self.engine.map_state == "valid":
                raise Stalled("simulator: stall")
            self.ioa_latch = index
            self.oea = value
            return
        eng = self.engine
        if req_type == 0x40 and req in (REQ_MAP_BEGIN, REQ_MAP_PIN, REQ_MAP_PARAM, REQ_MAP_HEARTBEAT,
                                        REQ_MAP_COMMIT, REQ_KEY):
            ok = {
                REQ_MAP_BEGIN: lambda: eng.map_begin(),
                REQ_MAP_PIN: lambda: eng.map_pin(value & 0xFF, value >> 8, index & 0xFF),
                REQ_MAP_PARAM: lambda: eng.map_param(value & 0xFF, value >> 8, index),
                REQ_MAP_HEARTBEAT: lambda: eng.map_heartbeat(index),
                REQ_MAP_COMMIT: lambda: eng.map_commit(),
                REQ_KEY: lambda: eng.key(value & 0xFF, bool(index & 0xFF)),
            }[req]()
            if not ok:
                raise Stalled("simulator: request refused")
            return
        raise Stalled("simulator: stall")

    def sleep(self, seconds):
        """Virtual time: the engine runs one millisecond at a time."""
        self.engine.step(int(round(seconds * 1000)))

    def bus_reset(self):
        self.engine.bus_reset()

    def suspend(self):
        self.engine.suspend()

    def _pa(self):
        """(output-enable, pin levels) of port A: the engine's mapped pins override the latch."""
        oea, pins = self.oea, (self.ioa_latch & self.oea) | (0xFF & ~self.oea)
        for c, ch in enumerate(self.engine.ch):
            if ch.level is None:
                continue
            bit = 1 << c
            oea = (oea & ~bit) | (bit if ch.level in (0, 1) else 0)
            pins = (pins & ~bit) | (bit if ch.level in (1, 2) else 0)
        return oea, pins

    def ctrl_in(self, req_type, req, value, index, length):
        self.log.append(("in", req_type, req, value, index, length))
        if req_type == 0xC0 and req == REQ_FIRMWARE_LOAD:
            if not self.cpu_reset:
                raise Fx2Error("simulator: RAM readback only while the CPU is in reset")
            return bytes(self.ram[value:value + length])
        if not self.running:
            raise Fx2Error("simulator: vendor request to an unprogrammed device")
        if req_type == 0xC0 and req == REQ_GET_INFO:
            return SIM_INFO[:length]
        if req_type == 0xC0 and req == REQ_GET_TICK:
            elapsed_counts = int((time.monotonic() - self._t0) * 4_000_000)    # 250 ns counts
            ms, sub = divmod(elapsed_counts, 4000)
            return (ms & 0xFFFFFFFF).to_bytes(4, "little") + sub.to_bytes(2, "little") + b"\x20\x01"
        if req_type == 0xC0 and req == REQ_GPIO_GET:
            oea, pins = self._pa()           # released pins float high
            return bytes([oea, pins & 0xFF, 0xFF, 0xFF])[:length]
        eng = self.engine
        if req_type == 0xC0 and req == REQ_GET_STATUS:
            return eng.status()[:length]
        if req_type == 0xC0 and req == REQ_HEARTBEAT:
            eng.heartbeat(value, index & 0xFF)
            return eng.status()[:length]
        if req_type == 0xC0 and req == REQ_GET_EVENTS:
            out = bytes([len(eng.events) & 0xFF])
            for reason, c, ms in reversed(eng.events[-8:]):
                out += bytes([reason, c]) + (ms & 0xFFFF).to_bytes(2, "little")
            return out[:length]
        if req_type == 0xC0 and req == REQ_GET_EDGES:
            out = bytes([len(eng.edges) & 0xFF])
            for c, level, ms in reversed(eng.edges[-8:]):
                out += bytes([c | (level << 4)]) + (ms & 0xFFFF).to_bytes(2, "little") + b"\0\0"
            return out[:length]
        raise Stalled("simulator: stall")

    def ram_loaded(self) -> bool:
        return any(self.ram[:16])
