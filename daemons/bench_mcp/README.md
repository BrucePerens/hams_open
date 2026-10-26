# bench_mcp

One MCP server (official `rmcp` SDK, stdio) for the bench test equipment. Drivers: a Siglent
**SDS1000X-E** series oscilloscope over the LAN (written against an SDS1202X-E, firmware 1.3.27),
an **Abestop ATM6301** bench supply (0-60 V, 0-10 A) on its CH340 USB-serial port, and a **DG8SAQ VNWA 3E**
vector network analyser (USB and its two audio codecs, through the `vnwa_open` crate, no vendor software). Status
(2026-10-26): all three checked on the real instruments. Adding an instrument means one driver file and
a `Kind`.

Licence: AGPL-3.0-or-later. Not published to crates.io.

## Configuration

`BENCH_MCP_CONFIG` names a JSON file (see `src/config.rs`):

    {"instruments": {
      "scope": {"driver": "siglent_sds", "tier": "read_config", "host": "192.168.10.51", "port": 5025},
      "psu":   {"driver": "atm6301", "tier": "read_config", "serial": "/dev/ttyUSB0", "baud": 115200,
                "max_volts": 5.5, "max_amps": 1.0},
      "vna":   {"driver": "vnwa_open", "tier": "read_config", "if_hz": 12000, "point_ms": 60, "rf_level": 16383}}}

The VNA entry's settings are all optional: `codec_s11` and `codec_s21` (parts of the ALSA ids of the two codecs, default
`hw:CARD=CODEC,` and `hw:CARD=CODEC_1,`), `if_hz` (a divisor of 48 kHz: 12000, 6000 or 4000), `point_ms` (0.2 to 100),
`rf_level` (0 to 16383), `settle_ms` and `window_ms`. A bad value is refused when the file is read.

`BENCH_MCP_OUT` is the directory for captures and screenshots (default: the temp dir). Each
instrument has its own permission tier (`read_only` when omitted). There is no raw-SCPI tool. The
server, not the model, refuses a tool above an instrument's tier and does not list a tool that no
instrument may use.

## Tools

Few generic tools take an `instrument` name (optional when only one instrument fits).

| Tool | Tier | Applies to |
|---|---|---|
| `bench_list`, `bench_identify`, `bench_status`, `bench_measure` | read_only | both |
| `bench_capture`, `bench_screenshot` | read_only | scope |
| `bench_set` | read_config | both (supply setpoints are capped by `max_volts`/`max_amps`) |
| `bench_psu_output` | read_config to switch off, **full** to switch on | supply |
| `bench_scope_self_cal` | full (needs `confirm_inputs_disconnected: true`) | scope |
| `bench_vna_sweep` | read_config (it puts RF on the TX port) | VNA |

`bench_identify` and `bench_status` also answer for the VNA (USB present, both codecs found, IF, time per point, level).
`bench_vna_sweep` takes `start_hz`, optionally `stop_hz`, `points` (1 to 401, about 0.2 s each), `if_hz`, `point_ms`
and `rf_level`, measures S21 (RX channel) and S11 (reflection channel), saves a CSV into `BENCH_MCP_OUT` and returns a
summary (and the rows when there are at most 51). The values are **raw ratios of the unit's own channels, not calibrated**:
the cable, bridge and front-end are not removed, so a cable between TX and RX is not 0 dB (about -1 dB at 10 MHz rising to
+2.6 dB at 300 MHz, with a phase that is linear in frequency). While a sweep runs the codecs belong to the server: the
vendor program cannot use them at the same time.

`bench_capture` refuses a stopped scope (its memory is an old frozen frame) unless `allow_stopped`.
The supply's output is switched on only if the setpoints stored in it are within the caps, since a
front-panel value may be left there (it came set to 12 V / 7.8 A).

## Why the LAN for the scope

The scope's USB descriptor declares its bulk endpoints with a 64-byte maximum packet size at high
speed (the kernel logs "bulk endpoint 0x81 has invalid maxpacket 64"), so only replies of 64 bytes or
less work over USB; a waveform read stops after its first 52 bytes with the kernel `usbtmc` driver,
a hand-written libusb client and libsigrok alike. Ethernet has no such limit.

## Notes on the instruments

* Scope: after the 1.3.27 firmware update replies come without the `C1:VDIV ` prefix; `Link::value`
  takes the part after the last space or comma so both parse. A self-cal reset the vertical settings
  (1 V/div to 20 mV/div). `*CAL?` answers `0` at once and the calibration keeps running for a while.
* Supply: SCPI at 115200 8N1 (no answer at other baud rates). It answers `OUTP?` with `OFF`/`ON`
  (the manual says 0/1). The serial port is locked with `flock` so two server processes cannot talk
  to it at once.
* No SCPI manual exists for the scope here; the supply's is `ATM SCPI programming manual_v1.0.0`.

## Checks this crate makes for the VNA (anchors)

* The arguments of a sweep are checked and override the instrument's settings for that sweep. [@ANCHOR: bench_mcp_vna_sweep_args]
* A sweep is written as a CSV with a header and one row per point. [@ANCHOR: bench_mcp_vna_csv]

## Tests

`cargo test` runs the pure checks. `BENCH_MCP_CONFIG=<file> cargo test -- --ignored` needs the real
instruments: it starts the server binary, does the MCP handshake and calls tools; it never switches
the supply output on.

## Checks this crate makes (anchors)

* The supply's limits refuse out-of-range and non-finite values before any command is sent. [@ANCHOR: bench_mcp_psu_caps]
* An instrument is picked by name, or implicitly when exactly one configured instrument fits the tool. [@ANCHOR: bench_mcp_pick]
* The configuration file parses both drivers (scope over TCP, supply over serial) with their defaults. [@ANCHOR: bench_mcp_config]
* The scope waveform analysis returns the edges and the frequency of a square wave. [@ANCHOR: scope_mcp_analyse]
