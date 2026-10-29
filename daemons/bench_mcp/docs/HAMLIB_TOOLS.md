# Hamlib tools (bench_mcp, `hamlib` driver)

The `bench_hamlib_*` tools drive radios supported by Hamlib. They live in `src/hamlib/` and are part of the bench_mcp server, so
one MCP server covers the bench: the sound tools (`bench_audio_*`) hear a receiver's audio, the USB hub tools find and power-cycle a
rig's USB port, and these tools control the rig.

## How it reaches a rig

Hamlib is LGPL. This module does not link it or copy its code. It starts Hamlib's `rigctld` daemon (a separate program) for the rig,
bound to 127.0.0.1, or connects to one that already runs, and speaks the daemon's documented text protocol over a socket. The list of
models and a model's capabilities come from running `rigctl -l` and `rigctl -m N -u`.

## Registering

```json
{"instruments": {
  "rig":   {"driver": "hamlib", "tier": "read_only", "model": 1021,
            "device": "/dev/serial/by-id/usb-EXAMPLE_rig-if00-port0", "speed": 19200},
  "dummy": {"driver": "hamlib", "tier": "read_config", "model": 1}
}}
```

One entry is one rig. The tier decides what is listed: `read_only` the information tools, `read_config` adds control, `full` the tools
that can transmit (which also need the owner's dummy-load attestation in the entry). Keys: `model` or `rigctld_port` (with
`rigctld_host`, loopback unless `allow_remote_rigctld`), `device`, `speed`, `set_conf`, `listen_port`, `reply_timeout_s`,
`rx_ranges_hz`, `managed_dir`. Every key is described in the manual page `instruments/hamlib/setup`.

## Tools

| Tool | Tier |
|---|---|
| `bench_hamlib_models` | read_only |
| `bench_hamlib_rigs` | read_only |
| `bench_hamlib_status` | read_only |
| `bench_hamlib_get` | read_only |
| `bench_hamlib_rigctld` | read_config |

## Safety

Every rigctld command has a timeout; reads are tried twice, writes never repeated blind; no argument can carry a second command;
the daemon listens on localhost only and is stopped when the server stops; a daemon left by a killed server is stopped at the next
start. Nothing here has run on a real rig: see `instruments/hamlib/limits`.
