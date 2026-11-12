---
title: RTL-SDR setup
summary: Software, the kernel DVB driver blacklist and the configuration entry for the RTL-SDR.
see_also: [instruments/rtlsdr/overview, instruments/rtlsdr/troubleshooting, concepts/configuration]
instrument: rtlsdr
verified: hardware, 2026-11-11
---

# RTL-SDR setup

Configuration: `"rtl": {"driver": "rtlsdr", "tier": "read_config"}`. Optional: `serial` (hex digits; the dongle's is shown by `bench_usb_topology` and by `rtl_test`; many dongles share one factory serial, so with two of them set the EEPROM serials apart with `rtl_eeprom -s`), and `direct_samp` (`"1"` I branch or `"2"` Q branch, the HF direct sampling mode). Nothing is wired and no other setting is needed. The tier is `read_config` because tuning is a receiver setting; there is nothing to transmit.

Software from apt: `rtl-sdr` and `soapysdr-module-rtlsdr` (with `libsoapysdr0.8`). The kernel's DVB drivers (`dvb_usb_rtl28xxu`, `rtl2832`, `rtl2830`) grab the dongle first and must be kept from loading: `/etc/modprobe.d/rtl-sdr-blacklist.conf` containing `blacklist dvb_usb_rtl28xxu`, `blacklist rtl2832`, `blacklist rtl2830` (the `rtl-sdr` package ships this rule). Without it, opening fails with a "kernel driver is active" message. The user needs access to the USB device (the `rtl-sdr` package installs the udev rule).

Check from a shell: `SoapySDRUtil --find='driver=rtlsdr'`. Only one program can have the dongle open at a time.
