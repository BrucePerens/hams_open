---
title: VNWA setup
summary: Cabling, power through the hub, USB permissions, the configuration entry and the first checks.
see_also: [instruments/vna/overview, instruments/vna/operation, instruments/usb-hubs/setup, concepts/configuration]
tools: [bench_status, bench_vna_power_cycle]
instrument: vna
verified: code only, 2026-10-29
---

# VNWA setup

## Configuration entry

```json
"vna": {"driver": "vnwa_open", "tier": "read_config", "hub": "3-2", "hub_port": 2}
```

All keys besides `driver` and `tier` are optional: `hub` and `hub_port` (together; the power-switching hub port the unit is on: they enable [bench_vna_power_cycle](tools/bench_vna_power_cycle) and the automatic cold reset of a failing sweep), `retries` (re-measurements of a failing point, default 2), `if_hz` (12000, 6000 or 4000; a divisor of 48 kHz), `point_ms` (0.2 to 100), `rf_level` (0 to 16383), `codec_s11` and `codec_s21` (parts of the ALSA card ids; by default found from the USB tree, which stays right when a power cycle changes the kernel's card order), `settle_ms` and `window_ms`. A bad value is refused when the server starts.

## Power and USB

The unit is powered from USB only. For power cycling it must be on a hub that really switches port power (`uhubctl` compatible); the helper runs `sudo -n uhubctl`, so passwordless sudo for it must work (see the [USB hub setup](instruments/usb-hubs/setup)). The USB permission for the unit (`20a0:4118`) must allow the server's user to open it; `bench_status` reports `not found or no permission` otherwise.

## Cabling

Every cable is the person's. For a loopback check: TX to RX (through an attenuator if the level is high). For calibration the standards are plugged on the port; see [calibrate the VNA](procedures/calibrate-the-vna). Never connect an antenna or anything that can put power on TX or RX.

## First checks

1. `bench_status {"instrument": "vna"}`: `usb` should be `present`; both codecs found; `instrument_monitoring` shows the firmware.
2. `bench_vna_sweep {"start_hz": 10e6}` with TX to RX: a single point; S21 should be near 0 dB (the raw level of the cable and bridge, see [limits](instruments/vna/limits)).
3. While a sweep runs the codecs belong to the server: the vendor program cannot use them at the same time.
