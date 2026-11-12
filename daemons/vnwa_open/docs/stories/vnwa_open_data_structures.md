# Data structures

### `Complex` [@ANCHOR: vnwa_open:vnwa_open_audio_complex]

**Purpose and location.** A complex number (no dependency for this). Defined in `daemons/vnwa_open/src/audio.rs:13` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `re` | `f64` | Real part. |
| `im` | `f64` | Imaginary part. |

**Invariants, ownership and concurrency.** Plain `Copy` value used for every phasor, ratio and error term in the crate. It implements Add, Sub, Mul and Div by hand (no dependency); division by a zero value is not guarded here (unknown whether callers check).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `divided_by` (src/audio.rs:28), `add` (src/audio.rs:39), `sub` (src/audio.rs:49), `mul` (src/audio.rs:59), `div` (src/audio.rs:69), `tone_at_if` (src/audio.rs:76), `vendor_convention` (src/audio.rs:115), `complex_of` (src/audio.rs:203).

**See also:** test `against_the_vendor_programs_own_s21_in_a_sweep_laid_out_in_time (src/audio.rs)`.

### `Reference` [@ANCHOR: vnwa_open:vnwa_open_audio_reference]

**Purpose and location.** Which channel of a codec carries the reference signal. Defined in `daemons/vnwa_open/src/audio.rs:126` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Left` | `Left` | The reference is on the left channel (the first codec on the observed unit). |
| `Right` | `Right` | The reference is on the right channel (the second codec on the observed unit). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_ratio_of_two_tones_is_their_amplitude_ratio_and_phase_difference (src/audio.rs)`.

### `RealExcerpt` [@ANCHOR: vnwa_open:vnwa_open_audio_vectors_real_excerpt]

**Purpose and location.** One recorded excerpt of real codec samples from the 10 MHz TX-to-RX loopback sweeps, kept as a test vector (48 kHz, 16 bit, 96 frames each). Defined in `daemons/vnwa_open/src/audio_vectors.rs:5` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | Label of the excerpt. |
| `if_hz` | `f64` | The IF in hertz the tone was at. |
| `left` | `[i16; 96]` | 96 left-channel samples. |
| `right` | `[i16; 96]` | 96 right-channel samples. |
| `ratio_re` | `f64` | Real part of the left/right ratio of the tone at the IF, computed independently with NumPy over all 96 frames. |
| `ratio_im` | `f64` | Imaginary part of that ratio. |

**Invariants, ownership and concurrency.** Static data compiled into the test vectors; never mutated.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `real_loopback_samples_give_the_independently_computed_ratio_and_the_expected_phase (src/audio.rs)`.

### `Setup` [@ANCHOR: vnwa_open:vnwa_open_cal_setup]

**Purpose and location.** The measurement settings a calibration is valid for. A calibration holds only at the settings it was made with; applying it at other ones is refused (`Calibration::check_setup`). Defined in `daemons/vnwa_open/src/cal.rs:24` as a pub `struct`. It derives Clone, Debug, PartialEq, Default.

| Field | Type | Meaning |
|---|---|---|
| `if_hz` | `f64` | The measurement IF in hertz the calibration was made at. |
| `point_ms` | `f64` | The time per point in milliseconds. |
| `rf_level` | `u16` | The 14-bit RF level setting. |
| `cable` | `String` | The cables and adapters between the ports and the reference plane, in the operator's words (compared as written). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `check_setup` (src/cal.rs:432).

**See also:** test `a_real_kit_is_modelled_and_the_calibration_removes_its_imperfections (src/cal.rs)`.

### `Offset` [@ANCHOR: vnwa_open:vnwa_open_cal_offset]

**Purpose and location.** One standard's published model: the polynomial of its reactance and the offset line in front of it. Defined in `daemons/vnwa_open/src/cal.rs:36` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `delay_s` | `f64` | One-way delay of the offset line in seconds and its loss in gigaohms per second at 1 GHz (the usual kit convention). |
| `loss_gohm_s` | `f64` | Loss of the offset line in gigaohms per second at 1 GHz. |

**Invariants, ownership and concurrency.** Part of a `KitDefinition`; plain data.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `through_offset` (src/cal.rs:88), `default_none` (src/cal.rs:101).

**See also:** test `a_real_kit_is_modelled_and_the_calibration_removes_its_imperfections (src/cal.rs)`.

### `KitDefinition` [@ANCHOR: vnwa_open:vnwa_open_cal_kit_definition]

**Purpose and location.** A calibration kit definition: open capacitance coefficients C0 to C3 (F, F/Hz, F/Hz^2, F/Hz^3), short inductance L0 to L3 (H, ...), the load's resistance and inductance, an offset line for each, and the through's delay. The default is the ideal kit. Defined in `daemons/vnwa_open/src/cal.rs:45` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | Name of the kit. |
| `open_c` | `[f64; 4]` | Open capacitance coefficients C0 to C3 (F, F/Hz, F/Hz^2, F/Hz^3). |
| `open_offset` | `Offset` | Offset line in front of the open. |
| `short_l` | `[f64; 4]` | Short inductance coefficients L0 to L3. |
| `short_offset` | `Offset` | Offset line in front of the short. |
| `load_r_ohm` | `f64` | Resistance of the load in ohms. |
| `load_l_h` | `f64` | Inductance of the load in henries. |
| `load_offset` | `Offset` | Offset line in front of the load. |
| `thru_delay_s` | `f64` | Delay of the through standard in seconds. |

**Invariants, ownership and concurrency.** Plain data; the default is the ideal kit.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/cal.rs:58), `from_text` (src/cal.rs:177).

**See also:** test `a_real_kit_is_modelled_and_the_calibration_removes_its_imperfections (src/cal.rs)`.

### `CalibratedPoint` [@ANCHOR: vnwa_open:vnwa_open_cal_calibrated_point]

**Purpose and location.** A point after correction; a parameter the calibration does not cover is `None`. Defined in `daemons/vnwa_open/src/cal.rs:264` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | Frequency of the point in hertz. |
| `s11` | `Option<Complex>` | Corrected S11, `None` when the calibration has no one-port terms. |
| `s21` | `Option<Complex>` | Corrected S21, `None` when the calibration has no through. |

**Invariants, ownership and concurrency.** A result value; a parameter the calibration does not cover is `None`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `correct` (src/cal.rs:526).

**See also:** test `a_point_is_corrected_in_each_parameter_the_calibration_covers (src/cal.rs)`.

### `OnePort` [@ANCHOR: vnwa_open:vnwa_open_cal_one_port_type]

**Purpose and location.** The one-port error terms at every calibrated frequency. Defined in `daemons/vnwa_open/src/cal.rs:272` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `e00` | `Vec<Complex>` | Directivity error term at each calibrated frequency. |
| `e11` | `Vec<Complex>` | Source match error term at each calibrated frequency. |
| `tracking` | `Vec<Complex>` | Reflection tracking error term at each calibrated frequency. |

**Invariants, ownership and concurrency.** The three vectors have one entry per frequency of the owning `Calibration`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_one_port_terms_are_recovered_and_a_known_load_is_corrected (src/cal.rs)`.

### `Calibration` [@ANCHOR: vnwa_open:vnwa_open_cal_calibration]

**Purpose and location.** A calibration: the kit and measurement settings it was made with, the frequencies, and the error terms. Defined in `daemons/vnwa_open/src/cal.rs:279` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `created` | `String` | When it was made (text, as the caller wrote it). |
| `kit` | `KitDefinition` | The kit definition it was made with. |
| `setup` | `Setup` | The measurement settings it holds for; applying it at other settings is refused (`check_setup`). |
| `freqs` | `Vec<f64>` | The calibrated frequencies in hertz. |
| `one_port` | `Option<OnePort>` | The one-port error terms, if a one-port calibration was done. |
| `thru` | `Option<Vec<Complex>>` | The S21 measured with the ports joined. |

**Invariants, ownership and concurrency.** A plain value owned by the caller; it is only valid at the settings in `setup`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_text` (src/cal.rs:585).

**See also:** test `the_one_port_terms_are_recovered_and_a_known_load_is_corrected (src/cal.rs)`.

### `Ring` [@ANCHOR: vnwa_open:vnwa_open_capture_ring]

**Purpose and location.** The state the audio callback and the reader share for one capture stream: a bounded queue of recent frames plus bookkeeping about overruns. Defined in `daemons/vnwa_open/src/capture.rs:15` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `frames` | `std::collections::VecDeque<[i16; 2]>` | The most recent frames, `[left, right]` samples, capped at 8 seconds (`KEEP_FRAMES`). |
| `total` | `u64` | Frames received since the stream started (not reduced when old frames are dropped). |
| `overruns` | `std::collections::VecDeque<Overrun>` | Overruns the audio layer reported, newest last; those older than the oldest frame kept are dropped. |
| `overrun_count` | `u64` | All overruns since the stream started. |
| `last_delivery` | `Option<Instant>` | When the audio layer last delivered a period. |
| `period` | `u64` | Length in frames of the last period the audio layer delivered (0 before the first). |
| `reports_since_delivery` | `u64` | Overrun reports since the last delivery; a restarting stream that keeps failing reports one every millisecond or so. |
| `storm` | `bool` | Set when the stream was closed because of a storm of reports. |

**Invariants, ownership and concurrency.** Lives inside an `Arc<(Mutex<Ring>, Condvar)>` shared by the audio callback thread (writer) and the measuring thread (reader, which waits on the condvar). Always accessed under the mutex.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `shared` (src/capture.rs:33).

**See also:** test `a_storm_of_reports_is_told_from_single_overruns_by_the_count_without_audio_between (src/capture.rs)`.

### `Overrun` [@ANCHOR: vnwa_open:vnwa_open_capture_overrun]

**Purpose and location.** One buffer overrun reported by the audio layer: the capture ring lost frames at this point. `at_frame` is the running frame count when it was reported (frames before it were delivered; the lost ones would have followed), `at` the instant. Defined in `daemons/vnwa_open/src/capture.rs:62` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `at` | `Instant` | The instant it was reported. |
| `at_frame` | `u64` | The running frame count when it was reported. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `overruns` (src/capture.rs:250), `overruns_in` (src/capture.rs:391).

**See also:** test `an_overrun_counts_against_a_window_only_from_just_before_the_mark_to_its_end (src/capture.rs)`.

### `Capture` [@ANCHOR: vnwa_open:vnwa_open_capture_capture]

**Purpose and location.** A running audio capture stream: the shared ring and the handle that keeps the stream alive. Defined in `daemons/vnwa_open/src/capture.rs:73` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `ring` | `Arc<(Mutex<Ring>, Condvar)>` | The shared frame ring and its condvar. |
| `stream` | `Arc<Mutex<Option<Box<dyn Send>>>>` | The audio stream object, `None` after `close`. |

**Invariants, ownership and concurrency.** Created by `Capture::open` or from a synthetic feed; `close` drops the stream and the handle can no longer deliver frames. Reading blocks on the ring's condvar.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (src/capture.rs:132), `open_with` (src/capture.rs:136), `with_parts` (src/sweep.rs:465).

**See also:** test `closing_the_streams_before_a_power_cycle_reports_no_overruns_and_idle_capture_reports_none (tests/hil_overrun.rs)`.

### `Options` [@ANCHOR: vnwa_open:vnwa_open_capture_options]

**Purpose and location.** How a stream is opened. Defined in `daemons/vnwa_open/src/capture.rs:86` as a pub `struct`. It derives Clone, Copy, Debug, Default, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `period_frames` | `Option<u32>` | The ALSA period in frames (cpal sets the buffer to two periods); `None` leaves it to the driver. |

**Invariants, ownership and concurrency.** Plain `Copy` options; `Default` leaves the period to the driver.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open_with` (src/capture.rs:136).

**See also:** test `the_sweep_asks_the_audio_layer_for_a_1024_frame_period (src/sweep.rs)`.

### `CpalHost` [@ANCHOR: vnwa_open:vnwa_open_capture_cpal_host]

**Purpose and location.** The default cpal audio host (ALSA on Linux), used to open real capture streams. Defined in `daemons/vnwa_open/src/capture.rs:97` as a pub `struct`. It derives no traits.

**Fields.** none (unit or opaque definition).

**Invariants, ownership and concurrency.** A unit struct with no state; it implements the host trait that tests replace with a synthetic one. No test opens a real sound card; the cited test uses the host trait with a fake host.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_input_ids_are_the_hosts_in_its_order_and_a_host_error_is_passed_on (src/capture.rs)`.

### `SyntheticFeed` [@ANCHOR: vnwa_open:vnwa_open_capture_synthetic_feed]

**Purpose and location.** A test and soak feed that produces audio frames on a thread instead of a sound card. Defined in `daemons/vnwa_open/src/capture.rs:369` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `stop` | `Arc<std::sync::atomic::AtomicBool>` | Flag that tells the feed thread to end. |
| `thread` | `Option<std::thread::JoinHandle<()>>` | The feed thread handle, joined on stop or drop. |

**Invariants, ownership and concurrency.** Owns one thread; dropping it sets `stop` and joins. Used by tests in place of `CpalHost`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_synthetic_capture_delivers_the_generated_frames_after_a_mark_until_it_is_closed (src/capture.rs)`.

### `Read` [@ANCHOR: vnwa_open:vnwa_open_generator_read]

**Purpose and location.** One device-to-host request of a sequence. The unit answers with no data of interest; the transfer itself is the command. Defined in `daemons/vnwa_open/src/generator.rs:15` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `request` | `u8` | The vendor request number (bRequest). |
| `value` | `u16` | wValue of the transfer. |
| `len` | `usize` | Number of bytes asked for. |

**Invariants, ownership and concurrency.** Private description of one device-to-host transfer in a generator command sequence; the answer is ignored.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run` (src/generator.rs:93).

**See also:** test `the_captured_am_sequence_is_sent_in_order_and_a_stalled_last_request_is_not_an_error (src/generator.rs)`.

### `AmFields` [@ANCHOR: vnwa_open:vnwa_open_generator_am_fields]

**Purpose and location.** The fields of an AM start that depend on the settings (all others are constants of the sequence). Defined in `daemons/vnwa_open/src/generator.rs:157` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `phase_step` | `u16` | 0x92 wValue: the modulation phase step. |
| `amplitude_high` | `u16` | 0x94: the modulation amplitude as a 32-bit number, wIndex the high half and wValue the low half. |
| `amplitude_low` | `u16` | Low 16 bits of the modulation amplitude (wValue of request 0x94). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `am_fields` (src/generator.rs:169).

**See also:** test `the_am_fields_reproduce_every_captured_start (src/generator.rs)`.

### `FmFields` [@ANCHOR: vnwa_open:vnwa_open_generator_fm_fields]

**Purpose and location.** The FM fields: 0x92 wValue (phase step) and 0x95 wValue (the deviation per step). Defined in `daemons/vnwa_open/src/generator.rs:190` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `phase_step` | `u16` | wValue of request 0x92, the phase step. |
| `step_word` | `u16` | wValue of request 0x95, the deviation per step. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `fm_fields` (src/generator.rs:195).

**See also:** test `the_fm_fields_reproduce_every_captured_start (src/generator.rs)`.

### `Modulation` [@ANCHOR: vnwa_open:vnwa_open_generator_modulation]

**Purpose and location.** Which modulation. Defined in `daemons/vnwa_open/src/generator.rs:215` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Am` | `Am { mod_hz: f64, depth_pct: f64 }` | Amplitude modulation: `mod_hz` modulation frequency and `depth_pct` depth in percent. |
| `Fm` | `Fm { mod_hz: f64, deviation_hz: f64 }` | Frequency modulation: `mod_hz` modulation frequency and `deviation_hz` peak deviation in hertz. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `modulation_reads` (src/generator.rs:222).

**See also:** test `a_frequency_whose_tuning_word_does_not_fit_is_refused_before_anything_is_sent (src/generator.rs)`.

### `IfRef` [@ANCHOR: vnwa_open:vnwa_open_if_reference_vectors_if_ref]

**Purpose and location.** One recorded IF-response check from 2026-10-26 with TX connected to RX: the vendor program's S21 at 5 MHz and each codec's frames, kept as a test vector. Defined in `daemons/vnwa_open/src/if_reference_vectors.rs:5` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `if_hz` | `f64` | The IF in hertz. |
| `vendor` | `(f64, f64)` | The vendor program's S21 as (real, imaginary), the mean of two single-point sweeps. |
| `codec1` | `Chunk` | 720 frames from codec 1 (reference on its left channel), recorded 10 ms after the tone starts. |
| `codec2` | `Chunk` | 720 frames from codec 2 (reference on its right channel). |

**Invariants, ownership and concurrency.** Static test data, never mutated.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_ifs_response_over_12_to_1_khz_matches_the_vendor_programs_s21_on_both_codecs (src/audio.rs)`.

### `IfScan` [@ANCHOR: vnwa_open:vnwa_open_if_scan_vectors_if_scan]

**Purpose and location.** One recorded IF scan from 2026-10-26: single-point sweep at 10 MHz at one of six more IFs, kept as a test vector. Defined in `daemons/vnwa_open/src/if_scan_vectors.rs:5` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `if_hz` | `f64` | The IF in hertz. |
| `vendor` | `(f64, f64)` | The vendor program's S21 as (real, imaginary), the mean of two runs. |
| `codec1` | `[[i16; 2]; 960]` | 960 frames (left, right) of codec 1, 20 ms after the tone starts. |
| `codec2` | `[[i16; 2]; 960]` | 960 frames (left, right) of codec 2. |

**Invariants, ownership and concurrency.** Static test data, never mutated.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `at_other_ifs_both_codecs_match_the_vendor_programs_s21_with_their_reference_channels (src/audio.rs)`.

### `Managed` [@ANCHOR: vnwa_open:vnwa_open_install_managed]

**Purpose and location.** One file the installer manages: where it goes, what it contains and its mode. Defined in `daemons/vnwa_open/src/install.rs:27` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `dest` | `&'static str` | Absolute destination path (below `root` when a root is set). |
| `content` | `&'static str` | The file content compiled into the program. |
| `mode` | `&'static str` | The file mode as a string for `install -m`. |

**Invariants, ownership and concurrency.** Static values; the three files are the WirePlumber rule, the udev rule and the helper script.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `managed` (src/install.rs:33), `state` (src/install.rs:161), `backup_exists` (src/install.rs:297), `install` (src/install.rs:347).

**See also:** test `apply_installs_is_idempotent_backs_up_what_it_replaces_and_uninstall_restores_it (src/install.rs)`.

### `Mode` [@ANCHOR: vnwa_open:vnwa_open_install_mode]

**Purpose and location.** What the installer was asked to do. Defined in `daemons/vnwa_open/src/install.rs:55` as a pub `enum`. It derives Clone, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Install` | `Install` | Install the files. |
| `Uninstall` | `Uninstall` | Remove them and restore what was there. |
| `Check` | `Check` | Report state only, change nothing. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `installer` (src/install.rs:468).

**See also:** test `a_dry_run_prints_the_files_and_changes_nothing_and_every_packaged_file_keeps_its_comment_block (src/install.rs)`.

### `Options` [@ANCHOR: vnwa_open:vnwa_open_install_options]

**Purpose and location.** What the installer was asked to do and with which switches. Defined in `daemons/vnwa_open/src/install.rs:62` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `mode` | `Mode` | Install, Uninstall or Check. |
| `apply` | `bool` | When false the run is a dry run that only reports the plan; when true it changes files. |
| `force` | `bool` | Proceed while a sweep or soak has a VNWA codec open (`--force`), and replace a file that differs from ours and has no backup. |
| `restart_audio` | `bool` | Restart the sound server after installing or uninstalling (not in Check). |
| `root` | `Option<PathBuf>` | A directory the destinations are placed under instead of `/` (like DESTDIR); no `sudo`, no `udevadm`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open_with` (src/capture.rs:136).

**See also:** test `apply_installs_is_idempotent_backs_up_what_it_replaces_and_uninstall_restores_it (src/install.rs)`.

### `State` [@ANCHOR: vnwa_open:vnwa_open_install_state_type]

**Purpose and location.** The state of one managed file. Defined in `daemons/vnwa_open/src/install.rs:73` as a pub `enum`. It derives Clone, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Missing` | `Missing` | The destination does not exist. |
| `Current` | `Current` | The destination has exactly the managed content. |
| `Different` | `Different` | The destination exists with other content. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `state` (src/install.rs:161).

**See also:** test `a_scope_that_never_answered_is_a_no_go_that_names_the_evidence_and_the_action_and_a_clean_log_is_go (tests/soak_gonogo.rs)`.

### `ProcessExec` [@ANCHOR: vnwa_open:vnwa_open_install_process_exec_type]

**Purpose and location.** The production command runner of the installer: runs each command as a real process. Defined in `daemons/vnwa_open/src/install.rs:84` as a pub `struct`. It derives no traits.

**Fields.** none (unit or opaque definition).

**Invariants, ownership and concurrency.** Unit struct. It is the implementation of `Exec` that tests replace with a recorder; it spawns the program and reports failure as an error string.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_failure_midway_is_a_clear_error_and_the_steps_done_are_undone (src/install.rs)`.

### `Installer` [@ANCHOR: vnwa_open:vnwa_open_install_installer]

**Purpose and location.** The installer: options, an executor and the sysfs / proc locations. Defined in `daemons/vnwa_open/src/install.rs:143` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `opts` | `Options` | The options of this run. |
| `exec` | `&'a dyn Exec` | The command runner (a process in production, a recorder in tests). |
| `sys` | `PathBuf` | Location of sysfs (tests point it at a fake tree). |
| `proc_asound` | `PathBuf` | Location of `/proc/asound`. |
| `log` | `PathBuf` | The log file the installer appends a line to for each action. |
| `root_user` | `bool` | Whether the caller is root; if not and no `root` option is set, the installer refuses to change system files (it would need sudo). |

**Invariants, ownership and concurrency.** Borrows its executor for its lifetime `'a`; one run per value.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `installer` (src/install.rs:468).

**See also:** test `apply_installs_is_idempotent_backs_up_what_it_replaces_and_uninstall_restores_it (src/install.rs)`.

### `Undo` [@ANCHOR: vnwa_open:vnwa_open_install_undo]

**Purpose and location.** How the installer reverses one file: remove it or restore the saved original. Defined in `daemons/vnwa_open/src/install.rs:448` as a private `enum`. It derives Clone, Copy.

| Variant | Definition | Meaning |
|---|---|---|
| `Remove` | `Remove` | The file did not exist before, delete it. |
| `Restore` | `Restore` | A backup of the original exists, move it back. |

**Invariants, ownership and concurrency.** Private `Copy` enum used only inside the uninstall path.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `install` (src/install.rs:347).

**See also:** test `a_failure_midway_is_a_clear_error_and_the_steps_done_are_undone (src/install.rs)`.

### `Reading` [@ANCHOR: vnwa_open:vnwa_open_meter_reading]

**Purpose and location.** One frequency-meter result: where the meter was centred, what the unit's TX DDS really produced, and the measured offset in Hz from each codec, their mean, the fit's standard error and its duration. Defined in `daemons/vnwa_open/src/meter.rs:93` as a pub `struct`. It derives Clone, Copy, Debug.

| Field | Type | Meaning |
|---|---|---|
| `center_hz` | `f64` | The frequency the meter was centred on, in hertz. |
| `tx_actual_hz` | `f64` | The frequency the unit's TX DDS really produced for it (tuning word). |
| `offset_s11_hz` | `f64` | Measured tone minus reference tone in Hz from codec 1. |
| `offset_s21_hz` | `f64` | Measured tone minus reference tone in Hz from codec 2. |
| `offset_hz` | `f64` | The mean of the two codec offsets. |
| `stderr_hz` | `f64` | Standard error of the fit in Hz. |
| `seconds` | `f64` | How long the fit ran, in seconds. |

**Invariants, ownership and concurrency.** A result value returned by the frequency meter; plain `Copy`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `meter` (src/meter.rs:118), `meter_once` (src/meter.rs:123).

**See also:** test `a_simulated_unit_reads_1_10_and_50_mhz_within_1_ppm_and_never_returns_a_wrong_number (src/meter.rs)`.

### `HubPort` [@ANCHOR: vnwa_open:vnwa_open_power_hub_port]

**Purpose and location.** A port of a power-switching hub, as `uhubctl -l <hub> -p <port>` names it (for example hub `3-2`, port 2). Defined in `daemons/vnwa_open/src/power.rs:10` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `hub` | `String` | The hub name as `uhubctl -l` takes it (for example `3-2`). |
| `port` | `u8` | The port number on that hub. |

**Invariants, ownership and concurrency.** Names the port that powers the unit; used by the power-cycle code and the recovery ladder.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `lsusb_check` (examples/soak.rs:140), `start_journal` (examples/soak.rs:165), `uhubctl` (src/power.rs:50), `cold_reset` (src/power.rs:189), `cold_reset_after_modulation` (src/power.rs:236), `port` (src/power.rs:399), `unit_subtree` (src/soak_topology.rs:36), `compare` (src/soak_topology.rs:85).

**See also:** test `an_installed_cycle_implementation_gets_the_port_and_the_ladder_and_its_error_is_returned (src/power.rs)`.

### `ProcessRunner` [@ANCHOR: vnwa_open:vnwa_open_power_process_runner_type]

**Purpose and location.** The production runner for power-switching: executes `uhubctl` and other programs as real processes. Defined in `daemons/vnwa_open/src/power.rs:20` as a pub `struct`. It derives no traits.

**Fields.** none (unit or opaque definition).

**Invariants, ownership and concurrency.** Unit struct implementing `Runner`; tests substitute a scripted runner.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_process_runner_gives_the_exit_status_and_both_output_streams_of_a_real_program (src/power.rs)`.

### `CycleHook` [@ANCHOR: vnwa_open:vnwa_open_power_cycle_hook]

**Purpose and location.** A callback type for the host program's own power-cycle implementation (see its doc comment): it takes a hub port and a ladder of (off time, wait for the unit) pairs and returns `Ok(())` when the unit is back. Defined in `daemons/vnwa_open/src/power.rs:163` as a pub `type`. It derives no traits.

**Definition.** `pub type CycleHook =` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** A boxed `Fn` that is `Send + Sync`, so it may be called from the measuring thread. It can power-cycle real hardware; standalone programs use the built-in `sudo -n uhubctl` path instead.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `set_cycle_hook` (src/power.rs:169).

**See also:** test `an_installed_cycle_implementation_gets_the_port_and_the_ladder_and_its_error_is_returned (src/power.rs)`.

### `Request` [@ANCHOR: vnwa_open:vnwa_open_protocol_request]

**Purpose and location.** A vendor control transfer, host to device (`bmRequestType` 0x40) or device to host (0xC0). Defined in `daemons/vnwa_open/src/protocol.rs:14` as a pub `struct`. It derives Clone, Debug, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `request_type` | `u8` | bmRequestType: 0x40 host to device, 0xC0 device to host. |
| `request` | `u8` | bRequest, the vendor command number. |
| `value` | `u16` | wValue. |
| `index` | `u16` | wIndex. |
| `data` | `Vec<u8>` | Payload bytes (for a read, the expected length is the `len`, data empty). |

**Invariants, ownership and concurrency.** Plain value built by the sequence builders and handed to a `Transport`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `generator_frequency` (src/protocol.rs:98), `out` (src/setup.rs:61), `setup_block` (src/setup.rs:72), `send` (src/usb.rs:118), `send_all` (src/usb.rs:132).

**See also:** test `if_words_match_the_captured_setup_requests (src/protocol.rs)`.

### `Chunk` [@ANCHOR: vnwa_open:vnwa_open_reference_vectors_chunk]

**Purpose and location.** Alias for 720 stereo frames (`[left, right]` i16) recorded in a reference sweep. Defined in `daemons/vnwa_open/src/reference_vectors.rs:24` as a pub `type`. It derives no traits.

**Definition.** `pub type Chunk = [[i16; 2]; 720];` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Array type of fixed size; the vector data is static.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `against_the_vendor_programs_own_s21_in_a_sweep_laid_out_in_time (src/audio.rs)`.

### `PointRef` [@ANCHOR: vnwa_open:vnwa_open_reference_vectors_point_ref]

**Purpose and location.** One recorded reference point: the frequency, the vendor program's S21 and the codec frames, kept as a test vector. Defined in `daemons/vnwa_open/src/reference_vectors.rs:25` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `f_mhz` | `f64` | Sweep frequency in MHz. |
| `vendor` | `(f64, f64)` | The vendor program's S21 as (real, imaginary). |
| `chunk` | `Chunk` | The recorded frames. |

**Invariants, ownership and concurrency.** Static test data.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `against_the_vendor_programs_own_s21_at_four_single_frequencies (src/audio.rs)`.

### `SetupParams` [@ANCHOR: vnwa_open:vnwa_open_setup_setup_params]

**Purpose and location.** What the set-up block depends on. Defined in `daemons/vnwa_open/src/setup.rs:14` as a pub `struct`. It derives Clone, Copy, Debug.

| Field | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | The frequency of this point and the frequency before the sweep started. |
| `previous_hz` | `f64` | The frequency before the sweep started. |
| `n_rf` | `u8` | The two chips' clock multipliers (the observed unit: 10 and 9). |
| `n_lo` | `u8` | Clock multiplier of the second chip (the observed unit: 9). |
| `if_hz` | `f64` | The measurement IF (12, 6 or 4 kHz seen) and the time per point. |
| `point_ms` | `f64` | The time per point in milliseconds. |
| `counter` | `u8` | The first byte of the 0x49 payload (3 or 4 for measurement points, 2 for the slow sweeps seen). |
| `mode` | `u16` | `wValue` of requests 0x40 and 0x41 (0, 1, 2, 0x0302, 0x0303 and 8 were seen; the meaning is unknown). |
| `rf_level` | `u16` | The RF level (14 bits) carried in `wIndex` of the first seven requests, or 0. (The vendor program sometimes sends it on fewer of them, apparently a race inside the program; that is not reproduced.) |

**Invariants, ownership and concurrency.** Plain `Copy` input to the set-up block builder; the builder is pure.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `setup_block` (src/setup.rs:72).

**See also:** test `the_block_reproduces_every_captured_block_byte_for_byte (src/setup.rs)`.

### `ScopeStream` [@ANCHOR: vnwa_open:vnwa_open_soak_scope_scope_stream]

**Purpose and location.** A connected oscilloscope: a buffered reader and a writer on the same TCP socket. Defined in `daemons/vnwa_open/src/soak_scope.rs:9` as a pub `type`. It derives no traits.

**Definition.** `pub type ScopeStream = (BufReader<TcpStream>, TcpStream);` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Tuple alias used by the soak program; the two halves share one connection, so they must be used from one thread at a time.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `try_once` (src/soak_scope.rs:39).

**See also:** test `a_scope_that_resets_the_first_connections_is_waited_out_and_then_answers (src/soak_scope.rs)`.

### `Card` [@ANCHOR: vnwa_open:vnwa_open_sound_server_card]

**Purpose and location.** One card the sound server lists with the codec chip's USB id. Defined in `daemons/vnwa_open/src/sound_server.rs:13` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `description` | `String` | The card's description text from the sound server. |
| `port` | `Option<String>` | The USB port path from the card's bus path, for example `2.2.2` from `pci-0000:00:14.0-usb-0:2.2.2:1.0`. |
| `id` | `Option<u64>` | The sound server's id of the card (the PipeWire object id), for `wpctl set-profile`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_pw_dump` (src/sound_server.rs:40), `parse_pactl_cards` (src/sound_server.rs:72), `judge` (src/sound_server.rs:99).

**See also:** test `pactl_cards_are_parsed_per_block_and_the_check_reports_what_it_could_not_inspect (src/sound_server.rs)`.

### `Exposure` [@ANCHOR: vnwa_open:vnwa_open_sound_server_exposure]

**Purpose and location.** What the check found. Defined in `daemons/vnwa_open/src/sound_server.rs:23` as a pub `enum`. It derives Clone, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Protected` | `Protected` | The sound server lists none of the VNWA codecs. |
| `Exposed` | `Exposed(Vec<Card>)` | The sound server lists these VNWA codecs (the list of cards). |
| `Unknown` | `Unknown(String)` | Could not tell; the text says why. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `judge` (src/sound_server.rs:99), `check` (src/sound_server.rs:120), `protected` (src/sound_server.rs:150), `warning` (src/sound_server.rs:160).

**See also:** test `a_pw_dump_with_the_unit_codec_is_exposed_but_a_dongle_elsewhere_is_not_the_units (src/sound_server.rs)`.

### `SaPoint` [@ANCHOR: vnwa_open:vnwa_open_spectrum_sa_point]

**Purpose and location.** One point of a spectrum. Defined in `daemons/vnwa_open/src/spectrum.rs:29` as a pub `struct`. It derives Clone, Copy, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | The LO frequency of the point. |
| `rms` | `f64` | The rms of the audio in the band from `DC_GUARD_HZ` to the requested band, in codec counts. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_band_rms_of_a_tone_is_its_rms_inside_the_band_and_nothing_outside_it (src/spectrum.rs)`.

### `Config` [@ANCHOR: vnwa_open:vnwa_open_sweep_config]

**Purpose and location.** Everything that is fixed for one measurement session: which codecs and hub port, the IF, point time, RF level, retry limits and timing. Defined in `daemons/vnwa_open/src/sweep.rs:23` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `codec_s11` | `Option<String>` | Part of the ALSA id of the S11 codec; `None` finds it by its place on the USB tree. |
| `codec_s21` | `Option<String>` | Same for the S21 codec. |
| `hub` | `Option<HubPort>` | Power-switching hub port; with it a failing point gets one cold reset. |
| `retries` | `u32` | How many times a failed point is measured again. |
| `if_hz` | `f64` | The measurement IF in hertz. |
| `point_ms` | `f64` | The time per point sent to the unit (0.2 to 100 ms). |
| `rf_level` | `u16` | The 14-bit RF level. |
| `settle_ms` | `Option<f64>` | Delay before the analysis window starts; `None` takes one point time plus 11 ms. |
| `window_ms` | `Option<f64>` | Analysis window length; `None` takes two point times. |
| `min_reference` | `f64` | Smallest reference amplitude accepted as a signal. |
| `capture` | `crate::capture::Options` | Capture options (ALSA period). |
| `overrun_retries` | `u32` | Retries after a buffer overrun inside a point's window (never leads to a power cycle). |
| `discard_points` | `u32` | Throw-away measurements at the start frequency before a sweep. |
| `first_wait_ms` | `f64` | Wait before them, in milliseconds. |
| `stall_pauses_ms` | `Vec<u64>` | Pauses between tries of the 0x0d read. |
| `min_gap_ms` | `f64` | Shortest time from the end of one point's send to the start of the next. |

**Invariants, ownership and concurrency.** Plain data cloned into the `Vna`; read-only during a sweep.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/sweep.rs:72), `check` (src/sweep.rs:111), `open` (src/sweep.rs:229), `open` (src/sweep.rs:457), `with_parts` (src/sweep.rs:465), `with_session` (src/sweep.rs:469), `fast_config` (src/sweep.rs:1099).

**See also:** test `the_sweep_asks_the_audio_layer_for_a_1024_frame_period (src/sweep.rs)`.

### `Point` [@ANCHOR: vnwa_open:vnwa_open_sweep_point]

**Purpose and location.** One measured point. Defined in `daemons/vnwa_open/src/sweep.rs:178` as a pub `struct`. It derives Clone, Copy, Debug.

| Field | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | Frequency of the point in hertz. |
| `s11` | `Complex` | Measured S11 (raw, before calibration). |
| `s21` | `Complex` | Measured S21 (raw). |
| `if_error_hz` | `f64` | The larger IF error of the two reference channels (Hz) and the larger relative level change between the window's halves. |
| `level_change` | `f64` | The larger relative level change between the window's halves. |

**Invariants, ownership and concurrency.** A result value returned per point; `Copy`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `worst` (examples/soak.rs:324), `correct` (src/cal.rs:526), `measure` (src/sweep.rs:512), `measure_once` (src/sweep.rs:551).

**See also:** test `a_point_is_corrected_in_each_parameter_the_calibration_covers (src/cal.rs)`.

### `Recovery` [@ANCHOR: vnwa_open:vnwa_open_sweep_recovery]

**Purpose and location.** What the unit needed to get a result, for the caller to report. Defined in `daemons/vnwa_open/src/sweep.rs:202` as a pub `struct`. It derives Clone, Copy, Debug, Default, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `remeasured` | `u32` | Points measured again after a failed attempt. |
| `transfer_resends` | `u32` | Control transfers sent again after a transient error (EPROTO and the like). |
| `power_cycles` | `u32` | Cold resets through the hub. |
| `overrun_retries` | `u32` | Points measured again because a buffer overrun fell inside their window (frames lost). |
| `overruns_reported` | `u64` | Buffer overruns the audio layer reported on the two codecs since the handle was opened, wherever they fell; each one inside a window cost a retry or failed the point, so none passes silently. |

**Invariants, ownership and concurrency.** Counters the `Vna` accumulates for the caller to report; reset only by creating a new handle.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tally` (src/sweep.rs:272), `tally` (src/sweep.rs:380), `recovery` (src/sweep.rs:501), `tally` (src/sweep.rs:767), `tally` (src/sweep.rs:933).

**See also:** test `a_recovery_that_worked_still_reports_every_failure_it_had (src/sweep.rs)`.

### `Session` [@ANCHOR: vnwa_open:vnwa_open_sweep_session]

**Purpose and location.** The unit and its two codecs, opened together; all three are reopened after a power cycle. Defined in `daemons/vnwa_open/src/sweep.rs:217` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `usb` | `Vnwa` | The open unit. |
| `s11` | `Capture` | The S11 codec capture. |
| `s21` | `Capture` | The S21 codec capture. |

**Invariants, ownership and concurrency.** Crate-internal; the three are opened together and all reopened after a power cycle, so a `Vna` never holds a half-reopened session.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (src/sweep.rs:229), `with_session` (src/sweep.rs:469).

**See also:** test `a_failed_attempt_is_repeated_and_only_a_success_is_returned (src/sweep.rs)`.

### `Vna` [@ANCHOR: vnwa_open:vnwa_open_sweep_vna]

**Purpose and location.** The measurement handle: the open session, its configuration, and the evidence kept for recovery and failure reports. Defined in `daemons/vnwa_open/src/sweep.rs:397` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `session` | `Session` | The opened unit and codecs. |
| `cfg` | `Config` | The session configuration. |
| `recovery` | `Recovery` | Counters of what recovery was needed. |
| `failures` | `Vec<FailureRecord>` | Records of points that needed more than one attempt. |
| `power_log` | `Vec<PowerRecord>` | Records of power cycles the recovery did itself. |
| `last_levels` | `std::sync::Mutex<String>` | Raw levels of the last attempt (behind a mutex). |
| `last_send_ms` | `std::sync::Mutex<f64>` | Duration of the last block send (behind a mutex). |
| `last_send_end` | `std::sync::Mutex<Option<std::time::Instant>>` | When the last send ended (behind a mutex). |
| `attempt_log` | `std::sync::Mutex<Vec<(f64, bool)>>` | (send time ms, success) of recent attempts (behind a mutex). |

**Invariants, ownership and concurrency.** One owner measures at a time (methods take `&mut self` where they change recovery or evidence); the four mutex fields allow the analysis path to read them through `&self`. Holds real hardware handles; a power cycle reopens them.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `temp` (examples/soak.rs:365), `drain` (examples/soak.rs:381), `open` (src/sweep.rs:457), `with_parts` (src/sweep.rs:465), `with_session` (src/sweep.rs:469).

**See also:** test `a_session_opens_a_point_is_traced_and_a_loopback_tone_is_metered (tests/hil_sweep.rs)`.

### `FailureRecord` [@ANCHOR: vnwa_open:vnwa_open_sweep_failure_record]

**Purpose and location.** A point that needed more than one attempt (or a power cycle) and was measured in the end: every failed attempt with its evidence. Defined in `daemons/vnwa_open/src/sweep.rs:419` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `epoch_s` | `u64` | Seconds since the epoch when the point finally succeeded. |
| `what` | `String` | Which point and what was being measured. |
| `attempts` | `Vec<String>` | One entry per failed attempt (and per power cycle) in order, each with the error text and the evidence appended by `Vna`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `failure_log` (src/sweep.rs:485).

**See also:** test `a_recovery_that_worked_still_reports_every_failure_it_had (src/sweep.rs)`.

### `PowerRecord` [@ANCHOR: vnwa_open:vnwa_open_sweep_power_record]

**Purpose and location.** A power cycle the recovery did by itself (a scheduled one is the caller's). Defined in `daemons/vnwa_open/src/sweep.rs:429` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `epoch_s` | `u64` | Seconds since the epoch when the cycle ended. |
| `reason` | `String` | Why the recovery cycled power. |
| `seconds` | `f64` | From power-off until the unit and both codecs were open again. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `power_log` (src/sweep.rs:496).

**See also:** test `when_the_retries_are_used_up_the_unit_is_power_cycled_once_and_then_tried_again (src/sweep.rs)`.

### `TestDir` [@ANCHOR: vnwa_open:vnwa_open_testdir_test_dir]

**Purpose and location.** A temporary directory for tests that removes itself when dropped. Defined in `daemons/vnwa_open/src/testdir.rs:8` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `0` | `PathBuf` | The directory path. |

**Invariants, ownership and concurrency.** Drop removes the tree, also while a failing test unwinds. Derefs to `Path`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tmp` (src/reserved.rs:73), `unit_tree` (src/sound_server.rs:188), `new` (src/testdir.rs:11).

**See also:** test `sysfs_is_read_for_the_codec_vendor_only (src/topology.rs)`.

### `Window` [@ANCHOR: vnwa_open:vnwa_open_timedomain_window]

**Purpose and location.** The windowing function applied over the used frequency span before the time-domain transform; the shapes are the textbook definitions (Blackman squared is the square of Blackman). Defined in `daemons/vnwa_open/src/timedomain.rs:21` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Rectangular` | `Rectangular` | No window. |
| `Hamming` | `Hamming` | Hamming window. |
| `Hanning` | `Hanning` | Hanning (Hann) window. |
| `Blackman` | `Blackman` | Blackman window. |
| `BlackmanSquared` | `BlackmanSquared` | The Blackman window squared. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/timedomain.rs:30).

**See also:** test `the_windows_have_their_textbook_shapes (src/timedomain.rs)`.

### `Mode` [@ANCHOR: vnwa_open:vnwa_open_timedomain_mode]

**Purpose and location.** The kind of time-domain transform: low-pass (response begins at DC) or band-pass (swept band only). Defined in `daemons/vnwa_open/src/timedomain.rs:65` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `LowPass` | `LowPass` | Uses the response's symmetry to give a real time response; the data must reach nearly DC (a grid starting at a multiple of its step is extended down to DC). |
| `BandPass` | `BandPass` | Uses the positive frequencies only; the result is complex and its magnitude is what is used. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `installer` (src/install.rs:468).

**See also:** test `a_delay_line_gives_a_peak_at_its_delay_in_both_modes_and_windows_lower_the_sidelobes (src/timedomain.rs)`.

### `Response` [@ANCHOR: vnwa_open:vnwa_open_timedomain_response]

**Purpose and location.** Which time-domain response is computed: the impulse response or the step response. Defined in `daemons/vnwa_open/src/timedomain.rs:71` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Impulse` | `Impulse` | The impulse response. |
| `Step` | `Step` | The step response; only low-pass mode applies to it (band-pass is refused). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_delay_line_gives_a_peak_at_its_delay_in_both_modes_and_windows_lower_the_sidelobes (src/timedomain.rs)`.

### `CodecCard` [@ANCHOR: vnwa_open:vnwa_open_topology_codec_card]

**Purpose and location.** One ALSA card on a USB codec: its card id and its USB port path (for example `3-2.2.3`). Defined in `daemons/vnwa_open/src/topology.rs:15` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `card_id` | `String` | The ALSA card id. |
| `usb_path` | `String` | The USB port path, for example `3-2.2.3`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `assign` (src/topology.rs:32), `find_cards` (src/topology.rs:71), `card` (src/topology.rs:116).

**See also:** test `the_codec_on_the_lower_port_is_s11_whatever_the_card_order (src/topology.rs)`.

### `Codecs` [@ANCHOR: vnwa_open:vnwa_open_topology_codecs]

**Purpose and location.** The ALSA ids (`hw:CARD=<id>,`) of the two codecs. Defined in `daemons/vnwa_open/src/topology.rs:22` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `s11` | `String` | ALSA id (`hw:CARD=<id>,`) of the S11 codec. |
| `s21` | `String` | ALSA id of the S21 codec. |

**Invariants, ownership and concurrency.** Result of finding the two codecs by place on the USB tree.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `assign` (src/topology.rs:32), `codecs` (src/topology.rs:108).

**See also:** test `the_codec_on_the_lower_port_is_s11_whatever_the_card_order (src/topology.rs)`.

### `RawPoint` [@ANCHOR: vnwa_open:vnwa_open_trace_raw_point]

**Purpose and location.** One point's raw audio and timing. Defined in `daemons/vnwa_open/src/trace.rs:12` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `codec1` | `Vec<[i16; 2]>` | The frames of codec 1 and codec 2 from `before_ms` before the send began, `[left, right]` each. |
| `codec2` | `Vec<[i16; 2]>` | The frames of codec 2, `[left, right]` each. |
| `send_ms` | `f64` | How long the block and the 0x0d read took to send, in milliseconds. |
| `send_end_frame` | `usize` | Where the send ended in the frames: the frame index of the end of the send, from the clock. |

**Invariants, ownership and concurrency.** Plain data produced when raw capture is on, one per point.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_if_tone_starts_61_ms_and_the_18_khz_burst_1_ms_after_the_end_of_the_send_in_a_recorded_point (src/trace.rs)`.

### `TransferError` [@ANCHOR: vnwa_open:vnwa_open_transport_transfer_error]

**Purpose and location.** Why a control transfer failed, as far as recovery is concerned. Defined in `daemons/vnwa_open/src/transport.rs:14` as a pub `enum`. It derives Clone, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Transient` | `Transient(String)` | Worth sending again (EPROTO, stall, timeout); carries the text. |
| `NoDevice` | `NoDevice(String)` | The unit is gone from the bus; carries the text. |
| `Other` | `Other(String)` | Anything else; sending again will not help; carries the text. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `reset` (src/generator.rs:373), `read` (src/generator.rs:388), `reset` (src/generator.rs:438), `reset` (src/generator.rs:635), `reset` (src/generator.rs:711), `reset` (src/meter.rs:259), `reset` (src/monitor.rs:91), `write` (src/monitor.rs:94).

**See also:** test `a_usb_reset_ends_the_modulation_stall_and_a_unit_that_stays_stalled_is_reported (src/generator.rs)`.

### `RusbTransport` [@ANCHOR: vnwa_open:vnwa_open_transport_rusb_transport]

**Purpose and location.** The production `Transport`: control transfers through libusb (rusb) on an open device handle. Defined in `daemons/vnwa_open/src/transport.rs:70` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `handle` | `rusb::DeviceHandle<rusb::Context>` | The open rusb device handle. |

**Invariants, ownership and concurrency.** Talks to real hardware; owned by one `Vnwa`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (src/transport.rs:75).

**See also:** test `libusb_errors_are_classified (src/usb.rs)`.

### `Seen` [@ANCHOR: vnwa_open:vnwa_open_transport_seen]

**Purpose and location.** One transfer seen by `Scripted`: the request number and whether it was a write. Defined in `daemons/vnwa_open/src/transport.rs:118` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `write` | `bool` | True for a host-to-device transfer. |
| `request` | `u8` | The request number. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `seen` (src/transport.rs:137).

**See also:** test `the_captured_am_sequence_is_sent_in_order_and_a_stalled_last_request_is_not_an_error (src/generator.rs)`.

### `Scripted` [@ANCHOR: vnwa_open:vnwa_open_transport_scripted]

**Purpose and location.** Plays back outcomes in order; once the script is used up every transfer succeeds with the bytes of the last `Ok` (or none). Defined in `daemons/vnwa_open/src/transport.rs:124` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `outcomes` | `Mutex<VecDeque<Result<Vec<u8>, TransferError>>>` | Queue of outcomes (bytes or an error) played back in order, behind a mutex. |
| `seen` | `Mutex<Vec<Seen>>` | Log of the transfers made, behind a mutex. |

**Invariants, ownership and concurrency.** Test double for `Transport`; a mutex around each field makes it usable through `&self`.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/transport.rs:130), `unit` (src/usb.rs:209).

**See also:** test `a_frequency_whose_tuning_word_does_not_fit_is_refused_before_anything_is_sent (src/generator.rs)`.

### `Vnwa` [@ANCHOR: vnwa_open:vnwa_open_usb_vnwa]

**Purpose and location.** The unit: a transport plus the resend policy for transient transfer errors. Defined in `daemons/vnwa_open/src/usb.rs:26` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `link` | `Box<dyn Transport>` | The boxed transport (libusb or a test double). |
| `resends` | `AtomicU32` | Number of transfers sent again after a transient error (atomic). |
| `recent` | `Mutex<VecDeque<String>>` | The last `TRANSFER_LOG` tries with their outcome, behind a mutex. |

**Invariants, ownership and concurrency.** Methods take `&self`; counters are atomic and the log is mutex-protected, so it can be read while measuring.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run` (src/generator.rs:93), `set_frequency` (src/generator.rs:102), `set_level` (src/generator.rs:115), `on` (src/generator.rs:121), `retune` (src/generator.rs:131), `set_attenuation` (src/generator.rs:136), `answers` (src/generator.rs:298), `am_captured_500hz_100pct` (src/generator.rs:338).

**See also:** test `a_usb_reset_ends_the_modulation_stall_and_a_unit_that_stays_stalled_is_reported (src/generator.rs)`.
