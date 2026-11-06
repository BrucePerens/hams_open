---
title: RX-888 overview
summary: What the RX-888 MkII is, how bench_mcp reaches it (rx888_boot, radiod, radiod_client), and which tools do what.
see_also: [instruments/rx888/setup, instruments/rx888/operation, instruments/rx888/protocol]
instrument: rx888
verified: measured on the unit (RX-888 MkII, hardware 0x04, firmware 2.3, no antenna), 2026-11-05
---

# RX-888 overview

The RX-888 MkII is a USB 3 receiver built on a Cypress FX3 controller. A 16-bit ADC samples the antenna input directly, so one stream covers 0 to about 30 MHz at the half rate of 64.8 Msps. On the USB bus it appears as `04b4:00f3` (the FX3 boot loader, before firmware) and as `04b4:00f1` "RX888mk2" once its firmware is in RAM. The firmware is lost at every power-off.

bench_mcp does not drive the USB device itself. It uses ka9q-radio, run unmodified as separate programs:

- `rx888_boot` loads the firmware image (`W1EUJ_0.1.0_FX3.img` from ka9q-radio's `share/`) into the FX3's RAM;
- `radiod` runs the receiver, loads ka9q-radio's `rx888.so` driver, and serves channels over loopback multicast with TTL 0;
- the tools talk to `radiod` through the `radiod_client` crate (hams_open `daemons/radiod_client`). That crate has a closed command schema: every channel command has a lifetime, and nothing can set a destination or touch the front end.

| Tool | Tier | Use |
|---|---|---|
| `bench_rx888_status` | read_only | Is it on the bus, booted, at USB 3 speed; is our `radiod` running; does the front end answer |
| `bench_rx888_frontend` | read_only | Sample rate, A/D overrange count, RF gain, attenuation, AGC, IF power |
| `bench_rx888_boot` | read_config | Load the firmware (the device re-enumerates) |
| `bench_rx888_radiod_start`, `bench_rx888_radiod_stop` | read_config | Run or stop `radiod` for it |
| `bench_rx888_tune` | read_config | One channel: level, noise density, SNR, measured input rate |
| `bench_rx888_spectrum` | read_config | Bins over a span, CSV plus floor and peaks |
| `bench_rx888_capture` | read_config | Seconds of I/Q or demodulated audio to a WAV |

Receive only: no tool can transmit, so none needs the `full` tier, a `dry_run` or a safe-state companion. The relay uses the same `radiod` as a receive-only virtual rig through radioctl (family `radiod-channel`).
