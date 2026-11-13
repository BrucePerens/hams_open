# Digital mode data structures (D-STAR header, FT8, PSK31, RTTY, WSPR, timing)

As a developer changing this code, I want every data structure in `src/dstar.rs`, `src/ft8.rs`, `src/ft8/ffi.rs`, `src/psk31.rs`, `src/rtty.rs`, `src/timing_characterizer.rs`, `src/wspr.rs`, `src/wspr_decode.rs` and `src/wspr_sync.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests cite the same anchors.

## `DStarHeader` [@ANCHOR: ham_digital_modes:dstar_dstar_header_struct]

**Purpose.** One D-STAR routing header (KM4ML's DV packet structure doc's own field order): 3 opaque flag bytes, then four space-padded ASCII callsign-shaped fields. All four callsign fields share the same real-world constraint (an amateur radio callsign, `RPT1`/`RPT2` a repeater's own suffixed callsign) -- represented as owned `String`s here (already trimmed of the padding used only on the wire), not fixed-size byte arrays, since nothing in this module needs to avoid the allocation and a `String` is far easier for a caller to work with correctly.

**Definition.** `pub struct DStarHeader` (struct) in `daemons/ham_digital_modes/src/dstar.rs`; derives `Debug`, `Clone`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `flags` | `[u8; 3]` | The three opaque flag bytes of the header. |
| `rpt2` | `String` | Destination repeater callsign, ≤8 ASCII characters. |
| `rpt1` | `String` | Departure repeater callsign, ≤8 ASCII characters. |
| `ur_call` | `String` | Companion/target station's own callsign, ≤8 ASCII characters. |
| `my_call` | `String` | Own station's callsign, ≤8 ASCII characters. |
| `my_call_suffix` | `String` | Own station's callsign suffix (module/SSID-shaped), ≤4 ASCII characters. |

**Used by.** `sample_header` (functions whose signature names it).

## `DStarHeaderError` [@ANCHOR: ham_digital_modes:dstar_dstar_header_error_enum]

**Purpose.** Why [`DStarHeader::pack`] refused to build a header.

**Definition.** `pub enum DStarHeaderError` (enum) in `daemons/ham_digital_modes/src/dstar.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `NonAscii` | - | A field held a real character (or byte) outside 7-bit ASCII printable range -- D-STAR callsign fields are ASCII space-padded text, not arbitrary bytes. |
| `FieldTooLong` | - | A field's own real content (excluding the space padding) was longer than the wire format has room for. |

**Used by.** `pack_field` (functions whose signature names it).

## `Ft8Decoder` [@ANCHOR: ham_digital_modes:ft8_ft8_decoder_struct]

**Purpose.** Safe wrapper around one FT8 (a weak-signal digital mode) decoding session of the vendored `ft8_lib`: audio is pushed in with `feed`, decoded messages are pulled out. Defined in `ft8.rs`; marked `Send` (one session may move between threads but is used by one thread at a time, which `&mut self` enforces).

**Definition.** `pub struct Ft8Decoder` (struct) in `daemons/ham_digital_modes/src/ft8.rs`.

| Field | Type | Meaning |
|---|---|---|
| `session` | `*mut ffi::Ft8Session` | Raw pointer to the C session (`ffi::Ft8Session`); owned by this struct and freed on drop. |
| `pending` | `Vec<f32>` | Audio samples buffered until a full block is available. |
| `block_size` | `usize` | Samples per processing block as reported by the C library (checked to be non-zero at construction). |

**Notes.** The C library keeps one process-wide callsign hash table (a documented limitation in `shim.c`).

## `Ft8Session` [@ANCHOR: ham_digital_modes:ffi_ft8_session_struct]

**Purpose.** The opaque handle of the vendored `ft8_lib` C session (`vendor/ft8_lib/shim.c`). Rust never constructs it or reads its fields: it only holds and passes the pointer the C allocator returns (`ft8_session_new`, freed by `ft8_session_free`).

**Definition.** `pub struct Ft8Session` (struct) in `daemons/ham_digital_modes/src/ft8/ffi.rs`.

| Field | Type | Meaning |
|---|---|---|
| `_private` | `[u8; 0]` | Zero-length private field that makes the type opaque (`#[repr(C)]`). |

**Used by.** `ft8_session_block_size`, `ft8_session_decode`, `ft8_session_free`, `ft8_session_new`, `ft8_session_process`, `ft8_session_reset` (functions whose signature names it).

**Notes.** Declared in `ft8/ffi.rs`; the safe wrapper is `Ft8Decoder`.

## `VaricodeAccumulator` [@ANCHOR: ham_digital_modes:psk31_varicode_accumulator_struct]

**Purpose.** Incremental Varicode bit accumulator -- the same boundary-detection logic `psk31_decode_bits` uses, factored out so `Psk31Decoder` can feed it one bit at a time across many separate `feed()` calls instead of needing the whole bitstream up front.

**Definition.** `struct VaricodeAccumulator` (struct) in `daemons/ham_digital_modes/src/psk31.rs`.

| Field | Type | Meaning |
|---|---|---|
| `acc` | `u16` | The Varicode bits of the character in progress, shifted in at the bottom in arrival order. |
| `count` | `u8` | How many bits are in `acc`. |
| `prev_was_zero` | `bool` | Whether the previous bit was 0; two zeros in a row mark a character boundary in Varicode. |

## `Psk31Decoder` [@ANCHOR: ham_digital_modes:psk31_psk31_decoder_struct]

**Purpose.** Stateful, incremental BPSK31 demodulator for a continuous audio stream delivered across many separate calls (e.g. a live audio pipeline processing small chunks at a time, where a plain whole-buffer `psk31_decode_audio` call per chunk would incorrectly reset the carrier phase reference and Varicode bit accumulator on every single chunk boundary, corrupting anything that didn't happen to align with one). Carries symbol timing, carrier phase, and partially-accumulated Varicode bits across `feed()` calls.

**Definition.** `pub struct Psk31Decoder` (struct) in `daemons/ham_digital_modes/src/psk31.rs`.

| Field | Type | Meaning |
|---|---|---|
| `carrier_hz` | `f64` | The carrier frequency in Hz. |
| `sample_rate` | `u32` | The audio sample rate in Hz. |
| `samples_per_symbol` | `usize` | Samples in one 31.25 baud symbol at that rate. |
| `pending_samples` | `Vec<i16>` | Audio received but not yet consumed (less than one symbol). |
| `sample_idx` | `u64` | Running sample count since the start, used to keep the carrier phase reference continuous across calls. |
| `prev_phase` | `Option<f64>` | Phase of the previous symbol (`None` before the first), for differential detection. |
| `varicode` | `VaricodeAccumulator` | The partially accumulated Varicode bits (`VaricodeAccumulator`). |

**Notes.** One per stream; `feed` takes `&mut self`.

## `TrigTables` [@ANCHOR: ham_digital_modes:rtty_trig_tables_struct]

**Purpose.** Precomputed per-sample I/Q correlator tables for one fixed `(mark_hz, space_hz, sample_rate)` triple, indexed by *window-relative* sample offset (0..capacity). Valid for a window of any length up to `capacity` starting anywhere in the stream, since the correlator phase reference `t = n / sample_rate` always counts from the window's own first sample, not an absolute stream position -- exactly the phase convention `window_mark_space_energy` already used, just computed once here instead of by every call. This removes a real inefficiency `frame_score` below would otherwise pay many times per character: several overlapping windows are evaluated per candidate edge, and a per-edge local-peak refinement (see `frame_score`'s own doc comment) evaluates the same shape of window at many nearby offsets -- recomputing `cos`/`sin` from scratch each time was pure waste once the frame- matched detector replaced the old single-window-per-candidate design.

**Definition.** `struct TrigTables` (struct) in `daemons/ham_digital_modes/src/rtty.rs`.

| Field | Type | Meaning |
|---|---|---|
| `cos_mark` | `Vec<f64>` | Cosine of the mark tone at each window-relative sample offset. |
| `sin_mark` | `Vec<f64>` | Sine of the mark tone at each offset. |
| `cos_space` | `Vec<f64>` | Cosine of the space tone at each offset. |
| `sin_space` | `Vec<f64>` | Sine of the space tone at each offset. |

**Used by.** `attempt_character`, `frame_score`, `pre_is_persistently_mark` (functions whose signature names it).

## `TrigBank` [@ANCHOR: ham_digital_modes:rtty_trig_bank_struct]

**Purpose.** A small bank of `TrigTables`, one per offset in `offsets`, all built from the same configured `mark_hz`/`sample_rate` -- the real mechanism the AFC search in `rtty_scan` scans across. Built once per decoder lifetime (`RttyDecoder::new`/`new_fixed`) or once per whole-buffer call (`rtty_demodulate`), the same real reason `TrigTables` itself moved out of the per-window hot path: `sin`/`cos` tables are expensive to recompute and never change once `mark_hz` and `sample_rate` are fixed. `offsets` is carried alongside the tables (not just consulted via the global `AFC_OFFSETS_HZ` constant) so `RttyDecoder::locked_frequency_offset_hz` reports the right value regardless of which offset list this particular decoder was built with -- a real bug this shape avoids: indexing a fixed single-table bank's `locked_offset_idx` (always 0) into the 9-entry `AFC_OFFSETS_HZ` array would silently report -40.0Hz instead of the true 0.0Hz.

**Definition.** `struct TrigBank` (struct) in `daemons/ham_digital_modes/src/rtty.rs`.

| Field | Type | Meaning |
|---|---|---|
| `tables` | `Vec<TrigTables>` | One `TrigTables` per AFC frequency offset. |
| `offsets` | `&'static [f64]` | The AFC offsets in Hz the tables belong to (carried so the locked offset is reported correctly). |

**Used by.** `best_frame_evidence`, `rtty_scan` (functions whose signature names it).

## `FrameGeometry` [@ANCHOR: ham_digital_modes:rtty_frame_geometry_struct]

**Purpose.** Real sample-domain geometry for one bit period at a given sample rate, computed once per scan rather than re-derived at every candidate position.

**Definition.** `struct FrameGeometry` (struct) in `daemons/ham_digital_modes/src/rtty.rs`.

| Field | Type | Meaning |
|---|---|---|
| `samples_per_bit` | `f64` | Samples in one bit period at the decoder's sample rate (fractional). |
| `window_len` | `usize` | Length in samples of one data-bit-period window, rounded. |
| `stop_window_len` | `usize` | Length in samples of the stop bit window (`RTTY_STOP_BIT_UNITS`, 1.5 bit units). |

**Used by.** `attempt_character`, `best_frame_evidence`, `frame_score`, `pre_is_persistently_mark`, `rtty_scan` (functions whose signature names it).

## `FrameEvidence` [@ANCHOR: ham_digital_modes:rtty_frame_evidence_struct]

**Purpose.** Combined frame-matched confidence for treating `edge_pos` as a real character's start-bit edge -- the real "processing gain" move this decoder is built on. The old design accepted a candidate based on a single mark->space transition (a hard 1-bit decision) plus a separate, amplitude-dependent absolute-energy floor bolted on afterward; this instead requires two independent kinds of evidence to agree, each built from RTTY (radio teletype)'s own known frame structure rather than a single instantaneous sample: `pre_is_persistently_mark` (a real *sustained*- tone check, immune to any one window's own fat-tailed dB blowup -- see its own doc comment), and this function's own clipped start+stop magnitude sum (the start bit must read SPACE, the stop bit must read MARK across its full real `RTTY_STOP_BIT_UNITS` duration, each clipped to `PER_WINDOW_CLIP_DB` so neither window alone can dominate the sum). Because both checks are built from signed *ratios*, not absolute energy, the same thresholds work unchanged across the full amplitude sweep the old design's own `#[ignore]`d test measured it failing on.  The three scores `frame_score` produces for one candidate edge, kept separate because they serve different purposes and would corrupt each other if merged: `accept_score` decides *whether* this is a real character (must be robust against fat-tailed noise outliers, so every term is clipped -- see `PER_WINDOW_CLIP_DB`'s own doc comment); `raw_score` finds *where* the real edge actually is, at a single fixed frequency hypothesis (an unclipped matched-filter output genuinely peaks at the true alignment, but clipping flattens that peak into a plateau -- picking a position off a flat plateau via a strict `>` comparison systematically biases the chosen position toward the plateau's first/leftmost point, up to a full grid step of avoidable timing error); `magnitude` picks *which frequency hypothesis* among several that all already pass `accept_score` is the real best match -- see `best_frame_evidence`'s own doc comment for why `accept_score` itself is unsuitable for that comparison (it saturates, making nearby frequency candidates falsely indistinguishable), and why `magnitude` is real *linear* correlator energy (`mark_e + space_e`, summed across all 7 windows), not a sum of dB values -- a matched filter's own frequency-mismatch rolloff is a real, physical statement about linear energy (`sinc`-shaped in frequency offset), and only shows up as a small, noise-comparable difference once compressed into dB and clipped/summed the way `accept_score` is.

**Definition.** `struct FrameEvidence` (struct) in `daemons/ham_digital_modes/src/rtty.rs`.

| Field | Type | Meaning |
|---|---|---|
| `accept_score` | `f64` | Decides whether the candidate edge is a real character: a clipped sum of the start-bit and stop-bit magnitudes, robust against noise outliers. |
| `raw_score` | `f64` | Unclipped matched-filter score at one fixed frequency hypothesis, used to find where the edge actually is. |
| `magnitude` | `f64` | Linear correlator energy (`mark_e + space_e`) summed over the 7 windows, used to choose between frequency hypotheses that all pass `accept_score`. |

**Used by.** `best_frame_evidence`, `frame_score` (functions whose signature names it).

**Notes.** Produced by `frame_score` for one candidate start-bit edge; private to `rtty.rs`.

## `ScanState` [@ANCHOR: ham_digital_modes:rtty_scan_state_struct]

**Purpose.** Resumable scan state, carried across `RttyDecoder::feed()` calls (a fresh, default one is used for the one-shot `rtty_demodulate`).

**Definition.** `struct ScanState` (struct) in `daemons/ham_digital_modes/src/rtty.rs`.

| Field | Type | Meaning |
|---|---|---|
| `pos` | `usize` | Sample index in the pending buffer where the scan resumes. |
| `current_figs` | `bool` | True while the decoder is in the FIGS (figures) shift, false in LTRS (letters). |
| `figs_non_digit_run` | `usize` | Consecutive non-digit FIGS characters decoded while `current_figs` is true -- the "sensible USOS" extension's own evidence counter (see `attempt_character`'s own doc comment for the reasoning). Real ham FIGS content is almost entirely digits (signal reports, serials, frequencies); a run of non-digit FIGS punctuation is a strong signal of a missed LTRS shift, not real transmitted content. Reset to 0 by anything that legitimately re-synchronizes shift state: an explicit shift code either direction, a digit, USOS's own space-triggered unshift, or this counter's own trigger firing. |
| `locked_offset_idx` | `usize` | Index into `AFC_OFFSETS_HZ`/`TrigBank::tables` of whichever candidate frequency won the most recent successful character decode -- purely informational (`rtty_scan`'s own AFC search re-evaluates the full bank at every candidate position regardless, so this is never consulted to narrow that search), tracked so a caller (or a future one) can report the real, currently-estimated off-air frequency offset instead of assuming zero. |

**Used by.** `attempt_character`, `rtty_scan` (functions whose signature names it).

## `RttyDecoder` [@ANCHOR: ham_digital_modes:rtty_rtty_decoder_struct]

**Purpose.** Stateful, incremental RTTY demodulator for a continuous audio stream delivered across many separate `feed()` calls -- the same reason `Psk31Decoder` exists for PSK31 (a plain whole-buffer decode per chunk would reset framing state at every chunk boundary), but with a real, RTTY-specific twist `Psk31Decoder` doesn't have: PSK31 decodes one fixed-size symbol at a time, so "have I received enough samples yet" is a simple length check. RTTY's start-bit framing is asynchronous -- a real character can begin at any sample offset -- so `feed()` must be able to recognize "this looks like a real start-bit edge, but I don't have enough buffered audio yet to confirm the whole character" and wait for the next call rather than either guessing wrong or skipping past the edge (see `rtty_scan`'s own doc comment for the mechanism). One real, honest consequence: the very last character of a burst can lag by up to one character's own duration (~150ms at 45.45 baud) behind when its audio actually finished arriving, appearing on the *next* `feed()` call instead -- not data loss, the same kind of small fixed latency `Psk31Decoder` already has waiting for a full symbol.  Unlike the original design, this carries no separate presence-gate state at all -- the frame-matched detector's own aggregate confidence threshold (see `frame_score`'s doc comment) *is* the presence/confidence gate now, amplitude-invariant by construction, so there's no floor to warm up, contaminate, or desynchronize from `ScanState`.  `geo`/`bank` are built once here, not per `feed()` call: `mark_hz`/`sample_rate` are fixed for the life of a decoder, so `TrigBank::new`'s `sin`/`cos` table construction (9 candidate frequencies -- see `AFC_OFFSETS_HZ`'s own doc comment -- at a real 48kHz/45.45-baud geometry) would otherwise repeat on every single `feed()` call from a live pipeline -- for `digital_decoder.rs`'s own ~20ms callback cadence, that is real, avoidable, per-callback work for a value that never changes after construction.

**Definition.** `pub struct RttyDecoder` (struct) in `daemons/ham_digital_modes/src/rtty.rs`.

| Field | Type | Meaning |
|---|---|---|
| `sample_rate` | `u32` | The audio sample rate in Hz. |
| `pending_samples` | `Vec<i16>` | Audio received but not yet consumed (an unconfirmed character may be waiting here for more audio). |
| `state` | `ScanState` | The resumable scan state (`ScanState`). |
| `geo` | `FrameGeometry` | The bit-period geometry (`FrameGeometry`), computed once at construction. |
| `bank` | `TrigBank` | The correlator tables for the candidate frequency offsets (`TrigBank`), computed once at construction. |

## `RttySignature` [@ANCHOR: ham_digital_modes:rtty_rtty_signature_struct]

**Purpose.** Sign-flip rate and run-length histogram of the mark/space ratio at one candidate frequency, hopped through in non-overlapping ~1-bit-period windows -- a cheap, decode-free RTTY *presence* signature, not a decoder. Per Bruce's own real-time direction ("Look at ways of recognizing RTTY cheaply without a full decoder... characterize the signal and then bring in the decoders that make sense"). No AFC search, no persistence gating, no character framing at all -- ~23us/call for a 960-sample chunk at one candidate (measured, see `probe_rtty_signature_separates_signal_noise_and_tone` below), a real order of magnitude cheaper than even `RttyDecoder::new_fixed`'s own per-character framing.  **Real, measured, honest finding, not the hypothesis this was built to test**: the original reasoning (real RTTY's own bit-period keying should cluster run lengths at 1-3 hops, distinct from noise's near-random-walk ~50% flip rate) does NOT hold up against measurement. `probe_rtty_signature_separates_signal_noise_and_tone` measured real RTTY at a 0.531 sign flip rate against real Gaussian noise at 0.473 -- both close to the 0.5 a symmetric random process produces, with similar-shaped run-length histograms, not the sharp separation expected. In hindsight this makes sense: arbitrary text's own data bits are themselves close to random content, so a real character's bit-level MARK/SPACE sequence isn't meaningfully more "clustered" than noise at this coarse, single-hop-at-a-time granularity -- only the *framing structure* (fixed start/stop bit positions, a stable 45.45-baud clock) actually distinguishes real RTTY from noise, and this signature doesn't look at either. The one case this DOES cleanly separate is a steady, unkeyed carrier (idle MARK: 0.0 flip rate, one giant run) from anything keyed -- a real, if narrower, win. A signature that actually separates signal from noise would need to test for bit-clock periodicity (do flips recur near integer multiples of one hop width, not just how often they occur) rather than raw flip rate -- genuinely more work, not attempted here. Kept and reported honestly rather than deleted: a negative result that took real measurement to find is exactly the kind of thing worth keeping in the code where the next person will actually see it before re-deriving it.  Deliberately NOT wired into `RttyDecoder`, `rtty_scan`, or any pipeline: this is the measurement step, not a shipped gating policy, and per the finding above, not yet a policy that would work if it were wired in.

**Definition.** `struct RttySignature` (struct) in `daemons/ham_digital_modes/src/rtty.rs`; derives `Debug`, `Clone`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `sign_flip_rate` | `f64` | Fraction of adjacent hops whose sign differs, in `[0.0, 1.0]`. |
| `run_length_histogram` | `[usize; RttySignature::RUN_LENGTH_HISTOGRAM_BUCKETS]` | `run_length_histogram[i]` is the count of runs exactly `i + 1` hops long, for `i` up to `RUN_LENGTH_HISTOGRAM_BUCKETS - 1`; the last bucket accumulates every run of at least that length instead of growing the histogram unboundedly for the idle-MARK case. |

**Used by.** `characterize_rtty_signature` (functions whose signature names it).

## `SymbolClock` [@ANCHOR: ham_digital_modes:timing_characterizer_symbol_clock_struct]

**Purpose.** One measured symbol-clock hypothesis.

**Definition.** `pub struct SymbolClock` (struct) in `daemons/ham_digital_modes/src/timing_characterizer.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `baud` | `f64` | The best-fit symbol rate, in baud (symbols/second). |
| `confidence` | `f64` | How many times the winning lag's |autocorrelation| exceeds the mean |autocorrelation| across the whole scanned lag range -- not an absolute probability, and only meaningful relative to a noise-floor measurement taken the same way (same feature extractor, same lag range).  This is "peak / mean," not a z-score ("(peak - mean) / std") -- that was tried first and measured WORSE at separating real RTTY from noise: RTTY's autocorrelation isn't a single clean spike against a flat floor, it has real (if smaller) correlation at neighboring sub-harmonic lags too (English text's own character/word rhythm bleeding into the profile), which inflates the z-score's mean AND std together and shrinks the apparent peak right along with the noise it's supposed to be measured against. Peak/mean is less sensitive to that because it only divides by the mean, not by a spread that the same contamination also inflates. Measured directly in `probe_dominant_symbol_period_separates_rtty_cw_and_noise`: z-score gave RTTY 3.22 vs. a 20-seed noise floor of mean 2.66/max 3.34 (overlapping -- not usable), while peak/mean on the same data gave RTTY 5.13 vs. noise mean 3.00/max 3.92 (clean separation), and CW 12.18 vs. noise mean 2.88/max 3.76 (also clean). Re-measure before changing this back. |

**Used by.** `dominant_symbol_period` (functions whose signature names it).

## `SplitMix64` [@ANCHOR: ham_digital_modes:wspr_split_mix64_struct]

**Purpose.** A small, deterministic (seeded) PRNG -- splitmix64 -- used only to generate reproducible AWGN test fixtures below. Not cryptographic and not used anywhere outside test/fixture generation; deliberately self-contained rather than pulling in the `rand` crate for this one narrow, test-only need (this crate otherwise has zero runtime dependencies beyond `libm`).

**Definition.** `struct SplitMix64(u64);` (struct) in `daemons/ham_digital_modes/src/wspr.rs`.

## `GaveUp` [@ANCHOR: ham_digital_modes:wspr_decode_gave_up_struct]

**Purpose.** Returned when the sequential decoder exhausts `max_cycles` node expansions without reaching the end of the trellis. Per this codebase's own fail-fast convention: a decoder that has given up must say so explicitly, never silently return its best-guess-so-far path as if it were a real decode (that's exactly the failure mode that would look like a rare/spurious wrong decode in the field, indistinguishable from a real one without this signal).

**Definition.** `pub struct GaveUp` (struct) in `daemons/ham_digital_modes/src/wspr_decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `cycles` | `u64` | Number of node expansions performed before giving up (`max_cycles`). |

## `StackNode` [@ANCHOR: ham_digital_modes:wspr_decode_stack_node_struct]

**Purpose.** One partial path in the stack algorithm's search. `bits` stores the decoded-so-far bits as a bitset (bit i = decision at trellis depth i) rather than a heap-allocated `Vec<bool>` per node -- WSPR's 81-stage trellis fits comfortably in a u128, and the stack algorithm can generate many thousands of nodes on a noisy channel, so avoiding a per-node allocation is a real, not premature, efficiency concern.

**Definition.** `struct StackNode` (struct) in `daemons/ham_digital_modes/src/wspr_decode.rs`.

| Field | Type | Meaning |
|---|---|---|
| `metric` | `f64` | The path metric so far (higher is better). |
| `depth` | `usize` | Trellis depth reached (number of decoded bits). |
| `state` | `u32` | The convolutional encoder state at this node. |
| `bits` | `u128` | The decoded-so-far bits as a bitset (bit i = decision at depth i); the 81-stage trellis fits in a `u128`. |

**Notes.** Private to the sequential decoder; many thousands may be created per decode on a noisy channel, which is why it holds no heap data.

## `ConfidenceGateError` [@ANCHOR: ham_digital_modes:wspr_decode_confidence_gate_error_enum]

**Purpose.** What `sequential_decode_with_confidence_gate()` returns when the search completed (unlike `GaveUp`, which means it didn't) but the winning path's own metric fell below `MIN_ACCEPTABLE_METRIC` -- distinguished from `GaveUp` deliberately: these are different failure modes with different real causes (see `MIN_ACCEPTABLE_METRIC`'s own doc comment) and a caller may reasonably want to react to them differently (e.g. retry with a longer integration time vs. simply discarding a too-uncertain result). The rejected bits are included, not discarded, for callers that want to log/inspect a low-confidence guess rather than treat it as pure noise.

**Definition.** `pub enum ConfidenceGateError` (enum) in `daemons/ham_digital_modes/src/wspr_decode.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Variant | Type | Meaning |
|---|---|---|
| `GaveUp` | `{ cycles: u64 }` | The search exhausted its cycle budget; `cycles` expansions were made. |
| `LowConfidence` | `{ bits: u128, metric: f64 }` | The search finished but the winning path's metric was below `MIN_ACCEPTABLE_METRIC`; `bits` are the rejected bits and `metric` its metric. |

## `SyncResult` [@ANCHOR: ham_digital_modes:wspr_sync_sync_result_struct]

**Purpose.** The winning (frequency, timing) hypothesis `find_sync()` found, plus its own raw correlation score (higher is a stronger sync match -- not itself a calibrated probability or SNR estimate, just a ranking statistic across candidates within one search). Guaranteed `>=` the `min_sync_score` the caller passed to `find_sync()`.

**Definition.** `pub struct SyncResult` (struct) in `daemons/ham_digital_modes/src/wspr_sync.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Field | Type | Meaning |
|---|---|---|
| `base_hz` | `f64` | Best-fit base frequency of the signal, Hz. |
| `start_sample` | `usize` | Sample index at which the transmission starts. |
| `sync_score` | `f64` | Raw correlation score of the match; a ranking statistic only. |

**Used by.** `find_sync`, `find_sync_uncached_reference` (functions whose signature names it).

## `WsprMessageError` [@ANCHOR: ham_digital_modes:wspr_sync_wspr_message_error_enum]

**Purpose.** What `sync_search_and_decode_message()` returns for a failure -- `ConfidenceGateError`'s own two variants, plus a real, distinct third outcome `ConfidenceGateError` alone can't represent: the decoder reached a confident (metric-accepted) bit pattern, but that bit pattern still doesn't unpack into a valid Type-1 message (`wspr.rs`'s own `unpack_wspr_message()` returned `None`) -- a real "confident in something that isn't a real encodable message" outcome, not the same as either a low-confidence reject or a search give-up.

**Definition.** `pub enum WsprMessageError` (enum) in `daemons/ham_digital_modes/src/wspr_sync.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`.

| Variant | Type | Meaning |
|---|---|---|
| `GaveUp` | `{ cycles: u64 }` | The sequential decoder gave up after `cycles` expansions. |
| `LowConfidence` | `{ bits: u128, metric: f64 }` | The search finished with a metric below the acceptance threshold; the rejected `bits` and `metric` are carried. |
| `UnpackFailed` | `{ bits: u128 }` | A confident bit pattern that does not unpack into a valid Type-1 message; the `bits` are carried. |

**Used by.** `sync_search_and_decode_message` (functions whose signature names it).

