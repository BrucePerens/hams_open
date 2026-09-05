// SPDX-License-Identifier: LGPL-3.0-or-later
//! **Not production code.** `Encoder` here is the original, fully-`f32`
//! encode pipeline -- moved into this dedicated subdirectory so its status
//! is structural, not just a doc comment: it exists as the validated
//! reference implementation every fixed-point stage in
//! `super::EncoderFixed` (`encoder_fixed.rs`), its parallel replacement,
//! is checked against, per
//! `docs/references/FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md`'s
//! own recorded product decision (Bruce's own call, 2026-09-04: build
//! `EncoderFixed` in parallel, keep this one live as a per-frame diff
//! reference until every stage in the punch list has its own passing
//! cross-check).
//!
//! **Every real float implementation `Encoder` calls now lives in this
//! same subdirectory**, one submodule per parent module it moved out of
//! (`lpc`, `nlp`, `quantise`, `voicing`), mirroring `codec2_3200`'s own
//! top-level layout -- completed 2026-09-04, per Bruce's own explicit
//! follow-up request ("move all of the floating encoder code to
//! floating_reference"), closing the "Real follow-on" the punch list had
//! left open. Each parent module keeps only what its own fixed-point
//! production code (or the one shared `Decoder`) still needs directly --
//! see each submodule's own doc comment for the specific pieces it
//! borrows back via `pub(crate)` (e.g. `nlp::lowpass_coeffs`, `lpc::
//! find_next_root_from_q23`, `quantise::lsp_dim_value_hz`) and why they
//! can't move. Verified with a hard invariant throughout the move: the
//! crate's own lib test count (147) never changed -- every test either
//! moved with the float function it validates, stayed behind as a
//! cross-validation test importing the moved function back, or was
//! genuinely unaffected.
//!
//! `Decoder` did **not** move -- it stays in `super` (`codec2_3200::mod`).
//! Historically this was because a decoder has no design freedom (it only
//! ever sees the transmitted bitstream, never how the encoder arrived at
//! it) and so needed no fixed-point *port* at all, just the one real
//! `Decoder` both encoders' bitstreams already go through -- but Bruce has
//! since directed a genuine fixed-point decoder be built (`DecoderFixed`,
//! parallel to `Decoder`, mirroring the encoder's own precedent), so this
//! module's own float pieces (`lsp_to_lpc`, `envelope.rs`, `synthesis.rs`)
//! remain shared/unmoved for a different, real reason now: `Decoder`
//! itself hasn't moved yet, not because it never will.

pub mod lpc;
pub mod nlp;
pub mod quantise;
pub mod voicing;

use super::bits;
use super::{bw_gamma, fallback_lsp, window};
use super::{BYTES_PER_FRAME, E_BITS, M_PITCH, N_SAMP, SAMPLES_PER_FRAME, WO_BITS};

/// Persistent per-call encoder state: the `M_PITCH`-sample speech
/// history window shared by pitch estimation and LPC analysis, plus
/// `nlp`/`voicing`'s own state.
pub struct Encoder {
    sn: [f32; M_PITCH],
    window: [f32; M_PITCH],
    nlp_state: nlp::NlpState,
    voicing_state: voicing::VoicingState,
}

impl Default for Encoder {
    fn default() -> Self {
        Encoder {
            sn: [0.0; M_PITCH],
            window: window::make_analysis_window(),
            nlp_state: nlp::NlpState::new(),
            voicing_state: voicing::VoicingState::new(),
        }
    }
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

    /// Encodes one 20ms (`SAMPLES_PER_FRAME`-sample) frame into
    /// `BYTES_PER_FRAME` real-format bytes. Matches the reference's own
    /// real `codec2_encode` structure: two 10ms analysis sub-steps (each
    /// advancing pitch estimation and contributing one `voiced` bit, the
    /// second also setting the transmitted `Wo`), then one LSP/energy
    /// analysis pass over the full `M_PITCH`-sample history window.
    pub fn encode(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
        self.shift_in(&speech[..N_SAMP]);
        nlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced0 = voicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        self.shift_in(&speech[N_SAMP..]);
        let f0 = nlp::nlp(&mut self.nlp_state, &self.sn);
        let voiced1 = voicing::is_voiced(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);

        // `f0_to_wo`/`encode_wo` stay in the parent (`codec2_3200::nlp`/
        // `codec2_3200::quantise`), shared unchanged by both encoders --
        // not part of this module's own local `nlp`/`quantise`
        // submodules.
        let wo_index = super::quantise::encode_wo(super::nlp::f0_to_wo(f0));

        let mut windowed = [0.0f32; M_PITCH];
        for ((w, &s), &win) in windowed
            .iter_mut()
            .zip(self.sn.iter())
            .zip(self.window.iter())
        {
            *w = s * win;
        }
        let r = lpc::autocorrelate(&windowed);
        // White noise correction (see lpc::apply_white_noise_correction's
        // own doc comment) applies only to Levinson-Durbin's own input --
        // a separate, explicitly-named copy, so lpc_energy below still
        // reports the real, uncorrected signal energy rather than the
        // artificially-inflated-by-alpha value.
        let mut r_for_levinson = r;
        lpc::apply_white_noise_correction(&mut r_for_levinson);
        let mut ak = lpc::levinson_durbin(&r_for_levinson);
        let e = lpc::lpc_energy(&ak, &r);
        for (i, a) in ak.iter_mut().enumerate() {
            *a *= bw_gamma(i);
        }
        let lsp = lpc::lpc_to_lsp(&ak).unwrap_or_else(fallback_lsp);

        let e_index = super::quantise::encode_energy(e);
        let lsp_indexes = quantise::encode_lsps_delta_scalar(&lsp);

        let fields = bits::FrameFields {
            voiced0,
            voiced1,
            wo_index,
            e_index,
            lsp_indexes,
        };
        bits::pack_frame(&fields, WO_BITS, E_BITS)
    }
}
