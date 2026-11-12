# Data structures

### `LoadCheck` [@ANCHOR: bench_mcp:bench_mcp_radioctl_attest_load_check]

**Purpose and location.** Settings of the dummy-load plausibility check the owner attests to: how many readings per frequency, how much above the baseline counts as a signal and how far readings may fluctuate. The defaults are the safe ones. Defined in `daemons/bench_mcp/src/radioctl/attest.rs:30` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `margin_db` | `f64` | A reading this many dB above the baseline (after the spread allowance) counts as a signal. |
| `audio_margin_db` | `f64` | The same margin for the audio level. |
| `max_spread_db` | `f64` | Readings that fluctuate more than this many dB at one frequency cannot show a difference. |
| `readings` | `usize` | S-meter readings taken per frequency. |
| `dwell_ms` | `f64` | Settling time in milliseconds before each frequency is read. |
| `references` | `usize` | Reference frequencies listened to besides the transmit frequency. |
| `af` | `f64` | Audio gain (AF level, 0 to 1) set during the check so that audio levels compare. |
| `preamp_db` | `Option<f64>` | Preamp setting in dB used during the check, if the rig has one. |

**Invariants, ownership and concurrency.** Plain value read from the owner's configuration and cloned into each check; not shared or mutated. It decides whether a keyed test may proceed, so loosening a value weakens a transmit safety check.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/radioctl/attest.rs:49), `settings_record` (src/radioctl/load.rs:420), `th` (src/radioctl/load.rs:506).

**See also:** test `a_transmit_block_needs_the_attestation_and_every_limit_and_nothing_defaults_wider (src/radioctl/attest.rs)`.

### `Transmit` [@ANCHOR: bench_mcp:bench_mcp_radioctl_attest_transmit]

**Purpose and location.** The owner's written attestation that allows the keyed (transmit) tools of this server for one rig: who attested, when, the cabling, and the limits on power, time, frequency, mode and SWR. Without it nothing is keyed. Defined in `daemons/bench_mcp/src/radioctl/attest.rs:64` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `attested_by` | `String` | Name of the person who attested. |
| `attested_on` | `String` | Date of the attestation as text. |
| `cabling` | `String` | Description of what the rig is connected to (the dummy load). |
| `max_power_w` | `f64` | Highest power in watts any keyed tool may use. |
| `max_seconds` | `f64` | Longest keyed interval in seconds. |
| `frequency_ranges` | `Vec<(f64, f64)>` | Allowed transmit ranges as (low_hz, high_hz) pairs; a frequency outside all of them is refused. |
| `allowed_modes` | `Vec<String>` | Modes a keyed test may use. |
| `max_swr` | `f64` | A standing-wave ratio above this at the low-power probe refuses the test. |
| `swr_probe` | `bool` | Whether a low-power probe is keyed first to read the SWR. |
| `swr_probe_s` | `f64` | Length of that probe in seconds. |
| `allow_lowering_power` | `bool` | May a tool lower the rig's power setting to the requested level (the old setting is restored)? |
| `operator_alert` | `bool` | Sound an alert for the operator (through the audio module) before keying; if it cannot be heard, nothing is sent. |
| `load` | `LoadCheck` | The dummy-load plausibility settings. |

**Invariants, ownership and concurrency.** Read-only configuration value; cloned freely. It is the authority the keyed tools check every request against (`preflight`), together with the rig's own transmit ranges. It carries no secret.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_transmit` (src/radioctl/attest.rs:96), `transmit` (src/radioctl/loadtools.rs:16), `attested` (src/radioctl/txtools.rs:59), `check_args` (src/radioctl/txtools.rs:118), `preflight` (src/radioctl/txtools.rs:201), `plan_json` (src/radioctl/txtools.rs:363).

**See also:** test `a_transmit_block_needs_the_attestation_and_every_limit_and_nothing_defaults_wider (src/radioctl/attest.rs)`.

### `PipeWireInput` [@ANCHOR: bench_mcp:bench_mcp_radioctl_audio_in_pipe_wire_input]

**Purpose and location.** The production audio source for the load check: captures a few seconds from one PipeWire input node through `Recorder`. Defined in `daemons/bench_mcp/src/radioctl/audio_in.rs:24` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `node` | `String` | The PipeWire node name of the input to record. |

**Invariants, ownership and concurrency.** Implements `AudioSource`; each `capture` starts a `pw-record` child and ends it; the length must be 0.1 to 30 seconds. No state is kept between captures.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `resolve` (src/radioctl/audio_in.rs:54).

**See also:** test `a_capture_has_bounds_and_a_missing_node_is_an_error_not_a_fall_back_to_the_default_input (src/radioctl/audio_in.rs)`.

### `TxRange` [@ANCHOR: bench_mcp:bench_mcp_radioctl_control_tx_range]

**Purpose and location.** A transmit range as the rig's `dump_state` gives it: (low Hz, high Hz, lowest power mW, highest power mW). Defined in `daemons/bench_mcp/src/radioctl/control.rs:77` as a pub `type`. It derives no traits.

**Definition.** `pub type TxRange = (f64, f64, f64, f64);` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** A tuple alias; parsed from the dump by `parse_dump_ranges`, and used to check that a requested transmit frequency is inside a range the rig itself reports.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_dump_ranges` (src/radioctl/control.rs:80).

**See also:** test `dump_state_ranges_are_read_until_the_zero_lines (src/radioctl/control.rs)`.

### `Plan` [@ANCHOR: bench_mcp:bench_mcp_radioctl_control_plan]

**Purpose and location.** Everything one `bench_radioctl_set` call will do, validated before anything is sent to the rig. Defined in `daemons/bench_mcp/src/radioctl/control.rs:159` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `freq` | `Option<f64>` | New frequency in hertz, if requested. |
| `mode` | `Option<(String, f64)>` | New mode and passband width in hertz, if requested. |
| `vfo` | `Option<String>` | VFO to select, if requested. |
| `split` | `Option<(bool, Option<String>)>` | Split on or off and the transmit VFO, if requested. |
| `rit` | `Option<f64>` | RIT offset in hertz, if requested. |
| `xit` | `Option<f64>` | XIT offset in hertz, if requested. |
| `levels` | `Vec<(String, f64)>` | Level settings as (name, value) pairs. |
| `funcs` | `Vec<(String, bool)>` | Function switches as (name, on or off) pairs. |

**Invariants, ownership and concurrency.** Built by the argument parser, consumed by the executor; private and not shared. It is built so that a refused request sends nothing. Whether any entry can key the transmitter is decided by the keyed tools, not here.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `plan` (src/gla/pattern.rs:56), `backend_args` (src/gla/pattern.rs:169), `plan` (src/glasgow/ops.rs:141), `done` (src/glasgow/ops.rs:735), `plan` (src/radioctl/control.rs:170).

**See also:** test `set_arguments_are_checked_before_anything_is_sent_and_transmit_side_settings_are_refused (src/radioctl/tests.rs)`.

### `ToneHit` [@ANCHOR: bench_mcp:bench_mcp_radioctl_detect_tone_hit]

**Purpose and location.** One steady tone found in a recording. Defined in `daemons/bench_mcp/src/radioctl/detect.rs:32` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `hz` | `f64` | Frequency in hertz. |
| `level_dbfs` | `f64` | Level in dBFS. |
| `over_floor_db` | `f64` | Level above the noise floor in dB. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tone_hits` (src/radioctl/detect.rs:113).

**See also:** test `minute_tones_are_found_by_their_narrow_peak_and_broadband_noise_is_not_a_tone (src/radioctl/detect.rs)`.

### `TickHit` [@ANCHOR: bench_mcp:bench_mcp_radioctl_detect_tick_hit]

**Purpose and location.** One once-per-second tick found in a recording (a time signal). Defined in `daemons/bench_mcp/src/radioctl/detect.rs:39` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `hz` | `f64` | Carrier or tone frequency of the tick in hertz. |
| `phase_s` | `f64` | Where in the second the tick falls. |
| `contrast_db` | `f64` | How far the tick stands above its surroundings in dB. |
| `consistent` | `f64` | The fraction of the seconds in which the tick fell at that phase. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tick_train` (src/radioctl/detect.rs:152).

**See also:** test `a_one_second_tick_train_is_found_with_and_without_noise_and_silence_has_none (src/radioctl/detect.rs)`.

### `Findings` [@ANCHOR: bench_mcp:bench_mcp_radioctl_detect_findings]

**Purpose and location.** What the detector found in one recording: whether it was digital silence, its RMS level, tones, ticks, beat notes and mains hum. Defined in `daemons/bench_mcp/src/radioctl/detect.rs:49` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `all_zero` | `bool` | Every sample was exactly zero (a dead input). |
| `rms_dbfs` | `f64` | Overall RMS level in dBFS. |
| `tones` | `Vec<ToneHit>` | The steady tones found. |
| `ticks` | `Vec<TickHit>` | The ticks found. |
| `beats_hz` | `Vec<f64>` | Narrow steady peaks that are not mains harmonics or station tones: carriers heard as beat notes, receiver birdies. |
| `mains_hum_hz` | `Option<f64>` | The mains hum frequency (50 or 60 Hz family) if one was found. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `analyze` (src/radioctl/detect.rs:221), `of` (src/radioctl/load.rs:46).

**See also:** test `analysis_reports_silence_and_dead_inputs_and_a_full_station (src/radioctl/detect.rs)`.

### `Word` [@ANCHOR: bench_mcp:bench_mcp_radioctl_guard_word]

**Purpose and location.** One message from the keyed tool to its watchdog, read from the watchdog's standard input. Defined in `daemons/bench_mcp/src/radioctl/guard.rs:63` as a pub `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Line` | `Line(String)` | A line of text (for example `done` when the tool has finished). |
| `Closed` | `Closed` | The pipe closed, which the watchdog treats like the tool having died. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_watchdog_unkeys_on_its_deadline_on_a_closed_pipe_and_on_abort_and_stands_down_on_done (src/radioctl/tests.rs)`.

### `ProcessSpawner` [@ANCHOR: bench_mcp:bench_mcp_radioctl_guard_process_spawner]

**Purpose and location.** The production starter of the transmit watchdog: runs this same program again as a separate process that will un-key the rig if the tool stops talking. Defined in `daemons/bench_mcp/src/radioctl/guard.rs:210` as a pub `struct`. It derives no traits.

**Fields.** none (unit or opaque definition).

**Invariants, ownership and concurrency.** Unit struct implementing `Spawner`; tests replace it. The child is started in its own process group (`process_group(0)`), so a signal sent to the tool's group does not reach it.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_watchdog_that_does_not_report_armed_stops_the_keying_before_it_starts (src/radioctl/tests.rs)`.

### `ProcessWatch` [@ANCHOR: bench_mcp:bench_mcp_radioctl_guard_process_watch]

**Purpose and location.** The handle on a running watchdog process. Defined in `daemons/bench_mcp/src/radioctl/guard.rs:212` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `child` | `std::process::Child` | The watchdog child process. |
| `stdin` | `Option<std::process::ChildStdin>` | Its standard input, through which the tool sends `done`; `None` once handed over. |

**Invariants, ownership and concurrency.** Implements `Watch`; the tool either tells it `done` or leaves it to un-key the rig (`leave_to_guard`). This is the safety backstop of every keyed interval.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_watchdog_unkeys_on_its_deadline_on_a_closed_pipe_and_on_abort_and_stands_down_on_done (src/radioctl/tests.rs)`.

### `Reading` [@ANCHOR: bench_mcp:bench_mcp_radioctl_load_reading]

**Purpose and location.** Everything measured at one frequency during the load check. Defined in `daemons/bench_mcp/src/radioctl/load.rs:24` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | The station's id, or `tx` for the transmit frequency. |
| `frequency_hz` | `f64` | Frequency in hertz. |
| `mode` | `String` | The mode used. |
| `mean_db` | `f64` | Mean signal strength in dB relative to S9. |
| `spread_db` | `f64` | Highest minus lowest reading in dB. |
| `squelch_open` | `Option<bool>` | Whether the squelch opened, if it could be read. |
| `audio` | `Option<AudioObs>` | Audio observations, if audio was captured. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_json` (src/radioctl/load.rs:66), `r` (src/radioctl/load.rs:510), `base` (src/radioctl/load.rs:532).

**See also:** test `readings_at_the_baseline_are_consistent_and_each_kind_of_excess_refuses (src/radioctl/load.rs)`.

### `AudioObs` [@ANCHOR: bench_mcp:bench_mcp_radioctl_load_audio_obs]

**Purpose and location.** What the audio of one reading showed. Defined in `daemons/bench_mcp/src/radioctl/load.rs:37` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `rms_dbfs` | `f64` | RMS level in dBFS. |
| `all_zero` | `bool` | Every sample was zero. |
| `time_station` | `bool` | A time-signal station was recognised. |
| `beats_hz` | `Vec<f64>` | Frequencies of beat notes found, in hertz. |
| `detail` | `Value` | The detector's full findings as JSON. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `of` (src/radioctl/load.rs:46), `audio` (src/radioctl/load.rs:522).

**See also:** test `audio_evidence_a_station_heard_loud_audio_a_new_carrier_note_and_a_dead_input (src/radioctl/load.rs)`.

### `Baseline` [@ANCHOR: bench_mcp:bench_mcp_radioctl_load_baseline]

**Purpose and location.** What was recorded when the owner attested the load: the readings at attest time, to compare later readings with. Defined in `daemons/bench_mcp/src/radioctl/load.rs:102` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `taken_on` | `String` | When the baseline was taken, as text. |
| `attested_by` | `String` | Who attested it. |
| `cabling` | `String` | The cabling attested. |
| `settings` | `Value` | The rig settings used, as JSON. |
| `readings` | `Vec<Reading>` | The readings. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `load_baseline` (src/radioctl/load.rs:255), `save_baseline` (src/radioctl/load.rs:289), `base` (src/radioctl/load.rs:532).

**See also:** test `a_baseline_is_stored_per_range_and_read_back_exactly_and_damage_is_an_error (src/radioctl/load.rs)`.

### `Verdict` [@ANCHOR: bench_mcp:bench_mcp_radioctl_load_verdict]

**Purpose and location.** The outcome of comparing current readings with the baseline. Defined in `daemons/bench_mcp/src/radioctl/load.rs:111` as a pub `enum`. It derives Clone, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Consistent` | `Consistent` | The readings match the baseline: consistent with a dummy load, which is not proof. |
| `Refuse` | `Refuse` | Something is above the baseline, a station was heard, or a measurement failed: do not transmit. |
| `NoBaseline` | `NoBaseline` | There is no baseline for this range: readings are only reported; transmitting needs the owner to record one. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `check_device` (src/usb_hub/protect.rs:241), `check_port` (src/usb_hub/protect.rs:271).

**See also:** test `readings_at_the_baseline_are_consistent_and_each_kind_of_excess_refuses (src/radioctl/load.rs)`.

### `Decision` [@ANCHOR: bench_mcp:bench_mcp_radioctl_load_decision]

**Purpose and location.** The verdict plus the reasons for it. Defined in `daemons/bench_mcp/src/radioctl/load.rs:132` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `verdict` | `Verdict` | Consistent, Refuse or NoBaseline. |
| `reasons` | `Vec<String>` | One sentence per reason, in words for the operator. |
| `compared` | `usize` | How many readings were compared with the baseline. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `readings_at_the_baseline_are_consistent_and_each_kind_of_excess_refuses (src/radioctl/load.rs)`.

### `Original` [@ANCHOR: bench_mcp:bench_mcp_radioctl_load_original]

**Purpose and location.** What the rig was set to before the load check (frequency, mode, levels), so the check can put it back. Defined in `daemons/bench_mcp/src/radioctl/load.rs:318` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `freq` | `f64` | Frequency in hertz. |
| `mode` | `Option<(String, f64)>` | Mode and passband width, if it could be read. |
| `levels` | `Vec<(&'static str, f64)>` | Level settings as (name, value) pairs. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `read_original` (src/radioctl/load.rs:324), `restore` (src/radioctl/load.rs:342).

**See also:** test `the_receiver_check_reports_signal_against_audio_and_the_tone_offset_and_restores_the_rig (src/radioctl/tests.rs)`.

### `Measured` [@ANCHOR: bench_mcp:bench_mcp_radioctl_load_measured]

**Purpose and location.** The raw result of a load measurement before judging: the readings, the rig settings used, the stations used, and any setting that could not be restored. Defined in `daemons/bench_mcp/src/radioctl/load.rs:424` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `readings` | `Vec<Reading>` | The readings. |
| `settings` | `Value` | The rig settings used as JSON. |
| `stations_used` | `Vec<Station>` | The stations listened to. |
| `restore_failed` | `Vec<String>` | Names of settings that could not be put back (the owner must check them). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `note_heard` (src/radioctl/load.rs:487), `baseline_problems` (src/radioctl/loadtools.rs:169).

**See also:** test `the_load_check_passes_at_the_baseline_and_refuses_a_signal_a_station_a_dead_input_and_a_broken_meter (src/radioctl/tests.rs)`.

### `Prepared` [@ANCHOR: bench_mcp:bench_mcp_radioctl_loadtools_prepared]

**Purpose and location.** A validated request for the load tools: the attestation, the range the frequency falls in and the managed directory. Defined in `daemons/bench_mcp/src/radioctl/loadtools.rs:68` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `tx` | `&'a Transmit` | The attestation (borrowed). |
| `range` | `(f64, f64)` | The (low, high) attested range the frequency lies in. |
| `dir` | `PathBuf` | The managed directory holding baselines and logs. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `prepare` (src/radioctl/loadtools.rs:74).

**See also:** test `the_baseline_needs_the_confirmation_the_attested_range_and_a_quiet_path (src/radioctl/tests.rs)`.

### `Rig` [@ANCHOR: bench_mcp:bench_mcp_radioctl_models_rig]

**Purpose and location.** What the head of a rig description file says about the rig. Defined in `daemons/bench_mcp/src/radioctl/models.rs:11` as a pub `struct`. It derives Clone, Debug, Default, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | The model id. |
| `name` | `String` | The model name. |
| `family` | `String` | The model family. |
| `capabilities` | `Vec<String>` | Capability names listed in the description. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `rig` (src/glasgow/tests.rs:26), `parse_description` (src/radioctl/models.rs:23), `list` (src/radioctl/models.rs:71), `preflight` (src/radioctl/txtools.rs:201), `restore` (src/radioctl/txtools.rs:303), `plan_json` (src/radioctl/txtools.rs:363).

**See also:** test `return_codes_have_names_and_a_next_step (src/radioctl/proto.rs)`.

### `RigError` [@ANCHOR: bench_mcp:bench_mcp_radioctl_proto_rig_error]

**Purpose and location.** Why a command to the rig daemon did not give a value. Defined in `daemons/bench_mcp/src/radioctl/proto.rs:14` as a pub `enum`. It derives Clone, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Link` | `Link(String)` | The socket failed, timed out or closed, or the reply was not in the protocol's form: nothing is known about the rig. |
| `Rig` | `Rig { code: i32, cmd: String }` | The daemon answered `RPRT <code>` with a negative code: the rig or backend refused or could not do it; carries the code and the command. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `apply` (src/radioctl/control.rs:312), `number` (src/radioctl/proto.rs:106), `connect` (src/radioctl/proto.rs:150), `reopen` (src/radioctl/proto.rs:166), `command` (src/radioctl/proto.rs:186), `exchange` (src/radioctl/proto.rs:203), `read` (src/radioctl/proto.rs:264), `dump_state` (src/radioctl/proto.rs:277).

**See also:** test `return_codes_have_names_and_a_next_step (src/radioctl/proto.rs)`.

### `Reply` [@ANCHOR: bench_mcp:bench_mcp_radioctl_proto_reply]

**Purpose and location.** The text of a reply: the lines between the echo of the command and `RPRT`. Defined in `daemons/bench_mcp/src/radioctl/proto.rs:87` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `lines` | `Vec<String>` | The lines in order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serve` (src/example/testkit.rs:32), `handle` (src/example/testkit.rs:147), `command` (src/radioctl/proto.rs:186), `exchange` (src/radioctl/proto.rs:203), `read` (src/radioctl/proto.rs:264), `dump_state` (src/radioctl/proto.rs:277), `ok` (src/radioctl/testkit.rs:25), `err` (src/radioctl/testkit.rs:30).

**See also:** test `reply_helpers_read_keys_and_bare_values_and_refuse_garbage (src/radioctl/proto.rs)`.

### `Client` [@ANCHOR: bench_mcp:bench_mcp_radioctl_proto_client]

**Purpose and location.** A client of the rig daemon's text protocol: host, port, timeout and a connection opened on first use. Defined in `daemons/bench_mcp/src/radioctl/proto.rs:142` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `host` | `String` | The daemon's host. |
| `port` | `u16` | The daemon's TCP port. |
| `timeout` | `Duration` | Socket timeout for each command. |
| `conn` | `Option<(TcpStream, BufReader<TcpStream>)>` | The open connection (a stream and its reader), or `None` before the first command or after a failure. |

**Invariants, ownership and concurrency.** Owned by one tool call and used from one thread; `command` takes `&mut self`. Commands that key the transmitter are sent only by the keyed tools after their checks.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `rx_ranges` (src/radioctl/control.rs:67), `ptt_is_on` (src/radioctl/control.rs:150), `set` (src/radioctl/control.rs:334), `memory` (src/radioctl/control.rs:431), `scan` (src/radioctl/control.rs:471), `read_original` (src/radioctl/load.rs:324), `restore` (src/radioctl/load.rs:342), `open` (src/radioctl/mod.rs:451).

**See also:** test `a_command_gets_its_reply_and_a_rig_error_is_an_error_with_a_next_step (src/radioctl/tests.rs)`.

### `Slot` [@ANCHOR: bench_mcp:bench_mcp_radioctl_rigd_slot]

**Purpose and location.** The bookkeeping for one rig's daemon that this server started. Defined in `daemons/bench_mcp/src/radioctl/rigd.rs:22` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `child` | `Option<Child>` | The daemon process, if running. |
| `port` | `u16` | The TCP port it listens on. |
| `starts` | `Vec<Instant>` | When it was started recently; more than `MAX_STARTS` within `WINDOW` is refused so a crashing daemon is not restarted forever. |
| `last_exit` | `Option<String>` | Text about how it last ended, for error reports. |

**Invariants, ownership and concurrency.** Lives in the process-wide `STATE` map (a mutex around a map keyed by rig name), so access is serialized; the mutex is taken poison-tolerantly.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_managed_daemon_starts_on_demand_stops_on_request_and_starts_again (src/radioctl/tests.rs)`.

### `Station` [@ANCHOR: bench_mcp:bench_mcp_radioctl_stations_station]

**Purpose and location.** One reference station the load check can listen to: a time station, an AM broadcast band station, weather or a local one. Defined in `daemons/bench_mcp/src/radioctl/stations.rs:21` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | Short id used in readings. |
| `name` | `String` | Human name. |
| `frequency_hz` | `f64` | Frequency in hertz. |
| `mode` | `String` | Mode to listen in. |
| `audio` | `String` | How to recognise it in audio (a short text). |
| `kind` | `String` | `time_station`, `am_band`, `weather` or `local`. |
| `note` | `String` | Free-text note. |
| `last_heard` | `Option<String>` | When it was last heard, if ever. |
| `retired` | `bool` | Taken out of use. |
| `user_added` | `bool` | Added by the owner, not part of the built-in table. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `audio_seconds` (src/radioctl/load.rs:410), `st` (src/radioctl/stations.rs:100), `defaults` (src/radioctl/stations.rs:116), `from_json` (src/radioctl/stations.rs:185), `check` (src/radioctl/stations.rs:211), `get_mut` (src/radioctl/stations.rs:312).

**See also:** test `the_shipped_table_lists_wwv_wwvh_am_and_weather_but_not_chu_and_every_entry_is_valid (src/radioctl/stations.rs)`.

### `Table` [@ANCHOR: bench_mcp:bench_mcp_radioctl_stations_table]

**Purpose and location.** The station table: the built-in stations plus the owner's changes, stored in the managed directory. Defined in `daemons/bench_mcp/src/radioctl/stations.rs:257` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `stations` | `Vec<Station>` | All stations. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `note_heard` (src/radioctl/load.rs:487), `load` (src/radioctl/stations.rs:263).

**See also:** test `references_follow_the_band_the_rig_coverage_and_the_time_of_day (src/radioctl/stations.rs)`.

### `Read` [@ANCHOR: bench_mcp:bench_mcp_radioctl_status_read]

**Purpose and location.** The result type of a status read: a link failure is the outer error, a refusal by the rig is data in the inner result. Defined in `daemons/bench_mcp/src/radioctl/status.rs:71` as a pub `type`. It derives no traits.

**Definition.** `pub type Read<T> = Result<Result<T, RigError>, String>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `drain` (src/glasgow/runner.rs:85), `tolerate` (src/radioctl/status.rs:73), `mode_of` (src/radioctl/status.rs:95).

**See also:** test `bounded_refuses_what_is_out_of_range_or_not_a_number (src/radioctl/status.rs)`.

### `Reply` [@ANCHOR: bench_mcp:bench_mcp_radioctl_testkit_reply]

**Purpose and location.** What the fake rig daemon of the tests does with one command. Defined in `daemons/bench_mcp/src/radioctl/testkit.rs:15` as a pub `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Rprt` | `Rprt(Vec<String>, i32)` | Answer with these lines and a return code (after the echo). |
| `Raw` | `Raw(String)` | Send exactly this text (no echo and no RPRT, as `chk_vfo` has). |
| `Silent` | `Silent` | Do not answer (to test timeouts). |
| `Hangup` | `Hangup` | Close the connection. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serve` (src/example/testkit.rs:32), `handle` (src/example/testkit.rs:147), `command` (src/radioctl/proto.rs:186), `exchange` (src/radioctl/proto.rs:203), `read` (src/radioctl/proto.rs:264), `dump_state` (src/radioctl/proto.rs:277), `ok` (src/radioctl/testkit.rs:25), `err` (src/radioctl/testkit.rs:30).

**See also:** test `reply_helpers_read_keys_and_bare_values_and_refuse_garbage (src/radioctl/proto.rs)`.

### `Dir` [@ANCHOR: bench_mcp:bench_mcp_radioctl_tests_dir]

**Purpose and location.** A fresh directory for a test's state files and logs; removed when dropped, after stopping any daemon a failed test left behind. Defined in `daemons/bench_mcp/src/radioctl/tests.rs:11` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `0` | `pub std::path::PathBuf` | The directory path. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `resolve` (src/audio/devices.rs:330), `pick` (src/audio/tools.rs:49), `new` (src/radioctl/tests.rs:16), `virtual_bench` (src/radioctl/tests.rs:44), `sysfs` (src/rx888/mod.rs:865), `new` (src/rx888/mod.rs:881), `none` (src/usb_hub/protect.rs:329), `new` (src/usb_hub/testkit.rs:12).

**See also:** test `the_managed_daemon_starts_on_demand_stops_on_request_and_starts_again (src/radioctl/tests.rs)`.

### `Active` [@ANCHOR: bench_mcp:bench_mcp_radioctl_tx_active]

**Purpose and location.** A rig that may be keyed right now, kept so the server can release it when it stops. Defined in `daemons/bench_mcp/src/radioctl/tx.rs:29` as a private `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `host` | `String` | Daemon host. |
| `port` | `u16` | Daemon port. |
| `min_power` | `Option<f64>` | The rig's lowest power setting as a fraction, set at release, if known. |

**Invariants, ownership and concurrency.** Held in the process-wide `ACTIVE` map under a poison-tolerant mutex; entries are added when a rig is keyed and removed when it is released.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_panic_while_keyed_still_releases_the_rig_and_sets_the_minimum_power (src/radioctl/tx.rs)`.

### `Purpose` [@ANCHOR: bench_mcp:bench_mcp_radioctl_tx_purpose]

**Purpose and location.** What a keyed interval is for, written to the log and the state file. Defined in `daemons/bench_mcp/src/radioctl/tx.rs:76` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `tool` | `String` | The tool that keyed. |
| `reason` | `String` | The reason the owner gave. |
| `by` | `String` | Who asked. |
| `frequency_hz` | `f64` | Frequency in hertz. |
| `mode` | `String` | Mode. |
| `power_w` | `f64` | Power in watts. |
| `kind` | `String` | `probe`, `test` or `sweep step`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_panic_while_keyed_still_releases_the_rig_and_sets_the_minimum_power (src/radioctl/tx.rs)`.

### `Sample` [@ANCHOR: bench_mcp:bench_mcp_radioctl_tx_sample]

**Purpose and location.** One set of meter readings during the keyed interval. Defined in `daemons/bench_mcp/src/radioctl/tx.rs:89` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `t_s` | `f64` | Seconds since keying. |
| `power_w` | `Option<f64>` | Power in watts, if the rig reports it. |
| `swr` | `Option<f64>` | Standing-wave ratio, if reported. |
| `alc` | `Option<f64>` | ALC reading, if reported. |
| `temp` | `Option<f64>` | Temperature reading, if reported. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `summary` (src/radioctl/txtools.rs:332).

**See also:** test `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval (src/radioctl/tests.rs)`.

### `KeyedRun` [@ANCHOR: bench_mcp:bench_mcp_radioctl_tx_keyed_run]

**Purpose and location.** The result of one keyed interval: the samples, how long it was keyed, and whether it was cut short. Defined in `daemons/bench_mcp/src/radioctl/tx.rs:111` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `samples` | `Vec<Sample>` | Meter samples in time order. |
| `seconds_keyed` | `f64` | How long the rig was keyed in seconds. |
| `aborted` | `Option<String>` | Why the interval ended early, if it did (for example an SWR or power limit passed). |
| `ptt_read_back` | `bool` | PTT was read back as off after release; false when the rig cannot report PTT. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_key_test_passes_every_gate_probes_keys_releases_restores_and_logs_each_interval (src/radioctl/tests.rs)`.

### `Keying` [@ANCHOR: bench_mcp:bench_mcp_radioctl_tx_keying]

**Purpose and location.** What `key` needs to know to key a rig for one interval: the rig and daemon, the watchdog starter, the purpose, and limits that stop the interval at once when a meter passes them. Defined in `daemons/bench_mcp/src/radioctl/tx.rs:121` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `&'a str` | Rig name. |
| `host` | `&'a str` | Daemon host. |
| `port` | `u16` | Daemon port. |
| `dir` | `&'a Path` | Managed directory for the state file and log. |
| `spawner` | `&'a dyn Spawner` | Starter of the watchdog process. |
| `min_power` | `Option<f64>` | The rig's lowest power setting as a fraction, set at release. |
| `purpose` | `Purpose` | What the interval is for. |
| `limit_swr` | `Option<f64>` | SWR above which the interval stops at once. |
| `limit_power_w` | `Option<f64>` | Power in watts above which the interval stops at once. |
| `previous_rfpower` | `Option<f64>` | The power setting before the test, to put back. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_panic_while_keyed_still_releases_the_rig_and_sets_the_minimum_power (src/radioctl/tx.rs)`.

### `Keyed` [@ANCHOR: bench_mcp:bench_mcp_radioctl_tx_keyed]

**Purpose and location.** The guard of one keyed interval: holds the keying request, the watchdog, and whether PTT was released. Defined in `daemons/bench_mcp/src/radioctl/tx.rs:134` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `k` | `&'a Keying<'a>` | The keying request. |
| `watch` | `Option<Box<dyn Watch>>` | The running watchdog; `None` after it finished or was handed to the guard. |
| `released` | `bool` | Set by `finish` once PTT was released and read back. |

**Invariants, ownership and concurrency.** Private; created when PTT is set and ended by `release`, which sets PTT off, reads it back and falls back to a new connection; if release cannot be confirmed the watchdog is left to un-key the rig and the error tells the operator to switch it off at the rig.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_panic_while_keyed_still_releases_the_rig_and_sets_the_minimum_power (src/radioctl/tx.rs)`.

### `AudioAlert` [@ANCHOR: bench_mcp:bench_mcp_radioctl_txtools_audio_alert]

**Purpose and location.** The operator alert (before keying) sent through the audio module's `bench_audio_alert`; if it cannot be heard nothing is keyed. Defined in `daemons/bench_mcp/src/radioctl/txtools.rs:36` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `insts` | `&'a [Instrument]` | The configured instruments. |
| `instrument` | `String` | Name of the audio instrument to alert on. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_operator_alert_goes_to_the_audio_instrument_and_an_unknown_one_is_an_error_without_sound (src/radioctl/tests.rs)`.

### `Deps` [@ANCHOR: bench_mcp:bench_mcp_radioctl_txtools_deps]

**Purpose and location.** What the keyed tools need besides the rig: the watchdog starter, the optional alert and the optional audio source for the load check. Defined in `daemons/bench_mcp/src/radioctl/txtools.rs:53` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `spawner` | `&'a dyn Spawner` | Starter of the watchdog. |
| `alert` | `Option<&'a dyn Alerter>` | The operator alert, if configured. |
| `audio` | `Option<&'a dyn AudioSource>` | The audio source for the load check, if configured. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `deps` (src/radioctl/tests.rs:1440), `gate` (src/radioctl/txtools.rs:411).

**See also:** test `stopping_the_server_releases_a_rig_that_is_keyed_before_the_daemons_are_stopped (src/radioctl/tests.rs)`.

### `TestArgs` [@ANCHOR: bench_mcp:bench_mcp_radioctl_txtools_test_args]

**Purpose and location.** Arguments of a keyed test, parsed and checked against the attestation. Defined in `daemons/bench_mcp/src/radioctl/txtools.rs:98` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `frequency_hz` | `f64` | Frequency in hertz. |
| `mode` | `String` | Mode. |
| `power_w` | `f64` | Power in watts. |
| `seconds` | `f64` | Length of the keyed interval in seconds. |
| `every_ms` | `f64` | Meter sampling interval in milliseconds. |
| `dry_run` | `bool` | Report the plan and key nothing. |
| `confirm` | `bool` | The caller confirms a dummy load is connected (`confirm_dummy_load`). |
| `reason` | `String` | The reason, written to the log. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `check_args` (src/radioctl/txtools.rs:118), `preflight` (src/radioctl/txtools.rs:201), `plan_json` (src/radioctl/txtools.rs:363), `gate` (src/radioctl/txtools.rs:411).

**See also:** test `a_test_is_refused_above_the_power_cap_outside_the_ranges_in_a_mode_not_allowed_or_too_long (src/radioctl/tests.rs)`.

### `Rig` [@ANCHOR: bench_mcp:bench_mcp_radioctl_txtools_rig]

**Purpose and location.** What the rig said before anything was changed: its power-setting limits and the settings to put back. Defined in `daemons/bench_mcp/src/radioctl/txtools.rs:190` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `f_min` | `f64` | The lowest power setting the rig reports, as a fraction. |
| `f_req` | `f64` | The requested power converted to a fraction of full scale. |
| `f_prev` | `f64` | The power setting before the test, as a fraction. |
| `w_prev` | `f64` | The previous power in watts. |
| `w_full` | `f64` | The power in watts at a setting of 1.0. |
| `prev_freq` | `f64` | The frequency before the test, in hertz. |
| `prev_mode` | `Option<(String, f64)>` | The mode and passband before the test, if readable. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `rig` (src/glasgow/tests.rs:26), `parse_description` (src/radioctl/models.rs:23), `list` (src/radioctl/models.rs:71), `preflight` (src/radioctl/txtools.rs:201), `restore` (src/radioctl/txtools.rs:303), `plan_json` (src/radioctl/txtools.rs:363).

**See also:** test `return_codes_have_names_and_a_next_step (src/radioctl/proto.rs)`.
