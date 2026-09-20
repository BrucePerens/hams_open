// SPDX-License-Identifier: LGPL-3.0-or-later
//! Regression tests from the encoder cross-validation against the OP25 `imbe_vocoder` (Pavel Yazev's fixed-point
//! IMBE encoder and decoder, GPL, used only as an external oracle built in a scratch directory; see
//! `docs/references/tia_102_baba_cross_validation.md`, "Encoder oracle"). Everything asserted here is a small number
//! measured from that oracle (or from this crate's own analysis of a synthetic signal), never oracle source.
//!
//! * The input high-pass filter of Eq. 3 (missing before the cross-validation): a signal with a DC offset must
//!   analyse like the same signal without one.
//! * The pitch error function `E(P)` (Eq. 5) is never negative.
//! * Steady-state `(b0, L)` on synthetic signals equal the oracle's.
//! * Bit prioritization (Fig. 22) produces the oracle's eight bit vectors and inverts them.
//! * The decoder's enhanced amplitudes equal the oracle decoder's (the oracle stores them times four).
//! * Silence encodes to the lowest pitch index and no voiced band, as in the oracle.

use ham_digital_modes::ambe::float::tia_102_baba::bit_prioritization::{deprioritize_bits, prioritize_bits};
use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::encode_code_vectors;
use ham_digital_modes::ambe::float::tia_102_baba::encoder::{Encoder, FrameAnalyzer};
use ham_digital_modes::ambe::float::tia_102_baba::enhancement::enhance_spectral_amplitudes;
use ham_digital_modes::ambe::float::tia_102_baba::pitch::PitchAnalysisFrame;
use ham_digital_modes::ambe::float::tia_102_baba::tables::{gain_bit_allocation, higher_order_bit_allocation};
use ham_digital_modes::ambe::float::tia_102_baba::vuv::frequency_bands_count;
use std::f64::consts::PI;

const FRAMES: usize = 30;

/// `sum_h 6000/h sin(2 pi h t / period + 0.3 h^2)` for `h < period / 2`, rounded to 16-bit PCM: the signal the
/// oracle was measured on.
fn harmonic_signal(period: f64) -> Vec<f64> {
    (0..FRAMES * 160)
        .map(|t| {
            let mut x = 0.0;
            let mut h = 1;
            while (h as f64) < period / 2.0 {
                x += 6000.0 / h as f64 * (2.0 * PI * h as f64 * t as f64 / period + 0.3 * (h * h) as f64).sin();
                h += 1;
            }
            x.round()
        })
        .collect()
}

fn pulse_train(period: usize) -> Vec<f64> {
    (0..FRAMES * 160).map(|t| if t % period == 0 { 20000.0 } else { 0.0 }).collect()
}

/// `(b0, L, voiced-band count)` of every frame the encoder produces for `pcm`.
fn encode(pcm: &[f64]) -> Vec<(u32, u32, usize)> {
    let mut enc = Encoder::new();
    enc.push_samples(pcm);
    let mut frames: Vec<[u32; 8]> = std::iter::from_fn(|| enc.next_frame()).collect();
    frames.extend(enc.finish());
    assert_eq!(enc.failed_frames, 0);
    frames
        .iter()
        .map(|&c| match DecoderState::new().decode_parameters(c) {
            Some(FrameOutcome::Decoded(p)) => (p.bits.b0, p.l_hat, p.voiced.iter().filter(|&&v| v).count()),
            _ => panic!("encoder produced an undecodable frame"),
        })
        .collect()
}

/// Steady-state frames (12..=26) of the oracle encoder's output for the same signals: `(signal, b0, L)`, where the
/// initial pitch estimate `2 P` was 60, 67, 96, 140, 162, 50 and 100 (the oracle refines to an eighth of a sample,
/// this crate to a quarter, so `b0` agrees only where the period sits on the half-sample grid: all these do).
#[test]
fn steady_state_b0_and_l_match_the_oracle_on_synthetic_signals() {
    let cases: [(&str, Vec<f64>, u32, u32); 7] = [
        ("harmonic period 30", harmonic_signal(30.0), 21, 13),
        ("harmonic period 33.3", harmonic_signal(33.3), 27, 14),
        ("harmonic period 47.7", harmonic_signal(47.7), 56, 22),
        ("harmonic period 70", harmonic_signal(70.0), 101, 32),
        ("harmonic period 81.25", harmonic_signal(81.25), 123, 37),
        ("pulse train period 25", pulse_train(25), 11, 11),
        ("pulse train period 50", pulse_train(50), 61, 23),
    ];
    for (name, pcm, b0, l) in cases {
        let frames = encode(&pcm);
        let agree = frames[14..28].iter().filter(|&&(fb0, fl, _)| fb0 == b0 && fl == l).count();
        assert!(agree >= 13, "{name}: only {agree} of 14 steady frames give b0 {b0}, L {l}: {:?}", &frames[14..28]);
    }
}

/// Median `E(P_hat_I)` over the steady frames of `pcm` as the bare (unfiltered) analyzer sees it.
fn median_initial_pitch_error(pcm: &[f64]) -> f64 {
    let mut analyzer = FrameAnalyzer::new();
    analyzer.push_samples(pcm);
    let mut errors: Vec<f64> = std::iter::from_fn(|| analyzer.next_analysis()).skip(4).map(|a| a.initial_pitch_error).collect();
    errors.sort_by(|a, b| a.total_cmp(b));
    errors[errors.len() / 2]
}

/// Eq. 3: a constant offset is correlated with itself at every lag, so without the high-pass filter noise with an
/// offset looks strongly periodic at any pitch (a small error function everywhere) and would be coded as voiced
/// speech. The encoder filters first, so it sees the noise for what it is. (The bare `FrameAnalyzer`, shared with the
/// other codec modes, deliberately does not filter.) This is what the oracle comparison caught: recordings with a DC
/// offset, quiet passages especially, gave pitch estimates that disagreed with the oracle's on 17% of frames.
#[test]
fn a_dc_offset_does_not_make_noise_look_periodic() {
    let mut state = 0x1234_5678_9abc_def1u64;
    let mut noise = || {
        // xorshift, summed to a roughly Gaussian value of RMS about 100
        (0..4)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state % 1000) as f64 / 1000.0 - 0.5
            })
            .sum::<f64>()
            * 100.0
    };
    let noisy: Vec<f64> = (0..FRAMES * 160).map(|_| noise().round()).collect();
    let with_dc: Vec<f64> = noisy.iter().map(|x| x - 300.0).collect();

    let plain = median_initial_pitch_error(&noisy);
    let offset_unfiltered = median_initial_pitch_error(&with_dc);
    assert!(plain > 0.6, "noise alone: E {plain}");
    assert!(offset_unfiltered < 0.3, "noise with an offset, unfiltered: E {offset_unfiltered}");

    // The encoder's filter removes the offset, so it codes the offset noise like the noise alone: the same b0 in nearly
    // every frame and about the same number of voiced bands.
    let (clean_frames, dc_frames) = (encode(&noisy), encode(&with_dc));
    let same_pitch = clean_frames.iter().zip(&dc_frames).skip(4).filter(|(a, b)| a.0 == b.0).count();
    assert!(same_pitch + 4 >= clean_frames.len() - 3, "b0 differs in {} frames", clean_frames.len() - 4 - same_pitch);
    let voiced = |f: &[(u32, u32, usize)]| f.iter().skip(4).map(|x| x.2).sum::<usize>() as f64;
    let (a, b) = (voiced(&clean_frames), voiced(&dc_frames));
    assert!((a - b).abs() <= 0.1 * a.max(b), "voiced bands: {a} without the offset, {b} with it");
}

#[test]
fn the_error_function_is_never_negative() {
    // A perfectly periodic signal drove Eq. 5 a few thousandths below zero at multiples of the period.
    let pcm = harmonic_signal(40.0);
    let mut raw = vec![0.0; 200];
    raw.extend(&pcm);
    raw.extend(vec![0.0; 400]);
    for center in [1000usize, 1500, 2000] {
        let frame = PitchAnalysisFrame::new(&raw, center);
        for i in 0..203 {
            let e = frame.error_function(21.0 + 0.5 * i as f64);
            assert!(e >= 0.0 && e.is_finite(), "E({}) = {e}", 21.0 + 0.5 * i as f64);
        }
    }
}

#[test]
fn silence_encodes_to_the_lowest_pitch_index_with_nothing_voiced() {
    // With no evidence every candidate scores E = 1, so look-back tracking walks down by the 20% limit of Eq. 10 each
    // frame (ties go to the lowest pitch) until it reaches the floor. The oracle, started from the same state, gives
    // exactly this b0 and L sequence; nothing is ever voiced.
    let frames = encode(&vec![0.0; FRAMES * 160]);
    let expected_b0 = [118, 86, 61, 41, 25, 12, 2];
    let expected_l = [36, 28, 23, 18, 14, 12, 9];
    for (k, &(b0, l, voiced)) in frames.iter().enumerate() {
        let (eb0, el) = if k < 7 { (expected_b0[k], expected_l[k]) } else { (0, 9) };
        assert_eq!((b0, l, voiced), (eb0, el, 0), "frame {k}");
    }
}

/// `(L, b0, b1, b2, gain elements b3..b7, higher-order b8..b_{L+1} with zero-width entries as 0, sync)` and the eight
/// vectors the oracle's frame packer produced for the same values.
const BIT_VECTORS: &[(&str, [u32; 8])] = &[
    ("9 2 6 1 720 365 271 217 212 121 149 38 0", [0x5, 0x89b, 0xb88, 0xb3c, 0x63a, 0x1ac, 0x42e, 0x6c]),
    ("9 1 7 54 1023 511 511 511 511 511 255 127 1", [0x37, 0xfff, 0xfff, 0xfff, 0x7ff, 0x7ff, 0x7ff, 0x73]),
    (
        "23 63 120 38 25 4 12 9 2 9 4 14 2 4 7 4 2 0 2 3 3 0 1 3 1 1 1",
        [0x3e6, 0xd8c, 0x6e0, 0x9ab, 0x3c7, 0x294, 0x10d, 0x77],
    ),
    (
        "36 116 2334 12 13 0 6 2 5 2 3 3 3 0 7 1 1 0 0 6 3 0 0 0 3 1 1 1 0 3 1 1 1 0 0 1 1 0 0 0",
        [0x74e, 0xacd, 0xf1d, 0x45d, 0x48f, 0x2c4, 0x3de, 0x60],
    ),
    (
        "37 123 855 49 2 1 0 0 1 2 0 2 3 1 5 2 1 0 2 5 3 0 0 0 0 1 1 0 1 0 1 0 0 0 1 0 0 1 1 0 1",
        [0x7b0, 0x1c2, 0xd28, 0x48e, 0x1ab, 0x498, 0x352, 0x3f],
    ),
    (
        "56 206 3664 12 7 7 3 3 3 7 3 3 1 1 1 1 0 7 3 3 1 1 1 1 0 3 3 1 1 1 1 1 0 3 3 1 1 1 0 0 0 3 1 1 1 1 0 0 0 0 3 1 \
         1 1 0 0 0 0 0 0",
        [0xccf, 0xfff, 0xfff, 0xfff, 0x728, 0x2ff, 0x7ff, 0x74],
    ),
];

#[test]
fn bit_prioritization_matches_the_oracle_packer_and_unpacker() {
    for &(line, expected) in BIT_VECTORS {
        let v: Vec<u32> = line.split_whitespace().map(|t| t.parse().unwrap()).collect();
        let (l, b0, b1, b2) = (v[0], v[1], v[2], v[3]);
        let k = frequency_bands_count(l);
        let gain_widths: [u8; 5] = std::array::from_fn(|i| gain_bit_allocation(l, i as u32 + 2).unwrap().0);
        let ho_widths = higher_order_bit_allocation(l).unwrap();
        let gain: [(u32, u8); 5] = std::array::from_fn(|i| (v[4 + i], gain_widths[i]));
        let hoc_values = &v[9..v.len() - 1];
        assert_eq!(hoc_values.len(), ho_widths.len(), "line for L = {l}");
        let higher_order: Vec<(u32, u8)> =
            ho_widths.iter().zip(hoc_values).filter(|(&w, _)| w > 0).map(|(&w, &x)| (x, w)).collect();
        let sync = *v.last().unwrap() == 1;

        let u = prioritize_bits(b0, b1, k, b2, gain, &higher_order, sync).unwrap();
        assert_eq!(u, expected, "L = {l}");

        let nonzero: Vec<u8> = ho_widths.iter().copied().filter(|&w| w > 0).collect();
        let back = deprioritize_bits(expected, k, gain_widths, &nonzero).unwrap();
        assert_eq!((back.b0, back.b1, back.b2, back.sync_bit), (b0, b1, b2, sync));
        assert_eq!(back.gain_vector, gain);
        assert_eq!(back.higher_order, higher_order);
    }
}

/// Four consecutive oracle-encoded frames (steady state of the period-70 harmonic signal) decoded from the initial
/// state by the oracle decoder: its enhanced amplitudes for harmonics 1..32, which it stores times four.
const ORACLE_DECODE_CHAIN: [([u32; 8], [u32; 32]); 4] = [
    (
        [0x67e, 0x7f8, 0xcd, 0xf42, 0x7ff, 0x400, 0x91, 0x3b],
        [
            2263, 1262, 1261, 977, 756, 1193, 587, 762, 676, 696, 1259, 923, 556, 1062, 734, 1235, 786, 488, 959, 575,
            846, 1455, 521, 488, 556, 851, 535, 364, 767, 1220, 783, 642,
        ],
    ),
    (
        [0x67e, 0x7ec, 0x9d, 0xba0, 0x7ff, 0x4a0, 0x6de, 0x6b],
        [
            4268, 2246, 1760, 2014, 1339, 1398, 619, 1234, 815, 602, 1865, 672, 673, 854, 776, 976, 323, 467, 855, 739,
            1364, 677, 445, 348, 447, 585, 823, 449, 362, 650, 759, 389,
        ],
    ),
    (
        [0x67e, 0x7f8, 0xc0, 0x4c2, 0x7ff, 0x41b, 0x56e, 0x5b],
        [
            7162, 2548, 2147, 1829, 1114, 1343, 971, 788, 680, 684, 1648, 958, 1280, 640, 762, 891, 545, 840, 485, 609,
            969, 299, 678, 619, 655, 523, 981, 500, 418, 381, 342, 534,
        ],
    ),
    (
        [0x67e, 0x6fc, 0x4cd, 0xfa4, 0x7ff, 0x400, 0x90, 0x3b],
        [
            9919, 2966, 3093, 2521, 2101, 2486, 982, 1105, 882, 911, 1696, 865, 635, 740, 574, 1265, 590, 492, 654, 458,
            705, 571, 335, 293, 346, 448, 931, 349, 506, 595, 264, 256,
        ],
    ),
];

#[test]
fn enhanced_amplitudes_equal_the_oracle_decoders_to_within_its_integer_resolution() {
    let mut dec = DecoderState::new();
    for (n, (frame, oracle)) in ORACLE_DECODE_CHAIN.iter().enumerate() {
        let c = encode_code_vectors(*frame);
        let Some(FrameOutcome::Decoded(p)) = dec.decode_parameters(c) else { panic!("frame {n} did not decode") };
        assert_eq!(p.l_hat, 32);
        let enhanced = enhance_spectral_amplitudes(&p.reconstructed_amplitudes, p.omega0_tilde);
        for (l, (&ours, &theirs)) in enhanced.iter().zip(oracle.iter()).enumerate() {
            let expected = theirs as f64 / 4.0;
            // The oracle stores 4 M as a 16-bit integer: half a unit of resolution on 4 M, plus its rounding.
            let tolerance = 0.02 * expected + 0.5;
            assert!((ours - expected).abs() <= tolerance, "frame {n}, harmonic {}: ours {ours}, oracle {expected}", l + 1);
        }
        dec.advance_history(&p);
    }
}
