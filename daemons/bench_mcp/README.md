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
      "vna":   {"driver": "vnwa_open", "tier": "read_config", "if_hz": 12000, "point_ms": 60, "rf_level": 16383},
      "blade": {"driver": "bladerf", "tier": "read_config", "xb200": "auto1db"},
      "lime":  {"driver": "limesdr", "tier": "read_config"}}}

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
| `bench_psu_output` | read_config to switch off, **full** to switch on | supply |
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
frequency, so a signal on an H or W connector is not heard. Levels are dBFS at the set gain, **not calibrated power**. Nothing transmits: there is no transmit tool.

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
