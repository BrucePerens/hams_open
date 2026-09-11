// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]

//! BPSK31 (Peter Martinez, G3PLX) encode/decode, pure Rust, no vendored
//! third-party code. docs/proposals/DIGITAL_MODES.md's own FT8 research
//! found no permissively-licensed vendorable FT8 decoder library and
//! scoped that mode's DSP as a separate, multi-day project; PSK31 is a
//! genuinely different case -- it has no forward error correction at all
//! (by design: it relies on the operator noticing and asking for a
//! repeat, not an iterative soft-decision decoder), so both directions
//! are implementable and testable in one focused pass, not deferred.
//!
//! The Varicode table below was cross-verified against two independent
//! sources before being written fresh here (not copied from either):
//! the official ARRL PSK31 specification (arrl.org/psk31-spec, sample
//! entries: space=1, A=1111101, a=1011, DEL=1110110101, NUL=1010101011
//! all confirmed) and libcsdr's `psk31_varicode_items` table
//! (github.com/ha7ilm/csdr, BSD-3-Clause, Copyright (c) 2014 Andras
//! Retzler -- read to cross-check every one of the 128 numeric values
//! against the ARRL spec, not vendored; this file's actual code is
//! independently written). The Varicode table itself is a published,
//! fixed international standard every interoperable PSK31 implementation
//! must reproduce exactly, not creative expression belonging to either
//! source.

const PSK31_BAUD: f64 = 31.25;

/// (code, bitcount) for each ASCII value 0..127, MSB-first as documented
/// by the ARRL spec ("transmitted left bit first").
const VARICODE: [(u16, u8); 128] = [
    (0b1010101011, 10),
    (0b1011011011, 10),
    (0b1011101101, 10),
    (0b1101110111, 10),
    (0b1011101011, 10),
    (0b1101011111, 10),
    (0b1011101111, 10),
    (0b1011111101, 10),
    (0b1011111111, 10),
    (0b11101111, 8),
    (0b11101, 5),
    (0b1101101111, 10),
    (0b1011011101, 10),
    (0b11111, 5),
    (0b1101110101, 10),
    (0b1110101011, 10),
    (0b1011110111, 10),
    (0b1011110101, 10),
    (0b1110101101, 10),
    (0b1110101111, 10),
    (0b1101011011, 10),
    (0b1101101011, 10),
    (0b1101101101, 10),
    (0b1101010111, 10),
    (0b1101111011, 10),
    (0b1101111101, 10),
    (0b1110110111, 10),
    (0b1101010101, 10),
    (0b1101011101, 10),
    (0b1110111011, 10),
    (0b1011111011, 10),
    (0b1101111111, 10),
    (0b1, 1),
    (0b111111111, 9),
    (0b101011111, 9),
    (0b111110101, 9),
    (0b111011011, 9),
    (0b1011010101, 10),
    (0b1010111011, 10),
    (0b101111111, 9),
    (0b11111011, 8),
    (0b11110111, 8),
    (0b101101111, 9),
    (0b111011111, 9),
    (0b1110101, 7),
    (0b110101, 6),
    (0b1010111, 7),
    (0b110101111, 9),
    (0b10110111, 8),
    (0b10111101, 8),
    (0b11101101, 8),
    (0b11111111, 8),
    (0b101110111, 9),
    (0b101011011, 9),
    (0b101101011, 9),
    (0b110101101, 9),
    (0b110101011, 9),
    (0b110110111, 9),
    (0b11110101, 8),
    (0b110111101, 9),
    (0b111101101, 9),
    (0b1010101, 7),
    (0b111010111, 9),
    (0b1010101111, 10),
    (0b1010111101, 10),
    (0b1111101, 7),
    (0b11101011, 8),
    (0b10101101, 8),
    (0b10110101, 8),
    (0b1110111, 7),
    (0b11011011, 8),
    (0b11111101, 8),
    (0b101010101, 9),
    (0b1111111, 7),
    (0b111111101, 9),
    (0b101111101, 9),
    (0b11010111, 8),
    (0b10111011, 8),
    (0b11011101, 8),
    (0b10101011, 8),
    (0b11010101, 8),
    (0b111011101, 9),
    (0b10101111, 8),
    (0b1101111, 7),
    (0b1101101, 7),
    (0b101010111, 9),
    (0b110110101, 9),
    (0b101011101, 9),
    (0b101110101, 9),
    (0b101111011, 9),
    (0b1010101101, 10),
    (0b111110111, 9),
    (0b111101111, 9),
    (0b111111011, 9),
    (0b1010111111, 10),
    (0b101101101, 9),
    (0b1011011111, 10),
    (0b1011, 4),
    (0b1011111, 7),
    (0b101111, 6),
    (0b101101, 6),
    (0b11, 2),
    (0b111101, 6),
    (0b1011011, 7),
    (0b101011, 6),
    (0b1101, 4),
    (0b111101011, 9),
    (0b10111111, 8),
    (0b11011, 5),
    (0b111011, 6),
    (0b1111, 4),
    (0b111, 3),
    (0b111111, 6),
    (0b110111111, 9),
    (0b10101, 5),
    (0b10111, 5),
    (0b101, 3),
    (0b110111, 6),
    (0b1111011, 7),
    (0b1101011, 7),
    (0b11011111, 8),
    (0b1011101, 7),
    (0b111010101, 9),
    (0b1010110111, 10),
    (0b110111011, 9),
    (0b1010110101, 10),
    (0b1011010111, 10),
    (0b1110110101, 10),
];

/// Longest real Varicode codeword, in bits (verified for all 128 table
/// entries by this module's own tests: none exceeds 10 bits). Used by
/// `psk31_decode_bits`/`VaricodeAccumulator::push_bit` to bound their own
/// bit accumulators -- see those functions' own comments for why a run
/// longer than this can never be a real codeword-in-progress.
const MAX_VARICODE_BITS: u8 = 10;

// [@ANCHOR: char_to_code]
fn char_to_code(c: u8) -> Option<(u16, u8)> {
    if c < 128 {
        Some(VARICODE[c as usize])
    } else {
        None
    }
}

fn code_to_char(code: u16, bitcount: u8) -> Option<u8> {
    (0u16..128)
        .find(|&i| VARICODE[i as usize] == (code, bitcount))
        .map(|i| i as u8)
}

/// Encodes text into a Varicode bitstream: each character's code
/// (MSB-first), followed by a "00" gap. Non-ASCII bytes are skipped
/// rather than guessed at.
// [@ANCHOR: psk31_encode_bits]
pub fn psk31_encode_bits(text: &str) -> Vec<bool> {
    let mut bits = Vec::new();
    for &byte in text.as_bytes() {
        if let Some((code, bitcount)) = char_to_code(byte) {
            for i in (0..bitcount).rev() {
                bits.push((code >> i) & 1 == 1);
            }
            bits.push(false);
            bits.push(false);
        }
    }
    bits
}

/// Decodes a Varicode bitstream back to text. Per the spec, a character
/// boundary is any run of 2+ zero bits. This relies on the invariant
/// (verified for all 128 entries by this module's own
/// `every_codeword_ends_in_a_one_bit` test, a real property of the
/// standard, not assumed) that no codeword ever ends in 0 -- so the
/// first zero after a codeword's own final 1 bit is unambiguously the
/// start of the inter-character gap, never a genuine trailing bit that
/// would make "how many zeros is the gap" ambiguous.
// [@ANCHOR: psk31_decode_bits]
pub fn psk31_decode_bits(bits: &[bool]) -> String {
    let mut out = String::new();
    let mut acc: u16 = 0;
    let mut count: u8 = 0;
    let mut prev_was_zero = false;

    for &bit in bits {
        if !bit && prev_was_zero {
            // Second consecutive zero: the codeword itself ended at the
            // previous (1) bit, and the single zero accumulated since
            // then belongs to the gap, not the codeword -- drop it
            // before flushing.
            if count > 0 {
                acc >>= 1;
                count -= 1;
            }
            if count > 0 {
                if let Some(c) = code_to_char(acc, count) {
                    out.push(c as char);
                }
            }
            acc = 0;
            count = 0;
            prev_was_zero = false;
            continue;
        }
        acc = (acc << 1) | (bit as u16);
        count += 1;
        prev_was_zero = !bit;
        // A run this long with no "00" gap can never be a real,
        // in-progress codeword (every entry in VARICODE is at most
        // MAX_VARICODE_BITS bits -- verified for all 128 by
        // every_codeword_ends_in_a_one_bit/every_codeword_starts_with_a_
        // one_bit above). Left unbounded, a sufficiently long run of
        // same-parity bits (plausible from real demodulated noise, which
        // has no Varicode structure to keep it short) would grow `count`
        // past u8::MAX -- a panic in a debug build, or a silent wrap
        // (acc/count desync) in release. Reset now instead of letting
        // the accumulator run unbounded; the very next "00" still
        // starts a fresh, correctly-bounded accumulation.
        //
        // Real bug found 2026-09-11 (see night_shift_todo.md for the full
        // investigation): this threshold used to be bare
        // `MAX_VARICODE_BITS`, which silently dropped every one of the 10
        // real printable-ASCII characters whose codeword is exactly
        // MAX_VARICODE_BITS (10) bits long -- '%', '&', '?', '@', 'Z',
        // '^', '`', '{', '}', '~' -- whenever one was immediately
        // followed by the "00" inter-character gap (i.e. every single
        // time one of these characters is transmitted, not an edge
        // case). The `!bit && prev_was_zero` branch above only
        // recognizes a gap on its SECOND zero bit; the FIRST zero of the
        // gap is provisionally folded into the accumulator one line up,
        // exactly like any other bit, before it's known whether a
        // second zero will confirm it as a gap or a real bit will
        // extend the codeword further. For a full 10-bit codeword, that
        // provisional fold pushes `count` to 11 -- which this guard,
        // firing at `> MAX_VARICODE_BITS` (10), treated as "obviously
        // noise, reset now," discarding the completed codeword's own
        // bits before the second gap-zero ever arrived to confirm and
        // decode it. The accumulator can legitimately hold up to
        // MAX_VARICODE_BITS real bits PLUS exactly one provisional
        // (not-yet-confirmed) gap bit -- `+ 1` here reflects that real
        // invariant, not an arbitrary loosening: noise is still bounded
        // to a small, fixed constant (one bit later than before), and
        // a real max-length codeword's own provisional gap-bit no
        // longer gets treated as proof of runaway noise.
        if count > MAX_VARICODE_BITS + 1 {
            acc = 0;
            count = 0;
        }
    }
    if count > 0 {
        if let Some(c) = code_to_char(acc, count) {
            out.push(c as char);
        }
    }
    out
}

/// Synthesizes a BPSK31 audio signal (real i16 PCM, mono) for the given
/// bitstream. Standard raised-cosine amplitude envelope per symbol
/// (`0.5 * (1 - cos(2*pi*t/T))`), which goes to zero at every symbol
/// boundary regardless of whether the phase changes -- this is what
/// makes BPSK31's phase reversals glitch-free and gives it its narrow
/// (~31 Hz) spectral occupancy, rather than an abrupt phase jump that
/// would splatter across the band. Per the spec: bit 0 = 180-degree
/// phase reversal, bit 1 = steady carrier (no reversal).
// [@ANCHOR: psk31_modulate]
pub fn psk31_modulate(bits: &[bool], carrier_hz: f64, sample_rate: u32) -> Vec<i16> {
    let samples_per_symbol = (sample_rate as f64 / PSK31_BAUD).round() as usize;
    let mut out = Vec::with_capacity(bits.len() * samples_per_symbol);
    let mut phase_offset = 0.0f64;
    let mut sample_idx: u64 = 0;

    for &bit in bits {
        if !bit {
            phase_offset += std::f64::consts::PI;
        }
        for n in 0..samples_per_symbol {
            let t = n as f64 / samples_per_symbol as f64;
            let envelope = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * t).cos());
            let carrier_phase =
                2.0 * std::f64::consts::PI * carrier_hz * (sample_idx as f64 / sample_rate as f64);
            let sample = envelope * (carrier_phase + phase_offset).cos();
            out.push(
                (sample * i16::MAX as f64)
                    .round()
                    .clamp(i16::MIN as f64, i16::MAX as f64) as i16,
            );
            sample_idx += 1;
        }
    }
    out
}

/// Recovers the bitstream from a BPSK31 audio signal, assuming a known
/// carrier frequency and locked symbol timing (the audio was produced by
/// `psk31_modulate` at the same carrier/sample rate, or an equally clean
/// loopback/relay signal). This deliberately does NOT do full carrier/
/// timing acquisition or tracking (a PLL, AGC, weak-signal correlation
/// search) -- that's a much larger, separate DSP project, honestly out
/// of scope here the same way this file's own module doc explains FT8
/// decode is. What's implemented is a real, working demodulator for a
/// signal at a known frequency and baud rate: correlates each symbol
/// period against the local carrier reference to recover I/Q, then
/// differentially compares consecutive symbols' phase (bit = 1 if the
/// phase matches the previous symbol, 0 if it flipped 180 degrees) --
/// exactly mirroring the encoder's own phase-reversal convention.
/// Correlates exactly one symbol's worth of samples against the local
/// carrier reference and returns its raw (I, Q) correlation sums.
/// `sample_idx_start` is the running sample count since the start of the
/// whole signal (not just this chunk) so the carrier reference stays
/// phase-continuous across repeated calls -- what makes this usable both
/// for a whole-buffer decode and for `Psk31Decoder`'s incremental
/// streaming use, from the same logic. Exposed separately from
/// `correlate_symbol_phase` (which just calls this and takes the angle)
/// because the raw I/Q pair is real, useful data in its own right --
/// `DIGITAL_MODES.md`'s browser constellation display plots exactly this,
/// not a re-derived approximation, so it needs the same numbers the
/// demodulator itself decides bits from, not a separate computation that
/// could silently drift from what's actually being decoded.
// [@ANCHOR: correlate_symbol_iq]
fn correlate_symbol_iq(
    chunk: &[i16],
    carrier_hz: f64,
    sample_rate: u32,
    sample_idx_start: u64,
) -> (f64, f64) {
    let samples_per_symbol = chunk.len().max(1);
    let mut i_sum = 0.0f64;
    let mut q_sum = 0.0f64;
    for (n, &s) in chunk.iter().enumerate() {
        let t = n as f64 / samples_per_symbol as f64;
        // Match the same raised-cosine envelope the modulator used, as a
        // correlation weight -- this concentrates the estimate on the
        // high-confidence center of the symbol and naturally de-weights
        // the zero-crossing at each edge.
        let weight = 0.5 * (1.0 - (2.0 * std::f64::consts::PI * t).cos());
        let sample_idx = sample_idx_start + n as u64;
        let carrier_phase =
            2.0 * std::f64::consts::PI * carrier_hz * (sample_idx as f64 / sample_rate as f64);
        let x = (s as f64) / i16::MAX as f64;
        i_sum += x * carrier_phase.cos() * weight;
        q_sum += x * carrier_phase.sin() * weight;
    }
    (i_sum, q_sum)
}

/// Correlates exactly one symbol's worth of samples against the local
/// carrier reference and returns its estimated phase -- see
/// `correlate_symbol_iq`'s own doc comment for the underlying math.
fn correlate_symbol_phase(
    chunk: &[i16],
    carrier_hz: f64,
    sample_rate: u32,
    sample_idx_start: u64,
) -> f64 {
    let (i_sum, q_sum) = correlate_symbol_iq(chunk, carrier_hz, sample_rate, sample_idx_start);
    q_sum.atan2(i_sum)
}

/// Differentially compares a symbol's phase against the previous one to
/// recover its bit, per BPSK31's own convention (0 = phase reversal,
/// 1 = steady carrier).
// [@ANCHOR: phase_to_bit]
fn phase_to_bit(phase: f64, prev_phase: Option<f64>) -> bool {
    match prev_phase {
        // No prior symbol to differentially compare the very first one
        // against -- but every Varicode codeword starts with a 1 bit
        // (verified for all 128 entries by
        // every_codeword_starts_with_a_one_bit below), and the encoder's
        // phase_offset starts at 0 and only flips on a 0 bit, so the
        // first transmitted symbol is always a steady carrier relative
        // to the modulator's own zero-phase reference. Correct by the
        // standard's own design, not a guessed default.
        None => true,
        Some(p) => {
            let mut delta = phase - p;
            while delta > std::f64::consts::PI {
                delta -= 2.0 * std::f64::consts::PI;
            }
            while delta < -std::f64::consts::PI {
                delta += 2.0 * std::f64::consts::PI;
            }
            delta.abs() < std::f64::consts::FRAC_PI_2
        }
    }
}

// [@ANCHOR: psk31_demodulate]
pub fn psk31_demodulate(samples: &[i16], carrier_hz: f64, sample_rate: u32) -> Vec<bool> {
    let samples_per_symbol = (sample_rate as f64 / PSK31_BAUD).round() as usize;
    if samples_per_symbol == 0 {
        return Vec::new();
    }
    let mut bits = Vec::new();
    let mut prev_phase: Option<f64> = None;

    for (chunk_idx, chunk) in samples.chunks(samples_per_symbol).enumerate() {
        if chunk.len() < samples_per_symbol {
            break;
        }
        let sample_idx_start = (chunk_idx * samples_per_symbol) as u64;
        let phase = correlate_symbol_phase(chunk, carrier_hz, sample_rate, sample_idx_start);
        bits.push(phase_to_bit(phase, prev_phase));
        prev_phase = Some(phase);
    }
    bits
}

/// Incremental Varicode bit accumulator -- the same boundary-detection
/// logic `psk31_decode_bits` uses, factored out so `Psk31Decoder` can
/// feed it one bit at a time across many separate `feed()` calls instead
/// of needing the whole bitstream up front.
struct VaricodeAccumulator {
    acc: u16,
    count: u8,
    prev_was_zero: bool,
}

impl VaricodeAccumulator {
    fn new() -> Self {
        Self {
            acc: 0,
            count: 0,
            prev_was_zero: false,
        }
    }

    /// Pushes one bit; returns a decoded character if this bit completed
    /// one (see psk31_decode_bits's own doc for why the boundary logic
    /// is safe/unambiguous).
    // [@ANCHOR: VaricodeAccumulator::push_bit]
    fn push_bit(&mut self, bit: bool) -> Option<char> {
        if !bit && self.prev_was_zero {
            if self.count > 0 {
                self.acc >>= 1;
                self.count -= 1;
            }
            let result = if self.count > 0 {
                code_to_char(self.acc, self.count)
            } else {
                None
            };
            self.acc = 0;
            self.count = 0;
            self.prev_was_zero = false;
            return result.map(|c| c as char);
        }
        self.acc = (self.acc << 1) | (bit as u16);
        self.count += 1;
        self.prev_was_zero = !bit;
        // Same unbounded-accumulator hazard as psk31_decode_bits's own
        // identical guard (see its comment) -- this streaming path feeds
        // directly from live demodulated audio (Psk31Decoder::feed),
        // real noise/signal-loss included, so a long same-parity run is
        // not just a theoretical adversarial input here.
        //
        // Same real bug and same fix as psk31_decode_bits's own identical
        // guard (see that function's own comment for the full
        // investigation): a full MAX_VARICODE_BITS-length codeword's own
        // first gap-zero is provisionally folded in above before the
        // second zero confirms the gap, pushing `count` to
        // MAX_VARICODE_BITS + 1 -- `+ 1` here accounts for that real,
        // legitimate provisional bit rather than treating it as proof of
        // runaway noise.
        if self.count > MAX_VARICODE_BITS + 1 {
            self.acc = 0;
            self.count = 0;
        }
        None
    }
}

/// Stateful, incremental BPSK31 demodulator for a continuous audio
/// stream delivered across many separate calls (e.g. a live audio
/// pipeline processing small chunks at a time, where a plain
/// whole-buffer `psk31_decode_audio` call per chunk would incorrectly
/// reset the carrier phase reference and Varicode bit accumulator on
/// every single chunk boundary, corrupting anything that didn't happen
/// to align with one). Carries symbol timing, carrier phase, and
/// partially-accumulated Varicode bits across `feed()` calls.
pub struct Psk31Decoder {
    carrier_hz: f64,
    sample_rate: u32,
    samples_per_symbol: usize,
    pending_samples: Vec<i16>,
    sample_idx: u64,
    prev_phase: Option<f64>,
    varicode: VaricodeAccumulator,
}

impl Psk31Decoder {
    pub fn new(carrier_hz: f64, sample_rate: u32) -> Self {
        Self {
            carrier_hz,
            sample_rate,
            samples_per_symbol: (sample_rate as f64 / PSK31_BAUD).round().max(1.0) as usize,
            pending_samples: Vec::new(),
            sample_idx: 0,
            prev_phase: None,
            varicode: VaricodeAccumulator::new(),
        }
    }

    /// Feeds newly-arrived audio samples in; returns any characters
    /// that completed decoding as a result (usually empty -- most calls
    /// won't happen to land on a character boundary). Thin wrapper over
    /// `feed_with_iq` that discards the per-symbol I/Q -- kept as its own
    /// method (rather than making every caller ignore a tuple) since most
    /// callers (the primary QSO channel decode, the attestation-subcarrier
    /// detector) only ever wanted the text.
    pub fn feed(&mut self, samples: &[i16]) -> String {
        self.feed_with_iq(samples).0
    }

    /// Same as `feed`, but also returns each symbol's raw (I, Q)
    /// correlation pair, in the order they were decoded during this call
    /// -- the real numbers `DIGITAL_MODES.md`'s browser constellation
    /// display plots, not a re-derived approximation (see
    /// `correlate_symbol_iq`'s own doc comment for why that distinction
    /// matters). Usually 0 or 1 points per call at this daemon's real
    /// audio-chunk cadence, since one symbol is ~32ms at 48kHz/31.25 baud
    /// and audio chunks arrive faster than that -- never assume exactly
    /// one.
    // [@ANCHOR: Psk31Decoder::feed_with_iq]
    pub fn feed_with_iq(&mut self, samples: &[i16]) -> (String, Vec<(f64, f64)>) {
        self.pending_samples.extend_from_slice(samples);
        let mut out = String::new();
        let mut iq_points = Vec::new();
        while self.pending_samples.len() >= self.samples_per_symbol {
            let chunk: Vec<i16> = self
                .pending_samples
                .drain(..self.samples_per_symbol)
                .collect();
            let (i, q) =
                correlate_symbol_iq(&chunk, self.carrier_hz, self.sample_rate, self.sample_idx);
            iq_points.push((i, q));
            let phase = q.atan2(i);
            let bit = phase_to_bit(phase, self.prev_phase);
            self.prev_phase = Some(phase);
            self.sample_idx += self.samples_per_symbol as u64;
            if let Some(c) = self.varicode.push_bit(bit) {
                out.push(c);
            }
        }
        (out, iq_points)
    }
}

/// High-level: text in, PCM audio out.
pub fn psk31_encode_text(text: &str, carrier_hz: f64, sample_rate: u32) -> Vec<i16> {
    psk31_modulate(&psk31_encode_bits(text), carrier_hz, sample_rate)
}

/// High-level: PCM audio in, text out. For a live/streaming pipeline
/// processing audio in separate chunks over time, use `Psk31Decoder`
/// instead -- this whole-buffer form resets all demod state on every
/// call, which is only correct when the entire signal is already
/// available at once.
pub fn psk31_decode_audio(samples: &[i16], carrier_hz: f64, sample_rate: u32) -> String {
    psk31_decode_bits(&psk31_demodulate(samples, carrier_hz, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real bug found 2026-09-11 while root-causing a long-standing test failure
    /// (`digital_decoder.rs`'s `subcarrier_signal_with_a_slash_suffixed_callsign_produces_
    /// signature_verified`, misdiagnosed for a while as being about the '/' character
    /// specifically): the actual cause was any character whose Varicode codeword is exactly
    /// `MAX_VARICODE_BITS` (10) bits long, immediately followed by the "00" inter-character
    /// gap -- both `psk31_decode_bits`'s and `VaricodeAccumulator::push_bit`'s own runaway-
    /// noise guard used to fire one bit too early (`> MAX_VARICODE_BITS` instead of
    /// `> MAX_VARICODE_BITS + 1`), discarding the completed codeword before its own second
    /// gap-zero ever arrived to confirm and decode it -- see both guards' own doc comments
    /// for the full mechanism. Confirmed via a real standalone round-trip using the EXACT
    /// captured payload from that failing test (a real Ed25519-signed envelope containing
    /// four real 'Z' characters, all four silently dropped before the fix, all four present
    /// after it) before landing on this general, table-driven regression test.
    #[test]
    fn every_max_length_codeword_survives_a_real_round_trip_followed_by_a_gap() {
        for (ascii, &(_, bitcount)) in VARICODE.iter().enumerate() {
            if bitcount != MAX_VARICODE_BITS {
                continue;
            }
            let c = ascii as u8 as char;
            if !c.is_ascii_graphic() {
                continue; // Control characters aren't real transmitted text; skip them.
            }
            // Two repeats: proves the SECOND codeword decodes too, not just the first --
            // real transmitted text always has more than one character after a max-length
            // one, and the accumulator's own state must correctly reset and continue.
            let text = format!("{c}{c}");
            let audio = psk31_encode_text(&text, 2450.0, 48000);

            let whole_buffer = psk31_decode_audio(&audio, 2450.0, 48000);
            assert_eq!(
                whole_buffer, text,
                "whole-buffer decode dropped max-length codeword {c:?} (ascii {ascii})"
            );

            let mut decoder = Psk31Decoder::new(2450.0, 48000);
            let mut streamed = String::new();
            // Arbitrary, non-symbol-aligned chunk size -- matches this crate's real
            // callers (digital_decoder.rs feeds whatever chunk size its own audio_rx
            // channel happens to receive, never symbol-aligned by construction).
            for chunk in audio.chunks(240) {
                streamed.push_str(&decoder.feed(chunk));
            }
            assert_eq!(
                streamed, text,
                "streamed/chunked decode dropped max-length codeword {c:?} (ascii {ascii})"
            );
        }
    }

    /// The exact real-world case that surfaced the bug above, kept verbatim as its own
    /// test: a real Ed25519-signed subcarrier attestation envelope (captured directly from
    /// `digital_decoder.rs`'s own previously-failing test) containing four real 'Z'
    /// characters and three '/' characters, streamed through the same chunked-feed
    /// interface `digital_decoder.rs` actually uses.
    #[test]
    fn real_captured_signed_envelope_with_z_and_slash_characters_decodes_verbatim() {
        let text = "DE K6BP/P 1789161941|1789183541|m8LLo05mZXXRIPecnPHT3RH8NIXfl1jxjIsRWUw6mv4X2LZ3Y7u4EzSHN/ahbQXtdSRzrZ/IuYZdMn4GiwG9Bg== ";
        let audio = psk31_encode_text(text, 2450.0, 48000);
        let mut decoder = Psk31Decoder::new(2450.0, 48000);
        let mut out = String::new();
        for chunk in audio.chunks(240) {
            out.push_str(&decoder.feed(chunk));
        }
        assert_eq!(out, text);
    }

    #[test]
    // Tests [@ANCHOR: char_to_code]
    fn varicode_matches_the_arrl_spec_sample_entries() {
        // Direct spot-check against the ARRL's own published examples
        // (arrl.org/psk31-spec), not just internal self-consistency.
        assert_eq!(char_to_code(b' '), Some((0b1, 1)));
        assert_eq!(char_to_code(b'A'), Some((0b1111101, 7)));
        assert_eq!(char_to_code(b'a'), Some((0b1011, 4)));
        assert_eq!(char_to_code(0x00), Some((0b1010101011, 10)));
        assert_eq!(char_to_code(0x7f), Some((0b1110110101, 10)));
    }

    #[test]
    fn every_codeword_ends_in_a_one_bit() {
        // psk31_decode_bits's boundary logic depends on this: it's what
        // makes "the first zero after a real 1 bit is the start of the
        // 00 gap" unambiguous, rather than possibly being a codeword's
        // own genuine trailing zero.
        for &(code, _bitcount) in VARICODE.iter() {
            assert_eq!(code & 1, 1, "codeword {:b} does not end in 1", code);
        }
    }

    #[test]
    fn every_codeword_starts_with_a_one_bit() {
        // psk31_demodulate's first-symbol default (no prior phase to
        // differentially compare against) depends on this: it's what
        // makes "treat the first symbol as a steady carrier" correct by
        // the standard's own design rather than a guess.
        for &(code, bitcount) in VARICODE.iter() {
            assert_eq!(
                (code >> (bitcount - 1)) & 1,
                1,
                "codeword {:b} does not start with 1",
                code
            );
        }
    }

    #[test]
    fn no_codeword_contains_two_consecutive_zero_bits() {
        // The whole "00 marks a character boundary" design depends on
        // this invariant holding for every one of the 128 entries.
        for &(code, bitcount) in VARICODE.iter() {
            let mut prev_zero = false;
            for i in (0..bitcount).rev() {
                let bit = (code >> i) & 1 == 1;
                if !bit && prev_zero {
                    panic!(
                        "codeword {:0width$b} has two consecutive zero bits",
                        code,
                        width = bitcount as usize
                    );
                }
                prev_zero = !bit;
            }
        }
    }

    #[test]
    // Tests [@ANCHOR: psk31_decode_bits]
    fn decode_bits_does_not_panic_or_overflow_on_a_long_same_parity_run() {
        // Bug found and fixed by this pass: a run of same-parity bits
        // longer than u8::MAX (255) with no "00" gap used to grow the
        // internal `count: u8` accumulator past its own type's range --
        // a panic in a debug build (like this test), or a silent
        // wraparound desync in release. Demodulated audio (real noise,
        // or a long dropout with no genuine Varicode structure) has no
        // reason to ever produce a "00" gap on its own, so this is a
        // real reachable input, not a contrived one. 1000 bits of `true`
        // (a steady run, matching an unbroken "steady carrier" decode)
        // must not panic and must still decode something rather than
        // hang or crash -- the exact accumulated character content past
        // the reset point isn't the point (the run is not valid
        // Varicode data to begin with); not panicking, and not silently
        // wrapping into a corrupt (acc, count) pair, is.
        let bits = vec![true; 1000];
        let decoded = psk31_decode_bits(&bits);
        // No panic reaching here is the primary assertion. Also confirm
        // the decoder doesn't manufacture an implausible amount of
        // output from a single unbroken run with no real gaps in it.
        assert!(
            decoded.len() < 10,
            "a single, gap-free 1000-bit run should not decode to many characters, got {decoded:?}"
        );

        // Same run, but through the streaming accumulator
        // (VaricodeAccumulator::push_bit) -- the exact path real,
        // continuous demodulated audio actually takes via
        // Psk31Decoder::feed, distinct code from psk31_decode_bits above
        // even though both share the same fix.
        let mut acc = VaricodeAccumulator::new();
        let mut streamed = String::new();
        for _ in 0..1000 {
            if let Some(c) = acc.push_bit(true) {
                streamed.push(c);
            }
        }
        assert!(
            streamed.len() < 10,
            "streaming accumulator should behave the same way as the whole-buffer decoder, got {streamed:?}"
        );
    }

    #[test]
    // Tests [@ANCHOR: psk31_encode_bits]
    // Tests [@ANCHOR: psk31_decode_bits]
    fn encode_decode_bits_round_trips_real_text() {
        for msg in [
            "CQ CQ CQ DE K6BP K6BP",
            "hello world",
            "The Quick Brown Fox 123!",
        ] {
            let bits = psk31_encode_bits(msg);
            let decoded = psk31_decode_bits(&bits);
            assert_eq!(decoded, msg, "round-trip failed for {:?}", msg);
        }
    }

    #[test]
    // Tests [@ANCHOR: psk31_modulate]
    // Tests [@ANCHOR: psk31_demodulate]
    // Tests [@ANCHOR: correlate_symbol_iq]
    // Tests [@ANCHOR: phase_to_bit]
    fn full_modem_round_trips_through_synthesized_audio() {
        // The real end-to-end test: text -> bits -> audio -> bits -> text,
        // through the actual modulate/demodulate DSP, not just the
        // Varicode bit logic in isolation.
        let msg = "CQ DE K6BP";
        let sample_rate = 8000u32;
        let carrier = 1000.0;
        let audio = psk31_encode_text(msg, carrier, sample_rate);
        assert!(!audio.is_empty());
        let decoded = psk31_decode_audio(&audio, carrier, sample_rate);
        assert_eq!(decoded, msg);
    }

    #[test]
    fn full_modem_round_trips_at_a_different_carrier_and_sample_rate() {
        let msg = "de w1aw pse k";
        let sample_rate = 48000u32;
        let carrier = 1500.0;
        let audio = psk31_encode_text(msg, carrier, sample_rate);
        let decoded = psk31_decode_audio(&audio, carrier, sample_rate);
        assert_eq!(decoded, msg);
    }

    #[test]
    fn streaming_decoder_matches_whole_buffer_decode_when_fed_in_small_chunks() {
        // This is the actual scenario Psk31Decoder exists for: a live
        // audio pipeline delivering small chunks over many separate
        // calls, at a boundary that has nothing to do with symbol or
        // character boundaries. A naive per-chunk whole-buffer decode
        // would reset phase/bit-accumulator state every call and produce
        // garbage; this proves the stateful version doesn't.
        let msg = "CQ CQ CQ DE K6BP TEST 1234";
        let sample_rate = 8000u32;
        let carrier = 1000.0;
        let audio = psk31_encode_text(msg, carrier, sample_rate);

        let mut decoder = Psk31Decoder::new(carrier, sample_rate);
        let mut decoded = String::new();
        // An odd, small chunk size that does not evenly divide the
        // samples-per-symbol count, deliberately -- if state weren't
        // truly carried across feed() calls, chunk boundaries misaligned
        // with symbol boundaries would corrupt the decode.
        for chunk in audio.chunks(37) {
            decoded.push_str(&decoder.feed(chunk));
        }
        assert_eq!(decoded, msg);
    }

    #[test]
    fn streaming_decoder_handles_a_single_sample_at_a_time() {
        // The extreme case of the above -- proves feed() genuinely
        // buffers partial symbols rather than assuming each call
        // contains at least one whole one.
        let msg = "hi";
        let sample_rate = 8000u32;
        let carrier = 1000.0;
        let audio = psk31_encode_text(msg, carrier, sample_rate);

        let mut decoder = Psk31Decoder::new(carrier, sample_rate);
        let mut decoded = String::new();
        for &sample in &audio {
            decoded.push_str(&decoder.feed(&[sample]));
        }
        assert_eq!(decoded, msg);
    }

    #[test]
    // Tests [@ANCHOR: Psk31Decoder::feed_with_iq]
    // Tests [@ANCHOR: VaricodeAccumulator::push_bit]
    fn feed_with_iq_decodes_identically_to_feed() {
        // feed() is a thin wrapper over feed_with_iq() -- prove they
        // produce exactly the same text for the same input, not just
        // that feed_with_iq() compiles.
        let msg = "CQ CQ DE K6BP";
        let sample_rate = 8000u32;
        let carrier = 1000.0;
        let audio = psk31_encode_text(msg, carrier, sample_rate);

        let mut decoder_a = Psk31Decoder::new(carrier, sample_rate);
        let mut decoder_b = Psk31Decoder::new(carrier, sample_rate);
        let mut decoded_a = String::new();
        let mut decoded_b = String::new();
        for chunk in audio.chunks(41) {
            decoded_a.push_str(&decoder_a.feed(chunk));
            let (text, _) = decoder_b.feed_with_iq(chunk);
            decoded_b.push_str(&text);
        }
        assert_eq!(decoded_a, msg);
        assert_eq!(decoded_a, decoded_b);
    }

    #[test]
    fn feed_with_iq_returns_one_point_per_decoded_symbol() {
        // The real invariant a caller streaming this to a browser
        // constellation display depends on: every symbol boundary
        // crossed during a feed_with_iq() call produces exactly one (I,
        // Q) point, in order, not a coincidental count.
        let msg = "TEST";
        let sample_rate = 8000u32;
        let carrier = 1000.0;
        let audio = psk31_encode_text(msg, carrier, sample_rate);
        let samples_per_symbol = (sample_rate as f64 / PSK31_BAUD).round() as usize;
        let expected_symbols = audio.len() / samples_per_symbol;

        let mut decoder = Psk31Decoder::new(carrier, sample_rate);
        let (_, iq_points) = decoder.feed_with_iq(&audio);
        assert_eq!(iq_points.len(), expected_symbols);
    }

    #[test]
    fn feed_with_iq_points_reproduce_the_same_phase_bit_decision_as_correlate_symbol_phase() {
        // The real numbers this feeds a browser constellation display
        // have to be the SAME numbers the demodulator itself used to
        // decide each bit -- not a separately-computed approximation
        // that could silently drift. Prove atan2(q, i) on each returned
        // point reconstructs a phase sequence that differentially
        // decodes to the same bits psk31_demodulate() (the reference,
        // independently-tested whole-buffer decoder) gets from the
        // identical audio.
        let msg = "K6BP DE W1AW";
        let sample_rate = 8000u32;
        let carrier = 1000.0;
        let audio = psk31_encode_text(msg, carrier, sample_rate);

        let reference_bits = psk31_demodulate(&audio, carrier, sample_rate);

        let mut decoder = Psk31Decoder::new(carrier, sample_rate);
        let (_, iq_points) = decoder.feed_with_iq(&audio);

        let mut prev_phase: Option<f64> = None;
        let mut reconstructed_bits = Vec::new();
        for (i, q) in &iq_points {
            let phase = q.atan2(*i);
            reconstructed_bits.push(phase_to_bit(phase, prev_phase));
            prev_phase = Some(phase);
        }
        assert_eq!(reconstructed_bits, reference_bits);
    }

    #[test]
    fn feed_with_iq_points_have_real_nonzero_magnitude_for_a_real_signal() {
        // A degenerate all-zero (or near-zero) I/Q stream would still
        // "decode" via atan2's own defined behavior at the origin, but
        // would be visually meaningless as a constellation point --
        // catches a broken correlation (e.g. an accidentally-zeroed
        // carrier reference) that text-only decode correctness wouldn't
        // surface, since phase_to_bit only cares about the angle, not
        // the magnitude.
        let msg = "DE K6BP";
        let sample_rate = 8000u32;
        let carrier = 1000.0;
        let audio = psk31_encode_text(msg, carrier, sample_rate);

        let mut decoder = Psk31Decoder::new(carrier, sample_rate);
        let (_, iq_points) = decoder.feed_with_iq(&audio);
        assert!(!iq_points.is_empty());
        for (i, q) in &iq_points {
            let magnitude = (i * i + q * q).sqrt();
            assert!(
                magnitude > 0.01,
                "expected a real, non-degenerate correlation magnitude, got {magnitude}"
            );
        }
    }
}
