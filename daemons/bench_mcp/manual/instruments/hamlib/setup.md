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
| `audio` | `{"instrument": "<name of an audio instrument in the bench config>", "input": "<alias or name of its sound input>"}`: the input that carries the rig's receive audio, by the names the sound tools use (`bench_audio_devices`, `bench_audio_discover`). Reserved devices are refused by the sound module |
| `transmit` | the owner's dummy-load attestation and limits; without it no tool transmits and the load tools do not exist. See below |
| `tier` | `read_only` lists the information tools; `read_config` adds control; `full` adds the tools that can transmit (and only with the transmit attestation) |

A bad value stops the server at start with a message naming the key.

## The audio input

The rig's receive audio reaches the computer through a sound card (the radio's USB audio, or a separate interface). Add an `audio` instrument to the bench config with an input alias for it (see [sound setup](instruments/sound/setup)), and name both in the rig's `audio` setting. `bench_audio_discover` finds a working input by listening; the 08bb:29c0 cards are reserved for the VNA and are never used.

## The transmit attestation

```json
"transmit": {"dummy_load": true, "attested_by": "K6BP", "attested_on": "2026-10-29",
             "cabling": "50 ohm dummy load on the rig's antenna port, no tuner",
             "max_power_w": 5, "max_seconds": 10,
             "frequency_ranges": [[7000000, 7100000]], "allowed_modes": ["CW", "USB"],
             "max_swr": 2.0, "swr_probe": true, "allow_lowering_power": true,
             "load_check": {"margin_db": 6, "max_spread_db": 6, "readings": 5, "references": 6}}
```

This block is the owner's statement that the rig's antenna port is on a dummy load, with their name, the date and what the cabling is, and the limits they accept. Every key but the optional ones is required, and there are no defaults that widen what may be done: `frequency_ranges` is the owner's list of the frequencies they may use for dummy-load tests and has no default. Compliance with the licence terms for any transmission, even into a dummy load, is the owner's responsibility. The server refuses to start with a malformed block. `max_seconds` is at most 60 and `max_power_w` at most 200 whatever the file says. The optional keys: `max_swr` (1 to 5, default 2: a higher SWR at the probe or during the interval stops the test); `swr_probe` (default true) and `swr_probe_s` (0.1 to 2 s, default 0.5): the low-power key that reads the SWR before the real interval; `allow_lowering_power` (default false): whether a tool may lower a power setting that is above the cap, restoring it afterwards; `operator_alert` (default false, needs the rig's `audio` setting): sound an alert through the audio instrument before keying, and send nothing if it cannot be heard; `load_check`: `margin_db`, `audio_margin_db`, `max_spread_db`, `readings`, `dwell_ms`, `references`, `af`, `preamp_db` of the [plausibility check](tools/bench_hamlib_load_check). The rig's own transmit time-out, if it has one, should be enabled as well. See [limits](instruments/hamlib/limits).
