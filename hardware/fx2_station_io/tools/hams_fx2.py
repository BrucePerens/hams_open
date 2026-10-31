#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright (C) 2026 Bruce Perens and the hams.com contributors.
"""Host-side loader and test tool for the hams FX2LP station I/O firmware.

Loads an Intel HEX image into the RAM of a Cypress CY7C68013A with the standard Cypress
vendor request 0xA0 (EZ-USB Technical Reference Manual, Document # 001-13670 Rev. *D,
section 3.8): hold the CPU in reset through CPUCS (0xE600), write code RAM, release.
Nothing else is ever written: not the configuration EEPROM, not any other register.
After release the firmware re-enumerates as 1209:0001 (pid.codes test PID), and this tool
reads the GET_INFO record (hams_com docs/proposals/HAMS_USB_DEVICE_PROTOCOL.md section 4)
and drives the two LED pins.

The USB access goes through a small Transport interface so the same code runs against a
simulator (tests) and against the real board (pyusb).
"""
from __future__ import annotations

import argparse
import dataclasses
import sys
import time
from typing import Dict, List, Optional, Tuple

BOOT_VID, BOOT_PID = 0x04B4, 0x8613     # unprogrammed FX2 (TRM 3.3, Default USB Device)
HAMS_VID, HAMS_PID = 0x1209, 0x0001     # pid.codes test PID; development only

REQ_FIRMWARE_LOAD = 0xA0                # TRM 3.8
CPUCS_ADDR = 0xE600                     # TRM 15.5.1
CODE_RAM_END = 0x4000                   # on-chip program/data RAM 0x0000-0x3FFF (TRM 3.8)
CHUNK = 1024                            # bytes per request, well below the 16 KiB RAM

REQ_GET_INFO = 0x01
REQ_GPIO_SET = 0x02
REQ_GPIO_GET = 0x03
REQ_GET_TICK = 0x04
REQ_GET_STATUS = 0x05
REQ_HEARTBEAT = 0x06
REQ_GET_EVENTS = 0x07
REQ_GET_EDGES = 0x08
REQ_MAP_BEGIN = 0x10
REQ_MAP_PIN = 0x11
REQ_MAP_PARAM = 0x12
REQ_MAP_HEARTBEAT = 0x13
REQ_MAP_COMMIT = 0x14
REQ_KEY = 0x15
REQ_MSOS20 = 0x20
INFO_ADDR = 0x2300                      # fixed address of the record in the image

CAP_NAMES = {
    0x1: "gpio_cmd", 0x2: "msos20", 0x4: "webusb",
    0x8: "hw_timeout", 0x10: "heartbeat", 0x20: "safe_on_reset",
}
SAFE_KINDS = {0: "release_all", 1: "per_map"}
EP_ROLES = {1: "control"}
DEVICE_TYPES = {0x0001: "station_io"}
LED_MASK = 0x03                         # PA0 and PA1 are the only pins this image drives


class Fx2Error(Exception):
    """A protocol, safety or hardware problem; the message says what to do."""


class Stalled(Fx2Error):
    """The device answered a control request with a STALL (it refused the request)."""


class Transport:
    """Control-transfer access to one device."""

    def sleep(self, seconds: float) -> None:
        """Let time pass on the device's clock (the simulator advances its virtual clock)."""
        time.sleep(seconds)

    def ctrl_in(self, req_type: int, req: int, value: int, index: int, length: int) -> bytes:
        raise NotImplementedError

    def ctrl_out(self, req_type: int, req: int, value: int, index: int, data: bytes = b"") -> None:
        raise NotImplementedError


@dataclasses.dataclass
class GetInfo:
    """Parsed GET_INFO record, protocol 0.1 (layout in README.md)."""
    length: int
    proto_major: int
    proto_minor: int
    device_type: int
    fw_version: Tuple[int, int, int]
    hw_id: int
    capabilities: int
    safe_state_kind: int
    hw_timeout_ms: int
    heartbeat_ms: int
    ports_exposed: int
    drivable_a: int
    endpoints: List[Tuple[int, int]]

    def capability_names(self) -> List[str]:
        return [name for bit, name in CAP_NAMES.items() if self.capabilities & bit]


# [@ANCHOR: HAMS_FX2_parse_get_info]
def parse_get_info(data: bytes) -> GetInfo:
    """Parse a GET_INFO record. Fields past those known are ignored (minor versions only add)."""
    if len(data) < 21:
        raise Fx2Error(f"GET_INFO record too short: {len(data)} bytes")
    length = data[0]
    if length > len(data) or length < 21:
        raise Fx2Error(f"GET_INFO length byte {length} does not fit the {len(data)} bytes received")
    if data[1] != 0:
        raise Fx2Error(f"unsupported protocol major version {data[1]}; refusing the device")
    n_eps = data[20]
    if 21 + 2 * n_eps > length:
        raise Fx2Error("GET_INFO endpoint list runs past the record length")
    return GetInfo(
        length=length,
        proto_major=data[1],
        proto_minor=data[2],
        device_type=data[3] | (data[4] << 8),
        fw_version=(data[5], data[6], data[7]),
        hw_id=data[8],
        capabilities=int.from_bytes(data[9:13], "little"),
        safe_state_kind=data[13],
        hw_timeout_ms=data[14] | (data[15] << 8),
        heartbeat_ms=data[16] | (data[17] << 8),
        ports_exposed=data[18],
        drivable_a=data[19],
        endpoints=[(data[21 + 2 * i], data[22 + 2 * i]) for i in range(n_eps)],
    )


# [@ANCHOR: HAMS_FX2_parse_ihx]
def parse_ihx(text: str) -> Dict[int, int]:
    """Intel HEX text to {address: byte}. Only data (00) and end-of-file (01) records are legal."""
    image: Dict[int, int] = {}
    for lineno, line in enumerate(text.splitlines(), 1):
        line = line.strip()
        if not line:
            continue
        if not line.startswith(":"):
            raise Fx2Error(f"hex line {lineno}: no colon")
        raw = bytes.fromhex(line[1:])
        if len(raw) < 5 or raw[0] != len(raw) - 5 or sum(raw) & 0xFF:
            raise Fx2Error(f"hex line {lineno}: bad length or checksum")
        count, addr, rtype = raw[0], (raw[1] << 8) | raw[2], raw[3]
        if rtype == 1:
            return image
        if rtype != 0:
            raise Fx2Error(f"hex line {lineno}: unsupported record type {rtype}")
        for i in range(count):
            image[addr + i] = raw[4 + i]
    raise Fx2Error("hex file has no end-of-file record")


# [@ANCHOR: HAMS_FX2_image_chunks]
def image_chunks(image: Dict[int, int], max_gap: int = 0x400) -> List[Tuple[int, bytes]]:
    """Group an image into contiguous (address, bytes) writes, zero-filling gaps up to max_gap.

    Refuses anything outside the on-chip code RAM (0x0000-0x3FFF): the loader never writes the
    CPUCS register or EEPROM through this path, and never any other address.
    """
    if not image:
        raise Fx2Error("empty firmware image")
    if max(image) >= CODE_RAM_END:
        raise Fx2Error(f"image byte at 0x{max(image):04X} is outside code RAM 0x0000-0x3FFF")
    chunks: List[Tuple[int, bytearray]] = []
    for addr in sorted(image):
        if chunks:
            start, buf = chunks[-1]
            gap = addr - (start + len(buf))
            if gap <= max_gap:
                buf.extend(bytes(gap))
                buf.append(image[addr])
                continue
        chunks.append((addr, bytearray([image[addr]])))
    out: List[Tuple[int, bytes]] = []
    for start, buf in chunks:
        for off in range(0, len(buf), CHUNK):
            out.append((start + off, bytes(buf[off:off + CHUNK])))
    return out


# [@ANCHOR: HAMS_FX2_load_ram]
def load_ram(t: Transport, image: Dict[int, int]) -> int:
    """Hold the CPU in reset, write the image to code RAM, verify by readback, release.

    Only request 0xA0 is used (TRM 3.8): a one-byte write to CPUCS and writes/reads of code
    RAM. Returns the number of bytes written.
    """
    chunks = image_chunks(image)
    t.ctrl_out(0x40, REQ_FIRMWARE_LOAD, CPUCS_ADDR, 0, b"\x01")        # 8051RES = 1
    total = 0
    for addr, data in chunks:
        t.ctrl_out(0x40, REQ_FIRMWARE_LOAD, addr, 0, data)
        total += len(data)
    for addr, data in chunks:
        back = t.ctrl_in(0xC0, REQ_FIRMWARE_LOAD, addr, 0, len(data))
        if bytes(back) != data:
            raise Fx2Error(f"RAM readback mismatch at 0x{addr:04X}; CPU left in reset, "
                           "unplug the board to recover")
    t.ctrl_out(0x40, REQ_FIRMWARE_LOAD, CPUCS_ADDR, 0, b"\x00")        # 8051RES = 0
    return total


# [@ANCHOR: HAMS_FX2_read_info]
def read_info(t: Transport) -> GetInfo:
    return parse_get_info(bytes(t.ctrl_in(0xC0, REQ_GET_INFO, 0, 0, 64)))


# [@ANCHOR: HAMS_FX2_gpio]
def gpio_set(t: Transport, oe_mask: int, levels: int) -> None:
    """Drive PA0/PA1: bits in oe_mask become outputs at the given levels; others are released."""
    if (oe_mask | levels) & ~LED_MASK:
        raise Fx2Error("only PA0 and PA1 may be driven")
    t.ctrl_out(0x40, REQ_GPIO_SET, oe_mask, levels)


def gpio_get(t: Transport) -> Dict[str, int]:
    d = bytes(t.ctrl_in(0xC0, REQ_GPIO_GET, 0, 0, 4))
    if len(d) != 4:
        raise Fx2Error(f"GPIO_GET returned {len(d)} bytes")
    return {"oea": d[0], "pa": d[1], "pb": d[2], "pd": d[3]}


@dataclasses.dataclass
class Tick:
    """The firmware's millisecond clock (Timer 2) and worst-case main-loop latency (Timer 0)."""
    ms: int
    sub: int              # 250 ns counts into the current millisecond (0-3999)
    loop_max: int         # longest pass of the main loop, 250 ns counts (0xFFFF = saturated)

    @property
    def seconds(self) -> float:
        return self.ms / 1000.0 + self.sub * 250e-9

    @property
    def loop_max_us(self) -> float:
        return self.loop_max * 0.25


# [@ANCHOR: HAMS_FX2_tick]
def read_tick(t: Transport) -> Tick:
    d = bytes(t.ctrl_in(0xC0, REQ_GET_TICK, 0, 0, 8))
    if len(d) != 8:
        raise Fx2Error(f"GET_TICK returned {len(d)} bytes")
    return Tick(int.from_bytes(d[0:4], "little"), int.from_bytes(d[4:6], "little"),
                int.from_bytes(d[6:8], "little"))


PIN_PA0, PIN_PA1 = 0x00, 0x01           # pin id = port << 4 | bit (port 0 = A, 1 = B, 2 = D)
FUNC_NONE, FUNC_PTT, FUNC_KEYLINE, FUNC_POWER = 0, 1, 2, 3
SAFE_LOW, SAFE_HIGH, SAFE_RELEASED = 0, 1, 2
PARAM_MAX_ON_MS, PARAM_MIN_OFF_MS, PARAM_LEAD_MS, PARAM_LAG_MS, PARAM_PARTNER = 1, 2, 3, 4, 5
FLAG_ACTIVE_HIGH, FLAG_SAFE_DEFINED = 0x04, 0x08
STATUS_LEN = 23

RELEASE_REASONS = {
    0: "none", 1: "timeout", 2: "heartbeat_lapse", 3: "heartbeat_sequence",
    4: "heartbeat_mismatch", 5: "usb_reset", 6: "usb_suspend", 7: "host_command", 8: "map_cleared",
}
MAP_ERRORS = {
    0: "none", 1: "pin_not_allowed", 2: "pin_has_two_functions", 3: "bad_function",
    4: "safe_state_undefined", 5: "safe_state_is_active_level", 6: "bad_parameter",
    7: "bad_partner", 8: "no_timeout", 9: "no_heartbeat", 10: "not_loading",
    11: "lead_lag_without_keyline",
}
REFUSALS = {0: "none", 1: "no_map", 2: "no_heartbeat", 3: "minimum_off_time", 4: "busy",
            5: "bad_channel"}
REL_TIMEOUT, REL_HB_LAPSE, REL_HB_SEQUENCE, REL_HB_MISMATCH = 1, 2, 3, 4
REL_USB_RESET, REL_USB_SUSPEND, REL_HOST, REL_MAP_CLEARED = 5, 6, 7, 8
CH_ST = {0: "idle", 1: "lead", 2: "on", 3: "lag"}


@dataclasses.dataclass
class Status:
    """The status record (README, "Status record")."""
    map_valid: bool
    hb_alive: bool
    out_of_sync: bool
    map_loading: bool
    keyed: int              # logical state: bit per channel (a ptt channel and its keyline together)
    active: int             # lines currently at their active level, bit per channel
    last_reason: int
    last_channel: int
    release_count: int
    map_error: int
    refusal: int
    state: List[int]
    last_seq: int
    ms: int                 # firmware clock, low 16 bits
    remaining_ms: List[int]  # maximum on-time left per channel (0xFFFF = not keyed)
    hb_remaining_ms: int    # heartbeat lapse time left (0xFFFF = no heartbeat running)
    loop_max: int

    @property
    def reason_name(self) -> str:
        return RELEASE_REASONS.get(self.last_reason, f"unknown_{self.last_reason}")


def _u16(d: bytes, i: int) -> int:
    return d[i] | (d[i + 1] << 8)


def parse_status(d: bytes) -> Status:
    if len(d) < STATUS_LEN or d[0] < STATUS_LEN:
        raise Fx2Error(f"status record too short: {len(d)} bytes")
    return Status(
        map_valid=bool(d[1] & 1), hb_alive=bool(d[1] & 2), out_of_sync=bool(d[1] & 4),
        map_loading=bool(d[1] & 8), keyed=d[2], active=d[3], last_reason=d[4],
        last_channel=d[5], release_count=d[6], map_error=d[7], refusal=d[8],
        state=[d[9], d[10]], last_seq=_u16(d, 11), ms=_u16(d, 13), remaining_ms=[_u16(d, 15), _u16(d, 17)],
        hb_remaining_ms=_u16(d, 19), loop_max=_u16(d, 21))


@dataclasses.dataclass
class PinSpec:
    """One map entry: a pin, its function, safe state and timers."""
    pin: int
    func: int
    safe: int = SAFE_LOW
    active_high: bool = True
    safe_defined: bool = True
    max_on_ms: Optional[int] = None
    min_off_ms: Optional[int] = None
    lead_ms: Optional[int] = None
    lag_ms: Optional[int] = None
    partner: Optional[int] = None


# [@ANCHOR: HAMS_FX2_function_map]
def load_map(t: Transport, pins: List[PinSpec], heartbeat_ms: Optional[int] = None) -> None:
    """Send a function map and commit it. The firmware validates; a refusal raises Fx2Error with
    the firmware's own error code (a pin with two functions, an undefined safe state, a pin the
    board map does not allow, a ptt without a time-out...). Everything is released on refusal."""
    steps: List[Tuple[int, int, int]] = [(REQ_MAP_BEGIN, 0, 0)]
    for p in pins:
        flags = (p.safe & 3) | (FLAG_ACTIVE_HIGH if p.active_high else 0) \
            | (FLAG_SAFE_DEFINED if p.safe_defined else 0)
        steps.append((REQ_MAP_PIN, p.pin | (p.func << 8), flags))
        for pid, val in ((PARAM_MAX_ON_MS, p.max_on_ms), (PARAM_MIN_OFF_MS, p.min_off_ms),
                         (PARAM_LEAD_MS, p.lead_ms), (PARAM_LAG_MS, p.lag_ms),
                         (PARAM_PARTNER, p.partner)):
            if val is not None:
                steps.append((REQ_MAP_PARAM, p.pin | (pid << 8), val))
    if heartbeat_ms is not None:
        steps.append((REQ_MAP_HEARTBEAT, 0, heartbeat_ms))
    steps.append((REQ_MAP_COMMIT, 0, 0))
    for req, value, index in steps:
        try:
            t.ctrl_out(0x40, req, value, index)
        except Stalled as exc:
            code = get_status(t).map_error
            raise Fx2Error(f"map refused by the firmware: {MAP_ERRORS.get(code, code)}") from exc


def get_status(t: Transport) -> Status:
    return parse_status(bytes(t.ctrl_in(0xC0, REQ_GET_STATUS, 0, 0, 64)))


def heartbeat(t: Transport, seq: int, commanded: int) -> Status:
    """One heartbeat: the sequence number and the commanded state of every channel (bit per
    channel; a ptt channel and its keyline carry the same bit). Returns the board's own state."""
    return parse_status(bytes(t.ctrl_in(0xC0, REQ_HEARTBEAT, seq & 0xFFFF, commanded & 0xFF, 64)))


def key(t: Transport, pin: int, down: bool) -> None:
    """Key down or up. Raises Stalled when the firmware refuses (see Status.refusal)."""
    t.ctrl_out(0x40, REQ_KEY, pin, 1 if down else 0)


def get_events(t: Transport) -> Tuple[int, List[Tuple[int, int, int]]]:
    """(total release count mod 256, [(reason, channel, ms16)] newest first)."""
    d = bytes(t.ctrl_in(0xC0, REQ_GET_EVENTS, 0, 0, 64))
    n = (len(d) - 1) // 4
    return d[0], [(d[1 + 4 * i], d[2 + 4 * i], d[3 + 4 * i] | (d[4 + 4 * i] << 8)) for i in range(n)]


def get_edges(t: Transport) -> Tuple[int, List[Tuple[int, int, float]]]:
    """(total edge count mod 256, [(channel, level, ms)] newest first). Level 0 or 1 is a driven
    level, 2 is released to input. ms is the firmware clock modulo 65536 with 250 ns resolution."""
    d = bytes(t.ctrl_in(0xC0, REQ_GET_EDGES, 0, 0, 64))
    n = (len(d) - 1) // 5
    out = []
    for i in range(n):
        o = 1 + 5 * i
        ms = d[o + 1] | (d[o + 2] << 8)
        sub = d[o + 3] | (d[o + 4] << 8)
        out.append((d[o] & 0x0F, d[o] >> 4, ms + sub / 4000.0))
    return d[0], out


class Heartbeater:
    """Sequence-numbered heartbeats with the commanded state, as the relay's transmit task sends."""

    def __init__(self, t: Transport) -> None:
        self.t = t
        self.seq = 0
        self.commanded = 0

    def beat(self) -> Status:
        self.seq = (self.seq + 1) & 0xFFFF
        return heartbeat(self.t, self.seq, self.commanded)


class UsbTransport(Transport):
    """pyusb transport for one device. Control transfers only; no interface is claimed."""

    def __init__(self, dev) -> None:
        self.dev = dev

    def ctrl_in(self, req_type, req, value, index, length):
        import usb.core
        try:
            return bytes(self.dev.ctrl_transfer(req_type, req, value, index, length, timeout=2000))
        except usb.core.USBError as exc:
            if exc.errno == 32:             # EPIPE: the device stalled the request
                raise Stalled(f"request 0x{req:02x} stalled") from exc
            raise

    def ctrl_out(self, req_type, req, value, index, data=b""):
        import usb.core
        try:
            self.dev.ctrl_transfer(req_type, req, value, index, bytes(data) if data else None,
                                   timeout=2000)
        except usb.core.USBError as exc:
            if exc.errno == 32:
                raise Stalled(f"request 0x{req:02x} stalled") from exc
            raise


def find_device(vid: int, pid: int, port: str):
    """The single device with vid:pid at the given bus-port path such as '3-1'; never any other."""
    import usb.core
    found = []
    for dev in usb.core.find(find_all=True, idVendor=vid, idProduct=pid):
        path = f"{dev.bus}-" + ".".join(str(p) for p in (dev.port_numbers or ()))
        if path == port:
            found.append(dev)
    if len(found) != 1:
        return None
    return found[0]


def wait_for(vid: int, pid: int, port: str, seconds: float, not_address: Optional[int] = None):
    """Wait for vid:pid at port; with not_address, ignore a device still at that old address."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        dev = find_device(vid, pid, port)
        if dev is not None and dev.address != not_address:
            return dev
        time.sleep(0.2)
    return None


def cmd_load(args) -> int:
    dev = find_device(BOOT_VID, BOOT_PID, args.port)
    if dev is None:
        # Request 0xA0 is always handled by the chip itself (TRM 3.8), so a board already
        # running our firmware can be reloaded the same way without unplugging it.
        dev = find_device(HAMS_VID, HAMS_PID, args.port)
    if dev is None:
        raise Fx2Error(f"no {BOOT_VID:04x}:{BOOT_PID:04x} or {HAMS_VID:04x}:{HAMS_PID:04x} at "
                       f"port {args.port}; nothing was written")
    with open(args.ihx, encoding="ascii") as fh:
        image = parse_ihx(fh.read())
    old_address = dev.address
    n = load_ram(UsbTransport(dev), image)
    print(f"loaded {n} bytes into code RAM; waiting for {HAMS_VID:04x}:{HAMS_PID:04x} at {args.port}")
    dev2 = wait_for(HAMS_VID, HAMS_PID, args.port, 8.0, not_address=old_address)
    if dev2 is None:
        raise Fx2Error("board did not re-enumerate in 8 s; unplug and replug it to restore the ROM state")
    print("re-enumerated")
    return 0


def _open_hams(args) -> Transport:
    dev = find_device(HAMS_VID, HAMS_PID, args.port)
    if dev is None:
        raise Fx2Error(f"no {HAMS_VID:04x}:{HAMS_PID:04x} at port {args.port}; run load first")
    return UsbTransport(dev)


def cmd_info(args) -> int:
    info = read_info(_open_hams(args))
    print(info)
    print("capabilities:", ", ".join(info.capability_names()))
    return 0


def cmd_gpio(args) -> int:
    t = _open_hams(args)
    gpio_set(t, int(args.oe, 0), int(args.levels, 0))
    print(gpio_get(t))
    return 0


def cmd_blink(args) -> int:
    t = _open_hams(args)
    try:
        for i in range(args.count):
            for oe, lv, label in ((3, 0, "PA0 low, PA1 low"), (3, 1, "PA0 high, PA1 low"),
                                  (3, 2, "PA0 low, PA1 high"), (3, 3, "PA0 high, PA1 high")):
                gpio_set(t, oe, lv)
                st = gpio_get(t)
                print(f"{label}: readback PA=0x{st['pa']:02x} OEA=0x{st['oea']:02x}")
                time.sleep(args.delay)
    finally:
        gpio_set(t, 0, 0)       # release both pins to inputs
        print("released:", gpio_get(t))
    return 0


def cmd_tick(args) -> int:
    t = _open_hams(args)
    a = read_tick(t)
    h0 = time.monotonic()
    time.sleep(args.seconds)
    b = read_tick(t)
    h1 = time.monotonic()
    dev = (b.seconds - a.seconds) / (h1 - h0)
    print(f"firmware {b.seconds - a.seconds:.4f} s, host {h1 - h0:.4f} s, ratio {dev:.5f}; "
          f"longest loop pass {b.loop_max_us:.1f} us")
    return 0


def cmd_probe(args) -> int:
    t = _open_hams(args)
    gpio_set(t, 0, 0)
    st = gpio_get(t)
    print(f"all inputs: PA=0x{st['pa']:02x} PB=0x{st['pb']:02x} PD=0x{st['pd']:02x} OEA=0x{st['oea']:02x}")
    return 0


def main(argv: Optional[List[str]] = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--port", required=True, help="USB bus-port path of the board, e.g. 3-1")
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("load", help="load an .ihx into RAM and wait for re-enumeration")
    p.add_argument("ihx")
    p.set_defaults(fn=cmd_load)
    sub.add_parser("info", help="read and print GET_INFO").set_defaults(fn=cmd_info)
    p = sub.add_parser("gpio", help="set PA0/PA1: output-enable mask and levels")
    p.add_argument("oe")
    p.add_argument("levels")
    p.set_defaults(fn=cmd_gpio)
    p = sub.add_parser("blink", help="cycle the four PA0/PA1 patterns, then release")
    p.add_argument("--count", type=int, default=3)
    p.add_argument("--delay", type=float, default=0.6)
    p.set_defaults(fn=cmd_blink)
    p = sub.add_parser("tick", help="compare the firmware millisecond clock with the host clock")
    p.add_argument("--seconds", type=float, default=2.0)
    p.set_defaults(fn=cmd_tick)
    sub.add_parser("probe", help="release outputs and print all input pin states").set_defaults(fn=cmd_probe)
    args = ap.parse_args(argv)
    try:
        return args.fn(args)
    except Fx2Error as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
