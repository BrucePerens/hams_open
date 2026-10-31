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
  | 0x04 GET_TICK | IN | 8 bytes: firmware ms (u32), 250 ns counts into the ms (u16), longest main-loop pass (u16) |
  | 0x05 GET_STATUS | IN | the status record (below) |
  | 0x06 HEARTBEAT | IN | wValue = sequence, wIndexL = commanded state; answers the status record |
  | 0x07 GET_EVENTS | IN | release log: count (u8), then newest first (reason, channel, ms16) x up to 8 |
  | 0x08 GET_EDGES | IN | output edge log: count (u8), then newest first (channel low nibble and level high nibble, ms16, 250 ns counts u16) x up to 8; level 2 = released |
  | 0x10 MAP_BEGIN | OUT | start a function map: releases everything, drops the heartbeat |
  | 0x11 MAP_PIN | OUT | wValueL = pin id (port << 4 or bit), wValueH = function (1 ptt, 2 keyline, 3 power), wIndexL = flags (bits 0-1 safe state: 0 low, 1 high, 2 released; bit 2 active high; bit 3 safe state defined) |
  | 0x12 MAP_PARAM | OUT | wValueL = pin id, wValueH = 1 max on-time ms, 2 min off ms, 3 lead ms, 4 lag ms, 5 partner (keyline pin id); wIndex = value |
  | 0x13 MAP_HEARTBEAT | OUT | wIndex = heartbeat lapse ms |
  | 0x14 MAP_COMMIT | OUT | validate and activate; stalls on error (code in the status) |
  | 0x15 KEY | OUT | wValueL = pin id, wIndexL = 1 key down, 0 key up; stalls when refused (reason in the status) |
  | 0x20 | IN | MS OS 2.0 descriptor set (wIndex 7) |
  | 0x21 | - | WebUSB vendor code (stalls: no landing page) |

  0xA0-0xAF are reserved by Cypress (TRM 3.8) and never used for our commands.
- Safe state: every port is an input at start. Once a map is active each mapped pin goes to the
  safe state its entry names (driven low, driven high or released) at commit and on every release.

## Function map, ptt channels, heartbeat (protocol 0.2)

All of this is enforced in the firmware; the host only asks. Pin ids: port << 4 | bit (A 0, B 1,
D 2). The board table (`station_io.c`) lets only **PA0 and PA1** be outputs; PA0 is channel 0 and
PA1 channel 1. The map is sent at load (MAP_BEGIN, MAP_PIN and MAP_PARAM per pin, MAP_HEARTBEAT,
MAP_COMMIT) and validated by the firmware, which stalls the refused step and records the code
(`[@ANCHOR: HAMS_FX2_function_map]`): a pin the board does not allow, a pin with two functions, a
function other than ptt/keyline/power, a safe state not defined, a driven safe state equal to the
active level, a ptt without a time-out (max on-time 0), a ptt partner that is not a free keyline, a
keyline nobody owns, lead or lag without a keyline, a zero heartbeat lapse, steps out of order.
Unlisted pins stay inputs. Direct GPIO_SET is refused while a map is active.

`ptt` channel (`[@ANCHOR: HAMS_FX2_ptt_channel]`, `[@ANCHOR: HAMS_FX2_ptt_timeout]`): KEY down
raises the line (active level) only if the heartbeat is alive and the minimum off time since the last release has
passed; the firmware releases it by itself when the maximum on-time (default 5000 ms, per ptt
channel from MAP_PARAM) has elapsed, counted from the key-down command, whatever the host does.
The time-out fires between max-1 ms and max ms after the command (1 ms tick granularity; early is
the safe direction). KEY down on a channel already keyed is accepted and changes nothing.

Heartbeat (`[@ANCHOR: HAMS_FX2_heartbeat]`): wValue = sequence, wIndexL = commanded state (bit per
channel; a ptt channel and its keyline carry the same bit, set while keyed). Armed by the first
good heartbeat; the firmware needs another within the lapse time (default 2000 ms). A heartbeat is
good only if its sequence differs from the previous one and its commanded state equals the board's
keyed state; otherwise the board releases every channel (reason sequence or mismatch), disarms, and
sets `out_of_sync` when the host claimed a channel keyed that the board had already released
(for example after a time-out; the first release reason is kept). A good heartbeat re-arms. A
lapse releases all (reason lapse). Every heartbeat is answered with the status record.

Release reasons (`[@ANCHOR: HAMS_FX2_release_on_usb]`): 1 timeout, 2 heartbeat lapse, 3 repeated
sequence, 4 commanded-state mismatch, 5 USB bus reset, 6 USB suspend (3 ms of bus silence, SUSP),
7 host key-up, 8 map cleared. Each keyed channel released is logged (GET_EVENTS, status counters),
a release with nothing keyed is not. A fault release drops every line at once, primary line first.

Lead and lag (`[@ANCHOR: HAMS_FX2_lead_lag]`): a ptt channel with a partner keyline asserts the keyline
at key-down, the ptt line after the lead time; at key-up it drops ptt, then the keyline after the
lag time. The maximum on-time counts from key-down. A fault drops both lines together.

### Status record (23 bytes, GET_STATUS and every heartbeat answer)

0 length; 1 flags (1 map valid, 2 heartbeat alive, 4 out of sync, 8 map loading); 2 keyed mask;
3 mask of lines at their active level; 4 last release reason; 5 its channel; 6 release count (wraps);
7 last map error; 8 last key refusal; 9, 10 channel states (0 idle, 1 lead, 2 on, 3 lag); 11-12 last
heartbeat sequence; 13-14 firmware ms mod 65536; 15-16 and 17-18 on-time left per channel (0xFFFF
none); 19-20 heartbeat time left (0xFFFF not armed); 21-22 longest main-loop pass in 250 ns.

### Timing: what is measured, and its limits

There is no second instrument: while this firmware runs the board cannot also be a logic analyzer
(fx2lafw). So timing is verified two ways. (1) Coarse, from the host: pin states read back after
known sleeps (tests check the keyline-before-ptt order and the lag hold). (2) Exact, from the
firmware: every output edge is stamped with the 1 ms Timer 2 tick plus the 250 ns Timer 2 count
inside the ms (`GET_EDGES`), so lead, lag, on-time and lapse are computed from the stamps; a
repeat with different host pauses gives the same lead. Honest limits: the stamp is taken when the
firmware changes the pin, not when the voltage moves (a few tens of ns of pin delay and whatever
the output stage adds are not seen); tick granularity makes every timer fire up to 1 ms early or
late relative to its command; the main loop is polled, the longest pass seen is about 0.3 ms (in
the status), so a timer can be serviced up to that late. Measured on the board: time-out 1999.2 ms
for 2000, lead and lag within the tolerances in `tests/test_fx2_ptt.py` (TOL 3 ms). A logic analyzer
measurement of the pin voltages needs a second board or a scope and is not done. Which of D1 and D2 is
on PA0 and PA1, and whether low means lit, is still unconfirmed, so tests use the pin readback and no test depends on the LEDs.

## GET_INFO record, protocol 0.2 (0.1 plus the time-out and heartbeat fields)

Little endian; byte 0 is the length of the whole record, so a host ignores bytes it does not
know. Unknown major version: refuse the device.

| Off | Size | Field | This image |
|---|---|---|---|
| 0 | 1 | length | 23 |
| 1, 2 | 1+1 | protocol major, minor | 0, 2 |
| 3 | 2 | device type (sub-ID) | 0x0001 station I/O (provisional placeholder; no registry yet) |
| 5, 6, 7 | 3 | firmware major, minor, patch | 0.2.0 |
| 8 | 1 | hardware / board map id | 1 = Geeetech "Lcsoft Mini Board" variant |
| 9 | 4 | capability flags | 0x3F: gpio_cmd 1, msos20 2, webusb 4, hw_timeout 8, heartbeat 0x10, safe_on_reset 0x20 |
| 13 | 1 | safe-state kind | 0 = every output released to input, 1 = per map entry (this image) |
| 14 | 2 | default maximum on-time ms | 5000 |
| 16 | 2 | default heartbeat lapse ms | 2000 |
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
- `core/io_engine.c` function-map engine (map validation, ptt/keyline/power channels, heartbeat, logs).
- `station_io.c` the application: GET_INFO record, product string, board table, vendor requests.
- `tools/hams_fx2.py` loader and test tool (pyusb); `tools/fx2_simulator.py` software model.
- `tests/test_fx2_ptt.py` map, ptt, heartbeat, lead/lag behaviour on the simulator and (HAMS_FX2_PORT=3-1, as root) the real board;
  `tests/test_fx2_host.py` GET_INFO parser, Intel HEX, loader safety rules, simulator, and the
  compiled image (descriptor sizes and alignment, GET_INFO bytes equal to the simulator's).

## Build, load, test

    make                                   # SDCC 4.5.0, produces build/station_io.ihx
    python3 -B tests/test_fx2_host.py; python3 -B tests/test_fx2_ptt.py      # simulator
    sudo env PYTHONDONTWRITEBYTECODE=1 HAMS_FX2_PORT=3-1 python3 -B tests/test_fx2_ptt.py BoardPtt   # real board
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
  **Confirmed by eye (Bruce, 2026-10-30): `blink` makes the LEDs blink, and driving PA0 alone (low for 0.7 s, then
  released, repeated) blinked the LEFT of the two LEDs with the USB connector at the bottom, which is D1 (D1 is the
  top LED when the board is held with the USB connector on the left, as in the photo).** So PA0 drives D1, and the
  LED is lit when the pin is driven low and dark when it is released, as the wiki-based inference said (LED from Vcc
  through J1 and 1 kohm). D2 on PA1 was seen to blink in the combined patterns but was not singled out on its own in
  the last test; continuity from the LEDs and the J1 and J2 traces are still to do.
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

- **SDCC small model runs out of direct RAM.** Locals of the engine are not overlaid across
  functions (74 bytes of DSEG), the link fails ("Could not get 74 consecutive bytes"). `--stack-auto`
  bloats the code past the descriptors at 0x2000; the fix was to keep time stamps 16 bit
  (intervals are all under 65 s) and keep the logs in `__xdata`, which the Makefile links at
  0x3000 (`--xram-loc`) because on the FX2 code RAM and external data RAM are the same 0x0000-0x3FFF
  memory. Code must stay below 0x2000 (descriptors); it ends near 0x1A00.
- Linux does not autosuspend a device while a process holds its usbfs handle; the suspend test
  closes the handle first. A suspended board with a keyed line released it (reason 6, verified).

## Anchors (ADR 0090, `verify_anchors.py`)

Infrastructure anchors cited here as this module's contract: `[@ANCHOR: HAMS_FX2_parse_get_info]`
(GET_INFO parser), `[@ANCHOR: HAMS_FX2_parse_ihx]` (Intel HEX reader), `[@ANCHOR: HAMS_FX2_image_chunks]`
(code-RAM-only write plan), `[@ANCHOR: HAMS_FX2_load_ram]` (0xA0 loader with readback),
`[@ANCHOR: HAMS_FX2_tick]` (millisecond clock and loop-latency read),
`[@ANCHOR: HAMS_FX2_get_info_ptt]` (GET_INFO time-out/heartbeat fields), the map/ptt/heartbeat anchors named above,
`[@ANCHOR: HAMS_FX2_read_info]` and `[@ANCHOR: HAMS_FX2_gpio]` (host commands). The tools directory
is excluded from the per-function ratchet like other `tools/` directories; the C firmware is covered by the
compiled-image tests and the hardware run above.

## Not done here (see the to-do)

Shared core pieces still to write: I2C master helper, watchdog, serial/EEPROM identity step.
Station image: the other pins (only PA0/PA1 are allowed outputs), sensor inputs, band outputs. Host driver in radioctl (Rust, nusb). Supported-boards list with photos,
continuity check of J1/J2 and EEPROM read (needs Bruce at the board). No linking against
fx2lafw or fx2lib; the TRM is the only source (Document # 001-13670 Rev. *D, downloaded from
community.infineon.com, 2026-10-30).
