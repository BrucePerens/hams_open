# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Bruce Perens and the hams.com contributors.
"""A software model of the station I/O firmware's USB interface, for host-side tests.

It implements the Cypress 0xA0 firmware-load behaviour of the unprogrammed chip (RAM only,
CPUCS gate) and the vendor commands of the loaded image, independently of the C source, so
the host code can be exercised without a board. It is built from the protocol constants in
the README, not from the compiled image; tests/test_fx2_host.py compares it with the image.
"""
from __future__ import annotations

from hams_fx2 import (CPUCS_ADDR, REQ_FIRMWARE_LOAD, REQ_GET_INFO, REQ_GPIO_GET, REQ_GPIO_SET,
                      Fx2Error, LED_MASK, Transport)

SIM_INFO = bytes([
    23, 0, 1, 0x01, 0x00, 0, 1, 0, 1,
    0x27, 0, 0, 0,          # gpio_cmd | msos20 | webusb | safe_on_reset
    0, 0, 0, 0, 0, 0x07, LED_MASK, 1, 0x00, 1,
])


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
            if (value | index) & ~LED_MASK:
                raise Fx2Error("simulator: stall")
            self.ioa_latch = index
            self.oea = value
            return
        raise Fx2Error("simulator: stall")

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
        if req_type == 0xC0 and req == REQ_GPIO_GET:
            pins = (self.ioa_latch & self.oea) | (0xFF & ~self.oea)   # released pins float high
            return bytes([self.oea, pins & 0xFF, 0xFF, 0xFF])[:length]
        raise Fx2Error("simulator: stall")

    def ram_loaded(self) -> bool:
        return any(self.ram[:16])
