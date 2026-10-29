# vnwa_open

An open Rust driver for the DG8SAQ **VNWA 3E** vector network analyzer (USB 20a0:4118, plus two PCM2900C
audio codecs), written from observed USB and audio traffic of the owner's own unit. Status (2026-10-26): it measures.
`usb` sends the confirmed requests (control transfers only, no reset, no interface claim), `capture` records the two
codecs (48 kHz, 16 bit, stereo: all they offer) and `sweep` turns a frequency list into uncalibrated S11 and S21 on the
loopback. Calibration (`cal`): one-port open/short/load and through, tested on synthetic error terms; the open/short/load standards are not yet measured on the unit. `bench_mcp` uses it (`bench_vna_sweep`). Examples: `probe`, `sweep`.

Licence: AGPL-3.0-or-later. Not published to crates.io (`publish = false`). No vendor code and no vendor firmware
is used, loaded or flashed.

## What is decoded

See `docs/PROTOCOL.md` for the evidence. In short, all control is vendor control transfers on endpoint 0 (the
measured signal arrives as audio from the two codecs):

| Item | Request | Status |
|---|---|---|
| Generator frequency | 0x4c | confirmed at 1, 2, 10, 20 MHz |
| Generator on, off, level (`generator.rs`) | 0x62, 0x61, 0x98, 0x9a, 0x0e, 0x04, 0x58, 0x91, 0x66 | sequences from a capture of the vendor program; the output is verified on a scope (tone at the commanded frequency, about -17 dBm, flat to 10 MHz); the level steps are exact to 0.1 dB from 0 to -15 dB (take a negative attenuation); below -20 dB is not yet verified |
| RF level | 0x58 (14 bits in `wValue`) | confirmed |
| Sweep frequency word and per-chip tuning words | 0x49 | confirmed at 1 to 100 MHz (nRF 10, nLO 9) |
| Offset between old and new frequency | 0x49 tail | confirmed, 14 values |
| IF (12 kHz / 6 kHz) | 0x42, 0x41 | confirmed by the codec audio |
| 18 kHz burst before a sweep | 0x43 | probable |
| The whole set-up block (nine requests) | 0x40, 0x41, 0x42, 0x43, 0x44, 0x47, 0x4b, 0x49, 0x64 | built by `setup::setup_block`, verified byte for byte against eight captured blocks; the time-per-point bytes against 84 settings from 0.2 to 100 ms |
| Everything else in the set-up block (0x44, 0x47, 0x4b and the constants in 0x40), the step counter, the audio to S-parameter maths | | not decoded |

| Audio to a complex measurement (`audio::measure_point`) | codec audio | tested on synthetic tones, on real loopback samples at 12, 6 and 4 kHz, and compared with the vendor program's own S21 (0.1 to 1 percent apart at three IFs, both codecs, four RF frequencies) |

**What the audio step is and is not.** It finds the amplitude and phase of the tone at the IF in whole IF periods (skipping
presamples and postsamples) and divides the left channel by the right (reference) channel; `vendor_convention` conjugates the
result to the vendor program's sign. Against the vendor program's own uncalibrated S21 on a TX-to-RX loopback (codec 2) it agrees
to 0.7 to 0.9 percent in magnitude and phase at 1, 10, 20 and 30 MHz, and to 1.5 percent rms over the first 15 points of a sweep when
the points are laid out in time (the unit drifts smoothly with time while it settles; the loopback is flat in RF frequency). It also
reproduces the unit's IF-dependent response (|S21| 0.874 at IF 12 kHz down to 0.767 at 1 kHz, nine IFs) to 0.1 to 1 percent on both codecs
(`Reference::Left` for codec 1, `Reference::Right` for codec 2). Not covered: a device under test with a frequency-dependent response, the open/short/load/through calibration (now `cal`, see below); the low-frequency range needs no special filtering down to 200 Hz
and the time-domain transforms with their windows are in `timedomain` (details in `docs/PROTOCOL.md`).

The two DDS chips are an AD9859 and an AD9951 (identical register maps, no sweep engine), clocked from a 12 MHz TCXO
times a premultiplier of 3, then times nRF = 10 (RF, 360 MHz) and nLO = 9 (LO, 324 MHz).

## Modes this hardware can serve (per the vendor's Help file)

All of these are built from the same operations (set the two DDS frequencies and levels, set the IF, read the audio),
so an open driver can offer them once those are decoded:

1. **Vector network analyzer**: S11, S21 (both at once on a 3E), with the open/short/load/through calibration.
2. **Signal generator**: CW with variable level; on a VNWA3/3E also AM or FM with a sine up to about 3 kHz. Output is
   about -17 dBm into 50 ohms at 1 MHz, falling with frequency, and it cannot run during a sweep. AM and FM are decoded and verified on a scope (`generator::modulate`); pulse is not.
3. **Frequency meter**: the phase of the RX input against the unit's reference at equal time steps; accuracy is the
   12 MHz TCXO's (about 0.3 ppm in the vendor's own comparison). Needs no calibration kit.
4. **Spectrum analyzer** (rudimentary, per the vendor): the LO is unfiltered so ghost lines appear, worst at higher
   frequencies; use below about 100 MHz. The unit is a direct-conversion receiver in this mode, so it also responds to
   subharmonics.

## Reliability (2026-10-28)

The unit is bus powered on a power-switching hub port (`sudo -n uhubctl -l 3-2 -p 2 -a off`, then `-a on`: a verified cold reset,
the unit goes dark, keeps no settings, and is back about 9 s after power-on). The driver handles the failures seen on it:

* **Transient transfer errors** (`transport::classify`: libusb reports the kernel's EPROTO as an I/O error; a stall is a pipe error;
  also timeouts) are sent again up to 3 times (`usb::Vnwa`), 5 ms apart, and counted (`Vnwa::resends`). A vanished unit is
  reported at once, other errors are not repeated. The read of request 0x0d after a block is stalled by the unit in about 7 % of
  tries and succeeds on a resend; since the transfers are no longer ignored, a transfer that fails for good fails the point.
* **A point that fails** (no signal on the reference, an unsteady tone, missing audio, a transfer error that outlasted its
  resends) is measured again `Config::retries` times; if the config names a hub (`Config::hub`), the unit is then cold reset once,
  reopened, and the retries repeated; then a clear error lists every attempt (`sweep::recover`). A failed attempt never
  yields a number. `Vna::recovery()` reports the re-measurements, transfer resends and power cycles.
* **Which codec is which** is read from the USB tree (`topology`): the PCM2900C on the lower port of the unit's internal hub is S11,
  the other S21; the card names (`CODEC`, `CODEC_1`) are kernel order. Checked over 5 power cycles: the order did not change here.
* `examples/power_cycle.rs <hub> <port>` does the cold reset; `VNWA_HUB=3-2:2 examples/sweep` sweeps with recovery.

## Safety

The first-run overclock offer of the vendor program was declined; this crate assumes the stock clocks (360 / 324 MHz,
inside the data sheets' 400 MSPS limit). The identity table the vendor program reads at start-up is not used.

## Tests

`cargo test` checks every encoding against bytes seen on the wire (generator word, sweep word, per-chip words,
offset, IF word).

## Checks this crate makes (anchors)

Each is a test against bytes or samples recorded from the vendor program.

* Frequency to the generator's tuning word. [@ANCHOR: vnwa_open_tuning_word]
* The generator amplitude for an attenuation of 0 to -50 dB. [@ANCHOR: vnwa_open_generator_level]
* The sweep word (start, step and count) of a captured sweep. [@ANCHOR: vnwa_open_sweep_word]
* The per-chip tuning word of the RF chip equals the generator read-back. [@ANCHOR: vnwa_open_chip_tuning_word]
* The IF word of the set-up request. [@ANCHOR: vnwa_open_if_word]
* The fields of request 0x41 that follow the IF. [@ANCHOR: vnwa_open_if_scaled_fields]
* The point-time byte that follows the time per point. [@ANCHOR: vnwa_open_point_time_byte]
* The offset tails of captured sweeps. [@ANCHOR: vnwa_open_sweep_offset]
* The generator payload: four zero bytes, then the little-endian word. [@ANCHOR: vnwa_open_generator_frequency]
* The RF level is fourteen bits. [@ANCHOR: vnwa_open_rf_level]
* The time-per-point fields reproduce all 84 observed settings. [@ANCHOR: vnwa_open_point_time_fields]
* The set-up block reproduces every captured block byte for byte. [@ANCHOR: vnwa_open_setup_block]
* Amplitude and phase of a tone at the IF are exact on a synthetic tone. [@ANCHOR: vnwa_open_tone_at_if]
* The ratio of two tones is their amplitude ratio and phase difference, in the vendor program's convention. [@ANCHOR: vnwa_open_vendor_convention]
* A measurement of a recorded sweep agrees with the vendor program's own S21, and the reference channel can be swapped. [@ANCHOR: vnwa_open_measure_point]
* A transient transfer error is sent again a bounded number of times, a vanished unit is reported, others are not repeated. [@ANCHOR: vnwa_open_transfer_retry]
* libusb errors are classified as transient, no device or other. [@ANCHOR: vnwa_open_transfer_classes]
* The S11 and S21 codecs are told apart by their USB port, whatever order the kernel named the cards in. [@ANCHOR: vnwa_open_codec_topology]
* The power cycle is off, wait, on through `sudo -n uhubctl`, then waits for the unit; failures are clear errors. [@ANCHOR: vnwa_open_power_cycle]
* A failing point is measured again, then after one cold reset, and only a success is returned. [@ANCHOR: vnwa_open_recovery]
* This driver's S11 and S21 agree with the vendor program's on the same cables to 0.25 % and 0.6 degrees (S11) and 1 % and 0.6 degrees (S21, a constant 0.8 % offset). [@ANCHOR: vnwa_open_vendor_agreement]
* The one-port error terms are recovered from open, short and load, and a known load is corrected. [@ANCHOR: vnwa_open_cal_one_port]
* A kit definition models real standards, round-trips its file, and the calibration with it removes their imperfections. [@ANCHOR: vnwa_open_cal_kit] [@ANCHOR: vnwa_open_kit_file]
* A stored calibration is refused when the IF, time per point, RF level or cable set-up differ. [@ANCHOR: vnwa_open_cal_valid]
* A calibration survives its text file and a damaged file is refused. [@ANCHOR: vnwa_open_cal_file]
* The windows have their textbook shapes. [@ANCHOR: vnwa_open_window]
* A delay line peaks at its delay in low-pass and band-pass mode, windows lower the sidelobes, a reflection's step settles at its coefficient. [@ANCHOR: vnwa_open_time_domain]
* The tone offset between the measured and reference channels is recovered with its sign, the codec drift cancels. [@ANCHOR: vnwa_open_meter_offset]
* A unit that does not come back from a power cycle gets a second cycle with a longer power-off. [@ANCHOR: vnwa_open_power_escalation]
* The soak run compares every sweep with the first, and counts errors, wrong results and hangs separately: `examples/soak.rs`.
* A point is accepted only if the reference tone at the IF is steady across the analysis window (IF error 2 Hz or less, level change 3 % or less); a slipped DDS PLL, a retune inside the window or a stage boundary fails it. [@ANCHOR: vnwa_open_stability]
* A window that contains a buffer overrun is an error and never frames; the point is measured again from a separate budget and never power cycles the unit. [@ANCHOR: vnwa_open_overrun_window]
* The streams are closed before the unit's power is cut (an open stream on a vanishing codec reports about 5,000 overruns a second). [@ANCHOR: vnwa_open_capture_close]
* The mark is the frame count at this instant, not at the last delivery, so any ALSA period works. [@ANCHOR: vnwa_open_capture_mark]
* The firmware version (0xff) and the internal temperature counts (0x85) are read, the temperature refused on a firmware older than v5.16. [@ANCHOR: vnwa_open_firmware_version] [@ANCHOR: vnwa_open_temperature]
* A failure that a retry or power cycle cured keeps its evidence (error text, raw channel levels, block send time, overruns, the last 16 transfers), and so does every power cycle of the recovery. [@ANCHOR: vnwa_open_failure_log] [@ANCHOR: vnwa_open_failure_evidence] [@ANCHOR: vnwa_open_transfer_log]
* The analysis window is timed from the end of the block send, not from before it. [@ANCHOR: vnwa_open_window_from_send_end]
* The installer runs only the commands on its allow-list (with or without `sudo`), and refuses every other command before it is run. [@ANCHOR: vnwa_open_install_commands]
* A USB device is the VNWA's codec only when it is a PCM2900C that has a VNWA control chip as a sibling on the same parent hub; no other sound card is reserved. [@ANCHOR: vnwa_open_reserved_codec]
* The codec cards are found in the `pw-dump` document of a PipeWire sound server. [@ANCHOR: vnwa_open_sound_server_pipewire]
* The codec cards are found in the text of `pactl list cards` of a PulseAudio sound server. [@ANCHOR: vnwa_open_sound_server_pulse]
* A sweep can measure a configured number of points at its start frequency first and throw the results away, to bring the unit to a steady state. [@ANCHOR: vnwa_open_discard_points]
* The three one-port error terms come from the three standards by solving a 3x3 complex system; standards that are not distinct give no solution. [@ANCHOR: vnwa_open_cal_solve3]
* A calibration is built from the open, short, load and through sweeps of one rising frequency grid; open, short and load must come together, sweeps of another length, a through with no signal or nothing to calibrate are refused. [@ANCHOR: vnwa_open_cal_build]
* The reflection of a standard is moved back through its offset line (delay and loss), there and back. [@ANCHOR: vnwa_open_cal_through_offset]
* A frequency is placed between two calibrated frequencies for interpolation, and one outside the calibrated range is refused. [@ANCHOR: vnwa_open_cal_locate]
* A raw reflection is corrected with the interpolated one-port terms. [@ANCHOR: vnwa_open_cal_s11]
* A raw transmission is divided by the interpolated through measurement. [@ANCHOR: vnwa_open_cal_s21]
* A measured point is corrected in each parameter the calibration covers and left `None` in the other. [@ANCHOR: vnwa_open_cal_correct]
* A calibration is written as a text file (header, frequency count, one line per frequency). [@ANCHOR: vnwa_open_cal_to_text]
* A kit definition is written as a text file that reads back to the same definition. [@ANCHOR: vnwa_open_kit_to_text]
* A codec is found by (part of) its name and opened with the chosen ALSA period. (Hardware test.) [@ANCHOR: vnwa_open_capture_open_with]
* The frames since the last delivery of a period are the clock times the rate, at most one period. [@ANCHOR: vnwa_open_capture_frames_since]
* The `vnwa_open` command line: `is-vnwa-codec` answers by exit status, `install-system-config` runs the installer, anything else prints the usage and exits 2. [@ANCHOR: vnwa_open_cli_main]
* The codec cards in sysfs are listed with their USB paths, and only the codec vendor is read. [@ANCHOR: vnwa_open_codec_find_cards]
* The captured AM sequence (500 Hz, 100 %) is sent in the captured order and its stalled last request is not an error. [@ANCHOR: vnwa_open_generator_am_captured]
* A managed file lives under the staging root when one is given. [@ANCHOR: vnwa_open_install_path]
* A managed file is missing, current or different from the packaged content. [@ANCHOR: vnwa_open_install_state]
* A command runs under `sudo -n` unless there is a staging root or the user is root. [@ANCHOR: vnwa_open_install_sudo_prefix]
* The installer runs a command only if it is on the allow-list, and logs it. [@ANCHOR: vnwa_open_install_do_run]
* What the installer did is appended to its log. [@ANCHOR: vnwa_open_install_note]
* The installer refuses while a codec is held open by a sweep or soak, unless forced. [@ANCHOR: vnwa_open_install_busy]
* The dry run lists the files and commands, and changes nothing. [@ANCHOR: vnwa_open_install_plan]
* The installer is run in install, check or uninstall mode, as a dry run or applied. [@ANCHOR: vnwa_open_install_run]
* Installing backs up what it replaces, is idempotent, and undoes its steps on a failure midway. [@ANCHOR: vnwa_open_install_install]
* Uninstalling removes what was installed and restores the backups. [@ANCHOR: vnwa_open_install_uninstall]
* The production executor runs a real command and returns its failure text. [@ANCHOR: vnwa_open_install_process_exec]
* One frequency-meter measurement (the recovery repeats it): the set-up block is sent and the tone offset is fitted. (Hardware test.) [@ANCHOR: vnwa_open_meter_once]
* The cold reset of the unit through its hub port, waiting for the unit and both codecs. (Hardware test.) [@ANCHOR: vnwa_open_power_cold_reset]
* The production runner returns the exit status and both output streams of a real program. [@ANCHOR: vnwa_open_power_process_runner]
* The hub port is switched through `sudo -n uhubctl`; a failure is a clear error. [@ANCHOR: vnwa_open_power_uhubctl]
* A condition is polled until it is true or the timeout passes. [@ANCHOR: vnwa_open_power_wait_until]
* The unit is opened through libusb by vendor and product id. (Hardware test.) [@ANCHOR: vnwa_open_rusb_transport_open]
* The scripted transport plays back read outcomes in order, then succeeds. [@ANCHOR: vnwa_open_scripted_read]
* A session opens the unit and both codecs (found by topology unless named). (Hardware test.) [@ANCHOR: vnwa_open_session_open]
* The sound server is asked (`pw-dump`, else `pactl`) and judged; what could not be inspected is reported as unknown. [@ANCHOR: vnwa_open_sound_server_check]
* Only the cards on the USB port of a codec that has the control chip beside it are the VNWA's. [@ANCHOR: vnwa_open_sound_server_judge]
* The USB port is read from the card's bus path. [@ANCHOR: vnwa_open_sound_server_port_of]
* The exposure is stated plainly: protected, exposed or unknown. [@ANCHOR: vnwa_open_sound_server_protected]
* An exposed codec gives a warning that names the fixes. [@ANCHOR: vnwa_open_sound_server_warning]
* The internal temperature counts are read from request 0x85 and refused on a firmware that has no sensor. [@ANCHOR: vnwa_open_temperature_counts]
* A time-domain transform needs at least four points on a uniform, rising frequency grid. [@ANCHOR: vnwa_open_time_domain_check_grid]
* The delay of the largest peak is found by parabolic interpolation. [@ANCHOR: vnwa_open_time_domain_peak_time]
* One point is traced raw: the audio of both codecs around the set-up block send. (Hardware test.) [@ANCHOR: vnwa_open_trace_point]
* A transfer error reads as its message, with `(transient)` when it is worth sending again. [@ANCHOR: vnwa_open_transfer_error_text]
* Every transfer try is remembered with its time, name and outcome, the last 16 kept. [@ANCHOR: vnwa_open_transfer_remember]
* The recovery's cold reset closes the streams, power cycles the hub port and reopens the session. (Hardware test.) [@ANCHOR: vnwa_open_vna_cold_reset]
* A window is chosen by name, several spellings each; an unknown name is refused with the list. [@ANCHOR: vnwa_open_window_from_name]

## Drift (2026-10-29)

The sensor of the unit shows a warm-up of about 100 counts in the first 5 minutes after a power-up, during which a loopback sweep changes by less than 0.04 % and 0.01 degrees on average
(`docs/PROTOCOL.md`). `examples/soak.rs` therefore judges a sweep against two baselines (the first of the run and one taken right after each power cycle) with a tolerance of 0.3 % and 0.3 degrees
plus 0.1 % and 0.1 degrees per 100 counts of temperature change since that baseline, and records an event for every deviation (the constants are to be refined from the second soak's data).
* A stalled 0x0d read is tried again after growing pauses, up to 6 times. [@ANCHOR: vnwa_open_patient_read]
* The IF tone starts 61 ms and the 18 kHz burst about 1 ms after the end of the send, in a recorded point. [@ANCHOR: vnwa_open_emission_onset]
* A storm of overrun reports with no audio between them closes the stream and fails the window at once. [@ANCHOR: vnwa_open_capture_close]
* AM and FM: the settings-dependent fields of the vendor's start sequence reproduce every captured start, and the output was checked on a scope. [@ANCHOR: vnwa_open_am_fields] [@ANCHOR: vnwa_open_fm_fields] [@ANCHOR: vnwa_open_modulation_sequence]
* A modulating unit refuses every request; a USB reset of its control chip ends the modulation, and a power cycle ladder is the fallback. [@ANCHOR: vnwa_open_end_modulation] [@ANCHOR: vnwa_open_usb_reset] [@ANCHOR: vnwa_open_modulation_recovery]
* A program that embeds this crate can install its own power-cycle implementation (bench_mcp installs its USB hub module's), used by the recovery and the generator-off recovery. [@ANCHOR: vnwa_open_cycle_hook]
* Spectrum analyzer: the audio power around the LO at each frequency (TX chip silent or a 12 kHz tracking generator), band power by DFT, settings refused if they would leave holes. [@ANCHOR: vnwa_open_sa_band_rms] [@ANCHOR: vnwa_open_sa_spectrum]
