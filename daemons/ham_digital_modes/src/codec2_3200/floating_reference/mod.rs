// SPDX-License-Identifier: LGPL-3.0-or-later
//! **Not production code.** `Encoder` here is the original, fully-`f32`
//! encode pipeline -- moved into this dedicated subdirectory so its status
//! is structural, not just a doc comment: it exists as the validated
//! reference implementation every fixed-point stage in
//! `super::floating_reference::Encoder`'s parallel replacement,
//! `super::EncoderFixed` (`encoder_fixed.rs`), is checked against, per
//! `docs/references/FIXED_POINT_ENCODER_IMPLEMENTATION_PUNCH_LIST.md`'s
//! own recorded product decision (Bruce's own call, 2026-09-04: build
//! `EncoderFixed` in parallel, keep this one live as a per-frame diff
//! reference until every stage in the punch list has its own passing
//! cross-check).
//!
//! `Decoder` did **not** move -- it stays in `super` (`codec2_3200::mod`)
//! since this move is scoped to the encoder half only; see that module's
//! own doc comment for why encode/decode aren't symmetric here (a decoder
//! only ever sees the transmitted bitstream, never how the encoder
//! arrived at it, so there's no equivalent "fixed-point decoder" question
//! forced by interoperability the way there is for `q_mul`/`div_round_
//! i128`-style internal arithmetic -- `Decoder` itself is already the one
//! and only real decoder this crate needs).

use super::{bw_gamma, fallback_lsp, nlp, quantise, voicing, window};
use super::{bits, lpc};
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

        let wo_index = quantise::encode_wo(nlp::f0_to_wo(f0));

        let mut windowed = [0.0f32; M_PITCH];
        for ((w, &s), &win) in windowed
            .iter_mut()
            .zip(self.sn.iter())
            .zip(self.window.iter())
        {
            *w = s * win;
        }
        let r = lpc::autocorrelate(&windowed);
        let mut ak = lpc::levinson_durbin(&r);
        let e = lpc::lpc_energy(&ak, &r);
        for (i, a) in ak.iter_mut().enumerate() {
            *a *= bw_gamma(i);
        }
        let lsp = lpc::lpc_to_lsp(&ak).unwrap_or_else(fallback_lsp);

        let e_index = quantise::encode_energy(e);
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
