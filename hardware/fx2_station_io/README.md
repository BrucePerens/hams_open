# FX2LP station I/O firmware and shared core (hams_open)

Status: bring-up image, tested on one real board on 2026-10-30. SPDX: AGPL-3.0-or-later
(firmware and host tool). The schematic/hardware licence is still undecided
(`hams_com/docs/proposals/HAMS_USB_DEVICE_PROTOCOL.md` section 7).

This is the first piece of the "Station I/O" device type of the hams USB device family
(`HAMS_USB_DEVICE_PROTOCOL.md`, `RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md` sections 4.2 to 4.4).
It runs on an inexpensive CY7C68013A (EZ-USB FX2LP) board. The firmware is loaded into the
chip's RAM by the host every time, so nothing is stored and unplugging the board undoes it.

## What it does

- Loaded over USB with the standard Cypress vendor request 0xA0 (CPUCS hold, write code RAM,
  release; TRM 3.8), it renumerates (TRM 3.9) as **1209:0001**, the pid.codes TEST PID
  (development only; **0x7373 is not used until registered**). Strings: manufacturer
  `hams.com`, product `Hams Station I/O (FX2LP, test PID)`, serial `HAMS-FX2-TEST-0001`
  (placeholder: the FX2 has no unique ID; a per-board serial belongs to the later EEPROM step).
- Answers the standard requests (descriptors incl. device qualifier, other-speed and BOS,
  status, configuration, interface), the Microsoft OS 2.0 descriptor set (WinUSB compatible ID
  plus a DeviceInterfaceGUIDs property) and the WebUSB BOS capability (no landing page).
- Vendor requests (bmRequestType vendor, device):

  | bRequest | Dir | Meaning |
  |---|---|---|
  | 0x01 GET_INFO | IN | self-describing record (below) |
  | 0x02 GPIO_SET | OUT | wValue = output-enable mask, wIndex = levels, PA0/PA1 only; a pin not in the mask is released to input; anything outside bits 0-1 stalls |
  | 0x03 GPIO_GET | IN | 4 bytes: OEA, IOA pins, IOB pins, IOD pins |
  | 0x20 | IN | MS OS 2.0 descriptor set (wIndex 7) |
  | 0x21 | - | WebUSB vendor code (stalls: no landing page) |

  0xA0-0xAF are reserved by Cypress (TRM 3.8) and never used for our commands.
- Safe state: every port is an input at start, on USB bus reset (verified) and whenever a
  command releases the pins. No time-out and no heartbeat exist in this image (flags say so).

## GET_INFO record, protocol 0.1

Little endian; byte 0 is the length of the whole record, so a host ignores bytes it does not
know. Unknown major version: refuse the device.

| Off | Size | Field | This image |
|---|---|---|---|
| 0 | 1 | length | 23 |
| 1, 2 | 1+1 | protocol major, minor | 0, 1 |
| 3 | 2 | device type (sub-ID) | 0x0001 station I/O (provisional placeholder; no registry yet) |
| 5, 6, 7 | 3 | firmware major, minor, patch | 0.1.0 |
| 8 | 1 | hardware / board map id | 1 = Geeetech "Lcsoft Mini Board" variant |
| 9 | 4 | capability flags | 0x27: gpio_cmd 1, msos20 2, webusb 4, safe_on_reset 0x20 (hw_timeout 8 and heartbeat 0x10 clear) |
| 13 | 1 | safe-state kind | 0 = every output released to input |
| 14 | 2 | hardware time-out ms | 0 = none |
| 16 | 2 | heartbeat period ms | 0 = none |
| 18 | 1 | ports exposed (bit0 A, bit1 B, bit2 D) | 0x07 |
| 19 | 1 | port A pins the firmware may drive | 0x03 |
| 20 | 1 | endpoint count N | 1 |
| 21 | 2N | (endpoint address, role) pairs; role 1 = control/vendor commands | (0x00, 1) |

The layout is this directory's proposal for the protocol document, which defines the
required content but not the byte layout; adopt or change it there.

## Layout

- `core/` shared core: `fx2_regs.h` (registers, each cited to the TRM), `hams_usb.h`
  (protocol constants), `fx2_core.c` (clock, ReNumeration, endpoint zero, standard requests,
  dispatcher; polled, no interrupts), `descriptors.c` (descriptors at fixed even code-RAM
  addresses so SUDPTR can serve them).
- `core/fx2_timer.c` Timers 0/1/2 (TRM chapter 14), all CLKOUT/12 = 4 MHz (250 ns): Timer 2 auto-reload
  1 ms tick (polled), Timer 1 blocking delays, Timer 0 free-running stopwatch for the longest main-loop
  pass. Measured on the board 2026-10-30: firmware clock = host clock within 0.02 %, longest loop pass
  about 110 us (`tools/hams_fx2.py tick`, request GET_TICK 0x04).
- `station_io.c` the application: GET_INFO record, product string, PA0/PA1 commands.
- `tools/hams_fx2.py` loader and test tool (pyusb); `tools/fx2_simulator.py` software model.
- `tests/test_fx2_host.py` GET_INFO parser, Intel HEX, loader safety rules, simulator, and the
  compiled image (descriptor sizes and alignment, GET_INFO bytes equal to the simulator's).

## Build, load, test

    make                                   # SDCC 4.5.0, produces build/station_io.ihx
    python3 -m unittest tests/test_fx2_host.py
    sudo python3 tools/hams_fx2.py --port 3-1 load build/station_io.ihx
    sudo python3 tools/hams_fx2.py --port 3-1 info
    sudo python3 tools/hams_fx2.py --port 3-1 blink      # cycles PA0/PA1 and prints readback
    sudo python3 tools/hams_fx2.py --port 3-1 probe      # all inputs, prints PA/PB/PD

`--port` (bus-port path, here `3-1`) is mandatory: the tool touches only the one device with
04b4:8613 or 1209:0001 at that path. The loader writes only request 0xA0 to CPUCS (one byte)
and code RAM 0x0000-0x3FFF, verifies by readback, and never touches the EEPROM.
A board already running this firmware can be reloaded the same way (0xA0 is always handled by
the chip); otherwise unplug and replug to return to the ROM state.

## Verified on the real board (2026-10-30, CY7C68013A-56PVXC dev board, bus 3 port 1)

- Load of 2040 bytes, readback verified, re-enumeration as 1209:0001 high speed in under 2 s.
- `lsusb -v`: device, configuration (vendor class 0xFF, subclass 1, no endpoints), strings and
  the BOS descriptor with the WebUSB (UUID 3408b638-...) and MS OS 2.0 (UUID d8dd60df-...)
  capabilities all decode; GET_INFO parses as above; MS OS 2.0 set returns 162 bytes;
  unknown vendor requests and pins other than PA0/PA1 stall (Pipe error) and the device keeps working.
- Linux runtime autosuspend (2 s) suspends and resumes the device without trouble.
- **Pin facts.** With every pin an input, all of PA, PB and PD read high (0xFF), as with fx2lafw.
  PA0 and PA1 can be driven low and high and read back what was driven (PA=0xFC/0xFD/0xFE/0xFF
  for the four patterns); other pins stay inputs; a USB bus reset releases them (OEA 3 -> 0).
  **Confirmed by eye (Bruce, 2026-10-30): running `blink` makes the board's LEDs blink ("Works!").** The
  mapping of D1 and D2 to PA0 and PA1 and the polarity are consistent with the wiki-based inference (LEDs from Vcc
  through J1, 1 kohm: PA low = LED on) but which LED is which pin, and whether low means lit, was not recorded; to be
  noted the next time the board is on the bench (continuity from the LEDs and the J1 and J2 traces are still to do).
  With J1 fitted the released pins read high, which is consistent with LEDs to Vcc.

## Findings worth keeping (bugs found while bringing it up)

- **SUDPTRCTL state matters for buffered replies.** Descriptor replies use SDPAUTO=0 (manual
  length in EP0BCH:L, then SUDPTRL). With SDPAUTO left at 0, replies built in EP0BUF (GET_STATUS,
  our GPIO_GET) never completed; the host timed out, the kernel reset the board and it came back
  after a renumeration. Setting SUDPTRCTL = 1 before a buffer reply fixed it. TRM 15.12.27 says
  only that the byte count bits apply when SDPAUTO=0; it does not state this consequence.
- A board whose EP0 misbehaves looks dead to `open()` (EIO) until the kernel's reset and its own
  renumeration finish (about 6 s); wait, do not reload.
- The first `wait_for` returned the old device before it dropped off the bus; the loader now waits
  for a different USB address.

## Anchors (ADR 0090, `verify_anchors.py`)

Infrastructure anchors cited here as this module's contract: `[@ANCHOR: HAMS_FX2_parse_get_info]`
(GET_INFO parser), `[@ANCHOR: HAMS_FX2_parse_ihx]` (Intel HEX reader), `[@ANCHOR: HAMS_FX2_image_chunks]`
(code-RAM-only write plan), `[@ANCHOR: HAMS_FX2_load_ram]` (0xA0 loader with readback),
`[@ANCHOR: HAMS_FX2_tick]` (millisecond clock and loop-latency read),
`[@ANCHOR: HAMS_FX2_read_info]` and `[@ANCHOR: HAMS_FX2_gpio]` (host commands). The tools directory
is excluded from the per-function ratchet like other `tools/` directories; the C firmware is covered by the
compiled-image tests and the hardware run above.

## Not done here (see the to-do)

Shared core pieces still to write: I2C master helper, timers and watchdog, the GPIO function-map
engine, serial/EEPROM identity step. Station image: ptt channels with hardware time-out, heartbeat,
host-loss safe state. Host driver in radioctl (Rust, nusb). Supported-boards list with photos,
continuity check of J1/J2 and EEPROM read (needs Bruce at the board). No linking against
fx2lafw or fx2lib; the TRM is the only source (Document # 001-13670 Rev. *D, downloaded from
community.infineon.com, 2026-10-30).
