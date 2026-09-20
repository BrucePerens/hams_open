
impl EncoderFixed {
    #[inline(never)]
    pub fn encode_profiled(&mut self, speech: &[i16; SAMPLES_PER_FRAME]) -> [u8; BYTES_PER_FRAME] {
        use crate::prof::mark;
        mark(15);
        self.shift_in(&speech[..N_SAMP]);
        nlp::nlp_fixed(&mut self.nlp_state, &self.sn);
        mark(0);
        let voiced0 = voicing::is_voiced_fixed(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);
        mark(1);
        self.shift_in(&speech[N_SAMP..]);
        let f0_bin = nlp::nlp_fixed_bin(&mut self.nlp_state, &self.sn);
        mark(0);
        let voiced1 = voicing::is_voiced_fixed(&mut self.voicing_state, &self.sn[M_PITCH - N_SAMP..]);
        mark(1);
        let wo_index = tables::NLP_BIN_WO_INDEX[f0_bin] as u32;
        mark(2);
        let mut wn_q = [0i32; M_PITCH];
        for ((w, &s), &win) in wn_q.iter_mut().zip(self.sn.iter()).zip(tables::WINDOW_ANALYSIS_Q30.iter()) {
            *w = ((s as i64 * win as i64) >> 7) as i32;
        }
        let r_q = lpc::autocorrelate_fixed(&wn_q);
        mark(3);
        let mut r_q_for_levinson = r_q;
        lpc::apply_white_noise_correction_fixed(&mut r_q_for_levinson);
        let (_ak, mut a_q23) = lpc::levinson_durbin_fixed_from_integer_r(&r_q_for_levinson);
        mark(4);
        let e_q23 = lpc::lpc_energy_q23(&a_q23, &r_q);
        mark(5);
        lpc::apply_bw_gamma_fixed(&mut a_q23);
        let lsp_q23 = lpc::lpc_to_lsp_q23_from_integer_ak(&a_q23).unwrap_or(tables::MOD_FALLBACK_LSP_Q23);
        mark(6);
        let e_index = quantise::encode_energy_q23(e_q23);
        let lsp_indexes = quantise::encode_lsps_delta_scalar_q23(&lsp_q23);
        mark(7);
        let fields = bits::FrameFields { voiced0, voiced1, wo_index, e_index, lsp_indexes };
        let r = bits::pack_frame(&fields, WO_BITS, E_BITS);
        mark(8);
        r
    }
}
