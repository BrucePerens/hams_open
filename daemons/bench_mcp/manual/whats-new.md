---
title: What is new in the manual
summary: Manual pages added or changed in each build version.
verified: n/a (changelog)
---

# What is new in the manual

One section per build version (`## <version>`), one line per page: `- <page path>: <what changed>`. Whoever adds or changes a page adds a line under the current version. `bench_manual_whats_new` returns this list.

## 0.1.0

- instruments/radioctl: the `hamlib` driver is replaced by the `radioctl` driver (a radioctld started from a rig description; `rig`, `rigs_dir`; `model` and `set_conf` removed), tools renamed `bench_hamlib_*` to `bench_radioctl_*` and `bench_hamlib_rigctld` to `bench_radioctl_daemon`; rotators use a radioctld rotator listener (`radioctld_port`)
- tools/bench_scope_math: channel 1 op channel 2 on the host from one frozen frame
- tools/bench_scope_cursors: values at chosen times and the differences between two cursors, from the waveform memory
- tools/bench_scope_acquire: acquisition mode, average count and memory depth with read-back
- tools/bench_scope_channel: bandwidth limit and inversion with read-back
- tools/bench_scope_display: screen saver, grid and intensity with read-back
- tools/bench_measure: new `statistics` argument (repeated readings summarised on the host)

- tools/bench_vna_time_domain: impulse or step response and time gating of a stored sweep, host only

- instruments/glasgow/logic-analyzer: how the bench_gla tools work on the revision C analyzer (host-side trigger, pre-trigger window and sample rate), the USB link limits with the full-speed rate measured on the unit
- instruments/glasgow/logic-bench-session: the wiring to verify the logic analyzer and the pattern generator on real hardware (no capture has run yet)
- tools/bench_gla_limits: bytes per edge and edges per second the link can carry, as a model
- tools/bench_gla_analyze: trigger window, sample rate, summary, glitch report, VCD, CSV and sigrok files and UART, I2C and SPI decode of a VCD file
- tools/bench_gla_capture: bench_glasgow_logic_capture plus the same analysis
- tools/bench_gla_pattern: square wave, PWM, pulse train and bit pattern planner on top of bench_glasgow_pattern
- tools/bench_gla_safe_state: the Glasgow's safe state, companion of bench_gla_pattern
- instruments/glasgow: the Glasgow Interface Explorer through the pinned glasgow software (all pages plus bench-session) and the 17 tools bench_glasgow_list, _status, _applets, _safe_state, _prepare, _port_set, _i2c_scan, _i2c_read, _i2c_write, _spi_jedec_id, _uart_transact, _uart_sniff, _logic_capture, _gpio, _pattern, _jtag_scan, _swd_idcode; revC3 identify and voltage readback measured on the unit, everything that drives pins fixtures only
- instruments/elad: the ELAD FDM-S2, FDM-DUO and FDM-S1 receivers (overview, setup, operation, limits, troubleshooting, protocol) and tools bench_elad_list, bench_elad_identify, bench_elad_capture, bench_elad_spectrum, bench_elad_level, bench_elad_tune, bench_elad_detect and bench_elad_sweep
- instruments/rx888: the RX-888 MkII receiver through ka9q-radio's radiod (all seven pages) and the tools bench_rx888_status, bench_rx888_frontend, bench_rx888_boot, bench_rx888_radiod_start, bench_rx888_radiod_stop, bench_rx888_tune, bench_rx888_spectrum, bench_rx888_capture; receive only, verified on the unit with no antenna
- instruments/usb-hubs/setup: the example port is labelled RX-888 (the VNWA is disconnected; hub 3-2 port 2 now powers the RX-888)
- concepts/several-instruments: the group form of the config, the `device` argument on every tool, per-instance tiers and locks, shared-device rules; every tool page gains `device`
- instruments/vna/nanovna: the NanoVNA driver (not hardware-verified), and a NanoVNA note on every bench_vna tool page
- procedures/sweep-series-with-retries: the shipped procedure example-sweep-series (repeat a sweep, retry, power-cycle after failures, tabulate the spread)
- procedures/soak-round: the shipped procedure example-soak-round (generator steps read on the scope and the VNA against a baseline)
- procedures/calibration-with-operator-steps: the shipped procedure example-calibration-sequence (the VNA calibration with operator prompts)
- concepts/procedures: the procedure runner (programs that call bench tools in loops, next to the server) and the tools bench_proc_run, bench_proc_status, bench_proc_output, bench_proc_result, bench_proc_cancel, bench_proc_list, bench_proc_save, bench_proc_load, bench_proc_delete, bench_proc_input (safe state, instrument reservations, saved procedures, operator steps, restart recovery)
- instruments/sound: operation, limits, troubleshooting, protocol and measurement recipes pages
- instruments/usb-hubs: protocol page
- instruments/bladerf: protocol page
- instruments/limesdr: protocol page
- concepts/extending: a new driver is a table row in config.rs and one in main.rs (DRIVER_KINDS, MODULES), not eleven edits
- instruments/rotator: antenna rotators through rotctld (overview, setup, operation, limits, troubleshooting, protocol) and the tools bench_rotator_status, bench_rotator_point, bench_rotator_stop, bench_rotator_park
- tools/bench_audio_safe_state: new, stops this server's playback and lowers outputs above their cap
- tools/bench_audio_play: dry_run on every sound-making tool, and the system default device is never used
- tools/bench_scope_self_cal: dry_run
- instruments/radioctl: bench_radioctl_watch (the dial by polling); stopping the daemon of a rig that may be keyed releases it first or is refused
- instruments/radioctl: transmit into an attested dummy load (bench_radioctl_tx_key_test, bench_radioctl_tx_power_sweep, bench_radioctl_safe_state), the transmit watchdog process, the keyed-interval log, the safety model and its limits; procedures/test-a-transmitter-into-a-dummy-load
- tools/bench_psu_output: now a full-tier tool with dry_run, confirm_load_connected and a switch-off timer (for_s, max_on_s)
- tools/bench_psu_safe_state: new, switches the supply output off and reads it back
- instruments/radioctl: reference stations (bench_radioctl_stations, bench_radioctl_station_set), receiver test with audio (bench_radioctl_rx_check), the dummy-load plausibility check and baseline (bench_radioctl_load_check, bench_radioctl_load_baseline), the transmit attestation in setup, measurement recipes
- procedures/check-a-receiver: read a rig, listen on a time station, judge the audio chain
- instruments/radioctl: receiver control tools bench_radioctl_set, bench_radioctl_memory, bench_radioctl_scan and the connection test bench_radioctl_diagnose
- instruments/radioctl: radioctl rigs through radioctld (overview, setup, operation, limits, troubleshooting, protocol) and the tools bench_radioctl_models, bench_radioctl_rigs, bench_radioctl_status, bench_radioctl_get, bench_radioctl_daemon
- concepts/extending: the contributor guide (what belongs, before you code, anatomy, standing safety rules, provenance, checks, documentation, submitting, worked examples)
- procedures/add-an-instrument: step by step from the skeleton to the pull request
- instruments/example: the template instrument (overview, setup, operation, limits, troubleshooting, protocol) and its four tools, the model for a new driver's pages
- index: the manual and its quickstart
- concepts: safety tiers, configuration, units, errors, reserved devices, verification, several instruments, etiquette
- tools/bench_manual_index: the manual tools and the pages of every other tool, with generated argument tables
- instruments/usb-hubs: USB hub tools (overview, setup, operation, limits, troubleshooting)
- instruments/sound: sound device tools (overview, setup, safety, discovery)
- instruments/vna: monitoring and recovery
- instruments/scope: overview, setup, operation, measurement recipes, limits, troubleshooting, protocol
- instruments/psu: overview, setup, operation, limits, troubleshooting, protocol
- instruments/bladerf: overview, setup, operation, limits, troubleshooting
- instruments/limesdr: overview, setup, operation, limits, troubleshooting
- instruments/vna: overview, setup, operation, measurement recipes, limits, protocol
- procedures: calibrate the VNA, verify a generator with the scope, measure a filter, power-cycle a stuck device, soak run, check a sound was made
- instruments/vna/sound-server-protection: new page; bench_status reports codecs_protected_from_sound_server, the detail and the fix
- instruments/vna/modulation: new page; tools/bench_vna_generator_am (frequency, depth, level), tools/bench_vna_generator_fm (new), tools/bench_vna_generator_off (power cycles a modulating unit)
- concepts/reserved-devices: the audio tools reserve a 08bb:29c0 card only beside the VNWA control chip; the VNA's power cycles run the hub module's implementation
- instruments/vna/spectrum-analyzer: new page, the uncalibrated spectrum analyzer and what is not verified
- tools/bench_vna_spectrum: new tool page
- instruments/vna/troubleshooting: a dead TX path (all four channels dead) is a cabling problem first; tools/bench_vna_sweep: the same in its error text
- instruments/sa: the Siglent SSA3000X spectrum analyzer with tracking generator (overview, setup, operation, limits, troubleshooting, protocol), and tools/bench_sa_status, _sweep, _marker, _screenshot, _tg_on, _tg_off, _tg_level, _tg_normalize, _tg_s21
- procedures/verify-a-radio-with-the-bench-runner: the verified line no longer reads as a hardware claim under `code only`
- instruments/sa: the Siglent SSA3000X spectrum analyzer with tracking generator (overview, setup, operation, limits, troubleshooting, protocol), and tools/bench_sa_status, _sweep, _marker, _screenshot, _tg_on, _tg_off, _tg_level, _tg_normalize, _tg_s21
- procedures/verify-a-radio-with-the-bench-runner: the verified line no longer reads as a hardware claim under `code only`
