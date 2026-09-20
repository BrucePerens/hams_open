
impl DecoderFixed {
    #[inline(never)]
    pub fn decode_profiled(&mut self, bytes: &[u8; BYTES_PER_FRAME]) -> [i16; SAMPLES_PER_FRAME] {
        use crate::prof::mark;
        mark(15);
        let fields = bits::unpack_frame(bytes, WO_BITS, E_BITS);
        let wo1 = quantise::decode_wo_fixed(fields.wo_index);
        let e1 = quantise::decode_energy_fixed(fields.e_index);
        let lsps1 = quantise::decode_lsps_delta_scalar_fixed(&fields.lsp_indexes);
        let voiced0 = interp::interp_voiced(fields.voiced0, self.prev_voiced, fields.voiced1);
        let wo0 = interp::interp_wo_fixed(fields.voiced0, self.prev_wo, self.prev_voiced, wo1, fields.voiced1, w0_min_q23());
        let e0 = interp::interp_energy_fixed(self.prev_e, e1);
        let lsps0 = interp::interpolate_lsp_fixed(&self.prev_lsps, &lsps1);
        mark(0);
        let mut out = [0i16; SAMPLES_PER_FRAME];
        let subframes = [(wo0, voiced0, lsps0, e0), (wo1, fields.voiced1, lsps1, e1)];
        for (i, (wo, voiced, lsps, e)) in subframes.into_iter().enumerate() {
            let ak = lpc::lsp_to_lpc_fixed(&lsps);
            mark(1);
            let mut model = envelope::ModelFixed::new(wo, voiced);
            let aw = envelope::compute_harmonic_amplitudes_fixed(&ak, e, &mut model);
            mark(2);
            envelope::apply_first_harmonic_correction_fixed(&mut model);
            mark(3);
            let sub = self.synth.synthesize_subframe_fixed(&mut model, &aw);
            mark(4);
            out[i * N_SAMP..(i + 1) * N_SAMP].copy_from_slice(&sub);
        }
        self.prev_wo = wo1;
        self.prev_voiced = fields.voiced1;
        self.prev_lsps = lsps1;
        self.prev_e = e1;
        out
    }
}
