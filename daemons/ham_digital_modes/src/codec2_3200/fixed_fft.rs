// SPDX-License-Identifier: LGPL-3.0-or-later
//! Genuinely fixed-point, phase-correct radix-2 FFT for the decoder's
//! own `FFT_ENC`=512-point transforms: `envelope.rs`'s forward analysis
//! (`ak[]` -> `Aw[]`) and `synthesis.rs`'s inverse synthesis (a sparse
//! harmonic spectrum -> time-domain samples).
//!
//! Deliberately a separate implementation from `nlp.rs`'s own
//! `fft_fixed`, even though both are the same radix-2 DIT butterfly
//! shape at the same real point count (`PE_FFT_SIZE == FFT_ENC == 512`,
//! a coincidence between the pitch estimator's own window size and the
//! decoder's own spectrum size, not a structural relationship worth
//! coupling the two to) -- `nlp.rs`'s own version only ever reads
//! magnitude/power afterward (see that module's own doc comment), so
//! its sign convention was deliberately left unpinned; this one needs
//! genuinely phase-correct output (`envelope::sample_filter_phase`
//! reads `Aw[b].conj()` directly, and `synthesis.rs`'s inverse FFT
//! needs a real, correctly-scaled time-domain result), so the
//! convention is pinned and verified directly against `rustfft`'s own
//! complex output, not just a power spectrum.
//!
//! Sign convention, verified by this module's own tests: `forward ==
//! true` matches `rustfft`'s `plan_fft_forward` (the standard DFT,
//! `X[k] = sum_n x[n] e^{-i 2 pi k n / N}`); `forward == false` matches
//! `plan_fft_inverse`, unnormalized (no `1/N` divide), exactly matching
//! `rustfft`'s own inverse convention (and `synthesis.rs`'s existing
//! float `ifft.process()` call, which relies on that same lack of
//! normalization: its per-bin amplitudes are already scaled for it).

use super::FFT_ENC;

const FRAC_BITS: u32 = 23;

fn f32_to_q23(x: f32) -> i64 {
    (x as f64 * (1i64 << FRAC_BITS) as f64).round() as i64
}

/// Genuinely fixed-point (Q23, `i64`) complex value -- `envelope.rs`/
/// `synthesis.rs`'s own fixed twins need complex multiply/conjugate
/// (the LPC spectrum's phase, and the synthesis filter's phase
/// response derived from it), which `fft_fixed`'s own bare `re`/`im`
/// arrays don't provide on their own.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ComplexQ23 {
    pub(crate) re: i64,
    pub(crate) im: i64,
}

impl ComplexQ23 {
    pub(crate) const ZERO: ComplexQ23 = ComplexQ23 { re: 0, im: 0 };

    pub(crate) fn conj(self) -> ComplexQ23 {
        ComplexQ23 { re: self.re, im: -self.im }
    }

    /// Complex multiply, `i128`-widened then rescaled back to Q23 --
    /// same accumulate-then-narrow pattern this port uses throughout.
    pub(crate) fn mul(self, other: ComplexQ23) -> ComplexQ23 {
        let re = rshift_round_i128(
            self.re as i128 * other.re as i128 - self.im as i128 * other.im as i128,
            FRAC_BITS,
        );
        let im = rshift_round_i128(
            self.re as i128 * other.im as i128 + self.im as i128 * other.re as i128,
            FRAC_BITS,
        );
        ComplexQ23 { re, im }
    }

    /// Multiplies by a plain (non-Q23) integer scalar -- stays Q23 with
    /// no rescale, the same "plain integer times a Q_n value stays
    /// Q_n" reasoning `lpc.rs`'s own `build_half_poly_q23` uses for its
    /// `-2 * cos_lsp_q23[i]`.
    pub(crate) fn scale_int(self, k: i64) -> ComplexQ23 {
        ComplexQ23 { re: self.re * k, im: self.im * k }
    }
}

/// Round-to-nearest right shift for an `i128` accumulator, narrowing to
/// `i64` -- same pattern `nlp.rs`'s own `rshift_round_i128` establishes,
/// with the same `debug_assert` documenting that this module's own
/// butterfly products are bounded well under `i64::MAX` by construction
/// (real LPC-coefficient-derived spectra never reach the extreme
/// full-scale-`i16`-amplitude magnitudes `nlp.rs`'s own power spectrum
/// can).
pub(crate) fn rshift_round_i128(x: i128, n: u32) -> i64 {
    let shifted = (x + (1i128 << (n - 1))) >> n;
    debug_assert!(
        shifted >= i64::MIN as i128 && shifted <= i64::MAX as i128,
        "rshift_round_i128: result {shifted} doesn't fit i64"
    );
    shifted as i64
}

/// Twiddles for the FORWARD convention (`e^{-i*2*pi*k/N}`) -- the
/// inverse convention's own `e^{+i*theta}` is just this table's
/// imaginary part negated (a conjugate), computed in the butterfly
/// itself rather than as a second table.
fn fft_twiddles_q23() -> &'static [(i64, i64); FFT_ENC / 2] {
    static TABLE: std::sync::OnceLock<[(i64, i64); FFT_ENC / 2]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|k| {
            let theta = -std::f32::consts::TAU * k as f32 / FFT_ENC as f32;
            (f32_to_q23(theta.cos()), f32_to_q23(theta.sin()))
        })
    })
}

fn fft_bit_reverse_table() -> &'static [usize; FFT_ENC] {
    static TABLE: std::sync::OnceLock<[usize; FFT_ENC]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let bits = FFT_ENC.trailing_zeros();
        std::array::from_fn(|i| ((i as u32).reverse_bits() >> (32 - bits)) as usize)
    })
}

/// In-place radix-2 decimation-in-time FFT, `FFT_ENC`-point, Q23
/// fixed-point throughout (no `f32` inside the transform itself -- only
/// the one-time twiddle-table construction above uses float, the same
/// "table construction isn't the hot path" convention this port uses
/// elsewhere). No per-stage rescaling: `i64`/`i128` headroom vastly
/// exceeds this transform's real dynamic range (LPC-spectrum and
/// sparse-harmonic-spectrum inputs, not full-scale noise), the same
/// reasoning `nlp.rs`'s own `fft_fixed` documents for its own,
/// differently-scaled input.
pub(crate) fn fft_fixed(re: &mut [i64; FFT_ENC], im: &mut [i64; FFT_ENC], forward: bool) {
    let bitrev = fft_bit_reverse_table();
    for (i, &j) in bitrev.iter().enumerate() {
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let twiddles = fft_twiddles_q23();
    let mut len = 2usize;
    while len <= FFT_ENC {
        let half = len / 2;
        let step = FFT_ENC / len;
        let mut i = 0;
        while i < FFT_ENC {
            for j in 0..half {
                let (wr, wi_fwd) = twiddles[j * step];
                let wi = if forward { wi_fwd } else { -wi_fwd };
                let br = re[i + j + half];
                let bi = im[i + j + half];
                let vr = rshift_round_i128(
                    wr as i128 * br as i128 - wi as i128 * bi as i128,
                    FRAC_BITS,
                );
                let vi = rshift_round_i128(
                    wr as i128 * bi as i128 + wi as i128 * br as i128,
                    FRAC_BITS,
                );
                let ar = re[i + j];
                let ai = im[i + j];
                re[i + j] = ar + vr;
                im[i + j] = ai + vi;
                re[i + j + half] = ar - vr;
                im[i + j + half] = ai - vi;
            }
            i += len;
        }
        len *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::num_complex::Complex32;
    use rustfft::FftPlanner;

    /// Real, checkable multi-tone input (clean bin frequencies, plus a
    /// touch of noise so it isn't a degenerate single-tone case) --
    /// same fixture-construction idea `nlp.rs`'s own FFT test uses.
    fn multi_tone_input() -> ([f32; FFT_ENC], [i64; FFT_ENC]) {
        let tones: &[(usize, f32)] = &[(5, 1.0), (30, 0.6), (120, 0.3), (200, 0.15)];
        let mut seed = 777u32;
        let mut input_f = [0.0f32; FFT_ENC];
        let mut re_q = [0i64; FFT_ENC];
        for i in 0..FFT_ENC {
            let mut v = 0.0f32;
            for &(bin, amp) in tones {
                v += amp * (std::f32::consts::TAU * bin as f32 * i as f32 / FFT_ENC as f32).cos();
            }
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            v += 0.01 * ((seed >> 16) as i16 as f32 / 32768.0);
            input_f[i] = v;
            re_q[i] = f32_to_q23(v);
        }
        (input_f, re_q)
    }

    #[test]
    fn forward_fft_fixed_matches_rustfft_plan_fft_forward_on_complex_output_directly() {
        // Direct complex-value comparison (re AND im separately), not
        // just power -- this is the whole point of this module existing
        // separately from nlp.rs's own phase-agnostic fft_fixed.
        let (input_f, re_q) = multi_tone_input();
        let mut re = re_q;
        let mut im = [0i64; FFT_ENC];
        fft_fixed(&mut re, &mut im, true);

        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(FFT_ENC);
        let mut buf: Vec<Complex32> = input_f.iter().map(|&x| Complex32::new(x, 0.0)).collect();
        fft.process(&mut buf);

        let mut max_abs_err = 0.0f32;
        let mut max_ref_mag = 0.0f32;
        for i in 0..FFT_ENC {
            let got_re = re[i] as f32 / (1i64 << FRAC_BITS) as f32;
            let got_im = im[i] as f32 / (1i64 << FRAC_BITS) as f32;
            max_abs_err = max_abs_err.max((got_re - buf[i].re).abs()).max((got_im - buf[i].im).abs());
            max_ref_mag = max_ref_mag.max(buf[i].re.abs()).max(buf[i].im.abs());
        }
        assert!(max_ref_mag > 1.0, "sanity: reference spectrum shouldn't be near-zero");
        assert!(
            max_abs_err / max_ref_mag < 1e-4,
            "forward fft_fixed diverged from rustfft's plan_fft_forward: max_abs_err={max_abs_err}, max_ref_mag={max_ref_mag}"
        );
    }

    #[test]
    fn inverse_fft_fixed_matches_rustfft_plan_fft_inverse_on_complex_output_directly() {
        // Feed a real spectrum (the forward transform's own output) into
        // both inverse paths -- both should reconstruct (an unnormalized,
        // N-scaled version of) the original real-valued time-domain input.
        let (input_f, re_q) = multi_tone_input();
        let mut re = re_q;
        let mut im = [0i64; FFT_ENC];
        fft_fixed(&mut re, &mut im, true);

        let mut float_buf: Vec<Complex32> = (0..FFT_ENC)
            .map(|i| Complex32::new(re[i] as f32 / (1i64 << FRAC_BITS) as f32, im[i] as f32 / (1i64 << FRAC_BITS) as f32))
            .collect();

        fft_fixed(&mut re, &mut im, false);

        let mut planner = FftPlanner::<f32>::new();
        let ifft = planner.plan_fft_inverse(FFT_ENC);
        ifft.process(&mut float_buf);

        let mut max_abs_err = 0.0f32;
        let mut max_ref_mag = 0.0f32;
        for i in 0..FFT_ENC {
            let got_re = re[i] as f32 / (1i64 << FRAC_BITS) as f32;
            let got_im = im[i] as f32 / (1i64 << FRAC_BITS) as f32;
            max_abs_err = max_abs_err.max((got_re - float_buf[i].re).abs()).max((got_im - float_buf[i].im).abs());
            max_ref_mag = max_ref_mag.max(float_buf[i].re.abs()).max(float_buf[i].im.abs());
        }
        assert!(max_ref_mag > 1.0, "sanity: reconstructed signal shouldn't be near-zero");
        assert!(
            max_abs_err / max_ref_mag < 1e-4,
            "inverse fft_fixed diverged from rustfft's plan_fft_inverse: max_abs_err={max_abs_err}, max_ref_mag={max_ref_mag}"
        );

        // Also confirm the round trip actually reconstructs the
        // ORIGINAL real input, scaled by FFT_ENC (rustfft's own
        // unnormalized convention) -- catches a convention bug (e.g.
        // forward/inverse accidentally swapped) that could still pass
        // the direct rustfft comparison above if both sides made the
        // same mistake.
        let mut max_recon_err = 0.0f32;
        for i in 0..FFT_ENC {
            let got_re = re[i] as f32 / (1i64 << FRAC_BITS) as f32;
            max_recon_err = max_recon_err.max((got_re / FFT_ENC as f32 - input_f[i]).abs());
        }
        assert!(
            max_recon_err < 1e-3,
            "forward-then-inverse round trip (rescaled by 1/FFT_ENC) diverged from the original input by {max_recon_err}"
        );
    }
}
