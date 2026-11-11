---
title: radioctl setup
summary: Building radioctld, finding the rig's description id and serial device, and the config entry with every key.
see_also: [instruments/radioctl/overview, instruments/radioctl/limits, concepts/configuration, tools/bench_usb_find]
instrument: radioctl
verified: virtual radioctl rig only, 2026-11-11
---

# radioctl setup

1. Build `radioctld` (`cargo build --release -p radioctld` in `daemons/radioctl`) and put it on the search path, or set `BENCH_MCP_RADIOCTLD` to its path. Point `RADIOCTL_RIGS` (or `rigs_dir` in the entry) at the `rigs/` directory with the description files.
2. Find the rig description id: `bench_radioctl_models {"search": "<make or model>"}`, then `{"rig": "<id>"}` to see what it declares.
3. Find the rig's serial device. For a USB rig use [bench_usb_find](tools/bench_usb_find) to see which USB device it is, and use the stable `/dev/serial/by-id/...` path, not `/dev/ttyUSB0`, which can move. The user running bench_mcp must be able to read and write the device (on Debian, membership of the `dialout` group); bench_mcp never changes permissions.
4. Add an instrument entry to the bench config:

```json
{"instruments": {
  "rig": {"driver": "radioctl", "tier": "read_only", "rig": "icom-ic-7300",
          "device": "/dev/serial/by-id/usb-EXAMPLE_rig-if00-port0", "speed": 19200,
          "rigs_dir": "/opt/radioctl/rigs", "reply_timeout_s": 3,
          "rx_ranges_hz": [[100000, 30000000]]},
  "virtual": {"driver": "radioctl", "tier": "read_config", "rig": "elecraft-kx3"}
}}
```

## Keys

| Key | Meaning |
|---|---|
| `rig` | the id of a radioctl rig description (the file name without `.toml`); this server starts a radioctld for it. Without `device` the rig is a virtual one made from the description, with no hardware |
| `device` | serial device under `/dev` (with `speed`) |
| `speed` | serial baud rate: 1200 to 921600; required with `device` |
| `full_power_w` | the rig's full-scale RF output in watts, from its manual (a KX3 is 15, an IC-7300 100). Required with a `transmit` attestation on a managed rig: radioctld has no power conversion, so the tools turn the owner's `max_power_w` and a requested power into a power setting as a fraction of this number, and the managed daemon is given it as its `tx_power_mw`. For a radioctld that already runs, set `tx_power_mw` in its own configuration (its default, 100 W, is wrong for most rigs; the cap is not enforced right with a wrong full scale) |
| `rigs_dir` | absolute path of the directory with the rig description files; default: the `RADIOCTL_RIGS` environment variable |
| `model`, `set_conf` | removed (they were Hamlib settings); they give an error that names `rig` |
| `listen_port` | fixed localhost port for the managed daemon (1024 and up); default: a free port |
| `radioctld_port`, `radioctld_host` | use a radioctld that already runs, instead of `rig`. The host must be this machine unless `allow_remote_radioctld` is true; radioctld has no authentication |
| `reply_timeout_s` | how long one command may take, 1 to 30, default 3 |
| `rx_ranges_hz` | list of `[low, high]`: the owner's limits on receive frequencies, in addition to what the rig reports |
| `managed_dir` | absolute path for state files and logs; default: a `radioctl` directory under `BENCH_MCP_OUT` |
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

This block is the owner's statement that the rig's antenna port is on a dummy load, with their name, the date and what the cabling is, and the limits they accept. Every key but the optional ones is required, and there are no defaults that widen what may be done: `frequency_ranges` is the owner's list of the frequencies they may use for dummy-load tests and has no default. Compliance with the licence terms for any transmission, even into a dummy load, is the owner's responsibility. The server refuses to start with a malformed block. `max_seconds` is at most 60 and `max_power_w` at most 200 whatever the file says. The optional keys: `max_swr` (1 to 5, default 2: a higher SWR at the probe or during the interval stops the test); `swr_probe` (default true) and `swr_probe_s` (0.1 to 2 s, default 0.5): the low-power key that reads the SWR before the real interval; `allow_lowering_power` (default false): whether a tool may lower a power setting that is above the cap, restoring it afterwards; `operator_alert` (default false, needs the rig's `audio` setting): sound an alert through the audio instrument before keying, and send nothing if it cannot be heard; `load_check`: `margin_db`, `audio_margin_db`, `max_spread_db`, `readings`, `dwell_ms`, `references`, `af`, `preamp_db` of the [plausibility check](tools/bench_radioctl_load_check). The rig's own transmit time-out, if it has one, should be enabled as well. See [limits](instruments/radioctl/limits).
