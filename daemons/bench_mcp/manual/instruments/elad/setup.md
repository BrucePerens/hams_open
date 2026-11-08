---
title: ELAD FDM receivers: setup
summary: Permissions, the FPGA image of the FDM-S1/S2, and the config entry.
see_also: [instruments/elad/overview, instruments/elad/limits, instruments/elad/troubleshooting]
instrument: elad
verified: measured on the unit (FDM-S2, USB firmware 3.0), 2026-11-08
---

# ELAD FDM receivers: setup

1. **Permission.** libusb needs read and write access to the device node. Install `daemons/elad_usb/udev/70-elad-fdm.rules` (`sudo cp` it to `/etc/udev/rules.d/`, then `sudo udevadm control --reload && sudo udevadm trigger`), or grant a temporary ACL on the one node: `sudo setfacl -m u:$USER:rw /dev/bus/usb/BBB/DDD` (the numbers are in `bench_elad_list`; it resets on replug).
2. **The FPGA (FDM-S1/S2 only).** These units enumerate and answer `bench_elad_identify` with no FPGA image, but they stream nothing until ELAD's own loader has programmed one for the sample rate in use (re-done after every power-up and rate change). This server does not contain, download or run that loader. If you have loaded it yourself, say so with `"fpga_loaded": true`; the tools refuse to stream an S1/S2 otherwise, with the reason. An FDM-DUO needs nothing.
3. **Antenna.** Leave the input as it is. The tools never connect or switch an antenna, and the receiver cannot transmit.
4. **Config entry** (`capture_dir` must exist and be absolute; `max_capture_s` is 0.1 to 30, default 5):

`{"driver": "elad", "tier": "read_config", "capture_dir": "/var/tmp/bench_elad", "fpga_loaded": false, "max_capture_s": 5}`
