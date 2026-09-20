// SPDX-License-Identifier: LGPL-3.0-or-later
//! Strong check of the shared float quantizer (`mbe_encode::quantize_speech`): decode random indices with the real
//! decoder, quantize the decoded parameters against the same previous-frame state, decode again, and require every
//! harmonic amplitude to match. A scale error anywhere in the amplitude, gain or predictor recursion (a wrong predictor
//! weight, gain scale, or unvoiced scaling in either the decoder or the quantizer) breaks this immediately, which the
//! encoders' own loose round-trip tests would not notice.

use ham_digital_modes::ambe::float::mbe_encode::{quantize_speech, ModeTables, PrevState, SpeechTarget};

struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: u32) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as u32
    }
}

const FRAMES: usize = 4000;

#[test]
fn dstar_quantizer_inverts_the_decoder() {
    use ham_digital_modes::ambe::float::dstar::decode::{
        dequantize, extract_raw_parameters, f0_from_b0, DStarDecoderState, DequantizedFrame, RawParameters, GAMMA_MEMORY,
        GAMMA_SCALE, PREDICTOR_RHO,
    };
    use ham_digital_modes::ambe::float::dstar::encode::pack_raw_parameters;
    use ham_digital_modes::ambe::float::dstar::tables::{self, DG, HOC_B5, HOC_B6, HOC_B7, HOC_B8, L_TABLE, PRBA24, PRBA58};

    let mut rng = Lcg(7);
    let mut state = DStarDecoderState::initial();
    let mut checked = 0;
    for _ in 0..FRAMES {
        let raw = RawParameters {
            b0: rng.below(120),
            b1: rng.below(16),
            b2: rng.below(64),
            b3: rng.below(512),
            b4: rng.below(128),
            b5: rng.below(16),
            b6: rng.below(16),
            b7: rng.below(16),
            b8: 2 * rng.below(8),
        };
        let d = pack_raw_parameters(&raw);
        assert_eq!(extract_raw_parameters(d).b0, raw.b0);
        let prev = DStarDecoderState { l: state.l, log2_ml: state.log2_ml.clone(), gamma: state.gamma };
        let DequantizedFrame::Speech(target) = dequantize(d, &mut state) else { continue };
        let f0 = f0_from_b0(raw.b0);
        let l = L_TABLE[raw.b0 as usize];
        let tables = ModeTables {
            vuv: &tables::VUV,
            dg: &DG,
            prba24: &PRBA24,
            prba58: &PRBA58,
            lmprbl: &tables::LMPRBL,
            hoc: [&HOC_B5, &HOC_B6, &HOC_B7, &HOC_B8],
            hoc_b8_even_only: true,
            rho: PREDICTOR_RHO,
            gamma_scale: GAMMA_SCALE,
            gamma_memory: GAMMA_MEMORY,
        };
        let q = quantize_speech(
            &SpeechTarget { l, w0: target.w0, vuv_f0: f0, voiced: &target.voiced, ml: &target.ml },
            &PrevState { l: prev.l, log2_ml: &prev.log2_ml, gamma: prev.gamma },
            &tables,
        );
        let again = RawParameters { b0: raw.b0, b1: q.b1, b2: q.b2, b3: q.b3, b4: q.b4, b5: q.b5, b6: q.b6, b7: q.b7, b8: q.b8 };
        let mut replay = DStarDecoderState { l: prev.l, log2_ml: prev.log2_ml.clone(), gamma: prev.gamma };
        let DequantizedFrame::Speech(back) = dequantize(pack_raw_parameters(&again), &mut replay) else { panic!("re-decode is not speech") };
        for h in 1..=l as usize {
            let (a, b) = (target.ml[h], back.ml[h]);
            assert!(
                (a - b).abs() <= 1e-6 * a.abs().max(1.0),
                "b0={} harmonic {h}: decoded {a}, re-decoded {b}; indices b1..b8 {:?} -> {:?}; previous L {} gamma {}",
                raw.b0,
                [raw.b1, raw.b2, raw.b3, raw.b4, raw.b5, raw.b6, raw.b7, raw.b8],
                [again.b1, again.b2, again.b3, again.b4, again.b5, again.b6, again.b7, again.b8],
                prev.l,
                prev.gamma
            );
        }
        checked += 1;
    }
    assert!(checked > FRAMES * 9 / 10, "too few speech frames checked: {checked}");
}

#[cfg(feature = "ambe_plus_2")]
#[test]
fn ambe_plus_2_quantizer_inverts_the_decoder() {
    use ham_digital_modes::ambe::float::ambe_plus_2::decode::{dequantize, DecoderState, DequantizedFrame, RawParameters};
    use ham_digital_modes::ambe::float::ambe_plus_2::encode::pack_raw_parameters;
    use ham_digital_modes::ambe::float::ambe_plus_2::tables::{self, DG, HOC_B5, HOC_B6, HOC_B7, HOC_B8, L_TABLE, PRBA24, PRBA58, W0_TABLE};

    let mut rng = Lcg(11);
    let mut state = DecoderState::initial();
    let mut checked = 0;
    for _ in 0..FRAMES {
        let raw = RawParameters {
            b0: rng.below(120),
            b1: rng.below(32),
            b2: rng.below(32),
            b3: rng.below(512),
            b4: rng.below(128),
            b5: rng.below(32),
            b6: rng.below(16),
            b7: rng.below(16),
            b8: rng.below(8),
        };
        let d = pack_raw_parameters(&raw);
        let prev = DecoderState { l: state.l, log2_ml: state.log2_ml.clone(), gamma: state.gamma };
        let DequantizedFrame::Speech(target) = dequantize(&raw, &mut state) else { continue };
        let l = L_TABLE[raw.b0 as usize];
        let _ = d;
        let tables = ModeTables {
            vuv: &tables::VUV,
            dg: &DG,
            prba24: &PRBA24,
            prba58: &PRBA58,
            lmprbl: &tables::LMPRBL,
            hoc: [&HOC_B5, &HOC_B6, &HOC_B7, &HOC_B8],
            hoc_b8_even_only: false,
            rho: 0.65,
            gamma_scale: 1.0,
            gamma_memory: 0.5,
        };
        let q = quantize_speech(
            &SpeechTarget { l, w0: target.w0, vuv_f0: W0_TABLE[raw.b0 as usize], voiced: &target.voiced, ml: &target.ml },
            &PrevState { l: prev.l, log2_ml: &prev.log2_ml, gamma: prev.gamma },
            &tables,
        );
        let again = RawParameters { b0: raw.b0, b1: q.b1, b2: q.b2, b3: q.b3, b4: q.b4, b5: q.b5, b6: q.b6, b7: q.b7, b8: q.b8 };
        let mut replay = DecoderState { l: prev.l, log2_ml: prev.log2_ml.clone(), gamma: prev.gamma };
        let DequantizedFrame::Speech(back) = dequantize(&again, &mut replay) else { panic!("re-decode is not speech") };
        for h in 1..=l as usize {
            let (a, b) = (target.ml[h], back.ml[h]);
            assert!((a - b).abs() <= 1e-6 * a.abs().max(1.0), "b0={} harmonic {h}: decoded {a}, re-decoded {b}", raw.b0);
        }
        checked += 1;
    }
    assert!(checked > FRAMES * 9 / 10, "too few speech frames checked: {checked}");
}
