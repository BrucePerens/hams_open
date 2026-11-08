# bench_mcp

One MCP server (official `rmcp` SDK, stdio) for the bench: about 100 tools, each with a manual page served through the MCP itself.
It drives a Siglent **oscilloscope** (SDS1000X-E, over the LAN), a Siglent **spectrum analyzer** with tracking generator (SSA3000X), a bench **power supply** (Abestop ATM6301), a **vector network
analyser** (DG8SAQ VNWA 3E, no vendor software), **SDRs** (bladeRF, LimeSDR), **USB hubs** (topology, diagnosis, and port power
cycling to reset a device), **sound devices** (play, record, meter, loopback, speech, with discovery by signature), **radios**
through Hamlib's `rigctld`, **antenna rotators**, and a **procedure runner**: long multi-step **Python programs** (CPython 3 with numpy and scipy) that run next to the server and call any
bench tool through the normal tier and safety checks, with structured failure reports and safe-state cleanup, so a soak or a
calibration does not pass through the agent's context step by step. There is also a template instrument for contributors. Adding an instrument means one driver module and one row in `DRIVER_KINDS` and `MODULES` (see
`docs/EXTENDING.md`). Each driver's manual pages carry a `verified` line saying what was checked on the real hardware and when.

| Driver (`driver` in the config) | What it is | Highlights | Section / doc |
|---|---|---|---|
| `siglent_sds` | Siglent SDS1000X-E oscilloscope over the LAN (written against an SDS1202X-E, firmware 1.3.27) | capture, screenshot, measure, spectrum, self-calibration | [Tools](#tools), [Scope functions](#scope-functions-probed-on-the-sds1202x-e-firmware-1327-2026-10-28) |
| `siglent_ssa` | Siglent SSA3000X spectrum analyzer with its tracking generator over the LAN (written against an SSA3021X, firmware 1.3.9.10); only the base analyzer and the TG option are used, nothing depends on the lapsing trial options (Meas, EMI, CAT) | status, screenshot, sweeps with a CSV and peaks, markers, tracking generator on, off and level (read_config, no confirm flags), through-normalised scalar S21 | [Spectrum analyzer](#spectrum-analyzer-and-tracking-generator-siglent_ssa-driver), [Tools](#tools) |
| `atm6301` | Abestop ATM6301 bench supply (0-60 V, 0-10 A), CH340 USB-serial | setpoints with caps, output switch with `confirm_load_connected`, `max_on_s` and a safe-state tool | [Tools](#tools) |
| `nanovna` (VNA) | NanoVNA-H (ChibiOS USB serial shell) behind the same `bench_vna_*` tools; NOT verified on hardware (the unit on the dev box did not answer 2026-10-31), tested against a fake, a pty fake and a synthetic transcript | S11/S21 sweeps (101 points on the H, 401 on the H4, 50 kHz to 900 MHz on the H), software SOLT calibration shared with the VNWA, power cycle with a hub; generator, AM, FM, meter and spectrum return `not supported by this instrument` | [manual page](manual/instruments/vna/nanovna.md) |
| `vnwa_open` (VNA) | DG8SAQ VNWA 3E vector network analyser through the `vnwa_open` crate, no vendor software | S11/S21 sweeps, calibration (open, short, load, through), generator (CW, level, AM, FM), frequency meter, spectrum analyser, firmware and temperature, reliability ladder with hub power cycle | [The VNA's functions](#the-vnas-functions-as-tools-2026-10-28) |
| `bladerf`, `limesdr` | SDRs (bladeRF with XB-200, LimeSDR-USB with two coherent receivers) | capture, survey, coherence; receive only | [Tools](#tools) |
| `rx888` | RX-888 MkII direct-sampling HF receiver (0 to 30 MHz) through ka9q-radio's `radiod` on loopback; verified on the unit with no antenna | firmware boot, radiod start and stop, front end (overrange, RF gain, attenuation), channel level and SNR, spectrum CSV, I/Q or audio WAV; receive only | [RX-888](#rx-888-receiver-rx888-driver) |
| `usb_hub` | USB topology and hub port power (uhubctl) | topology, find, diagnose, bandwidth, watch, history, port power (verified cold reset), protection of devices | [USB hub tools](#usb-hub-tools-usb_hub-driver), `docs/USB_HUB_TOOLS.md` |
| `audio` | Sound devices, with discovery by signature | play, record, meter, loopback, speak, alert, verify, synthesizers; never the system default device; `dry_run` and a safe state | [Audio tools](#audio-tools-audio-driver), `docs/AUDIO_TOOLS.md` |
| `hamlib` | Radios through Hamlib's `rigctld` (a separate process; LGPL, not linked) | models, get and set, scan, watch, memory, load checks, transmit tests with an attestation and a watchdog | [Hamlib rigs](#hamlib-rigs-hamlib-driver), `docs/HAMLIB_TOOLS.md` |
| `rotator` | Antenna rotators through `rotctld` | status, point, park, stop | [Rotators](#rotators-rotator-driver) |
| `procedure` | The procedure runner (Python) | long multi-step Python programs (CPython 3 with numpy and scipy) that call bench tools through the normal dispatcher, with failure reports, safe-state routines and saved examples | [Procedure runner](#procedure-runner-procedure-driver) |
| `elad` | ELAD FDM-S2, FDM-DUO and FDM-S1 USB receivers through the `elad_usb` crate; identity and calibration verified on an FDM-S2, streaming not yet (FPGA image) | list, identify, spectrum CSV, level at a frequency, cf32 I/Q capture; receive only | [ELAD receivers](#elad-receivers-elad-driver) |
| `example` | A template instrument with a simulated transport | the skeleton a new driver starts from, checked by the contributor tests | [Template instrument](#template-instrument-example-driver) |

Across all drivers: safety tiers (`read_only`, `read_config`, `full`) with `dry_run`, confirm arguments and a safe-state
companion on every tool that actuates; bounded timeouts and retries on every I/O call; the hierarchical **manual** served by the
MCP (`bench_manual_index`, `_read`, `_search`, `_for_tool`, `_whats_new`); the **contributor guide** for other people's agents
(`docs/EXTENDING.md`) with checklist tests that fail until a new tool has its page, tier, fixtures and anchors; and fixtures
scrubbed of serial numbers.

**Planned (not built yet):** an FX2LP bench instrument (GPIO, logic capture, I2C, SPI, a UART bridge, pulse and timing outputs,
frequency counting; see `night_shift_todo/low/fx2-bench-instrument-image-and-bench-mcp-tools-4d8f2b61.md` in hams_com).
USB cable testing is left to other contributors.

Licence: AGPL-3.0-or-later. Not published to crates.io.

## Configuration

`BENCH_MCP_CONFIG` names a JSON file (see `src/config.rs`):

    {"instruments": {
      "scope": {"driver": "siglent_sds", "tier": "read_config", "host": "192.168.10.51", "port": 5025},
      "sa":    {"driver": "siglent_ssa", "tier": "read_config", "host": "192.168.10.42", "port": 5025},
      "psu":   {"driver": "atm6301", "tier": "read_config", "serial": "/dev/serial/by-id/usb-EXAMPLE_supply-if00-port0", "baud": 115200,
                "max_volts": 5.5, "max_amps": 1.0, "max_on_s": 600},
      "vna":   {"driver": "vnwa_open", "tier": "read_config", "if_hz": 12000, "point_ms": 60, "rf_level": 16383},
      "vnas":  {"vnwa": {"driver": "vnwa_open", "tier": "read_config", "hub": "3-2", "hub_port": 2},
                "nano": {"driver": "nanovna", "tier": "read_config", "serial": "/dev/serial/by-id/usb-STMicroelectronics_ChibiOS_RT_Virtual_COM_Port_400-if00"}},
      "nano_alone":  {"driver": "nanovna", "tier": "read_config", "serial": "/dev/serial/by-id/usb-STMicroelectronics_ChibiOS_RT_Virtual_COM_Port_400-if00"},
      "blade": {"driver": "bladerf", "tier": "read_config", "xb200": "auto1db"},
      "lime":  {"driver": "limesdr", "tier": "read_config"},
      "rx888": {"driver": "rx888", "tier": "read_config", "radiod": "/usr/local/sbin/radiod",
                "driver_library": "/usr/local/lib/ka9q-radio/rx888.so", "boot": "/usr/local/sbin/rx888_boot",
                "firmware": "/usr/local/share/ka9q-radio/W1EUJ_0.1.0_FX3.img", "presets": "/usr/local/share/ka9q-radio/presets.conf",
                "status_group": "239.88.8.1", "data_group": "239.88.8.2"},
      "rig":   {"driver": "hamlib", "tier": "read_only", "model": 1},
      "rotor": {"driver": "rotator", "tier": "read_config", "rotctld_port": 4533, "min_az": 0, "max_az": 360}}}

The VNA entry's settings are all optional: `codec_s11` and `codec_s21` (parts of the ALSA ids of the two codecs; by default
they are found from their place on the USB tree, which stays right when a power cycle changes the kernel's card order),
`hub` and `hub_port` (the power-switching hub port the unit is on, for example `"3-2"` and `2`: with them a point that still
fails after `retries` (default 2) re-measurements gets one cold reset through `sudo -n uhubctl` and is measured again),
`if_hz` (a divisor of 48 kHz: 12000, 6000 or 4000), `point_ms` (0.2 to 100),
`rf_level` (0 to 16383), `settle_ms` and `window_ms`. A bad value is refused when the file is read.

The SDR entries need SoapySDR with the radio's module (`libsoapysdr0.8`, `soapysdr-module-bladerf`, `soapysdr-module-lms7`); the
library is loaded at run time, so the server runs without it and the SDR tools then say so. Optional: `serial` (hex digits, to
pick one board of several) and, for the bladeRF only, `xb200` (`50M`, `144M`, `222M`, `auto`, `auto1db`, `auto3db`, `custom`),
which switches on the XB-200 transverter and its filter bank so the receiver tunes from 9 kHz instead of 237.5 MHz.
The bladeRF needs its FPGA loaded (`bladeRF-cli -L hostedx115.rbf` writes it to flash for autoloading) and firmware 2.4 or later
for the FPGA 0.16; the LimeSDR needs firmware 4 and gateware 2.23 (`LimeUtil --update`).

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
| `bench_scope_spectrum` | read_config (it may set the timebase and memory depth) | scope |
| `bench_set` | read_config | both (supply setpoints are capped by `max_volts`/`max_amps`) |
| `bench_psu_output` | **full** (needs `confirm_load_connected: true`, has `dry_run`, switches itself off after `for_s`, at most `max_on_s`) | supply |
| `bench_psu_safe_state` | read_config (output off, read back) | supply |
| `bench_scope_self_cal` | full (needs `confirm_inputs_disconnected: true`) | scope |
| `bench_vna_sweep` | read_config (it puts RF on the TX port) | VNA |
| `bench_vna_generator_on`, `_off`, `_level`, `_am` | read_config (RF on the TX port at the unit's own level, about -17 dBm at 0 dB) | VNA |
| `bench_vna_meter` | read_config (the TX DDS runs at the centre during the measurement) | VNA |
| `bench_vna_calibrate_measure`, `bench_vna_calibrate_build` | read_config | VNA |
| `bench_vna_power_cycle` | read_config (switches the unit's hub port; the verification puts RF on the TX port) | VNA |
| `bench_sa_status`, `bench_sa_screenshot` | read_only | spectrum analyzer |
| `bench_sa_sweep`, `bench_sa_marker` | read_config | spectrum analyzer |
| `bench_sa_tg_on`, `_tg_off`, `_tg_level`, `bench_sa_tg_normalize`, `bench_sa_tg_s21` | read_config (RF out of the tracking generator, 0 dBm at most; off unless a tool turns it on; no confirm flags) | spectrum analyzer |
| `bench_sdr_capture`, `bench_sdr_survey`, `bench_sdr_coherence` | read_config (they tune the radio; receive only) | SDR |
| `bench_elad_list`, `bench_elad_identify` | read_only | ELAD receivers |
| `bench_elad_spectrum`, `bench_elad_level`, `bench_elad_capture` | read_config (writes a file in `capture_dir`; receive only) | ELAD receivers |
| `bench_rx888_status`, `bench_rx888_frontend` | read_only | RX-888 |
| `bench_rx888_boot`, `bench_rx888_radiod_start`, `bench_rx888_radiod_stop`, `bench_rx888_tune`, `bench_rx888_spectrum`, `bench_rx888_capture` | read_config (firmware boot re-enumerates the device; radiod and its channels are local; receive only) | RX-888 |

`bench_identify` and `bench_status` also answer for the VNA (USB present and answering a harmless read, hub, both codecs found from the USB tree, IF, time per point, level, power cycles done by this server).
`bench_vna_sweep` takes `start_hz`, optionally `stop_hz`, `points` (1 to 401, about 0.2 s each), `if_hz`, `point_ms`
and `rf_level`, measures S21 (RX channel) and S11 (reflection channel), saves a CSV into `BENCH_MCP_OUT` and returns a
summary (and the rows when there are at most 51). The values are **raw ratios of the unit's own channels, not calibrated**:
the cable, bridge and front-end are not removed, so a cable between TX and RX is not 0 dB (about -1 dB at 10 MHz rising to
+2.6 dB at 300 MHz, with a phase that is linear in frequency). While a sweep runs the codecs belong to the server: the
vendor program cannot use them at the same time.

`bench_identify` and `bench_status` answer for an SDR with its driver's hardware key and info (firmware, gateware, serial) and
receive channel count. `bench_sdr_capture` tunes to `freq_hz`, captures `samples` (up to 16 777 216) at `sample_rate_hz` and
`gain_db`, saves the raw I/Q (little-endian f32 pairs, full scale 1.0) into `BENCH_MCP_OUT` and returns RMS, peak, DC offset,
clipping, the noise floor, the strongest peaks and `stream_seconds` against `real_time_seconds` and `continuous`, whether the samples arrived at the sample rate (the bladeRF
with a newer FPGA than libbladeRF knows reports an overrun about every 504 samples without losing any, so the timing is trusted
and the driver's count is only reported). `bench_sdr_survey` steps the tuning over `start_hz` to `stop_hz` (at most 400 steps,
70 % of each capture's bandwidth kept, the radio's DC spike removed) and returns the floor and strongest peaks with a CSV.
`bench_sdr_coherence` (LimeSDR, two receivers on one oscillator) captures both channels, repeats with a fresh tune each time and
reports the phase and gain of channel 0 against channel 1 at the strongest tone, plus coherence; it needs one signal on both
inputs. `antenna` (LimeSDR only: `LNAH`, `LNAL` or `LNAW`) selects the receive connector; without it the driver uses LNAL at every
frequency, so a signal on an H or W connector is not heard. Levels are dBFS at the set gain, **not calibrated power**. The SDR tools do not transmit: there is no transmit tool for them.

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
* The hub port and retries of the VNA entry are read, and only as a pair. [@ANCHOR: bench_mcp_config]
* The power-cycle tool needs a hub in the config and read_config, and the VNA's lock does not hold up the scope or the supply. [@ANCHOR: bench_mcp_vna_power_cycle]
* The host FFT finds a tone's frequency and level in every window, and the spectrum tool does so through a scope link. [@ANCHOR: bench_mcp_spectrum]
* A calibration is built from stored standards and refused at other settings, cable or range; measuring a standard needs a valid session, standard and cable. [@ANCHOR: bench_mcp_vna_cal_build] [@ANCHOR: bench_mcp_vna_cal_apply]
* Several instances: an entry whose members are all instrument objects is a group of named instances (`"vnas": {"vnwa": {...}, "nano": {...}}`), the old single-object entry is one instance named by its key, every tool that takes `instrument` also takes `device`, omitting both with several fitting instances is an error that lists the names, `bench_identify` and `bench_status` without a device list every instance, each instance has its own tier and lock, and a config in which two instances claim one serial port, scope host, VNWA unit or hub port is refused. This dev box can run both VNAs: the VNWA 3E on its USB hub and the NanoVNA-H on `/dev/serial/by-id/usb-STMicroelectronics_ChibiOS_RT_Virtual_COM_Port_400-if00`. [@ANCHOR: bench_mcp_config_flatten] [@ANCHOR: bench_mcp_config_shared_claims] [@ANCHOR: bench_mcp_main_device_arg] [@ANCHOR: bench_mcp_main_survey] [@ANCHOR: bench_mcp_main_device_property]
* A VNWA that is not connected fails its tools with `device not present` before anything is attempted. [@ANCHOR: bench_vna_ensure_present]
* A sweep is written as a CSV with a header and one row per point; calibrated values go to a second CSV. [@ANCHOR: bench_mcp_vna_csv] [@ANCHOR: bench_mcp_vna_calibrated_csv]
* A `nanovna` entry reads its settings, parses the shell's replies (echo, prompt, partial reads, garbage, unknown commands, timeouts), reads identity and limits from `version`, `info` and `help`, checks sweep arguments against the unit's limits, sweeps by `scan` or by `sweep`/`data` and restores the range, and returns typed errors for what it cannot do. [@ANCHOR: bench_nanovna_parse_config] [@ANCHOR: bench_nanovna_parse_reply] [@ANCHOR: bench_nanovna_command] [@ANCHOR: bench_nanovna_identify] [@ANCHOR: bench_nanovna_limits] [@ANCHOR: bench_nanovna_parse_sweep_query] [@ANCHOR: bench_nanovna_parse_columns] [@ANCHOR: bench_nanovna_check_range] [@ANCHOR: bench_nanovna_measure] [@ANCHOR: bench_nanovna_sweep_args] [@ANCHOR: bench_nanovna_call] [@ANCHOR: bench_nanovna_error_display] [@ANCHOR: bench_nanovna_scan_points] [@ANCHOR: bench_nanovna_sweep_and_read] [@ANCHOR: bench_nanovna_open_shell] [@ANCHOR: bench_nanovna_sweep_tool] [@ANCHOR: bench_nanovna_power_cycle] [@ANCHOR: bench_nanovna_calibrate_measure]
* Calibration standards are stored, and a calibration applied, for any instrument. [@ANCHOR: bench_vna_tools_measure_names] [@ANCHOR: bench_vna_tools_store_standard] [@ANCHOR: bench_vna_tools_apply_setup]

## Checks this crate makes for the SDRs (anchors)

* The SoapySDR argument string is built from the config and a serial or XB-200 value cannot carry another argument. [@ANCHOR: bench_mcp_sdr_args]
* A capture's arguments are checked against each radio's tuning, rate and gain limits before the radio is opened. [@ANCHOR: bench_mcp_sdr_capture_args]
* A survey is bounded to 400 steps and a power-of-two FFT length. [@ANCHOR: bench_mcp_sdr_survey_args]
* The coherence tool checks its arguments before touching hardware. [@ANCHOR: bench_mcp_sdr_coherence_args]
* The averaged spectrum reads a tone's amplitude and offset, and peaks skip the DC spike. [@ANCHOR: bench_mcp_sdr_spectrum_math]
* The phase between two channels is measured block by block with its stability and coherence. [@ANCHOR: bench_mcp_sdr_phase_math]
* A capture that takes longer than its signal lasts is flagged as having lost samples. [@ANCHOR: bench_mcp_sdr_continuous]

## The manual (every tool and instrument function ships its page)

The documentation is part of the server: `manual/` holds one markdown page per topic, embedded in the binary by `build.rs`, and the agent reads it through the MCP with `bench_manual_index`, `bench_manual_read`, `bench_manual_search`, `bench_manual_for_tool` and `bench_manual_whats_new`, or as resources `bench-manual://<path>`. The framework is `src/manual.rs`. Every tool description ends with `Manual: tools/<tool name>`.

**Adding an instrument or function (for agents working for other owners): `docs/EXTENDING.md`, or `bench_manual_read concepts/extending`.** **Contributor rule: a new tool, a new function of an instrument or a changed behaviour ships its manual page in the same pull request, and the tests fail until it does.** What to add:

* `manual/tools/<tool name>.md` for every new tool: front matter `title` (the tool name), `summary` (one line), `see_also` (page paths from the root of the manual, no `.md`), `tools: [<tool name>]` first, `instrument` (names of directories under `manual/instruments/`), `tier` (`read_only`, `read_config` or `full`: the tool's own, as registered), `verified` (see below); the body has one `# <title>` heading, the purpose, a `## Arguments` section containing the line `<!-- arguments -->` (the argument table is generated from the tool's JSON schema; explain each argument that its schema does not describe, naming it in backticks, with unit, range and default), the return fields with units, side effects, failure modes and what to do, and a real example request and answer.
* The instrument's pages under `manual/instruments/<name>/` (`overview`, `setup`, `operation`, `measurement-recipes`, `limits`, `troubleshooting`, `protocol` as they apply). A new instrument is a new directory with an `index.md` whose body contains `<!-- children -->` (the list of pages is generated).
* A line in `manual/whats-new.md` under the build version.
* `verified` says what was checked on real hardware and when (`measured on the unit (...), 2026-10-29`), or starts with `fixtures only`, `code only`, `in progress`, `see <page>` or `n/a`. Do not claim more than was run.
* Pages are at most 16000 characters (split them), have exactly one `#` heading equal to the title, and link to other pages as `[text](instruments/vna/limits)` (paths from the root).

Checks this crate makes for the manual (anchors):

* Every file is a page with the required front matter, an honest `verified` line and a known tier. [@ANCHOR: bench_mcp_manual_front_matter]
* Every registered tool has a page under `tools/` with its own tier, every `tools:` entry names a registered tool. [@ANCHOR: bench_mcp_manual_tool_pages]
* The argument tables are generated from the schemas and every argument is explained. [@ANCHOR: bench_mcp_manual_arguments]
* Every tool description ends with its manual path. [@ANCHOR: bench_mcp_manual_pointers]
* `see_also` entries and links resolve. [@ANCHOR: bench_mcp_manual_links]
* Every directory has an index page that lists all its children, and every page is reachable from the top. [@ANCHOR: bench_mcp_manual_index_pages]
* Pages are valid markdown of bounded length. [@ANCHOR: bench_mcp_manual_markdown]
* The five tools (index, read with sections and cutting, BM25 search with filters, for-tool, whats-new) and the resources. [@ANCHOR: bench_mcp_manual_index] [@ANCHOR: bench_mcp_manual_read] [@ANCHOR: bench_mcp_manual_search] [@ANCHOR: bench_mcp_manual_for_tool] [@ANCHOR: bench_mcp_manual_whats_new] [@ANCHOR: bench_mcp_manual_resources]

## Tests

`cargo test` runs the pure checks. `BENCH_MCP_CONFIG=<file> cargo test -- --ignored` needs the real
instruments: it starts the server binary, does the MCP handshake and calls tools; it never switches
the supply output on.

## Checks this crate makes (anchors)

* The supply's limits refuse out-of-range and non-finite values before any command is sent. [@ANCHOR: bench_mcp_psu_caps]
* An instrument is picked by name, or implicitly when exactly one configured instrument fits the tool. [@ANCHOR: bench_mcp_pick]
* The configuration file parses both drivers (scope over TCP, supply over serial) with their defaults. [@ANCHOR: bench_mcp_config]
* The scope waveform analysis returns the edges and the frequency of a square wave. [@ANCHOR: scope_mcp_analyse]

## Scope functions probed on the SDS1202X-E (firmware 1.3.27, 2026-10-28)

* **Spectrum**: `bench_scope_spectrum` computes the FFT on the host from the waveform memory (up to 1 M points of 14 M; rect, Hanning, Hamming, Blackman) and lists the
  peaks. The scope's own FFT is reachable with `FFTW` (window), `FFTC` (centre), `FFTS` (scale), `FFTU` (unit) and `DEFINE EQN,"FFT(C1)"` (all answered), and
  `MATH:WF? DAT2` returns 1500 int8 points, but the trace's frequency axis and scale were not decoded, so the host FFT is the supported way. Real check: the
  generator at 10 MHz, -6 dB, through the pad and tee reads 9 999 965 Hz (the scope's timebase 3.5 ppm off or the unit's) at 5.22 mV rms (-45.65 dBV, -32.6 dBm; 1.5 % from the
  prediction), with the 360 MHz DDS clock line 36 dB down and 350 and 370 MHz images (the scope's 200 MHz bandwidth makes these lower than the real levels). Second and third
  harmonics were below the 8-bit floor. Level is rms at the 1 MOhm input; dBm assumes the 50 ohm source into 50 ohms.
* **Measurements**: `bench_measure` takes every parameter the scope's `PAVA?` answers (PKPK, MAX, MIN, AMPL, TOP, BASE, CMEAN, MEAN, RMS, CRMS, OVSN, FPRE, OVSP, RPRE, PER,
  FREQ, PWID, NWID, RISE, FALL, WID, DUTY, NDUTY). `PAVA? ALL` and the statistics query `PACU?` did not answer; the counter is wrong above about 150 MHz.
* **Triggers**: `bench_set` takes `trigger_type` EDGE, SLEW (slope), GLIT (pulse), INTV (interval), RUNT and TV, which the unit accepted; window, pattern, drop-out and the serial-bus
  (I2C, SPI, UART, CAN, LIN) triggers were not accepted with the tokens tried, so they are either licensed options or named differently. Serial decode was not found.
* **Cursors** exist (`CRMS?` answers) but are not wrapped. Channel math (add, subtract, multiply, divide) is not wrapped: only channel 1 is connected here.
* The display sleeps after 30 minutes (`SCSV 30MIN`); SCPI measurements and `SCDP` screenshots still work while it sleeps. `SCSV OFF` was not changed.

## The VNA's functions as tools (2026-10-28)

`bench_vna_sweep` (optionally corrected with `calibration=<file>` and the same `cable` text), `bench_vna_calibrate_measure` and `_build` (open, short, load, thru; a kit file with
the real kit's published numbers, see vnwa_open `cal::KitDefinition`), `bench_vna_generator_on`/`_off`/`_level`/`_am`, `bench_vna_meter`, `bench_vna_status`,
`bench_vna_power_cycle`. There is no spectrum-analyzer tool: the vendor's rudimentary VNA spectrum mode has not been decoded (the scope's `bench_scope_spectrum` is the
spectrum tool). The AM tool reproduces the one captured setting (500 Hz, 100 %, -20 dB) and only a power cycle stops it. A running generator and a sweep or the meter must not
overlap (the burst overwrites the generator's settings): one VNA call at a time is enforced by the server's per-instrument lock, but switch the generator off before a sweep.

## USB hub tools (`usb_hub` driver)

`bench_usb_topology`, `bench_usb_find`, `bench_usb_hubs`, `bench_usb_protection`, `bench_usb_history`, `bench_usb_watch`, `bench_usb_bandwidth`, `bench_usb_diagnose` and `bench_usb_port_power` (and the ladder tools `bench_usb_reset`, `_authorize`, `_driver`, `_autosuspend`) describe and control the host's USB hubs and ports (read from sysfs without root; power through `sudo -n uhubctl`).
Registration, tiers, labels and the safety rules are in `docs/USB_HUB_TOOLS.md`. Tests read a recording of this development machine's `/sys`, `uhubctl`, `lsusb` and kernel journal in
`tests/usb_fixtures/devbox` (made by `tests/usb_fixtures/capture.py`).

* The USB tree, its per-device JSON and the label lookup are checked against the recorded tree. [@ANCHOR: usb_hub_sysfs_tree]
* The `usb_hub` config entry (labels, critical ports and devices) parses and refuses bad entries. [@ANCHOR: usb_hub_config]
* `uhubctl` output (with and without descriptions) and the hub class descriptor parse from recordings. [@ANCHOR: usb_hub_uhubctl_parse]
* The protection list: root filesystem, mounts, keyboards, config, bench serial adapters, hubs on the path, hub uplinks. [@ANCHOR: usb_hub_protection]
* A power cycle removes and restores the port's device in a copy of sysfs, with the measured times and the new device number. [@ANCHOR: usb_hub_power_cycle]
* A unit that does not come back gets a second cycle with a longer off time. [@ANCHOR: usb_hub_power_escalation]
* Port power: tiers, dry run, protection refusals, ganged and unlisted hubs refused. [@ANCHOR: usb_hub_port_power]
* The verification levels 1 to 4 and their honest note. [@ANCHOR: usb_hub_verification_levels]
* The root helper accepts only its four operations and refuses every other argument before a write. [@ANCHOR: usb_hub_privileged_ops]
* The escalation ladder (reset, authorize, driver, autosuspend) goes through the helper, honours protection and dry run. [@ANCHOR: usb_hub_ladder]
* Kernel journal lines are classified (connect, disconnect, reset, enumeration failure, errno) and counted per device, with flapping detection, against the recorded journal. [@ANCHOR: usb_hub_history_parse]
* A watch reports a device leaving and returning with a new device number, and the watch checks its arguments (1 to 120 s) and runs for its duration; a test drives the same loop by poll count instead of by sleeping, so a loaded machine cannot miss the outage. [@ANCHOR: usb_hub_watch] [@ANCHOR: usb_hub_watch_with]
* Bandwidth: the VNWA's four full-speed devices share one translator and the endpoint arithmetic follows the USB reservation rules. [@ANCHOR: usb_hub_bandwidth]
* Diagnose: missing, powered-off, deauthorized and journal-backed findings. [@ANCHOR: usb_hub_diagnose]
* The power budget flags a bus-powered hub asked for more current than it has. [@ANCHOR: usb_hub_power_budget]

## Audio tools (`audio` driver)

`bench_audio_*` tools work with the host's sound devices through PipeWire: device list, status and limits, then record, play, verify a sound, detect events, speak, alert and discovery by listening.
Registration, tiers, the volume cap, quiet hours and the reserved VNWA codecs are in `docs/AUDIO_TOOLS.md`. Tests read a recording of this development machine's PipeWire graph in `tests/audio_fixtures/devbox`.

* The `audio` config entry parses and refuses bad entries; the VNWA codec is always reserved. [@ANCHOR: audio_config]
* The device list comes from `pw-dump`; the VNWA codecs are reserved by usb id. [@ANCHOR: audio_devices_parse]
* `wpctl get-volume` output parses. [@ANCHOR: audio_levels_parse]
* WAV, tones, level statistics and the tone-heard decision are tested on synthetic recordings. [@ANCHOR: audio_wav] [@ANCHOR: audio_tones] [@ANCHOR: audio_analysis] [@ANCHOR: audio_heard_tone]
* The volume limits, the restoring volume guard and the managed capture directory. [@ANCHOR: audio_safety_limits] [@ANCHOR: audio_safety_guard] [@ANCHOR: audio_store_managed] [@ANCHOR: audio_store_retention]
* `tests/hil_audio.rs` (ignored by default) runs record, play, loopback and volume tools against the real PipeWire through a silent virtual sink and source. [@ANCHOR: audio_record_tool] [@ANCHOR: audio_play_tool] [@ANCHOR: audio_loopback_tool] [@ANCHOR: audio_set_volume_tool]
* Sound events (beeps, clicks, quiet rooms), Morse decoding, the verification judgment, and the verify and events tools end to end on a virtual sink. [@ANCHOR: audio_events_detect] [@ANCHOR: audio_events_tool] [@ANCHOR: audio_morse] [@ANCHOR: audio_verify_judge] [@ANCHOR: audio_verify_tool]
* A stream aimed at a missing node fails instead of falling back to the default device (which could be a reserved one). [@ANCHOR: audio_engine_no_fallback]
* Speech: engine choice, voice list from a recording of `espeak-ng --voices`, command lines with the text on stdin; speaking to a WAV file and to a virtual output. [@ANCHOR: audio_speak_choose] [@ANCHOR: audio_speak_voices] [@ANCHOR: audio_speak_tool]
* Alerts: volume by severity and quiet hours; the text is returned also when no sound can be made. [@ANCHOR: audio_alert_volume] [@ANCHOR: audio_alert_tool]
* Audio path discovery: conclusions on synthetic observations (heard, not heard, dead input, no microphone, HDMI last, crosstalk), the volume ladder, staleness by device fingerprint, `auto` selection, and a run against a virtual output and input. [@ANCHOR: audio_discover_conclude] [@ANCHOR: audio_discover_ladder] [@ANCHOR: audio_discover_stale] [@ANCHOR: audio_select_auto] [@ANCHOR: audio_discover_auto_rerun] [@ANCHOR: audio_discover_tool]

## Hamlib rigs (`hamlib` driver)

`bench_hamlib_*` tools drive radios through Hamlib's `rigctld` daemon (a separate program, started by this server on 127.0.0.1 or already running; Hamlib is LGPL and is not linked): the model list, the rigs and their state, a receiver's status in one call, single values and signal readings, receiver control (frequency, mode, VFO, split, RIT/XIT, levels, functions, memory recall, a bounded scan), a connection test, a polling watch of the dial, the reference station table, a receiver test with the rig's audio (through the sound module's input names), the dummy-load plausibility check and its baseline (evidence, not proof; for rigs with the owner's transmit attestation), and the managed daemon. An entry is `{"driver": "hamlib", "tier": "read_only", "model": 1}` (the Hamlib dummy rig) or `{"driver": "hamlib", "tier": "read_only", "model": 1021, "device": "/dev/serial/by-id/usb-EXAMPLE_rig-if00-port0", "speed": 19200}`. Keys, tiers and the safety model are in `docs/HAMLIB_TOOLS.md` and the manual pages `instruments/hamlib/*`. Tests run against Hamlib's dummy rig through a managed rigctld on a free localhost port, and against a scripted fake of rigctld for faults; nothing here has run on a real rig.

* The config takes a managed or an existing rigctld, refuses remote daemons and bad values. [@ANCHOR: hamlib_config]
* Return codes have names and next steps; arguments cannot carry a second command. [@ANCHOR: hamlib_error_codes] [@ANCHOR: hamlib_arg_check]
* A command gets its reply or a bounded error; a read is tried once more, a write never. [@ANCHOR: hamlib_command] [@ANCHOR: hamlib_read_retry]
* The managed daemon binds localhost, starts on demand, is stopped on request, and a leftover of a dead server is stopped (a live owner is never touched). [@ANCHOR: hamlib_rigctld_args] [@ANCHOR: hamlib_rigctld_start] [@ANCHOR: hamlib_rigctld_ensure] [@ANCHOR: hamlib_rigctld_tool] [@ANCHOR: hamlib_reap_stale] [@ANCHOR: hamlib_release_after_crash]
* A missing or unusable serial device is reported before a daemon starts. [@ANCHOR: hamlib_device_check]
* The model table and capabilities are read from `rigctl` output, by the columns of its header. [@ANCHOR: hamlib_models_list] [@ANCHOR: hamlib_models_caps] [@ANCHOR: hamlib_models_tool]
* Status, single values and signal readings; what the rig lacks is reported, not hidden. [@ANCHOR: hamlib_s_units] [@ANCHOR: hamlib_capability_query] [@ANCHOR: hamlib_status] [@ANCHOR: hamlib_get] [@ANCHOR: hamlib_signal_readings]
* The tools open the rig, run under the tier check and list the rigs. [@ANCHOR: hamlib_open] [@ANCHOR: hamlib_call] [@ANCHOR: hamlib_rigs]
* Receiver control: the receive ranges come from the rig and the owner's limits and a frequency outside them is refused with the source of the limit; arguments are checked before anything is sent and transmit-side settings are refused; every change is read back; a keyed rig is left alone. [@ANCHOR: hamlib_dump_ranges] [@ANCHOR: hamlib_rx_ranges] [@ANCHOR: hamlib_freq_guard] [@ANCHOR: hamlib_set_plan] [@ANCHOR: hamlib_set] [@ANCHOR: hamlib_memory]
* A scan is bounded in steps and time, ranks the strongest and restores the rig, also after a failure. [@ANCHOR: hamlib_scan]
* The connection test reads the system and opens no serial port. [@ANCHOR: hamlib_device_holders] [@ANCHOR: hamlib_diagnose]
* The transmit attestation is required in full, defaults nothing wider and refuses a false or future-dated one; the station table ships WWV, WWVH, AM samples and weather radio (not CHU), is edited by the owner, and records last heard from detections. [@ANCHOR: hamlib_attest_config] [@ANCHOR: hamlib_attest_number] [@ANCHOR: hamlib_stations_defaults] [@ANCHOR: hamlib_stations_recent] [@ANCHOR: hamlib_stations_select] [@ANCHOR: hamlib_stations_edit] [@ANCHOR: hamlib_stations_save] [@ANCHOR: hamlib_stations_load] [@ANCHOR: hamlib_stations_heard] [@ANCHOR: hamlib_station_check] [@ANCHOR: hamlib_station_class] [@ANCHOR: hamlib_station_days] [@ANCHOR: hamlib_station_from_json] [@ANCHOR: hamlib_stations_tool] [@ANCHOR: hamlib_station_set_tool]
* Time-station detection on synthetic audio: the one-second tick rhythm with and without noise, 440/500/600 Hz tones, a carrier beat note, silence, and mains hum that must not trigger. [@ANCHOR: hamlib_detect_hum] [@ANCHOR: hamlib_detect_tones] [@ANCHOR: hamlib_detect_ticks] [@ANCHOR: hamlib_detect_beats] [@ANCHOR: hamlib_detect_analyze]
* The load plausibility check: the decision on readings (at the baseline, above it, unsteady, no baseline, other settings, a station heard, a dead or unreadable input), the baseline files, the measurement that restores the rig, and the tools. [@ANCHOR: hamlib_load_decide] [@ANCHOR: hamlib_verdict_word] [@ANCHOR: hamlib_baseline_load] [@ANCHOR: hamlib_baseline_save] [@ANCHOR: hamlib_load_audio_seconds] [@ANCHOR: hamlib_load_mode_for] [@ANCHOR: hamlib_load_read_original] [@ANCHOR: hamlib_load_restore] [@ANCHOR: hamlib_load_read_point] [@ANCHOR: hamlib_load_measure] [@ANCHOR: hamlib_load_note_heard] [@ANCHOR: hamlib_loadtools_measure] [@ANCHOR: hamlib_loadtools_prepare] [@ANCHOR: hamlib_baseline_problems] [@ANCHOR: hamlib_load_check_tool] [@ANCHOR: hamlib_load_baseline_tool]
* The receiver test with the rig's audio, and how the audio input is found from the sound module's names. [@ANCHOR: hamlib_rx_check] [@ANCHOR: hamlib_rx_listen] [@ANCHOR: hamlib_audio_resolve] [@ANCHOR: hamlib_audio_capture]
* Transmitting into the attested dummy load: the arguments against the attestation, the rig read first (transmit range, PTT, split, XIT, the power setting against the cap, the lowest power), the load check as a gate, the operator alert, the low-power SWR probe, the keyed interval with its watchdog, release and read-back on every path, the keyed-interval log, the power sweep and the safe state. [@ANCHOR: hamlib_dump_state] [@ANCHOR: hamlib_startup_note] [@ANCHOR: hamlib_tx_alert] [@ANCHOR: hamlib_tx_args] [@ANCHOR: hamlib_tx_gate] [@ANCHOR: hamlib_tx_key] [@ANCHOR: hamlib_tx_key_test] [@ANCHOR: hamlib_tx_keyed_drop] [@ANCHOR: hamlib_tx_keyed_released_flag] [@ANCHOR: hamlib_tx_log] [@ANCHOR: hamlib_tx_min_power] [@ANCHOR: hamlib_tx_operator_alert] [@ANCHOR: hamlib_tx_power_convert] [@ANCHOR: hamlib_tx_preflight] [@ANCHOR: hamlib_tx_probe] [@ANCHOR: hamlib_tx_release] [@ANCHOR: hamlib_tx_release_all] [@ANCHOR: hamlib_tx_safe_state] [@ANCHOR: hamlib_tx_startup_check] [@ANCHOR: hamlib_tx_sweep] [@ANCHOR: hamlib_tx_swr_valid] [@ANCHOR: hamlib_tx_take_note]
* The transmit watchdog process: its unkey, its run loop (deadline, closed pipe, abort, done), its entry point and how the tool starts it. [@ANCHOR: hamlib_guard_main] [@ANCHOR: hamlib_guard_run] [@ANCHOR: hamlib_guard_spawn] [@ANCHOR: hamlib_guard_unkey] [@ANCHOR: hamlib_load_gate]
* Plumbing of the transmit path: the meter readings, the verified settings, the restore, the keyed call, the summary, the watchdog's stand-down, the `chk_vfo` question before `dump_state` and the shutdown of all rigs. [@ANCHOR: hamlib_chk_vfo] [@ANCHOR: hamlib_guard_stand_down] [@ANCHOR: hamlib_shutdown_all] [@ANCHOR: hamlib_tx_keyed] [@ANCHOR: hamlib_tx_read_meters] [@ANCHOR: hamlib_tx_restore] [@ANCHOR: hamlib_tx_set_verified] [@ANCHOR: hamlib_tx_summary]
* Watching the dial by polling, and releasing a possibly keyed rig before its daemon is stopped. [@ANCHOR: hamlib_tx_release_rig] [@ANCHOR: hamlib_watch] [@ANCHOR: hamlib_watch_poll]
* Internals of the client and the daemon manager: connection, exchange, error text, daemon directory, endpoint, start, stop, status, log tail, shutdown, the rig choice and file-safe names. [@ANCHOR: hamlib_connect] [@ANCHOR: hamlib_reopen] [@ANCHOR: hamlib_exchange] [@ANCHOR: hamlib_reply_number] [@ANCHOR: hamlib_error_display] [@ANCHOR: hamlib_code_next_step] [@ANCHOR: hamlib_managed_dir] [@ANCHOR: hamlib_endpoint] [@ANCHOR: hamlib_kill_slot] [@ANCHOR: hamlib_log_tail] [@ANCHOR: hamlib_shutdown] [@ANCHOR: hamlib_rigctld_explicit_start] [@ANCHOR: hamlib_rigctld_status] [@ANCHOR: hamlib_rigctld_stop] [@ANCHOR: hamlib_instrument] [@ANCHOR: hamlib_safe_name]
* Helpers of the status and control tools: argument bounds, level text, tolerated rig refusals, applied-change reporting, the keyed-rig check, range combination, scan stepping, and the connection test's line settings and speed check. [@ANCHOR: hamlib_bounded] [@ANCHOR: hamlib_level_json] [@ANCHOR: hamlib_s_text] [@ANCHOR: hamlib_tolerate] [@ANCHOR: hamlib_apply] [@ANCHOR: hamlib_ptt_is_on] [@ANCHOR: hamlib_combine_ranges] [@ANCHOR: hamlib_scan_steps] [@ANCHOR: hamlib_lines_note] [@ANCHOR: hamlib_speed_in]

## Rotators (`rotator` driver)

`bench_rotator_*` tools drive an antenna rotator through Hamlib's `rotctld` (a separate program that the owner starts; Hamlib is LGPL and is not linked), with the same client as the hamlib module: position and limits, a move inside the limits (the rotator's own narrowed by the owner's `min_az`, `max_az`, `min_el`, `max_el`) with `dry_run`, and the safe-state pair stop and park. An entry is `{"driver": "rotator", "tier": "read_config", "rotctld_port": 4533, "min_az": 0, "max_az": 360, "min_el": 0, "max_el": 90}`. A managed `rotctld` is not done. Tests run against Hamlib's dummy rotator in its own `rotctld` and a scripted fake; no real rotator. Pages: `instruments/rotator/*`.

* The config needs a port, keeps rotctld on this machine and refuses bad limits; the instrument is chosen by name; errors end with a rotator's next action. [@ANCHOR: rotator_config] [@ANCHOR: rotator_instrument] [@ANCHOR: rotator_error_text] [@ANCHOR: rotator_call] [@ANCHOR: rotator_degrees] [@ANCHOR: rotator_wait_arg]
* Position, limits (narrowed by the owner's), the target check and the motion test. [@ANCHOR: rotator_position] [@ANCHOR: rotator_limits] [@ANCHOR: rotator_target_check] [@ANCHOR: rotator_moving] [@ANCHOR: rotator_status]
* A move is checked before it is sent, honours `dry_run`, waits for arrival and says when it did not arrive; the stop confirms the motion ended and works while another call runs; the park. [@ANCHOR: rotator_point] [@ANCHOR: rotator_wait] [@ANCHOR: rotator_stop] [@ANCHOR: rotator_park]

## Template instrument (`example` driver)

`src/example/` is a working skeleton for adding an instrument: an imaginary SCPI signal source with four tools (`bench_example_status`, `bench_example_measure`, `bench_example_safe_state`, `bench_example_pulse`), its manual pages, a recorded-session fixture, a fake device for the tests and an ignored hardware check (`tests/hil_example.rs`). It is listed only when the config has an entry such as `{"driver": "example", "tier": "read_config", "host": "192.0.2.10", "port": 5025, "max_level_v": 1.0, "max_pulse_s": 2.0}`; no real instrument answers to it. `python3 tools/new_instrument.py <name>` copies it as the start of a new driver. The checks in `src/checklist.rs` (a page for every tool, the standard instrument pages, `dry_run` on every new `full` tool, anchors that pair up, no addresses or credentials in fixtures and pages) run with `cargo test`. [@ANCHOR: bench_mcp_checklist]

Checks this crate makes for the template instrument (anchors):

* The config takes limits with defaults and refuses bad ones. [@ANCHOR: example_config]
* Arguments must be present, finite and in range, with the cause of the limit in the error. [@ANCHOR: example_args]
* A reply that is not a finite number is an error, never a zero. [@ANCHOR: example_readings]
* An address that does not answer as the expected model is sent nothing else. [@ANCHOR: example_identity]
* A query is retried a bounded number of times and then reported with the next step. [@ANCHOR: example_retry]
* Pulse arguments are checked against the rating and the owner's limits before anything is sent. [@ANCHOR: example_pulse_args]
* A pulse needs `confirm`, honours `dry_run`, and ends with the output off and verified off. [@ANCHOR: example_pulse]
* The safe state is verified by reading it back, and a failure says what to do by hand. [@ANCHOR: example_safe_state]
* The VNWA power-cycle hook is the hub module's one implementation, installed on the VNA. [@ANCHOR: bench_vna_power_hook]
* `bench_vna_spectrum` captures a spectrum and `bench_vna_modulation_stop` stops a running modulation; both are exercised on real hardware by `tests/hil.rs` (ignored by default). [@ANCHOR: bench_vna_spectrum] [@ANCHOR: bench_vna_modulation_stop]
* A sound node reserved by its USB topology (the VNWA codecs, found by bus path as well as by id) is refused as a target. [@ANCHOR: audio_reserved_by_topology]

## Spectrum analyzer and tracking generator (`siglent_ssa` driver)

A Siglent **SSA3000X** spectrum analyzer (written against an SSA3021X, firmware 1.3.9.10, 9 kHz to 2.1 GHz) over its LAN raw SCPI socket (port 5025), with its **tracking generator** (the TG option). Config entry: `{"driver": "siglent_ssa", "tier": "read_config", "host": "192.168.10.42", "port": 5025}`; `max_tg_dbm` (-20 to 0, default 0) is an optional extra cap on the tracking-generator level. The USB connection is not used: its replies are cut short.

**Only the base analyzer and the tracking generator are used.** The Meas, EMI and CAT options are trial options that lapse; no tool depends on them, and `bench_sa_status` merely lists whatever the unit reports. The tracking generator is treated like any other instrument setting: tier `read_config`, no confirm flags (its output is at most 0 dBm, so the manual says that it puts RF on the TG OUTPUT connector and nothing more). It is OFF unless a tool turns it on; the S21 tools switch it off again by default, and every switch-off is read back from the unit.

| Tool | Tier | What it does |
|---|---|---|
| `bench_sa_status` | read_only | identity, options, tracking generator state and level, sweep settings; changes nothing |
| `bench_sa_screenshot` | read_only | the screen as a 24-bit BMP (1024 x 600) in `BENCH_MCP_OUT` |
| `bench_sa_sweep` | read_config | set the range and optionally RBW, VBW, reference level, attenuation, detector, averaging; one sweep; the 751-point trace as a CSV, a summary and the strongest peaks |
| `bench_sa_marker` | read_config | marker 1 to 4 on trace 1: switch on, search or place, read frequency and level |
| `bench_sa_tg_on`, `bench_sa_tg_off`, `bench_sa_tg_level` | read_config | tracking generator on at a level (set and read back first), off (read back, up to 3 tries), level without changing on or off |
| `bench_sa_tg_normalize` | read_config | the through: TG cabled to the RF input, sweep with the TG on, store as the 0 dB reference (memory and a CSV with its settings) |
| `bench_sa_tg_s21` | read_config | scalar S21 sweep between the TG output and the RF input, relative to a stored or file reference, else to the TG level |

Every setting a tool asks for is read back from the unit and compared; a value the unit changed (a clamped range, a reference level moved by the coupled attenuator) is an error, not a silent substitute. An address that does not answer `*IDN?` as an `SSA3...` is sent nothing else. Queries are repeated (3 tries, 5 s); commands that change the instrument are never repeated blind. Fixtures: `tests/sa_fixtures/status.txt` (a scrubbed recorded session) and the simulator in `src/sa/testkit.rs`, both served on a local TCP socket so the production `Link::tcp` path is the one tested. `tests/hil_sa.rs` (`--ignored`) talks to the real unit and leaves the generator off.

### Anchor reference (ADR 0113)

Files: `src/sa/mod.rs` (config, tool list, shared helpers, status, dispatch), `src/sa/sweep.rs` (sweep, trace, peaks, CSV, marker, screenshot), `src/sa/tg.rs` (tracking generator, reference, S21), `src/sa/testkit.rs` (the fake instrument, test only). All functions are blocking, synchronous calls that run on the server's tool thread and hold one TCP connection to the unit for the duration of the call; the unit allows one sweep at a time, so two tools on one analyzer are serialised by the server's per-instrument lock (`bench_mcp_main_instrument_lock`). None of them can key a transmitter: the tracking generator is a low-level (at most 0 dBm) test source on its own connector, and it is not a radio transmit path, so the relay's lock, band check and transmit gate do not apply.

Contents: [Data structures](#sa-data-structures) | [mod.rs functions](#sa-modrs-functions) | [sweep.rs functions](#sa-sweeprs-functions) | [tg.rs functions](#sa-tgrs-functions)

#### Common errors

Every I/O error ends with the next step (check the LAN cable and host, call `bench_sa_status`, power-cycle the unit, never use USB). `SAFE STATE NOT CONFIRMED` means the unit did not report the generator off after 3 tries; the message says to press TG then TG off on the front panel or disconnect what is on the TG OUTPUT.

### SA data structures

<a id="sa-data-structures"></a>

#### `bench_mcp_sa_config` - `Config`

[@ANCHOR: bench_mcp_sa_config]

The `siglent_ssa` settings of an instrument entry, in `src/sa/mod.rs`; held in `Instrument.sa` (`src/config.rs`) for the life of the process.

| Field | Type | Meaning |
|---|---|---|
| `max_tg_dbm` | `f64`, dBm | Optional owner cap on the tracking-generator level. Valid -20.0 to 0.0 (the unit's own rating). Default 0.0, which is no extra cap. Set by `parse_config` from the config key `max_tg_dbm`; read by `level_arg`, `switch_on` and `status`. |

Invariants: finite and inside the rating whenever it came from `parse_config`. Created when the config file is read, immutable afterwards, shared read-only by all tool calls. Wire form: the optional key `max_tg_dbm` in the instrument's JSON entry; no versioning (an absent key means the default). Concurrency: none, it is never mutated.

#### `bench_mcp_sa_limits` - fixed limits

[@ANCHOR: bench_mcp_sa_limits]

Constants in `src/sa/mod.rs` and `src/sa/sweep.rs` and `src/sa/tg.rs` that bound what the driver does.

| Constant | Value | Meaning |
|---|---|---|
| `MODEL_PREFIX` | `"SSA3"` | The model name from `*IDN?` must start with this; otherwise nothing else is sent. |
| `REPLY_TIMEOUT_S` | 5 s | Read timeout of an ordinary exchange. |
| `READ_TRIES` | 3 | Tries of a query (never of a command). |
| `OFF_TRIES` | 3 | Attempts to switch the generator off and see it off. |
| `TG_RATED_MIN_DBM`, `TG_RATED_MAX_DBM` | -20.0, 0.0 dBm | The generator's rated output range (the unit's guide, section 3.9.2); `max_tg_dbm` may only lower the top. |
| `POINTS` (`sweep.rs`) | 751 | Points in a trace; fixed by the unit, no SCPI command sets it. A trace of another length is refused. |
| `MAX_SWEEP_S` (`sweep.rs`) | 120 s | Longest total sweep time (sweep time times average count) a tool will start. |
| `LEVEL_TOLERANCE_DB` (`tg.rs`) | 0.05 dB | How far a level read back may differ from the level set. |

Plain compile-time constants: no ownership, no wire form, no locking.

#### `bench_mcp_sa_spec` - `Spec`

[@ANCHOR: bench_mcp_sa_spec]

One tool's definition, in `src/sa/mod.rs`; created at compile time as an element of `SPECS`.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | The MCP tool name, `bench_sa_*`; must match a match arm in `call_with` and a manual page `manual/tools/<name>.md`. |
| `tier` | `Tier` | The lowest tier at which the tool is listed and allowed (`ReadOnly`, `ReadConfig`, `Full`). |
| `desc` | `&'static str` | The description the model sees; `tool_defs` prefixes it with the tier. |
| `props` | `&'static str` | The JSON-schema `properties` object as text (valid JSON, parsed by `tool_defs`). |
| `required` | `&'static [&'static str]` | Required argument names. |

Invariants: names are unique; `props` parses as a JSON object. Read-only, shared by all threads.

#### `bench_mcp_sa_specs` - `SPECS`

[@ANCHOR: bench_mcp_sa_specs]

The table of the nine tools (`bench_sa_status`, `_sweep`, `_marker`, `_screenshot`, `_tg_on`, `_tg_off`, `_tg_level`, `_tg_normalize`, `_tg_s21`) as `&[Spec]`. The sweep arguments shared by `bench_sa_sweep`, `bench_sa_tg_normalize` and `bench_sa_tg_s21` come from the macro `sweep_props!` (`instrument`, `start_hz`, `stop_hz`, `center_hz`, `span_hz`, `rbw_hz`, `vbw_hz`, `ref_level_dbm`, `attenuation_db`, `detector`, `average_count`, `continuous_after`). Adding a tool means one entry here, one arm in `call_with`, one manual page and a test. Read by `tool_defs` and `call_with`.

#### `bench_mcp_sa_ident` - `Ident`

[@ANCHOR: bench_mcp_sa_ident]

What `open` learned from `*IDN?`, in `src/sa/mod.rs`; created per call, dropped when the call returns.

| Field | Type | Meaning |
|---|---|---|
| `make` | `String` | Manufacturer field, for example `Siglent Technologies`. |
| `model` | `String` | Model, starts with `SSA3`. |
| `serial` | `String` | Serial number as reported (never stored in this repository's fixtures: `SN-0000`). |
| `firmware` | `String` | Firmware version. |
| `max_hz` | `f64`, Hz | The model's top frequency from the name (`SSA3021X` is 2.1e9): the limit for frequency arguments. |

#### `bench_mcp_sa_connector` - `Connector`

[@ANCHOR: bench_mcp_sa_connector]

`type Connector = dyn Fn(&Instrument) -> Result<Link, String>`: how a call reaches the instrument. Production passes `tcp`; the tests pass a closure to a fake instrument on a local socket, so the full path is the production one. It is called once per tool call (and by nothing else), borrowed, and must be safe to call from the tool thread. A failure to connect is the returned error text.

#### `bench_mcp_sa_span` - `Span`

[@ANCHOR: bench_mcp_sa_span]

The frequency range of a sweep, in `src/sa/sweep.rs`.

| Field | Type | Meaning |
|---|---|---|
| `start_hz` | `f64`, Hz | First frequency, 0 to the model's top. |
| `stop_hz` | `f64`, Hz | Last frequency, at least 100 Hz above `start_hz`, not above the model's top (+0.5 Hz). |

Copied by value; created by `sweep_args`.

#### `bench_mcp_sa_sweep_req` - `SweepReq`

[@ANCHOR: bench_mcp_sa_sweep_req]

A checked sweep request, created by `sweep_args` and read by `run_sweep`; a `None` field means "leave the instrument as it is".

| Field | Type | Meaning |
|---|---|---|
| `span` | `Span` | The range. |
| `rbw_hz` | `Option<f64>`, Hz | Resolution bandwidth 10 to 1 000 000; the unit rounds up to 10, 30, 100, 300 ... 1 MHz. Setting it switches RBW auto off. |
| `vbw_hz` | `Option<f64>`, Hz | Video bandwidth 1 to 1 000 000; switches VBW auto off. |
| `ref_level_dbm` | `Option<f64>`, dBm | Reference level -100 to 30. |
| `attenuation_db` | `Option<u32>`, dB | Input attenuator 0 to 50; switches attenuation auto off. |
| `detector` | `Option<&'static str>` | SCPI name: `POSitive`, `NEGative`, `SAMPle`, `AVERage` or `NORMal`. |
| `average` | `u32` | Sweeps averaged, 1 to 999; 1 is a single sweep. |
| `continuous_after` | `bool` | Put the unit back into continuous sweep afterwards (default true). |

#### `bench_mcp_sa_trace` - `Trace`

[@ANCHOR: bench_mcp_sa_trace]

One trace and the settings the unit itself reported for it (read back after they were set), in `src/sa/sweep.rs`. Created by `run_sweep`, consumed by `find_peaks`, `trace_summary`, `write_csv` and the S21 code; stored (cloned) inside a `Reference`.

| Field | Type | Meaning |
|---|---|---|
| `start_hz`, `stop_hz` | `f64`, Hz | The range the unit holds. |
| `dbm` | `Vec<f64>`, dBm | The 751 points, evenly spaced from `start_hz` to `stop_hz` inclusive. |
| `rbw_hz`, `vbw_hz` | `f64`, Hz | Bandwidths as reported. |
| `ref_level_dbm` | `f64`, dBm | Reference level as reported (0.0 in a trace parsed from a reference file, which does not record it). |
| `attenuation_db` | `f64`, dB | Input attenuation as reported. |
| `detector` | `String` | Detector name as reported. |
| `sweep_time_s` | `f64`, s | One sweep's duration as reported (0.0 in a parsed reference). |
| `average` | `u32` | Sweeps averaged. |

#### `bench_mcp_sa_peak` - `Peak`

[@ANCHOR: bench_mcp_sa_peak]

A spectral peak: `freq_hz` (`f64`, Hz, from the bin position) and `level_dbm` (`f64`, dBm). Created by `find_peaks`, serialised by `trace_summary` as `{"freq_hz", "level_dbm"}`.

#### `bench_mcp_sa_reference` - `Reference`

[@ANCHOR: bench_mcp_sa_reference]

A through reference, in `src/sa/tg.rs`: the trace taken with the TG cabled straight to the RF input.

| Field | Type | Meaning |
|---|---|---|
| `tg_level_dbm` | `f64`, dBm | The generator level the through was taken at. |
| `trace` | `Trace` | The through trace with the settings it was taken with. |

Lives in the process-wide `refs()` map under `"<instrument>:<name>"` until the server exits, and on disk as the CSV that `bench_sa_tg_normalize` writes: `# key=value` lines (`tool`, `tg_level_dbm`, `start_hz`, `stop_hz`, `rbw_hz`, `vbw_hz`, `attenuation_db`, `detector`, `average_count`), the header `freq_hz,level_dbm`, then 751 rows. The key set is the file format; a file lacking a key, or with a row count other than 751, is refused by `parse_reference`. Created by `normalize` and `parse_reference`; read by `s21`, `check_same` and `with_defaults`. The map is behind a `Mutex`, held only for an insert or a clone.

### SA mod.rs functions

<a id="sa-modrs-functions"></a>

#### `bench_mcp_sa_parse_config` - `parse_config(c: &Value) -> Result<Config, String>`

[@ANCHOR: bench_mcp_sa_parse_config]

Reads the optional `max_tg_dbm` from an instrument's JSON entry when the config file is loaded.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&Value` | The instrument's JSON object. |

Returns the `Config` (default when the key is absent). Errors: not a number, or not finite and inside -20 to 0 dBm. Pure; called once per instrument at start-up.

#### `bench_mcp_sa_instrument` - `instrument(insts, args) -> Result<&Instrument, String>`

[@ANCHOR: bench_mcp_sa_instrument]

Picks the analyzer for a call: the one named by `args.instrument`, or the only configured analyzer (`Kind::Sa`). Errors: no such name, none configured, or several and no name (the message lists them). Pure.

#### `bench_mcp_sa_tool_defs` - `tool_defs(insts: &[Instrument]) -> Vec<Tool>`

[@ANCHOR: bench_mcp_sa_tool_defs]

The tool list for the MCP: every `SPECS` entry whose tier is at or below the tier of at least one configured analyzer, with the description prefixed by the tier. Called by the server when a client lists tools (`bench_mcp_main_all_tool_defs`). Pure; panics only if a `props` text is not JSON, which the tests rule out.

#### `bench_mcp_sa_flag` - `flag(args, key, default) -> Result<bool, String>`

[@ANCHOR: bench_mcp_sa_flag]

A boolean argument. Absent or null gives `default`; `true`/`false` gives itself; anything else (a string, a number) is an error naming the key. No coercion.

#### `bench_mcp_sa_number` - `number(args, key, lo, hi, why) -> Result<Option<f64>, String>`

[@ANCHOR: bench_mcp_sa_number]

A numeric argument that may be absent.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The tool's arguments. |
| `key` | `&str` | The argument name. |
| `lo`, `hi` | `f64` | Inclusive range. |
| `why` | `&str` | Where the limit comes from, used in the error text. |

Returns `Ok(None)` when absent or null, `Ok(Some(x))` for a finite number inside the range. Errors: not a number; NaN, infinite or outside the range (the message names the range and `why`). Pure.

#### `bench_mcp_sa_next_step` - `next_step(what: &str) -> String`

[@ANCHOR: bench_mcp_sa_next_step]

Ends an I/O error message with the ordered next steps (check the LAN cable, host and port 5025; call `bench_sa_status`; power-cycle the unit; do not use USB). Pure.

#### `bench_mcp_sa_ask` - `ask(l: &mut Link, cmd, tries, secs) -> Result<String, String>`

[@ANCHOR: bench_mcp_sa_ask]

Sends a query and returns its reply, trying up to `tries` times in all with a `secs` second reply timeout each (the timeout is set on the link before each try). An empty reply counts as a failure. **Only for reads**: repeating a read changes nothing, and a command that changes the instrument is never passed through it. Blocking for up to `tries * secs` seconds. Error: the last failure text plus the next steps. The link is exclusively borrowed.

#### `bench_mcp_sa_ask_f64` - `ask_f64(l: &mut Link, cmd: &str) -> Result<f64, String>`

[@ANCHOR: bench_mcp_sa_ask_f64]

`ask` with 3 tries of 5 s, then the reply must parse as a finite number (SCPI scientific notation such as `+2.1E+09` is accepted). Error: no reply, or a reply that is not a finite number (quoted in the message).

#### `bench_mcp_sa_on_off` - `on_off(cmd: &str, reply: &str) -> Result<bool, String>`

[@ANCHOR: bench_mcp_sa_on_off]

Reads a state reply: `1`, `ON` or `1.000000` is true; `0`, `OFF` or `0.000000` is false (case and surrounding space ignored). Anything else is an error, not a guess; `cmd` is used for the message. Pure. Used to verify the generator's state, so a garbled reply can never be taken as "off".

#### `bench_mcp_sa_model_max_hz` - `model_max_hz(model: &str) -> Option<f64>`

[@ANCHOR: bench_mcp_sa_model_max_hz]

The top frequency in Hz from a model name: `SSA3021X` is 2.1e9 (the two digits after `SSA30` times 100 MHz), `SSA3032X` 3.2e9, `SSA3015X` 1.5e9. `None` when the name does not carry two positive digits after `SSA30`. Pure.

#### `bench_mcp_sa_open` - `open(connect, inst) -> Result<(Link, Ident), String>`

[@ANCHOR: bench_mcp_sa_open]

Connects, asks `*IDN?` and returns the link and the `Ident`. If the reply has fewer than four fields or the model is not an `SSA3...` with a known top frequency, it returns an error that names the host and port and says nothing else was sent. Blocking. Every tool calls it first, so a mistyped address never receives a command.

#### `bench_mcp_sa_options` - `options(l: &mut Link) -> Result<Vec<String>, String>`

[@ANCHOR: bench_mcp_sa_options]

The installed options from `:SYSTem:OPTions?`, split at spaces and commas (for example `Meas EMI TG CAT`). Used to check that `TG` is present; the other names are only reported, never required.

#### `bench_mcp_sa_status` - `status(connect, inst) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_status]

`bench_sa_status`: identity, options, the generator (`state`, `level_dbm`, `max_tg_dbm`, `level_offset_db`; `not installed` when `TG` is not listed) and the sweep settings (`start_hz`, `stop_hz`, `rbw_hz`, `vbw_hz`, `ref_level_dbm`, `attenuation_db`, `detector`, `sweep_time_s`, `continuous`, `points`), all read back. Reads only; it never changes the instrument or the generator.

#### `bench_mcp_sa_tcp` - `tcp(inst: &Instrument) -> Result<Link, String>`

[@ANCHOR: bench_mcp_sa_tcp]

The production `Connector`: opens `Link::tcp(host, port)`. Error: connection refused or timed out (the text of the link error).

#### `bench_mcp_sa_call` - `call(insts, name, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_call]

The entry point used by the server's dispatcher: `call_with` over `tcp`.

#### `bench_mcp_sa_call_with` - `call_with(connect, insts, name, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_call_with]

Looks the tool up in `SPECS`, picks the instrument, refuses a tool above the instrument's tier (before any connection), and dispatches by name to `status`, `sweep::sweep`, `sweep::marker`, `sweep::screenshot`, `tg::tg_on`, `tg::tg_off`, `tg::tg_level`, `tg::normalize` or `tg::s21`. Errors: unknown tool, no or ambiguous instrument, tier too low, or the tool's own error. Blocking for the tool's duration.

### SA sweep.rs functions

<a id="sa-sweeprs-functions"></a>

#### `bench_mcp_sa_sweep_args` - `sweep_args(args: &Value, max_hz: f64) -> Result<SweepReq, String>`

[@ANCHOR: bench_mcp_sa_sweep_args]

Checks the sweep arguments before anything is sent to the unit.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The tool arguments: `start_hz`+`stop_hz` or `center_hz`+`span_hz` (not a mixture), `rbw_hz`, `vbw_hz`, `ref_level_dbm`, `attenuation_db` (whole number), `detector` (`positive`, `negative`, `sample`, `average`, `normal`), `average_count` (whole number), `continuous_after`. |
| `max_hz` | `f64`, Hz | The model's top frequency (`Ident.max_hz`). |

Returns a `SweepReq`. Errors: neither or both frequency forms, a range outside 0..`max_hz` or narrower than 100 Hz, an unknown detector, a fractional count, a value outside the unit's ranges. Pure.

#### `bench_mcp_sa_parse_trace` - `parse_trace(reply: &str) -> Result<Vec<f64>, String>`

[@ANCHOR: bench_mcp_sa_parse_trace]

Parses the comma-separated ASCII trace of `:TRACe:DATA? 1`. Empty fields are skipped; every other field must be a finite number; at least two points. Error names the bad field or the count.

#### `bench_mcp_sa_bin_hz` - `bin_hz(t_start, t_stop, n, i) -> f64`

[@ANCHOR: bench_mcp_sa_bin_hz]

The frequency of point `i` (0-based) of `n` evenly spaced from `t_start` to `t_stop` inclusive: `t_start + (t_stop - t_start) * i / (n - 1)`. `n` must be at least 2. Pure.

#### `bench_mcp_sa_find_peaks` - `find_peaks(t: &Trace, count: usize, threshold_db: f64) -> Vec<Peak>`

[@ANCHOR: bench_mcp_sa_find_peaks]

The local maxima of `t.dbm` that are at least `threshold_db` above the trace median, strongest first, at most `count` of them, none within two points of a stronger one already taken. `count` 0 gives none. Pure; O(n log n).

#### `bench_mcp_sa_new_path` - `new_path(out: &Path, stem: &str, ext: &str) -> PathBuf`

[@ANCHOR: bench_mcp_sa_new_path]

`<out>/<stem>_<milliseconds since the epoch>_<counter>.<ext>`; the process-wide atomic counter makes two calls in the same millisecond differ. Does not touch the disk.

#### `bench_mcp_sa_write_csv` - `write_csv(out, stem, comments, header, start, stop, cols) -> Result<PathBuf, String>`

[@ANCHOR: bench_mcp_sa_write_csv]

Writes a CSV into `out` (created if missing): `# key=value` comment lines, the `header` line, then one row per point with the frequency (from `bin_hz`, one decimal) and one column per entry of `cols` (three decimals). Errors: no columns, fewer than two points, columns of unequal length, or a failed write. Returns the new path.

#### `bench_mcp_sa_run_sweep` - `run_sweep(l: &mut Link, req: &SweepReq) -> Result<Trace, String>`

[@ANCHOR: bench_mcp_sa_run_sweep]

Runs one measurement (see `sweep_inner`) and then, on every path including failure, puts the unit back into continuous sweep when `req.continuous_after`. Blocking for the sweep time times the average count (at most 120 s) plus the readbacks. Does not touch the tracking generator.

#### `bench_mcp_sa_sweep_inner` - `sweep_inner(l, req) -> Result<Trace, String>`

[@ANCHOR: bench_mcp_sa_sweep_inner]

Continuous sweep off, dBm, logarithmic scale, ASCII trace format; start, stop, start (so the unit's clamping cannot leave the wrong end); RBW, VBW, attenuation (before the reference level: the unit moves the reference level when the attenuator is set), reference level, detector, averaging; then reads everything back. Errors: the range the unit holds differs from the request by more than 1 Hz (it clamps); the sweep would take more than 120 s; the reference level or attenuation read back differs by more than 0.05 from the request ("coupled": choose a value that fits the other settings); `*OPC?` does not answer within twice the sweep time plus ten seconds; the trace is not exactly 751 points. In every error case no sweep was started unless the error is the `*OPC?` timeout.

#### `bench_mcp_sa_trace_summary` - `trace_summary(t: &Trace, peaks: &[Peak]) -> Value`

[@ANCHOR: bench_mcp_sa_trace_summary]

The JSON summary of a trace: points, start, stop, bin width, the unit's own settings, `min_dbm`, `median_dbm`, `max_dbm` and the peaks. Pure.

#### `bench_mcp_sa_sweep` - `sweep(connect, inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_sweep]

`bench_sa_sweep`. Arguments: those of `sweep_args`, plus `peaks` (0 to 50, default 5), `peak_threshold_db` (0 to 200, default 10) and `tg_off_after` (default false). Opens the unit, runs the sweep, reads the generator state, switches the generator off and verifies it when `tg_off_after` (also when the sweep failed; an off failure is the error returned), writes `sa_sweep_*.csv` (`freq_hz,level_dbm`) into `BENCH_MCP_OUT` and returns the summary with `file` and `tracking_generator`. Without `tg_off_after` the generator is reported and left as it is.

#### `bench_mcp_sa_marker` - `marker(connect, inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_marker]

`bench_sa_marker`. Arguments: `marker` (1 to 4, default 1), `search` (`max`, `next`, `left`, `right`) or `freq_hz` (0 to the model's top), not both. Switches the marker on, in position mode, on trace 1, runs the search or places it, and returns `marker`, `trace`, `freq_hz` and `level_dbm` as read. Works on the trace the unit currently holds.

#### `bench_mcp_sa_screenshot` - `screenshot(connect, inst) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_screenshot]

`bench_sa_screenshot`. Reads `:HCOPy:SDUMp:DATA?` as raw bytes (the answer has no length header and no line end, so the read ends when the unit has been quiet 1.5 s, at most 40 s), checks the BMP header's size against what arrived and saves `sa_screen_*.bmp`. Returns `file`, `bytes`, `width`, `height` and `format`. Error: not a BMP, or fewer bytes than the header says (call again).

#### `bench_mcp_sa_bmp_header` - `bmp_header(raw: &[u8]) -> Result<(u32, u32, usize), String>`

[@ANCHOR: bench_mcp_sa_bmp_header]

Width, height and total size from a BMP header: at least 54 bytes, starting `BM`, size at least 54, width and height 1 to 4096. Error otherwise. Pure.

### SA tg.rs functions

<a id="sa-tgrs-functions"></a>

#### `bench_mcp_sa_tg_state` - `tg_state(l: &mut Link) -> Result<(bool, f64), String>`

[@ANCHOR: bench_mcp_sa_tg_state]

The generator's output state (`:OUTPut:STATe?`) and level in dBm (`:SOURce:POWer?`) as the unit reports them. Reads only; a state reply that is not clearly on or off is an error.

#### `bench_mcp_sa_ensure_tg_off` - `ensure_tg_off(l: &mut Link) -> Result<(), String>`

[@ANCHOR: bench_mcp_sa_ensure_tg_off]

Sends `:OUTPut:STATe OFF` and reads the state back, up to 3 times; `Ok` only when the unit itself says off. Otherwise the error begins `SAFE STATE NOT CONFIRMED` and gives the manual remedy. Safe to call at any time and idempotent; this is the only place that switches the generator off, and it is attempted on every path that switched it on.

#### `bench_mcp_sa_finish` - `finish<T>(work: Result<T, String>, off: Option<Result<(), String>>) -> Result<T, String>`

[@ANCHOR: bench_mcp_sa_finish]

Combines a piece of work with the switch-off that followed it (`None`: none asked for). Work and switch-off fine: the work's value. Work failed, off fine: the work's error. Work fine, off failed: the off error. Both failed: `"<work error>; <off error>"`. A failed switch-off is never lost. Pure.

#### `bench_mcp_sa_level_arg` - `level_arg(inst, args) -> Result<Option<f64>, String>`

[@ANCHOR: bench_mcp_sa_level_arg]

`tg_level_dbm` checked against -20 dBm and the lower of 0 dBm and the instrument's `max_tg_dbm`; `None` when absent. Error: not a number or outside the range (names the range). Pure.

#### `bench_mcp_sa_require_tg` - `require_tg(l: &mut Link) -> Result<(), String>`

[@ANCHOR: bench_mcp_sa_require_tg]

Refuses unless `:SYSTem:OPTions?` lists `TG`. This is the only option the driver depends on besides the base analyzer.

#### `bench_mcp_sa_set_level` - `set_level(l: &mut Link, dbm: f64) -> Result<(), String>`

[@ANCHOR: bench_mcp_sa_set_level]

Sets `:SOURce:POWer`, reads it back and fails when the difference exceeds 0.05 dB. Does not change whether the output is on.

#### `bench_mcp_sa_switch_on` - `switch_on(l, inst, level: Option<f64>) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_switch_on]

Sets and verifies `level` (or, when `None`, checks that the level the unit already holds is not above `max_tg_dbm`), sends `:OUTPut:STATe ON` and requires the unit to report on. **Any failure switches the output off again** (through `ensure_tg_off`) before the error is returned. Returns `{"tracking_generator": "on", "level_dbm"}`. The level is always set before the output goes on.

#### `bench_mcp_sa_tg_on` - `tg_on(connect, inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_tg_on]

`bench_sa_tg_on`: `tg_level_dbm` (optional, -20 to the cap). Checks the argument, opens the unit, requires `TG`, calls `switch_on`. The output stays on until `bench_sa_tg_off` or a tool with `tg_off_after`.

#### `bench_mcp_sa_tg_off` - `tg_off(connect, inst) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_tg_off]

`bench_sa_tg_off`: `ensure_tg_off`, then the level; returns `{"tracking_generator": "off", "verified": true, "level_dbm"}`. Does not require `TG` to be listed.

#### `bench_mcp_sa_tg_level` - `tg_level(connect, inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_tg_level]

`bench_sa_tg_level`: `tg_level_dbm` (required). Sets and verifies the level without changing on or off; returns the state and level as read back.

#### `bench_mcp_sa_refs` - `refs() -> &'static Mutex<HashMap<String, Reference>>`

[@ANCHOR: bench_mcp_sa_refs]

The process-wide store of through references, created on first use and kept until the server exits. Key `"<instrument>:<name>"`. The lock is held only for an insert or a clone; it is never held across I/O.

#### `bench_mcp_sa_ref_name` - `ref_name(v: Option<&str>) -> Result<String, String>`

[@ANCHOR: bench_mcp_sa_ref_name]

A reference name: absent is `default`; otherwise 1 to 40 characters of letters, digits, `-` and `_`, so a name can never make a path. Error otherwise.

#### `bench_mcp_sa_reference_comments` - `comments(r: &Reference) -> Vec<(String, String)>`

[@ANCHOR: bench_mcp_sa_reference_comments]

The `# key=value` header lines of a reference CSV: `tool`, `tg_level_dbm`, `start_hz`, `stop_hz`, `rbw_hz`, `vbw_hz`, `attenuation_db`, `detector`, `average_count`. The keys are exactly those `parse_reference` reads and `check_same` compares.

#### `bench_mcp_sa_parse_reference` - `parse_reference(text: &str) -> Result<Reference, String>`

[@ANCHOR: bench_mcp_sa_parse_reference]

Reads a reference CSV (see `Reference`). Errors: a missing or non-numeric key, a data row that is not a finite number, or a count other than 751. For a file from an earlier session (`reference_file`); the file's text is read by `s21`, not here.

#### `bench_mcp_sa_check_same` - `check_same(r, t, tg_level_dbm) -> Result<(), String>`

[@ANCHOR: bench_mcp_sa_check_same]

Refuses a measurement taken with other settings than the reference: compares start, stop, RBW, VBW, attenuation, average count and TG level (relative tolerance 1e-6) and the detector name, and names the first that differs and what to do. The reference level is not compared (it does not change the measured dBm).

#### `bench_mcp_sa_with_defaults` - `with_defaults(args: &Value, r: &Reference) -> Value`

[@ANCHOR: bench_mcp_sa_with_defaults]

A copy of the arguments with the ones the caller left out filled from the reference: the range (only when the caller gave none of `start_hz`, `stop_hz`, `center_hz`, `span_hz`), RBW, VBW, attenuation, detector, average count and TG level. Explicit arguments win (and are then checked by `check_same`). Pure.

#### `bench_mcp_sa_tg_sweep` - `tg_sweep(l, inst, req, level, leave_on) -> Result<Trace, String>`

[@ANCHOR: bench_mcp_sa_tg_sweep]

Generator on at `level` (`switch_on`), one sweep (`run_sweep`) and, unless `leave_on`, the verified switch-off, which is attempted on every path, including a failed switch-on or sweep. Result combined by `finish`. The generator is on only for the duration of the call.

#### `bench_mcp_sa_normalize` - `normalize(connect, inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_normalize]

`bench_sa_tg_normalize`, step 1 of a through-normalised S21: the TG cabled straight to the RF input. Arguments: the sweep arguments, `name` (default `default`), `tg_level_dbm` (default -20), `leave_tg_on` (default false). Sweeps with the generator on, stores the `Reference` in memory under `<instrument>:<name>` and writes `sa_tg_reference_<name>_*.csv`. Returns the trace summary with `name`, `tg_level_dbm`, `file` and `tracking_generator`. Replaces an earlier reference of the same name.

#### `bench_mcp_sa_s21` - `s21(connect, inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_sa_s21]

`bench_sa_tg_s21`: a scalar transmission sweep of what is between the TG output and the RF input. Arguments: the sweep arguments, `tg_level_dbm`, `reference` (name from `normalize`) or `reference_file` (not both), `leave_tg_on`. With a reference, unset sweep arguments come from it (`with_defaults`), the settings are checked (`check_same`) and `s21_db` is the measured level minus the through, point by point; without one it is the measured level minus the TG level, and the answer says `uncalibrated: cable and generator flatness included`. Writes `sa_tg_s21_*.csv` (`freq_hz,s21_db,measured_dbm`) and returns the summary, `s21` (`relative_to`, `max_db` and `max_at_hz`, `min_db` and `min_at_hz`, `mean_db`), `tg_level_dbm`, `file` and `tracking_generator`. The generator is off again afterwards unless `leave_tg_on`, also when the settings check refused the comparison.

## RX-888 receiver (`rx888` driver)

The RX-888 MkII (Cypress FX3, 16-bit direct-sampling ADC, 0 to 30 MHz at 64.8 Msps) is run through ka9q-radio. Its `rx888_boot` loads the firmware, and its `radiod` (GPL-3.0, run unmodified as a separate process) runs the receiver on loopback multicast with TTL 0. The tools talk to `radiod` through the `radiod_client` crate (`daemons/radiod_client`). They are receive only, so none is tier `full`. Manual: `instruments/rx888`. Verified on the unit with no antenna (2026-11-05):
- boot to 5000 Mb/s;
- `radiod` at 64.8 Msps, measured 64.8 MS/s;
- overrange 0, RF AGC at 34 dB, IF power -52.4 dBFS;
- channel noise density about -159 to -161 dB/Hz;
- an 8192-bin spectrum with the floor at -122.7 dB per 5.9 kHz noise bandwidth;
- I/Q and audio captures with 0 lost.

The hardware check is `tests/hil_rx888.rs` (`--ignored`). A real-signal check is still owed.

### RX-888 anchor reference (ADR 0113)

Files: `src/rx888/mod.rs` (config, tool list, USB scan, boot, the `radiod` process, front end, dispatch), `src/rx888/radio.rs` (tune, spectrum, capture), `src/rx888/testkit.rs` (a fake `radiod` on loopback multicast, test only).

Every tool is a blocking call on the server's tool thread. A call that talks to `radiod` builds its own current-thread tokio runtime (`runtime`) and opens its own sockets, so calls share no socket state; the server's per-instrument lock serialises calls to one RX-888. Every channel a tool opens has a 5 s lifetime, renewed by its reads, and is removed before the tool returns.

#### `bench_mcp_rx888_config` - `Config` and `parse_config(c: &Value) -> Result<Config, String>`

[@ANCHOR: bench_mcp_rx888_config]

The `rx888` settings of an instrument entry. They are parsed when the config file is loaded, held in `Instrument.rx888`, and never change afterwards.

| Field (config key) | Type | Default | Meaning and check |
|---|---|---|---|
| `radiod` | `PathBuf` | `/usr/local/sbin/radiod` | the `radiod` binary; absolute |
| `driver_library` | `PathBuf` | `/usr/local/lib/ka9q-radio/rx888.so` | ka9q-radio's RX-888 driver, loaded by `radiod` with `library =`; absolute |
| `boot` | `PathBuf` | `/usr/local/sbin/rx888_boot` | the firmware loader; absolute |
| `firmware` | `PathBuf` | `/usr/local/share/ka9q-radio/W1EUJ_0.1.0_FX3.img` | the FX3 image; absolute (`rx888_boot` prefixes its own directory to a relative name) |
| `presets` | `PathBuf` | `/usr/local/share/ka9q-radio/presets.conf` | `radiod`'s presets file; absolute |
| `status_group`, `data_group` | `Ipv4Addr` | 239.88.8.1, 239.88.8.2 | multicast groups; must be multicast |
| `status_port`, `data_port` | `u16` | 5006, 5004 | 1 to 65535; status and data must differ in group or port |
| `ssrc_base` | `u32` | 88000 | first of the three SSRCs the tools use (tune, spectrum, capture); 1 to 4294967280 |

`parse_config` returns an error naming the key for any value that fails its check. An absent key keeps its default.

#### `bench_mcp_rx888_instrument` - `instrument(insts, args) -> Result<&Instrument, String>`

[@ANCHOR: bench_mcp_rx888_instrument]

The RX-888 a call is for. This is the one named in `instrument`, or the only one configured. An unknown name, no RX-888, or several RX-888s with no name are errors.

#### `bench_mcp_rx888_tool_defs` - `tool_defs(insts: &[Instrument]) -> Vec<Tool>`

[@ANCHOR: bench_mcp_rx888_tool_defs]

The `SPECS` entries listed for this config: a tool appears when an RX-888 is configured at or above its tier. At `read_only` that is `status` and `frontend`; at `read_config` all eight. Each description is prefixed with its tier.

#### `bench_mcp_rx888_number` - `number(args, key, default: Option<f64>, lo, hi) -> Result<f64, String>`

[@ANCHOR: bench_mcp_rx888_number]

A numeric argument. Absent or null gives `default`, and is an error when there is none. Present, it must be a finite number in `lo..=hi`; the error names the key and the range.

#### `bench_mcp_rx888_find_usb` - `find_usb(root: &Path) -> Vec<UsbSeen>`

[@ANCHOR: bench_mcp_rx888_find_usb]

Scans `root` (production: `/sys/bus/usb/devices`) for devices with `idVendor` 04b4 and `idProduct` 00f3 (unbooted) or 00f1 (booted), sorted by sysfs name. `UsbSeen` has these fields: `path` (sysfs name such as `2-3.2`), `product_id` (`00f3` or `00f1`), `speed_mbps` (`Option<u32>`, the kernel's `speed`: 480 is USB 2, 5000 is USB 3) and `product`. Read only; a missing or unreadable tree gives an empty list.

#### `bench_mcp_rx888_usb_json` - `usb_json(seen: &[UsbSeen]) -> Value`

[@ANCHOR: bench_mcp_rx888_usb_json]

The USB part of a status. `state` is `booted`, `unbooted (FX3 loader)...` or `not on the USB bus`. `devices` lists each device with `sysfs`, `usb_id`, `speed_mbps`, `superspeed` and `product`. Pure.

#### `bench_mcp_rx888_running` and `bench_mcp_rx888_running_pid` - the started `radiod`s

[@ANCHOR: bench_mcp_rx888_running] [@ANCHOR: bench_mcp_rx888_running_pid]

`running()` is a process-wide `Mutex<HashMap<instrument name, Running>>` holding each `radiod` this server started. `Running` holds the `Child`, the config path, the log path and the start time. `running_pid(name) -> Option<u32>` returns the child's pid if it is still alive. A child that has exited is reaped and removed. The mutex is held only for the lookup.

#### `bench_mcp_rx888_tail` - `tail(path: &Path, n: usize) -> Vec<String>`

[@ANCHOR: bench_mcp_rx888_tail]

The last `n` lines of a file, empty when it cannot be read. Used for `radiod` log tails in errors and in the stop result.

#### `bench_mcp_rx888_loopback_multicast` - `loopback_multicast(sys_class_net: &Path) -> bool`

[@ANCHOR: bench_mcp_rx888_loopback_multicast]

True when `<sys_class_net>/lo/flags` has `IFF_MULTICAST` (0x1000) set. Production passes `/sys/class/net`. `radiod` runs on `lo` only, so this must be on (`ip link set dev lo multicast on`, as root, until reboot).

#### `bench_mcp_rx888_radiod_config_text` - `radiod_config_text(cfg, description, sample_rate_hz, rf_gain_db: Option<f64>, rf_atten_db: Option<f64>, fft_threads) -> String`

[@ANCHOR: bench_mcp_rx888_radiod_config_text]

The `radiod` config: `[global]` with `hardware = rx888`, the two groups, `iface = lo`, `ttl = 0`, `mode = usb`, `advertise = no`, the presets file, `fft-threads` and `verbose = 1`, and `[rx888]` with `device`, `library` (the driver path), `description` (quotes and newlines replaced by spaces) and `samprate`. `gain =` and `att =` are written only when given. Pure.

#### `bench_mcp_rx888_runtime` - `runtime() -> Result<Runtime, String>`

[@ANCHOR: bench_mcp_rx888_runtime]

A current-thread tokio runtime with I/O and timers, built per call. Tool calls run under `spawn_blocking`, outside any runtime, so blocking on it is allowed.

#### `bench_mcp_rx888_control` - `control(cfg, rt) -> Result<Control, String>`

[@ANCHOR: bench_mcp_rx888_control]

Joins `radiod`'s status group on 127.0.0.1 with `radiod_client`'s default policy (10 kHz to 30 MHz, at most 8192 bins). The socket sends with TTL 0. The error says to check loopback multicast.

#### `bench_mcp_rx888_front_end` - `front_end(cfg, rt, wait: Duration) -> Result<ChannelStatus, String>`

[@ANCHOR: bench_mcp_rx888_front_end]

Polls the front end (SSRC 0, no channel created; the reply carries no tag) for up to `wait`. The error says `radiod` does not answer and names the next tools to call.

#### `bench_mcp_rx888_front_end_json` - `front_end_json(fe: &ChannelStatus) -> Value`

[@ANCHOR: bench_mcp_rx888_front_end_json]

The front-end fields as JSON: `input_sample_rate_hz`, `real_samples`, `ad_bits`, `ad_overrange_count`, `samples_since_overrange`, `rf_gain_db`, `rf_atten_db`, `rf_agc`, `if_power_dbfs`, `front_end_running`, `rf_level_cal_db`, `usable_low_hz`, `usable_high_hz` and `description`. Non-finite levels become null. While no channel exists the front end idles, so `if_power_dbfs` is null and `front_end_running` is false. Pure.

#### `bench_mcp_rx888_status` - `status(inst, usb_root) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_status]

`bench_rx888_status`: `usb` (`usb_json`), `radiod` (running, pid, config, log, uptime of the one this server started), `front_end` (`front_end_json`, or `answering: false` with the reason after 0.8 s), `loopback_multicast` and `groups`. Changes nothing.

#### `bench_mcp_rx888_frontend` - `frontend(inst) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_frontend]

`bench_rx888_frontend`: `front_end_json` of a front-end poll with 2 s to answer. Changes nothing.

#### `bench_mcp_rx888_boot` - `boot(inst, usb_root) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_boot]

`bench_rx888_boot`. It checks, in order:
1. this server runs no `radiod` for the instrument;
2. a booted device means nothing to do (`already_booted`);
3. an unbooted one must be present (else the error says to check the cable or power-cycle the hub port);
4. the firmware path must be a file.

It then runs `<boot> <firmware>`, waits up to 20 s for `04b4:00f1`, and returns `booted`, `sysfs`, `speed_mbps`, `superspeed`, a `note` about the link and the loader's output. Side effect: the device leaves the bus and re-enumerates; the firmware is loaded to RAM only. If it does not come back, the error tells the person to power-cycle its hub port, not to unbind the USB controller.

#### `bench_mcp_rx888_radiod_start` - `radiod_start(inst, args, usb_root) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_radiod_start]

`bench_rx888_radiod_start`.

| Argument | Type | Range | Default |
|---|---|---|---|
| `sample_rate_hz` | integer | 64800000 or 129600000 | 64800000 |
| `rf_gain_db` | number | -8 to 34 | omitted (driver default, RF AGC) |
| `rf_atten_db` | number | 0 to 31.5 in 0.5 steps | omitted (0) |
| `fft_threads` | integer | 1 to 8 | 2 |

Refused when:
- this server, or an earlier one (its pid file names a live `radiod` with this config), runs one;
- the device is not booted;
- loopback multicast is off;
- a configured file is missing.

It writes `rx888-<name>.conf`, `.log` and `.pid` in `BENCH_MCP_OUT` and starts `nice -n 10 <radiod> -v <conf>` as the server's user. It then waits up to 15 s for the front end and returns `started`, `pid`, `config`, `log` and `front_end`. If `radiod` exits or does not answer, it is stopped and the error carries the log tail.

#### `bench_mcp_rx888_is_our_radiod` - `is_our_radiod(pid: u32, radiod: &Path, config: &Path) -> bool`

[@ANCHOR: bench_mcp_rx888_is_our_radiod]

True when `/proc/<pid>/cmdline` has `radiod` as its first argument and `config` as one of its arguments. This keeps a stale pid file from naming another process. Read only.

#### `bench_mcp_rx888_stop_pid` - `stop_pid(inst) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_stop_pid]

Stops the instrument's `radiod`. That is the child this server holds; failing that, the pid in `rx888-<name>.pid` when `is_our_radiod` confirms it, and otherwise the stale pid file is removed and nothing is signalled. It sends `SIGTERM` (a `libc::kill` on that one pid only, never by name or pattern), waits up to 5 s, then sends `SIGKILL`. It removes the pid file and returns `stopped`, `pid`, `exit`, `sigkill_needed` and `log_tail`. Error when no `radiod` was started.

#### `bench_mcp_rx888_radiod_stop` - `radiod_stop(inst) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_radiod_stop]

`bench_rx888_radiod_stop`: `stop_pid`.

#### `bench_mcp_rx888_call` / `bench_mcp_rx888_call_with` - `call(insts, name, args)` and `call_with(usb_root, insts, name, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_call] [@ANCHOR: bench_mcp_rx888_call_with]

The dispatcher. It finds the tool in `SPECS` and the instrument, and refuses a tool above the instrument's tier before anything runs. `call` passes `/sys/bus/usb/devices` as the USB root.

#### `bench_mcp_rx888_ssrc` - `ssrc(cfg, which: u32) -> Result<Ssrc, String>`

[@ANCHOR: bench_mcp_rx888_ssrc]

The SSRC `ssrc_base + which`: 0 tune, 1 spectrum, 2 capture. Each call uses a fixed SSRC, so a repeated call reuses the same `radiod` channel slot.

#### `bench_mcp_rx888_mode_arg` - `mode_arg(args, default: Mode) -> Result<Mode, String>`

[@ANCHOR: bench_mcp_rx888_mode_arg]

`mode`: `usb`, `lsb`, `cw`, `am` or `fm` (`radiod_client` modes); absent gives `default`. I/Q is chosen by `kind`, not `mode`.

#### `bench_mcp_rx888_edges` - `edges(mode, width: f64) -> (f32, f32)`

[@ANCHOR: bench_mcp_rx888_edges]

Passband edges in Hz from the carrier for a width: USB `(50, 50+w)`, LSB `(-50-w, -50)`, CW `(600-w/2, 600+w/2)`, AM, FM and I/Q `(-w/2, w/2)`. These match radioctl's radiod channel. Pure.

#### `bench_mcp_rx888_ask` - `ask(rt, c, cmd, wait) -> Result<ChannelStatus, String>`

[@ANCHOR: bench_mcp_rx888_ask]

Sends one `radiod_client` command and waits up to `wait` for the status that echoes its tag. The client re-sends every 200 ms; commands are idempotent. Opening a channel uses 10 s, because the first channel after `radiod` starts waits for FFT planning; other commands use 1 s.

#### `bench_mcp_rx888_remove` - `remove(rt, c, s: Ssrc)`

[@ANCHOR: bench_mcp_rx888_remove]

Removes a channel (lifetime 1), best effort: an unacknowledged removal still expires by the 5 s lifetime.

#### `bench_mcp_rx888_snr_db` - `snr_db(baseband_db: f64, noise_in_band_db: f64) -> f64`

[@ANCHOR: bench_mcp_rx888_snr_db]

SNR from the channel's power (signal plus noise) and the noise in its passband: `10 log10(10^((P-N)/10) - 1)`. It returns -30 when the power is at or within 0.004 dB of the noise. Pure.

#### `bench_mcp_rx888_tune` - `tune(inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_tune]

`bench_rx888_tune`.

| Argument | Type | Range | Default |
|---|---|---|---|
| `freq_hz` | number | 10 000 to 30 000 000 | required |
| `mode` | string | usb, lsb, cw, am, fm | am |
| `bandwidth_hz` | number | 100 to 24 000 | the mode's own |
| `readings` | integer | 1 to 50 | 10 |

It opens the channel, sets the passband if one was given, waits 0.5 s, then takes `readings` polls 100 ms apart, each renewing the lifetime. The channel is removed even on error. Returns:
- the means of baseband power and noise density;
- the passband, the noise in it and `snr_db`;
- AGC, channel gain and output level;
- the input rate measured from the `INPUT_SAMPLES` counter over the readings, with `rate_interval_s` and `rate_resolution_fraction` (0.02 s divided by the interval: the counter advances in 20 ms blocks).

#### `bench_mcp_rx888_out_path` - `out_path(stem, ext) -> PathBuf`

[@ANCHOR: bench_mcp_rx888_out_path]

`<BENCH_MCP_OUT>/<stem>_<milliseconds since the epoch>.<ext>`. Does not create the file.

#### `bench_mcp_rx888_peaks` - `peaks(bins: &[(f64, f32)], floor: f64, above_db: f64, count: usize) -> Vec<(f64, f64)>`

[@ANCHOR: bench_mcp_rx888_peaks]

Local maxima (frequency, dB) at least `above_db` over `floor`, strongest first, at most `count`, none within 2 bins of a stronger one. A non-finite neighbour does not hide a peak. Pure.

#### `bench_mcp_rx888_spectrum` - `spectrum(inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_spectrum]

`bench_rx888_spectrum`.

| Argument | Type | Range | Default |
|---|---|---|---|
| `center_hz` | number | 0 to 64 800 000 | 16 200 000 |
| `span_hz` | number | 1000 to 129 600 000, and at most `max_spectrum_span_hz` of the front end | 32 400 000 |
| `bins` | integer | 64 to 16 000 | 1024 |
| `averaging` | integer | 1 to 64 | 10 |
| `peaks` | integer | 0 to 50 | 10 |

It reads the front end first and refuses a span the front end cannot fill: more than half its rate for real samples. The spectrum channel is asked twice (10 s each for FFT planning) and then removed. The bins, ascending, go to a CSV with a comment header. Returns:
- the CSV path and the geometry;
- the noise bandwidth and `fft_n`;
- floor (median), min and max;
- the zero-bin count;
- peaks at least 10 dB over the floor;
- the front end's overrange count and RF gain.

#### `bench_mcp_rx888_wav_header` - `wav_header(rate: u32, channels: u16, frames: u32) -> Vec<u8>`

[@ANCHOR: bench_mcp_rx888_wav_header]

The 44-byte RIFF/WAVE header for 16-bit little-endian PCM. Pure.

#### `bench_mcp_rx888_capture` - `capture(inst, args) -> Result<Value, String>`

[@ANCHOR: bench_mcp_rx888_capture]

`bench_rx888_capture`.

| Argument | Type | Range | Default |
|---|---|---|---|
| `freq_hz` | number | 10 000 to 30 000 000 | required |
| `kind` | string | iq, audio | iq |
| `mode` | string | usb, lsb, cw, am, fm (audio only) | am |
| `seconds` | number | 0.1 to 60 | 5 |
| `gain_db` | number | -20 to 160 (iq only) | chosen so the noise is near -30 dBFS |

It opens the channel; for I/Q it reads the baseband power and sets the fixed gain. It then joins the data group and collects this SSRC's packets of the announced payload type for up to `seconds + 3`, renewing the lifetime every second, and converts them from big- to little-endian. The result goes to `rx888_<name>_<kind>_<ms>.wav`, and the channel is removed. Returns the file, rate, channels, frames, seconds, RTP received/lost/out-of-order/skipped counts, the gain used, RMS in dBFS and peak counts. Error when no audio arrived.

## Procedure runner (`procedure` driver)

A program, written by the agent or saved earlier, that calls bench tools in loops runs next to the server, so a long sweep series, soak round or calibration does not pass through the agent's context step by step. Config entry: `{"driver": "procedure", "tier": "read_config", "dir": "/var/lib/bench/procedures", "max_runs": 2}` (more keys: `retention_days`, `keep_runs`, `python`, `grace_s`, `max_wall_s`, `max_memory_mb`, `wrapper`, `safe_state_psu_off`, `lock_wait_s`, `operator_timeout_s`); the entry's tier is the highest tier cap a run may have. Tools: `bench_proc_run`, `bench_proc_status`, `bench_proc_output`, `bench_proc_result`, `bench_proc_cancel`, `bench_proc_list`, `bench_proc_save`, `bench_proc_load`, `bench_proc_delete`, `bench_proc_input`. Every tool call a procedure makes goes through the server's normal dispatcher, so tiers, confirmations and attestations apply as for an agent's call. The first language is CPython 3 with numpy and scipy (`src/procedure/bench_runtime.py` is the program side). The limits and the audit hook are **not a sandbox**: they catch accidents and keep the tier model honest. See the manual page `concepts/procedures`.

### Checks the procedure runner makes (anchors)

* The procedure runner entry is read with defaults, bad values are refused, and the runner's tools are listed by tier, checked and dispatched by name. [@ANCHOR: proc_config] [@ANCHOR: proc_instrument] [@ANCHOR: proc_tool_defs] [@ANCHOR: proc_call] [@ANCHOR: proc_number_arg] [@ANCHOR: proc_flag_arg] [@ANCHOR: proc_run_params] [@ANCHOR: proc_backend_for] [@ANCHOR: proc_run_tool]
* A procedure's tools and tier cap: the tools it may call come from the same listing the clients see, the procedure tools themselves are never among them, and a tool above the cap is refused with the tier it needs. [@ANCHOR: proc_tier_name] [@ANCHOR: proc_tier_parse] [@ANCHOR: proc_tool_index] [@ANCHOR: proc_tool_allow] [@ANCHOR: proc_tool_index_json]
* Everything a run produces is kept in its directory: atomic JSON files, an append-only capped event list read by cursor, capped output streams read by offset, a run id that cannot escape the directory, and retention. [@ANCHOR: proc_store_now] [@ANCHOR: proc_store_root] [@ANCHOR: proc_store_write_json] [@ANCHOR: proc_store_read_json] [@ANCHOR: proc_store_new_run_dir] [@ANCHOR: proc_store_run_dir] [@ANCHOR: proc_store_event_open] [@ANCHOR: proc_store_event_append] [@ANCHOR: proc_store_event_count] [@ANCHOR: proc_store_event_dropped] [@ANCHOR: proc_store_read_events] [@ANCHOR: proc_store_append_stream] [@ANCHOR: proc_store_read_stream] [@ANCHOR: proc_store_purge] [@ANCHOR: proc_store_list_runs]
* A run has a supervisor that starts the program, forwards its tool calls through the dispatcher under the tier cap and the call budget, records events, enforces the wall-clock budget and a cancel request (grace time, then kill), and writes the final state and result with the failure report. [@ANCHOR: proc_run_registry] [@ANCHOR: proc_run_lookup] [@ANCHOR: proc_run_active_in] [@ANCHOR: proc_run_clip] [@ANCHOR: proc_run_is_active] [@ANCHOR: proc_run_event] [@ANCHOR: proc_run_persist] [@ANCHOR: proc_run_set_state] [@ANCHOR: proc_run_request_cancel] [@ANCHOR: proc_run_cancel_reason] [@ANCHOR: proc_run_exec_call] [@ANCHOR: proc_run_count_error] [@ANCHOR: proc_run_handle] [@ANCHOR: proc_run_supervise] [@ANCHOR: proc_run_finish] [@ANCHOR: proc_run_start]
* Status, output since a cursor, the result, cancel and the list read the managed directory, so they answer for live runs, finished runs and runs of an earlier server process. [@ANCHOR: proc_view_terminal] [@ANCHOR: proc_view_status] [@ANCHOR: proc_view_output] [@ANCHOR: proc_view_result] [@ANCHOR: proc_view_cancel] [@ANCHOR: proc_view_list]
* The CPython backend starts `python3 -I` with a stripped environment, a process group, resource limits and a parent-death signal, speaks the run protocol on pipes, and kills the whole group. [@ANCHOR: proc_cpython_env] [@ANCHOR: proc_cpython_user_tasks] [@ANCHOR: proc_cpython_limit] [@ANCHOR: proc_cpython_command_line] [@ANCHOR: proc_cpython_pump] [@ANCHOR: proc_cpython_check] [@ANCHOR: proc_cpython_start] [@ANCHOR: proc_cpython_no_limits] [@ANCHOR: proc_cpython_send] [@ANCHOR: proc_cpython_recv] [@ANCHOR: proc_cpython_kill] [@ANCHOR: proc_cpython_exit_status] [@ANCHOR: proc_cpython_pid] [@ANCHOR: proc_cpython_drop]
* The program side of the protocol: the wire, `bench.call`, `emit`, `log`, `step`, `sleep`, `checkpoint`, `cleanup`, `tools`, `deadline`, `result`, the program's captured output, the entry point and the way it ends. [@ANCHOR: proc_py_wire_init] [@ANCHOR: proc_py_wire_send] [@ANCHOR: proc_py_wire_request] [@ANCHOR: proc_py_wire_reader] [@ANCHOR: proc_py_check_cancel] [@ANCHOR: proc_py_jsonable] [@ANCHOR: proc_py_call] [@ANCHOR: proc_py_emit] [@ANCHOR: proc_py_log] [@ANCHOR: proc_py_step] [@ANCHOR: proc_py_sleep] [@ANCHOR: proc_py_checkpoint] [@ANCHOR: proc_py_cleanup] [@ANCHOR: proc_py_tools] [@ANCHOR: proc_py_deadline] [@ANCHOR: proc_py_result] [@ANCHOR: proc_py_output_init] [@ANCHOR: proc_py_output_write] [@ANCHOR: proc_py_output_flush] [@ANCHOR: proc_py_output_isatty] [@ANCHOR: proc_py_make_bench] [@ANCHOR: proc_py_run_cleanups] [@ANCHOR: proc_py_execute] [@ANCHOR: proc_py_check_source] [@ANCHOR: proc_py_main]
* The helpers `retry`, `expect`, `stats` and `series`, and the exceptions a procedure sees. [@ANCHOR: proc_py_retry] [@ANCHOR: proc_py_expect] [@ANCHOR: proc_py_stats] [@ANCHOR: proc_py_series] [@ANCHOR: proc_py_bench_error_init] [@ANCHOR: proc_py_failure_init]
* A failure is reported with the traceback, the local variables of the procedure's own frames (bounded) and the context given to `expect`; `trace` records the lines reached. [@ANCHOR: proc_py_short] [@ANCHOR: proc_py_frame_report] [@ANCHOR: proc_py_failure_report] [@ANCHOR: proc_py_tracer] [@ANCHOR: proc_py_flush_trace]
* The audit hook refuses subprocesses, sockets, device nodes, ctypes and writes outside the scratch directory, and says to use `bench.call`. IT IS NOT A SANDBOX. [@ANCHOR: proc_py_path_inside] [@ANCHOR: proc_py_open_refusal] [@ANCHOR: proc_py_make_hook]
* The safe-state routine: the tools that change an instrument have a row naming the tool that undoes it, a call is recorded before it is dispatched, and when a run ends other than by success the recorded undo calls are run through the dispatcher and their outcome is part of the result. [@ANCHOR: proc_safe_action_for] [@ANCHOR: proc_safe_record] [@ANCHOR: proc_safe_run]
* Instrument reservations between runs: a tool that changes an instrument reserves it with a lock file, reads share, several instruments are taken in sorted order all or none with a timeout, and a refusal names the run that holds the instrument. [@ANCHOR: proc_locks_instrument_of] [@ANCHOR: proc_locks_owner_text] [@ANCHOR: proc_locks_path] [@ANCHOR: proc_locks_release] [@ANCHOR: proc_locks_reserve] [@ANCHOR: proc_py_release] [@ANCHOR: proc_py_reserve] [@ANCHOR: proc_py_reserved] [@ANCHOR: proc_run_hold] [@ANCHOR: proc_run_reserve_op]
* A restart: runs left active by a server process that no longer exists are marked lost and their instruments are put in the safe state; a shutdown cancels every live run and waits for its cleanup and safe state. [@ANCHOR: proc_run_recover] [@ANCHOR: proc_run_recover_now] [@ANCHOR: proc_run_shutdown] [@ANCHOR: proc_run_start_ticks] [@ANCHOR: proc_run_stop_all]
* The program's handle on its pipe. [@ANCHOR: proc_py_wire]
* Saved procedures: a name that cannot make a path, versions (the last 20 kept), the declared tier checked against the tools the source names, load, delete and list. [@ANCHOR: proc_saved_delete] [@ANCHOR: proc_saved_dir_of] [@ANCHOR: proc_saved_list] [@ANCHOR: proc_saved_load] [@ANCHOR: proc_saved_save] [@ANCHOR: proc_saved_valid_name]
* The operator prompt: a run waits for an answer through `bench_proc_input` in the state `waiting_operator`, the wait is left out of the wall-clock budget, has its own limit, and ends with a cancel. [@ANCHOR: proc_py_ask_operator] [@ANCHOR: proc_run_answer] [@ANCHOR: proc_run_clear_ask] [@ANCHOR: proc_run_paused] [@ANCHOR: proc_run_poll_ask]
* The shipped example procedures (sweep series with retries and a power cycle, soak round against a baseline, calibration with operator steps) are program text with reserved `example-` names, parameterised by their arguments, identical to their manual pages, and run in the tests against a mock bench whose fake devices share a simulated world. [@ANCHOR: proc_examples_find]

## Checks the functions of every driver make (anchors)

One line per group of functions; each anchor is also on its functions in `src/`.

* An alert's severity word is parsed and sets the tone pattern (one soft beep, two beeps, or a fast two-tone siren) and the share of the volume limit it uses. [@ANCHOR: bench_mcp_audio_alert_severity_parse] [@ANCHOR: bench_mcp_audio_alert_severity_word] [@ANCHOR: bench_mcp_audio_alert_severity_volume_share] [@ANCHOR: bench_mcp_audio_alert_pattern]
* Audio analysis gives level in dB, RMS and statistics, a Hann-windowed Welch spectrum by radix-2 FFT (a power-of-two length), spectrum peaks, the median noise floor, one tone's amplitude and the similarity of two spectra. [@ANCHOR: bench_mcp_audio_analysis_db] [@ANCHOR: bench_mcp_audio_analysis_rms] [@ANCHOR: bench_mcp_audio_analysis_stats] [@ANCHOR: bench_mcp_audio_analysis_fft] [@ANCHOR: bench_mcp_audio_analysis_spectrum_range] [@ANCHOR: bench_mcp_audio_analysis_spectrum] [@ANCHOR: bench_mcp_audio_analysis_peaks] [@ANCHOR: bench_mcp_audio_analysis_median_db] [@ANCHOR: bench_mcp_audio_analysis_tone_amp] [@ANCHOR: bench_mcp_audio_analysis_similarity]
* Audio devices are read from `pw-dump` and `/proc/asound` with their transport, rates and formats, and are resolved by alias, node name or a unique fragment; a reserved device is never returned. [@ANCHOR: bench_mcp_audio_devices_dir_word] [@ANCHOR: bench_mcp_audio_devices_device_transport] [@ANCHOR: bench_mcp_audio_devices_rates_of] [@ANCHOR: bench_mcp_audio_devices_formats_of] [@ANCHOR: bench_mcp_audio_devices_list] [@ANCHOR: bench_mcp_audio_devices_resolve]
* Audio discovery fingerprints the candidate devices, stores and loads its result, and reports whether the result is stale and what `auto` selects now. [@ANCHOR: bench_mcp_audio_discover_fingerprint] [@ANCHOR: bench_mcp_audio_discover_load] [@ANCHOR: bench_mcp_audio_discover_save] [@ANCHOR: bench_mcp_audio_discover_report]
* A recording is started on one channel of a node (or on the monitor of an output), waits for its samples, and is shut down. [@ANCHOR: bench_mcp_audio_engine_recorder_start] [@ANCHOR: bench_mcp_audio_engine_recorder_wait_samples] [@ANCHOR: bench_mcp_audio_engine_recorder_shutdown]
* A playback is started on a node at a stream volume, waited for until a limit after which it is killed, and stopped when dropped. [@ANCHOR: bench_mcp_audio_engine_playing_start] [@ANCHOR: bench_mcp_audio_engine_playing_wait] [@ANCHOR: bench_mcp_audio_engine_playing_drop]
* Event detection measures the level of a tone, or of the whole signal, in short windows and takes percentiles of those levels. [@ANCHOR: bench_mcp_audio_events_tone_levels] [@ANCHOR: bench_mcp_audio_events_levels_for] [@ANCHOR: bench_mcp_audio_events_percentile]
* The tones heard in a recording are grouped into tracks. [@ANCHOR: bench_mcp_audio_heard_tracks]
* A device's volume and mute are read and set. [@ANCHOR: bench_mcp_audio_levels_get] [@ANCHOR: bench_mcp_audio_levels_set_volume] [@ANCHOR: bench_mcp_audio_levels_set_mute]
* An audio helper process is run and its outcome returned. [@ANCHOR: bench_mcp_audio_process_run]
* Quiet hours say whether a time of day falls inside them. [@ANCHOR: bench_mcp_audio_quiethours_contains]
* A device's alias is looked up and its volume cap is never above the global cap. [@ANCHOR: bench_mcp_audio_config_alias_of] [@ANCHOR: bench_mcp_audio_config_cap_for]
* Quiet-hour times are read as HH:MM. [@ANCHOR: bench_mcp_audio_hhmm]
* The ports of an audio device are listed. [@ANCHOR: bench_mcp_audio_ports]
* The local minutes since midnight are taken for the quiet-hours test. [@ANCHOR: bench_mcp_audio_local_minute_of_day]
* The audio instrument is built from its configuration entry. [@ANCHOR: bench_mcp_audio_instrument]
* The device-listing tool returns the live devices. [@ANCHOR: bench_mcp_audio_list_devices]
* An audio tool call is dispatched by name to its tool. [@ANCHOR: bench_mcp_audio_call_with]
* The volume guard saves the volume and mute before a sound is made and puts them back, once, when released or dropped. [@ANCHOR: bench_mcp_audio_safety_volumeguard_restore] [@ANCHOR: bench_mcp_audio_safety_volumeguard_drop]
* Speech is synthesized with espeak-ng or Piper from a request whose voice and language codes are limited to the characters such codes use, and is played on the chosen device. [@ANCHOR: bench_mcp_audio_speak_engine_name] [@ANCHOR: bench_mcp_audio_speak_safe_code] [@ANCHOR: bench_mcp_audio_speak_spd_args] [@ANCHOR: bench_mcp_audio_speak_run_with_stdin] [@ANCHOR: bench_mcp_audio_speak_request] [@ANCHOR: bench_mcp_audio_speak_synthesize] [@ANCHOR: bench_mcp_audio_speak_say] [@ANCHOR: bench_mcp_audio_speak_synthesizers]
* Captured audio is stored in an owner-only directory under a timestamped file name made of letters, digits and dashes. [@ANCHOR: bench_mcp_audio_store_timestamp] [@ANCHOR: bench_mcp_audio_store_slug] [@ANCHOR: bench_mcp_audio_store_new_path]
* A tone (sine, sweep, two tone or noise) is read from tool arguments, with its level limited to the maximum dBFS. [@ANCHOR: bench_mcp_audio_tones_hz] [@ANCHOR: bench_mcp_audio_tones_parse] [@ANCHOR: bench_mcp_audio_tones_level]
* The audio tools bound their numeric arguments, take their devices and sources, meter levels, and play under the volume guard. [@ANCHOR: bench_mcp_audio_tools_bounded] [@ANCHOR: bench_mcp_audio_tools_take] [@ANCHOR: bench_mcp_audio_tools_meter] [@ANCHOR: bench_mcp_audio_tools_source] [@ANCHOR: bench_mcp_audio_tools_play_guarded] [@ANCHOR: bench_mcp_audio_tools_levels_get]
* The audio verification tool bounds its arguments, states its expectations, and runs a configured action and waits for it. [@ANCHOR: bench_mcp_audio_verify_bounded] [@ANCHOR: bench_mcp_audio_verify_expectations] [@ANCHOR: bench_mcp_audio_verify_run_action]
* WAV files are written as 16-bit PCM with samples beyond full scale clipped, and a channel set can be averaged into one. [@ANCHOR: bench_mcp_audio_wav_wav_mono] [@ANCHOR: bench_mcp_audio_wav_encode]
* The template instrument shows the checks a driver makes: a boolean argument is exact, ON/OFF is exact, and a pulse returns the unit to its safe state. [@ANCHOR: bench_mcp_example_instrument] [@ANCHOR: bench_mcp_example_flag] [@ANCHOR: bench_mcp_example_on_off] [@ANCHOR: bench_mcp_example_status] [@ANCHOR: bench_mcp_example_measure] [@ANCHOR: bench_mcp_example_safe_state] [@ANCHOR: bench_mcp_example_run_pulse] [@ANCHOR: bench_mcp_example_call_with]
* The tool definitions the server lists are built from each instrument's tools. [@ANCHOR: bench_mcp_main_tool_defs]
* The manual is parsed from its embedded pages (front matter, lists, page paths, parent index), rendered with its generated argument tables, searched by lower-case tokens with an excerpt and a section, truncated by characters, and served as tools and as MCP resources; a missing page is a clear error. [@ANCHOR: bench_mcp_manual_tool_registry] [@ANCHOR: bench_mcp_manual_manual_build] [@ANCHOR: bench_mcp_manual_manual_render_body] [@ANCHOR: bench_mcp_manual_searchindex_build] [@ANCHOR: bench_mcp_manual_tokens] [@ANCHOR: bench_mcp_manual_manual_up_of] [@ANCHOR: bench_mcp_manual_not_found] [@ANCHOR: bench_mcp_manual_section_of] [@ANCHOR: bench_mcp_manual_str_arg] [@ANCHOR: bench_mcp_manual_truncate_chars] [@ANCHOR: bench_mcp_manual_uint_arg] [@ANCHOR: bench_mcp_manual_searchindex_score] [@ANCHOR: bench_mcp_manual_call] [@ANCHOR: bench_mcp_manual_excerpt] [@ANCHOR: bench_mcp_manual_releases] [@ANCHOR: bench_mcp_manual_split_front_matter] [@ANCHOR: bench_mcp_manual_parse_list] [@ANCHOR: bench_mcp_manual_path_of_file] [@ANCHOR: bench_mcp_manual_type_of] [@ANCHOR: bench_mcp_manual_notes_of] [@ANCHOR: bench_mcp_manual_notes_for] [@ANCHOR: bench_mcp_manual_clip_around] [@ANCHOR: bench_mcp_manual_page_text_for_resource] [@ANCHOR: bench_mcp_manual_read_resource]
* The scope's spectrum is computed on the host from the channel's waveform. [@ANCHOR: bench_mcp_scope_spectrum]
* The host spectrum applies a named window, an in-place radix-2 FFT and a strongest-peaks search. [@ANCHOR: bench_mcp_spectrum_window_parse] [@ANCHOR: bench_mcp_spectrum_window_weight] [@ANCHOR: bench_mcp_spectrum_fft] [@ANCHOR: bench_mcp_spectrum_peaks]
* USB bandwidth is estimated from the periodic endpoints of every alternate setting, with the protocol overhead of each packet, per bus domain and against its capacity. [@ANCHOR: bench_mcp_usb_hub_bandwidth_overhead] [@ANCHOR: bench_mcp_usb_hub_bandwidth_endpoint_load] [@ANCHOR: bench_mcp_usb_hub_bandwidth_interval_us] [@ANCHOR: bench_mcp_usb_hub_bandwidth_potential_streams] [@ANCHOR: bench_mcp_usb_hub_bandwidth_potential_load] [@ANCHOR: bench_mcp_usb_hub_bandwidth_domain_of] [@ANCHOR: bench_mcp_usb_hub_bandwidth_capacity]
* The missing-device diagnosis names the port the device would be on and runs the checks that apply. [@ANCHOR: bench_mcp_usb_hub_diagnose_rank] [@ANCHOR: bench_mcp_usb_hub_diagnose_missing_target] [@ANCHOR: bench_mcp_usb_hub_diagnose_finish] [@ANCHOR: bench_mcp_usb_hub_diagnose_missing] [@ANCHOR: bench_mcp_usb_hub_diagnose_present_checks]
* The USB history reads the kernel journal into events, finds a device that connected repeatedly in a window, summarizes events and compares two snapshots. [@ANCHOR: bench_mcp_usb_hub_history_port_target] [@ANCHOR: bench_mcp_usb_hub_history_errno_of] [@ANCHOR: bench_mcp_usb_hub_history_parse] [@ANCHOR: bench_mcp_usb_hub_history_flaps] [@ANCHOR: bench_mcp_usb_hub_history_analyze] [@ANCHOR: bench_mcp_usb_hub_history_journal] [@ANCHOR: bench_mcp_usb_hub_history_num] [@ANCHOR: bench_mcp_usb_hub_history_history] [@ANCHOR: bench_mcp_usb_hub_history_snapshot] [@ANCHOR: bench_mcp_usb_hub_history_diff]
* Hubs are listed and a port is named by hub and port, by label or by the device plugged into it. [@ANCHOR: bench_mcp_usb_hub_hubs_hub_json] [@ANCHOR: bench_mcp_usb_hub_hubs_hubs] [@ANCHOR: bench_mcp_usb_hub_hubs_resolve_port] [@ANCHOR: bench_mcp_usb_hub_hubs_secs] [@ANCHOR: bench_mcp_usb_hub_hubs_flag] [@ANCHOR: bench_mcp_usb_hub_hubs_plan] [@ANCHOR: bench_mcp_usb_hub_hubs_protection]
* The reset ladder runs its privileged steps through `sudo -n` on this binary. [@ANCHOR: bench_mcp_usb_hub_ladder_helper_command] [@ANCHOR: bench_mcp_usb_hub_ladder_run_helper] [@ANCHOR: bench_mcp_usb_hub_ladder_target] [@ANCHOR: bench_mcp_usb_hub_ladder_authorize] [@ANCHOR: bench_mcp_usb_hub_ladder_driver] [@ANCHOR: bench_mcp_usb_hub_ladder_autosuspend]
* A port is keyed as hub and port. [@ANCHOR: bench_mcp_usb_hub_port_key]
* The USB hub instrument is built from its configuration entry. [@ANCHOR: bench_mcp_usb_hub_instrument]
* A device argument is a path in the USB tree or a port label. [@ANCHOR: bench_mcp_usb_hub_resolve_device]
* The topology tool returns the USB tree. [@ANCHOR: bench_mcp_usb_hub_topology]
* The find tool locates devices in the USB tree. [@ANCHOR: bench_mcp_usb_hub_find]
* A USB hub tool call is dispatched by name to its tool. [@ANCHOR: bench_mcp_usb_hub_call_with]
* Port power is probed and set through `uhubctl`, waited for until it takes effect, and reported. [@ANCHOR: bench_mcp_usb_hub_power_probe] [@ANCHOR: bench_mcp_usb_hub_power_uhubctl] [@ANCHOR: bench_mcp_usb_hub_power_wait_until] [@ANCHOR: bench_mcp_usb_hub_power_report_level] [@ANCHOR: bench_mcp_usb_hub_power_set]
* The privileged helper resolves a device directory, interface and driver in sysfs, resets, and prints the outcome with an exit code. [@ANCHOR: bench_mcp_usb_hub_privileged_device_dir] [@ANCHOR: bench_mcp_usb_hub_privileged_interface_name] [@ANCHOR: bench_mcp_usb_hub_privileged_driver_dir] [@ANCHOR: bench_mcp_usb_hub_privileged_reset] [@ANCHOR: bench_mcp_usb_hub_privileged_main]
* A protected device or port blocks power-off, cycle, reset and deauthorize unless the caller overrides, and the verdict says what blocks. [@ANCHOR: bench_mcp_usb_hub_protect_add] [@ANCHOR: bench_mcp_usb_hub_protect_block_names] [@ANCHOR: bench_mcp_usb_hub_protect_verdict_permit] [@ANCHOR: bench_mcp_usb_hub_protect_check_device] [@ANCHOR: bench_mcp_usb_hub_protect_check_port] [@ANCHOR: bench_mcp_usb_hub_protect_report]
* The USB tree is read from sysfs: devices with their speed class, hub transaction-translator mode, parent port, children, endpoints and interfaces, a speed finding, and the tree as JSON. [@ANCHOR: bench_mcp_usb_hub_sysfs_device_speed_name] [@ANCHOR: bench_mcp_usb_hub_sysfs_device_tt_mode] [@ANCHOR: bench_mcp_usb_hub_sysfs_is_device_name] [@ANCHOR: bench_mcp_usb_hub_sysfs_parent_of] [@ANCHOR: bench_mcp_usb_hub_sysfs_child_path] [@ANCHOR: bench_mcp_usb_hub_sysfs_parse_interval_us] [@ANCHOR: bench_mcp_usb_hub_sysfs_endpoints_of] [@ANCHOR: bench_mcp_usb_hub_sysfs_dev_path] [@ANCHOR: bench_mcp_usb_hub_sysfs_collect_nodes] [@ANCHOR: bench_mcp_usb_hub_sysfs_class_nodes] [@ANCHOR: bench_mcp_usb_hub_sysfs_interface_of] [@ANCHOR: bench_mcp_usb_hub_sysfs_ports_of] [@ANCHOR: bench_mcp_usb_hub_sysfs_controller_of] [@ANCHOR: bench_mcp_usb_hub_sysfs_device_at] [@ANCHOR: bench_mcp_usb_hub_sysfs_path_key] [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_children] [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_roots] [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_ancestry] [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_subtree] [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_sorted] [@ANCHOR: bench_mcp_usb_hub_sysfs_speed_finding] [@ANCHOR: bench_mcp_usb_hub_sysfs_device_json] [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_json]
* `uhubctl` and `lsusb` output is parsed (hub line, switching mode, over-current mode, port status and link state), and a failed `sudo -n` is explained. [@ANCHOR: bench_mcp_usb_hub_uhubctl_processrunner_run] [@ANCHOR: bench_mcp_usb_hub_uhubctl_explain_failure] [@ANCHOR: bench_mcp_usb_hub_uhubctl_sudo] [@ANCHOR: bench_mcp_usb_hub_uhubctl_switching_name] [@ANCHOR: bench_mcp_usb_hub_uhubctl_link_state] [@ANCHOR: bench_mcp_usb_hub_uhubctl_decode_port] [@ANCHOR: bench_mcp_usb_hub_uhubctl_switching_of] [@ANCHOR: bench_mcp_usb_hub_uhubctl_bracket] [@ANCHOR: bench_mcp_usb_hub_uhubctl_hubdescriptor_switching] [@ANCHOR: bench_mcp_usb_hub_uhubctl_hubdescriptor_over_current] [@ANCHOR: bench_mcp_usb_hub_uhubctl_parse_hub_descriptor] [@ANCHOR: bench_mcp_usb_hub_uhubctl_hub_descriptor]
* The VNA's firmware version and internal temperature are read. [@ANCHOR: bench_vna_instrument_monitoring]
* The VNA is cold-reset through its hub port and waited for with its codecs. [@ANCHOR: bench_vna_power_cycle]
* The VNA tools parse their numeric, level and modulation arguments, name the generator and meter, and read and measure calibration standards. [@ANCHOR: bench_vna_tools_num] [@ANCHOR: bench_vna_tools_level_db] [@ANCHOR: bench_vna_tools_freq_hz] [@ANCHOR: bench_vna_tools_generator_on] [@ANCHOR: bench_vna_tools_generator_level] [@ANCHOR: bench_vna_tools_mod_args] [@ANCHOR: bench_vna_tools_generator_am] [@ANCHOR: bench_vna_tools_generator_fm] [@ANCHOR: bench_vna_tools_meter] [@ANCHOR: bench_vna_tools_name_ok] [@ANCHOR: bench_vna_tools_parse_meta] [@ANCHOR: bench_vna_tools_calibrate_measure] [@ANCHOR: bench_vna_tools_read_standard]

* The supply's OFF is recognised from its reply (`OFF` or `0`); the safe state switches the output off and reads it back, and succeeds only when the supply itself says off. [@ANCHOR: bench_mcp_psu_is_off] [@ANCHOR: bench_mcp_psu_safe_state]
* How long the output may stay on comes from `for_s`, never above `max_on_s`; switching on is refused unless the setpoints stored in the supply are within the caps, a plan of it (`plan_on`) reads only and switches nothing, and a timed switch-off is armed when the output goes on. [@ANCHOR: bench_mcp_psu_on_seconds] [@ANCHOR: bench_mcp_psu_stored_setpoints] [@ANCHOR: bench_mcp_psu_plan_on] [@ANCHOR: bench_mcp_psu_output_on] [@ANCHOR: bench_mcp_psu_arm_switch_off]
* The supply's timer and output tools dispatch to those functions. [@ANCHOR: bench_mcp_main_psu_timer] [@ANCHOR: bench_mcp_main_psu_output]
* The scope's self-calibration runs only when it is asked for, and a dry run changes nothing. [@ANCHOR: bench_mcp_scope_self_cal]
* Checklist dates are worked out from civil dates and read as ISO dates. [@ANCHOR: bench_mcp_checklist_days_from_civil] [@ANCHOR: bench_mcp_checklist_iso_dates]
* The configuration names the drivers it accepts. [@ANCHOR: bench_mcp_config_driver_names]
* A tool call is dispatched by name, and the server lists every tool definition. [@ANCHOR: bench_mcp_main_call] [@ANCHOR: bench_mcp_main_all_tool_defs]
* The audio tools plan a sound and a volume change without making them (dry run), and their safe state stops this server's own `pw-play` streams and lowers any output above its volume cap, without muting, unmuting or touching a reserved device. [@ANCHOR: bench_mcp_audio_tools_dry_plan] [@ANCHOR: bench_mcp_audio_tools_set_volume_plan] [@ANCHOR: bench_mcp_audio_tools_safe_state]
* The config reads each instrument's permission tier, the VNA settings (an unknown tier or a bad setting is refused), and the SDR serial and XB-200 setting (the XB-200 for the bladeRF only), and `BENCH_MCP_CONFIG` names the file the server loads. [@ANCHOR: bench_mcp_config_tier_of] [@ANCHOR: bench_mcp_config_vna] [@ANCHOR: bench_mcp_config_sdr] [@ANCHOR: bench_mcp_config_load]
* The host DSP finds a tone's amplitude and offset with its radix-2 FFT, windowed blocks, noise floor, strongest peaks and level statistics, and refuses FFT lengths that are not a power of two. [@ANCHOR: bench_mcp_dsp_fft] [@ANCHOR: bench_mcp_dsp_check_len] [@ANCHOR: bench_mcp_dsp_block_fft] [@ANCHOR: bench_mcp_dsp_noise_floor] [@ANCHOR: bench_mcp_dsp_peaks] [@ANCHOR: bench_mcp_dsp_stats]
* Serial devices are checked before they are opened: the config's top-level `reserved_serial` list (`/dev/serial/by-id/...` paths or `usb:<bus-port>`) names devices no bench tool opens (the SainSmart relay board's place), and a supply is refused on a bare `/dev/ttyUSBn` or `/dev/ttyACMn` when a `/dev/serial/by-id` name points at that device. The checks read the file system and never open the device. [@ANCHOR: bench_mcp_serial_guard_parse_reserved] [@ANCHOR: bench_mcp_serial_guard_set_reserved] [@ANCHOR: bench_mcp_serial_guard_reserved_reason] [@ANCHOR: bench_mcp_serial_guard_by_id_name] [@ANCHOR: bench_mcp_serial_guard_check_supply] [@ANCHOR: bench_mcp_serial_guard_usb_port_of] [@ANCHOR: bench_mcp_serial_guard_check_reserved_open]
* The SCPI link opens a TCP socket or a serial port, sends terminated commands, and reads lines, numbers (with units and either header setting), length-prefixed blocks and headerless streams; a refused connection, a peer that hangs up, a silent device and a port that is not a serial port are errors. [@ANCHOR: bench_mcp_link_tcp] [@ANCHOR: bench_mcp_link_new] [@ANCHOR: bench_mcp_link_byte] [@ANCHOR: bench_mcp_link_line] [@ANCHOR: bench_mcp_link_number] [@ANCHOR: bench_mcp_link_block] [@ANCHOR: bench_mcp_link_raw_until_idle] [@ANCHOR: bench_mcp_link_serial_open] [@ANCHOR: bench_mcp_link_serial_read]
* The supply tool reads volts, amps and power, and sends a setpoint only within the configured caps. [@ANCHOR: bench_mcp_psu_measure] [@ANCHOR: bench_mcp_psu_set]
* The scope tools read the trigger, timebase and channels, measure per channel, capture a waveform to a file with its edges (refusing a stopped scope), save a screenshot by image type, and send settings only when they are valid. [@ANCHOR: bench_mcp_scope_status] [@ANCHOR: bench_mcp_scope_ch] [@ANCHOR: bench_mcp_scope_measure] [@ANCHOR: bench_mcp_scope_capture] [@ANCHOR: bench_mcp_scope_screenshot] [@ANCHOR: bench_mcp_scope_set]
* The SDR tools check a radio's configuration and its tuning, rate and gain limits, parse numeric and whole-number arguments and the receive input, and write raw I/Q as little-endian f32 pairs. [@ANCHOR: bench_mcp_sdr_check_config] [@ANCHOR: bench_mcp_sdr_limits] [@ANCHOR: bench_mcp_sdr_number] [@ANCHOR: bench_mcp_sdr_whole] [@ANCHOR: bench_mcp_sdr_in_range] [@ANCHOR: bench_mcp_sdr_antenna_arg] [@ANCHOR: bench_mcp_sdr_write_iq]
* The VNA sweep tool reads its numeric arguments strictly. [@ANCHOR: bench_mcp_vna_number]
* The server connects a scope over TCP and a supply over serial, takes one lock per instrument name, creates its output directory, and serves tool calls over MCP until it is terminated. [@ANCHOR: bench_mcp_main_connect] [@ANCHOR: bench_mcp_main_instrument_lock] [@ANCHOR: bench_mcp_main_out_dir] [@ANCHOR: bench_mcp_main_serve] [@ANCHOR: bench_mcp_main_call_tool]

## Scope connection recovery

Both anchors are in `src/link.rs`; `connect` in `src/main.rs` uses them for every scope tool call.

### `bench_mcp_link_tcp_checked` [@ANCHOR: bench_mcp_link_tcp_checked]

**Purpose.** Opens a scope link and proves the scope answers before any tool uses it. The Siglent scope serves one SCPI client at a time. After the dev box lost power on 2026-10-29 the scope kept power and, probably, the dead session: it accepted TCP on 5025 and reset every connection on `*IDN?` until it was power-cycled (cause unconfirmed). This function waits and retries, and when the scope still does not answer it reports NO-GO with evidence and the action, instead of a bare "Connection reset by peer".

```rust
Link::tcp_checked(host: &str, port: u16, waits: &[Duration], diagnose: &dyn Fn(&str, u16) -> String) -> Result<Link, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `host` | `&str` | Name or address of the scope. Borrowed. |
| `port` | `u16` | SCPI port, 5025 for the scope. |
| `waits` | `&[Duration]` | The sleep after each failed attempt, in order. There is one more attempt than waits; an empty slice means one attempt and no sleep. `connect` uses 2, 5, 10 and 20 s (37 s in all, five attempts). |
| `diagnose` | `&dyn Fn(&str, u16) -> String` | Called once, after the last attempt, for the state text in the error. `Link::scope_state` is the production one. |

**Returns.** `Ok(Link)` on the first attempt that connects and gets a non-empty line back for `*IDN?` (the reply is consumed). `Err` with `NO-GO: <host>:<port> did not answer *IDN? in N attempts over S s (attempt 1: ...; attempt N: ...). State: <diagnose>. Recommended action: power-cycle the scope ...` when every attempt failed: connect error, send error, read error (reset, timeout) or an empty reply.

**Special concerns.** Blocking; sleeps the sum of `waits` at worst, so call it from a worker thread or a tool call, not a UI loop. Each failed attempt drops its socket, which also frees the scope's single-client slot if the failure was its own half-open session. A healthy scope costs one extra `*IDN?` round trip per connection. It sends only `*IDN?`, a query that changes nothing on the instrument. Safe to retry.

**Example.**

```rust
let waits = [2, 5, 10, 20].map(Duration::from_secs);
let link = Link::tcp_checked("192.168.10.51", 5025, &waits, &|h, p| Link::scope_state(h, &[p, 80]))?;
```

**See also.** `bench_mcp_link_tcp` (one plain connect), `bench_mcp_link_scope_state`, `bench_mcp_main_connect`. Tests: `a_scope_that_resets_the_first_connections_is_waited_out_and_then_answers` and `a_scope_that_never_answers_gives_a_no_go_with_the_evidence_and_the_action` in `src/link.rs` (a fake scope on loopback that resets with linger 0). Follow-up needing the real scope: confirm or refute the stale-session cause if it recurs (wait out its idle timeout before power-cycling).

### `bench_mcp_link_scope_state` [@ANCHOR: bench_mcp_link_scope_state]

**Purpose.** The evidence for a failed scope connection: is the scope on the network, and which of its ports accept TCP.

```rust
Link::scope_state(host: &str, ports: &[u16]) -> String
```

| Argument | Type | Meaning |
|---|---|---|
| `host` | `&str` | Name or address; borrowed. |
| `ports` | `&[u16]` | Ports to try with a 2 s connect limit each (5025 and 80 for the scope). |

**Returns.** One line, for example `ping answers, port 5025 accepts, port 80: Connection refused (os error 111)`. `ping unavailable` if the `ping` program is missing. Never an error.

**Special concerns.** Blocking, at most about 1 s for ping plus 2 s per port. Runs the system `ping -c 1 -W 1` and opens, then drops, a TCP connection to each port: opening the scope's SCPI port takes its single-client slot briefly, so call it only after the real attempts failed. Reads nothing from the scope.

**See also.** `bench_mcp_link_tcp_checked`. Test: `the_scope_state_reports_an_accepting_and_a_refused_port` in `src/link.rs`.

## ELAD receivers (`elad` driver)

`src/elad/` drives an ELAD FDM-S2, FDM-DUO or FDM-S1 directly over USB through the `elad_usb` crate (`daemons/elad_usb`, AGPL, no vendor library). It is receive only, so none of its five tools is tier `full`. Manual: `instruments/elad`. An example entry: `{"driver": "elad", "tier": "read_config", "capture_dir": "/var/tmp/bench_elad", "fpga_loaded": false, "max_capture_s": 5}`. Verified on an FDM-S2 (USB firmware 3.0, hardware 1.3) on 2026-11-08 for discovery, descriptors, firmware, hardware version and EEPROM calibration; **no I/Q has been received from a real unit**, because an FDM-S1/S2 needs ELAD's FPGA image loaded by ELAD's own loader, which this project neither includes nor runs (set `"fpga_loaded": true` only after loading it yourself). The streaming tools are tested against a fake unit that replays the recorded control replies and serves a synthetic tone; the hardware check is `tests/hil_elad.rs` (`--ignored`).

* The config takes `capture_dir`, `fpga_loaded` and `max_capture_s` and refuses bad values. [@ANCHOR: elad_config]
* Tools are dispatched by tier and instrument name. [@ANCHOR: bench_mcp_elad_call_with]
* One instrument is chosen by name, or the only one. [@ANCHOR: bench_mcp_elad_instrument]
* `bench_elad_list` opens nothing. [@ANCHOR: bench_mcp_elad_list]
* `bench_elad_identify` sends only read requests. [@ANCHOR: bench_mcp_elad_identify]
* `bench_elad_capture` writes a cf32 file and a summary. [@ANCHOR: bench_mcp_elad_capture]
* `bench_elad_spectrum` writes a CSV and finds peaks. [@ANCHOR: bench_mcp_elad_spectrum]
* `bench_elad_level` returns the level in dBFS with the calibration offset. [@ANCHOR: bench_mcp_elad_level]
* Numeric arguments must be present or defaulted, finite and in range, naming the cause of the limit. [@ANCHOR: elad_args]
* A boolean argument may be absent but not anything else. [@ANCHOR: elad_flag]
* The rate must be one the model streams. [@ANCHOR: elad_rate_arg]
* The sample count is capped by memory and by the config. [@ANCHOR: elad_target_samples]
* A file name is checked and an existing file is never overwritten. [@ANCHOR: elad_new_file]
* The FFT length must be a power of two. [@ANCHOR: elad_fft_len]
* The common summary of every streaming tool: samples, measured rate, timeouts, RMS, peak, DC and clipping. [@ANCHOR: elad_summary]
* An acquisition refuses an undeclared FPGA image before streaming, always stops the stream, and reports silence as a missing FPGA image. [@ANCHOR: elad_acquire]
