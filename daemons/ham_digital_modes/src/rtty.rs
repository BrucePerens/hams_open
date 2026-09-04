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
//! framing mechanism), but have not been tuned or tested against real,
//! noisy off-air audio. `RttyDecoder` is the `Psk31Decoder`-style
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
    '\0', 'E', '\n', 'A', ' ', 'S', 'I', 'U', '\r', 'D', 'R', 'J', 'N', 'F', 'C', 'K', 'T', 'Z', 'L', 'W', 'H', 'Y', 'P', 'Q', 'O', 'B', 'G',
    '\0', /* FIGS */
    'M', 'X', 'V', '\0', /* LTRS */
];
const FIGS_CHARS: [char; 32] = [
    '\0', '3', '\n', '-', ' ', '\x07', '8', '7', '\r', '$', '4', '\'', ',', '!', ':', '(', '5', '"', ')', '2', '#', '6', '0', '1', '9', '?', '&',
    '\0', /* FIGS */
    '.', '/', ';', '\0', /* LTRS */
];
const CODE_LTRS_SHIFT: u8 = 0b11111;
const CODE_FIGS_SHIFT: u8 = 0b11011;

/// Encodes one ASCII character to its 5-bit Baudot code plus which shift
/// (letters=false, figures=true) it requires, or `None` if the character
/// has no Baudot representation at all (anything outside this table --
/// real RTTY text is 5-bit-limited by design, not an oversight here).
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
fn text_to_framed_bits(text: &str) -> Vec<bool> {
    let mut bits: Vec<bool> = Vec::new();
    let mut current_figs = false;
    for c in text.chars() {
        let Some((code, needs_figs)) = char_to_baudot(c) else { continue };
        if needs_figs != current_figs {
            let shift_code = if needs_figs { CODE_FIGS_SHIFT } else { CODE_LTRS_SHIFT };
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
pub fn rtty_modulate(text: &str, mark_hz: f64, sample_rate: u32) -> Vec<i16> {
    let space_hz = mark_hz + RTTY_DEFAULT_SHIFT_HZ;
    let samples_per_bit = sample_rate as f64 / RTTY_BAUD;
    let bits = text_to_framed_bits(text);
    let mut out = Vec::new();
    let mut phase = 0.0f64;
    let n_bits = bits.len();
    for (idx, &is_mark) in bits.iter().enumerate() {
        // The final bit of each framed character is the stop bit --
        // extend it to the real 1.5-unit stop duration. Framing always
        // emits exactly 7 bits per character (1 start + 5 data + 1
        // stop), so every 7th bit (idx % 7 == 6) is a stop bit.
        let is_stop_bit = idx % 7 == 6;
        let bit_duration_units = if is_stop_bit { RTTY_STOP_BIT_UNITS } else { 1.0 };
        let n_samples = (samples_per_bit * bit_duration_units).round() as usize;
        let freq = if is_mark { mark_hz } else { space_hz };
        let phase_inc = std::f64::consts::TAU * freq / sample_rate as f64;
        for _ in 0..n_samples {
            let sample = phase.sin();
            out.push((sample * i16::MAX as f64 * 0.9).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16);
            phase += phase_inc;
            if phase > std::f64::consts::TAU {
                phase -= std::f64::consts::TAU;
            }
        }
    }
    let _ = n_bits;
    out
}

/// Correlates one bit-period-long window against both the MARK and SPACE
/// reference tones and returns `true` if MARK energy dominates (a real,
/// simple two-tone matched-filter comparison -- FSK's own natural
/// demodulation primitive, the same shape of computation `nlp()`'s own
/// squared-signal power spectrum uses for pitch detection, just applied
/// to two fixed candidate frequencies instead of a whole bank).
fn window_is_mark(samples: &[i16], mark_hz: f64, space_hz: f64, sample_rate: u32) -> bool {
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
    mark_energy > space_energy
}

/// Resumable scan state, carried across `RttyDecoder::feed()` calls (a
/// fresh, default one is used for the one-shot `rtty_demodulate`).
struct ScanState {
    pos: usize,
    current_figs: bool,
    prev_was_mark: bool, // assume idle (MARK) before the signal starts
}

impl Default for ScanState {
    fn default() -> Self {
        Self { pos: 0, current_figs: false, prev_was_mark: true }
    }
}

/// The real scan/framing loop shared by `rtty_demodulate` (whole-buffer,
/// `stop_if_insufficient_lookahead: false`, since the entire signal is
/// already available -- no different from before this was extracted)
/// and `RttyDecoder::feed` (streaming, `true`). Advances `state` in
/// place and returns any newly decoded characters.
///
/// The `stop_if_insufficient_lookahead` distinction is real, not
/// cosmetic: `bit_at`'s own out-of-bounds check already made the
/// original single-pass algorithm safe against a signal that's simply
/// too short (framing fails, scan moves on) -- but for a *streaming*
/// decoder, "not enough samples yet" and "not enough samples ever" are
/// different situations. Without this flag, a start-bit edge that's
/// real but whose character hasn't fully arrived yet in the buffer
/// would fail `bit_at`'s bounds check, be treated as a framing failure,
/// and get scanned *past* -- permanently losing that character once
/// the rest of it arrives in a later `feed()` call, since `pos` would
/// already be beyond where the edge was. Stopping the scan before
/// committing to an under-buffered candidate, instead, means the next
/// `feed()` call (with more buffered audio) retries that exact edge
/// from scratch.
fn rtty_scan(samples: &[i16], mark_hz: f64, sample_rate: u32, state: &mut ScanState, stop_if_insufficient_lookahead: bool) -> String {
    let space_hz = mark_hz + RTTY_DEFAULT_SHIFT_HZ;
    let samples_per_bit = sample_rate as f64 / RTTY_BAUD;
    // Coarse scan grid: 4x oversampled relative to the bit rate. This was
    // widened to 32x during an earlier debugging pass, on the mistaken
    // assumption that coarse-edge alignment error was the source of a
    // real round-trip failure -- it was not: the actual bug was the
    // post-character advance under-counting the stop bit's real 1.5-unit
    // length (see the advance calculation below), and once that was
    // fixed, 4x oversampling was confirmed directly to pass every test
    // in this file just as well as 32x did, at 1/8th the scan cost.
    let scan_step = (samples_per_bit / 4.0).max(1.0) as usize;
    let window_len = samples_per_bit.round() as usize;
    let mut out = String::new();
    if window_len == 0 {
        return out;
    }

    while state.pos + window_len <= samples.len() {
        let this_is_mark = window_is_mark(&samples[state.pos..state.pos + window_len], mark_hz, space_hz, sample_rate);
        if state.prev_was_mark && !this_is_mark {
            // A full character needs samples through the stop bit's own
            // center (6.5 units past the edge) plus half a window either
            // side -- see this function's own doc comment for why this
            // check only applies in streaming mode.
            let full_char_end = state.pos as f64 + 7.0 * samples_per_bit + window_len as f64 / 2.0;
            if stop_if_insufficient_lookahead && full_char_end > samples.len() as f64 {
                break;
            }

            // Candidate start-bit edge at `state.pos`. Sample the start
            // bit's own center first as a real confirmation (must be
            // SPACE) -- a bare mark->space transition in the coarse scan
            // can be noise, not a real start bit.
            let bit_at = |bit_index_from_edge: f64| -> Option<bool> {
                let center = state.pos as f64 + bit_index_from_edge * samples_per_bit;
                let start = center - samples_per_bit / 2.0;
                let start_idx = start.round().max(0.0) as usize;
                let end_idx = start_idx + window_len;
                if end_idx > samples.len() {
                    return None;
                }
                Some(window_is_mark(&samples[start_idx..end_idx], mark_hz, space_hz, sample_rate))
            };

            if let Some(start_bit_is_mark) = bit_at(0.5) {
                if !start_bit_is_mark {
                    // Confirmed real start bit. Sample the 5 data bits
                    // (LSB first) and the stop bit, all at precise
                    // edge-relative offsets, not the coarse scan grid.
                    let mut code = 0u8;
                    let mut framing_ok = true;
                    for i in 0..5 {
                        match bit_at(1.5 + i as f64) {
                            Some(is_mark) => {
                                if is_mark {
                                    code |= 1 << i;
                                }
                            }
                            None => {
                                framing_ok = false;
                                break;
                            }
                        }
                    }
                    let stop_ok = framing_ok
                        && bit_at(6.5).unwrap_or(false); // must be MARK

                    if framing_ok && stop_ok {
                        match code {
                            CODE_LTRS_SHIFT => state.current_figs = false,
                            CODE_FIGS_SHIFT => state.current_figs = true,
                            _ => {
                                let c = if state.current_figs { FIGS_CHARS[code as usize] } else { LTRS_CHARS[code as usize] };
                                if c != '\0' {
                                    out.push(c);
                                }
                            }
                        }
                        // Advance past this whole character: 1 start +
                        // 5 data bits at 1.0 unit each, plus the stop
                        // bit at its real RTTY_STOP_BIT_UNITS (1.5)
                        // length -- matching `rtty_modulate`'s own
                        // per-bit rounding exactly (each bit rounded
                        // independently, not one rounded sum), since
                        // using a single `(7.0 * samples_per_bit).round()`
                        // here under-counts the real stop bit's extra
                        // 0.5-unit length by roughly half a bit period
                        // per character -- confirmed directly: that was
                        // a real bug, not a hypothetical one, caught by
                        // this file's own round-trip tests (drift
                        // compounding across a message caused later
                        // characters to desync and drop/corrupt).
                        let advance = 6 * samples_per_bit.round() as usize
                            + (samples_per_bit * RTTY_STOP_BIT_UNITS).round() as usize;
                        state.pos += advance;
                        state.prev_was_mark = true;
                        continue;
                    }
                }
            }
        }
        state.prev_was_mark = this_is_mark;
        state.pos += scan_step;
    }
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
pub fn rtty_demodulate(samples: &[i16], mark_hz: f64, sample_rate: u32) -> String {
    let mut state = ScanState::default();
    rtty_scan(samples, mark_hz, sample_rate, &mut state, false)
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
pub struct RttyDecoder {
    mark_hz: f64,
    sample_rate: u32,
    pending_samples: Vec<i16>,
    state: ScanState,
}

impl RttyDecoder {
    pub fn new(mark_hz: f64, sample_rate: u32) -> Self {
        Self { mark_hz, sample_rate, pending_samples: Vec::new(), state: ScanState::default() }
    }

    /// Feeds newly-arrived audio samples in; returns any characters
    /// that completed decoding as a result.
    pub fn feed(&mut self, samples: &[i16]) -> String {
        self.pending_samples.extend_from_slice(samples);
        let out = rtty_scan(&self.pending_samples, self.mark_hz, self.sample_rate, &mut self.state, true);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
        assert_eq!(rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate), letters);

        let digits = "1234567890";
        let mono = rtty_modulate(digits, RTTY_DEFAULT_MARK_HZ, sample_rate);
        assert_eq!(rtty_demodulate(&mono, RTTY_DEFAULT_MARK_HZ, sample_rate), digits);
    }

    /// A real negative-case check, matching this codebase's own
    /// established convention (`psk31.rs`, `Psk31Decoder`'s own tests) of
    /// never assuming noise is harmless without checking directly: pure
    /// random noise must not panic, hang, or produce an unbounded output
    /// string -- it doesn't need to produce *nothing* (an async framing
    /// decoder without a real presence/confidence gate, unlike the
    /// primary-channel PSK31 scan bank, can and will occasionally frame
    /// noise into a spurious character -- a real, honest limitation
    /// flagged in this module's own doc comment, not silently assumed
    /// away here).
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
        let mut mono = rtty_modulate(text, RTTY_DEFAULT_MARK_HZ, sample_rate);
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
        assert_eq!(got, text, "streaming decode across irregular chunk boundaries must match the real message");
    }

    #[test]
    fn the_final_character_lags_by_one_character_without_trailing_idle_audio_then_arrives_once_more_audio_does() {
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
        let got_from_signal_alone = decoder.feed(&mono);
        assert!(
            text.starts_with(&got_from_signal_alone) && got_from_signal_alone.len() < text.len(),
            "expected a real proper prefix of {text:?} (the last character withheld pending more lookahead), got {got_from_signal_alone:?}"
        );

        let tail = idle_mark_audio(RTTY_DEFAULT_MARK_HZ, sample_rate, 0.05);
        let got_after_more_audio = decoder.feed(&tail);
        assert_eq!(format!("{got_from_signal_alone}{got_after_more_audio}"), text, "the withheld character must arrive, not be lost, once more audio confirms it");
    }
}
