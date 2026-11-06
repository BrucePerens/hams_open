---
title: RX-888 setup
summary: Building ka9q-radio's rx888 driver, the udev rule, loopback multicast, the bench config entry, and the first boot.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/troubleshooting]
instrument: rx888
verified: measured on the unit (RX-888 MkII on bruce16, ka9q-radio commit 8c50dbbe), 2026-11-05
---

# RX-888 setup

1. **ka9q-radio.** Build `radiod`, the driver and the loader: `make -C src ENABLE_RX888=1 radiod rx888.so rx888_boot` (needs `libusb-1.0-0-dev` as well as radiod's own `libfftw3-dev libiniparser-dev libbsd-dev libopus-dev`). `make install` puts them in `/usr/local/sbin` and `/usr/local/lib/ka9q-radio`, and the firmware and `presets.conf` in `/usr/local/share/ka9q-radio`. A build tree works too: give its absolute paths in the config.
2. **USB access.** Give your user the device in both of its identities. As root, write `/etc/udev/rules.d/63-rx888.rules` with:

   ```
   SUBSYSTEM=="usb", ATTR{idVendor}=="04b4", ATTR{idProduct}=="00f3", MODE="0660", GROUP="plugdev", TAG+="uaccess"
   SUBSYSTEM=="usb", ATTR{idVendor}=="04b4", ATTR{idProduct}=="00f1", MODE="0660", GROUP="plugdev", TAG+="uaccess"
   ```

   then `udevadm control --reload`. ka9q-radio's own rule (`70-rx888-boot.rules`) also starts its boot service automatically. That rule is not needed here, because `bench_rx888_boot` does the boot.
3. **USB 3.** Use a SuperSpeed port or hub. The boot loader always enumerates at 480 Mb/s; the booted device should come up at 5000 Mb/s. `bench_rx888_status` shows the speed.
4. **Loopback multicast.** `radiod` runs on `lo` only. Multicast must be on: as root, `ip link set dev lo multicast on` (until reboot). `bench_rx888_status` reports it, and `bench_rx888_radiod_start` refuses without it.
5. **Config entry** (`BENCH_MCP_CONFIG`). All paths must be absolute:

   ```json
   "rx888": {"driver": "rx888", "tier": "read_config",
             "radiod": "/usr/local/sbin/radiod",
             "driver_library": "/usr/local/lib/ka9q-radio/rx888.so",
             "boot": "/usr/local/sbin/rx888_boot",
             "firmware": "/usr/local/share/ka9q-radio/W1EUJ_0.1.0_FX3.img",
             "presets": "/usr/local/share/ka9q-radio/presets.conf",
             "status_group": "239.88.8.1", "data_group": "239.88.8.2"}
   ```

   Optional: `status_port` (5006) and `data_port` (5004). Also `ssrc_base` (88000): the tools use SSRCs `ssrc_base` to `ssrc_base + 2`, so keep them clear of other clients of the same `radiod`.
6. **Power switching.** Put it on a per-port switchable hub and label the port in the `usb_hub` entry (for example `"3-2/2": {"label": "RX-888"}`). A hung boot loader is then reset with `bench_usb_port_power`: off for 5 s, then on.
7. **First run.** `bench_rx888_status`, `bench_rx888_boot`, `bench_rx888_radiod_start`, then `bench_rx888_frontend`.
