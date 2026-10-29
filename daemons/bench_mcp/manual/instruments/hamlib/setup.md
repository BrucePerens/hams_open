---
title: Hamlib setup
summary: Installing the Hamlib utilities, finding the rig's model number and serial device, and the config entry with every key.
see_also: [instruments/hamlib/overview, instruments/hamlib/limits, concepts/configuration, tools/bench_usb_find]
instrument: hamlib
verified: dummy rig only, 2026-10-29
---

# Hamlib setup

1. Install the Hamlib utilities (Debian: `apt install libhamlib-utils`). The tools need `rigctld`; `bench_hamlib_models` also needs `rigctl`.
2. Find the model number: `bench_hamlib_models {"search": "<make or model>"}`, then `{"model": N}` to see what it offers.
3. Find the rig's serial device. For a USB rig use [bench_usb_find](tools/bench_usb_find) to see which USB device it is, and use the stable `/dev/serial/by-id/...` path, not `/dev/ttyUSB0`, which can move. The user running bench_mcp must be able to read and write the device (on Debian, membership of the `dialout` group); bench_mcp never changes permissions.
4. Add an instrument entry to the bench config:

```json
{"instruments": {
  "rig": {"driver": "hamlib", "tier": "read_only", "model": 1021,
          "device": "/dev/serial/by-id/usb-EXAMPLE_rig-if00-port0", "speed": 19200,
          "set_conf": {"serial_handshake": "None"}, "reply_timeout_s": 3,
          "rx_ranges_hz": [[100000, 30000000]]},
  "dummy": {"driver": "hamlib", "tier": "read_config", "model": 1}
}}
```

## Keys

| Key | Meaning |
|---|---|
| `model` | Hamlib model number; this server starts a rigctld for it. Model 1 is the dummy rig and needs no device |
| `device` | serial device under `/dev`; for model 2 (NET rigctl) `127.0.0.1:<port>` |
| `speed` | serial baud rate: 1200 to 921600 |
| `set_conf` | Hamlib config keys and values, passed as `--set-conf` (letters, digits and `_ . / : -` only) |
| `listen_port` | fixed localhost port for the managed daemon (1024 and up); default: a free port |
| `rigctld_port`, `rigctld_host` | use a rigctld that already runs, instead of `model`. The host must be this machine unless `allow_remote_rigctld` is true; rigctld has no authentication |
| `reply_timeout_s` | how long one command may take, 1 to 30, default 3 |
| `rx_ranges_hz` | list of `[low, high]`: the owner's limits on receive frequencies, in addition to what the rig reports |
| `managed_dir` | absolute path for state files and logs; default: a `hamlib` directory under `BENCH_MCP_OUT` |
| `tier` | `read_only` lists the information tools; `read_config` adds control; `full` adds the tools that can transmit (and only with the transmit attestation) |

A bad value stops the server at start with a message naming the key.
