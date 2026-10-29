---
title: Configuration and instrument aliases
summary: The configuration file, the environment variables, the driver names and what an alias is.
see_also: [concepts/safety-tiers, concepts/reserved-devices]
verified: code only, 2026-10-29
---

# Configuration and instrument aliases

The server reads a JSON file named by the environment variable `BENCH_MCP_CONFIG`. `BENCH_MCP_OUT` names the directory for captures, CSVs and screenshots (default: `bench_mcp` inside the system temp directory). The entries of `instruments` are the aliases; an alias is any name the owner chose and has no meaning to the tools except as the value of the `instrument` argument.

```json
{"instruments": {
  "scope": {"driver": "siglent_sds", "tier": "read_config", "host": "192.168.10.51", "port": 5025},
  "psu":   {"driver": "atm6301", "tier": "read_config", "serial": "/dev/ttyUSB0", "baud": 115200, "max_volts": 5.5, "max_amps": 1.0},
  "vna":   {"driver": "vnwa_open", "tier": "read_config", "hub": "3-2", "hub_port": 2},
  "blade": {"driver": "bladerf", "tier": "read_config", "xb200": "auto1db"},
  "lime":  {"driver": "limesdr", "tier": "read_config"}}}
```

That is the bench this manual was written against (aliases `scope`, `psu`, `vna`, `blade`, `lime`). Another site may use other aliases and fewer instruments. Use `bench_list` for the real ones.

| Driver | Kind | Manual |
|---|---|---|
| `siglent_sds` | oscilloscope over the LAN | [instruments/scope](instruments/scope) |
| `atm6301` | bench power supply on a USB-serial port | [instruments/psu](instruments/psu) |
| `vnwa_open` | DG8SAQ VNWA vector network analyser | [instruments/vna](instruments/vna) |
| `bladerf` | Nuand bladeRF with optional XB-200 | [instruments/bladerf](instruments/bladerf) |
| `limesdr` | LimeSDR-USB | [instruments/limesdr](instruments/limesdr) |
| `usb_hub` | the host's USB tree and switchable hub ports | [instruments/usb-hubs](instruments/usb-hubs) |
| `audio` | the host's sound devices | [instruments/sound](instruments/sound) |

Choosing the instrument: the `instrument` argument of a tool is optional when exactly one configured instrument can serve the tool (the only scope, the only supply). With two or more candidates the call fails with `name the instrument: a, b`. A name that does not exist fails with `no instrument named "x"`; a name of the wrong kind with `<tool> does not apply to "x"`.

A bad value in the file is refused when the server starts (the whole file, with the entry's name), so a running server has a valid configuration. Per-driver settings are on each instrument's `setup` page. The configuration cannot be changed through the tools.
