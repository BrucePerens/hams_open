// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]

//! RTTY (Radio Teletype), pure Rust, no vendored third-party code. Per
//! `docs/proposals/RTTY_DIGITAL_MODE.md`: this codebase already decodes
//! PSK31/FT8/WSPR natively but had no RTTY (Baudot/ITA2 FSK) support at
//! all, a real gap found during a Ham Radio Deluxe feature comparison
//! (HRD's DM-780 decodes RTTY directly). Standard amateur parameters,
//! each verified directly this session rather than assumed:
//!
//! - **45.45 baud, 170Hz shift, MARK = 2125Hz / SPACE = 2295Hz** (audio
//!   tones) -- confirmed directly via a real RTTY tutorial (iw5edi.com):
//!   "The recommended audio frequencies are 2125 Hz for the MARK audio
//!   frequency and 2295 Hz for the SPACE audio frequency," with MARK the
//!   idle/stop condition -- matching w1hkj.org's own independent
//!   confirmation ("the upper tone used for idle condition (MARK)" in
//!   that source's own RF-dial-frequency framing; this module works in
//!   the audio domain, matching this codebase's existing PSK31/FT8/WSPR
//!   convention, where MARK is the lower of the two audio tones).
//! - **1 start bit (SPACE) + 5 data bits (Baudot/ITA2, LSB first) + at
//!   least 1.5 stop bits (MARK)** -- confirmed directly (w1hkj.org: "a
//!   stop bit of the opposite sense at least 1.5 data bits long").
//!
//! The ITA2 letters-shift table (digits, punctuation via FIGS, space,
//! CR/LF, and the LTRS/FIGS shift codes themselves) was cross-checked
//! against two independent sources (Wikipedia's Baudot code article and
//! iw5edi.com's own RTTY tutorial) for every character actually needed
//! for real ham-radio text exchange (letters, digits, space, CR/LF,
//! basic punctuation). **Honest caveat, not silently glossed over**: a
//! handful of FIGS-shift punctuation characters (S/J/H/V's figure-shift
//! glyphs specifically) have real, sourced disagreement between the
//! international ITA2 table and the "US-TTY" variant amateur radio
//! conventionally uses -- this table follows the US-TTY convention for
//! those four positions, but they were not independently triple-checked
//! against a definitive standard the way the rest of the table was.
//! Real ham QSO text (callsigns, RST reports, "CQ DE") never needs them.
//!
//! **Scope, matching this exact codebase's own established precedent for
//! `psk31.rs`** (see that file's own module doc: "This deliberately does
//! NOT do full carrier/timing acquisition or tracking... What's
//! implemented is a real, working demodulator for a signal at a known
//! frequency and baud rate"): `rtty_demodulate`/`RttyDecoder` are real,
//! working decoders that do real start-bit edge detection (RTTY is
//! asynchronous by design -- unlike PSK31's continuous phase tracking,
//! edge detection isn't an optional robustness feature, it's the actual
//! framing mechanism). The detector (`frame_score`, `pre_is_persistently_
//! mark`) is a real frame-matched, amplitude-invariant design, measured
//! directly against a Gaussian-noise sensitivity harness (`tests/rtty_
//! processing_gain_harness.rs`) rather than just this file's own
//! synthetic-signal round-trip tests -- see that harness's own module
//! doc comment for the measured before/after comparison against the
//! original single-window/absolute-energy-floor design it replaced.
//! Unlike `psk31.rs`'s own carrier (still fixed-frequency, a real disclosed
//! limitation there), this module now does real bounded carrier-frequency
//! tracking: `AFC_OFFSETS_HZ`/`TrigBank` evaluate every candidate edge
//! against a small bank of nearby carrier-frequency hypotheses and lock
//! onto whichever one actually decodes, re-deriving the estimate fresh on
//! every character rather than needing a separate closed-loop filter --
//! see `rtty_scan`'s own doc comment for the mechanism, and `tests/rtty_
//! processing_gain_harness.rs`'s own measured before/after (real off-air
//! mistuning that used to collapse the old fixed-frequency correlator by
//! 30Hz now decodes cleanly through the harness's own full +/-50Hz sweep).
//! `RttyDecoder` is the `Psk31Decoder`-style
//! chunk-surviving streaming wrapper (see its own doc comment for the
//! real, RTTY-specific lookahead-margin subtlety asynchronous framing
//! needs that PSK31's fixed-symbol-length decode doesn't) -- built, and
//! wired into `hams_com`'s `digital_decoder.rs` dispatch the same way
//! PSK31/FT8/WSPR already are, proven end to end by that crate's own
//! `raw_input_audio_reaches_a_real_rtty_decode` test (real audio in
//! through the actual pipeline entry point, real decoded text out).

const RTTY_BAUD: f64 = 45.45;
const RTTY_DEFAULT_MARK_HZ: f64 = 2125.0;
const RTTY_DEFAULT_SHIFT_HZ: f64 = 170.0;
const RTTY_STOP_BIT_UNITS: f64 = 1.5;

/// One entry per 5-bit Baudot code (0..32): the letters-shift and
/// figures-shift (US-TTY) character each code represents. `None` where a
/// case has no printable character in that shift (there isn't one in
/// practice -- every code maps to something in both shifts -- kept as
/// `Option` only for the two shift-control codes themselves, which are
/// handled specially, not looked up here).
const LTRS_CHARS: [char; 32] = [
    '\0', 'E', '\n', 'A', ' ', 'S', 'I', 'U', '\r', 'D', 'R', 'J', 'N', 'F', 'C', 'K', 'T', 'Z',
    'L', 'W', 'H', 'Y', 'P', 'Q', 'O', 'B', 'G', '\0', /* FIGS */
    'M', 'X', 'V', '\0', /* LTRS */
];
const FIGS_CHARS: [char; 32] = [
    '\0', '3', '\n', '-', ' ', '\x07', '8', '7', '\r', '$', '4', '\'', ',', '!', ':', '(', '5',
    '"', ')', '2', '#', '6', '0', '1', '9', '?', '&', '\0', /* FIGS */
    '.', '/', ';', '\0', /* LTRS */
];
const CODE_LTRS_SHIFT: u8 = 0b11111;
const CODE_FIGS_SHIFT: u8 = 0b11011;

/// Encodes one ASCII character to its 5-bit Baudot code plus which shift
/// (letters=false, figures=true) it requires, or `None` if the character
/// has no Baudot representation at all (anything outside this table --
/// real RTTY text is 5-bit-limited by design, not an oversight here).
// [@ANCHOR: char_to_baudot]
fn char_to_baudot(c: char) -> Option<(u8, bool)> {
    let upper = c.to_ascii_uppercase();
    if let Some(code) = LTRS_CHARS.iter().position(|&ch| ch != '\0' && ch == upper) {
        return Some((code as u8, false));
    }
    if let Some(code) = FIGS_CHARS.iter().position(|&ch| ch != '\0' && ch == upper) {
        return Some((code as u8, true));
    }
    None
}

/// Encodes text to a Baudot bit sequence including real shift-code
/// insertion (only when the current shift state actually needs to
/// change, not before every character -- matching real RTTY transmit
/// practice, which minimizes redundant shift codes) and start/stop
/// framing per character. Returns `(bit, is_mark)` pairs is not how this
/// works -- instead returns the framed bit sequence as `bool` (true =
/// MARK, false = SPACE) at one bit per Baudot bit-period, ready for
/// `rtty_modulate`'s own tone synthesis.
// [@ANCHOR: text_to_framed_bits]
fn text_to_framed_bits(text: &str) -> Vec<bool> {
    let mut bits: Vec<bool> = Vec::new();
    let mut current_figs = false;
    for c in text.chars() {
        let Some((code, needs_figs)) = char_to_baudot(c) else {
            continue;
        };
        if needs_figs != current_figs {
            let shift_code = if needs_figs {
                CODE_FIGS_SHIFT
            } else {
                CODE_LTRS_SHIFT
            };
            push_framed_char(&mut bits, shift_code);
            current_figs = needs_figs;
        }
        push_framed_char(&mut bits, code);
    }
    bits
}

/// Appends one framed character (start bit + 5 data bits LSB-first + stop
/// "bit," modeled as one bit period here -- `rtty_modulate` extends the
/// final stop bit's own tone duration to the real 1.5-unit length, since
/// a fractional bit period doesn't fit this `bool`-per-bit-period
/// representation cleanly).
// [@ANCHOR: push_framed_char]
fn push_framed_char(bits: &mut Vec<bool>, code: u8) {
    bits.push(false); // start bit: SPACE
    for i in 0..5 {
        bits.push((code >> i) & 1 == 1); // data bits, LSB first; true=MARK(1), false=SPACE(0)
    }
    bits.push(true); // stop bit: MARK (duration extended by rtty_modulate)
}

/// Synthesizes an RTTY audio signal (real i16 PCM, mono) for the given
/// text. Continuous-phase FSK (no phase discontinuity at mark/space
/// transitions) -- the same reason `psk31_modulate` uses a raised-cosine
/// envelope rather than a hard amplitude step: an abrupt phase jump would
/// splatter energy across the band well outside the intended 170Hz
/// shift.
// [@ANCHOR: rtty_modulate]
pub fn rtty_modulate(text: &str, mark_hz: f64, sample_rate: u32) -> Vec<i16> {
    framed_bits_to_audio(&text_to_framed_bits(text), mark_hz, sample_rate)
}

/// The real per-bit FSK synthesis `rtty_modulate` uses, split out so tests can feed it framed
/// bits built directly via `push_framed_char` -- bypassing `text_to_framed_bits`' own automatic
/// LTRS/FIGS shift-code insertion -- to synthesize the specific, otherwise-hard-to-produce bit
/// sequences USOS (unshift-on-space, see `attempt_character`'s own doc comment) is meant to
/// recover from: a real transmission that never sends an explicit unshift code at all.
// [@ANCHOR: framed_bits_to_audio]
fn framed_bits_to_audio(bits: &[bool], mark_hz: f64, sample_rate: u32) -> Vec<i16> {
    let space_hz = mark_hz + RTTY_DEFAULT_SHIFT_HZ;
    let samples_per_bit = sample_rate as f64 / RTTY_BAUD;
    let mut out = Vec::new();
    let mut phase = 0.0f64;
    let n_bits = bits.len();
    for (idx, &is_mark) in bits.iter().enumerate() {
        // The final bit of each framed character is the stop bit --
        // extend it to the real 1.5-unit stop duration. Framing always
        // emits exactly 7 bits per character (1 start + 5 data + 1
        // stop), so every 7th bit (idx % 7 == 6) is a stop bit.
        let is_stop_bit = idx % 7 == 6;
        let bit_duration_units = if is_stop_bit {
            RTTY_STOP_BIT_UNITS
        } else {
            1.0
        };
        let n_samples = (samples_per_bit * bit_duration_units).round() as usize;
        let freq = if is_mark { mark_hz } else { space_hz };
        let phase_inc = std::f64::consts::TAU * freq / sample_rate as f64;
        for _ in 0..n_samples {
            let sample = phase.sin();
            out.push(
                (sample * i16::MAX as f64 * 0.9)
                    .round()
                    .clamp(i16::MIN as f64, i16::MAX as f64) as i16,
            );
            phase += phase_inc;
            if phase > std::f64::consts::TAU {
                phase -= std::f64::consts::TAU;
            }
        }
    }
    let _ = n_bits;
    out
}

/// The real correlation this module's demodulation is built on -- a per-window quadrature
/// (I/Q) correlator against both candidate tones, the same shape of computation `nlp()`'s own
/// squared-signal power spectrum uses for pitch detection, just applied to two fixed candidate
/// frequencies instead of a whole bank. Kept as a slow, from-scratch reference implementation
/// (recomputes `cos`/`sin` per sample rather than using `TrigTables`' cached version below) --
/// `rtty_scan`'s own frame-matched detector uses `TrigTables::signed_ratio_db` instead for real
/// performance reasons (many overlapping windows evaluated per candidate edge), but this
/// function stays as the independent ground truth `mark_space_ratio_separates_signal_from_
/// noise_across_a_full_amplitude_sweep` measures against -- the two are exercised by different
/// code paths, so agreement between them is a real cross-check, not a tautology.
// [@ANCHOR: window_mark_space_energy]
fn window_mark_space_energy(
    samples: &[i16],
    mark_hz: f64,
    space_hz: f64,
    sample_rate: u32,
) -> (bool, f64, f64) {
    let mut mark_i = 0.0f64;
    let mut mark_q = 0.0f64;
    let mut space_i = 0.0f64;
    let mut space_q = 0.0f64;
    for (n, &s) in samples.iter().enumerate() {
        let x = s as f64 / i16::MAX as f64;
        let t = n as f64 / sample_rate as f64;
        let mark_phase = std::f64::consts::TAU * mark_hz * t;
        let space_phase = std::f64::consts::TAU * space_hz * t;
        mark_i += x * mark_phase.cos();
        mark_q += x * mark_phase.sin();
        space_i += x * space_phase.cos();
        space_q += x * space_phase.sin();
    }
    let mark_energy = mark_i * mark_i + mark_q * mark_q;
    let space_energy = space_i * space_i + space_q * space_q;
    (mark_energy > space_energy, mark_energy, space_energy)
}

/// Precomputed per-sample I/Q correlator tables for one fixed `(mark_hz, space_hz,
/// sample_rate)` triple, indexed by *window-relative* sample offset (0..capacity). Valid for a
/// window of any length up to `capacity` starting anywhere in the stream, since the correlator
/// phase reference `t = n / sample_rate` always counts from the window's own first sample, not
/// an absolute stream position -- exactly the phase convention `window_mark_space_energy`
/// already used, just computed once here instead of by every call. This removes a real
/// inefficiency `frame_score` below would otherwise pay many times per character: several
/// overlapping windows are evaluated per candidate edge, and a per-edge local-peak refinement
/// (see `frame_score`'s own doc comment) evaluates the same shape of window at many nearby
/// offsets -- recomputing `cos`/`sin` from scratch each time was pure waste once the frame-
/// matched detector replaced the old single-window-per-candidate design.
struct TrigTables {
    cos_mark: Vec<f64>,
    sin_mark: Vec<f64>,
    cos_space: Vec<f64>,
    sin_space: Vec<f64>,
}

impl TrigTables {
    // [@ANCHOR: TrigTables::new]
    fn new(mark_hz: f64, space_hz: f64, sample_rate: u32, capacity: usize) -> Self {
        let mut cos_mark = Vec::with_capacity(capacity);
        let mut sin_mark = Vec::with_capacity(capacity);
        let mut cos_space = Vec::with_capacity(capacity);
        let mut sin_space = Vec::with_capacity(capacity);
        for n in 0..capacity {
            let t = n as f64 / sample_rate as f64;
            let mark_phase = std::f64::consts::TAU * mark_hz * t;
            let space_phase = std::f64::consts::TAU * space_hz * t;
            cos_mark.push(mark_phase.cos());
            sin_mark.push(mark_phase.sin());
            cos_space.push(space_phase.cos());
            sin_space.push(space_phase.sin());
        }
        Self {
            cos_mark,
            sin_mark,
            cos_space,
            sin_space,
        }
    }

    /// The real per-window correlation this module's demodulation is built on: MARK and SPACE
    /// energy over `samples[start..start+len)`, plus the signed dB ratio between them (positive
    /// means MARK dominates, negative means SPACE dominates -- amplitude-invariant by
    /// construction, since a real tone's dominant-frequency correlator and its counterpart both
    /// scale by the same input-amplitude factor, the same reasoning this module's own `mark_
    /// space_ratio_separates_signal_from_noise_across_a_full_amplitude_sweep` test already
    /// measured and confirmed). Returns both, not just the ratio, because they serve different
    /// comparisons: the ratio is what every bit/framing decision in this module uses (`signed_
    /// ratio_db`, below); the raw linear `mark_e + space_e` energy is what `best_frame_
    /// evidence`'s own AFC frequency selection needs instead (see its own doc comment for why a
    /// dB-domain comparison is the wrong tool for comparing *across* candidate frequencies, even
    /// though it's the right one for comparing MARK against SPACE *within* one).
    ///
    /// `None` when the window runs entirely past the sample buffer. A window that only
    /// *partially* overruns the buffer by a small amount is silently shortened to whatever
    /// samples remain rather than rejected outright -- real for the last character of a
    /// whole-buffer `rtty_demodulate` call, whose audio (by construction, see `rtty_modulate`)
    /// ends exactly at the end of the final stop bit with zero trailing margin, so an off-by-a-
    /// few-samples rounding mismatch between `rtty_modulate`'s per-bit-rounded synthesis and
    /// this module's own geometry would otherwise lose that character outright. A shortfall
    /// larger than 10% is a structurally different situation (not enough real audio for this
    /// bit at all, not a few stray samples) and *is* rejected outright (`None`) -- see the
    /// 90%-length guard below; a heavily shortened window's own ratio is itself an unreliable,
    /// fat-tailed coin flip (`PER_WINDOW_CLIP_DB`'s own doc comment), and silently accepting one
    /// let a whole-buffer decode cut off mid-character emit garbage for the truncated tail bits
    /// instead of refusing them the way the original design's own `bit_at` bounds check did.
    /// Safe to apply this shortening unconditionally here (not just for the whole-buffer
    /// caller) because `rtty_scan`'s own streaming-lookahead check already guarantees a full,
    /// unclipped window for every position it lets reach this far in `stop_if_insufficient_
    /// lookahead: true` mode -- the shortening only ever actually triggers at a real signal's
    /// true end.
    // [@ANCHOR: TrigTables::window_evidence]
    fn window_evidence(&self, samples: &[i16], start: usize, len: usize) -> Option<(f64, f64)> {
        if len == 0 || start >= samples.len() {
            return None;
        }
        let available = samples.len() - start;
        if available * 10 < len * 9 {
            return None;
        }
        let len = len.min(available).min(self.cos_mark.len());
        if len == 0 {
            return None;
        }
        let mut mark_i = 0.0f64;
        let mut mark_q = 0.0f64;
        let mut space_i = 0.0f64;
        let mut space_q = 0.0f64;
        for k in 0..len {
            let x = samples[start + k] as f64 / i16::MAX as f64;
            mark_i += x * self.cos_mark[k];
            mark_q += x * self.sin_mark[k];
            space_i += x * self.cos_space[k];
            space_q += x * self.sin_space[k];
        }
        let mark_e = mark_i * mark_i + mark_q * mark_q;
        let space_e = space_i * space_i + space_q * space_q;
        let ratio_db = 10.0 * (mark_e.max(1e-15) / space_e.max(1e-15)).log10();
        Some((ratio_db, mark_e + space_e))
    }

    /// Thin wrapper over `window_evidence` for the (large majority of) callers that only need
    /// the signed MARK-vs-SPACE ratio, not the raw energy -- see that function's own doc
    /// comment for the full mechanism and the real reason both are kept separate.
    // [@ANCHOR: TrigTables::signed_ratio_db]
    fn signed_ratio_db(&self, samples: &[i16], start: usize, len: usize) -> Option<f64> {
        self.window_evidence(samples, start, len)
            .map(|(ratio_db, _)| ratio_db)
    }
}

/// Candidate carrier-frequency offsets (Hz) a bounded AFC (automatic frequency control) search
/// evaluates on every candidate edge, added to the decoder's configured `mark_hz` (and,
/// implicitly, to `space_hz` too, since real off-air mistuning shifts a receiver's whole
/// passband -- both tones together -- not the transmitting station's own FSK shift width).
/// Real off-air mistuning is the single largest weak point this module's own sensitivity
/// harness (`tests/rtty_processing_gain_harness.rs`) measured: at +10dB SNR, a fixed-frequency
/// correlator decodes cleanly through 20Hz of offset and collapses by 30Hz (`sinc`-shaped
/// correlator loss, roughly half the 170Hz shift) -- exactly the range no amount of the
/// amplitude-domain processing gain elsewhere in this module (the frame-matched detector,
/// persistence gating) can recover, since a mismatched reference frequency loses real signal
/// energy at the correlation step itself, before any of that machinery ever sees it. 9 points
/// spanning +/-40Hz at 10Hz steps: fine enough that the worst-case residual mismatch after
/// snapping to the nearest candidate (5Hz) stays well inside the ~20Hz the harness measured as
/// still-clean, wide enough to cover real amateur-radio tuning error (a few tens of Hz is
/// typical for a manually-tuned receiver or a transmitter's own crystal drift) without paying
/// for a search wide enough to also need to worry about a *different* station's tone entirely.
const AFC_OFFSETS_HZ: [f64; 9] = [-40.0, -30.0, -20.0, -10.0, 0.0, 10.0, 20.0, 30.0, 40.0];

/// A single-candidate "AFC offset list" for `RttyDecoder::new_fixed` -- see that function's own
/// doc comment for why a wide-passband channel-scanning bank (`digital_decoder.rs`'s own RTTY
/// candidate bank) wants decoders with no inner AFC search at all, not a smaller version of the
/// 9-candidate search below.
const FIXED_OFFSET_HZ: [f64; 1] = [0.0];

/// A small bank of `TrigTables`, one per offset in `offsets`, all built from the same configured
/// `mark_hz`/`sample_rate` -- the real mechanism the AFC search in `rtty_scan` scans across.
/// Built once per decoder lifetime (`RttyDecoder::new`/`new_fixed`) or once per whole-buffer call
/// (`rtty_demodulate`), the same real reason `TrigTables` itself moved out of the per-window hot
/// path: `sin`/`cos` tables are expensive to recompute and never change once `mark_hz` and
/// `sample_rate` are fixed. `offsets` is carried alongside the tables (not just consulted via the
/// global `AFC_OFFSETS_HZ` constant) so `RttyDecoder::locked_frequency_offset_hz` reports the
/// right value regardless of which offset list this particular decoder was built with -- a real
/// bug this shape avoids: indexing a fixed single-table bank's `locked_offset_idx` (always 0)
/// into the 9-entry `AFC_OFFSETS_HZ` array would silently report -40.0Hz instead of the true 0.0Hz.
struct TrigBank {
    tables: Vec<TrigTables>,
    offsets: &'static [f64],
}

impl TrigBank {
    // [@ANCHOR: TrigBank::new_with_offsets]
    fn new_with_offsets(
        offsets: &'static [f64],
        mark_hz: f64,
        sample_rate: u32,
        capacity: usize,
    ) -> Self {
        let tables = offsets
            .iter()
            .map(|&offset_hz| {
                let candidate_mark_hz = mark_hz + offset_hz;
                let candidate_space_hz = candidate_mark_hz + RTTY_DEFAULT_SHIFT_HZ;
                TrigTables::new(candidate_mark_hz, candidate_space_hz, sample_rate, capacity)
            })
            .collect();
        Self { tables, offsets }
    }

    // [@ANCHOR: TrigBank::new]
    fn new(mark_hz: f64, sample_rate: u32, capacity: usize) -> Self {
        Self::new_with_offsets(&AFC_OFFSETS_HZ, mark_hz, sample_rate, capacity)
    }
}

/// Evaluates `frame_score` at `edge_pos` against *every* candidate frequency offset in `bank`
/// and returns the best-matching one -- by `magnitude`, not `accept_score` -- along with which
/// offset index won. This is the real AFC search, run at every scan position, not gated behind
/// "only search when the locked offset already looks promising": a station that has drifted
/// since the last successful character would, by definition, no longer score well at the
/// previously-locked offset, so a search that only widens after a locked-offset failure would
/// itself be too slow to reacquire. `AFC_OFFSETS_HZ`'s own doc comment covers why 9 candidates
/// is cheap enough to afford this unconditionally rather than needing a cheaper pre-filter
/// first.
///
/// **Two real bugs, found and fixed by direct measurement, not assumed correct from the shape
/// alone** (`rtty_decoder_tracks_a_real_off_air_frequency_offset`), both about *which metric*
/// selects the winning offset among candidates that already pass `accept_score`'s own noise-
/// rejection gate. First: picking by `accept_score` itself doesn't work -- it's *clipped*
/// (`PER_WINDOW_CLIP_DB`), so once a candidate is merely "good enough," several neighboring
/// offsets around the true one saturate to the same value and become indistinguishable by it;
/// picking among ties that way measured directly to sometimes prefer a genuinely worse-matched
/// candidate (e.g. locking a 30Hz offset for a real 5Hz mistuning) purely from correlator phase
/// noise at one specific sample position, corrupting that character's own data bits. Second:
/// picking by a *sum of dB ratios* (this function's own first fix attempt) is the wrong
/// physical quantity even though it isn't clipped -- a matched filter's real frequency-mismatch
/// rolloff is `sinc`-shaped in *linear* correlator energy, and only shows up as a small,
/// noise-comparable difference once compressed into dB. `magnitude` (see `FrameEvidence`'s own
/// doc comment) is real linear energy (`mark_e + space_e`, summed unclipped across all 7
/// windows) specifically so it tracks that physical rolloff directly -- `TrigTables::window_
/// evidence`'s own doc comment covers why the ratio and the energy have to come from the same
/// underlying correlation rather than being computed separately. `magnitude` is not itself
/// safe as the noise-rejection gate (unclipped sums are fat-tailed under noise, the same reason
/// `PER_WINDOW_CLIP_DB` exists at all) -- it only ever chooses *among* candidates
/// `accept_score` has already vetted.
// [@ANCHOR: best_frame_evidence]
fn best_frame_evidence(
    samples: &[i16],
    edge_pos: usize,
    geo: &FrameGeometry,
    bank: &TrigBank,
) -> Option<(usize, FrameEvidence)> {
    let mut best: Option<(usize, FrameEvidence)> = None;
    for (idx, trig) in bank.tables.iter().enumerate() {
        if let Some(evidence) = frame_score(samples, edge_pos, geo, trig) {
            if evidence.accept_score <= FRAME_SCORE_THRESHOLD_DB {
                continue;
            }
            let is_better = match &best {
                Some((_, b)) => evidence.magnitude > b.magnitude,
                None => true,
            };
            if is_better {
                best = Some((idx, evidence));
            }
        }
    }
    best
}

/// Real sample-domain geometry for one bit period at a given sample rate, computed once per
/// scan rather than re-derived at every candidate position.
struct FrameGeometry {
    samples_per_bit: f64,
    window_len: usize,      // one data-bit-period window, rounded
    stop_window_len: usize, // the stop bit's own real RTTY_STOP_BIT_UNITS (1.5-unit) window
}

impl FrameGeometry {
    fn new(sample_rate: u32) -> Self {
        let samples_per_bit = sample_rate as f64 / RTTY_BAUD;
        Self {
            samples_per_bit,
            window_len: samples_per_bit.round() as usize,
            stop_window_len: (samples_per_bit * RTTY_STOP_BIT_UNITS).round() as usize,
        }
    }
}

/// How many (clipped) dB of combined framing + data-bit evidence (`FrameEvidence::accept_score`,
/// see `frame_score`'s own doc comment) a candidate edge needs to be treated as a real
/// character -- this module's real presence/confidence gate, replacing the old `PresenceGate`'s
/// absolute-energy floor (see this module's own git history for that design and the amplitude-
/// scaling fragility it never fully escaped). With 7 clipped windows summed (start + stop + 5
/// data bits, each capped at `PER_WINDOW_CLIP_DB`), a real signal's own accept_score saturates
/// near `7 * PER_WINDOW_CLIP_DB` (126); 80 sits well below that but requires the equivalent of
/// roughly 4-5 of the 7 windows reading strongly, more than any one or two fat-tailed noise
/// window spikes can fake even after `pre_is_persistently_mark` and clipping already remove the
/// easy false positives. Tuned directly against `tests/rtty_processing_gain_harness.rs`'s own
/// `amplitude_invariance_false_alarm_sweep` (Gaussian noise across the full i16 RMS range,
/// 500-30000) and this crate's pre-existing regression tests (a different, uniform-xorshift
/// noise texture, at fixed amplitudes) -- both must independently stay at the same "0-2 stray
/// characters in 10s" tolerance the original `PresenceGate` design used, and both do at this
/// value.
const FRAME_SCORE_THRESHOLD_DB: f64 = 80.0;
/// Caps any single window's raw signed-ratio contribution before it's summed. Necessary, not
/// cosmetic: `TrigTables::signed_ratio_db` is a *ratio* of two correlator energies in dB, which
/// is mathematically unbounded above whenever the denominator happens to land near zero -- a
/// real, measured problem for noise specifically (individual noise windows were measured
/// spiking past 60dB, well above a real signal's own stable ~35-40dB per-window reading), since
/// a single low-probability near-zero-energy noise window can otherwise dominate the whole sum
/// and defeat the entire point of combining multiple windows' evidence. Clipping bounds each
/// window's own vote at a level real signal always saturates but noise only occasionally does,
/// which is what makes requiring *several* windows to jointly saturate (`FRAME_SCORE_
/// THRESHOLD_DB`'s own doc comment) a real, independent-evidence discriminator rather than one
/// outlier deciding the outcome alone.
const PER_WINDOW_CLIP_DB: f64 = 18.0;
/// Fine-grained scan grid: how many candidate positions to evaluate per bit period. The
/// original design used 4 (matched to that design's coarse mark/space transition detector,
/// which only needed to find *a* transition before switching to precise per-bit sampling);
/// this design's local-peak refinement (`FrameEvidence`'s own doc comment) needs a genuinely
/// finer starting grid to reliably land within the peak's own basin rather than stepping over
/// it entirely on a strongly time-compressed or noisy signal.
const SCAN_GRID_PER_BIT: f64 = 8.0;
/// How many independent sub-windows `pre_is_persistently_mark` splits the bit period right
/// before a candidate edge into. Matches this module's own already-measured, amplitude-
/// invariant finding (`mark_space_ratio_separates_signal_from_noise_across_a_full_amplitude_
/// sweep`): a real idle-MARK tone -- either genuine leading idle audio, or, for every character
/// after the first in a burst, the *previous* character's own stop-bit tail, so consecutive
/// characters give each other a real "settling window" for free -- sustains a confident MARK
/// ratio across its *entire* duration, while noise's longest observed run of confident windows
/// tops out at 3 (that test's own finding, at a stricter 15dB ratio threshold than this gate
/// uses). Requiring both `PERSIST_SUBWINDOWS` sub-windows to independently clear `PERSIST_
/// RATIO_THRESHOLD_DB` is a real, independent check on top of `accept_score`'s own 7-window
/// sum, not a duplicate of it -- it specifically rejects candidates whose *framing* context
/// (what comes immediately before the claimed start bit) doesn't look like real idle/stop-bit
/// MARK at all, regardless of how the start/stop/data windows themselves happen to read.
const PERSIST_SUBWINDOWS: usize = 2;
/// Per-sub-window MARK-ratio threshold for `pre_is_persistently_mark`. Deliberately much lower
/// than `mark_space_ratio_separates_signal_from_noise_across_a_full_amplitude_sweep`'s own 15dB
/// figure (an isolated, unclipped-magnitude measurement of that one statistic alone) -- tuned
/// down from an initial 15dB via `tests/rtty_processing_gain_harness.rs`'s own SNR sweep, which
/// found the stricter value rejecting real, moderately-noisy signal far more often than it
/// rejected noise, since `accept_score`'s own clipped 7-window sum (see `FRAME_SCORE_
/// THRESHOLD_DB`'s own doc comment) already carries most of the real discriminating power here;
/// this check only needs to rule out candidates with no plausible preceding idle/stop-bit MARK
/// at all, not re-litigate signal-vs-noise on its own.
const PERSIST_RATIO_THRESHOLD_DB: f64 = 4.0;

/// Checks that the bit period immediately before `edge_pos` is a genuinely *sustained* MARK
/// tone, not just a single favorable window -- the real, already-measured persistence
/// invariant this module's own `mark_space_ratio_separates_signal_from_noise_across_a_full_
/// amplitude_sweep` test exists to document (see `PERSIST_SUBWINDOWS`'s own doc comment).
/// Splits that one bit period into `PERSIST_SUBWINDOWS` independent sub-windows and requires
/// *every* one to individually clear `PERSIST_RATIO_THRESHOLD_DB` -- true for a real idle/
/// previous-stop-bit MARK tone (which holds for the tone's *entire* duration), while noise
/// sustaining that many independent windows in a row, at a fixed, un-scanned position, is a
/// genuinely rare coincidence rather than something a scan across many candidate positions
/// should expect to find often. Missing history (an edge at or near the very start of the
/// buffered stream) passes automatically -- there is no "before" to fail on, matching this
/// module's own fixed warmup-veto bug (a real signal starting the instant a receiver begins
/// listening must not be penalized for looking unfamiliar).
// [@ANCHOR: pre_is_persistently_mark]
fn pre_is_persistently_mark(
    samples: &[i16],
    edge_pos: usize,
    geo: &FrameGeometry,
    trig: &TrigTables,
) -> bool {
    let w = geo.window_len;
    if edge_pos < w {
        return true;
    }
    let sub_len = (w / PERSIST_SUBWINDOWS).max(1);
    let region_start = edge_pos - w;
    let mut pos = region_start;
    while pos + sub_len <= edge_pos {
        match trig.signed_ratio_db(samples, pos, sub_len) {
            Some(r) if r > PERSIST_RATIO_THRESHOLD_DB => {}
            _ => return false,
        }
        pos += sub_len;
    }
    true
}

/// Combined frame-matched confidence for treating `edge_pos` as a real character's start-bit
/// edge -- the real "processing gain" move this decoder is built on. The old design accepted a
/// candidate based on a single mark->space transition (a hard 1-bit decision) plus a separate,
/// amplitude-dependent absolute-energy floor bolted on afterward; this instead requires two
/// independent kinds of evidence to agree, each built from RTTY's own known frame structure
/// rather than a single instantaneous sample: `pre_is_persistently_mark` (a real *sustained*-
/// tone check, immune to any one window's own fat-tailed dB blowup -- see its own doc comment),
/// and this function's own clipped start+stop magnitude sum (the start bit must read SPACE, the
/// stop bit must read MARK across its full real `RTTY_STOP_BIT_UNITS` duration, each clipped to
/// `PER_WINDOW_CLIP_DB` so neither window alone can dominate the sum). Because both checks are
/// built from signed *ratios*, not absolute energy, the same thresholds work unchanged across
/// the full amplitude sweep the old design's own `#[ignore]`d test measured it failing on.
///
/// The three scores `frame_score` produces for one candidate edge, kept separate because they
/// serve different purposes and would corrupt each other if merged: `accept_score` decides
/// *whether* this is a real character (must be robust against fat-tailed noise outliers, so
/// every term is clipped -- see `PER_WINDOW_CLIP_DB`'s own doc comment); `raw_score` finds
/// *where* the real edge actually is, at a single fixed frequency hypothesis (an unclipped
/// matched-filter output genuinely peaks at the true alignment, but clipping flattens that peak
/// into a plateau -- picking a position off a flat plateau via a strict `>` comparison
/// systematically biases the chosen position toward the plateau's first/leftmost point, up to a
/// full grid step of avoidable timing error); `magnitude` picks *which frequency hypothesis*
/// among several that all already pass `accept_score` is the real best match -- see
/// `best_frame_evidence`'s own doc comment for why `accept_score` itself is unsuitable for that
/// comparison (it saturates, making nearby frequency candidates falsely indistinguishable), and
/// why `magnitude` is real *linear* correlator energy (`mark_e + space_e`, summed across all 7
/// windows), not a sum of dB values -- a matched filter's own frequency-mismatch rolloff is a
/// real, physical statement about linear energy (`sinc`-shaped in frequency offset), and only
/// shows up as a small, noise-comparable difference once compressed into dB and clipped/summed
/// the way `accept_score` is.
struct FrameEvidence {
    accept_score: f64,
    raw_score: f64,
    magnitude: f64,
}

/// Combined frame-matched confidence for treating `edge_pos` as a real character's start-bit
/// edge -- the real "processing gain" move this decoder is built on. The old design accepted a
/// candidate based on a single mark->space transition (a hard 1-bit decision) plus a separate,
/// amplitude-dependent absolute-energy floor bolted on afterward; this instead requires multiple
/// independent kinds of evidence to agree, each built from RTTY's own known frame structure
/// rather than a single instantaneous sample: `pre_is_persistently_mark` (a real *sustained*-
/// tone check, immune to any one window's own fat-tailed dB blowup -- see its own doc comment);
/// the start bit (must read SPACE) and the stop bit (must read MARK across its full real
/// `RTTY_STOP_BIT_UNITS` duration); and the 5 data bits' own combined magnitude (each data bit,
/// whichever way it reads, is real evidence of a genuine two-tone FSK signal at this exact
/// timing -- noise's own per-window magnitude is small once clipped, while a real signal's 5
/// data-bit windows add 5 more independent, clipped confirmations on top of the 2 framing bits,
/// which is what actually closes the sensitivity gap a framing-only 2-window sum leaves at
/// moderate SNR). Every term is clipped to `PER_WINDOW_CLIP_DB` before summing into
/// `accept_score`, for the reason `PER_WINDOW_CLIP_DB`'s own doc comment gives. Because it's
/// built from signed *ratios*, not absolute energy, the same thresholds work unchanged across
/// the full amplitude sweep the old design's own `#[ignore]`d test measured it failing on.
///
/// Returns `None` when there isn't yet enough buffered audio to evaluate the stop bit, or when
/// the persistence check fails outright -- mirrors the original design's own streaming-
/// lookahead handling (`rtty_scan`'s own doc comment): "not enough samples yet" and "not enough
/// samples ever" are different situations for a streaming decoder, and callers use this to
/// distinguish them (a persistence failure is treated the same as "not a candidate here" --
/// scanning continues at the normal grid step, not a hard stop).
// [@ANCHOR: frame_score]
fn frame_score(
    samples: &[i16],
    edge_pos: usize,
    geo: &FrameGeometry,
    trig: &TrigTables,
) -> Option<FrameEvidence> {
    if !pre_is_persistently_mark(samples, edge_pos, geo, trig) {
        return None;
    }

    let spb = geo.samples_per_bit;
    let w = geo.window_len;

    let start_start = ((edge_pos as f64 + 0.5 * spb) - w as f64 / 2.0)
        .round()
        .max(0.0) as usize;
    let (start_ratio, start_energy) = trig.window_evidence(samples, start_start, w)?;

    let stop_start = (edge_pos as f64 + 6.0 * spb).round() as usize;
    let (stop_ratio, stop_energy) =
        trig.window_evidence(samples, stop_start, geo.stop_window_len)?;

    let mut data_bit_evidence = 0.0;
    let mut data_bit_energy = 0.0;
    for i in 0..5 {
        let center = edge_pos as f64 + (1.5 + i as f64) * spb;
        let bit_start = (center - w as f64 / 2.0).round().max(0.0) as usize;
        let (ratio, energy) = trig.window_evidence(samples, bit_start, w)?;
        data_bit_evidence += ratio.abs().min(PER_WINDOW_CLIP_DB);
        data_bit_energy += energy;
    }

    let clipped_start = (-start_ratio).clamp(-PER_WINDOW_CLIP_DB, PER_WINDOW_CLIP_DB);
    let clipped_stop = stop_ratio.clamp(-PER_WINDOW_CLIP_DB, PER_WINDOW_CLIP_DB);
    Some(FrameEvidence {
        accept_score: clipped_start + clipped_stop + data_bit_evidence,
        raw_score: -start_ratio + stop_ratio,
        magnitude: start_energy + stop_energy + data_bit_energy,
    })
}

/// Attempts a full character decode at a frame-score-confirmed `edge_pos`, sampling the 5 data
/// bits (LSB first) plus a real hard-polarity check on the start/stop bits (must be SPACE/MARK
/// respectively) on top of `frame_score`'s own soft aggregate threshold -- belt and suspenders:
/// `frame_score`'s `accept_score` can in principle clear its threshold from an unusually strong
/// stop-bit/data-bit-magnitude reading even when the start bit itself is ambiguous, and a real
/// start-bit polarity check is cheap insurance against decoding a character whose own framing
/// bit doesn't actually match. Returns `None` (not a character, caller should keep scanning past
/// this
/// position at the normal grid step) if either hard check fails or a bit window runs off the
/// end of buffered audio; `Some((text, advance))` otherwise, where `text` is the newly decoded
/// output (empty for a LTRS/FIGS shift code, which changes `state.current_figs` but emits no
/// character) and `advance` is how many samples past `edge_pos` the next scan should resume at.
// [@ANCHOR: attempt_character]
fn attempt_character(
    samples: &[i16],
    edge_pos: usize,
    geo: &FrameGeometry,
    trig: &TrigTables,
    state: &mut ScanState,
) -> Option<(String, usize)> {
    let spb = geo.samples_per_bit;
    let w = geo.window_len;

    let start_start = ((edge_pos as f64 + 0.5 * spb) - w as f64 / 2.0)
        .round()
        .max(0.0) as usize;
    let start_ratio = trig.signed_ratio_db(samples, start_start, w)?;
    if start_ratio >= 0.0 {
        return None; // start bit must be SPACE-dominant
    }

    let mut code = 0u8;
    for i in 0..5 {
        let center = edge_pos as f64 + (1.5 + i as f64) * spb;
        let bit_start = (center - w as f64 / 2.0).round().max(0.0) as usize;
        let ratio = trig.signed_ratio_db(samples, bit_start, w)?;
        if ratio > 0.0 {
            code |= 1 << i; // MARK = 1
        }
    }

    let stop_start = (edge_pos as f64 + 6.0 * spb).round() as usize;
    let stop_ratio = trig.signed_ratio_db(samples, stop_start, geo.stop_window_len)?;
    if stop_ratio <= 0.0 {
        return None; // stop bit must be MARK-dominant
    }

    let mut emitted = String::new();
    match code {
        CODE_LTRS_SHIFT => {
            state.current_figs = false;
            state.figs_non_digit_run = 0;
        }
        CODE_FIGS_SHIFT => {
            state.current_figs = true;
            // A fresh, explicit shift command is real evidence this run really is meant to be
            // FIGS -- don't let a counter built up before this shift (from a *previous* figs
            // run this same decoder never got confirmation on) carry over and immediately
            // false-trigger the implausible-run check below.
            state.figs_non_digit_run = 0;
        }
        _ => {
            // "Sensible USOS": an asymmetric extension of plain USOS above, per Bruce's own
            // real-time direction to recognize when shifted text doesn't make sense and
            // pre-emptively unshift. The asymmetry matters: real ham FIGS content is almost
            // entirely digits (signal reports, serials, frequencies, dates) with only
            // occasional punctuation, so a *run* of non-digit FIGS symbols (`:(`, `$`, `'`,
            // `!`, ...) is a strong, near-unambiguous signal of a missed LTRS shift, not real
            // transmitted content -- correcting on that pattern is safe. The reverse direction
            // (guessing FIGS->LTRS from a run of consonants, e.g. via a vowel-pattern check) is
            // deliberately NOT implemented: consonant clusters are entirely legitimate in the
            // callsigns hams care most about getting right (`K6BP`, `W1AW`), so a symmetric
            // rule would misfire exactly on the content this decoder most needs to get right.
            //
            // This corrects going forward from the character that completes the pattern (this
            // one, reinterpreted below), not retroactively: the 1-2 characters already emitted
            // earlier in the same bad run were already returned to the caller by a previous
            // `feed()` call (or earlier in this one) and can't be unsent without a real
            // multi-character emit-delay buffer this decoder doesn't keep. Bounding the damage
            // to at most 2 wrong characters per missed shift (rather than USOS's own "to the
            // next space") is still a real, meaningful improvement, and adding no output
            // latency keeps this decoder's own "immediately showing a sensible text stream"
            // property intact.
            let mut use_figs = state.current_figs;
            if use_figs {
                if FIGS_CHARS[code as usize].is_ascii_digit() {
                    state.figs_non_digit_run = 0;
                } else {
                    state.figs_non_digit_run += 1;
                    if state.figs_non_digit_run >= 3 {
                        use_figs = false;
                        state.current_figs = false;
                        state.figs_non_digit_run = 0;
                    }
                }
            }
            let c = if use_figs {
                FIGS_CHARS[code as usize]
            } else {
                LTRS_CHARS[code as usize]
            };
            if c != '\0' {
                emitted.push(c);
            }
            // USOS (Unshift On Space) -- standard amateur RTTY convention (fldigi's own
            // default): a SPACE resets the shift state to letters, regardless of which shift
            // this decoder currently thinks it's in. Space shares the same code (4) in both
            // LTRS and FIGS per ITA2, so this is unambiguous whichever table `c` came from.
            // Real, measured value: without it, a decoder that starts mid-transmission (cold
            // shift-state guess) or garbles a single LTRS/FIGS shift code on noisy copy prints
            // digits/punctuation in place of letters for the rest of the message, since nothing
            // else ever re-synchronizes the shift state. USOS bounds that damage to at most one
            // word; the implausible-run check above bounds it further, to at most 2 characters,
            // even mid-word.
            if c == ' ' {
                state.current_figs = false;
                state.figs_non_digit_run = 0;
            }
        }
    }
    // Same real per-bit-rounded advance the original design measured and fixed a drift bug
    // over: 6 data-bit-equivalent units (start + 5 data bits) at 1.0 unit each, rounded once,
    // plus the stop bit at its own real, separately-rounded RTTY_STOP_BIT_UNITS length --
    // matching `rtty_modulate`'s own per-bit rounding exactly, not one rounded 7-unit sum.
    let advance = 6 * spb.round() as usize + geo.stop_window_len;
    Some((emitted, advance))
}

/// Resumable scan state, carried across `RttyDecoder::feed()` calls (a
/// fresh, default one is used for the one-shot `rtty_demodulate`).
struct ScanState {
    pos: usize,
    current_figs: bool,
    /// Consecutive non-digit FIGS characters decoded while `current_figs` is true -- the
    /// "sensible USOS" extension's own evidence counter (see `attempt_character`'s own doc
    /// comment for the reasoning). Real ham FIGS content is almost entirely digits (signal
    /// reports, serials, frequencies); a run of non-digit FIGS punctuation is a strong signal
    /// of a missed LTRS shift, not real transmitted content. Reset to 0 by anything that
    /// legitimately re-synchronizes shift state: an explicit shift code either direction, a
    /// digit, USOS's own space-triggered unshift, or this counter's own trigger firing.
    figs_non_digit_run: usize,
    /// Index into `AFC_OFFSETS_HZ`/`TrigBank::tables` of whichever candidate frequency won the
    /// most recent successful character decode -- purely informational (`rtty_scan`'s own AFC
    /// search re-evaluates the full bank at every candidate position regardless, so this is
    /// never consulted to narrow that search), tracked so a caller (or a future one) can report
    /// the real, currently-estimated off-air frequency offset instead of assuming zero.
    locked_offset_idx: usize,
}

impl Default for ScanState {
    fn default() -> Self {
        Self::new(AFC_OFFSETS_HZ.len() / 2) // index of the 0.0Hz entry
    }
}

impl ScanState {
    // [@ANCHOR: ScanState::new]
    fn new(locked_offset_idx: usize) -> Self {
        Self {
            pos: 0,
            current_figs: false,
            figs_non_digit_run: 0,
            locked_offset_idx,
        }
    }
}

/// The real scan/framing loop shared by `rtty_demodulate` (whole-buffer,
/// `stop_if_insufficient_lookahead: false`, since the entire signal is
/// already available) and `RttyDecoder::feed` (streaming, `true`). Advances `state` in place
/// and returns any newly decoded characters.
///
/// Frame-matched detection (see `frame_score`'s own doc comment for the real processing-gain
/// mechanism) replaces the old coarse mark->space transition scan entirely -- every grid
/// position is scored directly, so there's no separate "is this even a candidate edge" pre-
/// filter to fall out of sync, and no `PresenceGate`-style absolute-energy floor to warm up or
/// contaminate. When a position clears `FRAME_SCORE_THRESHOLD_DB`, a small local search over
/// the surrounding +/-half-bit neighborhood finds the real peak -- a matched filter's output
/// peaks at the true alignment, so this also recovers fine bit timing that the old fixed 4x
/// grid never refined past.
///
/// **AFC (automatic frequency control)**: the initial coarse grid position is scored against
/// the whole `AFC_OFFSETS_HZ` bank (`best_frame_evidence`), not just the decoder's nominally-
/// configured frequency -- see that constant's own doc comment for why real off-air mistuning
/// is worth this, and `best_frame_evidence`'s own doc comment for why offset selection uses an
/// unclipped `magnitude` metric rather than `accept_score` or `raw_score` (both real, measured
/// bugs an earlier version of this had). Whichever offset wins there becomes a single, fixed
/// frequency hypothesis for the rest of that character: the local-peak position refinement
/// below searches only within that one table (position and frequency are deliberately *not*
/// re-arbitrated together at every refinement candidate -- see `best_frame_evidence`'s own doc
/// comment for the specific bug that coupling caused), and `state.locked_offset_idx` is updated
/// to it once the character decodes -- real, if simple, frequency tracking: the estimate
/// re-derives itself fresh on every successfully decoded character rather than needing a
/// separate closed-loop filter, since RTTY characters arrive often enough (every ~150ms during
/// a real transmission) for "most recent character's own winning offset" to already track slow
/// drift well.
///
/// The `stop_if_insufficient_lookahead` distinction is real, not cosmetic (unchanged from the
/// original design): "not enough samples yet" and "not enough samples ever" are different
/// situations for a *streaming* decoder. Without this flag, a start-bit edge that's real but
/// whose character (plus the refinement neighborhood's own worst-case lookahead) hasn't fully
/// arrived yet would be scored as if it were, and get scanned past -- permanently losing that
/// character once the rest of it arrives in a later `feed()` call. Stopping the scan before
/// committing to an under-buffered candidate means the next `feed()` call (with more buffered
/// audio) retries that exact position from scratch.
// [@ANCHOR: rtty_scan]
fn rtty_scan(
    samples: &[i16],
    geo: &FrameGeometry,
    bank: &TrigBank,
    state: &mut ScanState,
    stop_if_insufficient_lookahead: bool,
) -> String {
    let mut out = String::new();
    if geo.window_len == 0 {
        return out;
    }

    let grid_step = (geo.samples_per_bit / SCAN_GRID_PER_BIT).max(1.0) as usize;
    // How far the local-peak refinement below searches on either side of a threshold-clearing
    // grid position -- also folded into the streaming lookahead check (below) so every position
    // the refinement might inspect is guaranteed to already have its own full stop-bit window
    // available, not just the coarse grid position that triggered the search.
    let refine_span = (geo.samples_per_bit * 0.5).round() as usize;

    let mut pos = state.pos;
    while pos < samples.len() {
        let worst_case_pos = pos + refine_span;
        let full_char_end = worst_case_pos as f64 + 7.5 * geo.samples_per_bit;
        if stop_if_insufficient_lookahead && full_char_end > samples.len() as f64 {
            break;
        }

        if let Some((offset_idx, evidence)) = best_frame_evidence(samples, pos, geo, bank) {
            // `best_frame_evidence` already picked, by `magnitude`, whichever frequency
            // candidate is the real best physical match among those that pass `accept_
            // score`'s own noise-rejection gate (see both their own doc comments) -- so the
            // frequency hypothesis for this whole character is fixed here, at the untried
            // coarse `pos`, and position refinement below searches only within that single
            // table. An earlier version of this also let position refinement re-pick the
            // offset at each candidate position via `raw_score` (a 2-window difference) --
            // measured directly to be a real bug: `raw_score` is not what discriminates
            // between frequency candidates (`magnitude` is, precisely because `raw_score`'s
            // own difference structure can favor a worse-matched offset by sheer correlator
            // phase noise at one specific sample position), so letting it re-arbitrate the
            // offset mid-refinement occasionally relocked onto a clearly-wrong candidate
            // (`rtty_decoder_tracks_a_real_off_air_frequency_offset`'s own doc comment).
            let trig = &bank.tables[offset_idx];

            // Refine on the *raw* (unclipped) score, not the clipped accept_score: a real
            // matched-filter output peaks sharply at the true alignment, but clipping
            // flattens a strong signal's own peak into a plateau, and picking among equal
            // clipped values via a strict `>` would just return the plateau's leftmost
            // point -- see `FrameEvidence`'s own doc comment.
            let lo = pos.saturating_sub(refine_span);
            let hi = pos + refine_span;
            let mut best_pos = pos;
            let mut best_raw = evidence.raw_score;
            let mut p = lo;
            while p <= hi {
                if let Some(e) = frame_score(samples, p, geo, trig) {
                    if e.raw_score > best_raw {
                        best_raw = e.raw_score;
                        best_pos = p;
                    }
                }
                p += grid_step.max(1);
            }

            if let Some((chr, advance)) = attempt_character(samples, best_pos, geo, trig, state) {
                state.locked_offset_idx = offset_idx;
                out.push_str(&chr);
                pos = best_pos + advance;
                continue;
            }
        }
        pos += grid_step;
    }
    state.pos = pos;
    out
}

/// Decodes a whole-buffer RTTY signal at a known mark frequency/baud rate
/// into text -- see this module's own doc comment for the real, honest
/// scope this covers (a clean/aligned signal, real async start-bit edge
/// detection). For a live/streaming pipeline processing audio in
/// separate chunks over time, use `RttyDecoder` instead -- this
/// whole-buffer form has no persistent state, so calling it once per
/// chunk would lose synchronization at every chunk boundary that
/// doesn't happen to land between characters.
// [@ANCHOR: rtty_demodulate]
pub fn rtty_demodulate(samples: &[i16], mark_hz: f64, sample_rate: u32) -> String {
    let geo = FrameGeometry::new(sample_rate);
    let bank = TrigBank::new(
        mark_hz,
        sample_rate,
        geo.stop_window_len.max(geo.window_len),
    );
    let mut state = ScanState::default();
    rtty_scan(samples, &geo, &bank, &mut state, false)
}

/// Stateful, incremental RTTY demodulator for a continuous audio stream
/// delivered across many separate `feed()` calls -- the same reason
/// `Psk31Decoder` exists for PSK31 (a plain whole-buffer decode per
/// chunk would reset framing state at every chunk boundary), but with a
/// real, RTTY-specific twist `Psk31Decoder` doesn't have: PSK31 decodes
/// one fixed-size symbol at a time, so "have I received enough samples
/// yet" is a simple length check. RTTY's start-bit framing is
/// asynchronous -- a real character can begin at any sample offset --
/// so `feed()` must be able to recognize "this looks like a real
/// start-bit edge, but I don't have enough buffered audio yet to
/// confirm the whole character" and wait for the next call rather than
/// either guessing wrong or skipping past the edge (see `rtty_scan`'s
/// own doc comment for the mechanism). One real, honest consequence:
/// the very last character of a burst can lag by up to one character's
/// own duration (~150ms at 45.45 baud) behind when its audio actually
/// finished arriving, appearing on the *next* `feed()` call instead --
/// not data loss, the same kind of small fixed latency `Psk31Decoder`
/// already has waiting for a full symbol.
///
/// Unlike the original design, this carries no separate presence-gate state at all -- the
/// frame-matched detector's own aggregate confidence threshold (see `frame_score`'s doc
/// comment) *is* the presence/confidence gate now, amplitude-invariant by construction, so
/// there's no floor to warm up, contaminate, or desynchronize from `ScanState`.
///
/// `geo`/`bank` are built once here, not per `feed()` call: `mark_hz`/`sample_rate` are fixed
/// for the life of a decoder, so `TrigBank::new`'s `sin`/`cos` table construction (9 candidate
/// frequencies -- see `AFC_OFFSETS_HZ`'s own doc comment -- at a real 48kHz/45.45-baud geometry)
/// would otherwise repeat on every single `feed()` call from a live pipeline -- for
/// `digital_decoder.rs`'s own ~20ms callback cadence, that is real, avoidable, per-callback
/// work for a value that never changes after construction.
pub struct RttyDecoder {
    sample_rate: u32,
    pending_samples: Vec<i16>,
    state: ScanState,
    geo: FrameGeometry,
    bank: TrigBank,
}

impl RttyDecoder {
    // [@ANCHOR: RttyDecoder::new]
    pub fn new(mark_hz: f64, sample_rate: u32) -> Self {
        let geo = FrameGeometry::new(sample_rate);
        let bank = TrigBank::new(
            mark_hz,
            sample_rate,
            geo.stop_window_len.max(geo.window_len),
        );
        Self {
            sample_rate,
            pending_samples: Vec::new(),
            state: ScanState::default(),
            geo,
            bank,
        }
    }

    /// A cheap, AFC-free decoder locked to exactly `mark_hz` -- no +/-40Hz search, one
    /// `TrigTables` instead of nine. Built for `digital_decoder.rs`'s wide-passband RTTY
    /// channel-scanning bank: running dozens of full-AFC `RttyDecoder::new` instances
    /// continuously (one per scan candidate) was measured, via `probe_rtty_bank_scaling_cost`,
    /// to cost up to ~48% of the real-time audio budget for a 32-candidate bank alone -- the 9x
    /// per-position correlator cost AFC adds, multiplied across every candidate. A bank of
    /// `new_fixed` decoders is used only to find *which* candidate frequency has a real signal
    /// (by decoded character count -- see the bank's own confidence/persistence logic); once one
    /// wins, the caller promotes it to a real `RttyDecoder::new` at that frequency for the actual
    /// decode, recovering the full +/-40Hz AFC reach exactly where it's needed instead of paying
    /// for it everywhere. A `new_fixed` decoder's own narrower frequency-mismatch tolerance
    /// (`frame_score`'s single-table correlator, no search) is fine for this detection-only role:
    /// `fixed_decoder_char_yield_vs_mismatch` measures a clean signal decoding perfectly out to
    /// 25Hz mismatch, still recovering the majority of characters at 30-35Hz, and only failing
    /// completely at 40Hz -- comfortably past the bank's own 40Hz candidate spacing (a worst-case
    /// 20Hz midpoint mismatch), so real signal always registers strongly enough on its nearest
    /// candidate(s) to win the argmax, even before AFC ever gets involved.
    // [@ANCHOR: RttyDecoder::new_fixed]
    pub fn new_fixed(mark_hz: f64, sample_rate: u32) -> Self {
        let geo = FrameGeometry::new(sample_rate);
        let bank = TrigBank::new_with_offsets(
            &FIXED_OFFSET_HZ,
            mark_hz,
            sample_rate,
            geo.stop_window_len.max(geo.window_len),
        );
        Self {
            sample_rate,
            pending_samples: Vec::new(),
            state: ScanState::new(0),
            geo,
            bank,
        }
    }

    /// The AFC search's own current best estimate of real off-air frequency offset from this
    /// decoder's configured `mark_hz`, in Hz -- whichever candidate offset in `self.bank.offsets`
    /// won the most recently decoded character (`rtty_scan`'s own doc comment covers the
    /// tracking mechanism), or 0.0 if nothing has decoded yet. Reads the offset back from the
    /// bank this decoder was actually built with, not the global `AFC_OFFSETS_HZ` constant --
    /// `new_fixed`'s single-entry bank has its own, different offset list (see `TrigBank`'s own
    /// doc comment for the bug this avoids). A real, honest estimate a caller (e.g.
    /// `digital_decoder.rs`'s own `DigitalDecode::audio_offset_hz`) can report instead of
    /// assuming zero mistuning.
    // [@ANCHOR: RttyDecoder::locked_frequency_offset_hz]
    pub fn locked_frequency_offset_hz(&self) -> f64 {
        self.bank.offsets[self.state.locked_offset_idx]
    }

    /// Feeds newly-arrived audio samples in; returns any characters
    /// that completed decoding as a result.
    // [@ANCHOR: RttyDecoder::feed]
    pub fn feed(&mut self, samples: &[i16]) -> String {
        // `rtty_scan` itself already guards a degenerate `window_len ==
        // 0` (a `sample_rate` too small relative to RTTY_BAUD for even
        // one whole sample per bit period -- e.g. `sample_rate: 0` from
        // a misconfigured/misdetected audio device) by returning
        // immediately without ever advancing `state.pos`. That's correct
        // for `rtty_demodulate`'s one-shot whole-buffer case, but here it
        // means `state.pos` (the trim threshold's own trigger below)
        // would never advance either -- so this decoder's
        // `pending_samples` buffer would grow completely unbounded for
        // the lifetime of the stream instead of merely failing to decode
        // anything, a real memory-leak-shaped bug distinct from (and
        // milder than, but the same root cause as) `Ft8Decoder::new`'s
        // own zero-sample-rate hang fixed in ft8.rs this same pass.
        // Bail out before ever accumulating samples we can never usefully
        // scan.
        if (self.sample_rate as f64 / RTTY_BAUD).round() as usize == 0 {
            return String::new();
        }
        self.pending_samples.extend_from_slice(samples);
        let out = rtty_scan(
            &self.pending_samples,
            &self.geo,
            &self.bank,
            &mut self.state,
            true,
        );

        // Trim everything already scanned once it's built up a real
        // amount, so a long-running stream doesn't grow this buffer
        // without bound -- matching the same "persistent but bounded"
        // discipline `digital_decoder.rs`'s own `WsprDecimator` uses for
        // its remainder buffer, just at a coarser threshold since RTTY's
        // own lookahead margin (one character, ~150ms) is far smaller
        // than WSPR's.
        let trim_threshold = 10 * self.sample_rate as usize; // ~10s headroom
        if self.state.pos > trim_threshold {
            self.pending_samples.drain(..self.state.pos);
            self.state.pos = 0;
        }

        out
    }
}

/// Sign-flip rate and run-length histogram of the mark/space ratio at one candidate frequency,
/// hopped through in non-overlapping ~1-bit-period windows -- a cheap, decode-free RTTY
/// *presence* signature, not a decoder. Per Bruce's own real-time direction ("Look at ways of
/// recognizing RTTY cheaply without a full decoder... characterize the signal and then bring in
/// the decoders that make sense"). No AFC search, no persistence gating, no character framing
/// at all -- ~23us/call for a 960-sample chunk at one candidate (measured, see
/// `probe_rtty_signature_separates_signal_noise_and_tone` below), a real order of magnitude
/// cheaper than even `RttyDecoder::new_fixed`'s own per-character framing.
///
/// **Real, measured, honest finding, not the hypothesis this was built to test**: the original
/// reasoning (real RTTY's own bit-period keying should cluster run lengths at 1-3 hops, distinct
/// from noise's near-random-walk ~50% flip rate) does NOT hold up against measurement.
/// `probe_rtty_signature_separates_signal_noise_and_tone` measured real RTTY at a 0.531 sign
/// flip rate against real Gaussian noise at 0.473 -- both close to the 0.5 a symmetric random
/// process produces, with similar-shaped run-length histograms, not the sharp separation
/// expected. In hindsight this makes sense: arbitrary text's own data bits are themselves close
/// to random content, so a real character's bit-level MARK/SPACE sequence isn't meaningfully
/// more "clustered" than noise at this coarse, single-hop-at-a-time granularity -- only the
/// *framing structure* (fixed start/stop bit positions, a stable 45.45-baud clock) actually
/// distinguishes real RTTY from noise, and this signature doesn't look at either. The one case
/// this DOES cleanly separate is a steady, unkeyed carrier (idle MARK: 0.0 flip rate, one giant
/// run) from anything keyed -- a real, if narrower, win. A signature that actually separates
/// signal from noise would need to test for bit-clock periodicity (do flips recur near integer
/// multiples of one hop width, not just how often they occur) rather than raw flip rate --
/// genuinely more work, not attempted here. Kept and reported honestly rather than deleted: a
/// negative result that took real measurement to find is exactly the kind of thing worth
/// keeping in the code where the next person will actually see it before re-deriving it.
///
/// Deliberately NOT wired into `RttyDecoder`, `rtty_scan`, or any pipeline: this is the
/// measurement step, not a shipped gating policy, and per the finding above, not yet a policy
/// that would work if it were wired in.
#[derive(Debug, Clone, PartialEq)]
struct RttySignature {
    /// Fraction of adjacent hops whose sign differs, in `[0.0, 1.0]`.
    sign_flip_rate: f64,
    /// `run_length_histogram[i]` is the count of runs exactly `i + 1` hops long, for `i` up to
    /// `RUN_LENGTH_HISTOGRAM_BUCKETS - 1`; the last bucket accumulates every run of at least
    /// that length instead of growing the histogram unboundedly for the idle-MARK case.
    run_length_histogram: [usize; RttySignature::RUN_LENGTH_HISTOGRAM_BUCKETS],
}

impl RttySignature {
    const RUN_LENGTH_HISTOGRAM_BUCKETS: usize = 8;
}

/// Computes an `RttySignature` for `samples` at `candidate_hz`. `None` if there isn't enough
/// audio for at least two hops (nothing to compare) or the sample rate is too low for even one
/// whole sample per bit period (same degenerate case `RttyDecoder::feed` itself guards).
// [@ANCHOR: characterize_rtty_signature]
fn characterize_rtty_signature(
    samples: &[i16],
    candidate_hz: f64,
    sample_rate: u32,
) -> Option<RttySignature> {
    let hop_len = (sample_rate as f64 / RTTY_BAUD).round() as usize;
    if hop_len == 0 {
        return None;
    }
    let space_hz = candidate_hz + RTTY_DEFAULT_SHIFT_HZ;
    let trig = TrigTables::new(candidate_hz, space_hz, sample_rate, hop_len);

    let mut signs = Vec::new();
    let mut pos = 0usize;
    while pos + hop_len <= samples.len() {
        let ratio = trig.signed_ratio_db(samples, pos, hop_len)?;
        signs.push(ratio >= 0.0);
        pos += hop_len;
    }
    if signs.len() < 2 {
        return None;
    }

    let mut flips = 0usize;
    let mut histogram = [0usize; RttySignature::RUN_LENGTH_HISTOGRAM_BUCKETS];
    let mut run_len = 1usize;
    for i in 1..signs.len() {
        if signs[i] != signs[i - 1] {
            flips += 1;
            histogram[(run_len - 1).min(RttySignature::RUN_LENGTH_HISTOGRAM_BUCKETS - 1)] += 1;
            run_len = 1;
        } else {
            run_len += 1;
        }
    }
    histogram[(run_len - 1).min(RttySignature::RUN_LENGTH_HISTOGRAM_BUCKETS - 1)] += 1;

    Some(RttySignature {
        sign_flip_rate: flips as f64 / (signs.len() - 1) as f64,
        run_length_histogram: histogram,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // Tests [@ANCHOR: char_to_baudot]
    // Tests [@ANCHOR: text_to_framed_bits]
    // Tests [@ANCHOR: push_framed_char]
    // Tests [@ANCHOR: rtty_modulate]
    // Tests [@ANCHOR: window_mark_space_energy]
    // Tests [@ANCHOR: rtty_scan]
    // Tests [@ANCHOR: TrigTables::new]
    // Tests [@ANCHOR: TrigTables::signed_ratio_db]
    // Tests [@ANCHOR: TrigTables::window_evidence]
    // Tests [@ANCHOR: frame_score]
    // Tests [@ANCHOR: attempt_character]
    // Tests [@ANCHOR: rtty_demodulate]
    // Tests [@ANCHOR: RttyDecoder::new]
    // Tests [@ANCHOR: pre_is_persistently_mark]
    fn round_trips_a_real_cq_call_through_letters_and_figures_shifts() {
        let text = "CQ CQ DE K6BP K6BP 599 599 PSE K";
        let sample_rate = 48000u32;
        let mono = rtty_modulate(text, RTTY_DEFAULT_MARK_HZ, sample_rate);
        let decoded = rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert_eq!(decoded, text);
    }

    #[test]
    fn round_trips_at_a_different_mark_frequency() {
        // Confirms the mark_hz parameter is actually used end to end,
        // not hardcoded anywhere in the pipeline -- the same "prove the
        // parameter matters" discipline psk31.rs's own tests apply.
        let text = "TEST 123";
        let sample_rate = 48000u32;
        let mono = rtty_modulate(text, 1500.0, sample_rate);
        let decoded = rtty_demodulate(&mono, 1500.0, sample_rate);
        assert_eq!(decoded, text);
    }

    #[test]
    fn every_ltrs_letter_and_every_figs_digit_round_trips() {
        let sample_rate = 48000u32;
        let letters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mono = rtty_modulate(letters, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert_eq!(
            rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate),
            letters
        );

        let digits = "1234567890";
        let mono = rtty_modulate(digits, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert_eq!(
            rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate),
            digits
        );
    }

    /// A real negative-case check, matching this codebase's own
    /// established convention (`psk31.rs`, `Psk31Decoder`'s own tests) of
    /// never assuming noise is harmless without checking directly: pure
    /// random noise must not panic, hang, or produce an unbounded output
    /// string. **Real behavior change from the original design**: the old
    /// `rtty_demodulate` deliberately ran with no presence gate at all
    /// (`gate: None`), on the reasoning that it's a one-shot function
    /// exercised only against known-clean synthesized signals in this
    /// crate's own tests. The rewritten frame-matched detector has no
    /// equivalent opt-out -- `frame_score`'s own threshold applies
    /// unconditionally to both `rtty_demodulate` and `RttyDecoder::feed`
    /// -- so `rtty_demodulate` now gets the same amplitude-invariant
    /// noise rejection for free, not just a looser "fewer than 1000
    /// characters" sanity bound.
    #[test]
    fn pure_noise_does_not_panic_or_hang() {
        let sample_rate = 48000u32;
        let mut state: u32 = 0xDEADBEEF;
        let noise: Vec<i16> = (0..sample_rate as usize * 2)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state % 4000) as i16 - 2000
            })
            .collect();
        let decoded = rtty_demodulate(&noise, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert!(decoded.len() < 1000, "noise produced an implausibly large amount of decoded text -- likely a framing/advance bug, not just expected occasional false characters");
    }

    #[test]
    // Tests [@ANCHOR: RttyDecoder::feed]
    fn feed_does_not_grow_its_buffer_unboundedly_at_a_degenerate_sample_rate() {
        // Bug found and fixed by this pass: rtty_scan's own `window_len
        // == 0` guard (correct for the one-shot rtty_demodulate case)
        // meant `state.pos` never advances at a sample_rate too low
        // relative to RTTY_BAUD to produce even one sample per bit
        // period (0 is the clearest case -- a plausible real
        // misconfiguration, e.g. a failed audio-device negotiation) --
        // so RttyDecoder::feed's own pending_samples buffer, gated only
        // on state.pos ever exceeding a threshold, would accumulate
        // every fed sample forever with no bound, for the life of the
        // stream. Confirm the fix directly: feeding a real amount of
        // audio into a sample_rate: 0 decoder must not leave a large,
        // ever-growing buffer behind.
        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, 0);
        for _ in 0..50 {
            let out = decoder.feed(&[0i16; 1000]);
            assert_eq!(out, "", "a zero sample rate can never decode anything");
        }
        assert_eq!(
            decoder.pending_samples.len(),
            0,
            "feed() must not accumulate samples it can structurally never scan"
        );
    }

    /// The real measurement behind `PresenceGate`: 10s of synthetic PRNG
    /// noise, fed through `RttyDecoder` in real 960-sample pipeline
    /// chunks (`digital_decoder.rs`'s own real per-callback chunk size),
    /// produced 44 spurious characters -- a steady stream -- before the
    /// gate existed. This is the discriminating check that finding was
    /// built to satisfy: with the gate active, this same noise must stay
    /// effectively silent, not just "fewer than 1000 characters" the way
    /// `pure_noise_does_not_panic_or_hang` (a looser, pre-gate,
    /// whole-buffer sanity bound) still tolerates.
    #[test]
    // Tests [@ANCHOR: RttyDecoder::feed]
    fn rtty_decoder_stays_effectively_silent_against_10_seconds_of_real_noise_in_real_pipeline_chunks(
    ) {
        let sample_rate = 48000u32;
        let mut state: u32 = 0xDEADBEEF;
        let noise: Vec<i16> = (0..sample_rate as usize * 10)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state % 4000) as i16 - 2000
            })
            .collect();
        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut total = String::new();
        for chunk in noise.chunks(960) {
            total.push_str(&decoder.feed(chunk));
        }
        assert!(total.len() <= 2, "expected the presence gate to keep 10s of real noise effectively silent (0-2 stray characters, not the 44 measured with no gate at all), got {} characters: {:?}", total.len(), total);
    }

    /// A wider-amplitude version of the noise test above (+/-20000
    /// instead of +/-2000, roughly 61% of full i16 scale) -- guards
    /// against the same class of absolute-energy-floor freeze this
    /// gate's own doc comment already fixed once, just at a higher
    /// trigger level. **Honest history**: an earlier version of this
    /// test used a noise-generator expression (`(state % N) as i16 -
    /// N/2` with `N` large enough that `state % N` exceeds i16 range)
    /// that silently wraps before the subtraction, producing a debug-
    /// mode integer-overflow panic rather than a real measurement --
    /// that panic was mistaken for a confirmed floor deadlock during
    /// this module's own gate-redesign investigation. Fixed by widening
    /// the intermediate arithmetic to `i32` before the final `as i16`
    /// cast. With the bug fixed, this test passes against the proven
    /// gate design as-is -- the deadlock is *not* reproduced at
    /// +/-20000, so the real deadlock boundary above this amplitude
    /// (if any) remains unmeasured, not a confirmed vulnerability.
    #[test]
    fn rtty_decoder_stays_effectively_silent_against_louder_noise_that_previously_deadlocked_the_floor(
    ) {
        let sample_rate = 48000u32;
        let mut state: u32 = 0xC0FFEE;
        let noise: Vec<i16> = (0..sample_rate as usize * 10)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                ((state % 40000) as i32 - 20000) as i16
            })
            .collect();
        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut total = String::new();
        for chunk in noise.chunks(960) {
            total.push_str(&decoder.feed(chunk));
        }
        assert!(total.len() <= 2, "expected the presence gate to stay effectively silent even against louder noise that would have deadlocked the pre-fix floor, got {} characters: {:?}", total.len(), total);
    }

    /// This test used to be `#[ignore]`d: the old `PresenceGate` design's absolute-energy floor
    /// had a genuine, measured, noise-texture-dependent fragility above ~65% of full i16 scale
    /// (bisecting amplitude by hand found a non-monotonic pass/fail pattern, not a clean
    /// cliff -- some noise realizations at a given amplitude triggered spurious decodes, some
    /// didn't), because an *absolute* energy floor is fundamentally amplitude-dependent no
    /// matter how it's tuned. Un-ignored now that the detector is amplitude-invariant by
    /// construction (`frame_score`/`pre_is_persistently_mark`, both built from signed energy
    /// *ratios* and sustained-tone persistence rather than any absolute floor) -- this test is
    /// this rewrite's own concrete acceptance criterion for that fragility being real closed,
    /// not just individually tuned to pass this one noise draw.
    #[test]
    fn rtty_decoder_stays_effectively_silent_against_noise_at_near_full_i16_scale() {
        let sample_rate = 48000u32;
        let mut state: u32 = 0xC0FFEE;
        let noise: Vec<i16> = (0..sample_rate as usize * 10)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                ((state % 64000) as i32 - 32000) as i16
            })
            .collect();
        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut total = String::new();
        for chunk in noise.chunks(960) {
            total.push_str(&decoder.feed(chunk));
        }
        assert!(total.len() <= 2, "expected the presence gate to stay effectively silent even against near-full-scale noise, got {} characters: {:?}", total.len(), total);
    }

    /// The other half of the same real measurement: the presence gate
    /// must not cost real signal detection. A genuine synthesized RTTY
    /// signal (the same real amplitude `rtty_modulate` always produces)
    /// must still decode correctly through `RttyDecoder`, confirming
    /// `FRAME_SCORE_THRESHOLD_DB` has real margin rather than being tuned
    /// so aggressively it rejects real signal along with the noise.
    #[test]
    fn the_presence_gate_does_not_reject_a_real_signal() {
        let text = "CQ CQ DE K6BP PSE K";
        let sample_rate = 48000u32;
        // Real leading idle-MARK carrier, matching real RTTY operating
        // practice (a transmitter keys up and sends idle MARK briefly
        // before the first character) -- also, not incidentally, gives
        // `pre_is_persistently_mark` something real to confirm before the
        // actual message starts, the same way a real receiver already
        // listening to a quiet band before a transmission begins would.
        let mut mono = idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.4);
        mono.extend(rtty_modulate(text, RTTY_DEFAULT_MARK_HZ, sample_rate));
        mono.extend(idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.05));

        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut got = String::new();
        for chunk in mono.chunks(960) {
            got.push_str(&decoder.feed(chunk));
        }
        assert_eq!(
            got, text,
            "the presence gate must not reject a real, full-amplitude RTTY signal"
        );
    }

    /// A permanent regression test for the AFC (automatic frequency control) search
    /// (`AFC_OFFSETS_HZ`/`TrigBank`/`best_frame_evidence`, see their own doc comments and
    /// `rtty_scan`'s for the real mechanism): synthesizes a signal at a real off-air-mistuned
    /// mark frequency (`RTTY_DEFAULT_MARK_HZ + 25.0`, comfortably inside the +/-40Hz search
    /// range but well past the ~20Hz point a fixed-frequency correlator was measured
    /// (`tests/rtty_processing_gain_harness.rs`) losing real signal at) and confirms
    /// `RttyDecoder`, still configured for the *nominal* `RTTY_DEFAULT_MARK_HZ`, decodes it
    /// correctly and reports a locked offset in the right direction and roughly the right
    /// magnitude -- not just "still decodes" but "the frequency estimate itself is real."
    #[test]
    // Tests [@ANCHOR: TrigBank::new]
    // Tests [@ANCHOR: best_frame_evidence]
    // Tests [@ANCHOR: RttyDecoder::locked_frequency_offset_hz]
    fn rtty_decoder_tracks_a_real_off_air_frequency_offset() {
        let text = "CQ CQ DE K6BP PSE K";
        let sample_rate = 48000u32;
        let actual_mark_hz = RTTY_DEFAULT_MARK_HZ + 25.0;
        let mut mono = idle_mark_audio(actual_mark_hz, sample_rate, 0.4);
        mono.extend(rtty_modulate(text, actual_mark_hz, sample_rate));
        mono.extend(idle_mark_audio(actual_mark_hz, sample_rate, 0.05));

        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut got = String::new();
        for chunk in mono.chunks(960) {
            got.push_str(&decoder.feed(chunk));
        }
        assert_eq!(
            got, text,
            "a real 25Hz-mistuned signal must still decode via the AFC search"
        );
        let locked = decoder.locked_frequency_offset_hz();
        assert!(
            (10.0..=40.0).contains(&locked),
            "expected the AFC search to lock onto a positive offset close to the real 25Hz \
             mistuning, got {locked}Hz"
        );
    }

    /// USOS (unshift-on-space) permanent regression test: a real transmission that shifts to
    /// FIGS for a digit, sends a SPACE, and then a LETTER *without ever sending an explicit
    /// unshift-to-LTRS code first* -- exactly the shape `text_to_framed_bits`' own automatic
    /// shift-insertion would never itself produce, but a real noisy/garbled transmission, or one
    /// this decoder starts listening to mid-FIGS-shift, can. Built directly via
    /// `push_framed_char` (bypassing that automatic insertion) so the bit sequence is real and
    /// exact, not simulated. Without USOS, the decoder would still be in FIGS shift when it hits
    /// the letter code and print `FIGS_CHARS`' own symbol for that code (`-` for `A`'s code)
    /// instead of the letter -- this asserts the real letter comes through instead.
    #[test]
    // Tests [@ANCHOR: attempt_character]
    // Tests [@ANCHOR: framed_bits_to_audio]
    fn unshift_on_space_recovers_a_letter_after_an_unsent_shift_code() {
        let (figs_one_code, _) = char_to_baudot('1').expect("'1' must be a real FIGS character");
        let (space_code, _) = char_to_baudot(' ').expect("space must be a real character");
        let (letter_a_code, _) = char_to_baudot('A').expect("'A' must be a real LTRS character");

        let mut bits = Vec::new();
        push_framed_char(&mut bits, CODE_FIGS_SHIFT);
        push_framed_char(&mut bits, figs_one_code);
        push_framed_char(&mut bits, space_code);
        push_framed_char(&mut bits, letter_a_code); // no CODE_LTRS_SHIFT before this

        let sample_rate = 48000u32;
        let mono = framed_bits_to_audio(&bits, RTTY_DEFAULT_MARK_HZ, sample_rate);
        let decoded = rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert_eq!(
            decoded, "1 A",
            "USOS must reset to LTRS shift on the space, so the un-shifted letter code decodes \
             as 'A', not FIGS_CHARS' own '-' for that same code"
        );
    }

    /// "Sensible USOS" regression test: a missed LTRS shift followed by real FIGS-shift
    /// punctuation (not digits, not a space) -- neither plain USOS above (no space appears)
    /// nor an explicit shift code (never sent) rescues this. Three non-digit FIGS symbols in a
    /// row is the implausible-run trigger (`attempt_character`'s own doc comment): the first
    /// two are emitted as their (wrong) FIGS interpretation since there isn't yet enough
    /// evidence, the third completes the pattern and is corrected to its LTRS interpretation in
    /// the same call, and every following un-shifted character decodes correctly from then on.
    #[test]
    // Tests [@ANCHOR: attempt_character]
    // Tests [@ANCHOR: framed_bits_to_audio]
    fn implausible_figs_run_pre_emptively_unshifts_without_a_space() {
        let figs_code = |c: char| -> u8 {
            FIGS_CHARS
                .iter()
                .position(|&f| f == c)
                .expect("must be a real FIGS_CHARS entry") as u8
        };
        let (letter_a_code, _) = char_to_baudot('A').expect("'A' must be a real LTRS character");

        let mut bits = Vec::new();
        push_framed_char(&mut bits, CODE_FIGS_SHIFT);
        push_framed_char(&mut bits, figs_code('-')); // 1st non-digit FIGS symbol: still emitted as FIGS
        push_framed_char(&mut bits, figs_code('$')); // 2nd: still not enough evidence yet
        push_framed_char(&mut bits, figs_code('\'')); // 3rd: completes the run -- corrected to LTRS
        push_framed_char(&mut bits, letter_a_code); // no CODE_LTRS_SHIFT before this either

        let sample_rate = 48000u32;
        let mono = framed_bits_to_audio(&bits, RTTY_DEFAULT_MARK_HZ, sample_rate);
        let decoded = rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert_eq!(
            decoded, "-$JA",
            "expected the first two FIGS symbols unchanged, the third corrected to its LTRS \
             interpretation ('\\'' is code 11, LTRS_CHARS[11] is 'J') the moment the implausible \
             run is recognized, and the un-shifted letter after it decoding correctly as 'A'"
        );
    }

    /// `figs_non_digit_run` (the implausible-run counter above) is persistent `ScanState`,
    /// carried across `RttyDecoder::feed()` calls exactly like `current_figs` already is -- but
    /// the test above only exercises it through `rtty_demodulate`'s one-shot, whole-buffer path
    /// with a fresh `ScanState`, never through a real streaming decoder split across multiple
    /// small, irregular `feed()` calls the way `streaming_decoder_matches_whole_buffer_decode_
    /// when_fed_in_small_irregular_chunks` already proves `current_figs` survives. This closes
    /// that real gap: the exact same bit sequence, through `RttyDecoder::feed()` in small
    /// chunks whose boundaries fall in the middle of characters (never aligned to a character or
    /// bit boundary), must still recognize the run and correct the same way.
    #[test]
    // Tests [@ANCHOR: attempt_character]
    // Tests [@ANCHOR: framed_bits_to_audio]
    // Tests [@ANCHOR: RttyDecoder::feed]
    fn implausible_figs_run_survives_across_streaming_feed_chunk_boundaries() {
        let figs_code = |c: char| -> u8 {
            FIGS_CHARS
                .iter()
                .position(|&f| f == c)
                .expect("must be a real FIGS_CHARS entry") as u8
        };
        let (letter_a_code, _) = char_to_baudot('A').expect("'A' must be a real LTRS character");

        let mut bits = Vec::new();
        push_framed_char(&mut bits, CODE_FIGS_SHIFT);
        push_framed_char(&mut bits, figs_code('-'));
        push_framed_char(&mut bits, figs_code('$'));
        push_framed_char(&mut bits, figs_code('\''));
        push_framed_char(&mut bits, letter_a_code);

        let sample_rate = 48000u32;
        let mono = framed_bits_to_audio(&bits, RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut got = String::new();
        // Deliberately awkward, non-bit-aligned chunk size (137 samples, not a multiple of one
        // bit period at 48kHz/45.45 baud, ~1056 samples/bit) -- the same "prove real chunk
        // boundaries don't matter" discipline `streaming_decoder_matches_whole_buffer_decode_
        // when_fed_in_small_irregular_chunks` already uses.
        for chunk in mono.chunks(137) {
            got.push_str(&decoder.feed(chunk));
        }
        got.push_str(&decoder.feed(&idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.05)));
        assert_eq!(
            got, "-$JA",
            "the implausible-run counter must survive real feed() chunk boundaries the same way \
             current_figs already does, and produce the identical correction"
        );
    }

    /// Pins a real, known, disclosed false-positive of the "sensible USOS" implausible-run
    /// check above: a *legitimate* transmission that happens to send 3+ consecutive non-digit
    /// FIGS symbols (rare in real ham traffic -- see `attempt_character`'s own doc comment for
    /// why the asymmetric rule was reasoned to be safe on that basis -- but not impossible, e.g.
    /// a run of punctuation) gets its 3rd symbol mis-corrected to LTRS exactly the same way a
    /// genuine missed shift would. This is a deliberate, bounded trade-off, not an undiscovered
    /// bug: pinning the exact behavior here means a future reader (or a future change to the
    /// run-length threshold) sees this as a known cost to weigh, not a surprise.
    #[test]
    // Tests [@ANCHOR: attempt_character]
    // Tests [@ANCHOR: framed_bits_to_audio]
    fn implausible_figs_run_check_false_triggers_on_legitimate_punctuation() {
        let figs_code = |c: char| -> u8 {
            FIGS_CHARS
                .iter()
                .position(|&f| f == c)
                .expect("must be a real FIGS_CHARS entry") as u8
        };

        let mut bits = Vec::new();
        push_framed_char(&mut bits, CODE_FIGS_SHIFT);
        push_framed_char(&mut bits, figs_code('?'));
        push_framed_char(&mut bits, figs_code('!'));
        push_framed_char(&mut bits, figs_code('?')); // legitimately meant as another '?'

        let sample_rate = 48000u32;
        let mono = framed_bits_to_audio(&bits, RTTY_DEFAULT_MARK_HZ, sample_rate);
        let decoded = rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert_eq!(
            decoded, "?!B",
            "known trade-off: the 3rd legitimate FIGS symbol is mis-corrected to its LTRS \
             interpretation ('?' is code 25, LTRS_CHARS[25] is 'B') by the same implausible-run \
             check that recovers a real missed shift -- if this assertion ever needs to change, \
             it means the run-length threshold or its safety reasoning changed, not that this \
             is a regression to silently accept"
        );
    }

    /// Diagnostic (not a pass/fail assertion, same convention as `digital_decoder.rs`'s own
    /// `probe_*_bank_scaling_cost` tests): measures how many real characters a `new_fixed`
    /// (AFC-free) decoder actually recovers as real off-air mistuning grows, to pick a real,
    /// measured candidate spacing for `digital_decoder.rs`'s wide-passband RTTY bank instead of
    /// guessing one. The bank's own two-stage design only needs `new_fixed` candidates to
    /// recover *enough* characters to win the argmax over their neighbors and noise -- not a
    /// perfect decode, since the winner gets promoted to a real full-AFC `RttyDecoder` anyway.
    /// Run with: `cargo test --release fixed_decoder_char_yield_vs_mismatch -- --nocapture --ignored`
    #[test]
    #[ignore]
    // Tests [@ANCHOR: RttyDecoder::new_fixed]
    // Tests [@ANCHOR: TrigBank::new_with_offsets]
    // Tests [@ANCHOR: ScanState::new]
    fn fixed_decoder_char_yield_vs_mismatch() {
        // Real, found gap (an `advisor` consult flagged this before it shipped unverified): the
        // original version of this test only ever ran at 48kHz, but `digital_decoder.rs`'s own
        // wide-passband RTTY bank feeds its `new_fixed` candidates *decimated 12kHz* audio (4:1,
        // via `decimate_4x_box_average`) to cut correlator cost -- a decision justified purely
        // by a cost benchmark (`probe_rtty_bank_scaling_cost`) that never checked whether
        // detection still works at the lower rate at all. Correlator resolution depends on
        // window *duration* (~22ms either way, same number of bit periods), not raw sample
        // count, so decimation should be free in principle -- but "should" is exactly what the
        // periodic-ramp probe input and the repeated-noise-buffer bug both also said, and both
        // were wrong. Measuring both rates side by side here is the real check.
        let text = "CQ CQ DE K6BP CQ CQ DE K6BP CQ CQ DE K6BP PSE K";
        for sample_rate in [48000u32, 12000u32] {
            for mismatch in [0.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 40.0] {
                let actual_mark_hz = RTTY_DEFAULT_MARK_HZ + mismatch;
                let modulation_rate = 48000u32;
                let mut mono = idle_mark_audio(actual_mark_hz, modulation_rate, 0.4);
                mono.extend(rtty_modulate(text, actual_mark_hz, modulation_rate));
                mono.extend(idle_mark_audio(actual_mark_hz, modulation_rate, 0.05));
                let mono = if sample_rate == modulation_rate {
                    mono
                } else {
                    // A real, whole-crate-boundary reason this doesn't just call
                    // `decimate_4x_box_average` from `ham_digital_modes::wspr_sync`: that
                    // function is real and correct, but pulling in the `wspr_sync` module
                    // (and its own FFT-planning dependencies) into an rtty.rs-only test for one
                    // four-sample average is real, avoidable coupling across this crate's own
                    // module boundaries -- the identical box-average computation, inlined.
                    mono.chunks_exact(4)
                        .map(|c| (c.iter().map(|&s| s as i32).sum::<i32>() / 4) as i16)
                        .collect()
                };

                let mut decoder = RttyDecoder::new_fixed(RTTY_DEFAULT_MARK_HZ, sample_rate);
                let mut got = String::new();
                let chunk_len = (sample_rate as usize * 960) / modulation_rate as usize;
                for chunk in mono.chunks(chunk_len.max(1)) {
                    got.push_str(&decoder.feed(chunk));
                }
                let sent_chars = text.chars().count();
                let got_chars = got.chars().count();
                eprintln!(
                    "sample_rate={sample_rate:5}  mismatch={mismatch:5.1}Hz  sent={sent_chars:3}  got={got_chars:3}  text={got:?}"
                );
            }
        }
    }

    /// Real bug from the original `PresenceGate` design (kept as a permanent regression test,
    /// not history for its own sake): this deliberately has NO leading idle-MARK audio at all
    /// -- exactly the real scenario a short message starting the instant a receiver begins
    /// listening (or `RttyDecoder` starting up mid-transmission) produces. The old gate's
    /// `PRESENCE_WARMUP_OBSERVATIONS` requirement silently rejected this signal's own start-bit
    /// edge regardless of how strong it was, since its observation counter started at 0
    /// regardless of energy. Structurally impossible now: `pre_is_persistently_mark` (see its
    /// own doc comment) passes automatically whenever `edge_pos < window_len` -- there is no
    /// observation count to warm up at all, so a strong signal at the very start of the stream
    /// was never at risk of this specific bug's own failure mode in the rewritten detector.
    #[test]
    fn a_real_signal_with_no_leading_idle_audio_at_all_is_not_lost_to_warmup() {
        let text = "J";
        let sample_rate = 48000u32;
        let mut mono = rtty_modulate(text, RTTY_DEFAULT_MARK_HZ, sample_rate);
        // Trailing padding only, deliberately NOT leading padding -- this test is specifically
        // about behavior with no preceding audio at all, not the separate, already-documented
        // "last character needs trailing lookahead margin" behavior
        // (`the_final_character_lags_by_one_character_without_trailing_idle_audio_then_
        // arrives_once_more_audio_does` above already covers that one on its own).
        mono.extend(idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.05));

        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut got = String::new();
        for chunk in mono.chunks(2) {
            got.push_str(&decoder.feed(chunk));
        }
        assert_eq!(
            got, text,
            "a real signal present from the very first window must not be lost to the \
             presence gate's own warmup period"
        );
    }

    #[test]
    fn char_to_baudot_round_trips_every_table_entry() {
        for code in 0u8..32 {
            if code == CODE_LTRS_SHIFT || code == CODE_FIGS_SHIFT {
                continue;
            }
            let ltr = LTRS_CHARS[code as usize];
            if ltr != '\0' && ltr != ' ' && ltr != '\r' && ltr != '\n' {
                assert_eq!(char_to_baudot(ltr), Some((code, false)), "LTRS_CHARS[{code}] = {ltr:?} must encode back to the same code in letters shift");
            }
        }
    }

    /// Real MARK-tone "idle" audio, the same as a real RTTY transmitter
    /// sends between/after characters -- used below to give
    /// `RttyDecoder` the trailing lookahead margin it needs to confirm
    /// a message's final character (see `RttyDecoder`'s own doc comment
    /// for why that margin is real and necessary, not an oversight).
    /// Measures the mark/space ratio at the *real* production stepping
    /// (`scan_step = samples_per_bit / SCAN_GRID_PER_BIT`, heavily overlapping windows), not an
    /// idealized independent-window sampling -- overlap matters here because it inflates
    /// run-lengths (a real, longest-observed-run statistic is what a persistence-based gate
    /// actually needs, not a per-window percentile).
    fn ratios_at_scan_step(
        samples: &[i16],
        mark_hz: f64,
        space_hz: f64,
        sample_rate: u32,
    ) -> Vec<(bool, f64)> {
        let samples_per_bit = sample_rate as f64 / RTTY_BAUD;
        let scan_step = (samples_per_bit / SCAN_GRID_PER_BIT).max(1.0) as usize;
        let window_len = samples_per_bit.round() as usize;
        let mut out = Vec::new();
        let mut pos = 0;
        while pos + window_len <= samples.len() {
            let (is_mark, mark_e, space_e) = window_mark_space_energy(
                &samples[pos..pos + window_len],
                mark_hz,
                space_hz,
                sample_rate,
            );
            let ratio_db = 10.0 * (mark_e.max(space_e) / mark_e.min(space_e).max(1e-12)).log10();
            out.push((is_mark, ratio_db));
            pos += scan_step;
        }
        out
    }

    /// The real, original measurement this module's live detector is built on -- not just
    /// historical evidence, but the actual validated basis for `pre_is_persistently_mark` and
    /// `TrigTables::signed_ratio_db`'s amplitude-invariant design (see both their own doc
    /// comments): the MARK/SPACE energy ratio is amplitude-invariant and separates signal from
    /// noise cleanly across a full sweep (+/-2000 through +/-32000, i16 full scale) where an
    /// absolute-energy floor cannot -- a real idle-MARK tone sustains a run of confidently-high
    /// ratio windows for its *entire* duration regardless of amplitude, while real broadband
    /// noise at every amplitude tested never sustains more than a handful of consecutive
    /// windows above the same threshold.
    #[test]
    fn mark_space_ratio_separates_signal_from_noise_across_a_full_amplitude_sweep() {
        let sample_rate = 48000u32;
        let mark_hz = RTTY_DEFAULT_MARK_HZ;
        let space_hz = mark_hz + RTTY_DEFAULT_SHIFT_HZ;
        const RATIO_THRESHOLD_DB: f64 = 15.0;
        const MEASURED_NOISE_LONGEST_RUN: usize = 3;

        let idle = idle_mark_audio(mark_hz, sample_rate, 1.0);
        let idle_readings = ratios_at_scan_step(&idle, mark_hz, space_hz, sample_rate);
        let idle_run = longest_run_above(&idle_readings, RATIO_THRESHOLD_DB);
        assert_eq!(
            idle_run,
            idle_readings.len(),
            "a real idle-MARK tone should sustain a confident-ratio run for its entire duration"
        );

        for amp in [2000i32, 10000, 20000, 32000] {
            let mut state: u32 = 0xC0FFEE ^ (amp as u32);
            let noise: Vec<i16> = (0..sample_rate as usize)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    ((state % (2 * amp as u32)) as i32 - amp) as i16
                })
                .collect();
            let noise_readings = ratios_at_scan_step(&noise, mark_hz, space_hz, sample_rate);
            let noise_run = longest_run_above(&noise_readings, RATIO_THRESHOLD_DB);
            assert!(
                noise_run <= MEASURED_NOISE_LONGEST_RUN,
                "noise at amplitude {amp} produced a {noise_run}-window confident-ratio run, longer than the {MEASURED_NOISE_LONGEST_RUN} measured across the original sweep"
            );
        }
    }

    fn longest_run_above(readings: &[(bool, f64)], threshold: f64) -> usize {
        let mut longest = 0;
        let mut current = 0;
        for &(is_mark, ratio_db) in readings {
            if is_mark && ratio_db > threshold {
                current += 1;
                longest = longest.max(current);
            } else {
                current = 0;
            }
        }
        longest
    }

    fn idle_mark_audio(mark_hz: f64, sample_rate: u32, seconds: f64) -> Vec<i16> {
        let n = (sample_rate as f64 * seconds) as usize;
        let mut phase = 0.0f64;
        let phase_inc = std::f64::consts::TAU * mark_hz / sample_rate as f64;
        (0..n)
            .map(|_| {
                let s = (phase.sin() * i16::MAX as f64 * 0.9).round() as i16;
                phase += phase_inc;
                if phase > std::f64::consts::TAU {
                    phase -= std::f64::consts::TAU;
                }
                s
            })
            .collect()
    }

    #[test]
    fn streaming_decoder_matches_whole_buffer_decode_when_fed_in_small_irregular_chunks() {
        // The actual scenario RttyDecoder exists for: a live audio
        // pipeline delivering small chunks over many separate calls, at
        // boundaries that have nothing to do with character boundaries.
        // A real trailing idle-MARK tail is included (see
        // idle_mark_audio's own doc comment) so the streaming decoder's
        // real lookahead-margin requirement doesn't hide the very last
        // character -- without it, this test would need to know that
        // real, documented lag is expected and account for it instead.
        let text = "CQ CQ CQ DE K6BP TEST 1234 PSE K";
        let sample_rate = 8000u32;
        // Leading idle-MARK padding: see the_presence_gate_does_not_
        // reject_a_real_signal's own comment for why this is both
        // realistic and gives `pre_is_persistently_mark` something real
        // to confirm before the message itself starts.
        let mut mono = idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.4);
        mono.extend(rtty_modulate(text, RTTY_DEFAULT_MARK_HZ, sample_rate));
        mono.extend(idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.05));

        let expected = rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert_eq!(expected, text, "sanity check: whole-buffer decode of the padded signal must still match the original text");

        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        let mut got = String::new();
        // Irregular chunk sizes (including some smaller than one bit
        // period), deliberately not aligned to any character/bit
        // boundary -- the same adversarial chunking
        // psk31.rs's own streaming test uses.
        let mut chunk_sizes = [37, 200, 5, 811, 1, 400, 63].iter().cycle();
        let mut pos = 0;
        while pos < mono.len() {
            let n = (*chunk_sizes.next().unwrap()).min(mono.len() - pos);
            got.push_str(&decoder.feed(&mono[pos..pos + n]));
            pos += n;
        }
        assert_eq!(
            got, text,
            "streaming decode across irregular chunk boundaries must match the real message"
        );
    }

    #[test]
    fn the_final_character_lags_by_one_character_without_trailing_idle_audio_then_arrives_once_more_audio_does(
    ) {
        // Directly verifies the real, documented latency behavior in
        // RttyDecoder's own doc comment, rather than just asserting it
        // in prose: feeding exactly the modulated signal (no trailing
        // padding, matching what rtty_modulate itself produces) leaves
        // the last character un-decoded, because the streaming scanner
        // correctly refuses to commit to a character it doesn't yet
        // have full lookahead for -- then confirms it's not lost, just
        // delayed, by feeding a bit more idle audio afterward.
        let text = "DE K6BP K";
        let sample_rate = 8000u32;
        let mono = rtty_modulate(text, RTTY_DEFAULT_MARK_HZ, sample_rate);

        let mut decoder = RttyDecoder::new(RTTY_DEFAULT_MARK_HZ, sample_rate);
        // Feed real leading idle-MARK audio first, in its own feed() call, kept separate from
        // `got_from_signal_alone` below so this test's own "real proper prefix of text" check
        // is only testing the real lookahead-margin behavior this test exists for.
        let leading_idle_leftover =
            decoder.feed(&idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.4));
        assert_eq!(
            leading_idle_leftover, "",
            "leading idle-MARK audio should never itself decode to a character"
        );

        let got_from_signal_alone = decoder.feed(&mono);
        assert!(
            text.starts_with(&got_from_signal_alone) && got_from_signal_alone.len() < text.len(),
            "expected a real proper prefix of {text:?} (the last character withheld pending more lookahead), got {got_from_signal_alone:?}"
        );

        let tail = idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.05);
        let got_after_more_audio = decoder.feed(&tail);
        assert_eq!(
            format!("{got_from_signal_alone}{got_after_more_audio}"),
            text,
            "the withheld character must arrive, not be lost, once more audio confirms it"
        );
    }

    /// Real Gaussian noise (Box-Muller), the same generator `tests/rtty_processing_gain_
    /// harness.rs` already validated against this crate's own detector -- self-contained here
    /// rather than importing that test binary's own copy, since `rtty.rs`'s unit tests can't
    /// depend on a separate integration-test crate.
    fn gaussian_noise(n: usize, rms: f64, seed: u64) -> Vec<i16> {
        let mut state = seed ^ 0x9E3779B97F4A7C15;
        let mut next_u64 = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        (0..n)
            .map(|_| {
                let u1 = ((next_u64() >> 11) as f64 + 1.0) / (1u64 << 53) as f64;
                let u2 = (next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                let g = (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos();
                (g * rms).round().clamp(-32768.0, 32767.0) as i16
            })
            .collect()
    }

    /// The real evidence behind `characterize_rtty_signature`'s own doc comment: real RTTY
    /// (keyed, real character content) measured at 0.531 sign-flip-rate against Gaussian noise
    /// (no signal) at 0.473 -- NOT the sharp separation the signature was built to test for; see
    /// that function's own doc comment for the honest finding and why. The one case this DOES
    /// separate cleanly is a steady idle-MARK tone (a carrier present but not keyed -- 0.0 flip
    /// rate, one giant run) from anything keyed. `#[ignore]`d diagnostic, same convention as
    /// this crate's other `probe_*` tests -- prints real numbers, not a pass/fail assertion,
    /// since this is reporting a measurement, not enforcing a since-abandoned hypothesis. Also
    /// measures the real per-candidate cost (~23us/960-sample chunk measured here) that would
    /// matter if a smarter signature (one that tests bit-clock periodicity, per that same doc
    /// comment) is built later. Run with:
    /// `cargo test --release probe_rtty_signature_separates_signal_noise_and_tone -- --nocapture --ignored`
    #[test]
    #[ignore]
    fn probe_rtty_signature_separates_signal_noise_and_tone() {
        let sample_rate = 48000u32;
        let text = "CQ CQ DE K6BP CQ CQ DE K6BP CQ CQ DE K6BP PSE K";
        let mut rtty_signal = idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.4);
        rtty_signal.extend(rtty_modulate(text, RTTY_DEFAULT_MARK_HZ, sample_rate));

        let noise = gaussian_noise(rtty_signal.len(), 4000.0, 0xC0FFEE);
        let steady_tone = idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 3.0);

        for (label, samples) in [
            ("real RTTY traffic", &rtty_signal),
            ("Gaussian noise, no signal", &noise),
            ("steady idle-MARK tone (carrier, not keyed)", &steady_tone),
        ] {
            let sig = characterize_rtty_signature(samples, RTTY_DEFAULT_MARK_HZ, sample_rate)
                .expect("enough audio for at least two hops");
            eprintln!(
                "{label}: sign_flip_rate={:.3}  run_length_histogram(1..=8+)={:?}",
                sig.sign_flip_rate, sig.run_length_histogram
            );
        }

        // Per-candidate cost: the real number a gating-policy decision would need. Warmed up
        // (first call may pay one-time setup cost, same convention `digital_decoder.rs`'s own
        // `probe_*_bank_scaling_cost` tests use) before the timed measurement.
        let chunk = gaussian_noise(960, 4000.0, 0xBEEF);
        let _ = characterize_rtty_signature(&chunk, RTTY_DEFAULT_MARK_HZ, sample_rate);
        let n_trials = 200;
        let start = std::time::Instant::now();
        for _ in 0..n_trials {
            let _ = characterize_rtty_signature(&chunk, RTTY_DEFAULT_MARK_HZ, sample_rate);
        }
        let per_call_us = start.elapsed().as_micros() as f64 / n_trials as f64;
        eprintln!(
            "characterize_rtty_signature: {per_call_us:.2}us/call for a 960-sample (20ms) chunk \
             at one candidate frequency"
        );
    }
}
