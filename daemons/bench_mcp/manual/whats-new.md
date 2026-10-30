---
title: What is new in the manual
summary: Manual pages added or changed in each build version.
verified: n/a (changelog)
---

# What is new in the manual

One section per build version (`## <version>`), one line per page: `- <page path>: <what changed>`. Whoever adds or changes a page adds a line under the current version. `bench_manual_whats_new` returns this list.

## 0.1.0

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
- instruments/hamlib: bench_hamlib_watch (the dial by polling); stopping the daemon of a rig that may be keyed releases it first or is refused
- instruments/hamlib: transmit into an attested dummy load (bench_hamlib_tx_key_test, bench_hamlib_tx_power_sweep, bench_hamlib_safe_state), the transmit watchdog process, the keyed-interval log, the safety model and its limits; procedures/test-a-transmitter-into-a-dummy-load
- tools/bench_psu_output: now a full-tier tool with dry_run, confirm_load_connected and a switch-off timer (for_s, max_on_s)
- tools/bench_psu_safe_state: new, switches the supply output off and reads it back
- instruments/hamlib: reference stations (bench_hamlib_stations, bench_hamlib_station_set), receiver test with audio (bench_hamlib_rx_check), the dummy-load plausibility check and baseline (bench_hamlib_load_check, bench_hamlib_load_baseline), the transmit attestation in setup, measurement recipes
- procedures/check-a-receiver: read a rig, listen on a time station, judge the audio chain
- instruments/hamlib: receiver control tools bench_hamlib_set, bench_hamlib_memory, bench_hamlib_scan and the connection test bench_hamlib_diagnose
- instruments/hamlib: Hamlib rigs through rigctld (overview, setup, operation, limits, troubleshooting, protocol) and the tools bench_hamlib_models, bench_hamlib_rigs, bench_hamlib_status, bench_hamlib_get, bench_hamlib_rigctld
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
