# Data structures

### `Severity` [@ANCHOR: bench_mcp:bench_mcp_audio_alert_severity_type]

**Purpose and location.** How urgent an audible alert is; it picks the tone pattern (patterns grow with severity and stay short) and, with quiet hours, whether the quiet volume limit applies. Defined in `daemons/bench_mcp/src/audio/alert.rs:21` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Info` | `Info` | An informational alert, the lowest severity. |
| `Warning` | `Warning` | A warning alert, the middle severity. |
| `Urgent` | `Urgent` | An urgent alert; the only severity that is not forced to the lowest volume during quiet hours. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse` (src/audio/alert.rs:28), `alert_volume` (src/audio/alert.rs:59), `pattern` (src/audio/alert.rs:69).

**See also:** test `quiet_hours_give_the_lowest_volume_except_for_urgent (src/audio/alert.rs)`.

### `Stats` [@ANCHOR: bench_mcp:bench_mcp_audio_analysis_stats_type]

**Purpose and location.** Level statistics of one recording: peak and RMS in dBFS, clipping, silence fraction, DC offset and whether every sample was exactly zero. Produced by `stats` and read by the verification and discovery tools. Defined in `daemons/bench_mcp/src/audio/analysis.rs:21` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `samples` | `usize` | Number of samples analysed. |
| `peak_dbfs` | `f64` | Highest absolute sample in dBFS (full scale is 0). |
| `rms_dbfs` | `f64` | Whole-recording RMS in dBFS. |
| `clipped` | `usize` | Samples at or beyond 0.999 of full scale. |
| `silence_fraction` | `f64` | The fraction of 20 ms windows whose RMS is under the silence threshold. |
| `all_zero` | `bool` | Every sample is exactly zero: a muted, disconnected or dead input, not a quiet room. |
| `dc_offset` | `f64` | Mean sample value (DC offset, linear, full scale 1.0). |

**Invariants, ownership and concurrency.** Plain owned value, built once per recording and not shared; cloneable. No invariant is enforced beyond what `stats` computes.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `stats` (src/audio/analysis.rs:42), `stats_json` (src/audio/tools.rs:83).

**See also:** test `digital_silence_clipping_and_silence_fraction_are_counted (src/audio/analysis.rs)`.

### `Spectrum` [@ANCHOR: bench_mcp:bench_mcp_audio_analysis_spectrum_type]

**Purpose and location.** The magnitude spectrum of one recording from an FFT: bin spacing and linear amplitude per bin from 0 Hz to Nyquist. Built by `Spectrum::new` (bad FFT lengths are errors) and used for peak finding and spectrum correlation. Defined in `daemons/bench_mcp/src/audio/analysis.rs:117` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `bin_hz` | `f64` | Width of one bin in hertz (sample rate divided by the FFT length). |
| `amp` | `Vec<f64>` | Amplitude per bin from 0 Hz to Nyquist (linear, full scale 1.0). |

**Invariants, ownership and concurrency.** Plain owned value; `amp[i]` belongs to frequency `i * bin_hz`. Not shared across threads.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `welch` (src/dsp.rs:98), `noise_floor_db` (src/dsp.rs:134), `spectrum` (src/spectrum.rs:99), `peaks` (src/spectrum.rs:133), `spectrum` (src/audio/analysis.rs:138), `peaks` (src/audio/analysis.rs:184), `dominant` (src/audio/analysis.rs:224), `median_db` (src/audio/analysis.rs:229).

**See also:** test `a_tone_between_bins_is_still_found_and_two_tones_are_separate_peaks (src/audio/analysis.rs)`.

### `Peak` [@ANCHOR: bench_mcp:bench_mcp_audio_analysis_peak_type]

**Purpose and location.** One spectral peak: its frequency and level. Returned by `peaks` and `dominant`. Defined in `daemons/bench_mcp/src/audio/analysis.rs:177` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `hz` | `f64` | Peak frequency in hertz (interpolated between bins). |
| `dbfs` | `f64` | Peak amplitude in dBFS. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `peaks` (src/spectrum.rs:133), `peaks` (src/audio/analysis.rs:184), `dominant` (src/audio/analysis.rs:224), `find_peaks` (src/sa/sweep.rs:161), `trace_summary` (src/sa/sweep.rs:358).

**See also:** test `peaks_are_local_maxima_above_the_median_strongest_first_and_not_adjacent (src/sa/sweep.rs)`.

### `Dir` [@ANCHOR: bench_mcp:bench_mcp_audio_devices_dir_type]

**Purpose and location.** Whether a device is an output (a sink) or an input (a source) in PipeWire terms. Defined in `daemons/bench_mcp/src/audio/devices.rs:11` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Output` | `Output` | A playback device (sink). |
| `Input` | `Input` | A capture device (source). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `resolve` (src/audio/devices.rs:328), `pick` (src/audio/tools.rs:48), `new` (src/radioctl/tests.rs:16), `virtual_bench` (src/radioctl/tests.rs:44), `sysfs` (src/rx888/mod.rs:865), `new` (src/rx888/mod.rs:881), `none` (src/usb_hub/protect.rs:329), `new` (src/usb_hub/testkit.rs:12).

**See also:** test `the_recorded_devbox_lists_its_sinks_and_sources_with_defaults (src/audio/devices.rs)`.

### `Device` [@ANCHOR: bench_mcp:bench_mcp_audio_devices_device_type]

**Purpose and location.** One PipeWire audio node as this server sees it, merged from `pw-dump`, ALSA's `/proc/asound` and the configuration: identity, bus and USB path, routing availability, formats and any reservation. Built by `devices::list`; the tools select devices from the list by name, alias or fragment. Defined in `daemons/bench_mcp/src/audio/devices.rs:27` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `id` | `u32` | The PipeWire object id; it changes when the device is re-plugged, so persistent names use `node_name`. |
| `node_name` | `String` | The persistent PipeWire node name (unlike `id`, it survives re-plugging); the name tools use. |
| `description` | `String` | PipeWire's human-readable description of the node. |
| `dir` | `Dir` | Output or input. |
| `card` | `Option<u32>` | ALSA card number, if the node belongs to an ALSA card. |
| `alsa_device` | `Option<u32>` | ALSA device number on that card, if known. |
| `alsa_path` | `Option<String>` | PipeWire's ALSA path string for the node, if any. |
| `bus` | `String` | `usb`, `pci`, `bluetooth`, `network` or whatever PipeWire says. |
| `usb_path` | `Option<String>` | The USB device path as the hub tools write it (`3-2.2.2`), for USB sound cards. |
| `usb_id` | `Option<String>` | `vid:pid` of the USB device (from PipeWire's device properties, and from `/proc/asound/cardN/usbid`). |
| `port_type` | `Option<String>` | The route's port type as PipeWire reports it (for example a jack or HDMI), if any. |
| `availability` | `Option<String>` | The route availability the driver reports for the port: `yes`, `no` (an unplugged jack, an HDMI port without a monitor) or `unknown`. Supporting evidence only. |
| `is_default` | `bool` | True when this node is PipeWire's default for its direction. |
| `state` | `String` | The node state string reported by PipeWire (for example running or idle). |
| `channels` | `Option<u64>` | Channel count, if reported. |
| `rates` | `Vec<u64>` | Sample rates the device supports, in hertz. |
| `formats` | `Vec<String>` | Sample formats the device supports, as PipeWire names them. |
| `reserved` | `Option<String>` | `Some(reason)` when the device belongs to other equipment (the two VNWA codecs, found by USB id beside the VNWA control chip); a reserved device never resolves and is never played or recorded. |
| `alias` | `Option<String>` | The operator's alias from the configuration, if one is set. |

**Invariants, ownership and concurrency.** Plain owned value rebuilt on every listing; nothing caches it except the discovery result, which stores a fingerprint of the list to notice changes. The `reserved` field is a safety rule: tools refuse a reserved device.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `reserved_reason` (src/audio/devices.rs:130), `reserved_reason_in` (src/audio/devices.rs:137), `parse` (src/audio/devices.rs:164), `list` (src/audio/devices.rs:314), `resolve` (src/audio/devices.rs:328), `is_hdmi` (src/audio/discover.rs:278), `fingerprint` (src/audio/discover.rs:301), `changes` (src/audio/discover.rs:360).

**See also:** test `hdmi_ports_without_a_monitor_report_unavailable_as_evidence (src/audio/devices.rs)`.

### `Obs` [@ANCHOR: bench_mcp:bench_mcp_audio_discover_obs_type]

**Purpose and location.** One observation of the audio path discovery: an output played a tone and one input listened, with whether it was heard and how loud. Defined in `daemons/bench_mcp/src/audio/discover.rs:36` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `output` | `String` | Node name of the output that played. |
| `input` | `String` | Node name of the input that recorded. |
| `freq` | `f64` | Tone frequency in hertz that this output was given. |
| `volume` | `f64` | The output volume at the step this observation was made at. |
| `heard` | `bool` | Whether the input heard the tone. |
| `level_dbfs` | `f64` | Level of the tone at the input in dBFS. |
| `snr_db` | `f64` | Tone level over the noise at the same frequency, in dB. |
| `reason` | `String` | Why it counts or not, in words. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `best` (src/audio/discover.rs:84), `conclude` (src/audio/discover.rs:91), `heard` (src/audio/discover.rs:654), `silent` (src/audio/discover.rs:667).

**See also:** test `the_measurement_hears_each_outputs_own_tone_and_not_another_outputs (src/audio/discover.rs)`.

### `InState` [@ANCHOR: bench_mcp:bench_mcp_audio_discover_in_state_type]

**Purpose and location.** What discovery found about one input before and during the run: availability, default flag, errors, mute and noise floor. Defined in `daemons/bench_mcp/src/audio/discover.rs:49` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | Node name of the input. |
| `is_default` | `bool` | Whether it is the default input. |
| `error` | `Option<String>` | The stream could not be opened or delivered no samples. |
| `all_zero` | `bool` | Every sample recorded was exactly zero. |
| `muted` | `Option<bool>` | Whether it is muted, if that could be read. |
| `noise_rms_dbfs` | `f64` | RMS noise of the input in dBFS before the tones. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `conclude` (src/audio/discover.rs:91), `inp` (src/audio/discover.rs:643).

**See also:** test `a_muted_or_all_zero_input_is_dead_and_never_chosen_and_never_proves_a_speaker (src/audio/discover.rs)`.

### `OutState` [@ANCHOR: bench_mcp:bench_mcp_audio_discover_out_state_type]

**Purpose and location.** What discovery found about one output: flags, tone frequency, the highest volume used and errors. Defined in `daemons/bench_mcp/src/audio/discover.rs:61` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | Node name of the output. |
| `is_default` | `bool` | Whether it is the default output. |
| `is_hdmi` | `bool` | Whether it is an HDMI output (chosen only when nothing else was heard). |
| `availability` | `Option<String>` | The route availability the driver reports (yes, no or unknown); supporting evidence only. |
| `freq` | `f64` | The tone frequency given to this output (each output gets its own). |
| `max_volume` | `f64` | The highest volume the tone was played at. |
| `error` | `Option<String>` | Why it could not play, if so; an output that could not play is an error, not silence. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `conclude` (src/audio/discover.rs:91), `out` (src/audio/discover.rs:631).

**See also:** test `a_failed_stream_is_unavailable_and_an_output_that_could_not_play_is_an_error_not_silence (src/audio/discover.rs)`.

### `Conclusion` [@ANCHOR: bench_mcp:bench_mcp_audio_discover_conclusion_type]

**Purpose and location.** The result of discovery in report form: the inputs, outputs and pairs as JSON values, the chosen alert output and verify input with reasons, and any check the operator still has to make by ear. Defined in `daemons/bench_mcp/src/audio/discover.rs:73` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `inputs` | `Vec<Value>` | One JSON entry per input with its state. |
| `outputs` | `Vec<Value>` | One JSON entry per output with its state. |
| `pairs` | `Vec<Value>` | One JSON entry per heard output and input pair. |
| `alert_output` | `Option<String>` | The output chosen for alerts, if one was proven. |
| `alert_output_reason` | `String` | Why that output was (or was not) chosen. |
| `verify_input` | `Option<String>` | The input chosen to verify sound, if one was proven. |
| `verify_input_reason` | `String` | Why that input was chosen. |
| `operator_check` | `Option<String>` | A message asking the operator to listen, when no input could prove an output (for example no inputs at all). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `conclude` (src/audio/discover.rs:91).

**See also:** test `a_speaker_and_a_microphone_are_present_only_because_one_heard_the_other (src/audio/discover.rs)`.

### `Stored` [@ANCHOR: bench_mcp:bench_mcp_audio_discover_stored_type]

**Purpose and location.** The stored result of the last discovery run, written to a file and read by the `auto` selection. It goes stale when the device list changes. Defined in `daemons/bench_mcp/src/audio/discover.rs:321` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `at_epoch` | `u64` | Seconds since the epoch when discovery ran. |
| `fingerprint` | `Vec<String>` | Fingerprint of the device list at that time; a changed list makes the result stale (reserved devices do not count). |
| `alert_output` | `Option<String>` | The output auto selects for alerts. |
| `verify_input` | `Option<String>` | The input auto selects for verification. |
| `raw` | `Value` | The whole report as JSON, round-tripped with the file. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `load` (src/audio/discover.rs:329).

**See also:** test `a_changed_device_list_makes_the_result_stale_and_reserved_devices_do_not (src/audio/discover.rs)`.

### `Recorder` [@ANCHOR: bench_mcp:bench_mcp_audio_engine_recorder_type]

**Purpose and location.** A running `pw-record` child that captures one channel of a PipeWire node as 32-bit float into a shared buffer. Created by `Recorder::start`. Defined in `daemons/bench_mcp/src/audio/engine.rs:20` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `child` | `Child` | The `pw-record` process. |
| `buf` | `Arc<Mutex<Vec<f32>>>` | The samples recorded so far, filled by a reader thread (behind a mutex). |
| `err` | `Arc<Mutex<String>>` | Text of the process's stderr (behind a mutex), for error reports. |
| `readers` | `Vec<JoinHandle<()>>` | The threads that read the child's stdout and stderr. |

**Invariants, ownership and concurrency.** Owns a child process and two threads; the buffer and the error text are shared with the reader threads under mutexes. `stop` ends it and returns the samples; a drop shuts the process down too. No transmit or radio path is involved.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `start` (src/audio/engine.rs:30).

**See also:** test `a_missing_target_fails_and_never_falls_back_to_the_default_device (src/audio/engine.rs)`.

### `Playing` [@ANCHOR: bench_mcp:bench_mcp_audio_engine_playing_type]

**Purpose and location.** A running `pw-play` child playing one file with fallback and reconnect disabled, so the sound goes only to the chosen node. Defined in `daemons/bench_mcp/src/audio/engine.rs:178` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `child` | `Child` | The `pw-play` process. |
| `started` | `Instant` | When playback started. |

**Invariants, ownership and concurrency.** Owns a child process (a drop kills it if it is still running); the command line begins with `PLAY_SIGNATURE`, which `bench_audio_safe_state` uses to find and stop playbacks this server started.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `start` (src/audio/engine.rs:185).

**See also:** test `dropping_a_playback_stops_the_process_that_is_still_running (src/audio/engine.rs)`.

### `Params` [@ANCHOR: bench_mcp:bench_mcp_audio_events_params_type]

**Purpose and location.** Settings of the event detector that finds sounds in a recording. Defined in `daemons/bench_mcp/src/audio/events.rs:9` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `frame_ms` | `f64` | Analysis frame length in milliseconds. |
| `margin_db` | `f64` | How far over the noise reference a frame must be to count as sound. |
| `threshold_dbfs` | `Option<f64>` | A fixed threshold in dBFS instead of noise plus margin. |
| `noise_dbfs` | `Option<f64>` | A separately measured noise reference in dBFS; otherwise the 20th percentile of the frames. |
| `merge_gap_ms` | `f64` | Gaps between sounds shorter than this are merged into one event. |
| `min_event_ms` | `f64` | Events shorter than this are dropped. |
| `tone_hz` | `Option<f64>` | Track the level of one frequency instead of the broadband level. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/audio/events.rs:25), `levels_for` (src/audio/events.rs:91), `detect` (src/audio/events.rs:115), `run_params` (src/procedure/mod.rs:296).

**See also:** test `three_beeps_are_found_with_their_times_levels_and_pitch (src/audio/events.rs)`.

### `Event` [@ANCHOR: bench_mcp:bench_mcp_audio_events_event_type]

**Purpose and location.** One detected sound. Defined in `daemons/bench_mcp/src/audio/events.rs:39` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `onset_s` | `f64` | Start in seconds from the start of the recording. |
| `offset_s` | `f64` | End in seconds from the start of the recording. |
| `peak_dbfs` | `f64` | Peak level in dBFS during the event. |
| `rms_dbfs` | `f64` | RMS level in dBFS during the event. |
| `hz` | `Option<f64>` | The pitch, when the event has a clear spectral peak. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `decode_morse` (src/audio/events.rs:244), `active_samples` (src/audio/verify.rs:258), `ev` (src/audio/verify.rs:533), `evidence` (src/audio/verify.rs:543), `parse_line` (src/usb_hub/history.rs:63), `parse` (src/usb_hub/history.rs:152), `analyze` (src/usb_hub/history.rs:190).

**See also:** test `three_beeps_are_found_with_their_times_levels_and_pitch (src/audio/events.rs)`.

### `Detection` [@ANCHOR: bench_mcp:bench_mcp_audio_events_detection_type]

**Purpose and location.** The result of the event detector: the noise reference and threshold that were used and the events found. Defined in `daemons/bench_mcp/src/audio/events.rs:109` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `noise_dbfs` | `f64` | The noise reference in dBFS. |
| `threshold_dbfs` | `f64` | The threshold in dBFS a frame had to pass. |
| `events` | `Vec<Event>` | The events found, in time order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `detect` (src/audio/events.rs:115).

**See also:** test `digital_silence_gives_no_events_and_a_fixed_threshold_is_honoured (src/audio/events.rs)`.

### `Morse` [@ANCHOR: bench_mcp:bench_mcp_audio_events_morse_type]

**Purpose and location.** Morse code decoded from detected events. Defined in `daemons/bench_mcp/src/audio/events.rs:235` as a pub `struct`. It derives Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `text` | `String` | The decoded text. |
| `dot_s` | `f64` | The estimated dot length in seconds. |
| `wpm` | `f64` | The speed in words per minute implied by the dot length. |
| `symbols` | `String` | The dots, dashes and gaps as symbols. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `decode_morse` (src/audio/events.rs:244).

**See also:** test `keyed_cw_decodes_to_text_with_its_speed (src/audio/events.rs)`.

### `Heard` [@ANCHOR: bench_mcp:bench_mcp_audio_heard_heard_type]

**Purpose and location.** The verdict of the "was this tone heard" check: levels at the tone's frequency compared with the noise and the neighbouring frequencies. Defined in `daemons/bench_mcp/src/audio/heard.rs:13` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `heard` | `bool` | Whether the tone was heard. |
| `hz` | `f64` | The tone frequency checked, in hertz. |
| `level_dbfs` | `f64` | The strongest level of the tone in dBFS (peak amplitude). |
| `noise_dbfs` | `f64` | The level at the same frequency in the part of the recording before the tone was expected. |
| `snr_db` | `f64` | Tone level over the noise level, in dB. |
| `over_neighbours_db` | `f64` | How far the tone stands above the loudest of two neighbouring frequencies during the tone. |
| `onset_s` | `Option<f64>` | When the tone started, if it was heard. |
| `offset_s` | `Option<f64>` | When the tone ended, if it was heard. |
| `input_all_zero` | `bool` | The input delivered only zero samples (a muted or dead input, which proves nothing). |
| `reason` | `String` | The reason for the verdict in words. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `hear_tone` (src/audio/heard.rs:67).

**See also:** test `the_owner_adds_retires_restores_and_removes_and_the_rules_hold (src/radioctl/stations.rs)`.

### `Level` [@ANCHOR: bench_mcp:bench_mcp_audio_levels_level_type]

**Purpose and location.** One device's volume and mute state as `wpctl` reports it. Defined in `daemons/bench_mcp/src/audio/levels.rs:8` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `volume` | `f64` | Linear wpctl volume (1.0 is 100 percent). |
| `muted` | `bool` | Whether the node is muted. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_get_volume` (src/audio/levels.rs:14), `get` (src/audio/levels.rs:36).

**See also:** test `wpctl_volume_lines_parse_with_and_without_mute (src/audio/levels.rs)`.

### `Limits` [@ANCHOR: bench_mcp:bench_mcp_audio_safety_limits_type]

**Purpose and location.** The volume limit in force now: the highest allowed wpctl volume and whether quiet hours apply. Defined in `daemons/bench_mcp/src/audio/safety.rs:10` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `volume` | `f64` | The highest wpctl volume allowed now. |
| `quiet` | `bool` | True during quiet hours (the lowest volume applies except for urgent alerts). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `limits` (src/nanovna.rs:273), `check_range` (src/nanovna.rs:387), `set` (src/psu.rs:66), `on_seconds` (src/psu.rs:119), `check_stored_setpoints` (src/psu.rs:138), `plan_on` (src/psu.rs:151), `lim` (src/psu.rs:255), `limits` (src/sdr.rs:109).

**See also:** test `quiet_hours_lower_the_limit_unless_urgent_and_a_port_cap_wins (src/audio/safety.rs)`.

### `Want` [@ANCHOR: bench_mcp:bench_mcp_audio_safety_want_type]

**Purpose and location.** What to do with a device's volume before making sound: keep it (lowered to the limit if above) or set a level (capped at the limit). Defined in `daemons/bench_mcp/src/audio/safety.rs:34` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Keep` | `Keep` | Leave it, but lower it to the limit if it is above. |
| `Level` | `Level(f64)` | Set it to this volume (or the limit, if lower). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_guard_lowers_to_the_cap_unmutes_and_restores_both (src/audio/safety.rs)`.

### `Applied` [@ANCHOR: bench_mcp:bench_mcp_audio_safety_applied_type]

**Purpose and location.** What `VolumeGuard::apply` did: the level before, and the volume and mute state now. Defined in `daemons/bench_mcp/src/audio/safety.rs:42` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `before` | `Level` | The level that was there before; it is what the guard restores. |
| `volume` | `f64` | The volume now in force. |
| `muted` | `bool` | Whether the node is muted now. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_guard_lowers_to_the_cap_unmutes_and_restores_both (src/audio/safety.rs)`.

### `VolumeGuard` [@ANCHOR: bench_mcp:bench_mcp_audio_safety_volume_guard_type]

**Purpose and location.** A guard that restores a node's previous volume and mute state when it is dropped, so a tool that raised the volume to play a tone always puts it back (a failure to restore is printed to stderr). Defined in `daemons/bench_mcp/src/audio/safety.rs:48` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `run` | `&'a dyn Run` | The program runner used to call `wpctl`. |
| `id` | `u32` | The PipeWire id of the node. |
| `before` | `Level` | The level to restore. |
| `done` | `bool` | Set once restored, so a second restore does nothing. |

**Invariants, ownership and concurrency.** Borrows its runner for its lifetime; not shared across threads. This is a protection on audible volume only; it has nothing to do with a transmitter.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_guard_lowers_to_the_cap_unmutes_and_restores_both (src/audio/safety.rs)`.

### `Engine` [@ANCHOR: bench_mcp:bench_mcp_audio_speak_engine_type]

**Purpose and location.** The text-to-speech engines the server can drive. Defined in `daemons/bench_mcp/src/audio/speak.rs:24` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `SpeechDispatcher` | `SpeechDispatcher` | Speech Dispatcher through `spd-say`. |
| `EspeakNg` | `EspeakNg` | The `espeak-ng` program. |
| `Piper` | `Piper` | The Piper neural voices. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `choose` (src/audio/speak.rs:63), `request_for` (src/audio/speak.rs:587).

**See also:** test `auto_prefers_speech_dispatcher_only_for_a_running_server_and_the_default_output (src/audio/speak.rs)`.

### `Found` [@ANCHOR: bench_mcp:bench_mcp_audio_speak_found_type]

**Purpose and location.** Which speech engines are present on this machine. Defined in `daemons/bench_mcp/src/audio/speak.rs:42` as a pub `struct`. It derives Clone, Debug, Default, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `espeak_ng` | `bool` | `espeak-ng` is installed. |
| `spd_say` | `bool` | `spd-say` is installed. |
| `speechd_running` | `bool` | The speech-dispatcher server socket exists, so a client does not have to start a server. |
| `piper` | `bool` | `piper` is installed. |
| `piper_model` | `Option<String>` | The Piper voice model found, if any. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `detect` (src/audio/speak.rs:51), `choose` (src/audio/speak.rs:63), `found` (src/audio/speak.rs:438), `find` (src/elad/mod.rs:176), `open` (src/elad/mod.rs:178), `find` (src/elad/mod.rs:185), `open` (src/elad/mod.rs:188), `take_lock_in` (src/elad/mod.rs:202).

**See also:** test `the_synthesizer_report_lists_installed_engines_and_the_espeak_voices (src/audio/speak.rs)`.

### `Voice` [@ANCHOR: bench_mcp:bench_mcp_audio_speak_voice_type]

**Purpose and location.** One selectable voice. Defined in `daemons/bench_mcp/src/audio/speak.rs:108` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `language` | `String` | The language code. |
| `name` | `String` | The voice name. |
| `gender` | `String` | The gender text reported by the engine. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_voices` (src/audio/speak.rs:115).

**See also:** test `the_recorded_espeak_voice_list_parses (src/audio/speak.rs)`.

### `Request` [@ANCHOR: bench_mcp:bench_mcp_audio_speak_request_type]

**Purpose and location.** A validated speaking request. Defined in `daemons/bench_mcp/src/audio/speak.rs:235` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `text` | `String` | The text to speak. |
| `engine` | `Engine` | The engine chosen. |
| `voice` | `String` | The voice name (empty for the engine's default). |
| `language` | `String` | The language code. |
| `rate_wpm` | `u32` | Speaking rate in words per minute. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `request` (src/audio/speak.rs:243), `synthesize` (src/audio/speak.rs:290), `say` (src/audio/speak.rs:318), `request_for` (src/audio/speak.rs:587).

**See also:** test `a_request_is_validated_and_takes_its_defaults_from_the_config (src/audio/speak.rs)`.

### `Scripted` [@ANCHOR: bench_mcp:bench_mcp_audio_testkit_scripted_type]

**Purpose and location.** Test double for the program runner: answers `pw-dump` with a recording and `wpctl get-volume` from a table, and records every other call. Defined in `daemons/bench_mcp/src/audio/testkit.rs:20` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `volumes` | `RefCell<Vec<(u32, f64, bool)>>` | Table of (node id, volume, muted) answered for `wpctl get-volume` (behind a RefCell). |
| `calls` | `RefCell<Vec<String>>` | Log of the other commands called (behind a RefCell). |

**Invariants, ownership and concurrency.** Plain data: its fields hold RefCell (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/audio/testkit.rs:26), `speaker_ctx` (src/audio/tools.rs:814), `no_change` (src/audio/tools.rs:824).

**See also:** test `a_dry_run_of_an_alert_returns_the_text_and_the_plan_and_makes_no_sound (src/audio/alert.rs)`.

### `Tone` [@ANCHOR: bench_mcp:bench_mcp_audio_tones_tone_type]

**Purpose and location.** A test tone to synthesize: a sine, a sweep, two tones at once, or noise. Defined in `daemons/bench_mcp/src/audio/tones.rs:9` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Sine` | `Sine { hz: f64 }` | A sine at `hz`. |
| `Sweep` | `Sweep { from_hz: f64, to_hz: f64 }` | A sweep from `from_hz` to `to_hz`. |
| `Dual` | `Dual { hz1: f64, hz2: f64 }` | Two tones, `hz1` and `hz2`. |
| `Noise` | `Noise` | Noise. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `generate` (src/audio/tones.rs:24), `parse` (src/audio/tones.rs:75).

**See also:** test `a_sine_has_the_requested_peak_and_fades_at_its_edges (src/audio/tones.rs)`.

### `Ctx` [@ANCHOR: bench_mcp:bench_mcp_audio_tools_ctx_type]

**Purpose and location.** What every audio tool needs: the program runner, the configuration, the device list, the local minute of the day (for quiet hours) and the stored discovery result. Defined in `daemons/bench_mcp/src/audio/tools.rs:14` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `run` | `&'a dyn Run` | The runner that starts programs (a real one or a test double). |
| `cfg` | `&'a Config` | The server configuration. |
| `devs` | `&'a [Device]` | The devices listed now. |
| `minute` | `u32` | Minute of the local day, used for quiet hours. |
| `discovery` | `Option<&'a super::discover::Stored>` | The stored result of the last audio path discovery, which decides `auto` selections. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `alert` (src/audio/alert.rs:92), `run` (src/audio/discover.rs:399), `report` (src/audio/discover.rs:605), `wants_rediscovery` (src/audio/mod.rs:785), `request` (src/audio/speak.rs:243), `synthesize` (src/audio/speak.rs:290), `say` (src/audio/speak.rs:318), `synthesizers` (src/audio/speak.rs:340).

**See also:** test `the_meter_reports_peak_and_rms_per_interval (src/audio/tools.rs)`.

### `Taken` [@ANCHOR: bench_mcp:bench_mcp_audio_tools_taken_type]

**Purpose and location.** A recording taken from the start of a stream: the samples, whether a trigger fired, and how long it waited. Defined in `daemons/bench_mcp/src/audio/tools.rs:92` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `samples` | `Vec<f32>` | The samples recorded. |
| `triggered` | `Option<bool>` | With a threshold, whether the level passed it; `None` when no trigger was used. |
| `waited_s` | `f64` | Seconds waited before enough samples or the trigger. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_take_is_the_first_seconds_or_starts_when_the_level_passes_the_threshold (src/audio/tools.rs)`.

### `Source` [@ANCHOR: bench_mcp:bench_mcp_audio_tools_source_type]

**Purpose and location.** The audio a verification tool analyses: a file and what it is. Defined in `daemons/bench_mcp/src/audio/tools.rs:240` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `path` | `std::path::PathBuf` | The file to read. |
| `seconds` | `f64` | Its length in seconds. |
| `temporary` | `bool` | Whether the file was made by the tool and is removed afterwards. |
| `description` | `String` | A description for the report. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `source` (src/audio/tools.rs:247).

**See also:** test `config_takes_a_managed_or_an_existing_radioctld_and_refuses_the_rest (src/radioctl/tests.rs)`.

### `Check` [@ANCHOR: bench_mcp:bench_mcp_audio_verify_check_type]

**Purpose and location.** One named pass or fail check of an audio verification, with its detail text. Defined in `daemons/bench_mcp/src/audio/verify.rs:32` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | The check's name. |
| `ok` | `bool` | Whether it passed. |
| `detail` | `String` | The measured values or reason, in words. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `judge` (src/audio/verify.rs:107).

**See also:** test `a_beep_that_matches_every_expectation_passes_with_reasons (src/audio/verify.rs)`.

### `Expect` [@ANCHOR: bench_mcp:bench_mcp_audio_verify_expect_type]

**Purpose and location.** What the caller expects of the sound; unset options are not checked. Defined in `daemons/bench_mcp/src/audio/verify.rs:46` as a pub `struct`. It derives Debug, Clone, Default.

| Field | Type | Meaning |
|---|---|---|
| `min_over_noise_db` | `f64` | The least number of dB the sound must stand over the noise. |
| `min_peak_dbfs` | `Option<f64>` | The least peak level in dBFS. |
| `hz` | `Option<f64>` | The expected frequency in hertz. |
| `tolerance_hz` | `f64` | How far from `hz` the dominant frequency may be. |
| `hz_min` | `Option<f64>` | The lowest acceptable dominant frequency. |
| `hz_max` | `Option<f64>` | The highest acceptable dominant frequency. |
| `min_duration_s` | `Option<f64>` | The least duration of the sound in seconds. |
| `max_duration_s` | `Option<f64>` | The longest duration of the sound in seconds. |
| `beeps` | `Option<usize>` | The exact number of beeps expected. |
| `min_beeps` | `Option<usize>` | The least number of beeps expected. |
| `silence` | `bool` | Expect silence (no event) instead of sound. |
| `min_similarity` | `f64` | The least spectrum similarity to a reference (when one is given). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `expectations` (src/audio/verify.rs:61), `judge` (src/audio/verify.rs:107), `expect` (src/audio/verify.rs:554).

**See also:** test `expectation_arguments_are_validated_and_the_tolerance_defaults_to_five_percent (src/audio/verify.rs)`.

### `Evidence` [@ANCHOR: bench_mcp:bench_mcp_audio_verify_evidence_type]

**Purpose and location.** The evidence an audio verification judges: input state, noise and threshold, the events found, the dominant frequency and the similarity to a reference. Defined in `daemons/bench_mcp/src/audio/verify.rs:97` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `input_all_zero` | `bool` | The input delivered only zeros. |
| `noise_dbfs` | `f64` | Noise reference in dBFS. |
| `threshold_dbfs` | `f64` | Threshold used in dBFS. |
| `events` | `&'a [Event]` | The detected events, borrowed. |
| `dominant_hz` | `Option<f64>` | The dominant frequency, if any. |
| `similarity` | `Option<f64>` | Spectrum correlation with a reference, if one was given. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `judge` (src/audio/verify.rs:107), `evidence` (src/audio/verify.rs:543), `proven` (src/usb_hub/power.rs:86), `remember` (src/usb_hub/power.rs:100).

**See also:** test `nothing_heard_fails_and_says_how_far_over_the_floor_it_looked (src/audio/verify.rs)`.

### `Wav` [@ANCHOR: bench_mcp:bench_mcp_audio_wav_wav_type]

**Purpose and location.** A decoded WAV file: sample rate, channel count and interleaved float samples. Reads 16, 24 and 32-bit integer PCM and 32-bit float, writes 16-bit PCM. Defined in `daemons/bench_mcp/src/audio/wav.rs:7` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `rate` | `u32` | Sample rate in hertz. |
| `channels` | `u16` | Number of interleaved channels. |
| `samples` | `Vec<f32>` | Interleaved samples, full scale 1.0. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse` (src/audio/wav.rs:41), `read` (src/audio/wav.rs:104), `encode` (src/audio/wav.rs:110), `write` (src/audio/wav.rs:133).

**See also:** test `a_wav_round_trips_within_one_sixteen_bit_step (src/audio/wav.rs)`.
