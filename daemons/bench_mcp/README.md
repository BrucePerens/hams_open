# bench_mcp

One MCP server (official `rmcp` SDK, stdio) for the bench: about 100 tools, each with a manual page served through the MCP itself.
It drives a Siglent **oscilloscope** (SDS1000X-E, over the LAN), a bench **power supply** (Abestop ATM6301), a **vector network
analyser** (DG8SAQ VNWA 3E, no vendor software), **SDRs** (bladeRF, LimeSDR), **USB hubs** (topology, diagnosis, and port power
cycling to reset a device), **sound devices** (play, record, meter, loopback, speech, with discovery by signature), **radios**
through Hamlib's `rigctld`, **antenna rotators**, and a **procedure runner** for long multi-step programs, plus a template
instrument for contributors. Adding an instrument means one driver module and one row in `DRIVER_KINDS` and `MODULES` (see
`docs/EXTENDING.md`). Each driver's manual pages carry a `verified` line saying what was checked on the real hardware and when.

| Driver (`driver` in the config) | What it is | Highlights | Section / doc |
|---|---|---|---|
| `siglent_sds` | Siglent SDS1000X-E oscilloscope over the LAN (written against an SDS1202X-E, firmware 1.3.27) | capture, screenshot, measure, spectrum, self-calibration | [Tools](#tools), [Scope functions](#scope-functions-probed-on-the-sds1202x-e-firmware-1327-2026-10-28) |
| `atm6301` | Abestop ATM6301 bench supply (0-60 V, 0-10 A), CH340 USB-serial | setpoints with caps, output switch with `confirm_load_connected`, `max_on_s` and a safe-state tool | [Tools](#tools) |
| `vnwa_open` (VNA) | DG8SAQ VNWA 3E vector network analyser through the `vnwa_open` crate, no vendor software | S11/S21 sweeps, calibration (open, short, load, through), generator (CW, level, AM, FM), frequency meter, spectrum analyser, firmware and temperature, reliability ladder with hub power cycle | [The VNA's functions](#the-vnas-functions-as-tools-2026-10-28) |
| `bladerf`, `limesdr` | SDRs (bladeRF with XB-200, LimeSDR-USB with two coherent receivers) | capture, survey, coherence; receive only | [Tools](#tools) |
| `usb_hub` | USB topology and hub port power (uhubctl) | topology, find, diagnose, bandwidth, watch, history, port power (verified cold reset), protection of devices | [USB hub tools](#usb-hub-tools-usb_hub-driver), `docs/USB_HUB_TOOLS.md` |
| `audio` | Sound devices, with discovery by signature | play, record, meter, loopback, speak, alert, verify, synthesizers; never the system default device; `dry_run` and a safe state | [Audio tools](#audio-tools-audio-driver), `docs/AUDIO_TOOLS.md` |
| `hamlib` | Radios through Hamlib's `rigctld` (a separate process; LGPL, not linked) | models, get and set, scan, watch, memory, load checks, transmit tests with an attestation and a watchdog | [Hamlib rigs](#hamlib-rigs-hamlib-driver), `docs/HAMLIB_TOOLS.md` |
| `rotator` | Antenna rotators through `rotctld` | status, point, park, stop | [Rotators](#rotators-rotator-driver) |
| `procedure` | The procedure runner | long multi-step programs (CPython) that call bench tools through the normal dispatcher, with failure reports, safe-state routines and saved examples | [Procedure runner](#procedure-runner-procedure-driver) |
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
      "psu":   {"driver": "atm6301", "tier": "read_config", "serial": "/dev/ttyUSB0", "baud": 115200,
                "max_volts": 5.5, "max_amps": 1.0, "max_on_s": 600},
      "vna":   {"driver": "vnwa_open", "tier": "read_config", "if_hz": 12000, "point_ms": 60, "rf_level": 16383},
      "blade": {"driver": "bladerf", "tier": "read_config", "xb200": "auto1db"},
      "lime":  {"driver": "limesdr", "tier": "read_config"},
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
| `bench_sdr_capture`, `bench_sdr_survey`, `bench_sdr_coherence` | read_config (they tune the radio; receive only) | SDR |

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
* A sweep is written as a CSV with a header and one row per point. [@ANCHOR: bench_mcp_vna_csv]

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
* A watch reports a device leaving and returning with a new device number. [@ANCHOR: usb_hub_watch]
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
