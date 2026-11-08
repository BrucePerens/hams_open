---
title: Glasgow protocol notes
summary: The glasgow commands used and where each fact came from; nothing is copied from the project.
see_also: [instruments/glasgow/overview, concepts/extending/provenance]
instrument: glasgow
verified: fixtures only
---

# Glasgow protocol notes

The server speaks to the device only through the `glasgow` command line of the pinned install. Used: `list`, `voltage [PORT VOLTS AMPS]`, `voltage-limit PORT VOLTS`, `safe`, `build --rev REV -t bitstream -f FILE APPLET ...`, `run APPLET ...` (spi-controller, analyzer, uart-analyzer, jtag-probe, control-gpio), and `script HELPER APPLET ... -- OP JSON` (i2c-controller, uart, control-gpio, swd-probe) with the helper `src/glasgow/ops.py`, which prints one `BENCH_GLASGOW_RESULT {json}` line. Never used: `flash`, `factory`, `test`, `tool`, `repl`, `multi`.

| Fact | Source kind | Source |
|---|---|---|
| table printed by `glasgow voltage` and `voltage-limit`, the `list` output | observation of the device's output | recorded 2026-11-08 from a revC3 (`tests/glasgow_fixtures/`), serial replaced |
| `voltage` leaves the rail on after the process exits; `safe` turns every port off | other public documentation | the pinned source's `cli.py` and `hardware/assembly.py` (0BSD OR Apache-2.0), read; and the `voltage` readback after a run |
| applet options, log line formats | other public documentation | `glasgow <command> --help` of the pinned install and its source |
| helper script context (`i2c_iface`, `uart_iface`, `gpio_iface`, `swd_iface`) | other public documentation | the pinned source's `applet/__init__.py` |
| link speed | measurement | sysfs `speed` and `glasgow run benchmark` on the unit |

Nothing was copied from the Glasgow project into this repository except what is a command-line option name; the software is installed from its pinned release archive, not vendored. The log-line fixtures for JTAG are the formats in its source, not recordings.
