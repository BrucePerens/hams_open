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
* A point with all four channels dead says nothing leaves the TX port and names the TX connector, pad, tee and leads first. [@ANCHOR: vnwa_open_no_reference_message]
* The functions that modulate, choose the power-cycle implementation, climb the modulation ladder and measure one spectrum point are anchored. [@ANCHOR: vnwa_open_generator_modulate] [@ANCHOR: vnwa_open_power_cycle_with] [@ANCHOR: vnwa_open_power_modulation_ladder] [@ANCHOR: vnwa_open_spectrum_sa_once]

## Soak scope connection (`soak_scope`)

`src/soak_scope.rs` is the soak harness's scope connection; `examples/soak.rs` uses it, and `tools/soak_gonogo.sh` reports its failure. It follows the same rule as bench_mcp's `Link::tcp_checked`: a Siglent scope that still holds a dead single-client SCPI session (after a host power loss, as on 2026-10-29) resets queries, so the harness waits and retries, then says NO-GO with the evidence and the action.

### `vnwa_open_soak_scope_open_checked` [@ANCHOR: vnwa_open_soak_scope_open_checked]

**Purpose.** Connect to the scope and prove it answers `*IDN?` before the generator step uses it.

```rust
pub fn open_checked(addr: &str, waits: &[Duration], diagnose: &dyn Fn(&str) -> String) -> Result<ScopeStream, String>
pub type ScopeStream = (BufReader<TcpStream>, TcpStream); // reader and writer on one socket, 5 s read timeout
```

| Argument | Type | Meaning |
|---|---|---|
| `addr` | `&str` | `host:port`, for example `192.168.10.51:5025`. |
| `waits` | `&[Duration]` | Sleep after each failed attempt; one more attempt than waits. The soak uses 2, 5, 10 and 20 s. |
| `diagnose` | `&dyn Fn(&str) -> String` | Called once after the last failure for the state text; `soak_scope::state` in production. |

**Returns.** `Ok` with the connected stream after the first non-empty `*IDN?` reply. `Err` starting `NO-GO: scope <addr> did not answer *IDN? in N attempts over S s (...)`, with each attempt's error, the state, and the recommended action (power-cycle the scope).

**Special concerns.** Blocking, up to the sum of `waits` plus 5 s connect and 5 s read per attempt. Each failed attempt drops its socket. Sends only `*IDN?`. In the soak the error becomes a `generator_step_failed` event, which `soak_gonogo.sh` prints in full (up to 600 characters) as the NO-GO reason.

**See also.** `vnwa_open_soak_scope_state`. Tests: the unit tests in `src/soak_scope.rs` (fake scope on loopback that resets the first connections) and `tests/soak_gonogo.rs` (the verdict script on a recorded log).

### `vnwa_open_soak_scope_state` [@ANCHOR: vnwa_open_soak_scope_state]

**Purpose.** Evidence for the failure report.

```rust
pub fn state(addr: &str) -> String
```

| Argument | Type | Meaning |
|---|---|---|
| `addr` | `&str` | `host:port`; the port defaults to 5025 if missing. |

**Returns.** `ping answers|ping does not answer|ping unavailable, port <p> accepts|port <p>: <error>, port 80 ...`. Never an error. Blocking (about 1 s ping plus 2 s per port); briefly opens TCP connections to the SCPI port and port 80, so call it only after the real attempts failed.

**See also.** `vnwa_open_soak_scope_open_checked`. Test: `the_state_names_an_accepting_port_and_a_refused_one`.

### `vnwa_open_soak_scope_try_once` [@ANCHOR: vnwa_open_soak_scope_try_once]

**Purpose.** One connection attempt for `open_checked`; private.

```rust
fn try_once(addr: &str) -> Result<ScopeStream, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `addr` | `&str` | `host:port`, resolved to its first address. |

**Returns.** `Ok(stream)` after a non-empty `*IDN?` reply. `Err` text starting `connect:`, `send:` or `read:` (with the OS error, for example `Connection reset by peer`), or `empty reply to *IDN? (the scope closed the connection)`.

**Special concerns.** Blocking: 5 s connect limit and 5 s read limit. Sends only `*IDN?`. Test: `a_scope_that_resets_the_first_connections_is_waited_out_and_then_answers`.

### `vnwa_open_soak_scope_connects` [@ANCHOR: vnwa_open_soak_scope_connects]

**Purpose.** A single TCP connect probe for `state`; private.

```rust
fn connects(host: &str, port: &str) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `host` | `&str` | Name or address. |
| `port` | `&str` | Port number as text. |

**Returns.** `Ok(())` if a connection was accepted within 2 s (it is dropped at once); `Err` with the resolver or OS error text otherwise. Blocking, 2 s at most. Test: `the_state_names_an_accepting_port_and_a_refused_one`.

## Soak USB topology check (`soak_topology`)

`src/soak_topology.rs` decides whether the USB tree changed in a way that matters to the unit. Before 2026-11-06 the soak compared the whole `lsusb -t` and recorded a false `usb_topology_changed` whenever an unrelated device (a USB-C dock, 2026-10-29) was unplugged. `examples/soak.rs` calls `compare` after the start, after each scheduled power cycle and after a recovery cycle. The test fixtures `tests/soak_fixtures/lsusb_start.txt` and `lsusb_cycle_2.txt` are the real pair from the second soak.

### `vnwa_open_soak_topology_change` [@ANCHOR: vnwa_open_soak_topology_change]

**Purpose.** The result of comparing two `lsusb -t` texts.

```rust
pub enum Change { None, OutsideOnly, Unit { first: String, now: String } }
```

| Variant | Meaning |
|---|---|
| `None` | The whole normalised trees are equal. |
| `OutsideOnly` | Only devices outside the unit's part differ. The soak logs an `INFO` line and records no event. |
| `Unit { first, now }` | The unit's part differs; `first` and `now` are its normalised subtrees (or `(bus not present)`). The soak records a `usb_topology_changed` event with both as the detail. |

Plain value, `Clone`, created by `compare`, owned by the caller; no serialized form and no shared state.

### `vnwa_open_soak_topology_normalise` [@ANCHOR: vnwa_open_soak_topology_normalise]

**Purpose.** Makes two `lsusb -t` texts comparable by removing the device numbers (`Dev 17,`), which change at every enumeration.

```rust
pub fn normalise(lsusb_t: &str) -> String
```

| Argument | Type | Meaning |
|---|---|---|
| `lsusb_t` | `&str` | Output of `lsusb -t`, borrowed. |

**Returns.** The same lines with each `Dev N,` removed (the space after the comma stays, so lines have two spaces there), joined with `\n`. Lines without a device number are unchanged. Pure; never fails.

### `vnwa_open_soak_topology_unit_subtree` [@ANCHOR: vnwa_open_soak_topology_unit_subtree]

**Purpose.** Picks out of `lsusb -t` the lines that belong to the unit: the root hub line of its bus (the controller), every line of each hub on the way down to the switched port, and the device at that port with everything below it (the unit's hub, both codecs and their interfaces, with ids, classes, drivers and speeds).

```rust
pub fn unit_subtree(lsusb_t: &str, hub: &HubPort) -> Option<String>
```

| Argument | Type | Meaning |
|---|---|---|
| `lsusb_t` | `&str` | Output of `lsusb -t`, borrowed. |
| `hub` | `&HubPort` | The hub in uhubctl's naming (`3-2`, or `3-2.1` for a hub behind a hub) and the switched port (2). The path is the hub's ports plus this port. |

**Returns.** `Some(text)` (normalised, lines joined with `\n`) if the hub's bus is in the output, even when the unit's branch is gone (the text then lacks the branch, so it differs). `None` if the bus is absent or the hub name is not of the form `<bus>-<port>[.<port>...]`. Siblings of the unit's branch, other ports of its hubs, and other buses are left out. Pure; reads nothing from the system.

### `vnwa_open_soak_topology_compare` [@ANCHOR: vnwa_open_soak_topology_compare]

**Purpose.** The soak's single decision: is this a change to report?

```rust
pub fn compare(first: &str, now: &str, hub: &HubPort) -> Change
```

| Argument | Type | Meaning |
|---|---|---|
| `first` | `&str` | The first `lsusb -t` of the run, raw or normalised. |
| `now` | `&str` | The current `lsusb -t`. |
| `hub` | `&HubPort` | As for `unit_subtree`. |

**Returns.** `Change::Unit` if the unit subtrees differ (a codec missing, a different port, a different speed or driver, the bus gone); otherwise `OutsideOnly` if the rest differs; otherwise `None`. Pure.

**See also.** Tests in `src/soak_topology.rs`: the dock removal in the recorded pair is `OutsideOnly`; a missing codec, a changed port, a changed speed and a missing bus are `Unit`. Real-hardware follow-up: none needed for the logic; the next real soak will show whether an `INFO` line appears when a device is unplugged.

## USB, generator, set-up and sweep-check functions (ADR 0113 entries)

### `vnwa_open_usb_send` [@ANCHOR: vnwa_open_usb_send]

**Purpose.** Send one host-to-device vendor control transfer (a `Request` built by `protocol`) to the unit. Every block of the sweep set-up, the generator frequency and the modulation sequences go through it, so every send gets the same bounded resend of transient errors.

```rust
impl Vnwa { pub fn send(&self, r: &Request) -> Result<(), String> }
```

| Argument | Type | Meaning |
|---|---|---|
| `r` | `&Request` | Borrowed; `request_type`, `request`, `value`, `index` and the `data` stage, all used as given. |

**Returns.** `Ok(())` when the transport accepted all of `r.data`. `Err` text `request 0x<nn>: ...` when a transient error outlasted `TRANSFER_RETRIES` (`still failing after 3 resends (...)`), when the unit has left the bus or the transfer failed for another reason (the transport's text), or when fewer bytes than `r.data.len()` were written (`N of M bytes sent`).

**Special concerns.** Blocking; waits `RETRY_PAUSE` (5 ms) between resends, so at worst about 15 ms of sleeping plus the transport timeout (1 s) per try. Every try, and each resend, is counted in `resends()` and remembered by `recent_transfers()` for the evidence record. Not idempotent in general (a repeated write reaches the unit twice), but the requests this crate sends are state settings, safe to repeat. No transmitter is keyed; the unit is a measuring instrument.

**See also.** `vnwa_open_usb_read`, `vnwa_open_transfer_retry`, `vnwa_open_transfer_log`. Test: `a_read_returns_only_the_bytes_the_unit_sent_and_a_send_is_a_write_of_the_request` (over the `Scripted` transport).

### `vnwa_open_usb_read` [@ANCHOR: vnwa_open_usb_read]

**Purpose.** Send one device-to-host vendor control transfer and return what the unit answered. Used for the generator read-back (0x3a), the 0x0e/0x04 handshakes and the reads the vendor program makes to switch modes.

```rust
impl Vnwa { pub fn read(&self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>, String> }
```

| Argument | Type | Meaning |
|---|---|---|
| `request` | `u8` | The vendor request number (`bRequest`). |
| `value` | `u16` | `wValue`; for 0x58 it is the RF level, for the generator reads a fixed vendor constant. |
| `index` | `u16` | `wIndex`; 0 for every request this crate uses. |
| `len` | `usize` | The size of the receive buffer, which is the most the unit may return. |

**Returns.** `Ok(bytes)` truncated to the number of bytes the unit actually sent (it may be fewer than `len`, even none). `Err` text `read request 0x<nn>: ...`, with the same causes as `vnwa_open_usb_send`.

**Special concerns.** Blocking, with the same resend rule (3 resends, 5 ms apart) and counters as `send`. The 0x0d read, which the unit stalls while busy, uses `read_patient` instead. Allocates a zeroed `len`-byte buffer per call.

**See also.** `vnwa_open_usb_send`, `vnwa_open_patient_read`. Test: as for `send`.

### `vnwa_open_generator_run` [@ANCHOR: vnwa_open_generator_run]

**Purpose.** Play a fixed list of vendor reads in order (the `ENTER` list that switches the generator mode on); private to `generator`.

```rust
fn run(usb: &Vnwa, reads: &[Read]) -> Result<(), String> // Read { request: u8, value: u16, len: usize }
```

| Argument | Type | Meaning |
|---|---|---|
| `usb` | `&Vnwa` | The open unit. |
| `reads` | `&[Read]` | The reads, each sent as `usb.read(request, value, 0, len)`, with the answers discarded. |

**Returns.** `Ok(())` after the last read; the first `Err` from a read stops the list (nothing after it is sent) and is returned unchanged.

**Special concerns.** Blocking; each read has the bounded resend of `vnwa_open_usb_read`. Order matters: the unit takes the vendor sequence only in the order the vendor program sends it. Test: `switching_the_generator_on_sends_the_vendor_sequence_and_a_bad_level_sends_nothing` checks all four reads with their wValues.

### `vnwa_open_generator_set_frequency` [@ANCHOR: vnwa_open_generator_set_frequency]

**Purpose.** Set the generator frequency of a unit already in generator mode; private, used by `on` (first frequency) and `retune`.

```rust
fn set_frequency(usb: &Vnwa, freq_hz: f64, first: bool) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `usb` | `&Vnwa` | The open unit, already in generator mode. |
| `freq_hz` | `f64` | The output frequency in hertz; converted to the unit's tuning word by `protocol::generator_frequency` (see `vnwa_open_generator_frequency` for the range). |
| `first` | `bool` | `true` for the first frequency after switching on: the write carries `wValue` 0x00c8, as the vendor program does; `false` for a retune: `wValue` 0. |

**Returns.** `Ok(())` after three transfers: read 0x0e (64 bytes), write 0x4c (the frequency, 8 data bytes), read 0x04 with `wValue` 0x090a (8 bytes). The first failing transfer returns its error and stops the sequence.

**Special concerns.** Blocking. Changes the frequency of the RF output on the unit's port at once; it does not change the level. Safe to repeat. Test: the generator sequence test above, including the retune without 0x00c8.

### `vnwa_open_generator_on` [@ANCHOR: vnwa_open_generator_on]

**Purpose.** Switch the unit's signal generator on at a frequency and a level and leave it running. The public entry of the generator mode, used by the generator example and the bench tools.

```rust
pub fn on(usb: &Vnwa, freq_hz: f64, attenuation_db: f64) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `usb` | `&Vnwa` | The open unit, borrowed. |
| `freq_hz` | `f64` | The output frequency in hertz (see `set_frequency`). |
| `attenuation_db` | `f64` | 0 (full scale) down to -50 dB, inclusive. |

**Returns.** `Ok(())` once the level is set. `Err` `generator attenuation <x> dB is outside -50 to 0 dB` before anything is sent; otherwise the first transfer error, with the later transfers not sent.

**Special concerns.** Blocking, 8 control transfers in the order: four mode reads (0x62, 0x61, 0x98, 0x9a), the frequency (0x0e, 0x4c, 0x04), the level (0x58). The output stays on until `generator::off` or another mode is entered. The level is checked first so a bad argument cannot leave the unit half set up. It emits a measurement signal at the unit's own low output level, not through a transmitter or a relay.

**See also.** `vnwa_open_generator_level`, `vnwa_open_generator_run`, `vnwa_open_generator_set_frequency`. Test: `switching_the_generator_on_sends_the_vendor_sequence_and_a_bad_level_sends_nothing`.

### `vnwa_open_generator_check_frequency` [@ANCHOR: vnwa_open_generator_check_frequency]

**Purpose.** Refuse a generator frequency the unit's DDS cannot be programmed with, before anything is sent. `protocol::tuning_word` converts with a saturating cast, so without this check a NaN or negative frequency became 0 Hz and 360 MHz or more became the largest word, a tone at a frequency other than the one asked for and no error.

```rust
pub fn check_frequency(freq_hz: f64) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | The requested output frequency in hertz. |

**Returns.** `Ok(())` when `freq_hz` is finite, above 0 and its tuning word `round(f * 2^32 / 360 MHz)` is at most `u32::MAX`. Otherwise `Err` naming the frequency and the 360 MHz DDS clock.

**Special concerns.** Pure and non-blocking, no side effects. Called first by `on`, `set_frequency` (so `retune`) and `modulate`, so a refused frequency sends no transfer. Between 180 MHz (the DDS Nyquist frequency) and 360 MHz the word is valid and the output is the spectral image; that range has not been measured and is not refused. It limits a signal at the unit's own low output level, not a transmitter. Test: `a_frequency_whose_tuning_word_does_not_fit_is_refused_before_anything_is_sent`.

### `vnwa_open_power_cold_reset_via` [@ANCHOR: vnwa_open_power_cold_reset_via]

**Purpose.** The body of `cold_reset`: cycle the unit's hub port through the program's installed power-cycle implementation (`set_cycle_hook`, the bench's hub module with its protection rules) when there is one, else through `sudo -n uhubctl`, using the two-step ladder (3 s off with 25 s for the unit, then 12 s off with 45 s), then pause so the unit and its codecs answer reliably.

```rust
fn cold_reset_via(hook: Option<&CycleHook>, port: &HubPort, settle: Duration) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `hook` | `Option<&CycleHook>` | The installed implementation, or `None` for the built-in one. |
| `port` | `&HubPort` | The hub and port to cycle. |
| `settle` | `Duration` | The pause after the unit is back (8 s in production, zero in tests). |

**Returns.** `Ok(())` once the unit is back and `settle` has passed; otherwise the error of the hook or the failures of every rung of the ladder.

**Special concerns.** Blocking for up to about a minute. Removes and restores power on the hub port, so every setting the unit held is lost. Not idempotent in effect, but safe to repeat. Test: `the_production_cold_reset_goes_through_an_installed_cycle_implementation`.

### `vnwa_open_audio_analysis_span` [@ANCHOR: vnwa_open_audio_analysis_span]

**Purpose.** Choose which part of one point's audio to analyse: a whole number of IF periods, so the DFT of the tone has no leakage. Called by `measure_point`.

```rust
pub fn analysis_span(len: usize, fs: f64, if_hz: f64, presamples: usize, postsamples: usize) -> Option<(usize, usize)>
```

| Argument | Type | Meaning |
|---|---|---|
| `len` | `usize` | Samples in one channel of the point. |
| `fs` | `f64` | Sample rate in hertz (48000). |
| `if_hz` | `f64` | The measurement IF in hertz; `fs / if_hz` is the samples per period (not necessarily a whole number). |
| `presamples` | `usize` | Samples skipped at the start (the settling). |
| `postsamples` | `usize` | Samples reserved at the end. |

**Returns.** `Some((start, count))`: `start` is `presamples`, `count` is `round(periods * fs / if_hz)` with `periods = floor((len - presamples - postsamples) / (fs / if_hz))`. `None` when not even one period fits, or when `presamples + postsamples` exceeds `len`; the vendor program refuses to sweep in that case too.

**Special concerns.** Pure, no allocation, no blocking, any thread. No panic: the subtraction is checked. `if_hz` of 0 gives an infinite period and so `None`.

**See also.** `vnwa_open_measure_point`. Test: `the_analysis_span_is_whole_if_periods_between_the_presamples_and_postsamples`.

### `vnwa_open_setup_timing_pair` [@ANCHOR: vnwa_open_setup_timing_pair]

**Purpose.** The two timing counts (bytes 2 and 5 of request 0x40) the unit wants for a time per point; private to `setup`, called by `point_time_fields`.

```rust
fn timing_pair(b0: u8) -> (u8, u8)
```

| Argument | Type | Meaning |
|---|---|---|
| `b0` | `u8` | The first byte: the time per point in the vendor program's 0.667 ms units. |

**Returns.** `(byte2, byte5)`: `ceil(16 / b0)` for both, except `(3, 4)` at `b0` = 5, and `(1, 1)` from `b0` = 16 up (and for 0). Fitted to the 84 captured settings in `TIMING_OBSERVED`; why the program has two counts is not known.

**Special concerns.** Pure; no panic. The rule is empirical, so a setting outside the captured ones is a fit, not a measurement. Test: `the_timing_pair_follows_the_ceiling_rule_with_the_one_exception_at_five`.

### `vnwa_open_sweep_check` [@ANCHOR: vnwa_open_sweep_check]

**Purpose.** Refuse a `Config` the unit or the analysis cannot use, before any hardware is opened. `Vna::open` calls it first.

```rust
pub fn check(c: &Config) -> Result<(), String>
```

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&Config` | Borrowed; read-only. |

**Returns.** `Ok(())`, or `Err` text for the first rule broken, in this order: `if_hz` not finite, not positive, not dividing 48 kHz into a whole number of samples of at least 4 (`does not divide 48 kHz`); `point_ms` outside 0.2 to 100 ms (from `point_time_fields`); `rf_level` above 0x3fff (`14-bit`); the analysis window (`settle` + `window`) ending later than 5 point times + 15 ms (`the analysis window ... must end by`); a window shorter than one IF period.

**Special concerns.** Pure and instant; any thread; no panic (a NaN or zero IF is an error). Safe to call repeatedly. Test: `a_configuration_the_unit_or_the_analysis_cannot_use_is_refused_with_the_reason`.
