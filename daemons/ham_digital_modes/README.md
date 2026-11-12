# ham_digital_modes

Amateur-radio digital-mode encoders and decoders as a library crate (LGPL-3.0-or-later): PSK31, RTTY, FT8, WSPR (digital amateur-radio modes), the D-STAR header,
Codec2 1600/3200 voice frames, and the AMBE voice codec family (D-STAR, P25 and, behind the `ambe_plus_2` feature, AMBE+2 half-rate).
The hams_local_relay daemon depends on it as a pinned git dependency. `--no-default-features` builds only the fixed-point Codec2 3200
path as `no_std` (see `docs/CODEC2_NO_STD.md`); everything else needs the default `std` feature. `cargo test` runs the unit and
integration tests; `cargo test --features ambe_plus_2` also builds the gated half-rate code.

This README holds the plumbing documentation: the internal DSP (digital signal processing) steps, tables and arithmetic helpers that no operator ever calls
directly, each cited by its semantic anchor (ADR 0090). What an operator gets from each mode (encode a message to audio or bits, decode
audio to text, the frame layouts) is in the user-visible stories under `docs/stories/`:

- `psk31_text_to_audio_and_back.md`, `rtty_text_to_audio_and_back.md`, `ft8_message_codec.md`, `wspr_transmit_and_receive.md`,
  `dstar_header_framing.md`, `signal_timing_characterizer.md`
- `codec2_voice_frames.md`, `codec2_3200_and_ambe_internals.md`
- `ambe_p25_voice_frame_wire_path.md`, `ambe_dstar_and_half_rate_frame_layout.md`, `ambe_pitch_refinement.md`

# Internals

## PSK31 receive internals

All of these are private helpers in `src/psk31.rs`. They are pure functions or small state machines with no I/O, no locks, no
threads and no hardware access, so they are safe to call from any thread; none of them keys a transmitter (the PSK31 transmit side
is `psk31_encode_bits` and the modulator, which are separate anchors). The audio they see is signed 16-bit PCM (`i16`) at a caller
chosen sample rate, and PSK31 itself runs at 31.25 baud (`PSK31_BAUD`).

### `char_to_code` [@ANCHOR: char_to_code]

Looks up the Varicode codeword (the variable-length bit pattern PSK31 uses for each character) and its length for one ASCII byte.
It gives nothing for a byte of 128 or more, which is how `psk31_encode_bits` skips non-ASCII input instead of guessing a code. It is
called by the encoder only; the decoder uses the inverse lookup `code_to_char`.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `u8` | The character byte; only values 0 to 127 have a Varicode entry. |

**Returns:** `Some((code, bitcount))` where `code` is the codeword as a `u16` (most significant bit sent first) and `bitcount` is its length in bits (1 to 10, at most `MAX_VARICODE_BITS`); `None` when `c >= 128`.

**Errors:** None; the `None` return is the only failure form and nothing panics (the table has exactly 128 entries).

**Special concerns:** Pure, non-blocking table lookup with no side effects. The table is `VARICODE`, a constant. It never transmits by itself.

**Signature:** `fn char_to_code(c: u8) -> Option<(u16, u8)>`

**See also:** test `varicode_matches_the_arrl_spec_sample_entries` in `src/psk31.rs` (spot-checks space, `A`, `a`, NUL and DEL against the ARRL published values); `psk31_encode_bits`.

### `correlate_symbol_iq` [@ANCHOR: correlate_symbol_iq]

Correlates one symbol's worth of samples against a cosine and a sine at the carrier frequency and returns the two sums (I, the
in-phase part, and Q, the quadrature part). Each sample is weighted by the same raised-cosine envelope the modulator applied, which
concentrates the estimate on the strong centre of the symbol. The carrier phase is computed from the absolute sample index of the
stream (`sample_idx_start` plus the position in the chunk), so consecutive calls stay phase-continuous. Both the whole-buffer
demodulator and the streaming decoder call it, so the constellation points shown to the operator are the numbers the bits were
decided from.

| Argument | Type | Meaning |
|---|---|---|
| `chunk` | `&[i16]` | Exactly one symbol of samples; its length is taken as the samples-per-symbol (minimum 1) for the envelope. |
| `carrier_hz` | `f64` | Audio carrier frequency in Hz. |
| `sample_rate` | `u32` | Sample rate in Hz. |
| `sample_idx_start` | `u64` | Absolute index, since the start of the stream, of the first sample in `chunk`. |

**Returns:** `(i_sum, q_sum)`, each an `f64`, with samples normalised by `i16::MAX`. The phase of the symbol is `atan2(q, i)` (done by `correlate_symbol_phase`).

**Errors:** None. An empty chunk returns `(0.0, 0.0)`.

**Special concerns:** Pure and non-blocking, cost proportional to the chunk length (one `cos`/`sin` pair per sample). It reads no state, so threads may call it freely. A caller that passes the wrong `sample_idx_start` loses phase continuity between symbols and the differential bit decision then fails.

**Signature:** `fn correlate_symbol_iq(chunk: &[i16], carrier_hz: f64, sample_rate: u32, sample_idx_start: u64) -> (f64, f64)`

**See also:** `phase_to_bit`, `Psk31Decoder::feed_with_iq`; test `full_modem_round_trips_through_synthesized_audio` (text to audio and back through this correlator).

### `phase_to_bit` [@ANCHOR: phase_to_bit]

Makes the BPSK31 (binary phase-shift keying at 31.25 baud) bit decision for one symbol. The phase change from the previous symbol,
wrapped into [-pi, pi], decides 1 when its magnitude is under pi/2 (steady carrier) and 0 otherwise (phase reversal). With no previous
symbol it returns 1, which is correct by design: every Varicode codeword starts with a 1 and the modulator starts at zero phase, so
the first transmitted symbol is always a steady carrier relative to that reference.

| Argument | Type | Meaning |
|---|---|---|
| `phase` | `f64` | This symbol's carrier phase in radians (from `atan2` of the I/Q sums). |
| `prev_phase` | `Option<f64>` | The previous symbol's phase, or `None` for the first symbol of a stream. |

**Returns:** `true` for bit 1 (steady), `false` for bit 0 (reversal).

**Errors:** None; the difference is wrapped with loops, so any finite input is accepted. A NaN phase compares false and decodes as 0.

**Special concerns:** Pure, no side effects. The decision is differential, so a constant carrier phase offset cancels and the receiver needs no absolute phase lock.

**Signature:** `fn phase_to_bit(phase: f64, prev_phase: Option<f64>) -> bool`

**See also:** `correlate_symbol_iq`; test `full_modem_round_trips_through_synthesized_audio`; the test `every_codeword_starts_with_a_one_bit` that backs the `None` case.

### `VaricodeAccumulator::push_bit` [@ANCHOR: VaricodeAccumulator::push_bit]

The streaming form of the character-boundary logic in `psk31_decode_bits`. A Varicode character ends with two consecutive zero bits
(the inter-character gap), and no codeword contains two zeros in a row. `push_bit` takes one decoded bit; on the second consecutive
zero it drops the first gap zero from the accumulator, looks the codeword up with `code_to_char`, resets, and returns the character if
it matched. Otherwise it returns nothing. A run longer than `MAX_VARICODE_BITS + 1` (11) accumulated bits is treated as noise or signal
loss and resets the accumulator, so a long run of garbage cannot grow without bound.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `VaricodeAccumulator` | Fields: `acc` (`u16` bits so far), `count` (`u8` bits so far), `prev_was_zero` (`bool`). Owned by one `Psk31Decoder`. |
| `bit` | `bool` | The next demodulated bit, `true` for 1. |

**Returns:** `Some(char)` when this bit completed a valid character; `None` otherwise, including when the gap closed on an unknown codeword (the bits are discarded).

**Errors:** None; unknown codewords and overlong runs are silently dropped, because live audio contains noise.

**Special concerns:** Mutates only its own three fields; non-blocking; not thread-safe (one decoder, one caller; `Psk31Decoder::feed` takes `&mut self`). The state persists across `feed()` calls, which is what lets a character straddle two audio chunks.

**Signature:** `fn push_bit(&mut self, bit: bool) -> Option<char>`

**See also:** test `feed_with_iq_decodes_identically_to_feed` in `src/psk31.rs`; `psk31_decode_bits` (the batch version).

## RTTY framing and synthesis internals

Both helpers are private to `src/rtty.rs`. RTTY here is 45.45 baud, 5-bit Baudot, with a 170 Hz shift (`RTTY_BAUD`, `RTTY_DEFAULT_SHIFT_HZ`) and a stop bit of 1.5 bit periods (`RTTY_STOP_BIT_UNITS`). They are pure and have no locks, I/O or hardware side effects; the produced audio is returned to the caller, which decides whether anything is ever transmitted.

### `push_framed_char` [@ANCHOR: push_framed_char]

Appends one character's seven bit periods to a bit vector: a SPACE start bit, the five data bits least significant first (true is MARK),
and a MARK stop bit. The stop bit is one entry here; the audio stage (`framed_bits_to_audio`) stretches it to 1.5 bit periods. It is
called by `text_to_framed_bits` for every character and for every inserted LTRS/FIGS shift code, and directly by tests that build
bit sequences the text path would never produce.

| Argument | Type | Meaning |
|---|---|---|
| `bits` | `&mut Vec<bool>` | The output being built; seven entries are pushed. `true` is MARK, `false` is SPACE. |
| `code` | `u8` | The 5-bit Baudot code; only the low five bits are used. |

**Returns:** Nothing; the vector grows by exactly 7 entries.

**Errors:** None.

**Special concerns:** No I/O, no locking. The "every seventh entry is a stop bit" assumption in `framed_bits_to_audio` relies on this function always pushing exactly 7 bits.

**Signature:** `fn push_framed_char(bits: &mut Vec<bool>, code: u8)`

**See also:** `text_to_framed_bits`, `framed_bits_to_audio`; test `round_trips_a_real_cq_call_through_letters_and_figures_shifts`.

### `framed_bits_to_audio` [@ANCHOR: framed_bits_to_audio]

The FSK (frequency-shift keying) synthesis behind `rtty_modulate`, kept separate so tests can feed it bit sequences that the text path
would never produce (for example a figures run with no unshift code, to exercise unshift-on-space). MARK is `mark_hz` and SPACE is
`mark_hz + 170`. The phase accumulator runs across all bits so the waveform is continuous (no splatter from phase jumps), and each bit
lasts `sample_rate / 45.45` samples, rounded per bit. Every seventh bit (index modulo 7 equal to 6) is taken to be a stop bit and is
lengthened to 1.5 times, so it assumes the input is built from 7-bit frames.

| Argument | Type | Meaning |
|---|---|---|
| `bits` | `&[bool]` | One entry per bit period, `true` MARK, `false` SPACE; length should be a multiple of 7. |
| `mark_hz` | `f64` | MARK tone frequency in Hz (the default in this module is 2125 Hz). |
| `sample_rate` | `u32` | Output sample rate in Hz. |

**Returns:** `Vec<i16>` of mono PCM at 0.9 of full scale. Empty input gives an empty vector.

**Errors:** None; it does not validate the frame structure.

**Special concerns:** Allocates the whole output; cost is linear in the output length; no I/O. This function only makes samples. Whoever sends them to a radio owns the transmit gate and band check; nothing in this crate keys a transmitter.

**Signature:** `fn framed_bits_to_audio(bits: &[bool], mark_hz: f64, sample_rate: u32) -> Vec<i16>`

**See also:** tests `unshift_on_space_recovers_a_letter_after_an_unsent_shift_code` and `implausible_figs_run_pre_emptively_unshifts_without_a_space` in `src/rtty.rs`; `rtty_modulate`.

## RTTY correlators

The decoder decides every bit by correlating a window of audio against the MARK and SPACE tones and comparing the energies. These items are private to `src/rtty.rs`.

### `window_mark_space_energy` [@ANCHOR: window_mark_space_energy]

The slow reference correlator. For one window it computes the quadrature (I and Q) correlation against the MARK and SPACE tones,
recomputing sine and cosine for every sample, and returns whether MARK is stronger together with both energies (I squared plus Q
squared). It is not used on the decode path; the tests compare it with the table-driven version so the two paths cross-check each
other.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]` | The window; phase counts from its first sample. |
| `mark_hz`, `space_hz` | `f64` | The two tone frequencies in Hz. |
| `sample_rate` | `u32` | Sample rate in Hz. |

**Returns:** `(mark_stronger, mark_energy, space_energy)`; energies are in normalised linear units (samples divided by `i16::MAX`).

**Errors:** None; an empty window gives `(false, 0.0, 0.0)`.

**Special concerns:** Pure; cost is one `cos` and `sin` pair per tone per sample, which is why `TrigTables` exists.

**Signature:** `fn window_mark_space_energy(samples: &[i16], mark_hz: f64, space_hz: f64, sample_rate: u32) -> (bool, f64, f64)`

**See also:** `TrigTables::window_evidence`; test `mark_space_ratio_separates_signal_from_noise_across_a_full_amplitude_sweep`.

### `TrigTables::new` [@ANCHOR: TrigTables::new]

Precomputes cosine and sine values for the MARK and SPACE tones at window-relative sample offsets 0 up to `capacity`. The phase
always counts from the window's first sample, so one set of tables serves a window starting anywhere in the stream. Built once per
decoder (or once per whole-buffer call) because recomputing trigonometry per window was the dominant cost.

| Argument | Type | Meaning |
|---|---|---|
| `mark_hz`, `space_hz` | `f64` | Tone frequencies in Hz. |
| `sample_rate` | `u32` | Sample rate in Hz. |
| `capacity` | `usize` | Number of sample offsets to precompute; it is the longest window the tables can serve. |

**Returns:** A `TrigTables` holding four `Vec<f64>` of length `capacity` (`cos_mark`, `sin_mark`, `cos_space`, `sin_space`).

**Errors:** None. Memory is 4 x 8 x `capacity` bytes.

**Special concerns:** Immutable after construction, so it can be shared between threads for reading. No I/O.

**Signature:** `fn new(mark_hz: f64, space_hz: f64, sample_rate: u32, capacity: usize) -> Self`

**See also:** `TrigBank::new_with_offsets`; test `round_trips_a_real_cq_call_through_letters_and_figures_shifts`.

### `TrigTables::window_evidence` [@ANCHOR: TrigTables::window_evidence]

Correlates `samples[start..start+len]` against the tables and returns the signed MARK-to-SPACE ratio in dB (positive means MARK is
stronger; energies are floored at 1e-15 so the logarithm is finite) together with the linear total MARK plus SPACE energy. The ratio
drives every bit decision; the energy is what the frequency search compares across candidates, because a dB value saturates and a
linear energy tracks the correlator's real frequency-mismatch rolloff. A window short by up to 10 percent, and one longer than the
tables, is shortened to what is available; this lets the last character of a whole-buffer decode through.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]` | The audio buffer. |
| `start` | `usize` | First sample of the window. |
| `len` | `usize` | Wanted window length in samples. |

**Returns:** `Some((ratio_db, total_energy))`, or `None` when `len` is zero, `start` is past the end, or more than 10 percent of the window is missing.

**Errors:** None beyond the `None` cases. A heavily shortened window would give an unreliable ratio, which is why it is refused.

**Special concerns:** Pure and non-blocking; read-only access to the tables.

**Signature:** `fn window_evidence(&self, samples: &[i16], start: usize, len: usize) -> Option<(f64, f64)>`

**See also:** `TrigTables::signed_ratio_db`, `frame_score`; test `round_trips_a_real_cq_call_through_letters_and_figures_shifts`.

### `TrigTables::signed_ratio_db` [@ANCHOR: TrigTables::signed_ratio_db]

`window_evidence` with the energy dropped, for the many callers that need only the MARK-to-SPACE ratio: the persistence check, the
data-bit decisions in `attempt_character` and `characterize_rtty_signature`. It has the same window rules, so it returns `None` for
exactly the same inputs.

| Argument | Type | Meaning |
|---|---|---|
| `samples`, `start`, `len` | `&[i16]`, `usize`, `usize` | As for `window_evidence`. |

**Returns:** `Some(ratio_db)` (positive = MARK stronger), or `None` as described there.

**Errors:** None.

**Special concerns:** Pure; no side effects.

**Signature:** `fn signed_ratio_db(&self, samples: &[i16], start: usize, len: usize) -> Option<f64>`

**See also:** `TrigTables::window_evidence`; test `round_trips_a_real_cq_call_through_letters_and_figures_shifts`.

### `TrigBank::new_with_offsets` [@ANCHOR: TrigBank::new_with_offsets]

Builds one `TrigTables` per carrier offset in the given list, each with MARK at the configured frequency plus that offset and SPACE
170 Hz above it. It keeps the offset list so a decoder can report the real offset of the table that won; indexing a one-table bank
into the nine-entry default list would otherwise report the wrong value (-40 Hz instead of 0 Hz). A decoder that scans many channels
passes the single-entry list `FIXED_OFFSET_HZ` (0 Hz) so each decoder does no inner frequency search.

| Argument | Type | Meaning |
|---|---|---|
| `offsets` | `&'static [f64]` | Offsets in Hz relative to `mark_hz`. |
| `mark_hz` | `f64` | Nominal MARK frequency in Hz. |
| `sample_rate` | `u32` | Sample rate in Hz. |
| `capacity` | `usize` | Table length per offset, in samples. |

**Returns:** A `TrigBank` with `tables` (one per offset, same order) and `offsets`.

**Errors:** None. Memory scales with offsets times capacity.

**Special concerns:** Built once and then read-only. No I/O.

**Signature:** `fn new_with_offsets(offsets: &'static [f64], mark_hz: f64, sample_rate: u32, capacity: usize) -> Self`

**See also:** `TrigBank::new`, `RttyDecoder::new_fixed`; test `fixed_decoder_char_yield_vs_mismatch`.

### `TrigBank::new` [@ANCHOR: TrigBank::new]

The bank over the standard nine offsets, -40 to +40 Hz in 10 Hz steps (`AFC_OFFSETS_HZ`). The spacing keeps the worst residual
mistuning after snapping to the nearest candidate at 5 Hz, well inside the roughly 20 Hz the sensitivity harness measured as still
clean, while covering typical manual-tuning error and transmitter drift. Used by `rtty_demodulate` and `RttyDecoder::new`.

| Argument | Type | Meaning |
|---|---|---|
| `mark_hz` | `f64` | Nominal MARK frequency in Hz. |
| `sample_rate` | `u32` | Sample rate in Hz. |
| `capacity` | `usize` | Table length in samples (at least the stop-bit window length). |

**Returns:** A `TrigBank` of nine tables.

**Errors:** None.

**Special concerns:** Nine tables of four vectors each, so memory is 9 x 4 x 8 x `capacity` bytes; build it once, not per window.

**Signature:** `fn new(mark_hz: f64, sample_rate: u32, capacity: usize) -> Self`

**See also:** `TrigBank::new_with_offsets`; test `rtty_decoder_tracks_a_real_off_air_frequency_offset`.

## RTTY edge detection and scan

These private functions in `src/rtty.rs` find character start edges by scoring a whole seven-bit frame, so noise and partial matches are rejected.
All are pure functions of the audio and the tables except `rtty_scan`, which advances a `ScanState`.

### `pre_is_persistently_mark` [@ANCHOR: pre_is_persistently_mark]

Checks that the bit period just before a candidate start edge is steady MARK: it splits that period into `PERSIST_SUBWINDOWS` (2)
sub-windows and requires each to read more than `PERSIST_RATIO_THRESHOLD_DB` (4 dB) toward MARK. This rejects candidates whose
context is not an idle or previous stop bit, because noise rarely sustains MARK across several independent windows. Where less than
one bit of history exists, only the whole sub-windows that fit are checked; an edge within one sub-window of the buffer start passes
without a test.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]` | The buffered audio. |
| `edge_pos` | `usize` | Candidate start-bit edge, in samples. |
| `geo` | `&FrameGeometry` | Bit and window lengths for the sample rate. |
| `trig` | `&TrigTables` | The tables for the frequency hypothesis being tested. |

**Returns:** `true` when every checked sub-window is MARK-ward, `false` otherwise (including when a window cannot be evaluated).

**Errors:** None.

**Special concerns:** Pure. It exists to stop the position refinement sliding a first character whose first data bit equals its start bit late by up to half a bit.

**Signature:** `fn pre_is_persistently_mark(samples: &[i16], edge_pos: usize, geo: &FrameGeometry, trig: &TrigTables) -> bool`

**See also:** `frame_score`; test `round_trips_a_real_cq_call_through_letters_and_figures_shifts`.

### `frame_score` [@ANCHOR: frame_score]

Rates a candidate start edge using the whole 7-bit frame, after the persistence check. It correlates the start-bit window, the five
data-bit windows and the 1.5-bit stop window, each from the table passed in. It returns three numbers in a `FrameEvidence`: an
`accept_score`, the sum of the start (SPACE-ward), stop (MARK-ward) and five data-bit ratios, each clipped to `PER_WINDOW_CLIP_DB`
(18 dB), which decides whether a character is there; a `raw_score`, the unclipped start-plus-stop ratio, used to find the exact edge
position because clipping flattens a strong peak; and `magnitude`, the total linear energy, used to choose between frequency
candidates. The scores are ratios, so the same thresholds work across amplitudes.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]` | The buffered audio. |
| `edge_pos` | `usize` | Candidate start-bit edge, in samples. |
| `geo` | `&FrameGeometry` | Window lengths. |
| `trig` | `&TrigTables` | Tables for one frequency hypothesis. |

**Returns:** `Some(FrameEvidence { accept_score, raw_score, magnitude })`, or `None` if the persistence check fails or any window runs past the buffered audio.

**Errors:** None. A streaming caller must distinguish "not enough audio yet" from "not a character"; `rtty_scan` does this with its lookahead check.

**Special concerns:** Pure, seven window correlations per call; it is the hot path of the decoder.

**Signature:** `fn frame_score(samples: &[i16], edge_pos: usize, geo: &FrameGeometry, trig: &TrigTables) -> Option<FrameEvidence>`

**See also:** `pre_is_persistently_mark`, `best_frame_evidence`, `rtty_scan`; test `round_trips_a_real_cq_call_through_letters_and_figures_shifts`.

### `best_frame_evidence` [@ANCHOR: best_frame_evidence]

Evaluates `frame_score` at one position for every table in the bank and keeps only candidates whose `accept_score` is above
`FRAME_SCORE_THRESHOLD_DB` (80 dB). Of those it returns the one with the greatest linear `magnitude`, with its table index. Energy
chooses the winner because the clipped score saturates and cannot tell neighbouring offsets apart, and a sum of dB ratios was
measured to be the wrong physical quantity; the clipped score only gates noise. This is the automatic frequency control (AFC) search,
run at every scan position so a drifted station is reacquired.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]` | Buffered audio. |
| `edge_pos` | `usize` | Candidate edge position. |
| `geo` | `&FrameGeometry` | Window lengths. |
| `bank` | `&TrigBank` | The frequency hypotheses. |

**Returns:** `Some((table_index, FrameEvidence))` for the winner, or `None` when no table clears the threshold.

**Errors:** None.

**Special concerns:** Pure; cost is the bank size (9 by default) times one `frame_score`.

**Signature:** `fn best_frame_evidence(samples: &[i16], edge_pos: usize, geo: &FrameGeometry, bank: &TrigBank) -> Option<(usize, FrameEvidence)>`

**See also:** `rtty_scan`; test `rtty_decoder_tracks_a_real_off_air_frequency_offset`.

### `ScanState::new` [@ANCHOR: ScanState::new]

Creates the scan state a decoder carries between `feed()` calls: position zero, letters shift, a zero count of consecutive
non-digit figures characters (the evidence counter for the unshift-on-space heuristic), and the given initial locked-offset index.
`ScanState::default()` uses the middle entry of the nine-offset list (0 Hz).

| Argument | Type | Meaning |
|---|---|---|
| `locked_offset_idx` | `usize` | Index into the decoder's offset list; purely informational, it reports which offset last decoded a character and never narrows the search. |

**Returns:** A `ScanState { pos, current_figs, figs_non_digit_run, locked_offset_idx }`.

**Errors:** None. An index outside the bank's offset list would be a caller bug.

**Special concerns:** Owned by one decoder; `rtty_scan` mutates it in place, so it needs `&mut` access and is not shared between threads.

**Signature:** `fn new(locked_offset_idx: usize) -> Self`

**See also:** `rtty_scan`; test `fixed_decoder_char_yield_vs_mismatch` (a one-table bank starts at index 0).

### `rtty_scan` [@ANCHOR: rtty_scan]

The loop shared by the whole-buffer decoder (`rtty_demodulate`) and the streaming decoder (`RttyDecoder::feed`). It steps through the
buffer in steps of one eighth of a bit period (`SCAN_GRID_PER_BIT`). At each position `best_frame_evidence` is tried against the full
offset bank. When it finds a candidate, the table is fixed for that character, and the position is refined over plus or minus half a
bit by taking the highest raw score. `attempt_character` then makes the hard decision, and on success the decoder emits the text,
records the winning offset and jumps past the character. A failed attempt just moves on one step. In streaming mode it stops,
leaving the position unchanged, when the character plus the refinement margin is not fully buffered yet, so the next call retries
the same position and no character is lost across a chunk boundary.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]` | The audio buffer; positions in `state` index into it. |
| `geo` | `&FrameGeometry` | Bit and window lengths for the sample rate. |
| `bank` | `&TrigBank` | Frequency hypotheses. |
| `state` | `&mut ScanState` | Resume position, shift state and locked offset; updated in place. |
| `stop_if_insufficient_lookahead` | `bool` | `false` for whole-buffer decoding, `true` for streaming. |

**Returns:** The newly decoded characters as a `String` (empty when none).

**Errors:** None. A zero-length bit window (a sample rate too low for one sample per bit) returns an empty string.

**Special concerns:** Pure apart from mutating `state`; no I/O or locks. Cost per position is one bank search. It decodes only; it never transmits.

**Signature:** `fn rtty_scan(samples: &[i16], geo: &FrameGeometry, bank: &TrigBank, state: &mut ScanState, stop_if_insufficient_lookahead: bool) -> String`

**See also:** `attempt_character`, `RttyDecoder::feed`; tests `round_trips_a_real_cq_call_through_letters_and_figures_shifts`, `rtty_decoder_tracks_a_real_off_air_frequency_offset`.

## RTTY presence signature

### `characterize_rtty_signature` [@ANCHOR: characterize_rtty_signature]

A cheap, decode-free probe that is not wired into any decoder. It cuts the audio into whole one-bit-period hops and takes the sign of
the MARK/SPACE ratio of each, and reports the fraction of adjacent hops that differ and a histogram of run lengths (runs of 8 hops or
more share the last bucket). Measurement showed that keyed RTTY and noise both flip about half the time, so the one case it separates
cleanly is a steady unkeyed carrier, which never flips. It is kept, with that negative result, so nobody re-derives it.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]` | Audio to examine. |
| `candidate_hz` | `f64` | MARK frequency to test; SPACE is 170 Hz above. |
| `sample_rate` | `u32` | Sample rate in Hz. |

**Returns:** `Some(RttySignature { sign_flip_rate: f64 in [0, 1], run_length_histogram: [usize; 8] })`, or `None` with fewer than two hops or when the sample rate is too low for one sample per bit.

**Errors:** None beyond `None`.

**Special concerns:** Pure; allocates one sign per hop. Not called by `RttyDecoder`, `rtty_scan` or any pipeline, so it affects no decode.

**Signature:** `fn characterize_rtty_signature(samples: &[i16], candidate_hz: f64, sample_rate: u32) -> Option<RttySignature>`

**See also:** test `rtty_signature_reports_flip_rate_run_lengths_and_degenerate_inputs`.

## FT8 waveform internals

### `gfsk_pulse` [@ANCHOR: gfsk_pulse]

The Gaussian-smoothed frequency pulse for one FT8 symbol, with a bandwidth-time product of 2.0 (`FT8_BT`): half the difference of two
error functions, `(erf(k*BT*(t+0.5)) - erf(k*BT*(t-0.5))) / (2T)`, where t is in symbol periods, T is the symbol period in seconds and
k is pi times the square root of 2 over ln 2, computed directly rather than as a literal. It integrates to 1 over time, so a symbol's
frequency offset adds up to the tone number. `synthesize_waveform` passes it to `synthesize_waveform_with_pulse`, which integrates the
deviation into phase.

| Argument | Type | Meaning |
|---|---|---|
| `t_over_symbol_period` | `f64` | Time relative to the symbol centre, in symbol periods. |
| `symbol_period_s` | `f64` | Symbol period in seconds (0.16 s for FT8). |

**Returns:** The pulse value in 1/seconds (the area-normalised deviation).

**Errors:** None; uses `libm::erf`, so it works in the same builds as the rest of the module.

**Special concerns:** Pure and cheap. Smoothing the tone steps confines the spectrum; the waveform it shapes is what goes to the radio, and the caller of `synthesize_waveform` owns the transmit gate.

**Signature:** `fn gfsk_pulse(t_over_symbol_period: f64, symbol_period_s: f64) -> f64`

**See also:** test `gfsk_pulse_shaping_suppresses_out_of_band_energy_more_than_an_unfiltered_pulse` in `src/ft8.rs`.

## WSPR encode internals (`wspr.rs`)

The two character tables behind the packers use different digit and letter codes. [@ANCHOR: callsign_char_code] maps
a digit to 0-9, a space to 36 and a capital letter to 10-35, and anything else (including lower case) to nothing;
callers upper-case first. [@ANCHOR: grid_char_code] maps a digit to 0-9, a space to 36 and a letter to 0-25 (so `A`
is 0, not 10), which is what the Maidenhead arithmetic in the grid packer needs.

[@ANCHOR: interleave_permutation] produces the interleaver's order: it runs the integers 0..255 through an 8-bit bit
reversal and keeps, in order, the results below 162. Coded bit `p` is sent in symbol position `perm[p]`, so the
result is a bijection on 0..161 (tested), and the decoder undoes it by reading position `perm[p]` for bit `p`.

[@ANCHOR: convolutional_encode] is the rate-1/2, constraint-length-32 encoder. For each of the 88 input bits, taken
most significant bit first from the 11 bytes, it shifts the bit into a 32-bit register and emits the parity of the
register ANDed with `POLY1` (`0xF2D05351`) and then with `POLY2` (`0xE4613C47`). The input holds 50 payload bits and
38 zero bits (6 to the byte boundary and 32 to flush the register), giving 176 coded bits of which only the first 162
are transmitted.

[@ANCHOR: wrap_mono_i16_as_wav] writes the WAV (a common audio file format) container used by the test fixtures: `RIFF` with length
`36 + data`, `WAVE`, a 16-byte PCM `fmt ` chunk (mono, the given sample rate, byte rate twice the sample rate, block
align 2, 16 bits) and a `data` chunk with the samples in little-endian order, so `data` always starts at byte 36 and
the samples at byte 44.

The noise generator for the fixtures is a SplitMix64 pseudo-random generator, deterministic for a given seed and not
for cryptographic use. [@ANCHOR: SplitMix64::next_u64] adds the golden-ratio constant `0x9E3779B97F4A7C15` to the
state and scrambles it with two xor-shift-multiply rounds, returning 64 bits. The Gaussian samples come from
[@ANCHOR: SplitMix64::next_gaussian], which draws two uniform numbers in (0, 1] (never 0, so the logarithm is
finite) and applies the Box-Muller transform, using only the cosine branch: `sqrt(-2 ln u1) * cos(2 pi u2)`.

## WSPR sequential decode internals (`wspr_decode.rs`)

[@ANCHOR: fano_bit_metric] is the branch metric of sequential decoding for one coded bit: `log2(P(r|hyp) / P(r)) - R`
with `R = 0.5` bits per coded bit for a rate-1/2 code, where the received value `r` is modelled as the bipolar
amplitude (+ for 1, - for 0) plus Gaussian noise of the given standard deviation, and `P(r)` averages the two
hypotheses. It subtracts the larger of the two Gaussian exponents before exponentiating so that a received value far
from both hypotheses gives a finite metric instead of `0/0`; the normalising constant cancels and is left out. A
correct path therefore gains metric and a wrong one loses it.

[@ANCHOR: deinterleave_symbol_values] converts values indexed by transmitted symbol position into values indexed by
coded-bit position, `out[p] = in[perm[p]]`, using the same permutation function as the encoder so it is the exact
inverse. The decoder is given the data-bit soft value of each symbol, never the sync bit.

## WSPR sync search internals (`wspr_sync.rs`)

[@ANCHOR: samples_per_symbol] is the rounded `sample_rate / (12000/8192)`, 8192 at 12 kHz. It is also the FFT length,
because WSPR's tone spacing equals its symbol rate, so every FFT bin is exactly one tone wide.

[@ANCHOR: spectrum_matrix] builds the 162-row table of magnitudes for a start position: row `s` is the spectrum of
the block starting at `start + s * samples_per_symbol`, restricted to bins `bin_lo..=bin_hi_inclusive` and
shifted down by a sub-bin frequency offset first. It returns nothing, rather than panicking, when the window runs
past the end of the audio or the bin range is empty or beyond the FFT length. The per-row work is
[@ANCHOR: block_spectrum]: multiply the block by `exp(-j 2 pi offset n / sample_rate)` (phase restarting at zero
for each block, so the result depends only on the block position and offset, which lets `find_sync` cache blocks),
take the FFT with a plan supplied by the caller, and return the magnitudes of the requested bins.

[@ANCHOR: tone_magnitudes_from_matrix] cuts four consecutive columns starting at `base_bin` out of each row of that
table, giving the magnitudes of tones 0-3 for every symbol; `base_bin` must not be below `bin_lo`.

The score for a candidate is built from three pieces. [@ANCHOR: sync_pattern] is the sync vector as +1/-1.
[@ANCHOR: sync_statistic] gives per symbol `max(tone 1, tone 3) - max(tone 0, tone 2)`, which is positive when the
sync bit is 1 and negative when it is 0 if the window is aligned, whatever the data bit. [@ANCHOR: cosine_score]
compares the two as `sum(tt * pattern) / (|tt| * |pattern|)`, in -1..1, and gives 0 for an all-zero statistic. It
replaced a mean-absolute-value normalisation, which scored the maximum whenever all signs merely agreed and so
accepted noise.

[@ANCHOR: extract_all_four_tone_magnitudes] is the shared extraction step: after rejecting a zero sample rate, a
non-finite or negative `base_hz`, or one above half the sample rate, it takes the nearest bin to `base_hz`, applies
the remaining fraction as the sub-bin offset, plans an FFT and returns all four tone magnitudes of the 162 symbols
(or nothing for a short buffer). [@ANCHOR: extract_impossible_tone_evidence] returns the two tones that cannot be
present given the known sync bit (indices `1 - sync` and `3 - sync`), a noise-only reference per symbol.

Two ways to turn evidence into decoder inputs exist. [@ANCHOR: evidence_to_symbol_values_windowed] is the production
one, with the window half-width as a parameter: signed value `v1 - v0`; amplitude the window mean of the larger tone
minus the window mean of the smaller; noise the square root of the summed variances of the two; amplitude and noise
floored at 1e-9. The window is the symbol plus `half_width` neighbours each side, clamped at both ends.
[@ANCHOR: evidence_to_symbol_values_windowed_clean_reference] is the alternative that takes the noise from the
impossible-tone reference instead (population standard deviation of the 2 x window values). It measured worse at low
SNR and is kept only for the diagnostic tests that record that result.

## AMBE (TIA-102.BABA) float codec internals: bit handling, pitch, voicing, quantization and synthesis

These are the internal steps of the float P25 full-rate codec in `src/ambe/float/tia_102_baba/`. The user-visible path through
them (receive, decode, transmit) is told in `docs/stories/ambe_p25_voice_frame_wire_path.md`; this section covers the helpers
and the equations behind each stage, in the order the encoder meets them. Equation numbers refer to TIA-102.BABA_2003.

### Bit prioritization helpers

`raster_unscan_bits` is the inverse of the raster scan: given the flat scanned bits and the column widths, it walks the same
level-by-level, column-by-column order and rebuilds each column's value. It returns `None` if the bit count does not match the
widths exactly (too few bits, or bits left over).
*(Reference: `src/ambe/float/tia_102_baba/bit_prioritization.rs` -> `[@ANCHOR: raster_unscan_bits]`)*

`read_bits` reads the next `n` bits of a bit slice, most significant first, as an integer and moves the cursor; it returns `None`
if fewer than `n` bits remain. `deprioritize_bits` uses it for each fixed-width field.
*(Reference: `src/ambe/float/tia_102_baba/bit_prioritization.rs` -> `[@ANCHOR: read_bits]`)*

`set_bit` writes one bit of the de-interleaved code vectors: it sets bit `source.1` (0 = least significant) of vector
`source.0` when the received symbol bit is 1, and does nothing for a 0 (the vectors start as zero).
*(Reference: `src/ambe/float/tia_102_baba/interleave.rs` -> `[@ANCHOR: set_bit]`)*

### Pitch analysis (section 5.1)

The analysis works on a lowpass-filtered copy of the speech. `lowpass_filtered_sample` computes one sample of it (Eq. 9): the sum
of 21 input samples around `center + n`, weighted by the symmetric Annex D filter taps. It indexes the raw buffer directly, so the
buffer must extend at least 10 samples past both ends of the range being asked for, or it panics.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: lowpass_filtered_sample]`)*

`PitchAnalysisFrame::new` prepares everything the pitch error function needs once per frame: the 301 lowpassed samples around
`center`, their windowed energy with the Annex B window, the sum of the window's fourth powers, and the autocorrelation table
`r(t)` of the doubly-windowed signal for integer lags 0 to 151 (Eq. 7; terms that fall outside the window vanish). The raw buffer
needs 160 samples of margin on each side of `center` (150 for the window plus 10 for the filter).
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: PitchAnalysisFrame::new]`)*

`PitchAnalysisFrame::r` returns `r(t)` for a fractional lag by linear interpolation between the two neighbouring table entries
(Eq. 8); the table only holds non-negative lags because `r` is even.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: PitchAnalysisFrame::r]`)*

`PitchAnalysisFrame::error_function` is `E(P)` of Eq. 5 for a candidate period `P` in samples: the energy minus `P` times the sum
of `r(nP)` over the multiples of `P` that fit in the window, divided by the energy times `1 - P * sum(w^4)`. Small is good. Two
deliberate departures are in the code and the comment above it: a value that comes out slightly negative is raised to 0 (the look-ahead
ratio tests would otherwise be ill-defined), and a silent frame (denominator near zero) returns 1.0, the worst error, instead of
`0/0`.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: PitchAnalysisFrame::error_function]`)*

`look_back_pitch_tracking` (Eq. 10-12) looks for continuity with the past. Among the candidate periods 21, 21.5, ..., 122 that lie
within 0.8 to 1.2 times the previous frame's pitch, it takes the one with the smallest current error `E(P)` and returns it with the
cumulative error `CE_B`, which is that error plus the previous two frames' errors. A stream's first frames pass `(100.0, 0.0)` for
both previous entries, the standard's initial values. It panics if no candidate lies within 0.8 to 1.2 times the previous pitch, which cannot happen when that pitch is itself a candidate.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: look_back_pitch_tracking]`)*

`look_ahead_pitch_tracking` (Eq. 13-20) looks at the two following frames, whose error functions can already be computed. For each
candidate `P0` it finds the cheapest chain of pitches for the next two frames, each within 0.8 to 1.2 times the one before, and the
cumulative error `CE_F` is the sum of the three errors. It picks the `P0` with the smallest `CE_F`, then tests its sub-multiples
`P0/2, P0/3, ...` (snapped to the candidate grid, smallest first) against the three accept rules of Eq. 18 to 20 so that a pitch
doubled by mistake is corrected. It returns the chosen period and its `CE_F`.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: look_ahead_pitch_tracking]`)*

`choose_initial_pitch_estimate` (Eq. 21-23) makes the final pick between the two trackers: the look-back pitch if its cumulative
error is at most 0.48 or no larger than the look-ahead error, otherwise the look-ahead pitch. The first two conditions are kept as
the standard's two separate rules even though they choose the same thing.
*(Reference: `src/ambe/float/tia_102_baba/pitch.rs` -> `[@ANCHOR: choose_initial_pitch_estimate]`)*

### Pitch refinement (section 5.1.5)

`window_dft_16384` is `W_R(m)` (Eq. 30), the spectrum of the refinement window at a 16384-point resolution. It sums the cosine
terms of the window's 221 non-zero samples directly (the sine terms cancel by symmetry) instead of running a transform; because
it is even in `m` and queried thousands of times per frame, the half-line `m = 0..=8192` is tabulated once per process and
anything beyond is computed on demand.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: window_dft_16384]`)*

`RefinementFrame::new` computes the frame's spectrum `S_w(m)` (Eq. 29): the 256-point DFT of the 221 speech samples around
`center` multiplied by the refinement window, for bins `m = -127..=128`, once per frame. It needs 110 samples of margin on both
sides of `center` and panics otherwise.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: RefinementFrame::new]`)*

`RefinementFrame::sw_at` returns one bin of that spectrum and gives zero outside `-127..=128`, so loops near the edge of the pitch
range read "no energy there" instead of panicking.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: RefinementFrame::sw_at]`)*

`harmonic_amplitude` estimates the complex amplitude `A_l(omega0)` of harmonic `l` (Eq. 26-28) as a least-squares fit: the sum
over the bins of the harmonic's band of `S_w(m)` times the window spectrum `W_R` at the matching offset, divided by the sum of
`W_R` squared. An empty or vanishingly narrow band returns zero.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: harmonic_amplitude]`)*

`refinement_error` is `E_R(omega0)` (Eq. 24): the squared distance between the real spectrum and the synthetic harmonic spectrum
for a candidate pitch, summed from bin 50 up to the last bin covered by the harmonics it implies. The encoder tries candidates
around the half-sample estimate and keeps the one with the smallest error.
*(Reference: `src/ambe/float/tia_102_baba/pitch_refinement.rs` -> `[@ANCHOR: refinement_error]`)*

### Voiced/unvoiced decision (section 5.2)

`frequency_bands_count` is `K_hat` (Eq. 34): the number of V/UV bands, `ceil(L/3)` for up to 36 harmonics and 12 above that.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: frequency_bands_count]`)*

`voicing_measure` is `D_k` (Eq. 35, and Eq. 36 for the top band): the share of a band's spectral energy that the harmonic model
fails to explain, `sum |S_w - synthetic|^2 / sum |S_w|^2` over the band's bins. Near 0 means voiced, near 1 unvoiced. A band with no
energy at all counts as 1 (unvoiced). The top band extends to the last harmonic.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: voicing_measure]`)*

`update_xi_max` tracks the loudness reference `xi_max` (Eq. 41): when the current frame energy `xi_0` exceeds it, the new value is
the average of the two; otherwise it decays as `0.99 * old + 0.01 * xi_0` and is never allowed below 20000.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: update_xi_max]`)*

`energy_dependent_function` is `M(xi)` (Eq. 42): `(0.0025 xi_max + xi_0) / (0.01 xi_max + xi_0)`, scaled down by
`sqrt(xi_LF / (5 xi_HF))` when the low-frequency energy is under five times the high-frequency energy.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: energy_dependent_function]`)*

`voicing_threshold` is `Theta(k, omega0)` (Eq. 37): band `k` is voiced when its measure is below it. It is zero for every band
after the first if the initial pitch error exceeds 0.5; otherwise `0.5625` (band was voiced in the previous frame) or `0.45` times
`1 - 0.3096 (k - 1) omega0` times `M(xi)`.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: voicing_threshold]`)*

`determine_voicing` runs the whole decision for a frame (Fig. 11): it computes `L` and `K`, the energy terms and `M(xi)`, and then
one voiced/unvoiced flag per band. It returns the flags and the updated `xi_max`. A previous frame with fewer bands (or none) reads
as unvoiced for the missing bands, which is this implementation's choice since the standard is silent on it.
*(Reference: `src/ambe/float/tia_102_baba/vuv.rs` -> `[@ANCHOR: determine_voicing]`)*

`encode_voicing_decisions` packs the per-band flags into `b_hat_1` (Eq. 49): band 1 is the most significant of the `K` bits.
*(Reference: `src/ambe/float/tia_102_baba/parameter_encoding.rs` -> `[@ANCHOR: encode_voicing_decisions]`)*

### Spectral amplitude estimation (section 5.3)

`voiced_amplitude` (Eq. 43) is the square root of the ratio of the signal's energy in the harmonic's band to the window spectrum's
energy over the same bins, so a pure sinusoid yields its amplitude. A band without window energy gives zero.
*(Reference: `src/ambe/float/tia_102_baba/spectral_amplitude.rs` -> `[@ANCHOR: voiced_amplitude]`)*

`unvoiced_amplitude` (Eq. 44) is the root of the band's mean energy per bin divided by the window's tap sum, a flat noise-like
level; an empty band gives zero.
*(Reference: `src/ambe/float/tia_102_baba/spectral_amplitude.rs` -> `[@ANCHOR: unvoiced_amplitude]`)*

`estimate_spectral_amplitudes` estimates `M_hat_l` for every harmonic `1..=L`, picking the voiced or unvoiced estimator by the
harmonic's band, `min(ceil(l/3), K)`.
*(Reference: `src/ambe/float/tia_102_baba/spectral_amplitude.rs` -> `[@ANCHOR: estimate_spectral_amplitudes]`)*

### Prediction, gain/block transforms and quantization (section 6)

The encoder does not send amplitudes directly; it sends the change from a prediction made out of the previous frame.
`prediction_coefficient` is `rho` (Eq. 55): 0.4 for up to 15 harmonics, `0.03 L - 0.05` for 16 to 24, and 0.7 above.
*(Reference: `src/ambe/float/tia_102_baba/prediction.rs` -> `[@ANCHOR: prediction_coefficient]`)*

`previous_log2_amplitude` returns `log2` of the previous frame's amplitude at index `j`, applying the standard's boundary rules:
index 0 gives exactly 0 (Eq. 56) and indexes past the previous frame's last harmonic repeat that last value (Eq. 57).
*(Reference: `src/ambe/float/tia_102_baba/prediction.rs` -> `[@ANCHOR: previous_log2_amplitude]`)*

`predicted_and_bias_correction` returns the two terms that the encoder (Eq. 54) and decoder (Eq. 77) both need, once: the previous
log amplitude interpolated at harmonic `l`'s position in the previous frame's harmonic scale (Eq. 52-53) times `rho`, and the
frame-wide bias, `rho / L` times the same interpolation summed over all harmonics. The two directions use them with opposite signs.
*(Reference: `src/ambe/float/tia_102_baba/prediction.rs` -> `[@ANCHOR: predicted_and_bias_correction]`)*

`gain_vector_dct` is the second-stage 6-point transform of Eq. 61: `G_m = (1/6) sum R_i cos(pi (m-1)(i-0.5) / 6)` over the six
per-block DC terms.
*(Reference: `src/ambe/float/tia_102_baba/mod.rs` -> `[@ANCHOR: gain_vector_dct]`)*

`partition_into_blocks` splits the `L` prediction residuals into six consecutive blocks whose lengths come from Annex J
(Eq. 58-59). It returns `None` for `L` outside 9 to 56 or a residual slice of the wrong length.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: partition_into_blocks]`)*

`block_dct` is the per-block DCT of Eq. 60: `C_k = (1/J) sum c_j cos(pi (k-1)(j-0.5) / J)` for a block of any length `J`; an empty
block gives an empty result.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: block_dct]`)*

`saturating_uniform_quantize` is the quantizer shared by Eq. 62 and 63: it divides by the step size, floors, and offsets by
`2^(bits-1)`, with values below the range going to 0 and values above to `2^bits - 1`. It needs at least 1 bit: Annex F never allocates 0 bits to a gain element, and the higher-order caller skips 0-bit entries before calling it.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: saturating_uniform_quantize]`)*

`quantize_gain_vector` quantizes the five non-DC gain elements `G_2..G_6` with the Annex F bit count and step size for the frame's
`L`, returning each value together with its width so the bit-prioritization step cannot be given mismatched widths. `None` for
`L` outside 9 to 56.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: quantize_gain_vector]`)*

`higher_order_coefficient_positions` lists the `(block, position)` of every higher-order DCT coefficient `C_{i,k}` for
`k >= 2`, in the order Annex G allocates bits: all of block 1 first, then block 2, and so on.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: higher_order_coefficient_positions]`)*

`quantize_higher_order_coefficients` quantizes those coefficients (Eq. 63). The step size is the Table 3 multiplier for the
coefficient's bit count times the Table 4 standard deviation for its position. Coefficients allocated 0 bits are not transmitted
and are left out of the result. It returns `None` for an out-of-range `L` or inconsistent tables.
*(Reference: `src/ambe/float/tia_102_baba/quantize.rs` -> `[@ANCHOR: quantize_higher_order_coefficients]`)*

### Reconstruction (decoder, and the encoder's local decoder)

`dequantize_uniform` is the reverse of the quantizer, for Eq. 68 and 71: the centre of the quantizer bin, `step * (q - 2^(bits-1) +
0.5)`, and 0 when no bits were sent.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: dequantize_uniform]`)*

`reconstruct_gain_vector` rebuilds `G_1..G_6` (Eq. 68): `G_1` is the Annex E level at index `b_hat_2`, and `G_2..G_6` are
dequantized with the same widths and steps the quantizer used. `None` for an invalid `b2` or `L`.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: reconstruct_gain_vector]`)*

`inverse_gain_vector_dct` recovers the six block DC terms `R_i` from `G` (Eq. 69-70), the inverse of `gain_vector_dct`.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: inverse_gain_vector_dct]`)*

`inverse_block_dct` is the inverse of `block_dct` for one block of any length (Eq. 73-74).
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: inverse_block_dct]`)*

`reconstruct_higher_order_coefficients` dequantizes all higher-order coefficients (Eq. 71-72) into six blocks, taking one value per
position that was allocated bits and leaving zero-bit positions at 0. The first slot of each block, the DC term, stays 0 here and is
filled in by the caller from `R_i`. It returns `None` if the number of values does not match the positions that carry bits.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: reconstruct_higher_order_coefficients]`)*

`reconstruct_spectral_amplitudes` is the full chain (Eq. 67-79): gain vector, inverse gain DCT, higher-order coefficients with
`R_i` inserted as each block's DC term, inverse block DCTs to the residuals `T_l`, then adding the prediction from the previous
frame in the log2 domain and exponentiating to get `M_l` for `l = 1..=L`. The encoder runs it too so its history matches the
decoder's. `None` for invalid inputs.
*(Reference: `src/ambe/float/tia_102_baba/reconstruct.rs` -> `[@ANCHOR: reconstruct_spectral_amplitudes]`)*

### Annex tables and lookups

`quantize_gain_index` returns `b_hat_2`: the index of the Annex E level (64 of them) closest to `G_1`. On an exact tie the lower
index wins.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: quantize_gain_index]`)*

`block_lengths_for_l` returns the six block lengths of Annex J for `L` in 9 to 56 (they sum to `L` and never decrease with
frequency), and `None` outside that range.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: block_lengths_for_l]`)*

`gain_bit_allocation` returns Annex F's bit count and step size for gain element `m` (2 to 6) at harmonic count `L`; `None`
outside those ranges.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: gain_bit_allocation]`)*

`higher_order_bit_allocation` returns Annex G's list of `L - 6` bit counts, one per higher-order coefficient in the order of
`higher_order_coefficient_positions`. Some entries are 0 (not transmitted). `None` outside 9 to 56.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: higher_order_bit_allocation]`)*

`higher_order_step_multiplier` is Table 3: the step size, as a multiple of the coefficient's standard deviation, for 1 to 10 bits
(for example 0.40 at 4 bits). `None` for 0 bits or more than 10.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: higher_order_step_multiplier]`)*

`higher_order_coefficient_sigma` is Table 4: the standard deviation of the `k`th coefficient of a block, for `k` 2 to 10, the same
for every block. `None` otherwise.
*(Reference: `src/ambe/float/tia_102_baba/tables.rs` -> `[@ANCHOR: higher_order_coefficient_sigma]`)*

### Spectral amplitude enhancement and smoothing (sections 8 and 9)

The decoder improves the reconstructed amplitudes before synthesis, without feeding the result back into its prediction history.
`weight` is `W_l` of Eq. 107, the raw enhancement factor for a harmonic, computed from its amplitude and the two frame energies
`R_M0` and `R_M1`. It must not be called with `R_M0 = 0`.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: weight]`)*

`enhance_spectral_amplitudes` (Eq. 105-110) leaves harmonics up to `L/8` alone, multiplies the others by `W_l` limited to 0.5 to
1.2, and then rescales everything so the total energy equals the input's. A silent frame is returned unchanged, since the formulas
would otherwise divide zero by zero.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: enhance_spectral_amplitudes]`)*

`adaptive_voicing_threshold` is `V_M` (Eq. 112): infinite (never force a decision) for a clean frame (rate at most 0.005 and at most
4 errors), `45.255 S_E^0.375 / exp(277.26 rate)` for a mildly damaged one (rate at most 0.0125 and no errors corrected in `u_hat_4`), and `1.414 S_E^0.375` otherwise.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: adaptive_voicing_threshold]`)*

`update_amplitude_threshold` is `tau_M` (Eq. 115): 20480 for a clean frame (rate at most 0.005 and at most 6 errors), otherwise
`6000 - 300 * errors + previous tau_M`.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: update_amplitude_threshold]`)*

`amplitude_smoothing_scale` is `gamma_M` (Eq. 116): 1 when `tau_M` exceeds the amplitude sum, otherwise `tau_M / A_M`.
*(Reference: `src/ambe/float/tia_102_baba/enhancement.rs` -> `[@ANCHOR: amplitude_smoothing_scale]`)*

`SynthesisState::finalize_parameters` chains those steps into the final synthesis inputs: enhancement, then the `S_E` update and
the voicing smoothing with `V_M`, then the amplitude sum, `tau_M` and `gamma_M` scaling. It stores the result for a later frame
repeat and returns the smoothed voicing and final amplitudes, or `None` if the two input lengths differ.
*(Reference: `src/ambe/float/tia_102_baba/synthesis.rs` -> `[@ANCHOR: SynthesisState::finalize_parameters]`)*

### Speech synthesis (section 11)

`SynthesisState::synthesize_core` produces 160 samples from final parameters: it advances the shared noise generator by one frame
(except before the first), runs unvoiced and voiced synthesis and adds them as Eq. 142, the unvoiced part times a per-mode gain
that is 1 for the standard. Normal frames and repeated frames both end here.
*(Reference: `src/ambe/float/tia_102_baba/synthesis.rs` -> `[@ANCHOR: SynthesisState::synthesize_core]`)*

`synthesis_window` is the Annex I window `w_S(n)`: a trapezoid equal to 1 for `|n| <= 55`, falling linearly to 0 at `|n| = 105`,
and 0 outside. It is computed by formula instead of table.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: synthesis_window]`)*

The unvoiced half uses a deterministic noise sequence (Eq. 117, `u(n+1) = (171 u(n) + 11213) mod 53125`, starting at 3147).
`NoiseState::new` builds the generator's window of 209 values, `u(-104)..u(104)`, by stepping the recurrence 209 times.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: NoiseState::new]`)*

`NoiseState::step` advances the recurrence one value and slides the window by one.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: NoiseState::step]`)*

`NoiseState::advance_frame` moves the window on by one frame, 160 steps.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: NoiseState::advance_frame]`)*

`NoiseState::at` reads `u(relative)` for `relative` in `-104..=104` and returns `None` outside it. Both the unvoiced transform and the
voiced phase randomisation read the same window.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: NoiseState::at]`)*

`unvoiced_dft` is `U_w(m)` (Eq. 118): the 256-point DFT of the windowed noise for `m = -128..=127` (note the one-bin offset from the
analysis spectrum's range). It uses a shared table of the 256 complex exponentials.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: unvoiced_dft]`)*

`unvoiced_scaling_coefficient` is the constant `gamma_w` (Eq. 121) relating the analysis and synthesis windows, about 146.64,
computed once from the two windows.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: unvoiced_scaling_coefficient]`)*

`unvoiced_spectrum` builds the shaped noise spectrum (Eq. 119-120, 124): all bins start at zero; for each *unvoiced* harmonic, the
noise spectrum inside its band is rescaled so its mean power matches that harmonic's amplitude, and the matching negative-frequency
bins get the same scaling. Bands that reach past the Nyquist bin are clipped. It returns `None` if the two input slices differ in
length.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: unvoiced_spectrum]`)*

`unvoiced_time_domain` inverts that spectrum with a 256-point inverse DFT (Eq. 125) and keeps the real part, which is the whole
result because the spectrum is conjugate-symmetric.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: unvoiced_time_domain]`)*

`time_domain_at` reads one sample of that 256-sample result for `n` in `-128..=127`, and returns 0 outside.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: time_domain_at]`)*

`UnvoicedState::synthesize` produces the 160 unvoiced samples (Eq. 117-126): it builds this frame's time-domain noise and
overlap-adds it with the previous frame's, each weighted by the synthesis window and normalised by the sum of the squared windows.
The current result becomes the next call's history. `None` if the voicing and amplitude slices differ in length.
*(Reference: `src/ambe/float/tia_102_baba/unvoiced_synthesis.rs` -> `[@ANCHOR: UnvoicedState::synthesize]`)*

`delta_omega` is `Delta omega_l` of Eq. 137-138, the small frequency correction that makes a harmonic's phase ramp across the frame
land exactly on the target phase: the phase error, after removing the nominal advance and wrapping into `[-pi, pi)`, divided by 160.
It is a per-harmonic constant, so it is computed once per harmonic.
*(Reference: `src/ambe/float/tia_102_baba/voiced_synthesis.rs` -> `[@ANCHOR: delta_omega]`)*

`VoicedState::synthesize` produces the 160 voiced samples (Eq. 127-141) as a sum of sinusoids, one per harmonic. First every
harmonic's phase is advanced (the phase accumulator `psi`, plus a random offset from the shared noise for harmonics above `L/4`
that are in use). Then each harmonic is rendered according to whether it was voiced in the previous frame and is voiced now: not at
all, faded out under the window, faded in under the window, or, if voiced in both, either the two halves cross-faded (for harmonics
8 and up, or a pitch change of 10% or more) or one continuous sinusoid with interpolated amplitude and frequency. Everything is
multiplied by 2. It then stores the frame's parameters as the history. It returns `None` for mismatched slices or more than 56
harmonics.
*(Reference: `src/ambe/float/tia_102_baba/voiced_synthesis.rs` -> `[@ANCHOR: VoicedState::synthesize]`)*

## Codec2 1600 decoder LSP (line spectral pair, a way of representing the speech filter) post-processing

The 1600 bps mode transmits one set of line spectral pairs (LSPs) per 40 ms, so after the scalar indexes are
dequantised the decoder repairs and spreads them, then interpolates them across the four 10 ms sub-frames.
All of these live in `src/codec2_1600/lsp_post.rs`; each has a float form and a Q23 integer twin that is
checked against it. LSP vectors have `LPC_ORD` = 10 entries, in radians (float) or Q23 radians (`i64`, 23
fractional bits). None of these functions does I/O, takes a lock, spawns a task or touches a transmitter; they
are pure transforms on a caller-owned array, called from the 1600 decoder (`Decoder::decode`,
`DecoderFixed::decode`, `DecoderFixed::decode_16k_fixed`).

### `check_lsp_order` [@ANCHOR: check_lsp_order]

Forces ascending order after dequantisation: whenever an adjacent pair is out of order it swaps them and pushes the pair
apart by 0.1 rad (the lower slot becomes `old_upper - 0.1`, the upper slot `old_lower + 0.1`). The scan restarts after a
swap in the same quirky way the reference does (the restart lands on index 2, not 1), reproduced on purpose rather than
"fixed", so the output matches the reference decoder bit for bit. Called by the 1600 float decoder on freshly dequantised LSPs.
Present only with the `std` feature.

```rust
pub fn check_lsp_order(lsp: &mut [f32; LPC_ORD]) -> usize
```

| Argument | Type | Meaning |
|---|---|---|
| `lsp` | `&mut [f32; 10]` | LSPs in radians, edited in place. Any values, including NaN, are accepted. |

**Returns:** the number of swaps made (0 means the vector was already ascending and is untouched).

**Errors:** none; it never panics.

**Special concerns:** pure and synchronous, no locks, no I/O, nothing keys a transmitter. The restart loop has no explicit
iteration cap; an adversarial probe (200,000 random vectors plus reversed, sawtooth and all-equal patterns) found at most
129 iterations and 37 swaps for 10 entries, and a test guards against a regression into an infinite loop with a 2 second
wall-clock budget.

**Signature:** see the block above.

**See also:** `check_lsp_order_fixed`; tests `check_lsp_order_fixes_a_single_out_of_order_pair`,
`check_lsp_order_leaves_an_already_ordered_vector_unchanged`,
`check_lsp_order_terminates_promptly_on_the_worst_case_fully_reversed_input`.

### `check_lsp_order_fixed` [@ANCHOR: check_lsp_order_fixed]

The same logic as `check_lsp_order` on `i64` Q23 angles, including the same restart-at-index-2 behaviour, with the 0.1 rad
nudge as the constant 838861 (`SWAP_NUDGE_Q23`, the value `f32_to_q_exact_round` gives for 0.1; a test checks it). Always
compiled (no `std` needed), so it serves the no-FPU decoder.

```rust
pub fn check_lsp_order_fixed(lsp: &mut [i64; LPC_ORD]) -> usize
```

| Argument | Type | Meaning |
|---|---|---|
| `lsp` | `&mut [i64; 10]` | LSPs in Q23 radians, edited in place. |

**Returns:** the number of swaps.

**Errors:** none in release builds. The nudge adds or subtracts from `i64` values; inputs near the `i64` limits are
unrealistic (LSPs are below pi, about 2.6e7 in Q23) and are not checked.

**Special concerns:** pure, no allocation, no I/O, no transmitter involvement.

**Signature:** see the block above.

**See also:** `check_lsp_order`; test `check_lsp_order_fixed_matches_the_float_version_on_an_out_of_order_pair`.

### `bw_expand_lsps` [@ANCHOR: bw_expand_lsps]

Enforces a minimum gap between neighbouring LSPs in one upward pass: gaps at indexes 1 to 3 are widened to `min_sep_low`
Hz and gaps at indexes 4 to 9 to `min_sep_high` Hz (the decoder passes 50 and 100, the reference decoder's values),
converting Hz to radians with `pi/4000`. Widening is done by raising `lsp[i]` to `lsp[i-1] + gap`; because the pass walks
upward, a raised entry that crowds its successor is repaired in the same pass. Without it, two LSPs that collapse together
make the synthesis filter nearly unstable. Present only with the `std` feature.

```rust
pub fn bw_expand_lsps(lsp: &mut [f32; LPC_ORD], min_sep_low: f32, min_sep_high: f32)
```

| Argument | Type | Meaning |
|---|---|---|
| `lsp` | `&mut [f32; 10]` | Ascending LSPs in radians, edited in place; index 0 is never changed. |
| `min_sep_low` | `f32` | Minimum gap in Hz for indexes 1 to 3 (decoder: 50). |
| `min_sep_high` | `f32` | Minimum gap in Hz for indexes 4 to 9 (decoder: 100). |

**Returns:** nothing. **Errors:** none; never panics.

**Special concerns:** pure, no I/O, no locks. It assumes `check_lsp_order` ran first; on an unordered vector the result is
still deterministic but not meaningful. It can push the last LSP above pi; the decoder tolerates that.

**Signature:** see the block above.

**See also:** `bw_expand_lsps_fixed`; test `bw_expand_lsps_separates_two_lsps_that_start_too_close`.

### `bw_expand_lsps_fixed` [@ANCHOR: bw_expand_lsps_fixed]

The Q23 twin of `bw_expand_lsps`: the margins arrive already in Q23 radians, from `min_sep_low_q23()` and
`min_sep_high_q23()` (329420 and 658840, the Q23 values of `50 * pi/4000` and `100 * pi/4000`; a test checks them against
`f32_to_q_exact_round`). Called by `DecoderFixed::decode` and `DecoderFixed::decode_16k_fixed` with the same margins.

```rust
pub fn bw_expand_lsps_fixed(lsp: &mut [i64; LPC_ORD], min_sep_low_q23: i64, min_sep_high_q23: i64)
```

| Argument | Type | Meaning |
|---|---|---|
| `lsp` | `&mut [i64; 10]` | Ascending LSPs in Q23 radians, edited in place. |
| `min_sep_low_q23` | `i64` | Minimum gap, Q23 radians, for indexes 1 to 3. |
| `min_sep_high_q23` | `i64` | Minimum gap, Q23 radians, for indexes 4 to 9. |

**Returns:** nothing. **Errors:** none.

**Special concerns:** pure integer code, no allocation, no I/O, no transmitter involvement.

**Signature:** see the block above.

**See also:** `bw_expand_lsps`; test `bw_expand_lsps_fixed_matches_the_float_version`.

### `interpolate_lsp_ver2` [@ANCHOR: interpolate_lsp_ver2]

Blends the previous frame's last LSPs with the new ones element by element, `(1 - w) * prev + w * next`. The 1600 decoder
uses weights 0.25, 0.5 and 0.75 for sub-frames 0 to 2 (and the new frame itself for sub-frame 3), because LSPs arrive once
per 40 ms but synthesis runs every 10 ms; the 3200 decoder's `interpolate_lsp` is the fixed 0.5 case. Present only with
the `std` feature.

```rust
pub fn interpolate_lsp_ver2(prev: &[f32; LPC_ORD], next: &[f32; LPC_ORD], weight: f32) -> [f32; LPC_ORD]
```

| Argument | Type | Meaning |
|---|---|---|
| `prev` | `&[f32; 10]` | Previous frame's LSPs, radians. |
| `next` | `&[f32; 10]` | Current frame's LSPs, radians. |
| `weight` | `f32` | Weight of `next`, normally 0.0 to 1.0; not clamped, so values outside extrapolate. |

**Returns:** a new array; the inputs are not modified. **Errors:** none.

**Special concerns:** pure, no I/O, no locks; the arrays are tiny and live on the stack. Interpolating between two ordered
vectors with a weight in 0 to 1 yields an ordered vector, so no re-check is needed.

**Signature:** see the block above.

**See also:** `interpolate_lsp_ver2_fixed`; test `interpolate_lsp_ver2_at_weight_zero_and_one_returns_the_endpoints`.

### `interpolate_lsp_ver2_fixed` [@ANCHOR: interpolate_lsp_ver2_fixed]

Restricts the weight to `quarters` of 1, 2 or 3 (0.25, 0.5, 0.75; debug-asserted), so the blend is
`((4 - q) * prev + q * next + 2) >> 2`: exact integer arithmetic with a single round-to-nearest shift and no division. It is
the only interpolation the integer decoders ever need.

```rust
pub fn interpolate_lsp_ver2_fixed(prev: &[i64; LPC_ORD], next: &[i64; LPC_ORD], quarters: i64) -> [i64; LPC_ORD]
```

| Argument | Type | Meaning |
|---|---|---|
| `prev` | `&[i64; 10]` | Previous frame's LSPs, Q23 radians. |
| `next` | `&[i64; 10]` | Current frame's LSPs, Q23 radians. |
| `quarters` | `i64` | Weight of `next` in quarters: 1, 2 or 3. |

**Returns:** a new Q23 array. **Errors:** in debug builds a `quarters` outside 1 to 3 panics (`debug_assert`); in release it
computes the formula anyway.

**Special concerns:** pure, no I/O, no transmitter involvement.

**Signature:** see the block above.

**See also:** `interpolate_lsp_ver2`; test `interpolate_lsp_ver2_fixed_matches_the_float_version_at_all_three_real_weights`.

## Codec2 1600 scalar LSP quantiser

`src/codec2_1600/lsp_quantiser.rs`. The ten LSP dimensions each have an evenly spaced codebook
(`start_hz + step_hz * index`), so quantising is a rounded division, not a table search. The codebooks are the
published Codec2 tables (`LSP_CB`: starts 225, 325, 500, 700, 950, 1100, 1500, 2300, 2500, 2900 Hz; steps 25, 25, 50,
100, 100, 100, 100, 100, 100, 200 Hz; 16 levels for dimensions 0 to 6, 8 for 7 and 8, 4 for 9). Every function is
pure and synchronous: no I/O, no locks, no transmitter involvement.

### `lsp_bits` [@ANCHOR: lsp_bits]

The bit width of dimension `i`: 4 for dimensions 0 to 6, 3 for 7 and 8, 2 for 9 (36 bits total); the frame packer uses it
for the LSP field widths, so the packed 64-bit frame and the codebook sizes cannot disagree.

```rust
pub fn lsp_bits(i: usize) -> u32
```

| Argument | Type | Meaning |
|---|---|---|
| `i` | `usize` | LSP dimension, 0 to 9. |

**Returns:** the width in bits (the codebook's `log2m`). **Errors:** `i >= 10` panics (array index).

**Special concerns:** pure, const-table lookup.

**Signature:** see the block above.

**See also:** test `lsp_bits_sums_to_36_total_bits`; `encode_lsps_scalar`.

### `quantise_dim` [@ANCHOR: quantise_dim]

Maps one target frequency in Hz to the nearest level of one dimension: round `(target - start) / step` and clamp into
`0..levels`. Because every real table is evenly spaced this equals the reference's linear scan for a one-dimensional
codebook, without the O(levels) search. Private; `std` only; called by `encode_lsps_scalar`.

```rust
fn quantise_dim(dim: &LspDim, target_hz: f32) -> u32
```

| Argument | Type | Meaning |
|---|---|---|
| `dim` | `&LspDim` | The dimension's `start_hz`, `step_hz`, `levels`. |
| `target_hz` | `f32` | Frequency to quantise, Hz; out-of-range values clamp to the first or last level. |

**Returns:** the codebook index, `0..levels`. **Errors:** none; a NaN input rounds to NaN and the clamp/cast gives index 0.

**Special concerns:** pure float; the integer path is `quantise_dim_fixed`.

**Signature:** see the block above.

**See also:** test `encode_then_decode_recovers_the_same_quantised_value_for_every_real_index`.

### `encode_lsps_scalar` [@ANCHOR: encode_lsps_scalar]

Converts ten LSPs from radians to Hz (`4000/pi`) and quantises each dimension independently with `quantise_dim`. The
1600 float encoder calls it once per 40 ms frame on the second analysis pass's LSPs. Present only with the `std` feature.

```rust
pub fn encode_lsps_scalar(lsp: &[f32; LPC_ORD]) -> [u32; LPC_ORD]
```

| Argument | Type | Meaning |
|---|---|---|
| `lsp` | `&[f32; 10]` | Unquantised LSPs, radians, ascending. |

**Returns:** ten indexes, each below the dimension's level count (fit `lsp_bits(i)` bits). **Errors:** none; out-of-range
LSPs clamp.

**Special concerns:** pure; the quantiser is not vector or delta coded, so a bit error in one index affects only that
dimension.

**Signature:** see the block above.

**See also:** `decode_lsps_scalar`, `encode_lsps_scalar_fixed`; test
`encode_then_decode_recovers_the_same_quantised_value_for_every_real_index`.

### `decode_lsps_scalar` [@ANCHOR: decode_lsps_scalar]

The inverse of `encode_lsps_scalar`: `start + step * index` in Hz, converted back to radians with `pi/4000`. This is the
table any compliant decoder must reproduce, so it is a wire-format fact, not a design choice. Present only with the `std`
feature.

```rust
pub fn decode_lsps_scalar(indexes: &[u32; LPC_ORD]) -> [f32; LPC_ORD]
```

| Argument | Type | Meaning |
|---|---|---|
| `indexes` | `&[u32; 10]` | One index per dimension, as unpacked from the frame. |

**Returns:** ten LSPs in radians. **Errors:** none; an index at or above the level count is not checked and simply
extrapolates the table (the frame unpacker's field width already bounds it).

**Special concerns:** pure. Run `check_lsp_order` and `bw_expand_lsps` on the result.

**Signature:** see the block above.

**See also:** `decode_lsps_scalar_fixed`; tests `decode_matches_the_real_upstream_codebook_tables_at_specific_indices`,
`encode_then_decode_recovers_the_same_quantised_value_for_every_real_index`.

### `q_mul_q23` [@ANCHOR: q_mul_q23]

Multiplies two Q23 numbers in `i128` and shifts the product back to Q23 (arithmetic shift, so rounding toward minus
infinity). The fixed LSP encoder and decoder use it for the radian/Hz conversions. Private to `lsp_quantiser.rs`.

```rust
fn q_mul_q23(a: i64, b: i64) -> i64
```

| Argument | Type | Meaning |
|---|---|---|
| `a`, `b` | `i64` | Q23 operands. |

**Returns:** `(a * b) >> 23` as `i64`; the intermediate cannot overflow, the final cast truncates only if the result
exceeds `i64`, which the callers' ranges never reach. **Errors:** none.

**Special concerns:** pure, no float.

**Signature:** see the block above.

**See also:** `encode_lsps_scalar_fixed`, `decode_lsps_scalar_fixed`; test
`decode_lsps_scalar_fixed_matches_the_float_version_for_every_real_index`.

### `encode_lsps_scalar_fixed` [@ANCHOR: encode_lsps_scalar_fixed]

The integer encoder: each Q23 radian angle goes to Q23 Hz with `q_mul_q23` and the constant `4000/pi` (10680707072), then
through the integer nearest-level search `quantise_dim_fixed` (round-half-up division, clamped to `0..levels`, with
differences at or below zero giving index 0). No `f32` is touched, so it runs on cores without an FPU.

```rust
pub fn encode_lsps_scalar_fixed(lsp_q23: &[i64; LPC_ORD]) -> [u32; LPC_ORD]
```

| Argument | Type | Meaning |
|---|---|---|
| `lsp_q23` | `&[i64; 10]` | Unquantised LSPs, Q23 radians. |

**Returns:** ten codebook indexes. **Errors:** none; out-of-range values clamp.

**Special concerns:** pure, no allocation, no I/O, no transmitter involvement. Used by `codec2_1600::EncoderFixed::encode`.

**Signature:** see the block above.

**See also:** `encode_lsps_scalar`; test `encode_then_decode_recovers_the_same_quantised_value_for_every_real_index_fixed`.

### `decode_lsps_scalar_fixed` [@ANCHOR: decode_lsps_scalar_fixed]

Computes `start + step * index` in Q23 Hz (the table entries are whole Hz shifted left 23, built at compile time by
`lsp_cb_q23`) and converts to Q23 radians with `q_mul_q23` and `pi/4000` (6588 in Q23). It reads the same table as
`decode_lsps_scalar`, never touching `f32`.

```rust
pub fn decode_lsps_scalar_fixed(indexes: &[u32; LPC_ORD]) -> [i64; LPC_ORD]
```

| Argument | Type | Meaning |
|---|---|---|
| `indexes` | `&[u32; 10]` | One index per dimension. |

**Returns:** ten Q23 radian LSPs. **Errors:** none; indexes are not range-checked.

**Special concerns:** pure integer code; the result still needs `check_lsp_order_fixed` and `bw_expand_lsps_fixed`.

**Signature:** see the block above.

**See also:** `decode_lsps_scalar`; tests `decode_lsps_scalar_fixed_matches_the_float_version_for_every_real_index`,
`encode_then_decode_recovers_the_same_quantised_value_for_every_real_index_fixed`.

## Codec2 1600 encoder internals

`src/codec2_1600/mod.rs`. The 1600 encoder keeps a 320-sample (40 ms) history window `sn` and processes four 10 ms
sub-frames of 80 samples per 40 ms frame. The helpers below are private to the module and run only inside
`Encoder::encode` / `EncoderFixed::encode`; they are synchronous, take no locks and do no I/O.

### `analyse_lsps_and_energy` [@ANCHOR: analyse_lsps_and_energy]

One linear-prediction pass over the 320-sample history window: apply the analysis window, autocorrelate, apply the
white-noise correction, run Levinson-Durbin, measure the prediction energy on the uncorrected autocorrelation, expand
bandwidth with `bw_gamma`, and convert to LSPs (an evenly spaced fallback, `pi/10 * i`, if the root search fails). The
encoder runs it twice per frame but sends the LSPs of the second pass only; the first pass exists for its energy. The
energy is taken before bandwidth expansion because afterwards it can come out negative.

```rust
fn analyse_lsps_and_energy(sn: &[f32; M_PITCH], analysis_window: &[f32; M_PITCH]) -> ([f32; LPC_ORD], f32)
```

| Argument | Type | Meaning |
|---|---|---|
| `sn` | `&[f32; 320]` | History window, newest sample last. |
| `analysis_window` | `&[f32; 320]` | The analysis window, `window::make_analysis_window()`. |

**Returns:** `(lsp, e)`: ten LSPs in radians and the frame energy. **Errors:** none; a failed root search falls back to
the even LSPs. Present only with the `std` feature.

**Special concerns:** pure float computation, no allocation beyond the stack.

**Signature:** see the block above.

*(Reference: `src/codec2_1600/mod.rs`)*

**See also:** `analyse_lsps_and_energy_fixed`; test `encode_decode_round_trip_produces_finite_reasonably_scaled_audio`.

### `analyse_lsps_and_energy_fixed` [@ANCHOR: analyse_lsps_and_energy_fixed]

The integer version of the same pass, implemented by the function `analyse_energy_and_ak_fixed`: the analysis window (Q30)
times the samples is shifted down 7 bits into a `Q8.23`-style `i32` buffer, then integer autocorrelation, integer
white-noise correction and Levinson-Durbin, the energy before bandwidth expansion, then `apply_bw_gamma_fixed`. The LSP
root search is a separate step (`lpc::lpc_to_lsp_q23_from_integer_ak`), run by the caller on the second pass only.

```rust
fn analyse_energy_and_ak_fixed(sn: &[i16; M_PITCH]) -> (i64, [i64; LPC_ORD + 1])
```

| Argument | Type | Meaning |
|---|---|---|
| `sn` | `&[i16; 320]` | History window, newest sample last. |

**Returns:** `(e_q23, a_q23)`: the Q23 energy and the eleven bandwidth-expanded Q23 coefficients. **Errors:** none.

**Special concerns:** integer end to end, no allocation, no I/O.

**Signature:** see the block above.

*(Reference: `src/codec2_1600/mod.rs`)*

**See also:** `analyse_lsps_and_energy`; test `fixed_encode_decode_round_trip_produces_finite_reasonably_scaled_audio`.

### `codec2_1600:Encoder::shift_in` [@ANCHOR: codec2_1600:Encoder::shift_in]

Slides the float history window left by 80 samples and appends the next 80 new samples as `f32` (converted from `i16`);
it asserts the slice is exactly 80 samples long (`N_SAMP`) instead of silently truncating or padding, as a `zip` loop
would. Called four times per 40 ms frame (once per 10 ms sub-frame) from `Encoder::encode`.

```rust
fn shift_in(&mut self, new_samples: &[i16])
```

| Argument | Type | Meaning |
|---|---|---|
| `new_samples` | `&[i16]` | Exactly 80 new samples at 8000 Hz. |

**Returns:** nothing. **Errors:** a slice of any other length panics (`assert_eq!`); every current caller passes 80.

**Special concerns:** mutates `self.sn` in place; the encoder is single-owner and not `Sync`. Float, `std` only.

**Signature:** see the block above.

**See also:** `codec2_1600:EncoderFixed::shift_in`; test `encode_decode_round_trip_produces_finite_reasonably_scaled_audio`.

### `codec2_1600:EncoderFixed::shift_in` [@ANCHOR: codec2_1600:EncoderFixed::shift_in]

Does the same on the `i16` history (called four times per frame from `EncoderFixed::encode`): `copy_within` moves the newest 240 samples to the front, `copy_from_slice` writes the 80
new ones at the end. A wrong slice length panics through `copy_from_slice`.

```rust
fn shift_in(&mut self, new_samples: &[i16])
```

| Argument | Type | Meaning |
|---|---|---|
| `new_samples` | `&[i16]` | Exactly 80 new samples at 8000 Hz. |

**Returns:** nothing. **Errors:** length not 80 panics.

**Special concerns:** mutates the history in place; no allocation; the caller owns the encoder.

**Signature:** see the block above.

**See also:** `codec2_1600:Encoder::shift_in`; tests `fixed_shift_in_drops_the_oldest_samples_and_appends_the_new_ones_at_the_end`,
`fixed_shift_in_panics_on_a_wrong_length_slice`.

## Bit packing primitives (3200 and 1600)

`src/codec2_3200/bits.rs`, shared by both frame formats. Bits are packed MSB-first within each byte, and any field wider
than one bit is Gray-coded; both are interoperability requirements of the Codec2 wire format.

### `gray_to_binary` [@ANCHOR: gray_to_binary]

Undoes a Gray code with five XOR-folds (shifts of 16, 8, 4, 2, 1); the encoding direction is `binary_to_gray`, `x ^ (x >> 1)`.
Round trips are checked over every value up to 10 bits and at 32-bit boundary and pattern values. Private; used by
`BitReader::read`.

```rust
fn gray_to_binary(g: u32) -> u32
```

| Argument | Type | Meaning |
|---|---|---|
| `g` | `u32` | A Gray-coded value. |

**Returns:** the binary value. **Errors:** none; defined for all 32-bit inputs.

**Special concerns:** pure, constant time.

**Signature:** see the block above.

**See also:** tests `gray_code_round_trips_over_every_value_up_to_10_bits`,
`gray_code_round_trips_at_full_32_bit_boundary_and_pattern_values`.

### `BitWriter::write` [@ANCHOR: BitWriter::write]

Appends a field of `width` bits MSB-first into a zeroed byte buffer, Gray-coding it when the width is greater than one bit.
Bits are only OR-ed in, so a value that does not fit its width would corrupt neighbouring fields already in the same byte;
that is a debug-asserted precondition. A field may straddle byte boundaries.

```rust
pub fn write(&mut self, field: u32, width: u32)
```

| Argument | Type | Meaning |
|---|---|---|
| `field` | `u32` | The value, which must be below `1 << width` (width 32 is allowed). |
| `width` | `u32` | Number of bits, 1 to 32; 0 writes nothing. |

**Returns:** nothing; the writer's bit position advances by `width`. **Errors:** a field that does not fit panics in debug
builds (silently corrupts in release); writing past the end of the buffer panics on the slice index.

**Special concerns:** the buffer given to `BitWriter::new` must already be zeroed. Not thread-safe; owned by the packer.

**Signature:** see the block above.

**See also:** `BitReader::read`; test `bit_writer_reader_round_trip_arbitrary_field_widths`.

### `BitReader::read` [@ANCHOR: BitReader::read]

The reverse of `BitWriter::write`: gathers `width` bits MSB-first across byte boundaries and un-Grays fields wider than one
bit.

```rust
pub fn read(&mut self, width: u32) -> u32
```

| Argument | Type | Meaning |
|---|---|---|
| `width` | `u32` | Number of bits, 1 to 32. |

**Returns:** the decoded field value; the reader's position advances by `width`. **Errors:** reading beyond the end of the
buffer panics (slice index); callers pass fixed-size frames.

**Special concerns:** no state besides the bit position; any bit pattern decodes to some value, so a corrupted frame is not
detected here.

**Signature:** see the block above.

**See also:** `BitWriter::write`; test `bit_writer_reader_round_trip_arbitrary_field_widths`.

## Codec2 3200 fixed-point encoder

### `encoder_fixed:shift_in` [@ANCHOR: encoder_fixed:shift_in]

Slides the `i16` pitch history left by one 10 ms step (`copy_within` of the newest 240 samples to the front) and appends
the 80 new samples at the end. `pub(super)`: `EncoderMbe` drives the same history through its inner `EncoderFixed`.
Called twice per 20 ms frame by `EncoderFixed::encode`.

```rust
pub(super) fn shift_in(&mut self, new_samples: &[i16])
```

| Argument | Type | Meaning |
|---|---|---|
| `new_samples` | `&[i16]` | Exactly 80 new samples at 8000 Hz. |

**Returns:** nothing. **Errors:** a wrong length panics (`copy_from_slice`).

**Special concerns:** mutates the history in place, no allocation, no I/O; single owner.

**Signature:** see the block above.

*(Reference: `src/codec2_3200/encoder_fixed.rs`)*

**See also:** test `shift_in_drops_the_oldest_samples_and_appends_the_new_ones_at_the_end`.

### `EncoderFixed::lpc_fields` [@ANCHOR: EncoderFixed::lpc_fields]

**Purpose.** The energy index and the ten line spectral pair indexes of the frame now in the encoder's history
buffer. It is the second half of `EncoderFixed::encode`, split out unchanged (bit for bit; the golden hash in
`tests/codec2_3200_encoder_golden_hash.rs` pins it) so `EncoderMbe` can share it and differ only in pitch and
voicing.

```rust
pub(super) fn lpc_fields(&self) -> (u32, [u32; LPC_ORD])
```

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&EncoderFixed` | Read only: its 320-sample `sn` history, after the second `shift_in` of a 20 ms frame. |

**Returns.** `(e_index, lsp_indexes)`: the 5-bit energy quantizer index and the ten scalar LSP quantizer indexes,
ready for `bits::pack_frame`.

**Special concerns.** Pure, integer end to end, no allocation. Windows and correlates only `window::ANALYSIS_SPAN`.
Applies white noise correction (`R[0] * (1 + 1e-3)`) before Levinson-Durbin: without it the recursion divides by a
prediction error that can come arbitrarily close to zero on strongly predictable frames, and the coefficients diverge
(`apply_white_noise_correction_fixed`). The line spectral pairs come from that corrected predictor, and so does the
frame energy: the corrected predictor's error on the corrected autocorrelation. The decoder rebuilds a frame's level
from the energy times the transmitted envelope, so the two must belong to one model. Measured 2026-11-07 (decoded
loudness against the original, four Open Speech Repository sentences): -0.5 to -1.0 dB mean with this energy; -0.8 to
-1.5 dB with the corrected predictor on the uncorrected autocorrelation (used until then); -2.7 to -3.8 dB with the
uncorrected minimum error, which is what the Codec2 C encoder sends with its own, uncorrected envelope, and which was
briefly used here on 2026-11-07 until that measurement. So this energy index matches the C encoder's only on 34 to 64
percent of frames, by design (see hams_com `docs/reviews/M17_CODEC2_WOJCIECH_2026-11-06.md`).


**See also.** `analysis_span`, `EncoderMbe::encode`. Test: `encoder_mbe_marks_a_steady_vowel_voiced_and_keeps_the_fixed_encoders_envelope`.

## Codec2 3200 spectral pitch and voicing analysis (`src/codec2_3200/encoder_mbe.rs`)

`EncoderMbe` is the 3200 bit/s encoder for hosts with a floating-point unit, and the relay's M17 voice encoder. It
replaces `EncoderFixed`'s pitch bin and energy/zero-crossing voicing heuristic with the multi-band-excitation analysis
every Codec2 encoder in the wild uses (Griffin and Lim, 1988, as Codec2 applies it; Codec2-mod keeps it): the windowed
speech spectrum, a harmonic-sum pitch refinement and a harmonic-fit voicing decision. Energy and line spectral pairs
come from `EncoderFixed::lpc_fields`. It is an independent implementation from the method, not a translation of the
LGPL-2.1-only C (see `LICENSING.md`). On the four Open Speech Repository sentences it agrees with the Codec2-mod C
encoder on both voicing bits in 93 to 98 percent of frames (`EncoderFixed`: 6 to 15 percent) and on the pitch index
in 62 to 65 percent (`EncoderFixed`: 25 to 31), `tests/codec2_3200_encoder_vs_reference.rs`. Cost: 47 s of speech in
62 ms on one x86-64 core (`EncoderFixed`: 29 ms; the C reference: 45 ms). Only with the `std` feature.

### `EncoderMbe` [@ANCHOR: EncoderMbe]

**Purpose.** State of one encoding stream: one per voice transmission (an M17 over), created at its start, dropped
at its end.

```rust
pub struct EncoderMbe { core: EncoderFixed, analyser: MbeAnalyser }
```

| Field | Type | Meaning |
|---|---|---|
| `core` | `EncoderFixed` | The 320-sample `i16` history, the pitch estimator's state, and the integer LPC path. |
| `analyser` | `MbeAnalyser` | The analysis window and its spectrum, fixed after `new`. |

**Lifetime and concurrency.** Owned by one caller, `Send`, not shared; about 13 KB. Not reset between overs: make a
new one. Created by `EncoderMbe::new`, changed only by `EncoderMbe::encode`.

### `EncoderMbe::new` [@ANCHOR: EncoderMbe::new]

**Purpose.** A fresh encoder: silent history, pitch estimator at its initial state.

```rust
pub fn new() -> EncoderMbe
```

**Returns.** The encoder. **Special concerns.** Computes the window spectrum once (one 512-point FFT); no I/O, no
panics. `Default` does the same.

### `EncoderMbe::encode` [@ANCHOR: EncoderMbe::encode]

**Purpose.** Encodes one 20 ms frame of 8 kHz speech to one 8-byte Codec2 3200 frame.

```rust
pub fn encode(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME]
```

| Argument | Type | Meaning |
|---|---|---|
| `speech` | `&[i16; 160]` | 20 ms of mono speech at 8000 samples per second, full scale +-32767, borrowed. |

**Returns.** The packed frame: voicing bit of the first 10 ms, voicing bit of the second, 7-bit pitch index (from the
second analysis), 5-bit energy index, 50 bits of line spectral pairs, in the standard Codec2 3200 layout (any
Codec2 3200 decoder decodes it).

**Special concerns.** Synchronous, CPU only (about 26 microseconds per frame on x86-64), no allocation besides two
small vectors per analysis, never panics on any input. Frames must be given in order; the encoder keeps 40 ms of
history, so a stream's first two frames see the silent start.

**Example.**

```rust
let mut enc = EncoderMbe::new();
for chunk in pcm.chunks_exact(SAMPLES_PER_FRAME) {
    frames.push(enc.encode(chunk.try_into().unwrap()));
}
```

**See also.** `EncoderMbe::analyse_half`, `EncoderFixed::lpc_fields`. Tests:
`encoder_mbe_voicing_and_pitch_agree_with_the_c_reference_on_real_speech`,
`encoder_mbe_bitstream_on_real_speech_and_noise_is_unchanged`.

### `EncoderMbe::analyse_half` [@ANCHOR: EncoderMbe::analyse_half]

**Purpose.** Shifts 10 ms (`half`, 80 samples) into the history, runs the integer pitch estimator for its 3.125 Hz bin,
and refines and classifies it with `MbeAnalyser::analyse`. Private; called twice per `encode`.

**Returns.** The `MbeFrame`. **Errors.** A `half` that is not 80 samples panics (in `shift_in`); `encode` always
passes 80.

### `MbeFrame` [@ANCHOR: MbeFrame]

**Purpose.** The result of one 10 ms analysis.

| Field | Type | Meaning |
|---|---|---|
| `wo` | `f32` | Fundamental in radians per sample (`2 pi f0 / 8000`), within `[2 pi / 160, 2 pi / 20]` (50 to 400 Hz). |
| `l` | `usize` | Harmonics below 4 kHz, `floor(pi / wo)`, one fewer if the last is within 5 percent of 4 kHz; at least 1. |
| `voiced` | `bool` | The voicing decision after the energy-ratio rules. |
| `snr_db` | `f32` | Harmonic-fit signal-to-error ratio below 1 kHz, dB, before those rules (diagnostic). |

`Copy`, created by `MbeAnalyser::analyse`, read by `EncoderMbe::encode`.

### `MbeAnalyser` [@ANCHOR: MbeAnalyser]

**Purpose.** What the analysis keeps between frames: nothing that changes.

| Field | Type | Meaning |
|---|---|---|
| `window` | `[f32; 320]` | `make_analysis_window()`: the 279-sample Hann window centred in the history. |
| `window_spectrum` | `[f32; 512]` | Real part of the window's 512-point spectrum with the window centred on time zero, rotated so index 256 is DC. The voicing fit places it on each harmonic. |

Immutable after `new`; shareable between threads by reference.

### `MbeAnalyser::new` [@ANCHOR: MbeAnalyser::new]

**Purpose.** Builds the analysis window (`window::make_analysis_window()`, the 279-sample Hann window centred in the 320-sample
history) and its spectrum. The window is laid out centred on time zero (its middle at index 0, its first half wrapped to the end
of a 512-point buffer) so its spectrum is real, transformed with one 512-point `microfft` complex FFT, and rotated so DC sits at
index 256; the real parts are kept as `window_spectrum`. `EncoderMbe::new` calls it once per stream, and `Default` does the same.

```rust
pub fn new() -> Self
```

**Returns.** The analyser, immutable from then on. **Errors.** None; no panics.

**Special concerns.** Pure CPU, about one FFT of work, no I/O, no allocation on the heap, no locks. Nothing about it keys a
transmitter; it feeds only the encoder's pitch and voicing decision.

**Signature.** See the block above.

**See also.** `MbeAnalyser`, `MbeAnalyser::spectrum`; test `window_spectrum_peaks_at_dc_in_the_middle_and_is_symmetric`.

### `MbeAnalyser::spectrum` [@ANCHOR: MbeAnalyser::spectrum]

**Purpose.** The 512-point spectrum `Sw` of the windowed history, the window's centre at time zero (its second half at
the start of the FFT input, its first half wrapped to the end), so harmonic phases are referred to the frame centre.

```rust
pub fn spectrum(&self, sn: &[i16; M_PITCH]) -> [Complex32; FFT_ENC]
```

| Argument | Type | Meaning |
|---|---|---|
| `sn` | `&[i16; 320]` | The encoder's 40 ms history, newest sample last. |

**Returns.** All 512 bins (the input is real, so bins above 256 mirror those below; the pitch search reads them).
Forward transform, no scaling. Pure.

### `MbeAnalyser::analyse` [@ANCHOR: MbeAnalyser::analyse]

**Purpose.** Pitch and voicing of the frame in `sn`.

```rust
pub fn analyse(&self, sn: &[i16; M_PITCH], f0_hz: f32) -> MbeFrame
```

| Argument | Type | Meaning |
|---|---|---|
| `sn` | `&[i16; 320]` | History, as for `spectrum`. |
| `f0_hz` | `f32` | Starting pitch in Hz (the estimator's); clamped to 50 to 400 Hz first, so any value is accepted. |

**Returns.** The `MbeFrame`: `refine_pitch`, then `harmonic_count`, `harmonic_amplitudes`, `harmonic_fit_snr_db`
against `VOICING_SNR_DB` (6 dB), then `apply_energy_ratio_rules`. Pure, no panics.

### Constants [@ANCHOR: mbe_voicing_constants]

| Name | Value | Meaning |
|---|---|---|
| `VOICING_SNR_DB` | `6.0` | Harmonic-fit signal-to-error ratio (dB, below 1 kHz) above which a frame is voiced. |
| `NLP_BIN_HZ` | `3.125` | Width of the pitch estimator's bin, `8000 / (512 * 5)` Hz; turns its bin into a starting `f0`. |

### `mbe_bin_of` [@ANCHOR: mbe_bin_of]

**Purpose.** Converts an angular frequency to a position in the encoder's 512-point spectrum, as a real number, so each
caller chooses its own rounding (round to nearest in `harmonic_sum` and `harmonic_amplitudes`, ceiling in
`harmonic_fit_snr_db`). The source function is named `bin_of` and is private.

```rust
fn bin_of(x: f32) -> f32
```

| Argument | Type | Meaning |
|---|---|---|
| `x` | `f32` | Angular frequency in radians per sample (`2 pi f / 8000`). |

**Returns.** `x * 512 / (2 pi)`: bin 256 is 4 kHz. Negative or huge inputs give correspondingly negative or huge values; the
callers bound them.

**Errors.** None. **Special concerns.** Pure, no state.

**See also.** `mbe_harmonic_sum`, `mbe_harmonic_amplitudes`, `mbe_harmonic_fit_snr_db`; test
`pitch_refinement_lands_on_the_true_fundamental_from_a_coarse_start`.

### `mbe_harmonic_count` [@ANCHOR: mbe_harmonic_count]

**Purpose.** How many harmonics of a fundamental fit below 4 kHz, the `L` of the Codec2 model, with a guard against rounding
at the band edge. Called by `MbeAnalyser::analyse` after the pitch is refined.

```rust
pub fn harmonic_count(wo: f32) -> usize
```

| Argument | Type | Meaning |
|---|---|---|
| `wo` | `f32` | Fundamental in radians per sample, greater than 0 (the analysis clamps it to 50 to 400 Hz first). |

**Returns.** `floor(pi / wo)`, minus one when that is more than 1 and `l * wo >= 0.95 pi` (the last harmonic is within 5
percent of 4 kHz), and never less than 1.

**Errors.** None, but `wo` of 0 or less is outside the contract: division by zero gives infinity, which casts to the largest
`usize`. **Special concerns.** Pure.

**See also.** `MbeFrame`; test `pitch_refinement_lands_on_the_true_fundamental_from_a_coarse_start`.

### `mbe_harmonic_sum` [@ANCHOR: mbe_harmonic_sum]

**Purpose.** The score the pitch search maximises: the spectral power at the harmonic bins of a candidate fundamental. The
private function is `harmonic_sum`; `refine_pitch` calls it about 50 times per analysis.

```rust
fn harmonic_sum(sw: &[Complex32; FFT_ENC], wo: f32, l: usize) -> f32
```

| Argument | Type | Meaning |
|---|---|---|
| `sw` | `&[Complex32; 512]` | The frame's spectrum from `MbeAnalyser::spectrum`. |
| `wo` | `f32` | Candidate fundamental, radians per sample. |
| `l` | `usize` | Number of harmonics to add, `1..=l`. |

**Returns.** The sum over `m` in `1..=l` of `|sw[round(bin_of(m * wo))]|^2`; a bin at or past index 512 adds nothing (checked
with `get`, so it cannot panic).

**Errors.** None. **Special concerns.** Pure, allocation free, `O(l)`.

**See also.** `mbe_refine_pitch`, `mbe_bin_of`; test `pitch_refinement_lands_on_the_true_fundamental_from_a_coarse_start`.

### `mbe_refine_pitch` [@ANCHOR: mbe_refine_pitch]

**Purpose.** Corrects the pitch estimator's coarse fundamental to the one whose harmonics collect the most power.

```rust
pub fn refine_pitch(sw: &[Complex32; FFT_ENC], wo: f32) -> f32
```

| Argument | Type | Meaning |
|---|---|---|
| `sw` | `&[Complex32; 512]` | The frame's spectrum (`MbeAnalyser::spectrum`). |
| `wo` | `f32` | Starting fundamental, radians per sample, > 0. |

**Returns.** The refined fundamental, clamped to `[2 pi / 160, 2 pi / 20]`. Two passes over the pitch period
`P = 2 pi / wo`: `P - 5 .. P + 5` samples in steps of 1, then `+-1` around the winner in steps of 0.25; each candidate
is scored by `harmonic_sum` with the harmonic count of the pass's starting pitch; the first maximum wins. Pure; about
50 candidate scorings per call. It can move the pitch by at most 6 samples of period, so it does not correct an octave
error of the estimator.

### `mbe_harmonic_amplitudes` [@ANCHOR: mbe_harmonic_amplitudes]

`pub fn harmonic_amplitudes(sw, wo, l) -> Vec<f32>`: index `m` in `1..=l` holds the square root of the power in
harmonic `m`'s band, bins `round((m - 1/2) wo)` up to but not including `round((m + 1/2) wo)` (bins of `bin_of`);
index 0 is 0. Length `l + 1`. Pure.

### `mbe_harmonic_fit_snr_db` [@ANCHOR: mbe_harmonic_fit_snr_db]

**Purpose.** How voiced the frame is: how well a sum of harmonics explains its spectrum below 1 kHz.

```rust
pub fn harmonic_fit_snr_db(sw: &[Complex32; FFT_ENC], window_spectrum: &[f32; FFT_ENC], wo: f32, l: usize, amps: &[f32]) -> f32
```

| Argument | Type | Meaning |
|---|---|---|
| `sw` | `&[Complex32; 512]` | The frame's spectrum. |
| `window_spectrum` | `&[f32; 512]` | `MbeAnalyser`'s, DC at index 256. |
| `wo`, `l` | `f32`, `usize` | Refined fundamental and harmonic count. |
| `amps` | `&[f32]` | `harmonic_amplitudes`' output, length at least `l + 1`. |

**Returns.** `10 log10(signal / error)` in dB: the signal is the power of the harmonics below 1 kHz
(`floor(l * 1000 / 4000)` of them); for each, the bins from `ceil((h - 1/2) wo)` to `ceil((h + 1/2) wo)` are fitted by
the window spectrum centred on harmonic `h` times the least-squares complex gain, and the error is the power the fit
leaves. Both have `1e-4` added, so the result is finite. Pure. **Errors.** `amps` shorter than `l + 1` panics.

### `mbe_apply_energy_ratio_rules` [@ANCHOR: mbe_apply_energy_ratio_rules]

`pub fn apply_energy_ratio_rules(voiced: bool, wo: f32, l: usize, amps: &[f32]) -> bool`: corrects the fit's decision
with the ratio of harmonic power below 2 kHz to power from 2 kHz up, in dB. An unvoiced frame becomes voiced above
10 dB (low-passed speech the fit missed); a voiced frame becomes unvoiced below -10 dB (sibilants), or below -4 dB when
`wo` is 60 Hz or lower (a pitch locked on low-frequency noise). Pure; `amps` shorter than `l + 1` panics.

## Spectral envelope (decoder)

`src/codec2_3200/envelope.rs` turns eleven LPC coefficients plus a frame energy into one amplitude per
harmonic, by sampling the LPC synthesis filter's magnitude at each harmonic and applying an energy-preserving
formant-sharpening postfilter (the classic Chen and Gersho idea with `gamma = 0.5`, `beta = 0.2`).

- `lpc_spectrum` zero-pads the coefficients into a 512-point buffer, runs a forward FFT and returns the
  first 257 bins.
  *(Reference: `[@ANCHOR: lpc_spectrum]`)*
- `lpc_spectrum_fixed` does this in Q23 with the sparse-prefix forward FFT into the shared scratch buffers
  (only the first 11 inputs are non-zero).
  *(Reference: `[@ANCHOR: lpc_spectrum_fixed]`)*
- `compute_harmonic_amplitudes` computes the power spectrum of the filter and of its bandwidth-expanded copy,
  derives a gain that keeps the postfiltered energy equal to the frame energy, and sets each harmonic's
  amplitude from the postfilter-weighted power over the bins around it (1.96 times weight below 1 kHz). It
  also returns the complex spectrum so the synthesis can read the filter's phase.
  *(Reference: `[@ANCHOR: compute_harmonic_amplitudes]`)*
- `compute_harmonic_amplitudes_fixed` is the Q23 version, replacing square roots and powers with
  `log2_q23`/`exp2_q23`, computing the per-bin postfilter weight once and reusing it for the gain and for
  every harmonic band; the gain is one exact 128-bit division.
  *(Reference: `[@ANCHOR: compute_harmonic_amplitudes_fixed]`)*
- `apply_first_harmonic_correction` multiplies the first harmonic's amplitude by 0.032 when the pitch is
  below 150 Hz (`wo < pi * 150 / 4000`), because LPC overestimates the fundamental of low voices.
  *(Reference: `[@ANCHOR: apply_first_harmonic_correction]`)*
- `apply_first_harmonic_correction_fixed` does the same comparison and scaling against Q23 constants.
  *(Reference: `[@ANCHOR: apply_first_harmonic_correction_fixed]`)*
- `sample_filter_phase` returns, for each harmonic `m`, the conjugate of the LPC spectrum at the bin nearest
  `m * wo`: the synthesis filter's phase, opposite to the analysis filter's.
  *(Reference: `[@ANCHOR: sample_filter_phase]`)*
- `sample_filter_phase_fixed` is a test-only wrapper that applies the production phase-sampling step to a
  full spectrum array (production reads the transform scratch directly).
  *(Reference: `[@ANCHOR: sample_filter_phase_fixed]`)*

## Fixed-point FFT, logarithm and trigonometry

- `ComplexQ23::mul` multiplies two Q23 complex numbers and rescales to Q23; a plain `i64` path when every
  component is under 2^30, an exact `i128` path otherwise, both bit-identical.
  *(Reference: `src/codec2_3200/fixed_fft.rs` -> `[@ANCHOR: ComplexQ23::mul]`)*
- `fixed_fft:rshift_round_i128` is the round-half-up right shift for an `i128` accumulator that narrows to
  `i64`, with a debug assertion that the result fits (the FFT butterflies are bounded far below that).
  *(Reference: `[@ANCHOR: fixed_fft:rshift_round_i128]`)*
- `build_bit_reverse_table` computes, at compile time, the bit-reversal permutation for an `N`-point
  transform as `u16` entries.
  *(Reference: `[@ANCHOR: build_bit_reverse_table]`)*
- `fft_twiddles_q23` (test helper) returns the twiddle-factor table for transform size `N`.
  *(Reference: `[@ANCHOR: fft_twiddles_q23]`)*
- `fft_bit_reverse_table` (test helper) returns the bit-reversal table for transform size `N`.
  *(Reference: `[@ANCHOR: fft_bit_reverse_table]`)*
- `log2_lut_generic_fixed` computes `log2(x)` for a positive `f32` by taking the exponent from the IEEE-754
  bits and interpolating the mantissa's logarithm in a Q23 table, using the raw mantissa bits as the
  interpolation weight; no float arithmetic until the final sum.
  *(Reference: `src/codec2_3200/fixed_point.rs` -> `[@ANCHOR: log2_lut_generic_fixed]`)*
- `f32_to_q_exact_round` converts an `f32` to a Q format with exact rounding (the magnitude is rounded half up,
  i.e. halves go away from zero) by shifting its 24-bit significand (zero maps to zero; subnormals and out-of-range values are debug-asserted).
  *(Reference: `[@ANCHOR: f32_to_q_exact_round]`)*
- `exp2_lut_generic_fixed` is the mirror image: `2^y` for an `f32`, with `y` converted exactly to Q24, split
  into a floor and a fraction, interpolated in a Q23 table and rebuilt as an IEEE-754 bit pattern.
  *(Reference: `[@ANCHOR: exp2_lut_generic_fixed]`)*
- `log2_q23` is the integer-in, integer-out base-2 logarithm of a positive Q23 value: normalise by counting
  leading zeros, table-interpolate the mantissa. Zero is treated as the smallest positive value.
  *(Reference: `[@ANCHOR: log2_q23]`)*
- `exp2_q23` is `2^y` for a Q23 exponent, returning Q23: the integer part shifts, the fractional part is
  table-interpolated; very negative `y` underflows to 0 and very large `y` is debug-asserted.
  *(Reference: `[@ANCHOR: exp2_q23]`)*
- `sin_cos_q23` returns `(cos, sin)` as a Q23 complex number for a `u32` binary angle (the full range is one
  turn), indexing a 4096-cell (4097-point) table with the top 12 bits and interpolating with the rest.
  *(Reference: `src/codec2_3200/trig_fixed.rs` -> `[@ANCHOR: sin_cos_q23]`)*

## Floating-point LPC analysis (reference, and the live 1600 encoder)

`src/codec2_3200/floating_reference/lpc.rs`.

- `autocorrelate` computes `R[j] = sum wn[i] * wn[i + j]` for lags 0 to 10 over the windowed buffer.
  *(Reference: `[@ANCHOR: autocorrelate]`)*
- `levinson_durbin` solves for the ten predictor coefficients from the autocorrelation (`ak[0] = 1`); a
  reflection coefficient larger than 1 in magnitude is replaced by 0, and the recursion stops with the
  identity filter when the residual energy reaches zero (silence) instead of dividing 0 by 0.
  *(Reference: `[@ANCHOR: levinson_durbin]`)*
- `build_p_q` forms the two symmetric and antisymmetric polynomials `P` and `Q` (the LPC filter plus and
  minus its mirror) in Chebyshev form, whose interleaved roots are the LSP frequencies.
  *(Reference: `[@ANCHOR: build_p_q]`)*
- `find_next_root` finds one root of such a polynomial by sweeping `x` downward from a start point in 0.01
  steps until the sign changes, then bisecting six times; it returns `None` when the sweep reaches -1
  without a sign change.
  *(Reference: `[@ANCHOR: find_next_root]`)*
- `lpc_to_lsp` finds the ten roots in turn, alternating between `P` and `Q`, each search starting where the
  last root was found, and returns their arccosines; `None` if fewer than ten roots are found.
  *(Reference: `[@ANCHOR: lpc_to_lsp]`)*
- `lpc_energy` is the prediction-error energy `sum ak[i] * R[i]`; it has to be taken before bandwidth
  expansion or it can come out negative.
  *(Reference: `[@ANCHOR: lpc_energy]`)*

## Pitch estimation

The pitch estimator squares the speech, removes DC, low-pass filters and decimates by 5, windows, transforms
and searches the spectrum for the strongest bin in the 50 to 400 Hz range, then corrects octave errors.

- `NlpState::default` (float reference) sets up the estimator state: empty squared history, zero filter
  memory, a previous pitch of 100 Hz, and a planned 512-point FFT.
  *(Reference: `src/codec2_3200/floating_reference/nlp.rs` -> `[@ANCHOR: NlpState::default]`)*
- `decimate` (float) low-pass filters the squared signal with the 25-tap filter and keeps every fifth
  sample, replicating the buffer edge at the borders.
  *(Reference: `[@ANCHOR: decimate]`)*
- `correct_sub_multiples` (float) walks down through integer sub-multiples of the strongest bin and prefers
  one that is a local peak and above 0.3 of the global peak (half that when it is near the previous frame's
  pitch).
  *(Reference: `[@ANCHOR: correct_sub_multiples]`)*
- `dc_notch` (float) is the first-order DC blocker `y = x - x_prev + 0.95 * y_prev`.
  *(Reference: `[@ANCHOR: dc_notch]`)*
- `nlp` (float) is the whole estimator for the newest 80 samples: square, notch, add 1, decimate, window,
  FFT, power, peak search, sub-multiple correction; it returns the pitch in Hz and remembers it.
  *(Reference: `[@ANCHOR: nlp]`)*
- `lowpass_coeffs` returns the cached 25-tap Hann-windowed sinc low-pass (cut-off 0.1 cycles per sample).
  *(Reference: `src/codec2_3200/nlp.rs` -> `[@ANCHOR: lowpass_coeffs]`)*
- `decimate_fixed` is the integer decimation, with interior fast paths that fold the symmetric taps and give
  bit-identical results to the clamped general form.
  *(Reference: `[@ANCHOR: decimate_fixed]`)*
- `nlp:fft_fixed` runs the estimator's 512-point transform on its 64 non-zero decimated inputs through the
  shared size-specialised kernel; only the magnitude is used afterwards.
  *(Reference: `[@ANCHOR: nlp:fft_fixed]`)*
- `correct_sub_multiples_fixed` is the integer sub-multiple correction on the 128-bit power spectrum, with
  0.8 and 1.2 as exact ratios 8/10 and 12/10 and the threshold kept in 128 bits.
  *(Reference: `[@ANCHOR: correct_sub_multiples_fixed]`)*
- `nlp_fixed` returns the integer estimator's pitch in Hz as an `f32` (bin times 3.125 Hz) for tests and the
  other codec modes.
  *(Reference: `[@ANCHOR: nlp_fixed]`)*

## Voicing decision

- `is_voiced` (float) judges a 10 ms block voiced when its energy is more than 12 dB above a tracked noise
  floor and its zero-crossing rate is below 0.15; the noise floor (starting at -20 dB) is updated only
  during unvoiced blocks.
  *(Reference: `src/codec2_3200/floating_reference/voicing.rs` -> `[@ANCHOR: is_voiced]`)*
- `is_voiced_fixed` is the integer decision without logarithms: the dB margin becomes a linear energy ratio
  (about 15.85), the zero-crossing test is a cross-multiplication, and an empty block is judged unvoiced.
  *(Reference: `src/codec2_3200/voicing.rs` -> `[@ANCHOR: is_voiced_fixed]`)*

## Codec2 3200 quantisers

`src/codec2_3200/quantise.rs` and `floating_reference/quantise.rs`.

- `encode_wo` quantises a pitch (`Wo` in radians) to a 7-bit index over `2*pi/160` to `2*pi/20`.
  *(Reference: `[@ANCHOR: encode_wo]`)*
- `decode_wo` maps an index back to `min + step * index`.
  *(Reference: `[@ANCHOR: decode_wo]`)*
- `decode_wo_fixed` is the same in Q23, `w0_min + w0_step * index` with no rescale.
  *(Reference: `[@ANCHOR: decode_wo_fixed]`)*
- `encode_energy` turns a linear frame energy (floored at 1e-12) into dB with the log table and quantises it
  to a 5-bit index over -10 to 40 dB.
  *(Reference: `[@ANCHOR: encode_energy]`)*
- `decode_energy` is the inverse, using the exp2 table.
  *(Reference: `[@ANCHOR: decode_energy]`)*
- `decode_energy_fixed` computes the exponent `y` from the index in Q23 and returns `exp2_q23(y)`.
  *(Reference: `[@ANCHOR: decode_energy_fixed]`)*
- `quantize_linear` is the shared uniform quantiser: `levels * (v - min) / (max - min) + 0.5`, truncated and
  clamped to `0..levels`.
  *(Reference: `[@ANCHOR: quantize_linear]`)*
- `dequantize_linear` returns `min + step * index` with `step = (max - min) / levels`.
  *(Reference: `[@ANCHOR: dequantize_linear]`)*
- `encode_lsps_delta_scalar` (float) quantises ten LSPs as indexes of 5 bits: the first as an absolute
  frequency, each later one as the delta from the previous reconstructed value.
  *(Reference: `[@ANCHOR: encode_lsps_delta_scalar]`)*
- `lsp_dim_nearest_level` (float) picks the closest of the 32 levels of one dimension, lowest index winning
  an exact tie.
  *(Reference: `[@ANCHOR: lsp_dim_nearest_level]`)*
- `decode_lsps_delta_scalar` accumulates the dequantised deltas in Hz and converts each to radians.
  *(Reference: `[@ANCHOR: decode_lsps_delta_scalar]`)*
- `lsp_dim_value_hz_q16` is the frequency in Q16 Hz of one level of one dimension (uniform steps up to the
  breakpoint, a second step size after it).
  *(Reference: `[@ANCHOR: lsp_dim_value_hz_q16]`)*
- `lsp_dim_nearest_level_q16` searches the 32 levels by Q16 distance, keeping the first level on a tie.
  *(Reference: `[@ANCHOR: lsp_dim_nearest_level_q16]`)*
- `encode_lsps_delta_scalar_fixed` converts float radians to Q23 and delegates to the integer encoder
  (`encode_lsps_delta_scalar_q23`).
  *(Reference: `[@ANCHOR: encode_lsps_delta_scalar_fixed]`)*
- `decode_lsps_delta_scalar_fixed` accumulates Q16 Hz and converts to Q23 radians once per dimension.
  *(Reference: `[@ANCHOR: decode_lsps_delta_scalar_fixed]`)*

## Decoder interpolation

`src/codec2_3200/interp.rs`: the pitch, energy and LSPs arrive per 20 ms but synthesis runs per 10 ms.

- `interp_wo` gives the interpolated half-frame its pitch. If the interpolated half is flagged voiced but both
  neighbours are unvoiced it is treated as unvoiced (the minimum pitch); otherwise it takes the midpoint of
  two voiced neighbours or whichever neighbour is voiced.
  *(Reference: `[@ANCHOR: interp_wo]`)*
- `interp_energy` is the geometric mean `sqrt(prev * next)`.
  *(Reference: `[@ANCHOR: interp_energy]`)*
- `interp_wo_fixed` is `interp_wo` in Q23; the midpoint is `(prev + next) >> 1`.
  *(Reference: `[@ANCHOR: interp_wo_fixed]`)*
- `interp_energy_fixed` computes the geometric mean as `exp2((log2 a + log2 b) / 2)` with the Q23 primitives;
  both inputs must be positive.
  *(Reference: `[@ANCHOR: interp_energy_fixed]`)*

## Spectral bridge numerics

- `extrapolate_amplitudes` fits `ln(a)` against harmonic frequency over the top `min(l, 16)` decoded
  harmonics by least squares, clamps a rising slope to flat, and continues the line out to `min(2l, 160)`
  harmonics; nothing is added unless the bridge is enabled and the sub-frame is voiced.
  *(Reference: `src/codec2_3200/spectral_bridge.rs` -> `[@ANCHOR: extrapolate_amplitudes]`)*
- `freq_hz_fixed` is the frequency of harmonic `m` in whole Hz, `m * wo * 4000 / pi`, rounded; keeping `x` a
  plain integer keeps the regression sums inside `i64`.
  *(Reference: `[@ANCHOR: freq_hz_fixed]`)*
- `spectral_bridge_div_round_i128` divides an `i128` by a positive divisor rounding to nearest, ties away from
  zero; it serves the two divisions of the fit (the lpc module has its own copy under another anchor).
  *(Reference: `[@ANCHOR: spectral_bridge_div_round_i128]`)*
- `extrapolate_amplitudes_fixed` is the same fit in base-2 logarithms: slope and intercept in Q23 log2
  amplitude, a non-rising clamp, and `exp2_q23` for each new harmonic.
  *(Reference: `[@ANCHOR: extrapolate_amplitudes_fixed]`)*

### The envelope-driven high band (`Decoder::decode_16k`, 2026-11-07)

`codec2_3200::Decoder::decode_16k` extends the band above 4 kHz from the decoded spectral envelope, for voiced and
unvoiced sub-frames alike. The 1600 bit/s decoder and the fixed-point path keep `extrapolate_amplitudes` (voiced only,
slope fitted to the harmonic amplitudes); the fixed-point 3200 decoder has its own port (below). Measured on eleven real 16 kHz
voice recordings (FreeDV project samples), each downsampled to 8 kHz, encoded with `EncoderMbe` and decoded at 16 kHz
(4474 active 20 ms frames, energy of the 4-8 kHz band against the original; hams_com
`docs/reviews/M17_CODEC2_WOJCIECH_2026-11-06.md`, section 4):

| | Unvoiced frames: band energy error, log spectral distance | Voiced frames: same |
|---|---|---|
| Bridge off | -25.0 dB, 48.8 dB | -24.2 dB, 36.2 dB |
| `extrapolate_amplitudes` | -22.6 dB, 42.6 dB | -10.1 dB, 18.3 dB |
| Envelope-driven (this) | -5.5 dB, 16.3 dB | -5.0 dB, 16.2 dB |

The remaining 5 dB shortfall is not tuned away here (it would be fitted on the same recordings it is measured on).

#### `envelope_slope_ln_per_hz` [@ANCHOR: envelope_slope_ln_per_hz]

**Purpose.** The slope of the decoded envelope `1/|A(w)|` across 2 to 4 kHz: the guess for how the spectrum continues
above 4 kHz.

```rust
pub fn envelope_slope_ln_per_hz(aw: &[Complex32]) -> f32
```

| Argument | Type | Meaning |
|---|---|---|
| `aw` | `&[Complex32]` | The LPC analysis filter's spectrum `A(w)`, bins of 15.625 Hz from 0 Hz (`envelope::compute_harmonic_amplitudes` returns 257 of them). Borrowed. |

**Returns.** Natural-log amplitude per Hz: the least-squares slope of `ln(1/|A|)` over the bins from 2000 Hz to the
last, clamped to at most 0 (a rising envelope continues flat). 0 when fewer than two bins lie above 2 kHz or the fit is
degenerate. **Special concerns.** Pure, no allocation, `std` only. About 129 bins of arithmetic.

#### `extrapolate_amplitudes_from_envelope` [@ANCHOR: extrapolate_amplitudes_from_envelope]

**Purpose.** The amplitudes of the harmonics added above 4 kHz.

```rust
pub fn extrapolate_amplitudes_from_envelope(model: &Model, slope_ln_per_hz: f32, enabled: bool) -> ([f32; MAX_AMP_SB + 1], usize)
```

| Argument | Type | Meaning |
|---|---|---|
| `model` | `&Model` | The decoded sub-frame: `wo`, `l`, `a[1..=l]`, `voiced` (either). |
| `slope_ln_per_hz` | `f32` | From `envelope_slope_ln_per_hz`; a positive value is treated as 0. |
| `enabled` | `bool` | `SpectralBridgeState::enabled`. |

**Returns.** `(a_ext, l2)`: `a_ext[1..=l]` is `model.a`, `a_ext[l+1..=l2]` continues `ln a` from the mean log amplitude
of the top `min(l, 16)` harmonics, at their mean frequency, along the slope; `l2 = min(2l, 160)`, or `l` when disabled.
Never above the anchor level. **Special concerns.** Pure, `std` only.

#### `SpectralBridgeState::synthesize_subframe_sb_from_envelope` [@ANCHOR: SpectralBridgeState::synthesize_subframe_sb_from_envelope]

**Purpose.** One 10 ms sub-frame at 16 kHz with the envelope-driven high band; `Decoder::decode_16k` calls it per
sub-frame.

```rust
pub fn synthesize_subframe_sb_from_envelope(&mut self, model: &Model, aw: &[Complex32]) -> [i16; N_SAMP_SB]
```

| Argument | Type | Meaning |
|---|---|---|
| `model` | `&Model` | The base decode's sub-frame, after its own synthesis set `phi[1..=l]` (reused unchanged). |
| `aw` | `&[Complex32]` | That sub-frame's LPC spectrum. |

**Returns.** 160 samples at 16 kHz. Harmonics `1..=l` are the base decode's; added voiced harmonics follow the voiced
excitation phase, added unvoiced ones take a random phase from the bridge's own generator (the 8 kHz output's random
sequence is untouched). With `enabled` false it gives exactly `synthesize_subframe_sb`'s output (tested).
**Special concerns.** Mutates the bridge's overlap-add buffer, phase track and generator; one 1024-point inverse FFT.

#### `SpectralBridgeState::synthesize_extended` [@ANCHOR: SpectralBridgeState::synthesize_extended]

Private. The synthesis both entry points share: places harmonics `1..=l2` in the 1024-point spectrum (decoded ones as
`phi * a`, added ones voiced or noise phase as above), mirrors it, inverse-transforms, windows, overlap-adds and applies
the output limiter. `a_ext` must hold `l2` amplitudes; `l2` at most 160.

### The envelope-driven high band, fixed point (`DecoderFixed::decode_16k_fixed`, 2026-11-07)

The integer port of the above, so the 16 kHz decode needs no floating point either (`no_std` with feature
`codec2_16k_bridge`). Same method in base-2 logarithms. Against the float decode of the same bitstreams (the four
OSR sentences encoded with `EncoderMbe`), the energy above 4.5 kHz differs by -0.06 to +0.02 dB on average and 0.16
to 0.32 dB frame by frame (`tests/codec2_3200_16k_fixed_vs_float.rs`); the unvoiced noise phases come from different
generators, so the waveforms are not compared. The 1600 bit/s fixed decoder keeps `synthesize_subframe_sb_fixed`.

#### `envelope_slope_log2_per_bin_fixed` [@ANCHOR: envelope_slope_log2_per_bin_fixed]

**Purpose.** The integer `envelope_slope_ln_per_hz`.

```rust
pub(crate) fn envelope_slope_log2_per_bin_fixed(ak_q23: &[i64; LPC_ORD + 1], scratch: &mut FftScratch) -> i64
```

| Argument | Type | Meaning |
|---|---|---|
| `ak_q23` | `&[i64; 11]` | The sub-frame's LPC coefficients, Q23 (`lpc::lsp_to_lpc_fixed`). |
| `scratch` | `&mut FftScratch` | Work buffers for the 512-point transform; overwritten. |

**Returns.** The least-squares slope of `log2(1/|A|)` (computed as `-log2_q23(|A|^2 + 1e-6)/2`) against the bin index
over bins 128 to 256 (2 to 4 kHz, 15.625 Hz each), as Q23 log2 amplitude per bin, clamped to at most 0; 0 for a
degenerate fit. Sums in `i128`, so no overflow for any coefficients. **Special concerns.** Integer only; feature
`codec2_16k_bridge`; one fixed-point FFT, the cost the 16 kHz path adds per sub-frame. Matches the float slope within
0.025 dB/kHz on 400 decoded envelopes (test: `the_fixed_envelope_slope_matches_the_float_one`).

#### `extrapolate_amplitudes_from_envelope_fixed` [@ANCHOR: extrapolate_amplitudes_from_envelope_fixed]

```rust
pub(crate) fn extrapolate_amplitudes_from_envelope_fixed(model: &ModelFixed, slope_q23_per_bin: i64, enabled: bool) -> ([i64; MAX_AMP_SB + 1], usize)
```

| Argument | Type | Meaning |
|---|---|---|
| `model` | `&ModelFixed` | The decoded sub-frame: `k_q23` (bins per harmonic, Q23), `l`, `a[1..=l]` (Q23), voiced or not. |
| `slope_q23_per_bin` | `i64` | From `envelope_slope_log2_per_bin_fixed`; a positive value is treated as 0. |
| `enabled` | `bool` | `SpectralBridgeStateFixed::enabled`. |

**Returns.** `(a_ext, l2)`: `a_ext[1..=l]` copied, `a_ext[l+1..=l2]` = `exp2_q23(y0 + slope * (m * k_q23 - x0))`, where
`y0` and `x0` are the mean `log2_q23` amplitude and mean bin position of the top `min(l, 16)` harmonics; `l2 =
min(2l, 160)`, or `l` when disabled or `l` is 0. Integer only. Within 0.0001 dB of the float version (test:
`the_fixed_envelope_extrapolation_matches_the_float_one`).

#### `SpectralBridgeStateFixed::synthesize_subframe_sb_from_envelope_fixed` [@ANCHOR: SpectralBridgeStateFixed::synthesize_subframe_sb_from_envelope_fixed]

```rust
pub(crate) fn synthesize_subframe_sb_from_envelope_fixed(&mut self, model: &ModelFixed, slope_q23_per_bin: i64) -> [i16; N_SAMP_SB]
```

One 10 ms sub-frame at 16 kHz with the envelope-driven band above 4 kHz; `DecoderFixed::decode_16k_fixed` calls it per
sub-frame with the slope of that sub-frame's coefficients. `model` must have been through the base synthesis's
preparation (`prepare_subframe_fixed`), which sets `phi[1..=l]`. With `enabled` false the output equals
`synthesize_subframe_sb_fixed`'s (tested). Mutates the bridge's overlap-add buffer, phase track and its own random
generator (`rng`, seeded `0x5B1D6E`), never the base synthesis's.

#### `SpectralBridgeStateFixed::synthesize_extended_fixed` [@ANCHOR: SpectralBridgeStateFixed::synthesize_extended_fixed]

Private. The synthesis both fixed entry points share: decoded harmonics as `phi * a`, added voiced ones with the voiced
excitation phase (`phi0 * m`), added unvoiced ones with a random phase (`synthesis::next_rand_fixed`), one 1024-point
sparse inverse transform, Parzen window and overlap-add. `a_ext` must hold `l2` amplitudes, `l2` at most 160.

## Synthesis internals

`src/codec2_3200/synthesis.rs`.

- `make_synthesis_window` builds the overlap-add window: a trapezoid ramping up over the 80 samples around
  sample 40 and down around sample 120, which for the codec's constants is a triangle peaking at sample 80.
  *(Reference: `[@ANCHOR: make_synthesis_window]`)*
- `next_rand` advances a xorshift generator and returns a uniform angle in `[0, 2*pi)`; it need not match
  the reference generator because it only randomises phase.
  *(Reference: `[@ANCHOR: next_rand]`)*
- `next_rand_fixed` is the same state update returning the raw 32-bit word as a binary angle.
  *(Reference: `[@ANCHOR: next_rand_fixed]`)*
- `phase_increment_q32` converts `wo * 80` (the pitch phase advance per sub-frame) into a 32-bit turn
  fraction by dividing by `2*pi` in Q23 with rounding and discarding whole turns.
  *(Reference: `[@ANCHOR: phase_increment_q32]`)*
- `postfilter_step` is the decision logic of the voiced postfilter: it computes the mean harmonic energy in
  dB; on an unvoiced frame below 40 dB it updates the background estimate (beta 0.1), and on a voiced frame
  it flags harmonics quieter than the background plus 6 dB.
  *(Reference: `[@ANCHOR: postfilter_step]`)*
- `postfilter_step_fixed` is the Q23 version (`l` is clamped to at least 1 so the division cannot trap).
  *(Reference: `[@ANCHOR: postfilter_step_fixed]`)*

## Analysis windows

- `make_analysis_window` builds the 320-sample Hann window over the central 279 samples, scaled by `1 / sqrt(sum(w^2) * 512)`, the
  normalisation the LPC analysis expects.
  *(Reference: `src/codec2_3200/window.rs` -> `[@ANCHOR: make_analysis_window]`)*
- `make_analysis_window_fixed` returns the same window as the committed Q30 table.
  *(Reference: `[@ANCHOR: make_analysis_window_fixed]`)*

### `analysis_span` [@ANCHOR: analysis_span]

**Purpose.** Where the analysis window is non-zero, so the encoders window and correlate only that span.

```rust
pub const ANALYSIS_SPAN: core::ops::Range<usize> = 21..300; // M_PITCH/2 - NW/2 .. + NW
```

**Meaning.** Samples 21 to 299 of the 320-sample history. Outside it both windows (`make_analysis_window`, the Q30
table) are exactly zero (tested), so every windowed sample there is zero and adds nothing to the autocorrelation:
restricting `EncoderFixed::lpc_fields` and `floating_reference::Encoder::encode` to it is bit-exact (each skipped term
is an exact zero; in `f32` adding a signed zero changes no sum) and saves about 13 percent of the multiply-accumulates.
The golden hashes in `tests/codec2_3200_encoder_golden_hash.rs` were measured before the change and still hold.
Codec2-mod made the same change in September 2026 (`W_START`/`W_END`); this is our own implementation of the idea.
Used by: `EncoderFixed::lpc_fields`, `floating_reference::Encoder::encode`.

## Codec2 3200 LPC analysis (`src/codec2_3200/lpc.rs`)

The 3200 bit/s encoder turns each windowed speech frame into ten line spectral frequencies (LSPs) in four steps:
autocorrelation, Levinson-Durbin to get linear prediction coefficients (LPCs), bandwidth expansion, and LPC-to-LSP
conversion. The decoder goes the other way with `lsp_to_lpc`. The integer path works in Q8.23 (`COEF_FRAC_BITS` = 23)
everywhere except inside the Levinson recursion, which uses Q8.40.

### Autocorrelation and white noise correction

`[@ANCHOR: autocorrelate_fixed]` computes the eleven autocorrelation lags `R[0..=10]` of the windowed frame
`wn_q` (Q8.23, `i32`). For each lag `j` it sums `wn[i] * wn[i+j]` over the `len - j` overlapping samples; each product is
formed in `i64` and shifted right by 23 *before* it is added (an arithmetic shift, so it floors), which keeps `R` in Q8.23
and the running sum far inside `i64`. A frame shorter than `LPC_ORD` samples would underflow the loop bound, so callers
always pass the full 320-sample frame.

`[@ANCHOR: apply_white_noise_correction_fixed]` implements the standard "white noise correction" (Rabiner and Schafer,
1978): it replaces `R[0]` by `R[0] + R[0]/1000` (a 0.1 % noise floor, about 30 dB below the signal) and leaves
`R[1..]` alone. The division is exact integer division by the literal 1000, rounded to nearest, half away from zero. It
bounds how small the Levinson prediction error can become on very well predicted frames, which is what makes the
recursion's `1/e` division fragile. An all-zero `R` stays all-zero. The caller must apply it before the integer Levinson
entry points.

### Levinson-Durbin recursion

`[@ANCHOR: levinson_durbin_fixed]` is the historical `f32`-in entry point: it runs `levinson_durbin_fixed_core` on an
`f32` autocorrelation and converts the Q8.23 result back to `f32` coefficients (`a[0]` is 1.0). It debug-asserts
`R[0] > 0`. The production encoder uses the integer-input entry points in the same file instead; this one is kept as the
bridge to the floating reference and is what the divergence tests compare against `levinson_durbin`.

`[@ANCHOR: levinson_durbin_fixed_core]` normalizes the `f32` autocorrelation by `R[0]` (`r[j]/r[0]`, quantized to Q8.40
with `f32_to_q64`), then calls the shared recursion. It returns the Q8.23 coefficients and, per iteration, whether the
`|k| > 1` safety clamp fired; the tests use that flag to tell expected clamp-boundary divergence from real arithmetic bugs.

`[@ANCHOR: levinson_durbin_fixed_from_integer_r]` is the production entry point for a caller that already holds the autocorrelation as
integers (the Q8.23 output of `autocorrelate_fixed`, after white-noise correction, which the caller must have applied). It
normalizes with `r0_normalize_fixed`, runs the shared recursion, and returns both the `f32` coefficients (the established output
boundary) and the integer Q8.23 coefficients, so a caller that wants to stay in fixed point (`EncoderFixed` feeds them straight
into `lpc_energy_fixed`, `apply_bw_gamma_fixed` and `lpc_to_lsp_from_integer_ak`) simply uses the second element. Its integer-only
sibling `levinson_durbin_q23_from_integer_r` returns just the Q8.23 coefficients and builds without floating point.

`[@ANCHOR: levinson_durbin_fixed_core_from_r_norm]` is the recursion proper, on an already normalized `r_norm_q`
(Q8.40, `r_norm[0]` taken as exactly 1.0 and never read). For `i = 1..=10` it forms the numerator
`r[i] + sum(a_prev[j] * r[i-j])`, the reflection coefficient `k = -numerator / e` (rounded integer division), snaps `k`
to zero when `|k| > 1.0` (the same clamp as the float reference), updates `a[j] = a_prev[j] + k * a_prev[i-j]` and
`e *= (1 - k^2)`. If `e` has reached exactly zero (a previous `k` of exactly +/-1) or the numerator is zero, `k` is set to
zero instead of dividing, so the function never traps on a division by zero. The Q8.40 result is rounded to Q8.23 at the end.

`[@ANCHOR: r0_normalize_fixed]` turns an integer autocorrelation `r_q` (any Q format) into `r_norm_q[j] = R[j] * 2^40 /
R[0]` with a rounded 128-bit division. If `R[0] <= 0` (a silent frame) it returns `[1.0, 0, 0, ...]` instead of dividing,
which makes the recursion return the identity filter `a = [1, 0, ..., 0]`. The caller must have applied white noise
correction first; the validation of this path assumes it.

`[@ANCHOR: rshift_round]` is `x >> n` rounded to nearest (add half, then shift). Plain `>>` floors and its bias, repeated
over ten iterations, produced a measurable coefficient error on the worst corpus frame. `n = 0` returns `x`. The
Levinson core uses it for the final Q8.40 to Q8.23 conversion.

`[@ANCHOR: q_mul]` multiplies two Q8.40 numbers: the 128-bit product plus half a unit, shifted right by 40, narrowed to
`i64` (debug-asserted not to overflow). `div_round_i128` (described in the codec2_3200 story) is its division counterpart.

### Q-format conversion helpers

`[@ANCHOR: dequantize_coef_q23]` converts Q8.23 coefficients to `f32` by dividing by 2^23, for callers that need the
float boundary without access to the crate-private `COEF_FRAC_BITS`.

`[@ANCHOR: f32_to_q]` converts an `f32` to a signed fixed-point `i32` with the given number of fractional bits: multiply
by 2^bits in `f64`, round half away from zero, cast (the cast saturates, so formats wider than 31 bits cannot be held).
`[@ANCHOR: f32_to_q64]` is the same conversion with an `i64` result, needed for the Q8.40 format of the Levinson recursion;
NaN converts to 0, which the silent-frame path relies on.

### LPC to LSP conversion

The ten LSPs are the roots, in `x = cos(omega)`, of the symmetric and antisymmetric polynomials `P` and `Q` built from
the coefficients (Sugamura and Itakura 1981). They interleave, so the search takes the roots alternately from `P` and `Q`,
each time starting just below the previous root.

`[@ANCHOR: build_p_q_fixed]` builds the six Chebyshev-basis coefficients of `P` and `Q` from Q8.23 `a_q23`: with
`p[0] = q[0] = 1`, `p[i] = a[i] + a[11-i] - p[i-1]` and `q[i] = a[i] - a[11-i] + q[i-1]` for `i = 1..=5`, then the first
five entries of each doubled. It is pure addition, so no precision is lost; results are narrowed to `i32` (the
coefficients stay under 128 in magnitude, which is why Q8.23 fits).

`[@ANCHOR: cheb_poly_eval_fixed_core]` evaluates `sum coef[5-i] * T_i(x)` for a real `x` given as `f32` against Q8.23
coefficients, by quantizing `x` to Q2.29 and running the Chebyshev three-term recurrence in the integer helper. Only the
sign of the result is used by the root search. It exists so the coefficients are quantized once per search rather than per
candidate `x`.

`[@ANCHOR: find_next_root_from_q23]` walks `x` downward from `x_start` in steps of 0.01 until the polynomial changes
sign between two neighbours, then bisects that bracket six times and returns the midpoint of the last bisection (about
0.00016 accurate). It returns `None` if no sign change is found before `x < -1`. It is `pub(crate)` because the float
reference's own `find_next_root` calls it.

`[@ANCHOR: lpc_to_lsp_from_integer_ak]` converts Q8.23 coefficients to ten LSP frequencies in radians (`f32`, ascending
from the first root near `x = 1`). It builds `P`/`Q`, finds each root with `find_next_root_from_q23` and converts it with
`acos_lut_fixed`; it returns `None` if any of the ten roots is not found. The all-integer form of the same step is
`lpc_to_lsp_q23_from_integer_ak`.

`[@ANCHOR: acos_lut_table_q23]` returns the 4097-entry table of `acos(i/4096)` in Q23 (generated into
`tables.rs`, entries within one unit of the exact value, with `acos(0) = pi/2` and `acos(1) = 0`), the input of the linear
interpolation in `acos_lut_q23`. `[@ANCHOR: pi_q23]` is pi in Q23 (26353590, the `f32` value of pi quantized); it is the
reflection constant for negative arguments in `acos_lut_q23` and the upper limit of `cos_q23`'s angle domain.

`[@ANCHOR: acos_lut_fixed]` is the `f32` wrapper of the integer `acos`: it quantizes `|x|` to Q23 (clamped to 1.0), restores
the sign, calls `acos_lut_q23` and converts the Q23 radians back to `f32`. `x` must lie in `[-1, 1]` (debug-asserted).
Worst-case error over the whole domain is about 7 Hz of an LSP at 8 kHz sampling, below the 25 Hz quantizer step.

### LSP to LPC conversion (decoder side)

`[@ANCHOR: lsp_to_lpc]` rebuilds `f32` LPC coefficients from ten LSP angles: `P(z)` is the product of
`1 - 2cos(w_i) z^-1 + z^-2` over the even-indexed LSPs times `(1 + z^-1)`, `Q(z)` the same over the odd-indexed ones times
`(1 - z^-1)`, and `a = (P + Q) / 2` truncated to eleven terms. The two helpers `[@ANCHOR: build_half_poly]` (the cascade of
quadratic factors and the boundary factor for one of `P`/`Q`, in `f32`, returning the buffer and its length) and
`[@ANCHOR: poly_mul_fixed]` (plain convolution of two ascending-power polynomials into a fixed-size buffer, returning the
product length; "fixed" refers to the buffer size, not the arithmetic) do the work without heap allocation.

The Q23 decoder path (`lsp_to_lpc_fixed`, used by the fixed `Decoder`) mirrors this in integers.
`[@ANCHOR: lsp_to_lpc_fixed]` computes `cos` of each Q23 angle with `cos_q23`, builds `P` and `Q` with
`build_half_poly_q23`, and returns `(p + q) >> 1` for eleven Q8.23 coefficients.
`[@ANCHOR: cos_q23]` takes an angle in Q23 radians, clamps it to `[0, pi]`, divides by pi (one 128-bit integer division) to
get a position in the 4097-entry table, and linearly interpolates. `[@ANCHOR: cos_lut_table_q23]` is that table:
`cos(pi * i/4096)` in Q23 (generated in `f32`, within two units of exact; exactly 1, 0 and -1 at the start, middle and end).
`[@ANCHOR: build_half_poly_q23]` is the Q8.23 twin of `build_half_poly` (the quadratic factor is `[1, -2cos, 1]` in Q23,
the boundary factor `[1, +-1]`), and `[@ANCHOR: poly_mul_q23]` the Q8.23 twin of `poly_mul_fixed`: each product is formed in
`i128` and shifted right by 23 before it is accumulated.

### Frame energy and bandwidth expansion

`[@ANCHOR: lpc_energy_fixed]` is the `f32` boundary of `lpc_energy_q23`: the sum `a[i] * R[i]` for `i = 0..=10` (the
prediction-error energy that the encoder quantizes) computed in Q23, converted to `f32` by dividing by 2^23.

`[@ANCHOR: apply_bw_gamma_fixed]` multiplies coefficient `a[i]` in place by `0.994^i` (the `BW_GAMMA_Q23` table, Q8.23),
shifting right by 23 (floor). It widens the LPC spectral peaks' bandwidth slightly and must be applied after the energy
is computed, because the energy is defined on the un-expanded coefficients.

## AMBE fixed-point arithmetic primitives (`src/ambe/fixed/general/fixed_ops.rs`, `explog.rs`, `trig.rs`)

These are the integer-only building blocks of the fixed-point AMBE chain. **Q16.16** means a signed `i32` whose low 16 bits are
fractional (`real = value / 65536`). A **phase** is a wrapping `u32` where a full turn is `1 << 32`, so a quarter turn is `1 << 30`
and wrap-around is the modulo-2*pi reduction. Wide values are an `i64` with the same 16 fractional bits. None of these functions
blocks, allocates, takes a lock or panics for any input; they are pure and safe on any thread. Their tests live in
`tests/ambe_fixed_general.rs` (an integration test, so the `f64` reference values never appear inside `src/ambe/fixed`).

| Function | Anchor | One line |
|---|---|---|
| `mul_q16` | `ambe_fixed_mul_q16` | Q16.16 product, rounded |
| `div_q16` | `ambe_fixed_div_q16` | Q16.16 quotient, truncated, saturating |
| `mul_q16_i64` | `ambe_fixed_mul_q16_i64` | Wide value times a Q16.16 scalar |
| `div_q16_i64` | `ambe_fixed_div_q16_i64` | Q16.16 ratio of two wide values |
| `sqrt_q16` | `ambe_fixed_sqrt_q16` | Square root of a Q16.16 value |
| `sqrt_wide_q16` | `ambe_fixed_sqrt_wide_q16` | Square root of a wide value, wide result |
| `log2_q16` | `ambe_fixed_log2_q16` | Base-2 logarithm by table and interpolation |
| `log2_q16_i64` | `ambe_fixed_log2_q16_i64` | The same for a wide positive value |
| `exp2_q16` | `ambe_fixed_exp2_q16` | `2^y` by table and interpolation |
| `quarter_sine_interp` | `ambe_fixed_quarter_sine_interp` | Interpolated quarter-wave sine table |
| `sin_q16` | `ambe_fixed_sin_q16` | Sine of a wrapping phase |
| `cos_pi_frac` | `ambe_fixed_cos_pi_frac` | `cos(pi * a * b / c)` for the DCT shapes |
| `radians_from_phase_q16` | `ambe_fixed_radians_from_phase_q16` | Phase to bounded Q16.16 radians |
| `radians_q32_from_phase` | `ambe_fixed_radians_q32_from_phase` | Phase to bounded Q32 radians |
| `round_div_to_u32` | `ambe_fixed_round_div_to_u32` | Rounded integer division to a phase |

### `ambe_fixed_mul_q16` [@ANCHOR: ambe_fixed_mul_q16]

**Purpose.** Multiply two Q16.16 numbers; used throughout the dequantize and synthesis chain.

```rust
pub fn mul_q16(a: i32, b: i32) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `a`, `b` | `i32` | Q16.16 factors (`real = value / 65536`), passed by value. |

**Returns.** `(a * b) >> 16` with half an LSB added first, computed in `i64` so the product itself never overflows. The final cast to `i32` wraps if the true result is outside `+-32768.0`; callers keep their operands scaled to avoid that.

**Special concerns.** Pure. Known limitation: for a negative product the half is subtracted and the shift floors, so such a result can be one count (1/65536) lower than round-to-nearest; filed as `night_shift_todo/low/ambe-fixed-mul-q16-negative-rounding-bias-*`. The test asserts the one-count bound, not exact negative results.

**See also.** `ambe_fixed_mul_q16_i64`, `ambe_fixed_div_q16`. Test: `mul_q16_rounds_to_nearest_and_tracks_f64`.

### `ambe_fixed_div_q16` [@ANCHOR: ambe_fixed_div_q16]

**Purpose.** Divide two Q16.16 numbers.

```rust
pub fn div_q16(a: i32, b: i32) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `a` | `i32` | Q16.16 numerator. |
| `b` | `i32` | Q16.16 divisor. |

**Returns.** `(a << 16) / b` truncated toward zero, clamped to `i32::MIN..=i32::MAX`. A zero `b` gives `i32::MAX` when `a >= 0` and `i32::MIN` when `a < 0` (the crate's never-panic convention, where `f64` would give an infinity).

**Special concerns.** Pure, no panic. Truncates; it does not round. Test: `div_q16_truncates_toward_zero_and_saturates_on_zero_or_overflow_instead_of_panicking`.

### `ambe_fixed_mul_q16_i64` [@ANCHOR: ambe_fixed_mul_q16_i64]

**Purpose.** Multiply a wide value (for example the enhancement stage's `R_M0` or `S_E`) by a Q16.16 scalar without overflow.

```rust
pub fn mul_q16_i64(a: i64, b_q16: i32) -> i64
```

| Argument | Type | Meaning |
|---|---|---|
| `a` | `i64` | A value with 16 fractional bits, any `i64`. |
| `b_q16` | `i32` | Q16.16 scalar. |

**Returns.** The product with the same 16-fractional-bit convention, from an `i128` intermediate so `a * b` may exceed `i64::MAX`. Rounds the same way as `mul_q16` (same negative one-count bias). The final cast wraps if the result itself exceeds `i64`.

**Special concerns.** Pure, no panic. Test: `mul_q16_i64_keeps_a_product_that_overflows_i64_and_rounds_like_mul_q16`.

### `ambe_fixed_div_q16_i64` [@ANCHOR: ambe_fixed_div_q16_i64]

**Purpose.** A dimensionless Q16.16 ratio of two wide values that individually do not fit `i32`, for example `R_M1 / R_M0` or the final `sqrt(R_M0 / E_enh)` rescale.

```rust
pub fn div_q16_i64(a: i64, b: i64) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `a` | `i64` | Numerator, 16 fractional bits. |
| `b` | `i64` | Divisor, the same convention. |

**Returns.** The Q16.16 ratio `a / b`. Both operands are first shifted right together so the larger has at most 47 significant bits, then divided as in `div_q16`, so `a << 16` cannot overflow. Division by zero, or a divisor that shifts away to zero (the ratio is beyond `i32`), gives `i32::MAX` for a non-negative numerator and `i32::MIN` for a negative one. Other overflow clamps.

**Special concerns.** Pure, no panic. The shift drops low bits of both operands, a relative error below 2^-47. Test: `div_q16_i64_gives_the_ratio_of_wide_values_and_saturates_instead_of_panicking`.

### `ambe_fixed_sqrt_q16` [@ANCHOR: ambe_fixed_sqrt_q16]

**Purpose.** Square root of a Q16.16 value.

```rust
pub fn sqrt_q16(x: i32) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `x` | `i32` | Q16.16 value. |

**Returns.** `isqrt(x * 65536)` as Q16.16: the exact integer square root, so the result is the floor of the true root at the Q16.16 grid. `0` for a negative `x` (where `f64` gives NaN).

**Special concerns.** Pure, no panic, accurate to one count. Test: `sqrt_q16_is_exact_to_the_nearest_q16_value_and_is_zero_for_a_negative`.

### `ambe_fixed_sqrt_wide_q16` [@ANCHOR: ambe_fixed_sqrt_wide_q16]

**Purpose.** Square root of a wide non-negative value whose root also exceeds `i32` (the unvoiced-synthesis windowed-noise power).

```rust
pub fn sqrt_wide_q16(x_q16: i64) -> i64
```

| Argument | Type | Meaning |
|---|---|---|
| `x_q16` | `i64` | Wide value, 16 fractional bits. |

**Returns.** The root on the same wide convention. `0` for zero or negative input. The value `x << 16` is formed in `u128`; if it does not fit `u64`, its lowest `2k` bits are dropped, the exact root taken, and the result shifted left by `k`, a relative error far below 1e-9 at these magnitudes.

**Special concerns.** Pure, no panic for any `i64`. Test: `sqrt_wide_q16_handles_values_far_beyond_i32_and_is_zero_for_zero_or_negative`.

### `ambe_fixed_log2_q16` [@ANCHOR: ambe_fixed_log2_q16]

**Purpose.** Base-2 logarithm of a Q16.16 value, the basis of every fractional power in the chain (`x^p = 2^(p * log2 x)`).

```rust
pub fn log2_q16(x: i32) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `x` | `i32` | Positive Q16.16 value. |

**Returns.** `log2(x / 65536)` as Q16.16 (negative for inputs below 1.0). `i32::MIN` (a stand-in for minus infinity) when `x <= 0`. The leading 1 is located, the 30-bit fraction below it is split into an 8-bit table index (257-entry table in `explog_table`) and 22 bits of linear interpolation.

**Special concerns.** Pure, no panic. Error stays small in absolute terms across the whole range. Tests: `log2_q16_tracks_f64_across_many_decades`, `log2_q16_and_exp2_q16_hit_exact_powers_of_two`.

### `ambe_fixed_log2_q16_i64` [@ANCHOR: ambe_fixed_log2_q16_i64]

**Purpose.** `log2_q16` for a wide positive value whose integer part exceeds `i32` (about 9 to 4e8 for the enhancement stage's `R_M0` and `S_E`).

```rust
pub fn log2_q16_i64(x: i64) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `x` | `i64` | Positive value with 16 fractional bits. |

**Returns.** The same table and interpolation as `log2_q16`, so it returns exactly `log2_q16(x)` for any `x` that fits `i32`. `i32::MIN` for `x <= 0`.

**Special concerns.** Pure, no panic. Test: `log2_q16_i64_matches_log2_q16_where_both_apply_and_reaches_beyond_i32`.

### `ambe_fixed_exp2_q16` [@ANCHOR: ambe_fixed_exp2_q16]

**Purpose.** `2^y`, the inverse of `log2_q16`.

```rust
pub fn exp2_q16(y: i32) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `y` | `i32` | Q16.16 exponent, may be negative. |

**Returns.** `2^(y / 65536)` as Q16.16, from a 257-entry table of `2^f` with linear interpolation, shifted by the integer part of `y`. Saturates to `i32::MAX` when the result would exceed `i32` (an integer part of 15 or more, or an overflow after the shift) and to `0` when the integer part is -63 or lower.

**Special concerns.** Pure, no panic. Relative precision falls for very negative `y` because the Q16.16 grid is fixed (a result near 2^-10 has about 6 significant bits); callers keep `y` above about -8. Tests: `exp2_q16_tracks_f64_within_its_comfortable_precision_range`, `log2_q16_and_exp2_q16_hit_exact_powers_of_two`, `exp2_q16_saturates_instead_of_overflowing_or_panicking`.

### `ambe_fixed_quarter_sine_interp` [@ANCHOR: ambe_fixed_quarter_sine_interp]

**Purpose.** The single table lookup under every sine and cosine; private to `trig`.

```rust
fn quarter_sine_interp(x: u32) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `x` | `u32` | Angle within the first quadrant, `0..=1 << 30` (a phase). |

**Returns.** The sine of that angle as Q16.16 (`0..=65536`): the top 8 bits index the 257-entry quarter-wave table, the low 22 bits interpolate linearly; an index past the end returns the last entry (65536).

**Special concerns.** Pure, no panic. Tested through `sin_q16`: `sin_cos_q16_track_f64_across_a_dense_sweep`, `sin_cos_q16_hit_the_four_cardinal_angles_exactly_at_full_scale`.

### `ambe_fixed_sin_q16` [@ANCHOR: ambe_fixed_sin_q16]

**Purpose.** Sine of a phase, for the voiced-synthesis oscillators and the DCT sums; `cos_q16` is the same shifted by a quarter turn.

```rust
pub fn sin_q16(phase: u32) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `phase` | `u32` | Wrapping phase, `1 << 32` is one turn. |

**Returns.** `sin` as Q16.16, `-65536..=65536`. The two top bits pick the quadrant and the remainder is folded into the one quarter-wave table, so the cardinal angles are exact (0, 65536, 0, -65536). Within 3 counts of `f64::sin` across a dense sweep.

**Special concerns.** Pure, no panic, any thread. Tests: the two named under `ambe_fixed_quarter_sine_interp`, and `sin_q16_never_exceeds_full_scale_magnitude`.

### `ambe_fixed_cos_pi_frac` [@ANCHOR: ambe_fixed_cos_pi_frac]

**Purpose.** `cos(pi * a * b / c)` for the integer-and-half-integer shapes of the 8-point and block DCTs.

```rust
pub fn cos_pi_frac(a: i64, b_times_2: i64, c: i64) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `a` | `i64` | Small non-negative integer. |
| `b_times_2` | `i64` | `2 * b`, odd (so `i - 0.5` is passed as `2 * i - 1`). |
| `c` | `i64` | Small positive integer; zero divides by zero and panics. |

**Returns.** The cosine as Q16.16. The exact integer ratio `a * b_times_2 / (2 * c)` is formed before the single conversion to a phase, so rounding does not compound. Within about 6 counts of `f64` for the DCT ranges (`a` 0..8, `b_times_2` 1..15, `c` 1..8).

**Special concerns.** Pure. The only panic is `c == 0`; the callers pass constants. Test: `cos_pi_frac_tracks_f64_for_the_dct_shapes_and_hits_a_half_turn_exactly`.

### `ambe_fixed_radians_from_phase_q16` [@ANCHOR: ambe_fixed_radians_from_phase_q16]

**Purpose.** Recover a bounded radian value from a wrapping phase accumulator, for the within-frame arithmetic in voiced synthesis.

```rust
pub fn radians_from_phase_q16(phase: u32) -> i32
```

| Argument | Type | Meaning |
|---|---|---|
| `phase` | `u32` | Wrapping phase. |

**Returns.** The angle in `(-pi, pi]` as Q16.16, from the phase read as `i32` and scaled by a 48-bit `pi`, rounded to nearest with ties away from zero. A half turn comes out as `-pi` (about -205887). It inverts `phase_from_radians_q16` within one count.

**Special concerns.** Pure, no panic. Test: `radians_from_phase_q16_inverts_phase_from_radians_q16_within_one_count`.

### `ambe_fixed_radians_q32_from_phase` [@ANCHOR: ambe_fixed_radians_q32_from_phase]

**Purpose.** The same recovery at 32 fractional bits, matching `phase_from_radians_q32` (pitch is carried at that precision).

```rust
pub fn radians_q32_from_phase(phase: u32) -> i64
```

| Argument | Type | Meaning |
|---|---|---|
| `phase` | `u32` | Wrapping phase. |

**Returns.** `(-pi, pi]` as an `i64` with 32 fractional bits, rounded to nearest, ties away from zero. `phase_from_radians_q32(radians_q32_from_phase(p)) == p` for every phase tried.

**Special concerns.** Pure, no panic. Test: `radians_q32_from_phase_recovers_a_bounded_radian_value_and_round_trips_the_phase`.

### `ambe_fixed_round_div_to_u32` [@ANCHOR: ambe_fixed_round_div_to_u32]

**Purpose.** The rounding step shared by `phase_from_radians_q16`, `_q16_i64` and `_q32`; private to `trig`.

```rust
fn round_div_to_u32(numerator: i128, denominator: i128) -> u32
```

| Argument | Type | Meaning |
|---|---|---|
| `numerator` | `i128` | Angle scaled up so one turn is a whole number of `denominator`s. |
| `denominator` | `i128` | The 48-bit `pi`; nonzero. |

**Returns.** `numerator / denominator` rounded to nearest, ties away from zero, then truncated (wrapped) to `u32`, which is the modulo-one-turn reduction.

**Special concerns.** Pure; a zero `denominator` would panic, and the only callers pass the constant. Tested through the phase conversions: `radians_from_phase_q16_inverts_phase_from_radians_q16_within_one_count`, `radians_q32_from_phase_recovers_a_bounded_radian_value_and_round_trips_the_phase`.

## AMBE tone detection and tone synthesis (`src/ambe/{float,fixed/general}/tone_detect.rs`, `tone_synthesis.rs`)

The D-STAR and AMBE+2 encoders recognise a DTMF (touch-tone) digit or one sustained tone in a 20 ms frame (160 samples at 8 kHz) and send a tone frame in place of
a speech frame; the decoders turn a tone frame back into PCM. The float versions are the reference; the fixed versions are integer-only ports that
agree with them (identical decisions on 449 synthetic frames, amplitude within 0.002 percent). Rules matched to the real chip: a DTMF digit has index
`128 + row + 4 * col`; a single tone has index `round(f / 31.25)` for 400 to 3800 Hz and exactly 200 Hz (index 6, a purer fit required), and none at
100, 300 or 3900 Hz; the level field is `186 + 17 * log2(A / 4000)` for each tone's peak amplitude `A`. Everything here is pure and non-blocking;
`ToneSynthesizer` carries phase between calls, so one instance belongs to one stream and is not shared across threads without a lock.

### `ambe_float_tone_fit_sinusoid` [@ANCHOR: ambe_float_tone_fit_sinusoid]

**Purpose.** The amplitude of the best-fit sinusoid at one frequency in a frame (least squares, sine and cosine fitted together, so the phase does not matter); private, the building block of `detect_tone`.

```rust
fn fit_sinusoid(x: &[f64], hz: f64) -> f64
```

| Argument | Type | Meaning |
|---|---|---|
| `x` | `&[f64]` | The frame's samples in PCM units; any length (the detector passes 160). |
| `hz` | `f64` | The test frequency in hertz at the 8 kHz sample rate. |

**Returns.** The fitted peak amplitude, in PCM units. `0.0` when the normal equations are degenerate (determinant below 1e-9: a frame too short to tell sine from cosine).

**Special concerns.** Pure. Cost is O(`x.len()`); `detect_tone` calls it about 8 times for DTMF plus about 300 for the scan. Tested through `detect_tone`: `detects_every_dtmf_digit_with_the_chip_index_and_level`, `single_tone_index_and_levels_match_the_chip`, `chip_non_detections_are_reproduced_and_speech_like_frames_are_rejected`.

### `ambe_float_tone_explained_fraction` [@ANCHOR: ambe_float_tone_explained_fraction]

**Purpose.** How much of a frame's energy sinusoids at the given frequencies account for; separates a real tone from voiced speech whose fundamental dominates a frame.

```rust
fn explained_fraction(x: &[f64], hzs: &[f64]) -> (f64, Vec<f64>)
```

| Argument | Type | Meaning |
|---|---|---|
| `x` | `&[f64]` | Frame samples. |
| `hzs` | `&[f64]` | Frequencies to fit, each independently (adequate for well separated tones). |

**Returns.** `(fraction, amplitudes)`: the sum of `a * a / 2 * len` over the fits divided by the frame's energy, capped at 1.5, and `0.0` for a silent frame; and each fit's amplitude.

**Special concerns.** Pure. The thresholds applied to it are `0.85` for DTMF, `SINGLE_TONE_MIN_EXPLAINED_FRACTION` (0.99) and, for 200 Hz, `LOW_TONE_MIN_EXPLAINED_FRACTION` (0.999). Tests: as for `fit_sinusoid`.

### `ambe_float_tone_detect` [@ANCHOR: ambe_float_tone_detect]

**Purpose.** Decide whether one frame is a DTMF digit or a single tone, as the chip's tone detector does; the encoders call it for every frame.

```rust
pub fn detect_tone(frame: &[f64]) -> Option<Detection> // Detection { tone: DetectedTone, amplitude: f64 }
```

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&[f64]` | Exactly `FRAME` (160) PCM samples. |

**Returns.** `None` for any other length, a frame quieter than a 100-amplitude sine, speech, noise or silence. `Some(Detection)`: `DetectedTone::Dtmf { row, col }` when the strongest row and column tones both exceed 100, are within a factor of 3, and explain over 85 percent of the energy (amplitude: their mean); otherwise `DetectedTone::Single { index, hz }` from a 12.5 Hz coarse scan of 150 to 3900 Hz refined in 1 Hz steps, when the amplitude exceeds 100 and the fit explains over 99 percent, and the index is 13 to 122 or the pure 200 Hz case (index 6, within 3 Hz, over 99.9 percent).

**Special concerns.** Pure, no panic, no state. About 300 frame fits per call, so the cost is a few hundred thousand multiply-adds per frame. Tests: `detects_every_dtmf_digit_with_the_chip_index_and_level`, `single_tone_index_and_levels_match_the_chip`, `chip_non_detections_are_reproduced_and_speech_like_frames_are_rejected`.

### `ambe_fixed_tone_fit_sinusoid` [@ANCHOR: ambe_fixed_tone_fit_sinusoid]

**Purpose.** The fixed-point `ambe_float_tone_fit_sinusoid`: sums `sin`/`cos` products in `i64` and solves the 2x2 normal equations in `i128`; private.

```rust
fn fit_sinusoid(x: &[i16], hz_q16: i32) -> (i64, i64)
```

| Argument | Type | Meaning |
|---|---|---|
| `x` | `&[i16]` | Frame samples. |
| `hz_q16` | `i32` | The test frequency, Q16.16 hertz; the phase step is `hz / 8000` turns as a wrapping `u32`. |

**Returns.** `(amplitude_q16, amplitude_squared_q16)`, Q16.16 in `i64` (two full-scale tones can exceed an `i32` Q16.16). `(0, 0)` for a degenerate determinant. Pure, no panic. Tests: as for `ambe_fixed_tone_detect`.

### `ambe_fixed_tone_volume_for_amplitude` [@ANCHOR: ambe_fixed_tone_volume_for_amplitude]

**Purpose.** The level field of a tone frame for a per-tone amplitude: `round(186 + 17 * log2(A / 4000))`.

```rust
pub fn volume_for_amplitude_q16(amplitude_q16: i64) -> u32
```

| Argument | Type | Meaning |
|---|---|---|
| `amplitude_q16` | `i64` | Per-tone peak amplitude, Q16.16 PCM units; raised to at least 1.0 first. |

**Returns.** The field, clamped to `0..=255` (500 gives 135, 1000 gives 152, 12000 gives 213). Pure, no panic; uses `log2_q16_i64`. Tests: `detects_every_dtmf_digit_with_the_chip_index_and_level`, `single_tone_index_and_levels_match_the_chip`.

### `ambe_fixed_tone_detect` [@ANCHOR: ambe_fixed_tone_detect]

**Purpose.** The fixed-point `ambe_float_tone_detect`, with every threshold decided on squared quantities by cross-multiplication so no division or square root enters a decision.

```rust
pub fn detect_tone(frame: &[i16]) -> Option<Detection> // Detection { tone: DetectedTone, amplitude_q16: i64 }
```

| Argument | Type | Meaning |
|---|---|---|
| `frame` | `&[i16]` | Exactly `FRAME` (160) samples. |

**Returns.** The same decisions as the float detector (see its entry): `None` for a wrong length, total energy below 800000, or anything that is not a tone; `Dtmf { row, col }` with `amplitude_q16` the mean of the two; `Single { index, hz_q16 }` (the measured frequency, Q16.16 Hz) with the 99 percent, and 99.9 percent at 200 Hz, explained-energy tests. Ties for the strongest row or column resolve to the last, like the float's `max_by`.

**Special concerns.** Pure, no panic, no state; same cost as the float version. Tests: `detects_every_dtmf_digit_with_the_chip_index_and_level`, `single_tone_index_and_levels_match_the_chip`, `chip_non_detections_are_reproduced_and_speech_like_frames_are_rejected`; the cross-check against the float detector is `tests/ambe_fixed_tone_detect.rs`.

### `ambe_float_tone_synthesize` [@ANCHOR: ambe_float_tone_synthesize]

**Purpose.** One 20 ms frame of one or two sinusoids, with the phase kept between frames so a held tone is continuous; `dtmf` calls it with a row and a column.

```rust
impl ToneSynthesizer { pub fn synthesize(&mut self, freqs_hz: &[f64], peak: f64) -> [f64; N] }
```

| Argument | Type | Meaning |
|---|---|---|
| `freqs_hz` | `&[f64]` | One or two frequencies in hertz; only the first two are used, slot `i` keeps its own phase. |
| `peak` | `f64` | The amplitude of each tone, PCM units. |

**Returns.** The `N` (160) samples, the sum of the tones. Each slot's phase is reduced modulo 2 pi and stored for the next call; `reset` clears it. Pure apart from that stored phase; no panic. Tests: `single_tone_lands_on_its_frequency_with_requested_peak`, `phase_is_continuous_across_frames`.

### `ambe_fixed_dstar_tone_amplitude` [@ANCHOR: ambe_fixed_dstar_tone_amplitude]

**Purpose.** The per-tone peak amplitude for a D-STAR tone frame's 8-bit volume field: the chip's measured curve `3268 * exp(0.04084 * (volume - 180))`.

```rust
pub fn dstar_tone_amplitude_q16(volume: u32) -> i64
```

| Argument | Type | Meaning |
|---|---|---|
| `volume` | `u32` | The volume field, 0 to 255. |

**Returns.** The peak in Q16.16 PCM units, clamped to 32767 (the chip saturates near volume 238). Evaluated as `3268 * 2^y` with the integer part of `y` applied as a shift, so the extremes do not saturate `exp2_q16`. Within 4 percent of the chip's measured peaks at volumes 120, 150, 180 and 210, and exactly 3268 at 180. Pure, no panic. Test: `dstar_levels_match_the_chip_measurements`.

### `ambe_fixed_dstar_tone_volume_for_amplitude` [@ANCHOR: ambe_fixed_dstar_tone_volume_for_amplitude]

**Purpose.** The inverse of `dstar_tone_amplitude_q16`: the volume field an encoder writes for a detected tone amplitude.

```rust
pub fn dstar_tone_volume_for_amplitude_q16(amplitude_q16: i64) -> u32
```

| Argument | Type | Meaning |
|---|---|---|
| `amplitude_q16` | `i64` | Per-tone peak, Q16.16 PCM units; values below 1.0 are treated as 1.0. |

**Returns.** `round(180 + log2(A / 3268) / slope)` clamped to `0..=255`. Round-trips every volume from 60 to 230 exactly. Pure, no panic. Test: `dstar_tone_volume_inverts_the_amplitude_curve`.

### `ambe_fixed_tone_synthesize` [@ANCHOR: ambe_fixed_tone_synthesize]

**Purpose.** The fixed-point `ambe_float_tone_synthesize`: the phase accumulator is a wrapping `u32` in turns, so continuity across frames costs nothing.

```rust
impl ToneSynthesizer { pub fn synthesize(&mut self, freqs_hz_q16: &[i32], peak_q16: i64) -> [i64; N] }
```

| Argument | Type | Meaning |
|---|---|---|
| `freqs_hz_q16` | `&[i32]` | One or two frequencies in Q16.16 hertz (only the first two are used; a negative one is treated as 0). |
| `peak_q16` | `i64` | Each tone's peak, Q16.16 PCM units. |

**Returns.** The `N` (160) samples in Q16.16 (`i64`), the sum of the tones, each sample rounded to nearest. The stored phases advance by `N` steps. `synthesize_hz` and `dtmf` wrap it. Pure apart from the phase; no panic. Test: `single_tone_has_requested_peak_and_continuous_phase`.
