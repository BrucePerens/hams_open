//! Top-level bitstream decoder: the real inverse of [`super::encode_frame`], turning a received
//! 144-bit modulated frame (the eight code vectors `c_hat_0..c_hat_7`, or the 72 interleaved dibit
//! symbols an actual P25 channel decoder hands over -- [`super::interleave::deinterleave_from_dibit_symbols`]
//! is the bridge between the two) into a synthesized 20 ms PCM frame.
//!
//! # Decode order, and why it isn't just `encode_frame` run backwards field by field
//!
//! 1. **Golay-decode `c_hat_0`** to recover `u_hat_0` (never modulated) and its own corrected-error
//!    count `epsilon_0`.
//! 2. **Demodulate**: [`super::modulation::modulate_code_vectors`] is its own inverse (XOR), so
//!    calling it again with the just-recovered `u_hat_0` un-does the encoder's own modulation step,
//!    recovering `nu_hat_1..nu_hat_6` (still FEC codewords) and `nu_hat_7 = c_hat_7` (no FEC ever).
//! 3. **FEC-decode** `nu_hat_1..nu_hat_3` (Golay) and `nu_hat_4..nu_hat_6` (Hamming), each with its own
//!    corrected-error count -- together with `epsilon_0` these feed
//!    [`super::error_estimation::estimate_errors`].
//! 4. **Bootstrap `b_hat_0`** directly from `u_hat_0`/`u_hat_7` alone
//!    ([`super::bit_prioritization::extract_fundamental_frequency_quantizer`]) -- breaking the real
//!    chicken-and-egg problem [`super::bit_prioritization::deprioritize_bits`] can't solve on its own:
//!    that function needs `k_hat` and the Annex F/G column widths just to know where the bitstream
//!    splits, but `k_hat`/`L~` are only knowable *after* decoding `b_hat_0`.
//! 5. **Check for a frame repeat** (section 7.7, Eq. 97-98 plus an out-of-range `b_hat_0`) before
//!    doing any further, parameter-dependent decoding at all.
//! 6. If not repeating: dequantize `b_hat_0` into `omega0_tilde`/`L~`/`K~`, run the full
//!    [`super::bit_prioritization::deprioritize_bits`] now that `k_hat`/widths are known, decode the
//!    per-band V/UV bits into per-harmonic ones (Eq. 50-51), reconstruct the spectral amplitudes
//!    (section 6.4), and hand everything to [`super::synthesis::SynthesisState::synthesize_frame`].
//! 7. If repeating: skip straight to
//!    [`super::synthesis::SynthesisState::synthesize_repeated_frame`] (Eq. 99-104), and leave this
//!    decoder's own `l_hat_prev`/`spectral_amplitudes_prev` untouched (Eq. 100/103's own
//!    "current = previous" reduces to "don't reassign them" here).
//!
//! **What this module does not (yet) do**: section 7.8's own comfort-noise generation for a
//! persistently high error rate (`error_estimation::should_mute_frame`) is not yet implemented as its
//! own real synthesis path -- for now, a muted frame is treated the same as a repeated one (a
//! reasonable, documented degradation, not the spec's own literal comfort-noise algorithm). This is
//! recorded as still-open work, not silently presented as the real thing.

use super::bit_prioritization::{deprioritize_bits, extract_fundamental_frequency_quantizer, DeprioritizedBits};
use super::error_estimation::{estimate_errors, should_mute_frame, should_repeat_frame, FrameErrors};
use super::fec::{golay_decode, hamming_decode};
use super::modulation::modulate_code_vectors;
use super::parameter_encoding::{decode_voicing_decisions_per_harmonic, dequantize_fundamental_frequency};
use super::prediction::INITIAL_L_HAT_PREV;
use super::reconstruct::reconstruct_spectral_amplitudes;
use super::synthesis::SynthesisState;
use super::tables::{gain_bit_allocation, higher_order_bit_allocation};
use super::unvoiced_synthesis::N;
use super::vuv::{frequency_bands_count, harmonics_count};

/// Everything [`DecoderState::decode_parameters`] recovers from a non-repeat frame, exposed on its
/// own (rather than only fed straight into synthesis) so a caller -- notably this module's own
/// round-trip test -- can assert exact parameter fidelity against what was encoded, not just that
/// synthesis produced *some* finite PCM.
pub struct DecodedParameters {
    pub omega0_tilde: f64,
    pub l_hat: u32,
    pub k_hat: u32,
    pub bits: DeprioritizedBits,
    pub voiced: Vec<bool>,
    pub reconstructed_amplitudes: Vec<f64>,
    pub errors: FrameErrors,
}

/// The two things a received frame can resolve to before synthesis: a genuine decode, or a repeat
/// (section 7.7/7.8) with no new parameters of its own.
pub enum FrameOutcome {
    Repeat,
    Decoded(DecodedParameters),
}

/// `b_hat_0` values `208..=255` are reserved/unused (section 6.1's own stated range, `0 <= b_hat_0 <=
/// 207`) -- section 7.7's own text: a value outside that range is *itself* a frame-repeat trigger,
/// independent of `error_estimation::should_repeat_frame`'s own Eq. 97-98 check.
const MAX_VALID_B0: u32 = 207;

/// Persistent decoder state across frames: the shared [`SynthesisState`] (noise generator, both
/// synthesis halves, enhancement-stage `S_E`/`tau_M`, and the last real frame's own final synthesis
/// inputs for a repeat), plus the two pieces [`reconstruct_spectral_amplitudes`] itself needs frame to
/// frame -- `l_hat_prev`/`spectral_amplitudes_prev`, the *unenhanced* `M~_l(-1)` history (Eq. 77's own
/// prediction input, a real, separate history from `SynthesisState`'s own enhanced `M_bar_l(-1)`), and
/// `error_rate_prev` (`epsilon_R`, Eq. 96's own running average). Initial values per Annex A:
/// `L~(-1) = 30` ([`INITIAL_L_HAT_PREV`]), `M~_l(-1) = 1` for all `l` (unity, not silent -- matching
/// this codebase's own encoder-side `FrameState::initial()` choice, itself independently confirmed
/// against Annex A when `voiced_synthesis.rs` was built), `epsilon_R(-1) = 0.0`.
pub struct DecoderState {
    synthesis: SynthesisState,
    l_hat_prev: u32,
    spectral_amplitudes_prev: Vec<f64>,
    error_rate_prev: f64,
}

impl DecoderState {
    // [@ANCHOR: ambe:decoder_state_new]
    pub fn new() -> Self {
        let l_hat_prev = INITIAL_L_HAT_PREV;
        Self {
            synthesis: SynthesisState::new(),
            l_hat_prev,
            spectral_amplitudes_prev: vec![1.0; l_hat_prev as usize],
            error_rate_prev: 0.0,
        }
    }

    /// Steps 1-6 of this module's own doc comment, stopping short of synthesis: recovers either a
    /// repeat decision or the frame's full decoded parameter set. Split out from
    /// [`Self::decode_frame`] so a caller can assert on the actual recovered parameters (`b0`, `b1`,
    /// `b2`, gain/higher-order bits, per-harmonic voicing) rather than only on synthesized PCM, which
    /// -- because Annex F/G's own bit budgets total 88 bits for *every* valid `(L, K)` pair -- stays
    /// `Some` and "looks fine" even when decode derived the wrong `L~`/`K~` for the frame.
    // [@ANCHOR: ambe:decode_parameters]
    pub fn decode_parameters(&mut self, c: [u32; 8]) -> Option<FrameOutcome> {
        let (u0, epsilon_0) = golay_decode(c[0]);
        let nu = modulate_code_vectors(c, u0 as u32);

        let (u1, epsilon_1) = golay_decode(nu[1]);
        let (u2, epsilon_2) = golay_decode(nu[2]);
        let (u3, epsilon_3) = golay_decode(nu[3]);
        let (u4, epsilon_4) = hamming_decode(nu[4] as u16);
        let (u5, epsilon_5) = hamming_decode(nu[5] as u16);
        let (u6, epsilon_6) = hamming_decode(nu[6] as u16);
        let u7 = nu[7]; // No FEC (fec.rs's own doc comment): no decode, no error count.

        let u_vectors: [u32; 8] = [
            u0 as u32, u1 as u32, u2 as u32, u3 as u32, u4 as u32, u5 as u32, u6 as u32, u7,
        ];
        let corrected_error_counts = [
            epsilon_0, epsilon_1, epsilon_2, epsilon_3, epsilon_4, epsilon_5, epsilon_6,
        ];
        let errors = estimate_errors(&corrected_error_counts, self.error_rate_prev);
        self.error_rate_prev = errors.rate;

        let b0 = extract_fundamental_frequency_quantizer(&u_vectors);
        let mute = should_mute_frame(&errors); // See this module's own doc comment: treated as a
                                                // repeat for now, pending real comfort noise (7.8).
        let repeat = b0 > MAX_VALID_B0 || should_repeat_frame(&errors) || mute;

        if repeat {
            return Some(FrameOutcome::Repeat);
        }

        let omega0_tilde = dequantize_fundamental_frequency(b0);
        let l_hat = harmonics_count(omega0_tilde);
        let k_hat = frequency_bands_count(l_hat);

        let gain_widths: [u8; 5] =
            std::array::from_fn(|i| gain_bit_allocation(l_hat, i as u32 + 2).unwrap().0);
        let higher_widths: Vec<u8> = higher_order_bit_allocation(l_hat)?
            .iter()
            .copied()
            .filter(|&w| w > 0)
            .collect();

        let bits = deprioritize_bits(u_vectors, k_hat, gain_widths, &higher_widths)?;

        let voiced = decode_voicing_decisions_per_harmonic(bits.b1, k_hat, l_hat);

        let gain_values: [u32; 5] = std::array::from_fn(|i| bits.gain_vector[i].0);
        let higher_order_values: Vec<u32> = bits.higher_order.iter().map(|&(v, _)| v).collect();
        let reconstructed_amplitudes = reconstruct_spectral_amplitudes(
            bits.b2 as u8,
            gain_values,
            &higher_order_values,
            l_hat,
            self.l_hat_prev,
            &self.spectral_amplitudes_prev,
        )?;

        Some(FrameOutcome::Decoded(DecodedParameters {
            omega0_tilde,
            l_hat,
            k_hat,
            bits,
            voiced,
            reconstructed_amplitudes,
            errors,
        }))
    }

    /// Decodes and synthesizes one received frame's own eight modulated code vectors
    /// `c_hat_0..c_hat_7` (from [`super::interleave::deinterleave_from_dibit_symbols`]) into a 20 ms
    /// PCM frame. Returns `None` only for a genuinely unrecoverable case: the very first frame of a
    /// stream itself needs a repeat, and there is no real previous frame for
    /// [`SynthesisState::synthesize_repeated_frame`] to reuse (the honest "no comfort noise yet
    /// either" answer, not a silent zero-fill).
    // [@ANCHOR: ambe:decode_frame]
    pub fn decode_frame(&mut self, c: [u32; 8]) -> Option<[f64; N]> {
        match self.decode_parameters(c)? {
            FrameOutcome::Repeat => self.synthesis.synthesize_repeated_frame(),
            FrameOutcome::Decoded(params) => {
                let pcm = self.synthesis.synthesize_frame(
                    &params.reconstructed_amplitudes,
                    params.omega0_tilde,
                    &params.voiced,
                    &params.errors,
                )?;

                self.l_hat_prev = params.l_hat;
                self.spectral_amplitudes_prev = params.reconstructed_amplitudes;

                Some(pcm)
            }
        }
    }
}

impl Default for DecoderState {
    // [@ANCHOR: ambe:decoder_state_default]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambe::{
        bit_prioritization::prioritize_bits, encode_code_vectors, parameter_encoding, tables,
    };

    /// Builds a genuine synthetic voiced frame the same way `decode_parameters` will itself derive
    /// `L~`/`K~` on the receiving end: from `omega0_tilde = dequantize(quantize(omega0_hat))`, not
    /// from a hand-picked `L_hat` paired with an unrelated `omega0_hat`. An earlier draft of this test
    /// picked `L_hat=16` by hand while encoding `omega0_hat = 2*pi/100` -- but that pitch's own
    /// round-tripped `omega0_tilde` actually decodes to `L~=46` (Annex F/G's own per-`(L,K)` bit
    /// budgets all total exactly 88 bits, so the mismatched frame still parsed "successfully" into
    /// silently wrong parameters). Building against the decoder's own derivation here closes that
    /// gap, and `harmonics_count_agrees_with_itself_across_the_full_quantize_dequantize_round_trip`
    /// (`parameter_encoding.rs`) separately proves this agreement holds for every real pitch, not
    /// just this one.
    struct SyntheticFrame {
        c: [u32; 8],
        b0: u32,
        b1: u32,
        b2: u32,
        gain_vector: [(u32, u8); 5],
        higher_order: Vec<(u32, u8)>,
    }

    fn build_synthetic_voiced_frame() -> SyntheticFrame {
        let omega0_hat = 2.0 * std::f64::consts::PI / 100.0;
        let b0 = parameter_encoding::quantize_fundamental_frequency(omega0_hat);
        let omega0_tilde = parameter_encoding::dequantize_fundamental_frequency(b0);
        let l_hat = crate::ambe::vuv::harmonics_count(omega0_tilde);
        let k_hat = crate::ambe::vuv::frequency_bands_count(l_hat);

        let voiced_bands = vec![true; k_hat as usize];
        let b1 = parameter_encoding::encode_voicing_decisions(&voiced_bands);

        let gain: [u8; 5] =
            std::array::from_fn(|i| tables::gain_bit_allocation(l_hat, i as u32 + 2).unwrap().0);
        let higher = tables::higher_order_bit_allocation(l_hat).unwrap().to_vec();
        let b2 = 32u32; // Mid-range gain index.
        let gain_vector: [(u32, u8); 5] =
            std::array::from_fn(|i| ((1u32 << (gain[i] - 1)) & ((1 << gain[i]) - 1), gain[i]));
        let higher_order: Vec<(u32, u8)> = higher
            .iter()
            .filter(|&&w| w > 0)
            .map(|&w| (1u32 & ((1 << w) - 1), w))
            .collect();

        let u = prioritize_bits(b0, b1, k_hat, b2, gain_vector, &higher_order, false).unwrap();
        let c = encode_code_vectors(u);
        SyntheticFrame {
            c,
            b0,
            b1,
            b2,
            gain_vector,
            higher_order,
        }
    }

    /// A full, real encode -> decode round trip that asserts *exact* parameter fidelity, not just
    /// that synthesis produced finite, non-silent PCM (a much weaker claim that an earlier draft of
    /// this test relied on, and that would have passed even with the L~ mismatch documented on
    /// `build_synthetic_voiced_frame` above). `b0`/`b1`/`b2`/the gain vector/the higher-order
    /// coefficients are all recoverable exactly through this path -- no quantization loss between
    /// `prioritize_bits` and `deprioritize_bits` -- so this checks `assert_eq!`, not a tolerance.
    // Tests [@ANCHOR: ambe:decoder_state_new]
    // Tests [@ANCHOR: ambe:decode_parameters]
    // Tests [@ANCHOR: ambe:decode_frame]
    #[test]
    fn decode_frame_round_trips_a_real_synthetic_voiced_frame() {
        let frame = build_synthetic_voiced_frame();

        let mut params_decoder = DecoderState::new();
        let params = match params_decoder.decode_parameters(frame.c).unwrap() {
            FrameOutcome::Decoded(params) => params,
            FrameOutcome::Repeat => {
                panic!("expected a real decode for this synthetic frame, got a repeat")
            }
        };

        assert_eq!(params.bits.b0, frame.b0);
        assert_eq!(params.bits.b1, frame.b1);
        assert_eq!(params.bits.b2, frame.b2);
        assert_eq!(params.bits.gain_vector, frame.gain_vector);
        assert_eq!(params.bits.higher_order, frame.higher_order);
        assert!(!params.bits.sync_bit);
        assert!(
            params.voiced.iter().all(|&v| v),
            "every harmonic was encoded voiced, so every decoded one should be too"
        );

        let mut decoder = DecoderState::new();
        let pcm = decoder.decode_frame(frame.c).unwrap();
        assert_eq!(pcm.len(), N);
        for &sample in &pcm {
            assert!(sample.is_finite(), "non-finite decoded sample: {sample}");
        }
        assert!(
            pcm.iter().any(|&s| s.abs() > 1e-9),
            "expected real synthesized energy from a genuine voiced frame"
        );
    }

    #[test]
    fn decode_frame_does_not_panic_on_an_all_zero_first_frame() {
        let mut decoder = DecoderState::new();
        // c[0] deliberately garbage enough that Golay-decoding it, taken together with a
        // deliberately-invalid b0, may force a repeat with no real previous frame to fall back on.
        // Whether all-zero input actually triggers that path depends on golay_decode's own behavior
        // on an all-zero (technically valid-looking) codeword -- the only thing this test asserts is
        // that decode_frame never panics on a first-frame edge case, whichever branch it takes.
        let c = [0u32, 0, 0, 0, 0, 0, 0, 0];
        let _ = decoder.decode_frame(c);
    }

    /// `Default::default()` is a trivial one-line delegation to `Self::new()` -- checked for real
    /// rather than left implicitly covered by `new()`'s own tests above, since a future edit could
    /// make the two diverge without either test noticing on its own.
    // Tests [@ANCHOR: ambe:decoder_state_default]
    #[test]
    fn default_produces_the_same_initial_state_as_new() {
        let frame = build_synthetic_voiced_frame();
        let mut via_default = DecoderState::default();
        let mut via_new = DecoderState::new();
        let pcm_default = via_default.decode_frame(frame.c).unwrap();
        let pcm_new = via_new.decode_frame(frame.c).unwrap();
        assert_eq!(pcm_default, pcm_new);
    }
}
