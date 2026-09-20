// SPDX-License-Identifier: LGPL-3.0-or-later
//! The audio-quality damaged-frame policy (`ErrorPolicy::Concealing`) in the float D-STAR and AMBE+2 decoders: identical to the clean
//! policy on an error-free stream, and a fading repeat (not a hard mute) through a burst of ruined frames.

use ham_digital_modes::ambe::float::mbe_synthesis::ErrorPolicy;

fn read_wav(path: &str) -> Vec<f64> {
    let bytes = std::fs::read(path).unwrap();
    bytes[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).collect()
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|s| s * s).sum::<f64>() / x.len() as f64).sqrt()
}

/// Deterministic pseudo-random word source.
fn lcg(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state >> 33
}

macro_rules! concealment_tests {
    ($modname:ident, $encoder:ty, $decoder:ty) => {
        mod $modname {
            use super::*;

            fn frames() -> Vec<u128> {
                let pcm = read_wav("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
                let mut e = <$encoder>::new();
                e.push_samples(&pcm[..160 * 200]);
                let mut fr = Vec::new();
                while let Some(f) = e.next_frame() {
                    fr.push(f);
                }
                fr.extend(e.finish());
                fr
            }

            fn decode(frames: &[u128], policy: ErrorPolicy) -> Vec<f64> {
                let mut d = <$decoder>::new().with_error_policy(policy);
                frames.iter().flat_map(|&f| d.decode_frame(f).unwrap_or([0.0; 160])).collect()
            }

            #[test]
            fn a_clean_stream_decodes_exactly_like_the_clean_policy() {
                let fr = frames();
                assert_eq!(decode(&fr, ErrorPolicy::Clean), decode(&fr, ErrorPolicy::Concealing));
            }

            #[test]
            fn a_burst_of_ruined_frames_fades_instead_of_muting_and_recovers() {
                let mut fr = frames();
                // Ruin frames 100..106 (six frames, 120 ms) with random bits in the protected part and the raw part.
                let mut seed = 99u64;
                for f in fr.iter_mut().take(106).skip(100) {
                    let noise = ((lcg(&mut seed) as u128) << 40) ^ ((lcg(&mut seed) as u128) << 8);
                    *f ^= noise & ((1u128 << 72) - 1);
                }
                let out = decode(&fr, ErrorPolicy::Concealing);
                let level = |k: usize| rms(&out[k * 160..(k + 1) * 160]);
                // The stream was loud there; the ruined frames must not be silent at first, and must not be louder than what came before.
                let before = (94..100).map(level).fold(0.0, f64::max);
                assert!(before > 50.0, "test segment should be speech ({before})");
                let ruined: Vec<f64> = (100..106).map(level).collect();
                assert!(ruined.iter().all(|&l| l <= before * 1.5), "no burst louder than the speech before: {ruined:?} vs {before}");
                assert!(ruined[0] > 0.0, "the first damaged frame still sounds (repeat), not a mute");
                // After the burst the stream recovers to normal speech.
                assert!((108..118).map(level).fold(0.0, f64::max) > 20.0, "recovers after the burst");
            }
        }
    };
}

concealment_tests!(dstar, ham_digital_modes::ambe::float::dstar::encoder::Encoder, ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder);
#[cfg(feature = "ambe_plus_2")]
concealment_tests!(ambe_plus_2, ham_digital_modes::ambe::float::ambe_plus_2::encoder::Encoder, ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder);

/// The fixed-point concealment against the float one.
mod fixed_point {
    use super::*;
    use ham_digital_modes::ambe::fixed::dstar::synthesis::DStarSynthesisDecoder as FixedDecoder;
    use ham_digital_modes::ambe::float::concealment::ConcealParams as FloatParams;
    use ham_digital_modes::ambe::fixed::general::concealment::ConcealParams as FixedParams;
    use ham_digital_modes::ambe::float::dstar::encoder::Encoder;
    use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder as FloatDecoder;

    fn frames() -> Vec<u128> {
        let pcm = read_wav("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
        let mut e = Encoder::new();
        e.push_samples(&pcm[..160 * 200]);
        let mut fr = Vec::new();
        while let Some(f) = e.next_frame() {
            fr.push(f);
        }
        fr.extend(e.finish());
        fr
    }

    fn q16(x: f64) -> i64 {
        (x * 65536.0).round() as i64
    }

    #[test]
    fn fixed_constants_equal_the_float_ones() {
        let (f, x) = (FloatParams::default(), FixedParams::default());
        assert_eq!(x.level_scale_db_q16, q16(f.level_scale_db));
        assert_eq!(x.shape_scale_db_q16, q16(f.shape_scale_db));
        assert_eq!(x.voicing_cost_q16, q16(f.voicing_cost));
        assert_eq!(x.pitch_scale_q16, q16(f.pitch_scale));
        assert_eq!(x.garbage_cost_q16, q16(f.garbage_cost));
        assert_eq!(x.error_suspicion_q16, q16(f.error_suspicion));
        assert_eq!(x.ber_alpha_q16, q16(f.ber_alpha));
        assert_eq!(x.ber_floor_q32, (f.ber_floor * 4294967296.0).round() as i64);
        assert_eq!(x.flip_gate_q16, q16(f.flip_gate));
        assert_eq!(x.fade_per_frame_q16, q16(f.fade_per_frame));
        assert_eq!(x.reset_after, f.reset_after);
        assert_eq!(x.widen_per_repeat_q16, q16(f.widen_per_repeat));
    }

    #[test]
    fn a_clean_stream_decodes_exactly_like_the_fixed_clean_policy() {
        let fr = frames();
        let run = |policy| {
            let mut d = FixedDecoder::new().with_error_policy(policy);
            fr.iter().flat_map(|&f| d.decode_frame(f).unwrap_or([0; 160])).collect::<Vec<i64>>()
        };
        assert_eq!(run(ErrorPolicy::Clean), run(ErrorPolicy::Concealing));
    }

    #[test]
    fn fixed_and_float_agree_on_a_damaged_stream() {
        let mut fr = frames();
        let mut seed = 5u64;
        for f in fr.iter_mut() {
            for bit in 0..72 {
                if lcg(&mut seed) % 100 < 3 {
                    *f ^= 1u128 << bit;
                }
            }
        }
        let mut fl = FloatDecoder::new().with_error_policy(ErrorPolicy::Concealing);
        let mut fx = FixedDecoder::new().with_error_policy(ErrorPolicy::Concealing);
        let (mut agree_energy, mut n) = (0usize, 0usize);
        let (mut a, mut b) = (Vec::new(), Vec::new());
        for &f in &fr {
            let o = fl.decode_frame(f).unwrap_or([0.0; 160]);
            let x = fx.decode_frame(f).unwrap_or([0; 160]);
            let (rl, rx) = (rms(&o), rms(&x.iter().map(|&v| v as f64 / 65536.0).collect::<Vec<_>>()));
            a.push((rl + 1.0).ln());
            b.push((rx + 1.0).ln());
            n += 1;
            if (rl + 1.0).ln() - (rx + 1.0).ln() < 0.5 && (rx + 1.0).ln() - (rl + 1.0).ln() < 0.5 {
                agree_energy += 1;
            }
        }
        // The two implementations make (nearly) the same accept/repeat decisions, so frame energies track each other.
        assert!(agree_energy as f64 / n as f64 > 0.95, "frame energy agreement {}", agree_energy as f64 / n as f64);
    }

    #[test]
    fn fixed_burst_fades_instead_of_muting() {
        let mut fr = frames();
        let mut seed = 99u64;
        for f in fr.iter_mut().take(106).skip(100) {
            let noise = ((lcg(&mut seed) as u128) << 40) ^ ((lcg(&mut seed) as u128) << 8);
            *f ^= noise & ((1u128 << 72) - 1);
        }
        let mut d = FixedDecoder::new().with_error_policy(ErrorPolicy::Concealing);
        let out: Vec<f64> = fr.iter().map(|&f| rms(&d.decode_frame(f).unwrap_or([0; 160]).iter().map(|&v| v as f64 / 65536.0).collect::<Vec<_>>())).collect();
        let before = out[94..100].iter().cloned().fold(0.0, f64::max);
        assert!(before > 50.0);
        assert!(out[100] > 0.0, "the first damaged frame still sounds");
        assert!(out[100..106].iter().all(|&l| l <= before * 1.5));
        assert!(out[108..118].iter().cloned().fold(0.0, f64::max) > 20.0);
    }
}
