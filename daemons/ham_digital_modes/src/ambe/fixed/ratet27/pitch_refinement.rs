// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point quarter-sample pitch refinement (TIA-102.BABA_2003.pdf section 5.1.5): the 256-point
//! windowed DFT `S_w(m)`, the harmonic amplitude `A_l` (Eq. 26-28), the synthetic spectrum (Eq. 25), the
//! refinement error `E_R` (Eq. 24) and [`refine_pitch`]. Fixed-point sibling of
//! [`crate::ambe::float::ratet27::pitch_refinement`].
//!
//! # Pitch representation: `P8`, the pitch period in eighths of a sample
//!
//! Every pitch the refinement examines is `p_hat_i +- k/8` with `p_hat_i` a half-sample candidate, so
//! the period is an exact integer `P8 = 8 * P`. With `omega0 = 2*pi/P` every place the float code
//! multiplies by `omega0` and by `256/(2*pi)` or `16384/(2*pi)` cancels the `pi`:
//!
//! * band edges `a_l = 256 (l - 1/2) / P = 1024 (2l - 1) / P8` (and `b_l = a_{l+1}`),
//! * window-DFT index `floor(64 m - 16384 l / P + 1/2) = floor((128 m P8 - 262144 l + P8) / (2 P8))`,
//! * `L_est = floor(0.9254 P / 2 - 1/2)`.
//!
//! All are evaluated with exact integer division, so no fixed-point `pi` or reciprocal is needed and
//! the band/bin bookkeeping cannot drift from the real-number definition. [`omega0_q30_from_p8`]
//! converts to radians per sample (Q30) for the consumers that need `omega0` itself.
//!
//! # Numeric formats
//!
//! * `S_w(m)`: complex, each part an `i64` in Q10 (a 16-bit input times the window sum 110 is below
//!   `2^23`, so Q10 is below `2^33`). Computed from `raw * w_R` (Q10) times a Q30 cosine table, exact
//!   to rounding of each partial product; `m < 0` uses conjugate symmetry of a real input.
//! * `W_R(m)`: Q22 table for `|m| <= 512` (indices beyond that are treated as 0; the reachable
//!   range for pitches of at least 19.875 is about 476).
//! * Harmonic amplitude `A_l`: Q16 complex `i64` (`num / den` with `num = sum S_w W_R` in Q32 and
//!   `den = sum W_R^2` in Q44, both `i128`).
//! * Error/energy sums: `i128`, Q60 (`|diff|^2` of Q30 differences).
//!
//! # Verification (see `tests/ambe_fixed_ratet27_pitch_refinement.rs`)
//!
//! On real speech (all 7748 frames of the four `osr_speech` files, the same initial pitch handed to
//! both implementations) the refined period `P8` equals the float sibling's refined period, to the
//! eighth of a sample, on 99.99% of frames (one frame differs, by one eighth), and `E_R` at the chosen
//! period agrees to a worst-case relative error of 7.0e-5.

use super::pitch::p2_of_index;
use super::pitch_refinement_tables::{
    COS_256_Q30, REFINEMENT_WINDOW_Q30, WINDOW_DFT_16384_Q22, WINDOW_DFT_HALF_RANGE,
};

/// `round(2 * pi * 2^30)`.
pub const TWO_PI_Q30: i64 = 6_746_518_852;

/// A complex number with `i64` parts; the scale is stated by whoever holds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cplx {
    pub re: i64,
    pub im: i64,
}

impl Cplx {
    pub const ZERO: Cplx = Cplx { re: 0, im: 0 };

    /// `re^2 + im^2` (no rescaling; the caller tracks the scale).
    pub fn norm_sqr(self) -> i128 {
        self.re as i128 * self.re as i128 + self.im as i128 * self.im as i128
    }
}

/// `omega0 = 2*pi / (P8 / 8)` in radians per sample, Q30.
pub fn omega0_q30_from_p8(p8: u32) -> i32 {
    let num = TWO_PI_Q30 * 8;
    ((num + p8 as i64 / 2) / p8 as i64) as i32
}

/// `P8` of a half-sample pitch candidate index.
pub fn p8_of_index(idx: usize) -> u32 {
    (4 * p2_of_index(idx)) as u32
}

/// The 256-point windowed DFT of one frame: `S_w(m)` for `m = -127..=128`, Q10.
pub struct RefinementFrame {
    sw: [Cplx; 256],
}

impl RefinementFrame {
    /// `raw` is PCM; the window is centred on `raw[center]` and reads `center-110..=center+110`.
    pub fn new(raw: &[i32], center: usize) -> Self {
        let mut xw = [0i64; 221];
        for (i, slot) in xw.iter_mut().enumerate() {
            let n = i as i32 - 110;
            let w = REFINEMENT_WINDOW_Q30[n.unsigned_abs() as usize] as i64;
            *slot = (raw[(center as i32 + n) as usize] as i64 * w + (1 << 19)) >> 20;
        }
        let mut sw = [Cplx::ZERO; 256];
        for m in 0..=128i32 {
            let mut re = 0i64;
            let mut im = 0i64;
            for (i, &x) in xw.iter().enumerate() {
                let n = i as i32 - 110;
                let k = (m * n).rem_euclid(256) as usize;
                re += x * COS_256_Q30[k] as i64;
                im -= x * COS_256_Q30[(k + 192) & 255] as i64;
            }
            let bin = Cplx { re: (re + (1 << 29)) >> 30, im: (im + (1 << 29)) >> 30 };
            sw[(m + 127) as usize] = bin;
            if (1..=127).contains(&m) {
                sw[(-m + 127) as usize] = Cplx { re: bin.re, im: -bin.im };
            }
        }
        Self { sw }
    }

    /// `S_w(m)` (Q10), zero outside `-127..=128`.
    pub fn sw_at(&self, m: i32) -> Cplx {
        if (-127..=128).contains(&m) {
            self.sw[(m + 127) as usize]
        } else {
            Cplx::ZERO
        }
    }
}

/// `W_R(m)` (Q22), zero outside the table.
pub fn window_dft_16384_q22(m: i32) -> i64 {
    if m.abs() <= WINDOW_DFT_HALF_RANGE {
        WINDOW_DFT_16384_Q22[(m + WINDOW_DFT_HALF_RANGE) as usize] as i64
    } else {
        0
    }
}

fn div_ceil_i64(a: i64, b: i64) -> i64 {
    -((-a).div_euclid(b))
}

/// `ceil(a_l) = ceil(1024 (2l - 1) / P8)`, which is also the (exclusive) upper bin of band `l - 1`.
pub fn band_start(l: i32, p8: u32) -> i32 {
    div_ceil_i64(1024 * (2 * l as i64 - 1), p8 as i64) as i32
}

/// Index into `W_R` for bin `m` of harmonic `l`: `floor(64 m - 16384 l / P + 1/2)`.
pub fn window_index(m: i32, l: i32, p8: u32) -> i32 {
    let p = p8 as i64;
    (128 * m as i64 * p - 262144 * l as i64 + p).div_euclid(2 * p) as i32
}

/// `A_l` (Eq. 26-28) as a Q16 complex, from the bins `[ceil(a_l), ceil(b_l))`.
pub fn harmonic_amplitude_q16(frame: &RefinementFrame, l: u32, p8: u32) -> Cplx {
    let (lo, hi) = (band_start(l as i32, p8), band_start(l as i32 + 1, p8));
    let mut num_re = 0i128;
    let mut num_im = 0i128;
    let mut den = 0i128;
    for m in lo..hi {
        let wr = window_dft_16384_q22(window_index(m, l as i32, p8)) as i128;
        let s = frame.sw_at(m);
        num_re += s.re as i128 * wr;
        num_im += s.im as i128 * wr;
        den += wr * wr;
    }
    if den == 0 {
        return Cplx::ZERO;
    }
    Cplx { re: ((num_re << 28) / den) as i64, im: ((num_im << 28) / den) as i64 }
}

/// Over the bins `m_start <= m < m_end`: returns `(err, real)` with `real = sum |S_w(m)|^2` and
/// `err = sum |S_w(m) - S_synth(m)|^2`, where the synthetic spectrum is built from harmonics
/// `0..=max_l` at period `p8` (Eq. 25; a bin in no harmonic's band has synthetic value 0). Both are
/// Q60 `i128`.
pub fn spectrum_error_and_energy(
    frame: &RefinementFrame,
    p8: u32,
    max_l: u32,
    m_start: i32,
    m_end: i32,
) -> (i128, i128) {
    let to_q30 = |c: Cplx| Cplx { re: c.re << 20, im: c.im << 20 };
    let mut real = 0i128;
    for m in m_start..m_end {
        real += to_q30(frame.sw_at(m)).norm_sqr();
    }
    let mut err = real;
    for l in 0..=max_l as i32 {
        let (lo, hi) = (band_start(l, p8), band_start(l + 1, p8));
        let (s, e) = (lo.max(m_start), hi.min(m_end));
        if s >= e {
            continue;
        }
        let amp = harmonic_amplitude_q16(frame, l as u32, p8);
        for m in s..e {
            let wr = window_dft_16384_q22(window_index(m, l, p8));
            let syn = Cplx { re: (amp.re * wr) >> 8, im: (amp.im * wr) >> 8 }; // Q16 * Q22 = Q38 -> Q30
            let real_bin = to_q30(frame.sw_at(m));
            let diff = Cplx { re: real_bin.re - syn.re, im: real_bin.im - syn.im };
            err += diff.norm_sqr() - real_bin.norm_sqr();
        }
    }
    (err, real)
}

/// `L_est = floor(0.9254 * pi / omega0 - 1/2) = floor((4627 P8 - 40000) / 80000)` (Eq. 24's limit).
fn l_estimate(p8: u32) -> i64 {
    (4627 * p8 as i64 - 40000).div_euclid(80000)
}

/// The refinement error `E_R` (Eq. 24) at period `p8`, Q60.
pub fn refinement_error(frame: &RefinementFrame, p8: u32) -> i128 {
    let l_est = l_estimate(p8);
    let upper_m = (l_est * 2048).div_euclid(p8 as i64) as i32;
    let max_l = l_est.max(0) as u32 + 1;
    spectrum_error_and_energy(frame, p8, max_l, 50, upper_m + 1).0
}

/// Refines the half-sample candidate `p_hat_i_index` (see [`super::pitch`]) to quarter-sample
/// accuracy: tries `P = p_hat_i + {-9,-7,...,+9}/8` and returns the `P8` with the smallest `E_R`
/// (first of equals, as the float sibling's `min_by`).
pub fn refine_pitch(frame: &RefinementFrame, p_hat_i_index: usize) -> u32 {
    let base = p8_of_index(p_hat_i_index) as i32;
    let mut best_p8 = 0u32;
    let mut best_err = i128::MAX;
    for off in [-9, -7, -5, -3, -1, 1, 3, 5, 7, 9] {
        let p8 = (base + off) as u32;
        let e = refinement_error(frame, p8);
        if e < best_err {
            best_err = e;
            best_p8 = p8;
        }
    }
    best_p8
}
