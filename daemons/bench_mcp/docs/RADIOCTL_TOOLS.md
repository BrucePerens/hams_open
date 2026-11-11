# Radioctl tools (bench_mcp, `radioctl` driver)

The `bench_radioctl_*` tools drive radios that radioctl supports. They live in `src/radioctl/` and are part of the bench_mcp server, so
one MCP server covers the bench: the sound tools (`bench_audio_*`) hear a receiver's audio, the USB hub tools find and power-cycle a
rig's USB port, and these tools control the rig.

## How it reaches a rig

It starts the `radioctld` daemon of `daemons/radioctl` (a separate program, found on the search path or at `BENCH_MCP_RADIOCTLD`) for the rig,
bound to 127.0.0.1, with a configuration it writes into the managed directory, or connects to one that already runs, and speaks the daemon's
text protocol (the `rigctld` line protocol) over a socket. The list of rigs and a rig's capabilities come from radioctl's rig description
files (`rigs/*.toml`, in `rigs_dir` or `RADIOCTL_RIGS`). The old radioctl `rigctld` is not used any more.

## Registering

```json
{"instruments": {
  "rig":   {"driver": "radioctl", "tier": "read_only", "rig": "icom-ic-7300",
            "device": "/dev/serial/by-id/usb-EXAMPLE_rig-if00-port0", "speed": 19200},
  "virtual": {"driver": "radioctl", "tier": "read_config", "rig": "elecraft-kx3"}
}}
```

One entry is one rig. The tier decides what is listed: `read_only` the information tools, `read_config` adds control, `full` the tools
that can transmit (which also need the owner's dummy-load attestation in the entry). Keys: `rig` (a radioctl rig description id; with `device` and `speed` a serial rig, without them a virtual rig) or `radioctld_port` (with
`radioctld_host`, loopback unless `allow_remote_radioctld`), `rigs_dir`, `full_power_w` (the rig's full-scale RF watts; required with `transmit` on a managed rig: the tools scale the power setting by it), `listen_port`, `reply_timeout_s`,
`rx_ranges_hz`, `managed_dir`. Every key is described in the manual page `instruments/radioctl/setup`.

## Tools

| Tool | Tier |
|---|---|
| `bench_radioctl_models` | read_only |
| `bench_radioctl_rigs` | read_only |
| `bench_radioctl_status` | read_only |
| `bench_radioctl_get` | read_only |
| `bench_radioctl_watch` | read_only |
| `bench_radioctl_diagnose` | read_only |
| `bench_radioctl_set` | read_config |
| `bench_radioctl_memory` | read_config |
| `bench_radioctl_scan` | read_config |
| `bench_radioctl_stations` | read_only |
| `bench_radioctl_station_set` | read_config |
| `bench_radioctl_rx_check` | read_config (needs the rig's `audio` setting for the audio part) |
| `bench_radioctl_load_check` | read_config (only with a `transmit` attestation) |
| `bench_radioctl_load_baseline` | full (only with a `transmit` attestation) |
| `bench_radioctl_safe_state` | read_config (every radioctl rig) |
| `bench_radioctl_tx_key_test` | full (only with a `transmit` attestation) |
| `bench_radioctl_tx_power_sweep` | full (only with a `transmit` attestation) |
| `bench_radioctl_daemon` | read_config |

## Safety

Every radioctld command has a timeout; reads are tried twice, writes never repeated blind; no argument can carry a second command;
the daemon listens on localhost only and is stopped when the server stops; a daemon left by a killed server is stopped at the next
start. Nothing here has run on a real rig: see `instruments/radioctl/limits`.

## Transmitting

A rig can transmit through these tools only if its entry has the `transmit` block: the owner's statement that the rig's antenna port is on a dummy load (name, date, cabling), `max_power_w`, `max_seconds`, `frequency_ranges` (the owner's list; no default), `allowed_modes`, and optional `max_swr`, `swr_probe`, `allow_lowering_power`, `operator_alert` and `load_check` settings. **The software cannot know what is connected.** The attestation is the control; the load plausibility check (receive-only, against a baseline recorded with the load) and the low-power SWR probe are evidence, not proof. A separate watchdog process (`bench_mcp --radioctl-guard`) un-keys the rig and sets the lowest power on a deadline, a dead server or an abort. Every keyed interval is logged. Licence compliance for any transmission is the owner's responsibility. Details: manual pages `instruments/radioctl/limits` and `procedures/test-a-transmitter-into-a-dummy-load`.

Left out: radioctl memory channels (radioctld answers `RPRT -4` for them), radioctld's own event channel (the watch polls), an audio-through-the-rig transmit check, automatic generator sweeps and an S-meter calibration table (the recipes are in `instruments/radioctl/measurement-recipes`).
