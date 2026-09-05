// SPDX-License-Identifier: LGPL-3.0-or-later
//! An independently-authored Rust port of Codec2's 1600bps mode --
//! the codec M17's "Voice + Data" stream type uses (`M17_spec.tex`
//! line 727: "64 bits encoded speech + 64 bits arbitrary data" per
//! 40ms frame; this module implements the speech half only, not M17's
//! own framing/FEC/sync layer, which is a separate, larger, and
//! currently-shelved concern -- see
//! `hams_com/docs/proposals/blocked/M17_IMPLEMENTATION_PLAN.md`).
//!
//! Written the same way `codec2_3200` was: a from-scratch reading of
//! plain upstream Codec2's *algorithm* (`codec2_encode_1600`/
//! `codec2_decode_1600` in `codec2.c`, `encode_lsps_scalar`/
//! `decode_lsps_scalar`/`bw_expand_lsps`/`check_lsp_order`/
//! `interpolate_lsp_ver2` in `quantise.c`/`interp.c`), not translated
//! line-by-line -- informed by, not a derivative of, that LGPL-2.1-only
//! source (same reasoning `codec2_3200::mod`'s own doc comment gives,
//! and the same license: plain upstream Codec2 is LGPL-2.1-only, same
//! trap `vendor/codec2-mod/VENDORED_FROM.md` documents for the M17
//! fork). Reference used: a local unmodified build of plain upstream
//! Codec2 (not `vendor/codec2-mod`, which has been stripped down to
//! 3200bps only and has no 1600bps mode at all -- confirmed by reading
//! its own `codec2_mod.c`, 149 lines, no mode dispatch).
//!
//! 1600bps shares almost its entire signal-processing pipeline with
//! 3200bps -- pitch estimation, voicing decision, LPC analysis,
//! LSP<->LPC conversion, spectral envelope reconstruction, sinusoidal
//! synthesis are all genuinely mode-independent and reused directly
//! from `codec2_3200` (`floating_reference::{nlp, voicing, lpc}`,
//! `codec2_3200::{nlp::f0_to_wo, window, lpc::lsp_to_lpc, envelope,
//! synthesis, interp::{interp_wo, interp_voiced, interp_energy}}`).
//! What's genuinely new here: a different LSP quantizer (ten
//! independent per-dimension scalar codebooks instead of one 10-D
//! delta-scalar quantizer -- `lsp_quantiser.rs`), LSP order/bandwidth
//! post-processing and 3-way interpolation 40ms-cadence LSPs need that
//! 3200bps's 20ms cadence never does (`lsp_post.rs`), and a materially
//! different 64-bit frame layout (`bits.rs`).
//!
//! Frame structure, matching the real reference exactly (see
//! `bits.rs`'s own doc comment for the bit layout): 320 samples (40ms)
//! encoded as four 10ms sub-frame analysis passes -- sub-frames 0 and
//! 2 contribute only a voicing bit, sub-frames 1 and 3 each contribute
//! a voicing bit plus a transmitted `Wo`/energy pair (20ms cadence,
//! same as 3200bps), and sub-frame 3's own LPC/LSP analysis is the one
//! that gets quantized and transmitted (LSPs update only once per
//! 40ms). Decode interpolates sub-frames 0 and 2 from their neighbors
//! (`interp_wo`/`interp_energy`, unchanged from 3200bps) and
//! sub-frames 0/1/2 all interpolate their own LSPs from the previous
//! frame's decoded LSPs and this frame's newly decoded ones at weights
//! 0.25/0.5/0.75 (`lsp_post::interpolate_lsp_ver2`); sub-frame 3 uses
//! its own newly decoded LSPs directly.
//!
//! `apply_lpc_correction` (upstream) turned out, on inspection, to be
//! byte-for-byte the same formula `codec2_3200::envelope::
//! apply_first_harmonic_correction` already implements (`Wo < PI*150/
//! 4000` -> `A[1] *= 0.032`) -- reused directly, not reimplemented.
//!
//! **Encoder verified against the real reference decoder.**
//! `examples/codec2_1600_encode_raw.rs` encoded a locally synthesized
//! test signal in Rust and fed the resulting bitstream to an
//! unmodified, separately-built real `c2dec 1600`. Comparing that C
//! decode against *this crate's own* `Decoder`'s decode of the exact
//! same Rust-encoded bitstream (the meaningful check -- comparing
//! decoded vocoder output against the undistorted original waveform
//! isn't, since a sinusoidal codec's reconstruction doesn't preserve
//! fine time-domain structure against the input even when working
//! correctly, the same reasoning `codec2_3200::mod`'s own doc comment
//! gives for checking RMS, not correlation, against original input on
//! its own equivalent check) gave correlation **0.99996** -- the real
//! reference decoder and this crate's own decoder reconstruct
//! essentially identical audio from this crate's own encoder's output,
//! real cross-implementation interoperability in both directions, not
//! just internal self-consistency.
//!
//! **Decoder verified against the real reference decoder** -- see
//! this module's own `#[cfg(test)]` block: two real captured
//! bitstream/PCM fixtures (`tests/fixtures/codec2_1600/`), one mixed
//! voiced/noise content (correlation 0.9492, investigated and
//! attributed to real by-design PRNG divergence on the noise-dominated
//! blocks, not a defect) and one mostly-voiced (correlation 0.99996,
//! matching 3200bps's own bar on equivalent content) -- both with RMS
//! ratio within 5% of the reference, ruling out a gain/scale bug.
//!
//! **Fixed-point (`EncoderFixed`/`DecoderFixed`) verified the same
//! two ways.** `DecoderFixed` against the mostly-voiced fixture:
//! correlation 0.998, RMS ratio ~1.0005. `EncoderFixed`'s own
//! bitstream, fed to the real `c2dec 1600` and compared against this
//! crate's own `DecoderFixed` decoding the same bitstream (the same
//! cross-implementation check the float `Encoder` got above):
//! correlation 0.998. `DecoderFixed` is genuinely `i64` fixed-point end
//! to end, `f32` only at the final PCM sample conversion.
//! `EncoderFixed` keeps one honest `f32` boundary -- the LSP array
//! between `lpc::lpc_to_lsp_from_integer_ak` (whose own root-finding
//! returns `f32` even though its internal `acos()` is a fixed-point
//! LUT) and this mode's own scalar quantizer -- the exact same
//! boundary `codec2_3200::EncoderFixed`'s own doc comment documents
//! and accepts, not a gap unique to this port.

pub mod bits;
pub mod lsp_post;
pub mod lsp_quantiser;

use crate::codec2_3200::floating_reference::{lpc as flpc, nlp as fnlp, voicing as fvoicing};
use crate::codec2_3200::{
    self, bw_gamma, envelope, interp, lpc, nlp, quantise, synthesis, voicing, window, LPC_ORD,
    M_PITCH, N_SAMP,
};

/// LSPs to substitute when `lpc::lpc_to_lsp` fails to find all
/// `LPC_ORD` roots -- same fallback `codec2_3200::fallback_lsp` uses
/// (that function is private to its own module, so this is the same
/// evenly-spaced-across-`[0,pi]` construction, not a re-export).
fn fallback_lsp() -> [f32; LPC_ORD] {
    std::array::from_fn(|i| (std::f32::consts::PI / LPC_ORD as f32) * i as f32)
}

/// One 1600bps frame: 64 bits (8 bytes) covering 40ms of audio (four
/// 10ms, `N_SAMP`-sample sub-frames) -- 64 bits / 40ms = 1600bps.
pub const BYTES_PER_FRAME: usize = 8;
pub const SAMPLES_PER_FRAME: usize = 4 * N_SAMP;

/// Persistent per-call encoder state -- same shape as
/// `codec2_3200::floating_reference::Encoder`'s own, since the
/// underlying analysis pipeline (pitch/voicing/LPC) is identical; this
/// mode just runs it across four 10ms steps instead of two, and
/// quantizes/transmits LSPs from only the last of them.
pub struct Encoder {
    sn: [f32; M_PITCH],
    analysis_window: [f32; M_PITCH],
    nlp_state: fnlp::NlpState,
    voicing_state: fvoicing::VoicingState,
}

impl Default for Encoder {
    fn default() -> Self {
        Encoder {
            sn: [0.0; M_PITCH],
            analysis_window: window::make_analysis_window(),
            nlp_state: fnlp::NlpState::new(),
            voicing_state: fvoicing::VoicingState::new(),
        }
    }
}

/// One 20ms LPC/LSP analysis pass over the current `M_PITCH`-sample
/// history window -- identical to the inline block in
/// `floating_reference::Encoder::encode`, factored out here since
/// 1600bps runs it twice per 40ms frame but only quantizes/transmits
/// the second pass's own LSPs (the first pass's LSPs are computed only
/// because `e` falls out of the same analysis, matching the
/// reference's own `speech_to_uq_lsps` call for that purpose).
fn analyse_lsps_and_energy(sn: &[f32; M_PITCH], analysis_window: &[f32; M_PITCH]) -> ([f32; LPC_ORD], f32) {
    let mut windowed = [0.0f32; M_PITCH];
    for ((w, &s), &win) in windowed.iter_mut().zip(sn.iter()).zip(analysis_window.iter()) {
        *w = s * win;
    }
    let r = flpc::autocorrelate(&windowed);
    let mut r_for_levinson = r;
    flpc::apply_white_noise_correction(&mut r_for_levinson);
    let mut ak = flpc::levinson_durbin(&r_for_levinson);
    let e = flpc::lpc_energy(&ak, &r);
    for (i, a) in ak.iter_mut().enumerate() {
        *a *= bw_gamma(i);
    }
    let lsp = flpc::lpc_to_lsp(&ak).unwrap_or_else(fallback_lsp);
    (lsp, e)
}

impl Encoder {
    pub fn new() -> Self {
        Self::default()
    }

    fn shift_in(&mut self, new_samples: &[i16]) {
        self.sn.copy_within(N_SAMP.., 0);
        for (dst, &s) in self.sn[M_PITCH - N_SAMP..].iter_mut().zip(new_samples) {
            *dst = s as f32;
        }
    }

    /// Encodes one 40ms (`SAMPLES_PER_FRAME`-sample) frame into
    /// `BYTES_PER_FRAME` real-format bytes. Matches
    /// `codec2_encode_1600`'s own real structure exactly (see this
    /// module's own doc comment).
    pub fn encode(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
        self.shift_in(&speech[0..N_SAMP]);
        fnlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced0 = fvoicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        self.shift_in(&speech[N_SAMP..2 * N_SAMP]);
        let f0_a = fnlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced1 = fvoicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);
        let wo_index_a = quantise::encode_wo(nlp::f0_to_wo(f0_a));
        let (_lsp_a_unused, e_a) = analyse_lsps_and_energy(&self.sn, &self.analysis_window);
        let e_index_a = quantise::encode_energy(e_a);

        self.shift_in(&speech[2 * N_SAMP..3 * N_SAMP]);
        fnlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced2 = fvoicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        self.shift_in(&speech[3 * N_SAMP..4 * N_SAMP]);
        let f0_b = fnlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced3 = fvoicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);
        let wo_index_b = quantise::encode_wo(nlp::f0_to_wo(f0_b));
        let (lsp_b, e_b) = analyse_lsps_and_energy(&self.sn, &self.analysis_window);
        let e_index_b = quantise::encode_energy(e_b);
        let lsp_indexes = lsp_quantiser::encode_lsps_scalar(&lsp_b);

        bits::pack_frame_1600(&bits::FrameFields1600 {
            voiced0,
            voiced1,
            wo_index_a,
            e_index_a,
            voiced2,
            voiced3,
            wo_index_b,
            e_index_b,
            lsp_indexes,
        })
    }
}

/// LSPs the reference's own decoder starts from before any real frame
/// has been decoded -- same fallback `codec2_3200::initial_lsps` uses
/// (its own function is private to that module, so this is the same
/// evenly-spaced-across-`[0,pi]` construction, not a re-export).
fn initial_lsps() -> [f32; LPC_ORD] {
    std::array::from_fn(|i| (i as f32 * std::f32::consts::PI) / (LPC_ORD as f32 + 1.0))
}

/// Persistent per-decoder state: the previous frame's own decoded
/// `Wo`/voiced/LSPs/energy (sub-frame 3's own, needed to interpolate
/// the next frame's sub-frames 0/1/2), plus `synthesis::SynthesisState`'s
/// own overlap-add memory and a forward FFT plan. `spectral_bridge` is
/// the same opt-in 16kHz harmonic-extension state
/// `codec2_3200::Decoder` carries -- see `decode_16k` below and
/// `codec2_3200::spectral_bridge`'s own doc comment; `.enabled` toggles
/// it (on by default), the ordinary `decode()` above never touches it.
pub struct Decoder {
    prev_wo: f32,
    prev_voiced: bool,
    prev_lsps: [f32; LPC_ORD],
    prev_e: f32,
    synth: synthesis::SynthesisState,
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    pub spectral_bridge: codec2_3200::spectral_bridge::SpectralBridgeState,
}

impl Default for Decoder {
    fn default() -> Self {
        let mut planner = rustfft::FftPlanner::<f32>::new();
        Decoder {
            prev_wo: codec2_3200::W0_MIN,
            prev_voiced: false,
            prev_lsps: initial_lsps(),
            prev_e: 1.0,
            synth: synthesis::SynthesisState::new(),
            fft: planner.plan_fft_forward(codec2_3200::FFT_ENC),
            spectral_bridge: codec2_3200::spectral_bridge::SpectralBridgeState::new(),
        }
    }
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `BYTES_PER_FRAME` real-format bytes into one 40ms
    /// (`SAMPLES_PER_FRAME`-sample) frame of audio. Matches
    /// `codec2_decode_1600`'s own real structure exactly (see this
    /// module's own doc comment).
    pub fn decode(&mut self, bytes: &[u8; BYTES_PER_FRAME]) -> [i16; SAMPLES_PER_FRAME] {
        let fields = bits::unpack_frame_1600(bytes);
        let wo_a = quantise::decode_wo(fields.wo_index_a);
        let e_a = quantise::decode_energy(fields.e_index_a);
        let wo_b = quantise::decode_wo(fields.wo_index_b);
        let e_b = quantise::decode_energy(fields.e_index_b);

        let mut lsps3 = lsp_quantiser::decode_lsps_scalar(&fields.lsp_indexes);
        lsp_post::check_lsp_order(&mut lsps3);
        lsp_post::bw_expand_lsps(&mut lsps3, 50.0, 100.0);

        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo(
            fields.voiced0,
            self.prev_wo,
            self.prev_voiced,
            wo_a,
            fields.voiced1,
            codec2_3200::W0_MIN,
        );
        let e0 = interp::interp_energy(self.prev_e, e_a);

        let voiced2 = interp::interp_voiced(fields.voiced2, fields.voiced1, fields.voiced3);
        let wo2 = interp::interp_wo(
            fields.voiced2,
            wo_a,
            fields.voiced1,
            wo_b,
            fields.voiced3,
            codec2_3200::W0_MIN,
        );
        let e2 = interp::interp_energy(e_a, e_b);

        let lsps0 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.25);
        let lsps1 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.5);
        let lsps2 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.75);

        let mut out = [0i16; SAMPLES_PER_FRAME];
        let subframes = [
            (wo0, voiced0, lsps0, e0),
            (wo_a, fields.voiced1, lsps1, e_a),
            (wo2, voiced2, lsps2, e2),
            (wo_b, fields.voiced3, lsps3, e_b),
        ];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc(&lsps);
            let mut model = envelope::Model::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes(self.fft.as_ref(), &ak, e, &mut model);
            envelope::apply_first_harmonic_correction(&mut model);
            let sub = self.synth.synthesize_subframe(&mut model, &aw);
            out[i * N_SAMP..(i + 1) * N_SAMP].copy_from_slice(&sub);
        }

        self.prev_wo = wo_b;
        self.prev_voiced = fields.voiced3;
        self.prev_lsps = lsps3;
        self.prev_e = e_b;

        out
    }

    /// Opt-in 16kHz decode, the same Spectral Bridge extension
    /// `codec2_3200::Decoder::decode_16k` provides -- see that method's
    /// own doc comment and `codec2_3200::spectral_bridge`'s module doc
    /// comment for the mechanism. Deliberately a *separate* method from
    /// `decode()` above, which this leaves byte-for-byte untouched;
    /// duplicates `decode()`'s own four-sub-frame loop structure rather
    /// than sharing it, for the same reason `codec2_3200`'s own
    /// `decode_16k` does: zero risk of this newer, less-validated path
    /// regressing the real, reference-validated 8kHz one.
    ///
    /// **A given `Decoder` serves one rate or the other, not both**:
    /// this and `decode()` both advance the same inter-frame state
    /// (`prev_wo`/`prev_voiced`/`prev_lsps`/`prev_e`, `self.synth`'s own
    /// overlap-add memory) from the same bitstream. Calling both on one
    /// instance for the same frame sequence advances that shared state
    /// twice per frame, corrupting the next frame's interpolation for
    /// whichever call runs second. Use two separate `Decoder`s (as this
    /// module's own tests do) if both rates are ever needed from the
    /// same stream.
    pub fn decode_16k(
        &mut self,
        bytes: &[u8; BYTES_PER_FRAME],
    ) -> [i16; 4 * codec2_3200::spectral_bridge::N_SAMP_SB] {
        let fields = bits::unpack_frame_1600(bytes);
        let wo_a = quantise::decode_wo(fields.wo_index_a);
        let e_a = quantise::decode_energy(fields.e_index_a);
        let wo_b = quantise::decode_wo(fields.wo_index_b);
        let e_b = quantise::decode_energy(fields.e_index_b);

        let mut lsps3 = lsp_quantiser::decode_lsps_scalar(&fields.lsp_indexes);
        lsp_post::check_lsp_order(&mut lsps3);
        lsp_post::bw_expand_lsps(&mut lsps3, 50.0, 100.0);

        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo(
            fields.voiced0,
            self.prev_wo,
            self.prev_voiced,
            wo_a,
            fields.voiced1,
            codec2_3200::W0_MIN,
        );
        let e0 = interp::interp_energy(self.prev_e, e_a);

        let voiced2 = interp::interp_voiced(fields.voiced2, fields.voiced1, fields.voiced3);
        let wo2 = interp::interp_wo(
            fields.voiced2,
            wo_a,
            fields.voiced1,
            wo_b,
            fields.voiced3,
            codec2_3200::W0_MIN,
        );
        let e2 = interp::interp_energy(e_a, e_b);

        let lsps0 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.25);
        let lsps1 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.5);
        let lsps2 = lsp_post::interpolate_lsp_ver2(&self.prev_lsps, &lsps3, 0.75);

        let mut out = [0i16; 4 * codec2_3200::spectral_bridge::N_SAMP_SB];
        let subframes = [
            (wo0, voiced0, lsps0, e0),
            (wo_a, fields.voiced1, lsps1, e_a),
            (wo2, voiced2, lsps2, e2),
            (wo_b, fields.voiced3, lsps3, e_b),
        ];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc(&lsps);
            let mut model = envelope::Model::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes(self.fft.as_ref(), &ak, e, &mut model);
            envelope::apply_first_harmonic_correction(&mut model);
            // Populates model.phi[1..=l] with its own final phase (the
            // spectral bridge synthesis below reuses this unchanged for
            // harmonics 1..=l) -- discards the 8kHz output itself, this
            // method's own caller wants the 16kHz one instead.
            let _sub = self.synth.synthesize_subframe(&mut model, &aw);
            let sub_sb = self.spectral_bridge.synthesize_subframe_sb(&model);
            let n = codec2_3200::spectral_bridge::N_SAMP_SB;
            out[i * n..(i + 1) * n].copy_from_slice(&sub_sb);
        }

        self.prev_wo = wo_b;
        self.prev_voiced = fields.voiced3;
        self.prev_lsps = lsps3;
        self.prev_e = e_b;

        out
    }
}

const FRAC_BITS: u32 = 23;

fn w0_min_q23() -> i64 {
    static V: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *V.get_or_init(|| crate::codec2_3200::fixed_point::f32_to_q_exact_round(codec2_3200::W0_MIN, FRAC_BITS))
}

fn initial_lsps_q23() -> [i64; LPC_ORD] {
    static V: std::sync::OnceLock<[i64; LPC_ORD]> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        let lsps = initial_lsps();
        std::array::from_fn(|i| crate::codec2_3200::fixed_point::f32_to_q_exact_round(lsps[i], FRAC_BITS))
    })
}

/// Persistent per-call fixed-point encoder state -- same shape as
/// `codec2_3200::EncoderFixed`'s own (see that struct's own doc comment
/// for the real, honest current state of what's genuinely fixed-point
/// end to end): `nlp::NlpStateFixed`/`voicing::VoicingStateFixed` and
/// the windowing->autocorrelate->Levinson-Durbin->energy->bandwidth-
/// expansion->LSP chain are all genuinely `i64` fixed-point, no `f32`
/// anywhere in that path; the one remaining `f32` boundary is the LSP
/// array itself between `lpc::lpc_to_lsp_from_integer_ak` (whose own
/// root-finding returns `f32` LSPs even though its internal `acos()`
/// call is a fixed-point LUT) and this mode's own LSP quantizer --
/// exactly the same boundary `codec2_3200::EncoderFixed`'s own doc
/// comment documents and accepts, not a gap unique to this port.
pub struct EncoderFixed {
    sn: [i16; M_PITCH],
    window_fixed: [i32; M_PITCH],
    nlp_state: nlp::NlpStateFixed,
    voicing_state: voicing::VoicingStateFixed,
}

impl Default for EncoderFixed {
    fn default() -> Self {
        EncoderFixed {
            sn: [0; M_PITCH],
            window_fixed: window::make_analysis_window_fixed(),
            nlp_state: nlp::NlpStateFixed::new(),
            voicing_state: voicing::VoicingStateFixed::new(),
        }
    }
}

/// Fixed-point sibling of `analyse_lsps_and_energy`: the same real
/// windowing->autocorrelate->white-noise-correction->Levinson-Durbin->
/// energy->bandwidth-expansion->LSP-conversion chain
/// `codec2_3200::EncoderFixed::encode` already validated, factored out
/// here for the same reason its float sibling was (1600bps runs it
/// twice per 40ms, quantizing only the second pass's own LSPs).
fn analyse_lsps_and_energy_fixed(sn: &[i16; M_PITCH], window_fixed: &[i32; M_PITCH]) -> ([f32; LPC_ORD], f32) {
    let mut wn_q = [0i32; M_PITCH];
    for ((w, &s), &win) in wn_q.iter_mut().zip(sn.iter()).zip(window_fixed.iter()) {
        *w = ((s as i64 * win as i64) >> 7) as i32;
    }
    let r_q = lpc::autocorrelate_fixed(&wn_q);
    let mut r_q_for_levinson = r_q;
    lpc::apply_white_noise_correction_fixed(&mut r_q_for_levinson);
    let (_ak, mut a_q23) = lpc::levinson_durbin_fixed_from_integer_r(&r_q_for_levinson);
    let e = lpc::lpc_energy_fixed(&a_q23, &r_q);
    lpc::apply_bw_gamma_fixed(&mut a_q23);
    let lsp = lpc::lpc_to_lsp_from_integer_ak(&a_q23).unwrap_or_else(fallback_lsp);
    (lsp, e)
}

impl EncoderFixed {
    pub fn new() -> Self {
        Self::default()
    }

    fn shift_in(&mut self, new_samples: &[i16]) {
        self.sn.copy_within(N_SAMP.., 0);
        self.sn[M_PITCH - N_SAMP..].copy_from_slice(new_samples);
    }

    /// Same real frame structure as `Encoder::encode` (see this
    /// module's own doc comment) -- this is `EncoderFixed`'s own
    /// mirror.
    pub fn encode(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
        self.shift_in(&speech[0..N_SAMP]);
        nlp::nlp_fixed(&mut self.nlp_state, &self.sn);
        let voiced0 = voicing::is_voiced_fixed(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        self.shift_in(&speech[N_SAMP..2 * N_SAMP]);
        let f0_a = nlp::nlp_fixed(&mut self.nlp_state, &self.sn);
        let voiced1 = voicing::is_voiced_fixed(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);
        let wo_index_a = quantise::encode_wo(nlp::f0_to_wo(f0_a));
        let (_lsp_a_unused, e_a) = analyse_lsps_and_energy_fixed(&self.sn, &self.window_fixed);
        let e_index_a = quantise::encode_energy(e_a);

        self.shift_in(&speech[2 * N_SAMP..3 * N_SAMP]);
        nlp::nlp_fixed(&mut self.nlp_state, &self.sn);
        let voiced2 = voicing::is_voiced_fixed(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        self.shift_in(&speech[3 * N_SAMP..4 * N_SAMP]);
        let f0_b = nlp::nlp_fixed(&mut self.nlp_state, &self.sn);
        let voiced3 = voicing::is_voiced_fixed(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);
        let wo_index_b = quantise::encode_wo(nlp::f0_to_wo(f0_b));
        let (lsp_b, e_b) = analyse_lsps_and_energy_fixed(&self.sn, &self.window_fixed);
        let e_index_b = quantise::encode_energy(e_b);
        let lsp_indexes = lsp_quantiser::encode_lsps_scalar(&lsp_b);

        bits::pack_frame_1600(&bits::FrameFields1600 {
            voiced0,
            voiced1,
            wo_index_a,
            e_index_a,
            voiced2,
            voiced3,
            wo_index_b,
            e_index_b,
            lsp_indexes,
        })
    }
}

/// Fixed-point sibling of `Decoder` -- genuinely integer end to end, no
/// `f32` anywhere except the final `i16` PCM boundary each sub-frame's
/// own `synthesize_subframe_fixed` call already handles, same standard
/// `codec2_3200::DecoderFixed` established.
pub struct DecoderFixed {
    prev_wo: i64,
    prev_voiced: bool,
    prev_lsps: [i64; LPC_ORD],
    prev_e: i64,
    synth: synthesis::SynthesisStateFixed,
    pub(crate) spectral_bridge: codec2_3200::spectral_bridge::SpectralBridgeStateFixed,
}

impl Default for DecoderFixed {
    fn default() -> Self {
        DecoderFixed {
            prev_wo: w0_min_q23(),
            prev_voiced: false,
            prev_lsps: initial_lsps_q23(),
            prev_e: 1i64 << FRAC_BITS,
            synth: synthesis::SynthesisStateFixed::new(),
            spectral_bridge: codec2_3200::spectral_bridge::SpectralBridgeStateFixed::new(),
        }
    }
}

impl DecoderFixed {
    pub fn new() -> Self {
        Self::default()
    }

    /// Same real frame structure as `Decoder::decode` (see this
    /// module's own doc comment) -- this is `DecoderFixed`'s own
    /// mirror, genuinely fixed-point end to end.
    pub fn decode(&mut self, bytes: &[u8; BYTES_PER_FRAME]) -> [i16; SAMPLES_PER_FRAME] {
        let fields = bits::unpack_frame_1600(bytes);
        let wo_a = quantise::decode_wo_fixed(fields.wo_index_a);
        let e_a = quantise::decode_energy_fixed(fields.e_index_a);
        let wo_b = quantise::decode_wo_fixed(fields.wo_index_b);
        let e_b = quantise::decode_energy_fixed(fields.e_index_b);

        let mut lsps3 = lsp_quantiser::decode_lsps_scalar_fixed(&fields.lsp_indexes);
        lsp_post::check_lsp_order_fixed(&mut lsps3);
        lsp_post::bw_expand_lsps_fixed(&mut lsps3, lsp_post::min_sep_low_q23(), lsp_post::min_sep_high_q23());

        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo_fixed(
            fields.voiced0,
            self.prev_wo,
            self.prev_voiced,
            wo_a,
            fields.voiced1,
            w0_min_q23(),
        );
        let e0 = interp::interp_energy_fixed(self.prev_e, e_a);

        let voiced2 = interp::interp_voiced(fields.voiced2, fields.voiced1, fields.voiced3);
        let wo2 = interp::interp_wo_fixed(
            fields.voiced2,
            wo_a,
            fields.voiced1,
            wo_b,
            fields.voiced3,
            w0_min_q23(),
        );
        let e2 = interp::interp_energy_fixed(e_a, e_b);

        let lsps0 = lsp_post::interpolate_lsp_ver2_fixed(&self.prev_lsps, &lsps3, 1);
        let lsps1 = lsp_post::interpolate_lsp_ver2_fixed(&self.prev_lsps, &lsps3, 2);
        let lsps2 = lsp_post::interpolate_lsp_ver2_fixed(&self.prev_lsps, &lsps3, 3);

        let mut out = [0i16; SAMPLES_PER_FRAME];
        let subframes = [
            (wo0, voiced0, lsps0, e0),
            (wo_a, fields.voiced1, lsps1, e_a),
            (wo2, voiced2, lsps2, e2),
            (wo_b, fields.voiced3, lsps3, e_b),
        ];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc_fixed(&lsps);
            let mut model = envelope::ModelFixed::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes_fixed(&ak, e, &mut model);
            envelope::apply_first_harmonic_correction_fixed(&mut model);
            let sub = self.synth.synthesize_subframe_fixed(&mut model, &aw);
            out[i * N_SAMP..(i + 1) * N_SAMP].copy_from_slice(&sub);
        }

        self.prev_wo = wo_b;
        self.prev_voiced = fields.voiced3;
        self.prev_lsps = lsps3;
        self.prev_e = e_b;

        out
    }

    /// Fixed-point sibling of `Decoder::decode_16k` (this module's own
    /// float version) -- see that method's own doc comment for the
    /// design and the same warning about not interleaving
    /// `decode()`/`decode_16k_fixed()` on one instance.
    pub fn decode_16k_fixed(
        &mut self,
        bytes: &[u8; BYTES_PER_FRAME],
    ) -> [i16; 4 * codec2_3200::spectral_bridge::N_SAMP_SB] {
        let fields = bits::unpack_frame_1600(bytes);
        let wo_a = quantise::decode_wo_fixed(fields.wo_index_a);
        let e_a = quantise::decode_energy_fixed(fields.e_index_a);
        let wo_b = quantise::decode_wo_fixed(fields.wo_index_b);
        let e_b = quantise::decode_energy_fixed(fields.e_index_b);

        let mut lsps3 = lsp_quantiser::decode_lsps_scalar_fixed(&fields.lsp_indexes);
        lsp_post::check_lsp_order_fixed(&mut lsps3);
        lsp_post::bw_expand_lsps_fixed(&mut lsps3, lsp_post::min_sep_low_q23(), lsp_post::min_sep_high_q23());

        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo_fixed(
            fields.voiced0,
            self.prev_wo,
            self.prev_voiced,
            wo_a,
            fields.voiced1,
            w0_min_q23(),
        );
        let e0 = interp::interp_energy_fixed(self.prev_e, e_a);

        let voiced2 = interp::interp_voiced(fields.voiced2, fields.voiced1, fields.voiced3);
        let wo2 = interp::interp_wo_fixed(
            fields.voiced2,
            wo_a,
            fields.voiced1,
            wo_b,
            fields.voiced3,
            w0_min_q23(),
        );
        let e2 = interp::interp_energy_fixed(e_a, e_b);

        let lsps0 = lsp_post::interpolate_lsp_ver2_fixed(&self.prev_lsps, &lsps3, 1);
        let lsps1 = lsp_post::interpolate_lsp_ver2_fixed(&self.prev_lsps, &lsps3, 2);
        let lsps2 = lsp_post::interpolate_lsp_ver2_fixed(&self.prev_lsps, &lsps3, 3);

        let mut out = [0i16; 4 * codec2_3200::spectral_bridge::N_SAMP_SB];
        let subframes = [
            (wo0, voiced0, lsps0, e0),
            (wo_a, fields.voiced1, lsps1, e_a),
            (wo2, voiced2, lsps2, e2),
            (wo_b, fields.voiced3, lsps3, e_b),
        ];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc_fixed(&lsps);
            let mut model = envelope::ModelFixed::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes_fixed(&ak, e, &mut model);
            envelope::apply_first_harmonic_correction_fixed(&mut model);
            // Populates model.phi[1..=l] as a side effect, reused
            // unchanged by the spectral bridge synthesis below -- same
            // reasoning as decode_16k's own float version.
            let _sub = self.synth.synthesize_subframe_fixed(&mut model, &aw);
            let sub_sb = self.spectral_bridge.synthesize_subframe_sb_fixed(&model);
            let n = codec2_3200::spectral_bridge::N_SAMP_SB;
            out[i * n..(i + 1) * n].copy_from_slice(&sub_sb);
        }

        self.prev_wo = wo_b;
        self.prev_voiced = fields.voiced3;
        self.prev_lsps = lsps3;
        self.prev_e = e_b;

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_speech_frame(f0: f32, t0: usize) -> [i16; SAMPLES_PER_FRAME] {
        std::array::from_fn(|i| {
            let t = (t0 + i) as f32 / codec2_3200::SAMPLE_RATE as f32;
            let v = 8000.0 * (std::f32::consts::TAU * f0 * t).sin()
                + 3000.0 * (std::f32::consts::TAU * 2.0 * f0 * t).sin();
            v as i16
        })
    }

    /// Full round trip through this crate's own encoder and decoder --
    /// same basic sanity bar `codec2_3200`'s own equivalent test uses:
    /// finite, reasonably-scaled, non-degenerate audio, no panics.
    #[test]
    fn encode_decode_round_trip_produces_finite_reasonably_scaled_audio() {
        let mut encoder = Encoder::new();
        let mut decoder = Decoder::new();
        let mut max_abs = 0i32;
        let mut sumsq = 0.0f64;
        let mut n_samples = 0u64;

        for frame_idx in 0..40 {
            let f0 = 120.0 + 40.0 * (frame_idx as f32 * 0.3).sin();
            let speech = synthetic_speech_frame(f0, frame_idx * SAMPLES_PER_FRAME);
            let bits = encoder.encode(&speech);
            let out = decoder.decode(&bits);
            for &s in &out {
                max_abs = max_abs.max(s.abs() as i32);
                sumsq += (s as f64) * (s as f64);
                n_samples += 1;
            }
        }

        let rms = (sumsq / n_samples as f64).sqrt();
        assert!(rms > 50.0, "decoded audio looks like silence, RMS={rms}");
        assert!(rms < 20000.0, "decoded audio implausibly loud, RMS={rms}");
        assert!(max_abs > 0, "decoded audio is all zero");
    }

    /// Same round-trip sanity bar, fully fixed-point path.
    #[test]
    fn fixed_encode_decode_round_trip_produces_finite_reasonably_scaled_audio() {
        let mut encoder = EncoderFixed::new();
        let mut decoder = DecoderFixed::new();
        let mut max_abs = 0i32;
        let mut sumsq = 0.0f64;
        let mut n_samples = 0u64;

        for frame_idx in 0..40 {
            let f0 = 120.0 + 40.0 * (frame_idx as f32 * 0.3).sin();
            let speech = synthetic_speech_frame(f0, frame_idx * SAMPLES_PER_FRAME);
            let bits = encoder.encode(&speech);
            let out = decoder.decode(&bits);
            for &s in &out {
                max_abs = max_abs.max(s.abs() as i32);
                sumsq += (s as f64) * (s as f64);
                n_samples += 1;
            }
        }

        let rms = (sumsq / n_samples as f64).sqrt();
        assert!(rms > 50.0, "decoded audio looks like silence, RMS={rms}");
        assert!(rms < 20000.0, "decoded audio implausibly loud, RMS={rms}");
        assert!(max_abs > 0, "decoded audio is all zero");
    }

    /// Real cross-implementation check for `DecoderFixed`, same two
    /// real captured bitstream/PCM fixtures the float `Decoder`'s own
    /// equivalent tests use.
    #[test]
    fn decoder_fixed_matches_the_real_reference_decoder_on_a_mostly_voiced_synthetic_signal_bitstream(
    ) {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_1600/synthetic_voiced_c_encoded_bits.bin"
        );
        let pcm_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_1600/synthetic_voiced_c_decoded_pcm.bin"
        );
        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let pcm_data = std::fs::read(pcm_path).unwrap_or_else(|e| panic!("{pcm_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(n_frames > 150, "expected the real captured fixture corpus, got {n_frames} frames");

        let mut decoder = DecoderFixed::new();
        let mut fixed_pcm: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] = bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME]
                .try_into()
                .unwrap();
            fixed_pcm.extend_from_slice(&decoder.decode(&frame));
        }
        let ref_pcm: Vec<i16> = pcm_data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        let n = fixed_pcm.len();
        let mean_a: f64 = fixed_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = ref_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = fixed_pcm[i] as f64 - mean_a;
            let db = ref_pcm[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        let rms_a = (var_a / n as f64 + mean_a * mean_a).sqrt();
        let rms_b = (var_b / n as f64 + mean_b * mean_b).sqrt();
        println!("codec2_1600 DecoderFixed vs reference (mostly-voiced fixture): correlation={corr}, rms_fixed={rms_a}, rms_reference={rms_b}");
        assert!(
            (0.95..1.05).contains(&(rms_a / rms_b)),
            "DecoderFixed's own RMS diverged from the reference's, ratio={}",
            rms_a / rms_b
        );
        assert!(corr > 0.99, "DecoderFixed diverged from the real reference decoder: correlation={corr} (expected > 0.99 on mostly-voiced content)");
    }

    /// The real comparison the parallel fixed-point encoder exists for:
    /// does `EncoderFixed`'s own bitstream, decoded, sound like
    /// `Encoder`'s (the float reference) does, on identical input --
    /// same methodology `codec2_3200::encoder_fixed`'s own equivalent
    /// test uses (real, measured correlation between the two decoded
    /// waveforms, not bit-exactness -- encoders have real design
    /// freedom in how they arrive at a quantizer index).
    #[test]
    fn encoder_fixed_produces_highly_correlated_audio_with_the_float_reference_encoder() {
        let mut fixed_encoder = EncoderFixed::new();
        let mut float_encoder = Encoder::new();
        let mut fixed_decoder = DecoderFixed::new();
        let mut float_decoder = Decoder::new();

        let mut fixed_pcm: Vec<i16> = Vec::new();
        let mut float_pcm: Vec<i16> = Vec::new();

        for frame_idx in 0..40 {
            let f0 = 120.0 + 40.0 * (frame_idx as f32 * 0.3).sin();
            let speech = synthetic_speech_frame(f0, frame_idx * SAMPLES_PER_FRAME);
            let fixed_bits = fixed_encoder.encode(&speech);
            let float_bits = float_encoder.encode(&speech);
            fixed_pcm.extend_from_slice(&fixed_decoder.decode(&fixed_bits));
            float_pcm.extend_from_slice(&float_decoder.decode(&float_bits));
        }

        let n = fixed_pcm.len();
        let mean_a: f64 = fixed_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = float_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = fixed_pcm[i] as f64 - mean_a;
            let db = float_pcm[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        println!("codec2_1600 EncoderFixed vs Encoder decoded-audio correlation: {corr}");
        assert!(
            corr > 0.99,
            "EncoderFixed's own bitstream diverged from Encoder's on identical input: correlation={corr} (expected > 0.99)"
        );
    }

    /// Decodes a real captured bitstream fixture with this crate's own
    /// `Decoder` and compares against the real reference decoder's own
    /// captured PCM output for the same bitstream: Pearson correlation
    /// (shape agreement) and RMS ratio (scale agreement -- correlation
    /// alone is scale-invariant and would miss a gain bug, same
    /// reasoning `codec2_3200::DecoderFixed`'s own equivalent test
    /// documents).
    fn decode_and_compare_to_reference(bits_path: &str, pcm_path: &str) -> (f64, f64) {
        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let pcm_data = std::fs::read(pcm_path).unwrap_or_else(|e| panic!("{pcm_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(
            n_frames > 150,
            "expected the real captured fixture corpus, got {n_frames} frames"
        );
        assert_eq!(
            pcm_data.len(),
            n_frames * SAMPLES_PER_FRAME * 2,
            "bitstream/PCM fixture frame counts don't line up"
        );

        let mut decoder = Decoder::new();
        let mut rust_pcm: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] = bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME]
                .try_into()
                .unwrap();
            rust_pcm.extend_from_slice(&decoder.decode(&frame));
        }
        let ref_pcm: Vec<i16> = pcm_data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        let n = rust_pcm.len();
        let mean_a: f64 = rust_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = ref_pcm.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = rust_pcm[i] as f64 - mean_a;
            let db = ref_pcm[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        let rms_a = (var_a / n as f64 + mean_a * mean_a).sqrt();
        let rms_b = (var_b / n as f64 + mean_b * mean_b).sqrt();
        (corr, rms_a / rms_b)
    }

    /// Real cross-implementation decoder check on a mixed
    /// voiced/noise-dominated synthetic signal (see this module's own
    /// doc comment on `analyse_lsps_and_energy` and
    /// `tests/fixtures/codec2_1600/README.md` for how it was captured):
    /// a real captured bitstream from plain upstream Codec2's own
    /// unmodified `c2enc 1600`, decoded both by this crate's `Decoder`
    /// and (already, at fixture-capture time) by upstream's own `c2dec
    /// 1600`.
    ///
    /// Correlation lands at 0.9492 here, well below 3200bps's own
    /// 0.99-plus bar on its own (mostly-voiced) fixture -- investigated,
    /// not just accepted: RMS ratio is 0.9987 (essentially exact scale
    /// match, ruling out a gain bug), and re-running this exact
    /// comparison on a mostly-voiced fixture with the same signal
    /// character as `codec2_3200`'s own (see
    /// `decoder_matches_the_real_reference_decoder_on_a_mostly_voiced_synthetic_signal_bitstream`
    /// below) recovers 0.99+ correlation. That isolates the gap to
    /// this fixture's own noise-dominated blocks: unvoiced harmonics
    /// draw phase from this crate's own independent PRNG (`next_rand`'s
    /// own doc comment already documents this as by-design, not
    /// transmitted), so divergence on unvoiced content is real and
    /// expected, not a defect -- confirmed by measurement, not assumed.
    #[test]
    fn decoder_matches_the_real_reference_decoder_on_a_real_captured_synthetic_signal_bitstream() {
        let (corr, rms_ratio) = decode_and_compare_to_reference(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codec2_1600/synthetic_c_encoded_bits.bin"
            ),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codec2_1600/synthetic_c_decoded_pcm.bin"
            ),
        );
        println!("codec2_1600 Decoder vs reference (mixed fixture): correlation={corr}, rms_ratio={rms_ratio}");
        assert!(
            (0.95..1.05).contains(&rms_ratio),
            "Decoder's own RMS diverged from the reference's, ratio={rms_ratio} -- looks like a gain/scale bug"
        );
        assert!(corr > 0.9, "decoder output diverged from the real reference decoder on the same real captured bitstream: correlation={corr} (expected > 0.9, measured 0.9492 when this test was written)");
    }

    /// Same real cross-implementation check, this time on a
    /// mostly-voiced synthetic signal (swept sinusoid plus a little
    /// deterministic pseudo-noise) -- the same signal character
    /// `codec2_3200`'s own fixture uses, and for the same reason (its
    /// own module doc comment: mostly-voiced content keeps PRNG-driven
    /// divergence small enough that a tight threshold is actually
    /// discriminating). Exists specifically to isolate whether this
    /// module's lower correlation on the mixed fixture above is real
    /// PRNG divergence on noisy content or a genuine bug -- it's the
    /// former: correlation here lands at 0.99+, matching 3200bps's own
    /// bar.
    #[test]
    fn decoder_matches_the_real_reference_decoder_on_a_mostly_voiced_synthetic_signal_bitstream() {
        let (corr, rms_ratio) = decode_and_compare_to_reference(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codec2_1600/synthetic_voiced_c_encoded_bits.bin"
            ),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/codec2_1600/synthetic_voiced_c_decoded_pcm.bin"
            ),
        );
        println!("codec2_1600 Decoder vs reference (mostly-voiced fixture): correlation={corr}, rms_ratio={rms_ratio}");
        assert!(
            (0.95..1.05).contains(&rms_ratio),
            "Decoder's own RMS diverged from the reference's, ratio={rms_ratio} -- looks like a gain/scale bug"
        );
        assert!(corr > 0.99, "decoder output diverged from the real reference decoder on the same real captured bitstream: correlation={corr} (expected > 0.99 on mostly-voiced content)");
    }

    /// Same real, end-to-end design invariant `codec2_3200::mod::tests`'s
    /// own `decode_16k_with_spectral_bridge_disabled_matches_the_base_8khz_decoder_when_decimated`
    /// checks there: with Spectral Bridge disabled, `decode_16k`'s
    /// harmonics `1..=l` are reused unchanged from the base 8kHz decode,
    /// so decimating its 16kHz output by 2 should closely match
    /// `decode()`'s own output for the same bitstream. Samples the *odd*
    /// indices (`skip(1).step_by(2)`), matching the same one-sample
    /// overlap-add framing offset already measured and documented on
    /// `codec2_3200`'s own version of this test -- this is the same
    /// `SpectralBridgeState` synthesis, so the same offset applies here.
    #[test]
    fn decode_16k_with_spectral_bridge_disabled_matches_the_base_8khz_decoder_when_decimated() {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_1600/synthetic_voiced_c_encoded_bits.bin"
        );
        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(n_frames > 50, "expected the real captured fixture corpus, got {n_frames} frames");

        let mut decoder_8k = Decoder::new();
        let mut decoder_16k = Decoder::new();
        decoder_16k.spectral_bridge.enabled = false;

        let mut pcm_8k: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        let mut pcm_16k_decimated: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] =
                bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME].try_into().unwrap();
            pcm_8k.extend_from_slice(&decoder_8k.decode(&frame));
            let out_16k = decoder_16k.decode_16k(&frame);
            pcm_16k_decimated.extend(out_16k.iter().skip(1).step_by(2).copied());
        }

        assert_eq!(pcm_8k.len(), pcm_16k_decimated.len());
        let n = pcm_8k.len();
        let mean_a: f64 = pcm_8k.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = pcm_16k_decimated.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = pcm_8k[i] as f64 - mean_a;
            let db = pcm_16k_decimated[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        println!("codec2_1600 decode_16k (disabled, decimated) vs decode(): correlation={corr}");
        assert!(
            corr > 0.99,
            "decode_16k's own reused-harmonics content diverged from the base decoder: correlation={corr} (expected > 0.99)"
        );
    }

    /// Same measurement `codec2_3200::mod::tests`'s own
    /// `decode_16k_with_spectral_bridge_enabled_places_bounded_energy_in_the_new_4_to_8khz_band`
    /// makes there: with Spectral Bridge enabled (the real default),
    /// does the newly-available 4-8kHz band actually carry bounded,
    /// non-trivial extrapolated energy? Uses the mostly-voiced fixture
    /// specifically, since extrapolation only ever runs on voiced
    /// sub-frames.
    #[test]
    fn decode_16k_with_spectral_bridge_enabled_places_bounded_energy_in_the_new_4_to_8khz_band() {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_1600/synthetic_voiced_c_encoded_bits.bin"
        );
        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(n_frames > 50, "expected the real captured fixture corpus, got {n_frames} frames");

        let mut decoder_16k = Decoder::new();
        assert!(decoder_16k.spectral_bridge.enabled, "Spectral Bridge should be on by default");

        let mut pcm_16k: Vec<f32> =
            Vec::with_capacity(n_frames * 4 * codec2_3200::spectral_bridge::N_SAMP_SB);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] =
                bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME].try_into().unwrap();
            let out_16k = decoder_16k.decode_16k(&frame);
            pcm_16k.extend(out_16k.iter().map(|&s| s as f32));
        }

        const WIN: usize = 1024;
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(WIN);
        let mut low_energy = 0.0f64;
        let mut high_energy = 0.0f64;
        let mut windows = 0usize;
        for chunk in pcm_16k.chunks_exact(WIN) {
            let mut buf: Vec<rustfft::num_complex::Complex32> =
                chunk.iter().map(|&s| rustfft::num_complex::Complex32::new(s, 0.0)).collect();
            fft.process(&mut buf);
            for (k, c) in buf.iter().enumerate().take(WIN / 2) {
                let e = (c.norm() as f64).powi(2);
                if k < WIN / 4 {
                    low_energy += e;
                } else {
                    high_energy += e;
                }
            }
            windows += 1;
        }
        assert!(windows > 10, "expected enough 1024-sample windows to be meaningful, got {windows}");

        println!("codec2_1600 decode_16k (enabled): low(0-4kHz) energy={low_energy:e}, high(4-8kHz) energy={high_energy:e}, ratio={:e}", high_energy / low_energy);
        assert!(
            high_energy > low_energy * 1e-4,
            "extrapolated 4-8kHz band carries ~no energy (high={high_energy:e}, low={low_energy:e}) -- Spectral Bridge looks like a no-op on this fixture"
        );
        assert!(
            high_energy < low_energy,
            "extrapolated 4-8kHz band ({high_energy:e}) exceeds the real 0-4kHz band ({low_energy:e}) -- the amplitude fit may be running away despite its beta.min(0.0) clamp"
        );
    }

    /// Fixed-point sibling of
    /// `decode_16k_with_spectral_bridge_disabled_matches_the_base_8khz_decoder_when_decimated`.
    #[test]
    fn decode_16k_fixed_with_spectral_bridge_disabled_matches_the_base_8khz_decoder_when_decimated()
    {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_1600/synthetic_voiced_c_encoded_bits.bin"
        );
        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(n_frames > 50, "expected the real captured fixture corpus, got {n_frames} frames");

        let mut decoder_8k = DecoderFixed::new();
        let mut decoder_16k = DecoderFixed::new();
        decoder_16k.spectral_bridge.enabled = false;

        let mut pcm_8k: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        let mut pcm_16k_decimated: Vec<i16> = Vec::with_capacity(n_frames * SAMPLES_PER_FRAME);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] =
                bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME].try_into().unwrap();
            pcm_8k.extend_from_slice(&decoder_8k.decode(&frame));
            let out_16k = decoder_16k.decode_16k_fixed(&frame);
            pcm_16k_decimated.extend(out_16k.iter().skip(1).step_by(2).copied());
        }

        assert_eq!(pcm_8k.len(), pcm_16k_decimated.len());
        let n = pcm_8k.len();
        let mean_a: f64 = pcm_8k.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mean_b: f64 = pcm_16k_decimated.iter().map(|&s| s as f64).sum::<f64>() / n as f64;
        let mut cov = 0.0f64;
        let mut var_a = 0.0f64;
        let mut var_b = 0.0f64;
        for i in 0..n {
            let da = pcm_8k[i] as f64 - mean_a;
            let db = pcm_16k_decimated[i] as f64 - mean_b;
            cov += da * db;
            var_a += da * da;
            var_b += db * db;
        }
        let corr = cov / (var_a * var_b).sqrt();
        println!("codec2_1600 decode_16k_fixed (disabled, decimated) vs decode(): correlation={corr}");
        assert!(
            corr > 0.99,
            "decode_16k_fixed's own reused-harmonics content diverged from the base fixed decoder: correlation={corr} (expected > 0.99)"
        );
    }

    /// Fixed-point sibling of
    /// `decode_16k_with_spectral_bridge_enabled_places_bounded_energy_in_the_new_4_to_8khz_band`.
    #[test]
    fn decode_16k_fixed_with_spectral_bridge_enabled_places_bounded_energy_in_the_new_4_to_8khz_band()
    {
        let bits_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/codec2_1600/synthetic_voiced_c_encoded_bits.bin"
        );
        let bits_data = std::fs::read(bits_path).unwrap_or_else(|e| panic!("{bits_path}: {e}"));
        let n_frames = bits_data.len() / BYTES_PER_FRAME;
        assert!(n_frames > 50, "expected the real captured fixture corpus, got {n_frames} frames");

        let mut decoder_16k = DecoderFixed::new();
        assert!(decoder_16k.spectral_bridge.enabled, "Spectral Bridge should be on by default");

        let mut pcm_16k: Vec<f32> =
            Vec::with_capacity(n_frames * 4 * codec2_3200::spectral_bridge::N_SAMP_SB);
        for f in 0..n_frames {
            let frame: [u8; BYTES_PER_FRAME] =
                bits_data[f * BYTES_PER_FRAME..(f + 1) * BYTES_PER_FRAME].try_into().unwrap();
            let out_16k = decoder_16k.decode_16k_fixed(&frame);
            pcm_16k.extend(out_16k.iter().map(|&s| s as f32));
        }

        const WIN: usize = 1024;
        let mut planner = rustfft::FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(WIN);
        let mut low_energy = 0.0f64;
        let mut high_energy = 0.0f64;
        let mut windows = 0usize;
        for chunk in pcm_16k.chunks_exact(WIN) {
            let mut buf: Vec<rustfft::num_complex::Complex32> =
                chunk.iter().map(|&s| rustfft::num_complex::Complex32::new(s, 0.0)).collect();
            fft.process(&mut buf);
            for (k, c) in buf.iter().enumerate().take(WIN / 2) {
                let e = (c.norm() as f64).powi(2);
                if k < WIN / 4 {
                    low_energy += e;
                } else {
                    high_energy += e;
                }
            }
            windows += 1;
        }
        assert!(windows > 10, "expected enough 1024-sample windows to be meaningful, got {windows}");

        println!("codec2_1600 decode_16k_fixed (enabled): low(0-4kHz) energy={low_energy:e}, high(4-8kHz) energy={high_energy:e}, ratio={:e}", high_energy / low_energy);
        assert!(
            high_energy > low_energy * 1e-4,
            "extrapolated 4-8kHz band carries ~no energy (high={high_energy:e}, low={low_energy:e}) -- Spectral Bridge looks like a no-op on this fixture"
        );
        assert!(
            high_energy < low_energy,
            "extrapolated 4-8kHz band ({high_energy:e}) exceeds the real 0-4kHz band ({low_energy:e}) -- the amplitude fit may be running away despite its beta.min(0.0) clamp"
        );
    }
}
