// SPDX-License-Identifier: LGPL-3.0-or-later
//! The float encoders fail loudly on non-finite input instead of coercing it into a plausible frame.

use ham_digital_modes::ambe::float::dstar::encoder::Encoder as DStarEncoder;
use ham_digital_modes::ambe::float::dstar::quantize::quantize_pitch as dstar_quantize_pitch;
use ham_digital_modes::ambe::float::dstar::tables as dstar_tables;
use ham_digital_modes::ambe::float::mbe_encode::{quantize_speech, ModeTables, PrevState, SpeechTarget};

#[test]
#[should_panic(expected = "non-finite sample")]
fn dstar_encoder_rejects_nan_samples() {
    DStarEncoder::new().push_samples(&[0.0, f64::NAN, 1.0]);
}

#[test]
#[should_panic(expected = "non-finite sample")]
fn dstar_encoder_rejects_infinite_samples() {
    DStarEncoder::new().push_samples(&[f64::INFINITY]);
}

#[test]
#[should_panic(expected = "finite positive")]
fn dstar_pitch_quantizer_rejects_nan() {
    dstar_quantize_pitch(f64::NAN);
}

/// The amplitude half of the same 2026-09-19 bug-hunt finding (`mbe_quantize_speech.md`): the pitch
/// half (`quantize_pitch`) was fixed the same day (`d1bc1ea4`), but `quantize_speech` itself still
/// silently floored a NaN amplitude to `1e-6` via `f64::max` until this test's own fix.
#[test]
#[should_panic(expected = "must be finite")]
fn dstar_speech_quantizer_rejects_nan_amplitude() {
    let l = 8u32;
    let voiced = vec![true; (l + 1) as usize];
    let mut ml = vec![500.0f64; (l + 1) as usize];
    ml[3] = f64::NAN;
    let mode = ModeTables {
        vuv: &dstar_tables::VUV,
        dg: &dstar_tables::DG,
        prba24: &dstar_tables::PRBA24,
        prba58: &dstar_tables::PRBA58,
        lmprbl: &dstar_tables::LMPRBL,
        hoc: [&dstar_tables::HOC_B5, &dstar_tables::HOC_B6, &dstar_tables::HOC_B7, &dstar_tables::HOC_B8],
        hoc_b8_even_only: true,
        rho: ham_digital_modes::ambe::float::dstar::decode::PREDICTOR_RHO,
        gamma_scale: ham_digital_modes::ambe::float::dstar::decode::GAMMA_SCALE,
        gamma_memory: ham_digital_modes::ambe::float::dstar::decode::GAMMA_MEMORY,
    };
    let prev_log2_ml = vec![0.0f64; (l + 1) as usize];
    quantize_speech(
        &SpeechTarget { l, w0: 2.0 * std::f64::consts::PI / 60.0, vuv_f0: 1.0 / 60.0, voiced: &voiced, ml: &ml },
        &PrevState { l, log2_ml: &prev_log2_ml, gamma: 0.0 },
        &mode,
    );
}

#[cfg(feature = "ambe_plus_2")]
mod ambe_plus_2 {
    use ham_digital_modes::ambe::float::ambe_plus_2::encoder::Encoder;
    use ham_digital_modes::ambe::float::ambe_plus_2::quantize::quantize_pitch;

    #[test]
    #[should_panic(expected = "non-finite sample")]
    fn encoder_rejects_nan_samples() {
        Encoder::new().push_samples(&[f64::NAN]);
    }

    #[test]
    #[should_panic(expected = "finite positive")]
    fn pitch_quantizer_rejects_nan() {
        quantize_pitch(f64::NAN);
    }
}
