---
title: Glasgow setup
summary: The pinned install, the udev rule, the config entry, first-use build times and disk use.
see_also: [instruments/glasgow/overview, instruments/glasgow/troubleshooting, concepts/extending/standing-rules]
instrument: glasgow
verified: measured on the unit (Glasgow revC3, device firmware API level 3, glasgow software 70cb38c9), 2026-11-08
---

# Glasgow setup

## 1. The pinned install (no sudo, nothing outside its directory)

```
nice -n 10 python3 -I daemons/bench_mcp/glasgow/install_glasgow.py --dest ~/.local/share/bench_mcp/glasgow-70cb38c9
```

It needs Python 3.13 or newer and network access to pypi.org and codeload.github.com. It creates a venv, installs `requirements.txt` with `pip install --require-hashes --only-binary=:all:` (24 packages at exact versions, every file's sha256 listed), downloads Glasgow's source archive for commit `70cb38c9b03471acd2eabb76c82308891bec2ca5`, refuses it unless its sha256 is `d20f1910669c1a011ca020c4cf4eaa51135ea5abe7414c9f3b7c5cb6ba708139` (51.8 MB; licence 0BSD OR Apache-2.0), and installs it without dependencies. Measured: 8 s with the archive on disk (`--archive FILE`), 552 MB on disk, of which the gateware toolchain (YoWASP yosys and nextpnr for the iCE40) is about 350 MB. The ECP5 toolchain, needed only by revision D, is left out (166 MB). It never touches another venv or the system Python.

Gateware is **not** shipped prebuilt. Each applet with its pins is built on first use (measured on a 32-core box: 4 to 7 s per applet at low load) and cached in `<install>/cache`. [bench_glasgow_prepare](tools/bench_glasgow_prepare) builds ahead of time.

## 2. Permission to open the USB device

Install the rule (the one command; the file is in this repository and names exactly what it does):

```
sudo install -m 644 daemons/bench_mcp/glasgow/70-glasgow-bench.rules /etc/udev/rules.d/70-glasgow-bench.rules && sudo udevadm control --reload && sudo udevadm trigger --subsystem-match=usb --attr-match=idVendor=20b7
```

The Glasgow re-enumerates after every firmware load, so a hand-set ACL on its device node is lost each time; the rule gives group `plugdev` and the console user access to every future node. Remove it by deleting the file.

## 3. The config entry

```
"glasgow": {"driver": "glasgow", "tier": "full", "serial": "<serial from bench_glasgow_list>",
            "max_volts": 3.6, "max_on_s": 300, "max_trip_a": 0.1,
            "max_capture_s": 30, "max_capture_mb": 64, "op_timeout_s": 120, "build_timeout_s": 600,
            "venv": "/home/USER/.local/share/bench_mcp/glasgow-70cb38c9"}
```

Everything but `driver` is optional. `serial` (the device's own serial, see `bench_glasgow_list`) is required when more than one Glasgow is attached. `max_volts` is the highest port voltage any tool may set (default 5.5; the revision's own range still applies). `max_on_s` is how long a port may stay powered after it was set (default 600). `max_trip_a` caps the supply current trip (0.01 to 0.325). Use `tier: read_config` to forbid driving pins altogether: then only the read and passive tools exist (and no port can be powered, so the passive tools cannot run).

## Reserved device

Declare the Glasgow's USB port in `reserved_serial` (`usb:<bus-port>`) if other tools might otherwise touch it. The hub tools refuse devices marked critical; mark the Glasgow's hub port so nobody power-cycles it mid-capture.
