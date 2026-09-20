// SPDX-License-Identifier: LGPL-3.0-or-later
//! Parity of the fixed-point TIA-102.BABA `Encoder` (`ambe::fixed::tia_102_baba::encoder::Encoder`, built on
//! `encode::encode_frame`) with the float one on the first 150 frames of each of the four OSR speech files and on a
//! mid-speech window, both streams decoded with the float decoder. `--nocapture` prints the measured numbers; the
//! thresholds sit just below them. See the measurement notes next to each assertion.

mod common;

use common::{compare_streams, FrameView, Parity};
use ham_digital_modes::ambe::fixed::tia_102_baba::encoder::Encoder as FixedEncoder;
use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::encoder::Encoder as FloatEncoder;

type Frame = [u32; 8];

fn view(frame: Frame) -> FrameView {
    match DecoderState::new().decode_parameters(frame) {
        Some(FrameOutcome::Decoded(p)) => FrameView { tone: false, b0: p.bits.b0, b1: p.bits.b1 },
        _ => FrameView { tone: false, b0: u32::MAX, b1: u32::MAX },
    }
}

fn encode_fixed(pcm: &[i16]) -> Vec<Frame> {
    let mut enc = FixedEncoder::new();
    let mut out = Vec::new();
    for chunk in pcm.chunks(97) {
        enc.push_samples(chunk);
        while let Some(f) = enc.next_frame() {
            out.push(f);
        }
    }
    out.extend(enc.finish());
    assert_eq!(enc.failed_frames, 0);
    out
}

fn encode_float(pcm: &[i16]) -> Vec<Frame> {
    let mut enc = FloatEncoder::new();
    let mut out = Vec::new();
    for chunk in pcm.chunks(97) {
        let c: Vec<f64> = chunk.iter().map(|&s| s as f64).collect();
        enc.push_samples(&c);
        while let Some(f) = enc.next_frame() {
            out.push(f);
        }
    }
    out.extend(enc.finish());
    assert_eq!(enc.failed_frames, 0);
    out
}

fn parity(name: &str, start: usize, frames: usize) -> Parity {
    let mut p = Parity::default();
    for path in common::OSR_FILES {
        let pcm = common::read_wav_mono_i16(path);
        let lo = (start * 160).min(pcm.len());
        let hi = (lo + frames * 160).min(pcm.len());
        let pcm = &pcm[lo..hi];
        let (fx, fl) = (encode_fixed(pcm), encode_float(pcm));
        compare_streams(&mut p, &fx, &fl, view, || {
            let mut d = DecoderState::new();
            Box::new(move |f| d.decode_frame(f))
        });
    }
    p.report(name);
    p
}

#[test]
fn tia_parity_first_150_frames() {
    let p = parity("TIA-102.BABA first 150 frames", 0, common::parity_frames());
    assert_eq!(p.count_mismatch, 0);
    assert!(p.frames >= 4 * (common::parity_frames().min(1000) - 2));
    // Measured: 88.3% of code vectors identical, b0 and b1 always equal, decoded SNR 34.8 dB. (Was 96.2% and 46.4 dB
    // before both encoders applied the standard's input high-pass filter, Eq. 3: with the DC offset gone the quiet
    // opening frames sit at the noise floor, where a one-step difference in a higher-order coefficient quantizer
    // between the float and integer arithmetic is more common, and the closed prediction loop carries it forward.
    // The differing frames all differ only in the low-priority vectors u4..u6.)
    assert!(p.frac(p.identical) >= 0.85, "identical {}", p.frac(p.identical));
    assert!(p.frac(p.b0_equal) >= 0.999 && p.frac(p.b1_equal) >= 0.999);
    assert!(p.snr_db() >= 30.0, "decoded SNR {} dB", p.snr_db());
    assert!(p.min_envelope_corr() >= 0.9999);
}

#[test]
fn tia_parity_mid_file_window() {
    let p = parity("TIA-102.BABA frames 600..750", 600, 150);
    assert_eq!(p.count_mismatch, 0);
    // Measured: 95.0% identical, b1 always equal, b0 equal on 99.83% (one frame in 600 is a half-sample tie in the
    // initial pitch estimate, which limits the decoded SNR over this window to 16.6 dB; before the input high-pass
    // filter, Eq. 3, was added the measurement was 90.2% identical, all b0 equal, 68.3 dB). The closed prediction loop
    // lets a codebook near-tie in one frame perturb the next few frames' choices.
    assert!(p.frac(p.identical) >= 0.93, "identical {}", p.frac(p.identical));
    assert!(p.frac(p.b0_equal) >= 0.998 && p.frac(p.b1_equal) >= 0.999);
    assert!(p.snr_db() >= 15.0, "decoded SNR {} dB", p.snr_db());
    assert!(p.min_envelope_corr() >= 0.9999);
}

/// The fixed encoder's frames decode with the fixed decoder (the full fixed-point chain, no float anywhere) to speech
/// whose level tracks the input's.
#[test]
fn fixed_frames_decode_with_the_fixed_decoder() {
    use ham_digital_modes::ambe::fixed::tia_102_baba::decode::DecoderState as FixedDecoder;
    let pcm = common::read_wav_mono_i16(common::OSR_FILES[0]);
    let pcm = &pcm[600 * 160..750 * 160];
    let frames = encode_fixed(pcm);
    let mut dec = FixedDecoder::new();
    let (mut input_energy, mut output_energy) = (0f64, 0f64);
    for (k, &c) in frames.iter().enumerate().take(140) {
        let y = dec.decode_frame(c).expect("fixed decode");
        output_energy += y.iter().map(|&s| (s as f64 / 65536.0).powi(2)).sum::<f64>();
        input_energy += pcm[k * 160..(k + 1) * 160].iter().map(|&s| (s as f64).powi(2)).sum::<f64>();
    }
    let ratio_db = 10.0 * (output_energy / input_energy).log10();
    println!("fixed decode level vs input: {ratio_db:.2} dB");
    assert!(ratio_db.abs() < 6.0, "decoded level {ratio_db} dB from the input's");
}

/// Eq. 45's integer `b_hat_0` matches the float formula for every refined period the analyzer can produce.
#[test]
fn quantizer_b0_matches_the_float_formula_for_every_period() {
    use ham_digital_modes::ambe::fixed::tia_102_baba::pitch_refinement::Pitch;
    use ham_digital_modes::ambe::float::tia_102_baba::parameter_encoding::quantize_fundamental_frequency;
    // Where `p8 / 4 - 39` is exactly an integer (`p8` a multiple of 4) the float `floor` lands on either side depending
    // on rounding of `4 pi / omega0`; the integer result is the exact one (`p8 / 4 - 39`), so check that there and
    // require exact agreement everywhere else.
    let (mut differ_off_boundary, mut differ_on_boundary) = (0, 0);
    for p8 in 19 * 8..=124 * 8 {
        let omega0 = 2.0 * std::f64::consts::PI / (p8 as f64 / 8.0);
        let fixed = Pitch::from_p8(p8).quantizer_b0();
        let float = quantize_fundamental_frequency(omega0);
        if p8 % 4 == 0 {
            assert_eq!(fixed as i64, (p8 as i64 / 4 - 39).max(0), "exact value at p8 = {p8}");
            differ_on_boundary += usize::from(fixed != float);
        } else {
            differ_off_boundary += usize::from(fixed != float);
        }
    }
    println!("b0 differs from the float formula only at exact boundaries: {differ_on_boundary} of them");
    assert_eq!(differ_off_boundary, 0);
}
