# scope_mcp

An MCP server (built on the official `rmcp` SDK, stdio transport) that lets an AI agent drive a Siglent **SDS1000X-E** series
oscilloscope (written against an SDS1202X-E) over the LAN with SCPI on TCP port 5025. Status
(2026-10-25): identify, status, measurements, waveform capture (7 Mpts at 1 GSa/s) and settings were
checked on a real SDS1202X-E (firmware 1.3.27) with the probe on the calibrator (1.000 kHz, 50 %).

Licence: AGPL-3.0-or-later. Not published to crates.io.

## Why the LAN and not USB

The scope's USB descriptor declares its bulk endpoints with a 64-byte maximum packet size at high
speed (the kernel logs "bulk endpoint 0x81 has invalid maxpacket 64"), so only replies of 64 bytes
or less work: a waveform read stops after its first 52 bytes with the kernel `usbtmc` driver, a
hand-written libusb client and libsigrok alike. Ethernet has no such limit.

## Tools and permission tiers

There is no raw-SCPI tool. The server (not the model) refuses a tool above the tier set in
`SCOPE_MCP_TIER`: `read_only` (default), `read_config` or `full`.

| Tool | Tier | Does |
|---|---|---|
| `scope_identify` | read_only | `*IDN?` |
| `scope_status` | read_only | running/stopped, timebase, sample rate, per-channel settings |
| `scope_measure` | read_only | the scope's own parameter measurements |
| `scope_capture` | read_only | waveform to a little-endian f32 file plus a summary; refuses a stopped scope (its memory is an old frame) unless `allow_stopped` |
| `scope_screenshot` | read_only | `SCDP` image to a file |
| `scope_set` | read_config | timebase, volts/div, offset, coupling, probe attenuation, trigger mode |
| `scope_self_cal` | full | `*CAL?`; needs `confirm_inputs_disconnected: true`; the self-cal changed the vertical settings (1 V/div to 20 mV/div) |

## Run

    SCOPE_MCP_HOST=192.168.10.51 SCOPE_MCP_TIER=read_config cargo run --release

Other settings: `SCOPE_MCP_PORT` (5025), `SCOPE_MCP_OUT` (captures; default the temp dir).
SCPI on port 5025 has no authentication: anyone who can reach the scope can control it.

## Reply format

After the 1.3.27 firmware update the scope answers without the `C1:VDIV ` prefix; earlier it
included it. `Scope::value` takes the part after the last space or comma, so both parse.
Waveforms are `#9nnnnnnnnn` blocks; the screenshot has no header and is read until idle.

## Tests

`cargo test` runs the pure checks; `cargo test -- --ignored` needs a scope (see `tests/hil.rs`); it starts the real server binary, does the MCP initialize handshake and calls tools.
