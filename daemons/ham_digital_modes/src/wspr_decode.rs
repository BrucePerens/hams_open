// Copyright © Bruce Perens K6BP.
// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]

//! WSPR sequential decode for the K=32, rate-1/2 "Layland-Lushbaugh"
//! convolutional code `wspr.rs` already encodes with (`POLY1`/`POLY2`).
//! 100% original code, NOT vendored or ported from any Karn source --
//! see `docs/proposals/WSPR_DECODE_IMPLEMENTATION_PLAN.md`'s own header
//! correction (2026-09-01) for the full trace of why: the only real
//! candidate third-party implementation for this specific code
//! (Phil Karn's separate `libfano` package, distinct from the actual
//! LGPL `libfec`, which doesn't support K=32 at all) has no usable
//! license -- no grant of any kind, just a bare 1995 copyright line.
//!
//! **Deliberately uses the stack algorithm, not Fano's own threshold
//! algorithm**, despite this project informally being called "the Fano
//! decoder" throughout the planning doc. Both are classic, publicly
//! documented sequential-decoding algorithms from the same body of
//! 1960s-70s information theory (Fano 1963; the stack/ZJ algorithm,
//! Zigangirov 1966 / Jelinek 1969) achieving equivalent decoding
//! performance -- Wikipedia's own "Sequential decoding" article covers
//! both as the two classic variants. Fano's algorithm needs intricate,
//! easy-to-get-subtly-wrong threshold-raising/lowering bookkeeping with
//! O(1) extra memory; the stack algorithm is a plain best-first search
//! over a priority queue of partial paths -- far lower implementation-
//! correctness risk for a first from-scratch implementation, at the cost
//! of memory proportional to search effort (a complete non-issue at
//! WSPR's 81-stage trellis depth). Chosen deliberately for that risk
//! tradeoff, not out of ignorance of Fano's own algorithm.
//!
//! ## The branch metric
//!
//! The Fano/sequential-decoding bit metric (public information theory,
//! not any one author's formula): for a received bipolar channel value
//! `r` (transmitted bit b -> signal `+amplitude` if b=1, `-amplitude` if
//! b=0, real received value = signal + N(0, noise_stddev^2) additive
//! Gaussian noise) and a hypothesized bit `hyp_bit`:
//!
//!   metric(hyp_bit) = log2( P(r|hyp_bit) / P(r) ) - R
//!
//! where P(r) = 0.5*P(r|0) + 0.5*P(r|1) (equiprobable transmitted bits)
//! and R is the code rate in bits per channel-bit-observation (0.5 for
//! this rate-1/2 code, evaluated one channel bit at a time -- summing
//! both channel bits of one branch subtracts a full 1.0 bit of rate per
//! branch, i.e. per info bit of tree depth advanced). This is what makes
//! a correct path's cumulative metric trend positive over many bits and
//! an incorrect path's trend negative -- the property every sequential
//! decoder depends on. Per Karn's own (merely read, not vendored)
//! `libfano/README`: "the performance of a sequential decoder depends
//! critically on the accuracy of its metric table" -- this implementation
//! computes the metric directly from the assumed channel's real
//! Gaussian parameters rather than a precomputed byte table (Karn's own
//! choice was purely a 1990s CPU-performance concession, not part of the
//! algorithm itself), so accuracy here depends on how well the caller's
//! `amplitude`/`noise_stddev` estimate the real channel -- exactly the
//! same sensitivity Karn's README describes.
//!
//! ## Scope: 81 of 88 input bits
//!
//! `wspr.rs`'s own `convolutional_encode()` produces 176 channel bits
//! (2 per input bit, 88 input bits: 50 real payload bits + 6 padding
//! zero bits to fill byte 6 + 32 zero tail bits to flush the K=32 shift
//! register), but `wspr_encode_symbols()` only ever transmits the FIRST
//! 162 of those 176 bits (`WSPR_NUM_SYMBOLS`) -- the last 14 channel
//! bits (the tail end of the tail-flushing bits) are never sent at all.
//! 162 channel bits = 81 complete bit-pairs = the first 81 of the 88
//! input bits recoverable from a real transmission. This is sufficient:
//! the 56 real bits (50 payload + 6 padding) are entirely inside the
//! first 81, so decoding stops at depth 81 rather than 88 without losing
//! any real information -- pinned by this module's own round-trip test
//! against `wspr_encode_symbols()`'s real output, not asserted from
//! derivation alone.

use crate::wspr::{WSPR_NUM_SYMBOLS, POLY1, POLY2, parity, interleave_permutation};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Number of input bits actually recoverable from a real 162-symbol
/// WSPR transmission -- see this module's own doc comment "Scope"
/// section.
pub const WSPR_DECODABLE_INPUT_BITS: usize = WSPR_NUM_SYMBOLS / 2;

/// Returned when the sequential decoder exhausts `max_cycles` node
/// expansions without reaching the end of the trellis. Per this
/// codebase's own fail-fast convention: a decoder that has given up
/// must say so explicitly, never silently return its best-guess-so-far
/// path as if it were a real decode (that's exactly the failure mode
/// that would look like a rare/spurious wrong decode in the field,
/// indistinguishable from a real one without this signal).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GaveUp {
    pub cycles: u64,
}

/// One partial path in the stack algorithm's search. `bits` stores the
/// decoded-so-far bits as a bitset (bit i = decision at trellis depth i)
/// rather than a heap-allocated `Vec<bool>` per node -- WSPR's 81-stage
/// trellis fits comfortably in a u128, and the stack algorithm can
/// generate many thousands of nodes on a noisy channel, so avoiding a
/// per-node allocation is a real, not premature, efficiency concern.
struct StackNode {
    metric: f64,
    depth: usize,
    state: u32,
    bits: u128,
}

impl PartialEq for StackNode {
    fn eq(&self, other: &Self) -> bool {
        self.metric == other.metric
    }
}
impl Eq for StackNode {}
impl PartialOrd for StackNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for StackNode {
    fn cmp(&self, other: &Self) -> Ordering {
        self.metric
            .partial_cmp(&other.metric)
            .expect("branch metric must be finite -- NaN indicates a channel-model parameter bug (zero/negative noise_stddev?)")
    }
}

/// The Fano/sequential-decoding bit metric -- see this module's own doc
/// comment for the full formula and its provenance. The shared Gaussian
/// normalizing constant `1/(sqrt(2*pi)*noise_stddev)` is deliberately
/// omitted from `gaussian_shape` below: it multiplies P(r|0), P(r|1),
/// and therefore P(r) identically, so it cancels exactly in the
/// P(r|hyp_bit)/P(r) ratio this function computes and never needs to be
/// evaluated.
fn fano_bit_metric(r: f64, hyp_bit: bool, amplitude: f64, noise_stddev: f64) -> f64 {
    const RATE_BITS_PER_CHANNEL_BIT: f64 = 0.5;
    let gaussian_shape = |x: f64, mean: f64| -> f64 {
        let z = (x - mean) / noise_stddev;
        (-0.5 * z * z).exp()
    };
    let p_r_given_1 = gaussian_shape(r, amplitude);
    let p_r_given_0 = gaussian_shape(r, -amplitude);
    let p_r = 0.5 * p_r_given_0 + 0.5 * p_r_given_1;
    let p_r_given_hyp = if hyp_bit { p_r_given_1 } else { p_r_given_0 };
    (p_r_given_hyp / p_r).log2() - RATE_BITS_PER_CHANNEL_BIT
}

/// Runs the stack-algorithm sequential decode over `channel_bit_values`
/// (exactly `2 * WSPR_DECODABLE_INPUT_BITS` bipolar received values, in
/// the SAME order `wspr.rs`'s own `convolutional_encode()` produces them
/// -- i.e. already de-interleaved; see `deinterleave_symbol_values()`
/// below for the step that gets real per-symbol observations into this
/// order). `amplitude`/`noise_stddev` are the assumed Gaussian channel
/// parameters the branch metric is computed against, one PER CHANNEL BIT
/// POSITION rather than one shared scalar pair (Per-Symbol Channel Model
/// Redesign, `docs/proposals/WSPR_DECODE_IMPLEMENTATION_PLAN.md`) --
/// indexed the same way `channel_bit_values` itself is, since each real
/// transmitted WSPR symbol can have genuinely different local channel
/// reliability (fading, adjacent-channel QRM) than its neighbors, and a
/// single global pair can't represent that (see this module's own doc
/// comment on why metric-table/channel-model accuracy matters).
/// Returns the decoded bits (as a bitset, bit i = trellis depth i) and
/// the winning path's own final cumulative metric on success, or
/// `GaveUp` if `max_cycles` node expansions were exhausted without
/// reaching the end of the trellis. The metric is returned deliberately,
/// not discarded: "reached full depth" alone is NOT sufficient evidence
/// of a correct decode at low SNR (see this module's own doc comment on
/// `MIN_ACCEPTABLE_METRIC` and the real, measured data behind it) --
/// callers that need a confidence gate, not just a raw completion,
/// should compare this value against that threshold themselves (or use
/// `sequential_decode_with_confidence_gate()` below, which already does).
pub fn sequential_decode(
    channel_bit_values: &[f64; WSPR_NUM_SYMBOLS],
    amplitude: &[f64; WSPR_NUM_SYMBOLS],
    noise_stddev: &[f64; WSPR_NUM_SYMBOLS],
    max_cycles: u64,
) -> Result<(u128, f64), GaveUp> {
    let mut heap = BinaryHeap::new();
    heap.push(StackNode { metric: 0.0, depth: 0, state: 0, bits: 0u128 });

    let mut cycles = 0u64;
    while let Some(node) = heap.pop() {
        cycles += 1;
        if cycles > max_cycles {
            return Err(GaveUp { cycles });
        }
        if node.depth == WSPR_DECODABLE_INPUT_BITS {
            return Ok((node.bits, node.metric));
        }
        for hyp_bit in [false, true] {
            let new_state = (node.state << 1) | (hyp_bit as u32);
            let expected1 = parity(new_state & POLY1) == 1;
            let expected2 = parity(new_state & POLY2) == 1;
            let r1 = channel_bit_values[node.depth * 2];
            let r2 = channel_bit_values[node.depth * 2 + 1];
            // Per-position channel parameters, not a shared scalar pair
            // (Per-Symbol Channel Model Redesign) -- r1/r2 are two
            // DIFFERENT physically-transmitted WSPR symbols, each with
            // its own real local channel reliability, so each gets its
            // own amplitude/noise_stddev at this same index.
            let branch_metric = fano_bit_metric(r1, expected1, amplitude[node.depth * 2], noise_stddev[node.depth * 2])
                + fano_bit_metric(r2, expected2, amplitude[node.depth * 2 + 1], noise_stddev[node.depth * 2 + 1]);
            let new_bits = if hyp_bit { node.bits | (1u128 << node.depth) } else { node.bits };
            heap.push(StackNode {
                metric: node.metric + branch_metric,
                depth: node.depth + 1,
                state: new_state,
                bits: new_bits,
            });
        }
    }
    // Every popped node (short of success or the max_cycles bailout
    // above) always pushes exactly 2 children, so the heap can never
    // truly empty -- if this is ever reached, that's a real invariant
    // violation worth crashing loud on, not something to paper over.
    unreachable!("stack algorithm heap emptied without success or exceeding max_cycles")
}

/// Why `sequential_decode()` alone is not enough for real use, and why
/// this constant exists: measured directly (not assumed) by sweeping
/// real symbol-level Gaussian noise against a known message and
/// recording every outcome's own final metric (`wspr_decode_sweep`
/// scratch tool, 2026-09-01) -- `sequential_decode()`'s stack algorithm
/// reliably finds SOME complete path well past the point where that
/// path stops being the correct one. At moderate noise it stops being
/// reliably correct without ever exhausting `max_cycles` -- unlike a
/// too-noisy-to-search-at-all channel (which genuinely does exhaust
/// `max_cycles`, see `gives_up_loudly_rather_than_returning_a_wrong_
/// answer_on_a_hopeless_channel` below), a moderately-noisy channel can
/// still cheaply find A complete path, just not reliably the RIGHT one.
/// "Reached full depth" therefore is NOT by itself sufficient evidence
/// of a correct decode -- exactly the silently-wrong-answer failure mode
/// this codebase's own fail-fast convention exists to prevent.
///
/// The real, measured data: correct decodes' own final metric averages
/// substantially higher than wrong decodes' at every SNR level tested
/// (e.g. at ~1.4dB per-bit SNR: correct mean +18.5, range [3.5, 33.0];
/// wrong mean +5.6, range [-13.5, 19.2] -- a real, consistently-signed
/// difference, but with genuine overlap near the decoding threshold,
/// not a clean separation). This is the expected, textbook behavior of
/// ANY soft-decision confidence metric operating near a finite-length
/// code's own decoding threshold, not a discovered bug -- real WSPR/
/// WSJT-X decoders have a nonzero false-decode rate near their own
/// sensitivity floor too, a well-known, accepted characteristic of
/// weak-signal digital modes generally.
///
/// `0.0` bits (over the full `WSPR_DECODABLE_INPUT_BITS`-branch trellis)
/// is the DEFAULT threshold suggested here: the natural, information-
/// theoretically motivated boundary where a decoded path's own
/// likelihood, after the per-bit rate-bias subtraction, has stopped
/// outperforming what the code's own rate demands -- not a threshold
/// picked purely by curve-fitting the measured data above.
///
/// Measured effect is real but weak, not a safety barrier: in the same
/// sweep, wrong-decode metrics ranged up to +19.2 and correct-decode
/// metrics ranged down to +3.5 -- most of that overlap sits above 0.0,
/// so this default rejects only a minority of wrong decodes (see
/// `confidence_gate_meaningfully_reduces_the_wrong_decode_rate` below,
/// which asserts and prints the real counts rather than just a
/// direction). `sequential_decode_with_confidence_gate()` therefore
/// takes the threshold as a caller-supplied argument, not a fixed
/// constant: a caller posting spots to a public database wants a much
/// higher bar (fewer false accepts, more false rejects) than one doing
/// a local display-only decode. This constant is offered purely as the
/// documented, information-theoretically-motivated starting point for
/// callers that haven't yet measured their own tradeoff.
pub const MIN_ACCEPTABLE_METRIC: f64 = 0.0;

/// What `sequential_decode_with_confidence_gate()` returns when the
/// search completed (unlike `GaveUp`, which means it didn't) but the
/// winning path's own metric fell below `MIN_ACCEPTABLE_METRIC` --
/// distinguished from `GaveUp` deliberately: these are different failure
/// modes with different real causes (see `MIN_ACCEPTABLE_METRIC`'s own
/// doc comment) and a caller may reasonably want to react to them
/// differently (e.g. retry with a longer integration time vs. simply
/// discarding a too-uncertain result). The rejected bits are included,
/// not discarded, for callers that want to log/inspect a low-confidence
/// guess rather than treat it as pure noise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConfidenceGateError {
    GaveUp { cycles: u64 },
    LowConfidence { bits: u128, metric: f64 },
}

/// `sequential_decode()` plus the confidence gate described in
/// `MIN_ACCEPTABLE_METRIC`'s own doc comment -- the function real
/// callers (a future `digital_decoder.rs` integration included) should
/// actually use, not the raw `sequential_decode()`, which by itself
/// treats "reached full depth" as success even when that's not reliably
/// true near the coding threshold.
///
/// `min_acceptable_metric` is caller-supplied rather than a fixed
/// constant, deliberately: the measured separation between correct and
/// wrong decodes near the coding threshold is real but has genuine
/// overlap (see `MIN_ACCEPTABLE_METRIC`'s own doc comment), so the right
/// threshold depends on the caller's own false-accept tolerance, not on
/// this function's. Pass `MIN_ACCEPTABLE_METRIC` for the documented,
/// information-theoretically-motivated default.
pub fn sequential_decode_with_confidence_gate(
    channel_bit_values: &[f64; WSPR_NUM_SYMBOLS],
    amplitude: &[f64; WSPR_NUM_SYMBOLS],
    noise_stddev: &[f64; WSPR_NUM_SYMBOLS],
    max_cycles: u64,
    min_acceptable_metric: f64,
) -> Result<u128, ConfidenceGateError> {
    match sequential_decode(channel_bit_values, amplitude, noise_stddev, max_cycles) {
        Ok((bits, metric)) if metric >= min_acceptable_metric => Ok(bits),
        Ok((bits, metric)) => Err(ConfidenceGateError::LowConfidence { bits, metric }),
        Err(GaveUp { cycles }) => Err(ConfidenceGateError::GaveUp { cycles }),
    }
}

/// Inverts `wspr.rs`'s own `interleave_permutation()`-based interleave
/// step exactly (`interleaved[perm[p]] = channel_bits[p]` at encode
/// time, so `channel_bits[p] = interleaved[perm[p]]` here) -- reusing
/// the identical permutation function rather than re-deriving the
/// mapping independently, so this is provably the correct inverse by
/// construction, not by a second derivation that could itself be wrong.
/// `symbol_values` are indexed by symbol position (what a real receiver
/// observes per WSPR symbol -- the DATA bit's soft value, NOT the sync
/// bit, which this decoder doesn't consume at all).
pub fn deinterleave_symbol_values(symbol_values: &[f64; WSPR_NUM_SYMBOLS]) -> [f64; WSPR_NUM_SYMBOLS] {
    let perm = interleave_permutation();
    let mut channel_bit_values = [0.0f64; WSPR_NUM_SYMBOLS];
    for (p, slot) in channel_bit_values.iter_mut().enumerate() {
        *slot = symbol_values[perm[p]];
    }
    channel_bit_values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wspr::{pack_call, pack_grid4_power, wspr_encode_symbols};

    /// Independently reconstructs the first `WSPR_DECODABLE_INPUT_BITS`
    /// bits of the 88-bit `data` array `wspr_encode_symbols()` builds
    /// internally, from the same `pack_call`/`pack_grid4_power` this
    /// module now shares access to -- a second, independent computation
    /// of the expected answer, not a reuse of the encoder's own internal
    /// data-array construction, so this test doesn't just check the
    /// decoder against its own assumptions.
    fn expected_decodable_bits(callsign: &str, grid4: &str, power_dbm: i32) -> u128 {
        let n = pack_call(callsign).unwrap();
        let m = pack_grid4_power(grid4, power_dbm).unwrap();
        let mut data = [0u8; 11];
        data[0] = ((n >> 20) & 0xFF) as u8;
        data[1] = ((n >> 12) & 0xFF) as u8;
        data[2] = ((n >> 4) & 0xFF) as u8;
        data[3] = (((n & 0x0F) << 4) + ((m >> 18) & 0x0F)) as u8;
        data[4] = ((m >> 10) & 0xFF) as u8;
        data[5] = ((m >> 2) & 0xFF) as u8;
        data[6] = ((m & 0x03) << 6) as u8;
        // data[7..11] already zero (the tail).

        let mut bits: u128 = 0;
        for i in 0..WSPR_DECODABLE_INPUT_BITS {
            let byte = data[i / 8];
            let bit = (byte >> (7 - (i % 8))) & 1;
            if bit == 1 {
                bits |= 1u128 << i;
            }
        }
        bits
    }

    /// Real per-symbol data-bit values, exactly as `wspr_encode_symbols`
    /// transmits them (symbol = 2*data_bit + sync_bit, so data_bit =
    /// symbol>>1) -- optionally with Gaussian noise added, matching the
    /// same channel model `sequential_decode`'s own metric assumes.
    fn symbol_values_from_real_transmission(symbols: &[u8; WSPR_NUM_SYMBOLS], amplitude: f64, noise_stddev: f64, seed: u64) -> [f64; WSPR_NUM_SYMBOLS] {
        let mut rng_state = seed;
        let mut next_gaussian = || -> f64 {
            // Same splitmix64 + Box-Muller construction as wspr.rs's own
            // add_awgn() -- duplicated here deliberately rather than
            // exposed as a shared pub(crate) item, since this is test-only
            // code in a different module and the two noise generators
            // serve genuinely different purposes (audio-sample AWGN vs.
            // symbol-level AWGN).
            rng_state = rng_state.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = rng_state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            let bits = (z ^ (z >> 31)) >> 11;
            let u1 = ((bits as f64) + 1.0) / ((1u64 << 53) as f64 + 1.0);
            rng_state = rng_state.wrapping_add(0x9E3779B97F4A7C15);
            let mut z2 = rng_state;
            z2 = (z2 ^ (z2 >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z2 = (z2 ^ (z2 >> 27)).wrapping_mul(0x94D049BB133111EB);
            let bits2 = (z2 ^ (z2 >> 31)) >> 11;
            let u2 = ((bits2 as f64) + 1.0) / ((1u64 << 53) as f64 + 1.0);
            (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
        };

        let mut out = [0.0f64; WSPR_NUM_SYMBOLS];
        for (i, &sym) in symbols.iter().enumerate() {
            let data_bit = (sym >> 1) & 1;
            let clean = if data_bit == 1 { amplitude } else { -amplitude };
            out[i] = clean + next_gaussian() * noise_stddev;
        }
        out
    }

    /// Broadcasts a uniform scalar channel parameter to the per-position
    /// array `sequential_decode`/`sequential_decode_with_confidence_gate`
    /// now require (Per-Symbol Channel Model Redesign) -- the right
    /// fixture for tests that deliberately model a channel with NO local
    /// variation (uniform SNR across the whole transmission), as opposed
    /// to `diagnostic_...windowed_local_noise_burst_is_detected_and_
    /// down_weighted` below, which builds a genuinely non-uniform array
    /// on purpose.
    fn uniform(value: f64) -> [f64; WSPR_NUM_SYMBOLS] {
        [value; WSPR_NUM_SYMBOLS]
    }

    #[test]
    fn genie_test_zero_noise_recovers_the_exact_bits_in_very_few_cycles() {
        // The first diagnostic advisor() called for: at (near-)zero
        // noise with a correct metric, the decoder should walk almost
        // straight to the answer. A tight max_cycles bound here means a
        // pass proves the search logic AND the metric sign/scale are
        // both right together -- a slow pass would mean the threshold/
        // search logic works but the metric itself is wrong (most likely
        // a missing or mis-signed rate bias).
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let expected = expected_decodable_bits("K6BP", "CM87", 30);

        // amplitude=1.0, noise_stddev tiny (not literally zero -- would
        // make the metric's Gaussian shape divide-by-zero) -- this is
        // the "genie" (near-certain) case.
        let symbol_values = symbol_values_from_real_transmission(&symbols, 1.0, 1e-6, 1);
        let channel_bit_values = deinterleave_symbol_values(&symbol_values);

        let (decoded, _metric) = sequential_decode(&channel_bit_values, &uniform(1.0), &uniform(1e-6), 200)
            .unwrap_or_else(|e| panic!("genie test gave up after {} cycles -- should have decoded almost immediately", e.cycles));
        assert_eq!(decoded, expected, "decoded bits must exactly match the independently-computed expected data bits");
    }

    #[test]
    fn genie_test_cycle_count_is_close_to_the_trellis_depth() {
        // A separate, explicit assertion on cycle count (not just
        // "didn't exceed 200") -- at true near-zero noise the decoder
        // should need close to WSPR_DECODABLE_INPUT_BITS + 1 pops (one
        // per depth, essentially no backtracking), not just "under some
        // generous ceiling."
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let symbol_values = symbol_values_from_real_transmission(&symbols, 1.0, 1e-6, 2);
        let channel_bit_values = deinterleave_symbol_values(&symbol_values);

        // Instrumented copy of the search loop's cycle count via a
        // generous max_cycles and checking the actual GaveUp path is
        // never hit; cycle count itself is only observable via a
        // successful decode's own internal counter, so re-run with a
        // tight bound scaled to the expected near-ideal cost instead.
        let tight_bound = (WSPR_DECODABLE_INPUT_BITS as u64) * 3;
        let result = sequential_decode(&channel_bit_values, &uniform(1.0), &uniform(1e-6), tight_bound);
        assert!(
            result.is_ok(),
            "expected near-ideal cycle count (<= {tight_bound}) at near-zero noise, got GaveUp -- metric or search logic likely wrong"
        );
    }

    #[test]
    fn round_trip_against_the_real_encoders_own_transmitted_bits() {
        // Pins the 81-of-88 truncation derivation with a real round-trip
        // test, not just a comment -- per advisor()'s explicit ask.
        // Several different real messages, not just one.
        for (callsign, grid4, power_dbm) in [
            ("K6BP", "CM87", 30),
            ("K9AN", "EN50", 33),
            ("W1AW", "FN31", 37),
        ] {
            let symbols = wspr_encode_symbols(callsign, grid4, power_dbm).unwrap();
            let expected = expected_decodable_bits(callsign, grid4, power_dbm);
            let symbol_values = symbol_values_from_real_transmission(&symbols, 1.0, 1e-6, 3);
            let channel_bit_values = deinterleave_symbol_values(&symbol_values);
            let (decoded, _metric) = sequential_decode(&channel_bit_values, &uniform(1.0), &uniform(1e-6), 1000)
                .unwrap_or_else(|e| panic!("{callsign}/{grid4}/{power_dbm}: gave up after {} cycles", e.cycles));
            assert_eq!(decoded, expected, "{callsign}/{grid4}/{power_dbm}: decoded bits mismatch");
        }
    }

    #[test]
    fn decodes_correctly_at_a_real_moderate_noise_level() {
        // Not the genie case: real AWGN at a moderate SNR the decoder
        // should comfortably handle, exercising actual backtracking
        // (cycle count meaningfully above the near-ideal genie case)
        // without exceeding a generous cycle budget.
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let expected = expected_decodable_bits("K6BP", "CM87", 30);
        // amplitude=1.0, noise_stddev=0.5 -> roughly 6dB Eb/N0-ish
        // (amplitude^2/noise_variance = 1/0.25 = 4 -> ~6dB), a real,
        // meaningfully noisy but not extreme channel.
        let symbol_values = symbol_values_from_real_transmission(&symbols, 1.0, 0.5, 4);
        let channel_bit_values = deinterleave_symbol_values(&symbol_values);
        let (decoded, _metric) = sequential_decode(&channel_bit_values, &uniform(1.0), &uniform(0.5), 100_000)
            .unwrap_or_else(|e| panic!("gave up after {} cycles at a moderate noise level", e.cycles));
        assert_eq!(decoded, expected);
    }

    #[test]
    fn gives_up_loudly_rather_than_returning_a_wrong_answer_on_a_hopeless_channel() {
        // Fail-fast per this codebase's own convention: a channel far
        // too noisy to decode must produce an explicit GaveUp, never a
        // silently-wrong best-guess path. Uses a tiny max_cycles against
        // heavy noise to force the give-up path deterministically rather
        // than relying on noise alone to make decoding fail (which could
        // occasionally succeed by chance and flake this test).
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let symbol_values = symbol_values_from_real_transmission(&symbols, 1.0, 5.0, 5);
        let channel_bit_values = deinterleave_symbol_values(&symbol_values);
        let result = sequential_decode(&channel_bit_values, &uniform(1.0), &uniform(5.0), 10);
        assert!(matches!(result, Err(GaveUp { .. })), "expected an explicit GaveUp on a tiny cycle budget against heavy noise");
    }

    #[test]
    fn confidence_gate_meaningfully_reduces_the_wrong_decode_rate() {
        // Direct, real test of the effect MIN_ACCEPTABLE_METRIC's own
        // doc comment claims -- not just an assertion that the gate
        // exists, a measurement of how much it actually helps. Sweeps
        // many seeds at a real noise level chosen (from the sweep behind
        // MIN_ACCEPTABLE_METRIC's own doc comment) to sit right in the
        // zone where sequential_decode() alone produces a real, non-
        // trivial rate of wrong (not GaveUp) decodes, then measures how
        // many of those wrong cases the gate actually catches as
        // LowConfidence. Deliberately asserts only "at least one" rather
        // than "most" or "a majority": the real measured effect at the
        // default threshold is weak (correct- and wrong-decode metrics
        // overlap substantially near the coding threshold -- see
        // MIN_ACCEPTABLE_METRIC's own doc comment), so a stronger
        // assertion would misrepresent what's actually measured here.
        // The real counts are always printed on failure so the next
        // reader sees the true magnitude, not just a pass/fail verdict.
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let expected = expected_decodable_bits("K6BP", "CM87", 30);
        let amplitude = 1.0;
        let noise_stddev = 0.9; // ~0.9dB per-bit SNR -- squarely in the mixed zone measured above.
        let trials = 30;

        let mut raw_wrong = 0;
        let mut gated_wrong = 0;
        let mut gated_low_confidence = 0;
        for seed in 0..trials {
            let symbol_values = symbol_values_from_real_transmission(&symbols, amplitude, noise_stddev, 10_000 + seed);
            let channel_bit_values = deinterleave_symbol_values(&symbol_values);

            if let Ok((bits, _metric)) = sequential_decode(&channel_bit_values, &uniform(amplitude), &uniform(noise_stddev), 2_000_000) {
                if bits != expected {
                    raw_wrong += 1;
                }
            }
            match sequential_decode_with_confidence_gate(&channel_bit_values, &uniform(amplitude), &uniform(noise_stddev), 2_000_000, MIN_ACCEPTABLE_METRIC) {
                Ok(bits) if bits != expected => gated_wrong += 1,
                Err(ConfidenceGateError::LowConfidence { .. }) => gated_low_confidence += 1,
                _ => {}
            }
        }

        let counts = format!(
            "raw_wrong={raw_wrong}/{trials}, gated_wrong={gated_wrong}/{trials}, gated_low_confidence={gated_low_confidence}/{trials}"
        );
        assert!(raw_wrong > 0, "test setup problem: this noise level should produce some raw wrong decodes to guard against ({counts})");
        assert!(
            gated_wrong < raw_wrong,
            "confidence gate should reject at least some raw wrong decodes as LowConfidence ({counts})"
        );
        assert!(gated_low_confidence > 0, "expected at least one LowConfidence rejection at this noise level ({counts})");
    }

    /// Direct test of the gap `MIN_ACCEPTABLE_METRIC`'s own doc comment
    /// and `diagnostic_exact_known_sync_failure_mode_breakdown` (in
    /// `wspr_sync.rs`) both flag but never actually ran: the real
    /// audio-domain WSPR sensitivity ladder's own failing SNR rungs
    /// (-30dB and below) produce a calibrated noise_stddev/amplitude
    /// ratio of 0.48-0.71 -- LOWER (i.e. theoretically easier) than the
    /// 0.9 ratio `confidence_gate_meaningfully_reduces_the_wrong_decode_
    /// rate` above already exercises successfully. This decoder's own
    /// metric/gate had simply never been run at that specific ratio
    /// range with a *perfectly known* channel model (exact amplitude and
    /// noise_stddev, no real-audio estimation error at all) -- this
    /// closes that gap.
    ///
    /// If the decoder decodes reliably here (as the monotonic-ratio
    /// intuition predicts, since 0.48-0.71 is less noisy than the
    /// already-working 0.9 case), that's real evidence AGAINST a
    /// decoder-algorithm/metric-formula bug: it would mean
    /// `sequential_decode`/`fano_bit_metric` are fine at this ratio, and
    /// the real audio-domain failure must instead be a channel-model
    /// mismatch -- the true per-symbol error statistics in real
    /// (extracted-from-FFT) evidence aren't well described by the single
    /// global (amplitude, noise_stddev) pair `evidence_to_symbol_values()`
    /// reports, even though the pair's own *reported* ratio looks benign.
    /// If it instead fails or is unreliable here too, that would be much
    /// stronger evidence for a real decoder-side problem at this specific
    /// ratio regime, worth chasing before assuming a channel-model
    /// mismatch.
    #[test]
    #[ignore]
    fn diagnostic_synthetic_decode_at_the_real_failing_ratio_range_with_perfectly_known_channel_params() {
        let symbols = wspr_encode_symbols("K6BP", "CM87", 30).unwrap();
        let expected = expected_decodable_bits("K6BP", "CM87", 30);
        let amplitude = 1.0;
        let ratios = [0.48, 0.55, 0.6, 0.65, 0.71];
        let trials = 10;

        println!("ratio | correct | gave_up | low_confidence | wrong");
        let mut any_ratio_failed_to_mostly_decode = false;
        for &ratio in &ratios {
            let noise_stddev = amplitude * ratio;
            let mut correct = 0;
            let mut gave_up = 0;
            let mut low_confidence = 0;
            let mut wrong = 0;
            for seed in 0..trials {
                let symbol_values =
                    symbol_values_from_real_transmission(&symbols, amplitude, noise_stddev, 20_000 + seed);
                let channel_bit_values = deinterleave_symbol_values(&symbol_values);
                match sequential_decode_with_confidence_gate(
                    &channel_bit_values,
                    &uniform(amplitude),
                    &uniform(noise_stddev),
                    2_000_000,
                    MIN_ACCEPTABLE_METRIC,
                ) {
                    Ok(bits) if bits == expected => correct += 1,
                    Ok(_) => wrong += 1,
                    Err(ConfidenceGateError::GaveUp { .. }) => gave_up += 1,
                    Err(ConfidenceGateError::LowConfidence { .. }) => low_confidence += 1,
                }
            }
            println!("{ratio:>5.2} | {correct:>7} | {gave_up:>7} | {low_confidence:>15} | {wrong:>5}");
            // A perfectly-modeled channel at a ratio easier than the
            // already-working 0.9 case should decode correctly nearly
            // every time -- this is a soft, printed-diagnosis assertion
            // (not exact "all trials"), matching this codebase's own
            // convention elsewhere of not over-asserting near a coding
            // threshold, but a majority-fail result here would be a real,
            // actionable finding, not noise.
            if correct < trials / 2 {
                any_ratio_failed_to_mostly_decode = true;
            }
        }
        assert!(
            !any_ratio_failed_to_mostly_decode,
            "at least one ratio in the real failing range decoded correctly less than half the time \
             with a PERFECTLY known channel model -- see printed table above. This points at a real \
             decoder-side problem specific to this ratio range (not just a channel-model mismatch in \
             real audio evidence), worth chasing before assuming the gap is purely a real-audio \
             calibration issue."
        );
    }

    #[test]
    fn fano_bit_metric_favors_the_hypothesis_matching_the_received_sign() {
        // A basic sanity check on the metric's own direction, isolated
        // from the search algorithm entirely: a strongly positive
        // received value must score higher under hyp_bit=true than
        // hyp_bit=false, and vice versa.
        let m_true = fano_bit_metric(0.9, true, 1.0, 0.3);
        let m_false = fano_bit_metric(0.9, false, 1.0, 0.3);
        assert!(m_true > m_false, "a received value near +amplitude must favor hyp_bit=true");

        let m_true_neg = fano_bit_metric(-0.9, true, 1.0, 0.3);
        let m_false_neg = fano_bit_metric(-0.9, false, 1.0, 0.3);
        assert!(m_false_neg > m_true_neg, "a received value near -amplitude must favor hyp_bit=false");
    }

    #[test]
    fn deinterleave_is_the_exact_inverse_of_the_encoders_own_interleave_step() {
        // Feeds a known, distinguishable value per symbol position
        // (its own index, as a float) through deinterleave and confirms
        // each value lands at the position the encoder's own interleave
        // step would have read it from -- proves the inversion directly,
        // not just indirectly via a full decode round-trip.
        let mut symbol_values = [0.0f64; WSPR_NUM_SYMBOLS];
        for (i, v) in symbol_values.iter_mut().enumerate() {
            *v = i as f64;
        }
        let channel_bit_values = deinterleave_symbol_values(&symbol_values);
        let perm = interleave_permutation();
        for p in 0..WSPR_NUM_SYMBOLS {
            assert_eq!(channel_bit_values[p], perm[p] as f64, "position {p} did not invert correctly");
        }
    }
}
