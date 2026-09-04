// SPDX-License-Identifier: LGPL-3.0-or-later
//! Decoder-side spectral envelope: turns `LPC_ORD+1` LPC coefficients
//! into per-harmonic amplitudes (`Model::a`), by sampling the LPC
//! filter's own frequency response at each harmonic and blending in a
//! standard LPC postfilter (formant sharpening via a bandwidth-expanded
//! "gamma" copy of the filter, energy-normalized so the sharpening
//! doesn't change overall loudness) -- the general postfilter technique
//! traces to Chen & Gersho's classic adaptive postfiltering work, not
//! anything specific to this one codec's own source; reimplemented here
//! from that general understanding.
//!
//! Purely decoder-*audio-quality*-internal, not a bitstream format
//! question (see `mod.rs`'s own note on asymmetric interoperability):
//! nothing here is transmitted, so exact formulas are a design choice,
//! though the general shape (spectral-envelope sampling weighted by
//! harmonic bandwidth, gamma-based postfiltering) is kept close to the
//! reference's own real, published approach since it's a genuinely
//! good, well-motivated technique, not incidental.

use super::{FFT_ENC, LPCPF_GAMMA, LPCPF_TWO_BETA, LPC_ORD, MAX_AMP, SAMPLE_RATE};
use rustfft::num_complex::Complex32;
use rustfft::Fft;

/// Sinusoidal-synthesis model parameters for one 10ms sub-frame: pitch
/// (`wo`, normalized angular frequency), harmonic count (`l`), per
/// harmonic amplitude/phase (`a`/`phi`, both 1-indexed -- index 0
/// unused, matching the harmonics' own 1-based numbering), and voicing.
pub struct Model {
    pub wo: f32,
    pub l: usize,
    pub a: [f32; MAX_AMP + 1],
    pub phi: [f32; MAX_AMP + 1],
    pub voiced: bool,
}

impl Model {
    pub fn new(wo: f32, voiced: bool) -> Self {
        let l = ((std::f32::consts::PI / wo) as usize).min(MAX_AMP);
        Model {
            wo,
            l,
            a: [0.0; MAX_AMP + 1],
            phi: [0.0; MAX_AMP + 1],
            voiced,
        }
    }
}

/// Bins actually used from an `FFT_ENC`-point real-input spectrum (the
/// rest is the conjugate mirror, redundant).
const SPEC_BINS: usize = FFT_ENC / 2 + 1;

/// `ak[]` zero-padded into a `FFT_ENC`-point real buffer, forward FFT'd,
/// returning the complex spectrum's first `SPEC_BINS` bins. Fixed-size
/// stack buffers throughout (`FFT_ENC` is a compile-time constant) --
/// this runs twice per 10ms sub-frame on a real-time codec's decode
/// path, so no heap allocation here.
fn lpc_spectrum(fft: &dyn Fft<f32>, ak: &[f32; LPC_ORD + 1]) -> [Complex32; SPEC_BINS] {
    let mut buf = [Complex32::new(0.0, 0.0); FFT_ENC];
    for (i, &a) in ak.iter().enumerate() {
        buf[i] = Complex32::new(a, 0.0);
    }
    fft.process(&mut buf);
    std::array::from_fn(|i| buf[i])
}

/// Computes `model.a[1..=model.l]` from `ak`/`e` (the real LPC energy),
/// and returns the raw LPC spectrum (`Aw`, `SPEC_BINS` complex bins)
/// alongside it, since `synthesis.rs`'s own phase reconstruction needs
/// that same spectrum (`H[m] = conj(Aw[bin])`, the synthesis filter
/// being the LPC analysis filter's own phase response, reversed).
pub fn compute_harmonic_amplitudes(
    fft: &dyn Fft<f32>,
    ak: &[f32; LPC_ORD + 1],
    e: f32,
    model: &mut Model,
) -> [Complex32; SPEC_BINS] {
    let aw = lpc_spectrum(fft, ak);
    let a2: [f32; SPEC_BINS] =
        std::array::from_fn(|i| aw[i].re * aw[i].re + aw[i].im * aw[i].im + 1e-6);

    let mut ak_gamma = [0.0f32; LPC_ORD + 1];
    ak_gamma[0] = ak[0];
    let mut g = LPCPF_GAMMA;
    for i in 1..=LPC_ORD {
        ak_gamma[i] = ak[i] * g;
        g *= LPCPF_GAMMA;
    }
    let awg = lpc_spectrum(fft, &ak_gamma);
    let a2g: [f32; SPEC_BINS] =
        std::array::from_fn(|i| awg[i].re * awg[i].re + awg[i].im * awg[i].im + 1e-6);

    let mut e_before = 1e-12f32;
    let mut e_after = 1e-12f32;
    // Matches the reference's own real range (0..FFT_ENC/2, excluding
    // the Nyquist bin) -- `a2`/`a2g` are one element longer than this
    // (`lpc_spectrum` keeps FFT_ENC/2+1 bins, since `sample_filter_phase`
    // separately needs that same range), but this gain normalization
    // sum shouldn't include the extra bin.
    for i in 0..(FFT_ENC / 2) {
        let inv_a2 = 1.0 / a2[i];
        let r = (a2g[i] * inv_a2).sqrt();
        e_before += inv_a2;
        e_after += inv_a2 * r.powf(LPCPF_TWO_BETA);
    }
    let gain = e * e_before / e_after;

    let fft_r = std::f32::consts::TAU / FFT_ENC as f32;
    // `m` is the harmonic number, used in real arithmetic (`m as f32 -
    // 0.5`) well beyond plain array indexing.
    #[allow(clippy::needless_range_loop)]
    for m in 1..=model.l {
        let am = (((m as f32 - 0.5) * model.wo / fft_r) + 0.5) as usize;
        let bm = ((((m as f32 + 0.5) * model.wo / fft_r) + 0.5) as usize).min(FFT_ENC / 2);

        let mut em = 0.0f32;
        for i in am..bm {
            let r = (a2g[i] / a2[i]).sqrt();
            let mut pw_i = r.powf(LPCPF_TWO_BETA) / a2[i];
            let freq_hz = i as f32 * (SAMPLE_RATE as f32 * 0.5 / (FFT_ENC / 2) as f32);
            if freq_hz < 1000.0 {
                pw_i *= 1.96;
            }
            em += pw_i;
        }
        em *= gain;
        model.a[m] = em.sqrt();
    }

    aw
}

/// First-harmonic correction: for very low-pitched (typically male)
/// voices, LPC modelling tends to overestimate the fundamental's own
/// amplitude -- a real, documented quirk of this general modelling
/// approach at low pitch, not specific to any one codec's source; the
/// reference's own correction factor (0.032, kept here for the same
/// documented low-pitch quality reason) is a specific tuned value, but
/// applying *some* correction here is a design choice available either
/// way.
pub fn apply_first_harmonic_correction(model: &mut Model) {
    if model.wo < (std::f32::consts::PI * 150.0 / 4000.0) {
        model.a[1] *= 0.032;
    }
}

/// `H[m] = conj(Aw[bin])` for each harmonic `m` -- the synthesis
/// filter's phase response at each harmonic, opposite phase to the
/// analysis filter (`Aw`) it's derived from.
pub fn sample_filter_phase(aw: &[Complex32], model: &Model) -> [Complex32; MAX_AMP + 1] {
    let mut h = [Complex32::new(0.0, 0.0); MAX_AMP + 1];
    let fft_r = std::f32::consts::TAU / FFT_ENC as f32;
    let k = model.wo / fft_r;
    #[allow(clippy::needless_range_loop)]
    for m in 1..=model.l {
        let b = (((m as f32 * k) + 0.5) as usize).min(FFT_ENC / 2 - 1);
        h[m] = aw[b].conj();
    }
    h
}
