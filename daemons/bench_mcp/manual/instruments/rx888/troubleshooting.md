---
title: RX-888 troubleshooting
summary: The escalation ladder for a boot loader that stops answering, radiod that does not answer, and multicast problems.
see_also: [instruments/rx888/setup, instruments/usb-hubs/operation]
instrument: rx888
verified: measured on the unit (RX-888 MkII on bruce16), 2026-11-05
---

# RX-888 troubleshooting

| Symptom | Cause | Next |
|---|---|---|
| `not on the USB bus`, and the kernel log shows `device descriptor read/64, error -110` or `not accepting address` | The FX3 boot loader hung. Seen once, after an `rx888_boot` run that failed (a relative firmware path) and reset the device. | Power-cycle its hub port: `bench_usb_port_power` off, wait 5 s, on. A logical port disable and a USB controller rebind do not clear it, and the rebind also takes Bluetooth down for a minute. Without a switchable port, unplug it for 5 s. |
| boot error `unable to open .../share/...` | `rx888_boot` adds its install directory to a relative firmware name | Use an absolute `firmware` path (the config refuses a relative one). |
| booted at 480 Mb/s | USB 2 port, hub or cable | Move it to a SuperSpeed path. Until then the full 64.8 Msps cannot be carried. |
| `radiod_start`: multicast is off | `lo` has no multicast flag | As root: `ip link set dev lo multicast on`. |
| `radiod exited at start` | Look at the log tail in the error: driver path, presets path, device busy (another `radiod` holds it) | Fix the path, or stop the other `radiod`. |
| `radiod does not answer` | `radiod` not running, or on other groups | `bench_rx888_status`; compare `status_group` with the `radiod` config. |
| First tune or spectrum slow | `radiod` plans its 1.62M-point FFT when the first channel starts (no FFTW wisdom file) | Wait; the tools allow 10 s. |
| `front_end_running: false` | No channel exists, so `radiod` idles the front end | Any tune, spectrum or capture starts it. |
| Spectrum with half the bins zero | Span wider than half the sample rate | The tool refuses such a span; for a wider view, raise the sample rate. |

`metadump` and `tune` from ka9q-radio without an interface option send on the default (LAN) interface. Give them `-i lo`, or use these tools, which always use loopback with TTL 0.
