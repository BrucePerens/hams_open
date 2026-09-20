// SPDX-License-Identifier: LGPL-3.0-or-later
//! The float encoders fail loudly on non-finite input instead of coercing it into a plausible frame.

use ham_digital_modes::ambe::float::dstar::encoder::Encoder as DStarEncoder;
use ham_digital_modes::ambe::float::dstar::quantize::quantize_pitch as dstar_quantize_pitch;

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
