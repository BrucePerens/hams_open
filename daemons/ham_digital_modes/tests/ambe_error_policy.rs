// SPDX-License-Identifier: LGPL-3.0-or-later
//! The two damaged-frame policies (`ErrorPolicy`) in the float and fixed-point D-STAR and AMBE+2 decoders.
//!
//! `Clean` (the default) is mbelib's: more than 3 corrected errors in total repeats the previous frame, and the fourth
//! repeat in a row mutes. `ChipCompatible` copies the real chip, which repeats whenever the first Golay block corrected 3
//! errors and never mutes (it decodes garbage after three repeats, the chip's audible "squeaks"). The chip behaviour
//! is opt-in, for conformance tests only.

use ham_digital_modes::ambe::float::mbe_synthesis::ErrorPolicy;

/// Flips in C0's Golay codeword (frame bits 49..=71): three errors, which the code corrects.
const THREE_C0_ERRORS: u128 = (1u128 << 70) | (1u128 << 60) | (1u128 << 52);
/// Two more in C1's codeword (bits 25..=47): five corrected errors in total.
const PLUS_TWO_C1_ERRORS: u128 = (1u128 << 45) | (1u128 << 35);

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|s| s * s).sum::<f64>() / x.len() as f64).sqrt()
}

macro_rules! policy_tests {
    ($modname:ident, $decoder:ty, $quiet:expr, $loud:expr, to_f64 = $to_f64:expr) => {
        mod $modname {
            use super::*;

            fn decode(dec: &mut $decoder, frame: u128) -> Vec<f64> {
                let to_f64: fn(&[_]) -> Vec<f64> = $to_f64;
                to_f64(&dec.decode_frame(frame).expect("frame decodes"))
            }

            fn quiet_and_loud() -> (u128, u128) {
                ($quiet, $loud)
            }

            #[test]
            fn three_errors_in_c0_alone_are_trusted_by_clean_and_repeated_by_chip() {
                let (quiet, loud) = quiet_and_loud();
                let level_after = |policy: ErrorPolicy| {
                    let mut dec = <$decoder>::new().with_error_policy(policy);
                    for _ in 0..6 {
                        decode(&mut dec, quiet);
                    }
                    rms(&decode(&mut dec, loud ^ THREE_C0_ERRORS))
                };
                let (clean, chip) = (level_after(ErrorPolicy::Clean), level_after(ErrorPolicy::ChipCompatible));
                assert!(clean > 3.0 * chip, "clean {clean} sounds like the loud frame, chip {chip} like the repeated quiet one");
            }

            #[test]
            fn clean_mutes_on_the_fourth_damaged_frame_and_chip_keeps_decoding() {
                let (quiet, loud) = quiet_and_loud();
                let bad = loud ^ THREE_C0_ERRORS ^ PLUS_TWO_C1_ERRORS;
                for (policy, expect_mute) in [(ErrorPolicy::Clean, true), (ErrorPolicy::ChipCompatible, false)] {
                    let mut dec = <$decoder>::new().with_error_policy(policy);
                    decode(&mut dec, quiet);
                    for i in 0..3 {
                        assert!(rms(&decode(&mut dec, bad)) > 0.0, "{policy:?}: damaged frame {i} repeats");
                    }
                    let fourth = decode(&mut dec, bad);
                    assert_eq!(fourth.iter().all(|&s| s == 0.0), expect_mute, "{policy:?}: fourth damaged frame");
                }
            }

            #[test]
            fn the_default_policy_is_concealing_which_fades_instead_of_muting() {
                let (quiet, loud) = quiet_and_loud();
                let bad = loud ^ THREE_C0_ERRORS ^ PLUS_TWO_C1_ERRORS;
                let mut dec = <$decoder>::new();
                decode(&mut dec, quiet);
                for _ in 0..4 {
                    assert!(rms(&decode(&mut dec, bad)) > 0.0, "the default decoder does not mute after three repeats");
                }
            }
        }
    };
}

fn fixed_to_f64(x: &[i64]) -> Vec<f64> {
    x.iter().map(|&v| v as f64 / 65536.0).collect()
}
fn float_identity(x: &[f64]) -> Vec<f64> {
    x.to_vec()
}

mod dstar_frames {
    use ham_digital_modes::ambe::float::dstar::decode::RawParameters;
    use ham_digital_modes::ambe::float::dstar::encode::{build_frame, pack_raw_parameters};

    pub fn quiet() -> u128 {
        build_frame(pack_raw_parameters(&RawParameters { b0: 40, b1: 15, b2: 12, b3: 100, b4: 50, b5: 3, b6: 4, b7: 5, b8: 2 }))
    }
    pub fn loud() -> u128 {
        build_frame(pack_raw_parameters(&RawParameters { b0: 40, b1: 15, b2: 40, b3: 100, b4: 50, b5: 3, b6: 4, b7: 5, b8: 2 }))
    }
}

policy_tests!(
    dstar_float,
    ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder,
    dstar_frames::quiet(),
    dstar_frames::loud(),
    to_f64 = float_identity
);
policy_tests!(
    dstar_fixed,
    ham_digital_modes::ambe::fixed::dstar::synthesis::DStarSynthesisDecoder,
    dstar_frames::quiet(),
    dstar_frames::loud(),
    to_f64 = fixed_to_f64
);

#[cfg(feature = "ambe_plus_2")]
mod ambe_plus_2_frames {
    use ham_digital_modes::ambe::float::ambe_plus_2::decode::RawParameters;
    use ham_digital_modes::ambe::float::ambe_plus_2::encode::build_frame;

    pub fn quiet() -> u128 {
        build_frame(&RawParameters { b0: 40, b1: 31, b2: 8, b3: 100, b4: 50, b5: 3, b6: 4, b7: 5, b8: 2 })
    }
    pub fn loud() -> u128 {
        build_frame(&RawParameters { b0: 40, b1: 31, b2: 26, b3: 100, b4: 50, b5: 3, b6: 4, b7: 5, b8: 2 })
    }
}

#[cfg(feature = "ambe_plus_2")]
policy_tests!(
    ambe_plus_2_float,
    ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder,
    ambe_plus_2_frames::quiet(),
    ambe_plus_2_frames::loud(),
    to_f64 = float_identity
);
#[cfg(feature = "ambe_plus_2")]
policy_tests!(
    ambe_plus_2_fixed,
    ham_digital_modes::ambe::fixed::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder,
    ambe_plus_2_frames::quiet(),
    ambe_plus_2_frames::loud(),
    to_f64 = fixed_to_f64
);

/// The chip treats the reserved D-STAR pitch codes 125 and 127 as invalid frames (repeat three times, then mute); normal operation
/// decodes them leniently.
mod dstar_reserved_codes {
    use super::*;
    use ham_digital_modes::ambe::float::dstar::decode::RawParameters;
    use ham_digital_modes::ambe::float::dstar::encode::{build_frame, pack_raw_parameters};

    fn frame(b0: u32) -> u128 {
        build_frame(pack_raw_parameters(&RawParameters { b0, b1: 15, b2: 30, b3: 100, b4: 50, b5: 3, b6: 4, b7: 5, b8: 2 }))
    }

    #[test]
    fn chip_compatible_repeats_then_mutes_reserved_pitch_code_125_and_clean_decodes_it() {
        use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder;
        let (normal, reserved) = (frame(40), frame(125));
        for (policy, expect_mute_on_fourth) in [(ErrorPolicy::Clean, false), (ErrorPolicy::ChipCompatible, true)] {
            let mut dec = DStarSynthesisDecoder::new().with_error_policy(policy);
            dec.decode_frame(normal).unwrap();
            let outputs: Vec<[f64; 160]> = (0..4).map(|_| dec.decode_frame(reserved).unwrap()).collect();
            assert_eq!(outputs[3].iter().all(|&s| s == 0.0), expect_mute_on_fourth, "{policy:?}");
            if expect_mute_on_fourth {
                assert!(outputs[..3].iter().all(|o| o.iter().any(|&s| s != 0.0)), "the first three repeat");
            }
        }
    }

    #[test]
    fn fixed_decoder_matches_on_reserved_pitch_code_125() {
        use ham_digital_modes::ambe::fixed::dstar::synthesis::DStarSynthesisDecoder;
        let (normal, reserved) = (frame(40), frame(125));
        let mut dec = DStarSynthesisDecoder::new().with_error_policy(ErrorPolicy::ChipCompatible);
        dec.decode_frame(normal).unwrap();
        let out: Vec<_> = (0..4).map(|_| dec.decode_frame(reserved).unwrap()).collect();
        assert!(out[3].iter().all(|&s| s == 0), "the fourth reserved frame mutes");
        assert!(out[0].iter().any(|&s| s != 0));
    }
}
