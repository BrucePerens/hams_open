//! AMBE (Advanced Multi-Band Excitation) vocoder -- D-STAR's own generation of the algorithm,
//! implemented from TIA-102.BABA (the 2003 base standard) directly, per
//! `docs/proposals/AMBE_CODEC_AND_DSTAR_IMPLEMENTATION_PLAN.md`. That document also names the real
//! reasoning for scope: DMR and Yaesu System Fusion both use the later AMBE+2 generation (the 2009
//! addendum, which names 12 specific patents) and are deliberately NOT implemented here -- see
//! `AMBE_PLUS_2_NOTES.md` in this same directory for what's known about that generation, kept as
//! documentation only.
//!
//! # Real, current state, corrected 2026-09-07: both encode AND decode are now implemented, tested, and wired end to end
//!
//! **Stale as of this correction**: this section used to say "What remains: the entire decoder side
//! proper... remains genuinely unstarted." That was true when written but hadn't been updated since
//! -- the decoder side is real, complete, and tested: [`decode::DecoderState::decode_frame`] is the
//! real inverse of [`encode_frame`], covering frame-repeat robustness (section 7.7, Eq. 97-104),
//! mute/comfort-noise generation (section 7.8, real uniform-on-`[-5,5]` noise per the spec's own
//! literal text), spectral enhancement ([`enhancement`], sections 8-9), and the actual synthesis
//! filterbank ([`synthesis`], [`voiced_synthesis`], [`unvoiced_synthesis`]) that turns reconstructed
//! parameters back into real 20ms PCM. [`interleave::deinterleave_from_dibit_symbols`] is verified,
//! end to end, as the real inverse of [`interleave::interleave_to_dibit_symbols`] feeding straight
//! into `decode_parameters` -- not just checked in isolation against the raw bit-position math, per
//! `decode.rs`'s own `decode_parameters_agrees_whether_fed_c_directly_or_via_a_real_interleave_
//! deinterleave_round_trip` test.
//!
//! Every stage of section 5-7's own encode pipeline is implemented and tested (see the pipeline
//! list below), and [`encode_frame`] composes all eight of them into one real function that takes
//! an analysis frame and estimated pitch in and returns the final 144-bit modulated code vectors --
//! not just independently-tested pieces left for a caller to assemble correctly. It also closes the
//! gap an earlier version of this module disclosed as still open: `encode_frame` now runs its own
//! quantizer values back through [`reconstruct::reconstruct_spectral_amplitudes`] (dequantization
//! and the inverse DCTs, section 6.4) to produce real decoder-equivalent history for the *next*
//! frame's own prediction, rather than reusing this frame's own unquantized estimate. What already
//! exists: the frame structure and pipeline stage documentation below, `fec.rs`'s
//! Golay/Hamming FEC
//! (generator matrices independently verified two ways -- against each code's own published weight
//! distribution, and against the PDF's own separate vector-text layer, see that module's doc comment),
//! and `tables.rs`'s Annexes E, F, G, and J (gain quantizer levels, gain-vector bit allocation/step
//! size, higher-order DCT coefficient bit allocation, and prediction-residual block lengths) --
//! parsed programmatically from the real PDF (confirmed real vector text throughout, not a raster,
//! via `pdfimages -list` finding zero embedded images on any of these pages) and checked against real
//! structural invariants before being trusted, never read digit-by-digit by eye and taken on faith.
//!
//! **Annex G was the hard one, and needed a materially different technique than E/F/J**: `pdftotext`
//! garbles or drops entries on 31 of its 48 rows (a much bigger watermark-collision problem than
//! Annex F's 4 affected rows, and often 2-3 entries missing per row rather than 1, which defeated
//! Annex F's simpler "recover by elimination" fix). Resolved instead by extracting the PDF's raw
//! per-character glyph stream directly (bypassing `pdftotext`'s line-reconstruction heuristic
//! entirely) and reading each entry's actual coefficient-index label rather than inferring position
//! from layout -- see `tables.rs`'s own `higher_order_bit_allocation` doc comment for the full
//! methodology and its three-way independent verification (zero mismatches against all 1207 values
//! `pdftotext` DID get right; the real, per-block non-increasing bit-allocation invariant holding
//! with zero exceptions across all 288 blocks; and exact `L-6` entry counts with no gaps or
//! duplicates for all 48 `L` values). All four Annex tables are now real, verified, and available for
//! whatever encoder/decoder logic gets built against them next.
//!
//! # The encode pipeline, per TIA-102.BABA sections 5-7 -- now fully implemented
//!
//! 1. **Pitch estimation and voicing decision** (section 5.1, [`pitch`]/[`pitch_refinement`]; section
//!    5.2, Eq. 31-42, [`vuv`]): the fundamental frequency and a per-band voiced/unvoiced decision.
//! 2. **Spectral amplitude estimation** (section 5.3, Eq. 43-44, [`spectral_amplitude`]): each
//!    harmonic's own magnitude `M_hat_l`, voiced or unvoiced per [`vuv`]'s own decision.
//! 3. **Fundamental frequency and V/UV bit encoding** (section 6.1-6.2, Eq. 45/49,
//!    [`parameter_encoding`]): `b_hat_0` and `b_hat_1`.
//! 4. **Prediction residual** (section 6.2, Eq. 52-57, [`prediction`]): the log2-domain differential
//!    encoding of the spectral amplitudes against the previous frame's own reconstructed history (a
//!    real, disclosed scope boundary: that history is taken as an external input, since it depends on
//!    a decoder-reconstruction loop this module doesn't build -- see `prediction`'s own doc comment).
//! 5. **Block DCT and quantization** (section 6.3, Eq. 58-63, [`quantize`], using [`tables`]'s
//!    Annexes E/F/G/J and [`gain_vector_dct`] below): the gain vector's first element (`b_hat_2`) via
//!    Annex E's 6-bit non-uniform quantizer, the remaining gain elements and higher-order DCT
//!    coefficients via uniform quantizers whose bit allocation and step size depend on `L_hat`
//!    (Annexes F and G, plus the small in-body Tables 3-4 for the higher-order step sizes).
//! 6. **Bit prioritization** (Fig. 22, [`bit_prioritization`]): `b_hat_0..b_hat_{L+2}` reordered by
//!    importance into eight bit vectors `u_hat_0..u_hat_7`.
//! 7. **Forward error correction** (Eq. 81-83, [`fec`]): `u_hat_0..u_hat_3` each get a `[23,12]`
//!    Golay code, `u_hat_4..u_hat_6` each get a `[15,11]` Hamming code, `u_hat_7` is left unprotected.
//! 8. **Bit modulation** (Eq. 84-94, [`modulation`]): each FEC code vector XORed with a data-dependent
//!    pseudo-random sequence, producing the final modulated code vectors `c_hat_0..c_hat_7` --
//!    [`encode_code_vectors`] below wires steps 6-8 together into one call.
//!
//! 9. **Intra-frame bit interleaving** (section 7.5, Annex H, [`interleave`]): rearranges
//!    `c_hat_0..c_hat_7`'s own 144 bits into 72 two-bit dibit symbols, spreading short error bursts
//!    across several different error-correction code words -- see this module's own "real scope
//!    boundary" note below for exactly what this does and doesn't cover.
//! 10. **Spectral amplitude reconstruction** (section 6.4, Eq. 67-79, [`reconstruct`]): the decoder-
//!     side inverse of stage 5 -- dequantization, inverse DCTs, and Eq. 75-79's own log2 reassembly --
//!     that produces real decoder-equivalent history for stage 4's *next* frame prediction, closing
//!     the loop a real closed-loop predictive coder needs (Fig. 16's own "Reconstruct" feedback
//!     block).
//! 11. **End-to-end composition** ([`encode_frame`]): wires stages 1-8 and 10 together into one call
//!     (stage 9's own interleaving is a separate, optional final step over `encode_frame`'s own
//!     output -- see [`interleave::interleave_to_dibit_symbols`]), carrying the per-frame state
//!     ([`FrameState`]) that Eq. 41's energy tracker and Eq. 54's prediction residual both genuinely
//!     need from the previous frame.
//!
//! **A real scope boundary, corrected once during this build rather than left wrong**: an earlier
//! version of this doc comment claimed "this document never defines a bit-interleaving permutation
//! of its own," reasoning from Annex K's own flow chart ("interleaved ... into Project 25 Frame
//! Structure") and the encryption section's own deferral to "the Project 25 Common Air Interface."
//! That claim was too broad -- section 7.5 and Annex H ("Bit Frame Format") DO fully define a real
//! intra-frame interleaving: the 144 bits across `c_hat_0..c_hat_7` rearranged into 72 two-bit dibit
//! symbols ([`interleave::interleave_to_dibit_symbols`]), verified as a real bijection over all 144
//! bit positions before being trusted (see that module's own doc comment). What section 7.5 actually
//! defers to the Project 25 Common Air Interface is narrower than "interleaving" as a whole: only
//! *where* those 72 already-interleaved symbols land inside an actual transmitted channel frame
//! (timing, sync patterns, other channel overhead) -- its own text says the symbols should be
//! "inserted into the Project 25 frame format beginning with symbol 0," which is placement, not
//! reordering. `c_hat_0..c_hat_7` (144 bits total: `4*23 + 3*15 + 7`, matching `FRAME_BITS`) and
//! their own interleaved-symbol form are both this codec's own real, complete output; only the actual
//! channel-frame placement remains a separate protocol layer's concern. The decoder side (synthesis,
//! frame-repeat/mute robustness, spectral enhancement) is complete too -- see this doc comment's own
//! "Real, current state" section above.
//!
//! # What's left, honestly: D-STAR's own framing, not this codec's core algorithm
//!
//! What this module does NOT cover, and what `AMBE_CODEC_AND_DSTAR_IMPLEMENTATION_PLAN.md`'s own
//! "Real next steps" still names as open: D-STAR's own protocol-level framing around this codec (its
//! header/slow-data structure, radio-ID fields, and wherever D-STAR's own channel format differs from
//! the Project 25 CAI this module's own `interleave` doc comment discusses) -- the same "core codec
//! vs. channel-frame placement" scope boundary already drawn above for P25, not yet drawn for D-STAR
//! specifically. Also open: real AMBE-chip bit-exact validation once Bruce's own hardware arrives
//! (the proposal's own item 3), and the 2009 AMBE+2 addendum, deliberately out of scope pending patent
//! clearance (this doc comment's own opening paragraph).

pub mod bit_prioritization;
pub mod decode;
pub mod enhancement;
pub mod error_estimation;
pub mod fec;
pub mod interleave;
pub mod modulation;
pub mod parameter_encoding;
pub mod pitch;
pub mod pitch_refinement;
pub mod prediction;
pub mod quantize;
pub mod reconstruct;
pub mod spectral_amplitude;
pub mod synthesis;
pub mod tables;
pub mod unvoiced_synthesis;
pub mod voiced_synthesis;
pub mod vuv;

/// 7.2kbps frame rate: 144 bits every 20ms, per TIA-102.BABA section 7.3 ("At 7.2 kbps with a 20 ms
/// frame size, 144 bits per frame are available for voice coding").
pub const FRAME_BITS: usize = 144;
/// Of `FRAME_BITS`, this many carry the actual quantized model parameters (spectral amplitudes,
/// pitch, gain); the rest (`FRAME_BITS - VOICE_BITS`) are forward error correction.
pub const VOICE_BITS: usize = 88;
/// `FRAME_BITS - VOICE_BITS`: divided between four [23,12] Golay codes (12 data bits each, so 48 of
/// the 88 voice bits) and three [15,11] Hamming codes (11 data bits each, 33 more), with the
/// remaining `88 - 48 - 33 = 7` bits (`u_7`) left completely unprotected -- per Eq. 81-83.
pub const FEC_BITS: usize = FRAME_BITS - VOICE_BITS;
/// Frame duration in milliseconds -- the other half of the "144 bits every 20ms" rate statement.
pub const FRAME_DURATION_MS: f64 = 20.0;

/// The gain vector's second-stage DCT, Eq. 61: a 6-point DCT-II-family transform across the six
/// per-block DC coefficients `r_hat[0..6]` (themselves each block's own first DCT coefficient, per
/// Eq. 60 and Fig. 18), producing the transformed gain vector `G_hat[0..6]`.
///
/// A plain, unambiguous closed-form formula (not a table), so implemented and tested directly rather
/// than deferred with the FEC/quantizer tables above.
///
/// `G_hat_m = (1/6) * sum_{i=1}^{6} R_hat_i * cos(pi*(m-1)*(i-0.5)/6)`, for `1 <= m <= 6`
/// (Eq. 61, 1-indexed in the spec; `r_hat` here is 0-indexed, `r_hat[i-1] == R_hat_i`).
pub fn gain_vector_dct(r_hat: &[f64; 6]) -> [f64; 6] {
    let mut g_hat = [0.0f64; 6];
    for (m, slot) in g_hat.iter_mut().enumerate() {
        let mut sum = 0.0;
        for (i, &r) in r_hat.iter().enumerate() {
            let i1 = (i + 1) as f64; // 1-indexed i, matches the spec's own Eq. 61
            let m1 = m as f64; // (m-1) in the spec's 1-indexed m, so plain m here
            sum += r * (std::f64::consts::PI * m1 * (i1 - 0.5) / 6.0).cos();
        }
        *slot = sum / 6.0;
    }
    g_hat
}

/// Wires bit prioritization ([`bit_prioritization::prioritize_bits`]), forward error correction
/// ([`fec`]), and bit modulation ([`modulation::modulate_code_vectors`]) together: takes the eight
/// prioritized bit vectors `u_hat_0..u_hat_7` and produces the final modulated code vectors
/// `c_hat_0..c_hat_7` -- this codec's own real final output (see this module's own doc comment on
/// why bit-interleaving into an actual channel frame is a separate protocol layer's concern, not
/// unfinished work here).
///
/// `u` must already be [`bit_prioritization::prioritize_bits`]'s own output: `u[0..=3]` fit in 12
/// bits, `u[4..=6]` in 11 bits, `u[7]` in 7 bits (this function doesn't re-check that, matching
/// [`fec::golay_encode`]/[`fec::hamming_encode`]'s own "trust the caller's own bit width" contract).
pub fn encode_code_vectors(u: [u32; 8]) -> [u32; 8] {
    let nu = [
        fec::golay_encode(u[0] as u16),
        fec::golay_encode(u[1] as u16),
        fec::golay_encode(u[2] as u16),
        fec::golay_encode(u[3] as u16),
        fec::hamming_encode(u[4] as u16) as u32,
        fec::hamming_encode(u[5] as u16) as u32,
        fec::hamming_encode(u[6] as u16) as u32,
        u[7],
    ];
    modulation::modulate_code_vectors(nu, u[0])
}

/// The per-frame state that carries forward into the *next* call to [`encode_frame`] -- Eq. 41's
/// energy tracker (`xi_max`) and Eq. 54's prediction residual (`l_hat`, `voiced`, and each
/// harmonic's own spectral amplitude) are all genuinely stateful across frames, unlike every other
/// stage in this pipeline. `spectral_amplitudes` here is real reconstructed history (via
/// [`reconstruct::reconstruct_spectral_amplitudes`]) for every frame after [`Self::initial`]'s own
/// frame-0 placeholder -- not the current frame's own unquantized estimate, which `encode_frame`
/// only ever uses locally to compute *this* frame's own residual, never stores.
///
/// [`Self::initial`] gives this module's own reasoned frame-0 initialization (see its doc comment
/// for exactly which pieces the spec itself dictates and which are our own low-stakes choice);
/// every later frame's state comes from the previous call's own return value. Fields are private --
/// [`Self::initial`] and `encode_frame`'s own returned state are the only ways to construct or
/// advance one, so a caller can't build a `FrameState` with a `voiced`/`spectral_amplitudes` length
/// that doesn't match its own `l_hat`.
pub struct FrameState {
    xi_max: f64,
    l_hat: u32,
    voiced: Vec<bool>,
    spectral_amplitudes: Vec<f64>,
}

impl FrameState {
    /// The initial state for the very first frame of a stream: no prior voicing history (every band
    /// defaults to unvoiced per `vuv::determine_voicing`'s own `unwrap_or(false)`), `xi_max` starting
    /// at `update_xi_max`'s own documented floor (`20000.0`; Eq. 41 has no real frame `-1` to draw an
    /// initial value from, so the floor -- not `0.0`, which would make the very first frame's own
    /// energy-tracker update jump by 50% of the frame's real energy per Eq. 41's first branch -- is
    /// the honest "no history yet" value), and `l_hat` from [`prediction::INITIAL_L_HAT_PREV`] (the
    /// spec's own literal initialization value for `L_hat_prev`).
    ///
    /// **`spectral_amplitudes`'s own initial value is our own choice, not a spec-stated one, and
    /// worth being precise about rather than mislabeling**: the spec initializes `L_hat_prev`
    /// explicitly but names no accompanying initial amplitude or log-amplitude history. `log2(1.0)
    /// == 0.0` is a flat *unity-amplitude* history (0 in the log2 domain means "unchanged," not
    /// "silent" -- true silence would be `-inf`), chosen specifically because it's a *constant*
    /// value: `prediction.rs`'s own `a_constant_previous_frame_level_cancels_out_of_the_residual`
    /// test already proves the bias-correction term exactly cancels any constant previous-frame
    /// level, so frame 0's residual is provably insensitive to which constant is picked here -- a
    /// genuinely low-stakes choice, not an arbitrary unverified one.
    pub fn initial() -> Self {
        let l_hat = prediction::INITIAL_L_HAT_PREV;
        FrameState {
            xi_max: 20000.0,
            l_hat,
            voiced: Vec::new(),
            spectral_amplitudes: vec![1.0; l_hat as usize],
        }
    }
}

/// Encodes one 20ms frame end to end (sections 5-7's own full pipeline, steps 1-8 in this module's
/// own doc comment above): from an analysis frame and its already-estimated fundamental frequency,
/// through voicing decision, spectral amplitude estimation, prediction, block DCT, quantization, bit
/// prioritization, FEC, and modulation, to the final 144-bit modulated code vectors
/// `c_hat_0..c_hat_7`.
///
/// Returns `None` if `l_hat` (derived from `omega0_hat` via `vuv::harmonics_count`) falls outside
/// Annex F/G/J's own tabulated `9..=56` range, or if bit prioritization's own total-bit-count check
/// fails -- both real preconditions this function doesn't re-validate itself, just propagates from
/// the stages that already do.
///
/// **Closes the gap `prediction`'s own doc comment used to disclose as still open**: the
/// `FrameState` this returns carries [`reconstruct::reconstruct_spectral_amplitudes`]'s own real
/// output -- this frame's quantizer values run back through dequantization and the inverse DCTs, the
/// same "what the decoder will actually have" history Eq. 54 needs -- not this encoder's own
/// unquantized estimate. The *next* call's `previous_state.spectral_amplitudes` is therefore real
/// reconstructed history, matching the spec's own closed-loop predictive design (Fig. 16's own
/// "Reconstruct" feedback block) rather than a placeholder.
pub fn encode_frame(
    frame: &pitch_refinement::RefinementFrame,
    omega0_hat: f64,
    initial_pitch_error: f64,
    previous_state: &FrameState,
    sync_bit: bool,
) -> Option<([u32; 8], FrameState)> {
    let l_hat = vuv::harmonics_count(omega0_hat);
    let k_hat = vuv::frequency_bands_count(l_hat);

    let (voiced, xi_max) = vuv::determine_voicing(
        frame,
        omega0_hat,
        initial_pitch_error,
        previous_state.xi_max,
        &previous_state.voiced,
    );

    let spectral_amplitudes =
        spectral_amplitude::estimate_spectral_amplitudes(frame, l_hat, k_hat, omega0_hat, &voiced);

    let residuals: Vec<f64> = (1..=l_hat)
        .map(|l| {
            prediction::prediction_residual(
                l,
                spectral_amplitudes[(l - 1) as usize],
                l_hat,
                previous_state.l_hat,
                &previous_state.spectral_amplitudes,
            )
        })
        .collect();

    let blocks = quantize::partition_into_blocks(&residuals, l_hat)?;
    let dct_blocks: [Vec<f64>; 6] = std::array::from_fn(|i| quantize::block_dct(&blocks[i]));
    // Each block's own DC term (Eq. 60's k=1 output) feeds the second-stage gain-vector DCT
    // (Eq. 61, Fig. 18) -- R_hat_i == dct_blocks[i][0], never empty since every real Annex J block
    // length is >= 1 (checked against tables::BLOCK_LENGTHS directly, not merely assumed).
    let r_hat: [f64; 6] = std::array::from_fn(|i| dct_blocks[i][0]);
    let g_hat = gain_vector_dct(&r_hat);

    let b0 = parameter_encoding::quantize_fundamental_frequency(omega0_hat);
    let b1 = parameter_encoding::encode_voicing_decisions(&voiced);
    let b2 = tables::quantize_gain_index(g_hat[0]);
    let gain_vector = quantize::quantize_gain_vector(&g_hat, l_hat)?;
    let higher_order = quantize::quantize_higher_order_coefficients(&dct_blocks, l_hat)?;

    let u = bit_prioritization::prioritize_bits(
        b0,
        b1,
        k_hat,
        b2 as u32,
        gain_vector,
        &higher_order,
        sync_bit,
    )?;

    let c = encode_code_vectors(u);

    // Real reconstructed history for the *next* frame's own prediction (Eq. 54 needs "what the
    // decoder will have," not this frame's own unquantized estimate above) -- reruns this frame's
    // own quantizer values back through dequantization and the inverse DCTs.
    let gain_values: [u32; 5] = std::array::from_fn(|i| gain_vector[i].0);
    let higher_order_values: Vec<u32> = higher_order.iter().map(|&(v, _)| v).collect();
    let reconstructed_spectral_amplitudes = reconstruct::reconstruct_spectral_amplitudes(
        b2,
        gain_values,
        &higher_order_values,
        l_hat,
        previous_state.l_hat,
        &previous_state.spectral_amplitudes,
    )?;

    Some((
        c,
        FrameState {
            xi_max,
            l_hat,
            voiced,
            spectral_amplitudes: reconstructed_spectral_amplitudes,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gain_vector_dct_of_a_constant_input_is_zero_except_the_first_coefficient() {
        // A constant r_hat is the DC case: the DCT's own m=1 (index 0) term collapses to the
        // plain sum (since cos(0) = 1 for every i), and every higher-order term integrates to
        // zero for a constant input -- the same real, checkable property `spectral_tilt_db_per_
        // octave`'s own DCT-adjacent tests in the daemon crate already lean on for validating a
        // transform implementation before trusting it against anything harder.
        let r_hat = [2.0; 6];
        let g_hat = gain_vector_dct(&r_hat);
        assert!(
            (g_hat[0] - 2.0).abs() < 1e-9,
            "expected the DC term to equal the constant input, got {}",
            g_hat[0]
        );
        for (m, &g) in g_hat.iter().enumerate().skip(1) {
            assert!(
                g.abs() < 1e-9,
                "expected higher-order term {m} to vanish for constant input, got {g}"
            );
        }
    }

    #[test]
    fn gain_vector_dct_matches_a_hand_computed_value_for_a_real_asymmetric_input() {
        // A real, non-constant input, computed independently by hand (not by calling the
        // function under test with different inputs and hoping) -- guards against a sign error
        // or an off-by-one in the (i - 0.5) term that the constant-input test above can't catch
        // (a constant input is symmetric under exactly that class of bug).
        let r_hat = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let g_hat = gain_vector_dct(&r_hat);
        // With only r_hat[0] = 1 nonzero (i=1 in the spec's 1-indexing), Eq. 61 collapses to
        // G_hat_m = (1/6) * cos(pi*(m-1)*0.5/6) for each m.
        for (m, &g) in g_hat.iter().enumerate() {
            let expected = (std::f64::consts::PI * m as f64 * 0.5 / 6.0).cos() / 6.0;
            assert!(
                (g - expected).abs() < 1e-9,
                "m={m}: expected {expected}, got {g}"
            );
        }
    }

    #[test]
    fn encode_code_vectors_produces_exactly_frame_bits_across_all_eight_vectors() {
        let u = [
            0b101010101010u32,
            0xABC,
            0x123,
            0x555,
            0x321,
            0x654,
            0x2AA,
            0b1011010,
        ];
        let c = encode_code_vectors(u);
        let widths = [23u32, 23, 23, 23, 15, 15, 15, 7];
        assert_eq!(widths.iter().sum::<u32>() as usize, FRAME_BITS);
        for (&value, &width) in c.iter().zip(widths.iter()) {
            assert!(
                value < (1 << width),
                "value {value} doesn't fit in {width} bits"
            );
        }
    }

    #[test]
    fn encode_code_vectors_leaves_c0_and_c7_unmodulated() {
        // m_hat_0 and m_hat_7 are always all-zero (modulation.rs), so c0 is exactly u0's own
        // Golay codeword and c7 is exactly u7 itself, unmodified by modulation.
        let u = [0xABC, 0, 0, 0, 0, 0, 0, 0b1010101];
        let c = encode_code_vectors(u);
        assert_eq!(c[0], fec::golay_encode(u[0] as u16));
        assert_eq!(c[7], u[7]);
    }

    /// The real composition test the per-stage unit tests can't catch: a genuine synthetic
    /// harmonic signal (same construction `pitch_refinement`/`vuv`/`spectral_amplitude`'s own tests
    /// already use) run through the entire pipeline end to end, twice in a row -- the second call
    /// using the first's own returned `FrameState`, so the stateful seams (Eq. 41's `xi_max`,
    /// Eq. 54's prediction against real prior history) are actually exercised, not just the
    /// degenerate `FrameState::initial()` case.
    #[test]
    fn encode_frame_produces_a_full_frame_from_a_real_synthetic_harmonic_signal() {
        fn harmonic_signal(
            fundamental_hz: f64,
            sample_rate: f64,
            num_harmonics: u32,
            total_len: usize,
        ) -> Vec<f64> {
            (0..total_len)
                .map(|n| {
                    let t = n as f64 / sample_rate;
                    (1..=num_harmonics)
                        .map(|k| {
                            (1.0 / k as f64)
                                * (2.0 * std::f64::consts::PI * fundamental_hz * k as f64 * t).sin()
                        })
                        .sum::<f64>()
                })
                .collect()
        }

        let sample_rate = 8000.0;
        let period = 80.0;
        let fundamental_hz = sample_rate / period;
        let omega0_hat = 2.0 * std::f64::consts::PI / period;

        let raw = harmonic_signal(fundamental_hz, sample_rate, 36, 400);
        let frame = pitch_refinement::RefinementFrame::new(&raw, 200);

        let widths = [23u32, 23, 23, 23, 15, 15, 15, 7];

        let initial = FrameState::initial();
        let (c1, next_state) = encode_frame(&frame, omega0_hat, 0.01, &initial, false)
            .expect("a real, in-range harmonic signal should produce a valid frame");
        for (&value, &width) in c1.iter().zip(widths.iter()) {
            assert!(
                value < (1 << width),
                "frame 1: value {value} doesn't fit in {width} bits"
            );
        }

        let (c2, _) = encode_frame(&frame, omega0_hat, 0.01, &next_state, true)
            .expect("a second frame with real prior state should also produce a valid frame");
        for (&value, &width) in c2.iter().zip(widths.iter()) {
            assert!(
                value < (1 << width),
                "frame 2: value {value} doesn't fit in {width} bits"
            );
        }
    }
}
