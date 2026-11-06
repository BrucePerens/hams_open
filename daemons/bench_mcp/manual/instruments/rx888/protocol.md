---
title: RX-888 protocol notes
summary: How the tools reach the receiver: rx888_boot, radiod's TLV control protocol on loopback multicast, RTP, and the clean-room boundary.
see_also: [instruments/rx888/overview, instruments/rx888/limits]
instrument: rx888
verified: measured on the unit (radiod commit 8c50dbbe), 2026-11-05
---

# RX-888 protocol notes

- **Boot.** `rx888_boot <absolute image path>` uploads the FX3 image to the boot loader (`04b4:00f3`). The device then leaves the bus and comes back as `04b4:00f1`. The tool reads `idVendor`, `idProduct` and `speed` from `/sys/bus/usb/devices`.
- **radiod.** The config written by `bench_rx888_radiod_start` has `hardware = rx888`, `iface = lo`, `ttl = 0` and `advertise = no`, and loads the driver with `library =`. `radiod` runs under `nice -n 10` as the server's user. It cannot get real-time priority as an ordinary user, and runs without it.
- **Control.** `radiod`'s status/command protocol: UDP to the status group on port 5006. A packet is a type byte (0 status, 1 command) followed by TLV entries. The type numbers come from ka9q-radio's `status.h` (`radiod_client::tlv::ty`). `radiod` acknowledges a command only by echoing its tag in a status. Observed on the RX-888:
  - the front end (SSRC 0) answers only when polled, and does not echo the tag;
  - a channel's levels have the RF gain removed;
  - the spectrum is computed once per command.
- **Lifetimes.** A channel created without a lifetime never expires. Every command the tools send carries one; the front-end poll is not a channel command and carries none.
- **RTP.** Each channel streams 50 packets per second of 16-bit big-endian PCM, with a dynamic payload type taken from the channel's status (122 for 12 kHz mono, 123 for 12 kHz I/Q on this `radiod`).
- **Clean room.** ka9q-radio is GPL-3.0 and is only run, never linked. Only its documentation, configs, `status.h` and the files listed in hams_com `docs/reviews/RADIOD_TRIAL_2026-10-30.md` were read. The RX-888 driver source (`rx888.c` and the files it uses) was not read, so that a later Rust driver can be written without it.
