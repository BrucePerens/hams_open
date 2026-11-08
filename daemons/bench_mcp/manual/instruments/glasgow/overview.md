---
title: Glasgow overview
summary: What the Glasgow is, what the tools do with it, what they cannot do, and how the safety model works.
see_also: [instruments/glasgow/setup, instruments/glasgow/limits, instruments/glasgow/operation]
instrument: glasgow
verified: measured on the unit (Glasgow revC3, device firmware API level 3, glasgow software 70cb38c9), 2026-11-08
---

# Glasgow overview

The Glasgow Interface Explorer is a USB device with a Lattice iCE40 FPGA and an FX2 USB controller. Each of its two ports (A and B) has 8 pins behind level shifters whose I/O voltage (Vio) the host sets; the pins and the Vio rail are the only connection to the target. An *applet* is a gateware image plus host code; the `glasgow` command line builds it, loads it and runs it. The bench tools drive the `glasgow` command line of a pinned, isolated install ([setup](instruments/glasgow/setup)) and return numbers, never raw logs.

Written against revision C3 (serial prefix `C3`), glasgow software commit `70cb38c9`. Revision D differs in its voltage range (0.9 to 5.5 V) and is accepted but untested.

## Tools

| Tool | Tier | What |
|---|---|---|
| `bench_glasgow_list`, `_status`, `_applets` | read_only | the device, its link, port voltages as the device reports them, the applet table |
| `bench_glasgow_safe_state` | read_config | `glasgow safe` and a readback proving 0 V |
| `bench_glasgow_prepare` | read_config | build and cache an applet's gateware |
| `bench_glasgow_uart_sniff`, `_logic_capture` | read_config | passive capture to a file with a summary |
| `bench_glasgow_port_set` | full | power a port at an exact voltage |
| `bench_glasgow_i2c_scan`, `_i2c_read`, `_i2c_write`, `_spi_jedec_id`, `_uart_transact`, `_gpio`, `_pattern`, `_jtag_scan`, `_swd_idcode` | full | drive pins |

## Safety model

* No pin is powered until a person-approved [bench_glasgow_port_set](tools/bench_glasgow_port_set). The hardware voltage limit is set to the same value first.
* A pin tool takes the voltage from the server's record of that call, never from its own arguments, reads the device back first, and fails closed (ports off) if the device disagrees.
* After every pin tool the used ports are parked: all pins inputs, pulls released. The answer says `pins_state: parked`. The port rail stays at the set voltage until `bench_glasgow_safe_state`, until `max_on_s` (a watchdog), or until the server stops.
* One session at a time (the server's instrument lock plus a file lock in the install), bounded timeouts everywhere, captures bounded in time and size, no unbounded run.
* `bench_glasgow_safe_state` is the safe-state companion; if it cannot confirm 0 V it says to unplug the USB cable.

## What it cannot do

No transmit, no RF. No `flash`, `factory`, firmware or EEPROM change, no applet outside the list, no free-form command lines. Revision C's analyzer has no trigger and no sample-rate setting (48 MHz fixed). The pattern generator is timed from the host (about 1 ms), not by the FPGA. The I/O voltage range is 1.65 to 5.0 V (revision D 0.9 to 5.5 V): it cannot power a target. Glasgow's own sense-voltage mode (`A=SA`) is not offered.
