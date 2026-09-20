// SPDX-License-Identifier: LGPL-3.0-or-later
//! Genuinely fixed-point, phase-correct radix-2 FFT, generic over its size
//! (`Size<N>`: `Rate8k` = 512 points for the decoder's `envelope.rs`
//! forward analysis (`ak[]` -> `Aw[]`) and `synthesis.rs`'s inverse synthesis
//! of a sparse harmonic spectrum, `Rate16k` = 1024 points for
//! `spectral_bridge.rs`'s doubled-resolution inverse synthesis) plus the
//! pitch estimator's 512-point transform of its 64 windowed samples. Each
//! size and use is its own monomorphised, constant-table instance; there is
//! no run-time size dispatch. See `docs/CODEC2_NO_STD.md` for the measured
//! effect.
//!
//! `nlp.rs`'s pitch estimator shares the transform machinery but keeps its
//! own twiddle table (it differs from this module's in the last bit at some
//! entries) and its inverse sign convention: it only reads magnitude
//! afterward, whereas the decoder's uses need genuinely phase-correct output
//! (`envelope::sample_filter_phase` reads `Aw[b].conj()` directly, and
//! `synthesis.rs`'s inverse FFT needs a real, correctly-scaled time-domain
//! result), so their convention is pinned and verified directly against
//! `rustfft`'s own complex output, not just a power spectrum.
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

#[cfg(test)]
fn f32_to_q23(x: f32) -> i64 {
    (x as f64 * (1i64 << FRAC_BITS) as f64).round() as i64
}

#[cfg(test)]
fn f64_to_q23(x: f64) -> i64 {
    (x * (1i64 << FRAC_BITS) as f64).round() as i64
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
        ComplexQ23 {
            re: self.re,
            im: -self.im,
        }
    }

    /// Complex multiply, `i128`-widened then rescaled back to Q23 --
    /// same accumulate-then-narrow pattern this port uses throughout.
    // [@ANCHOR: ComplexQ23::mul]
    pub(crate) fn mul(self, other: ComplexQ23) -> ComplexQ23 {
        // Fast path: when every component is under 2^30 in magnitude the
        // products and their sums stay under 2^61, so plain `i64`
        // arithmetic (a handful of instructions on a 32-bit core, versus
        // a software 128-bit multiply) gives the identical result. Larger
        // values (rare: extreme spectral peaks) take the exact `i128` path.
        const LIM: u64 = 1 << 30;
        if (self.re.unsigned_abs() | self.im.unsigned_abs() | other.re.unsigned_abs()
            | other.im.unsigned_abs())
            < LIM
        {
            let re = rshift_round_i64(self.re * other.re - self.im * other.im, FRAC_BITS);
            let im = rshift_round_i64(self.re * other.im + self.im * other.re, FRAC_BITS);
            return ComplexQ23 { re, im };
        }
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

    /// Squared magnitude `re^2 + im^2` in Q46 (the raw, unshifted sum), as
    /// `i128`-exact but computed with `u64` arithmetic whenever both
    /// components are under 2^31 in magnitude (each square is then under
    /// 2^62 and the sum under 2^63).
    pub(crate) fn mag_sq_raw(self) -> i128 {
        if (self.re.unsigned_abs() | self.im.unsigned_abs()) < (1u64 << 31) {
            let (r, i) = (self.re.unsigned_abs(), self.im.unsigned_abs());
            (r * r + i * i) as i128
        } else {
            self.re as i128 * self.re as i128 + self.im as i128 * self.im as i128
        }
    }
}

/// `(x + 2^(n-1)) >> n` for an `i64` value the caller has bounded away
/// from overflow (|x| < 2^62).
#[inline(always)]
pub(crate) fn rshift_round_i64(x: i64, n: u32) -> i64 {
    (x + (1i64 << (n - 1))) >> n
}

/// One radix-2 butterfly multiply, `(wr + j wi) * (br + j bi)` rounded
/// back to Q23. `|wr|, |wi| <= 2^23` (twiddle factors). When both
/// operands are under 2^38 in magnitude the whole thing fits `i64`
/// (|wr br - wi bi| < 2^23 * 2^39 = 2^62), otherwise the exact `i128`
/// path runs. Bit-identical either way. (Measured 2026-09-20: real speech
/// and adversarial encoder input stay under 2^63 here, but decoding
/// pseudo-random garbage bitstreams reaches 2^67 in the butterfly, which is
/// why the exact `i128` path is kept rather than assuming `i64` suffices.)
#[inline(always)]
fn twiddle_mul(wr: i64, wi: i64, br: i64, bi: i64) -> (i64, i64) {
    if (br.unsigned_abs() | bi.unsigned_abs()) < (1u64 << 38) {
        (
            rshift_round_i64(wr * br - wi * bi, FRAC_BITS),
            rshift_round_i64(wr * bi + wi * br, FRAC_BITS),
        )
    } else {
        (
            rshift_round_i128(wr as i128 * br as i128 - wi as i128 * bi as i128, FRAC_BITS),
            rshift_round_i128(wr as i128 * bi as i128 + wi as i128 * br as i128, FRAC_BITS),
        )
    }
}

/// Round-to-nearest right shift for an `i128` accumulator, narrowing to
/// `i64` -- same pattern `nlp.rs`'s own `rshift_round_i128` establishes,
/// with the same `debug_assert` documenting that this module's own
/// butterfly products are bounded well under `i64::MAX` by construction
/// (real LPC-coefficient-derived spectra never reach the extreme
/// full-scale-`i16`-amplitude magnitudes `nlp.rs`'s own power spectrum
/// can).
// [@ANCHOR: fixed_fft:rshift_round_i128]
pub(crate) fn rshift_round_i128(x: i128, n: u32) -> i64 {
    let shifted = (x + (1i128 << (n - 1))) >> n;
    debug_assert!(
        shifted >= i64::MIN as i128 && shifted <= i64::MAX as i128,
        "rshift_round_i128: result {shifted} doesn't fit i64"
    );
    shifted as i64
}

#[cfg(feature = "codec2_16k_bridge")]
/// `spectral_bridge.rs`'s own doubled-resolution FFT size -- imported
/// here (rather than re-derived as `2*FFT_ENC`) so there is exactly one
/// definition of it, matching `spectral_bridge.rs`'s own `pub const
/// FFT_ENC_SB`.
use super::spectral_bridge::FFT_ENC_SB;

/// Real formula behind [`TWIDDLES_512_Q23`]/[`TWIDDLES_1024_Q23`] below --
/// kept live (not deleted after generating those tables) specifically so
/// `twiddle_tables_match_their_own_generating_formula` can regenerate and
/// compare against the checked-in constants on every test run, the same
/// "generated once from the real function's own output, never trusted as
/// a static fact" discipline `lpc.rs`'s own `BW_GAMMA_Q23` established
/// (that table shipped with a real, caught bug from an independently
/// hand-typed value before this project adopted this pattern). This
/// function itself is *not* itself part of the fixed-point production
/// path -- see the module doc comment's own no-heap-allocation rationale
/// for why the two tables below exist as `const` data rather than a
/// lazily-built `Vec` computed by calling this at runtime, which is what
/// this function replaced.
///
/// **Deliberately `f64`, not `f32`, for `theta`/`cos`/`sin`** -- found
/// 2026-09-15 on this project's first-ever Windows/MinGW test run: an
/// earlier `f32`-based version of this function passed on Linux but
/// failed `twiddle_tables_match_their_own_generating_formula` on
/// `x86_64-pc-windows-gnu`, off by exactly 1 in the low bit of the Q23
/// fixed-point value at 4 of the 256 unique `TWIDDLES_512_Q23` entries
/// (confirmed a genuine cross-platform libm rounding difference in last-
/// bit `f32::cos`/`sin` output, not real drift or hand-edit corruption in
/// the checked-in table: the source file was verified byte-identical
/// between the two machines, and the same source passed cleanly on Linux
/// immediately before and after the Windows run). An `f32` mantissa (24
/// bits) sits barely above the Q23 fixed-point boundary (23 fractional
/// bits), so a single differing rounding decision in a platform's `cos`/
/// `sin` implementation is enough to flip the quantized integer -- the
/// formula genuinely had no single well-defined answer across platforms
/// at that precision. `f64` (53-bit mantissa) puts roughly 30 bits of
/// headroom above the same Q23 boundary, so a last-bit `cos`/`sin`
/// difference between platforms' libm implementations no longer has
/// enough weight to change the rounded result -- the checked-in tables
/// below were regenerated from this `f64` version and are now expected to
/// match bit-for-bit on every platform, not just the one that happened to
/// generate them. This is a fix to this test-only generator, not to any
/// production fixed-point code -- `TWIDDLES_512_Q23`/`TWIDDLES_1024_Q23`
/// are baked-in `const` data either way, so the actual decode/encode path
/// was already 100% platform-independent; only this self-check's own
/// regeneration was non-deterministic across platforms.
#[cfg(test)]
fn build_twiddles_q23(n: usize) -> Vec<(i64, i64)> {
    (0..n / 2)
        .map(|k| {
            let theta = -std::f64::consts::TAU * k as f64 / n as f64;
            (f64_to_q23(theta.cos()), f64_to_q23(theta.sin()))
        })
        .collect()
}

/// Twiddle factors for `FFT_ENC`=512, Q23, generated once via
/// [`build_twiddles_q23`]`(512)` and checked in as `const` data --
/// no runtime float trig, no heap allocation, matching the real C
/// reference's own "fully static memory allocation... no malloc
/// anywhere" design goal (`CODEC2_MOD_FIXED_POINT_PLAN.md`'s own
/// quoted characterization of Codec2-mod), which this table's earlier
/// `Vec`+`OnceLock` form did not actually meet despite living in the
/// otherwise-genuinely-fixed-point decode path. Regenerated and
/// diffed against the real formula by
/// `twiddle_tables_match_their_own_generating_formula` below, not
/// hand-typed.
const TWIDDLES_512_Q23: [(i64, i64); 256] = [
    (8388608, 0),
    (8387976, -102941),
    (8386082, -205867),
    (8382924, -308761),
    (8378504, -411609),
    (8372822, -514396),
    (8365879, -617104),
    (8357676, -719720),
    (8348215, -822227),
    (8337496, -924611),
    (8325522, -1026855),
    (8312294, -1128945),
    (8297814, -1230864),
    (8282085, -1332599),
    (8265108, -1434132),
    (8246887, -1535450),
    (8227423, -1636536),
    (8206721, -1737376),
    (8184783, -1837954),
    (8161612, -1938256),
    (8137212, -2038265),
    (8111587, -2137968),
    (8084740, -2237349),
    (8056675, -2336392),
    (8027397, -2435084),
    (7996911, -2533410),
    (7965220, -2631353),
    (7932329, -2728901),
    (7898244, -2826037),
    (7862970, -2922748),
    (7826511, -3019018),
    (7788874, -3114834),
    (7750063, -3210181),
    (7710086, -3305045),
    (7668947, -3399411),
    (7626654, -3493264),
    (7583212, -3586592),
    (7538628, -3679380),
    (7492909, -3771613),
    (7446061, -3863279),
    (7398092, -3954362),
    (7349009, -4044851),
    (7298819, -4134730),
    (7247530, -4223986),
    (7195149, -4312606),
    (7141685, -4400577),
    (7087145, -4487885),
    (7031538, -4574518),
    (6974873, -4660461),
    (6917156, -4745702),
    (6858399, -4830229),
    (6798608, -4914029),
    (6737793, -4997088),
    (6675964, -5079395),
    (6613129, -5160937),
    (6549299, -5241701),
    (6484482, -5321677),
    (6418688, -5400850),
    (6351928, -5479211),
    (6284212, -5556746),
    (6215549, -5633445),
    (6145949, -5709295),
    (6075425, -5784285),
    (6003985, -5858405),
    (5931642, -5931642),
    (5858405, -6003985),
    (5784285, -6075425),
    (5709295, -6145949),
    (5633445, -6215549),
    (5556746, -6284212),
    (5479211, -6351928),
    (5400850, -6418688),
    (5321677, -6484482),
    (5241701, -6549299),
    (5160937, -6613129),
    (5079395, -6675964),
    (4997088, -6737793),
    (4914029, -6798608),
    (4830229, -6858399),
    (4745702, -6917156),
    (4660461, -6974873),
    (4574518, -7031538),
    (4487885, -7087145),
    (4400577, -7141685),
    (4312606, -7195149),
    (4223986, -7247530),
    (4134730, -7298819),
    (4044851, -7349009),
    (3954362, -7398092),
    (3863279, -7446061),
    (3771613, -7492909),
    (3679380, -7538628),
    (3586592, -7583212),
    (3493264, -7626654),
    (3399411, -7668947),
    (3305045, -7710086),
    (3210181, -7750063),
    (3114834, -7788874),
    (3019018, -7826511),
    (2922748, -7862970),
    (2826037, -7898244),
    (2728901, -7932329),
    (2631353, -7965220),
    (2533410, -7996911),
    (2435084, -8027397),
    (2336392, -8056675),
    (2237349, -8084740),
    (2137968, -8111587),
    (2038265, -8137212),
    (1938256, -8161612),
    (1837954, -8184783),
    (1737376, -8206721),
    (1636536, -8227423),
    (1535450, -8246887),
    (1434132, -8265108),
    (1332599, -8282085),
    (1230864, -8297814),
    (1128945, -8312294),
    (1026855, -8325522),
    (924611, -8337496),
    (822227, -8348215),
    (719720, -8357676),
    (617104, -8365879),
    (514396, -8372822),
    (411609, -8378504),
    (308761, -8382924),
    (205867, -8386082),
    (102941, -8387976),
    (0, -8388608),
    (-102941, -8387976),
    (-205867, -8386082),
    (-308761, -8382924),
    (-411609, -8378504),
    (-514396, -8372822),
    (-617104, -8365879),
    (-719720, -8357676),
    (-822227, -8348215),
    (-924611, -8337496),
    (-1026855, -8325522),
    (-1128945, -8312294),
    (-1230864, -8297814),
    (-1332599, -8282085),
    (-1434132, -8265108),
    (-1535450, -8246887),
    (-1636536, -8227423),
    (-1737376, -8206721),
    (-1837954, -8184783),
    (-1938256, -8161612),
    (-2038265, -8137212),
    (-2137968, -8111587),
    (-2237349, -8084740),
    (-2336392, -8056675),
    (-2435084, -8027397),
    (-2533410, -7996911),
    (-2631353, -7965220),
    (-2728901, -7932329),
    (-2826037, -7898244),
    (-2922748, -7862970),
    (-3019018, -7826511),
    (-3114834, -7788874),
    (-3210181, -7750063),
    (-3305045, -7710086),
    (-3399411, -7668947),
    (-3493264, -7626654),
    (-3586592, -7583212),
    (-3679380, -7538628),
    (-3771613, -7492909),
    (-3863279, -7446061),
    (-3954362, -7398092),
    (-4044851, -7349009),
    (-4134730, -7298819),
    (-4223986, -7247530),
    (-4312606, -7195149),
    (-4400577, -7141685),
    (-4487885, -7087145),
    (-4574518, -7031538),
    (-4660461, -6974873),
    (-4745702, -6917156),
    (-4830229, -6858399),
    (-4914029, -6798608),
    (-4997088, -6737793),
    (-5079395, -6675964),
    (-5160937, -6613129),
    (-5241701, -6549299),
    (-5321677, -6484482),
    (-5400850, -6418688),
    (-5479211, -6351928),
    (-5556746, -6284212),
    (-5633445, -6215549),
    (-5709295, -6145949),
    (-5784285, -6075425),
    (-5858405, -6003985),
    (-5931642, -5931642),
    (-6003985, -5858405),
    (-6075425, -5784285),
    (-6145949, -5709295),
    (-6215549, -5633445),
    (-6284212, -5556746),
    (-6351928, -5479211),
    (-6418688, -5400850),
    (-6484482, -5321677),
    (-6549299, -5241701),
    (-6613129, -5160937),
    (-6675964, -5079395),
    (-6737793, -4997088),
    (-6798608, -4914029),
    (-6858399, -4830229),
    (-6917156, -4745702),
    (-6974873, -4660461),
    (-7031538, -4574518),
    (-7087145, -4487885),
    (-7141685, -4400577),
    (-7195149, -4312606),
    (-7247530, -4223986),
    (-7298819, -4134730),
    (-7349009, -4044851),
    (-7398092, -3954362),
    (-7446061, -3863279),
    (-7492909, -3771613),
    (-7538628, -3679380),
    (-7583212, -3586592),
    (-7626654, -3493264),
    (-7668947, -3399411),
    (-7710086, -3305045),
    (-7750063, -3210181),
    (-7788874, -3114834),
    (-7826511, -3019018),
    (-7862970, -2922748),
    (-7898244, -2826037),
    (-7932329, -2728901),
    (-7965220, -2631353),
    (-7996911, -2533410),
    (-8027397, -2435084),
    (-8056675, -2336392),
    (-8084740, -2237349),
    (-8111587, -2137968),
    (-8137212, -2038265),
    (-8161612, -1938256),
    (-8184783, -1837954),
    (-8206721, -1737376),
    (-8227423, -1636536),
    (-8246887, -1535450),
    (-8265108, -1434132),
    (-8282085, -1332599),
    (-8297814, -1230864),
    (-8312294, -1128945),
    (-8325522, -1026855),
    (-8337496, -924611),
    (-8348215, -822227),
    (-8357676, -719720),
    (-8365879, -617104),
    (-8372822, -514396),
    (-8378504, -411609),
    (-8382924, -308761),
    (-8386082, -205867),
    (-8387976, -102941),
];

#[cfg(feature = "codec2_16k_bridge")]
/// Twiddle factors for `FFT_ENC_SB`=1024 (`spectral_bridge.rs`'s own
/// doubled-resolution transform) -- same generation/checked-in-data
/// convention as [`TWIDDLES_512_Q23`] above, same validating test.
const TWIDDLES_1024_Q23: [(i64, i64); 512] = [
    (8388608, 0),
    (8388450, -51472),
    (8387976, -102941),
    (8387187, -154407),
    (8386082, -205867),
    (8384660, -257319),
    (8382924, -308761),
    (8380871, -360192),
    (8378504, -411609),
    (8375820, -463011),
    (8372822, -514396),
    (8369508, -565761),
    (8365879, -617104),
    (8361935, -668425),
    (8357676, -719720),
    (8353102, -770988),
    (8348215, -822227),
    (8343012, -873436),
    (8337496, -924611),
    (8331666, -975751),
    (8325522, -1026855),
    (8319064, -1077920),
    (8312294, -1128945),
    (8305210, -1179927),
    (8297814, -1230864),
    (8290105, -1281756),
    (8282085, -1332599),
    (8273752, -1383392),
    (8265108, -1434132),
    (8256153, -1484819),
    (8246887, -1535450),
    (8237310, -1586023),
    (8227423, -1636536),
    (8217227, -1686988),
    (8206721, -1737376),
    (8195906, -1787699),
    (8184783, -1837954),
    (8173351, -1888141),
    (8161612, -1938256),
    (8149565, -1988298),
    (8137212, -2038265),
    (8124552, -2088156),
    (8111587, -2137968),
    (8098316, -2187700),
    (8084740, -2237349),
    (8070859, -2286914),
    (8056675, -2336392),
    (8042188, -2385783),
    (8027397, -2435084),
    (8012305, -2484294),
    (7996911, -2533410),
    (7981215, -2582430),
    (7965220, -2631353),
    (7948924, -2680177),
    (7932329, -2728901),
    (7915436, -2777521),
    (7898244, -2826037),
    (7880755, -2874446),
    (7862970, -2922748),
    (7844888, -2970939),
    (7826511, -3019018),
    (7807839, -3066984),
    (7788874, -3114834),
    (7769615, -3162567),
    (7750063, -3210181),
    (7730220, -3257674),
    (7710086, -3305045),
    (7689661, -3352291),
    (7668947, -3399411),
    (7647945, -3446402),
    (7626654, -3493264),
    (7605076, -3539995),
    (7583212, -3586592),
    (7561062, -3633054),
    (7538628, -3679380),
    (7515910, -3725567),
    (7492909, -3771613),
    (7469625, -3817518),
    (7446061, -3863279),
    (7422216, -3908894),
    (7398092, -3954362),
    (7373689, -3999682),
    (7349009, -4044851),
    (7324052, -4089867),
    (7298819, -4134730),
    (7273311, -4179437),
    (7247530, -4223986),
    (7221475, -4268377),
    (7195149, -4312606),
    (7168552, -4356674),
    (7141685, -4400577),
    (7114549, -4444315),
    (7087145, -4487885),
    (7059475, -4531287),
    (7031538, -4574518),
    (7003337, -4617576),
    (6974873, -4660461),
    (6946145, -4703170),
    (6917156, -4745702),
    (6887907, -4788056),
    (6858399, -4830229),
    (6828632, -4872221),
    (6798608, -4914029),
    (6768328, -4955652),
    (6737793, -4997088),
    (6707005, -5038336),
    (6675964, -5079395),
    (6644672, -5120262),
    (6613129, -5160937),
    (6581338, -5201417),
    (6549299, -5241701),
    (6517013, -5281788),
    (6484482, -5321677),
    (6451706, -5361364),
    (6418688, -5400850),
    (6385428, -5440133),
    (6351928, -5479211),
    (6318189, -5518082),
    (6284212, -5556746),
    (6249998, -5595201),
    (6215549, -5633445),
    (6180865, -5671477),
    (6145949, -5709295),
    (6110802, -5746898),
    (6075425, -5784285),
    (6039819, -5821455),
    (6003985, -5858405),
    (5967926, -5895134),
    (5931642, -5931642),
    (5895134, -5967926),
    (5858405, -6003985),
    (5821455, -6039819),
    (5784285, -6075425),
    (5746898, -6110802),
    (5709295, -6145949),
    (5671477, -6180865),
    (5633445, -6215549),
    (5595201, -6249998),
    (5556746, -6284212),
    (5518082, -6318189),
    (5479211, -6351928),
    (5440133, -6385428),
    (5400850, -6418688),
    (5361364, -6451706),
    (5321677, -6484482),
    (5281788, -6517013),
    (5241701, -6549299),
    (5201417, -6581338),
    (5160937, -6613129),
    (5120262, -6644672),
    (5079395, -6675964),
    (5038336, -6707005),
    (4997088, -6737793),
    (4955652, -6768328),
    (4914029, -6798608),
    (4872221, -6828632),
    (4830229, -6858399),
    (4788056, -6887907),
    (4745702, -6917156),
    (4703170, -6946145),
    (4660461, -6974873),
    (4617576, -7003337),
    (4574518, -7031538),
    (4531287, -7059475),
    (4487885, -7087145),
    (4444315, -7114549),
    (4400577, -7141685),
    (4356674, -7168552),
    (4312606, -7195149),
    (4268377, -7221475),
    (4223986, -7247530),
    (4179437, -7273311),
    (4134730, -7298819),
    (4089867, -7324052),
    (4044851, -7349009),
    (3999682, -7373689),
    (3954362, -7398092),
    (3908894, -7422216),
    (3863279, -7446061),
    (3817518, -7469625),
    (3771613, -7492909),
    (3725567, -7515910),
    (3679380, -7538628),
    (3633054, -7561062),
    (3586592, -7583212),
    (3539995, -7605076),
    (3493264, -7626654),
    (3446402, -7647945),
    (3399411, -7668947),
    (3352291, -7689661),
    (3305045, -7710086),
    (3257674, -7730220),
    (3210181, -7750063),
    (3162567, -7769615),
    (3114834, -7788874),
    (3066984, -7807839),
    (3019018, -7826511),
    (2970939, -7844888),
    (2922748, -7862970),
    (2874446, -7880755),
    (2826037, -7898244),
    (2777521, -7915436),
    (2728901, -7932329),
    (2680177, -7948924),
    (2631353, -7965220),
    (2582430, -7981215),
    (2533410, -7996911),
    (2484294, -8012305),
    (2435084, -8027397),
    (2385783, -8042188),
    (2336392, -8056675),
    (2286914, -8070859),
    (2237349, -8084740),
    (2187700, -8098316),
    (2137968, -8111587),
    (2088156, -8124552),
    (2038265, -8137212),
    (1988298, -8149565),
    (1938256, -8161612),
    (1888141, -8173351),
    (1837954, -8184783),
    (1787699, -8195906),
    (1737376, -8206721),
    (1686988, -8217227),
    (1636536, -8227423),
    (1586023, -8237310),
    (1535450, -8246887),
    (1484819, -8256153),
    (1434132, -8265108),
    (1383392, -8273752),
    (1332599, -8282085),
    (1281756, -8290105),
    (1230864, -8297814),
    (1179927, -8305210),
    (1128945, -8312294),
    (1077920, -8319064),
    (1026855, -8325522),
    (975751, -8331666),
    (924611, -8337496),
    (873436, -8343012),
    (822227, -8348215),
    (770988, -8353102),
    (719720, -8357676),
    (668425, -8361935),
    (617104, -8365879),
    (565761, -8369508),
    (514396, -8372822),
    (463011, -8375820),
    (411609, -8378504),
    (360192, -8380871),
    (308761, -8382924),
    (257319, -8384660),
    (205867, -8386082),
    (154407, -8387187),
    (102941, -8387976),
    (51472, -8388450),
    (0, -8388608),
    (-51472, -8388450),
    (-102941, -8387976),
    (-154407, -8387187),
    (-205867, -8386082),
    (-257319, -8384660),
    (-308761, -8382924),
    (-360192, -8380871),
    (-411609, -8378504),
    (-463011, -8375820),
    (-514396, -8372822),
    (-565761, -8369508),
    (-617104, -8365879),
    (-668425, -8361935),
    (-719720, -8357676),
    (-770988, -8353102),
    (-822227, -8348215),
    (-873436, -8343012),
    (-924611, -8337496),
    (-975751, -8331666),
    (-1026855, -8325522),
    (-1077920, -8319064),
    (-1128945, -8312294),
    (-1179927, -8305210),
    (-1230864, -8297814),
    (-1281756, -8290105),
    (-1332599, -8282085),
    (-1383392, -8273752),
    (-1434132, -8265108),
    (-1484819, -8256153),
    (-1535450, -8246887),
    (-1586023, -8237310),
    (-1636536, -8227423),
    (-1686988, -8217227),
    (-1737376, -8206721),
    (-1787699, -8195906),
    (-1837954, -8184783),
    (-1888141, -8173351),
    (-1938256, -8161612),
    (-1988298, -8149565),
    (-2038265, -8137212),
    (-2088156, -8124552),
    (-2137968, -8111587),
    (-2187700, -8098316),
    (-2237349, -8084740),
    (-2286914, -8070859),
    (-2336392, -8056675),
    (-2385783, -8042188),
    (-2435084, -8027397),
    (-2484294, -8012305),
    (-2533410, -7996911),
    (-2582430, -7981215),
    (-2631353, -7965220),
    (-2680177, -7948924),
    (-2728901, -7932329),
    (-2777521, -7915436),
    (-2826037, -7898244),
    (-2874446, -7880755),
    (-2922748, -7862970),
    (-2970939, -7844888),
    (-3019018, -7826511),
    (-3066984, -7807839),
    (-3114834, -7788874),
    (-3162567, -7769615),
    (-3210181, -7750063),
    (-3257674, -7730220),
    (-3305045, -7710086),
    (-3352291, -7689661),
    (-3399411, -7668947),
    (-3446402, -7647945),
    (-3493264, -7626654),
    (-3539995, -7605076),
    (-3586592, -7583212),
    (-3633054, -7561062),
    (-3679380, -7538628),
    (-3725567, -7515910),
    (-3771613, -7492909),
    (-3817518, -7469625),
    (-3863279, -7446061),
    (-3908894, -7422216),
    (-3954362, -7398092),
    (-3999682, -7373689),
    (-4044851, -7349009),
    (-4089867, -7324052),
    (-4134730, -7298819),
    (-4179437, -7273311),
    (-4223986, -7247530),
    (-4268377, -7221475),
    (-4312606, -7195149),
    (-4356674, -7168552),
    (-4400577, -7141685),
    (-4444315, -7114549),
    (-4487885, -7087145),
    (-4531287, -7059475),
    (-4574518, -7031538),
    (-4617576, -7003337),
    (-4660461, -6974873),
    (-4703170, -6946145),
    (-4745702, -6917156),
    (-4788056, -6887907),
    (-4830229, -6858399),
    (-4872221, -6828632),
    (-4914029, -6798608),
    (-4955652, -6768328),
    (-4997088, -6737793),
    (-5038336, -6707005),
    (-5079395, -6675964),
    (-5120262, -6644672),
    (-5160937, -6613129),
    (-5201417, -6581338),
    (-5241701, -6549299),
    (-5281788, -6517013),
    (-5321677, -6484482),
    (-5361364, -6451706),
    (-5400850, -6418688),
    (-5440133, -6385428),
    (-5479211, -6351928),
    (-5518082, -6318189),
    (-5556746, -6284212),
    (-5595201, -6249998),
    (-5633445, -6215549),
    (-5671477, -6180865),
    (-5709295, -6145949),
    (-5746898, -6110802),
    (-5784285, -6075425),
    (-5821455, -6039819),
    (-5858405, -6003985),
    (-5895134, -5967926),
    (-5931642, -5931642),
    (-5967926, -5895134),
    (-6003985, -5858405),
    (-6039819, -5821455),
    (-6075425, -5784285),
    (-6110802, -5746898),
    (-6145949, -5709295),
    (-6180865, -5671477),
    (-6215549, -5633445),
    (-6249998, -5595201),
    (-6284212, -5556746),
    (-6318189, -5518082),
    (-6351928, -5479211),
    (-6385428, -5440133),
    (-6418688, -5400850),
    (-6451706, -5361364),
    (-6484482, -5321677),
    (-6517013, -5281788),
    (-6549299, -5241701),
    (-6581338, -5201417),
    (-6613129, -5160937),
    (-6644672, -5120262),
    (-6675964, -5079395),
    (-6707005, -5038336),
    (-6737793, -4997088),
    (-6768328, -4955652),
    (-6798608, -4914029),
    (-6828632, -4872221),
    (-6858399, -4830229),
    (-6887907, -4788056),
    (-6917156, -4745702),
    (-6946145, -4703170),
    (-6974873, -4660461),
    (-7003337, -4617576),
    (-7031538, -4574518),
    (-7059475, -4531287),
    (-7087145, -4487885),
    (-7114549, -4444315),
    (-7141685, -4400577),
    (-7168552, -4356674),
    (-7195149, -4312606),
    (-7221475, -4268377),
    (-7247530, -4223986),
    (-7273311, -4179437),
    (-7298819, -4134730),
    (-7324052, -4089867),
    (-7349009, -4044851),
    (-7373689, -3999682),
    (-7398092, -3954362),
    (-7422216, -3908894),
    (-7446061, -3863279),
    (-7469625, -3817518),
    (-7492909, -3771613),
    (-7515910, -3725567),
    (-7538628, -3679380),
    (-7561062, -3633054),
    (-7583212, -3586592),
    (-7605076, -3539995),
    (-7626654, -3493264),
    (-7647945, -3446402),
    (-7668947, -3399411),
    (-7689661, -3352291),
    (-7710086, -3305045),
    (-7730220, -3257674),
    (-7750063, -3210181),
    (-7769615, -3162567),
    (-7788874, -3114834),
    (-7807839, -3066984),
    (-7826511, -3019018),
    (-7844888, -2970939),
    (-7862970, -2922748),
    (-7880755, -2874446),
    (-7898244, -2826037),
    (-7915436, -2777521),
    (-7932329, -2728901),
    (-7948924, -2680177),
    (-7965220, -2631353),
    (-7981215, -2582430),
    (-7996911, -2533410),
    (-8012305, -2484294),
    (-8027397, -2435084),
    (-8042188, -2385783),
    (-8056675, -2336392),
    (-8070859, -2286914),
    (-8084740, -2237349),
    (-8098316, -2187700),
    (-8111587, -2137968),
    (-8124552, -2088156),
    (-8137212, -2038265),
    (-8149565, -1988298),
    (-8161612, -1938256),
    (-8173351, -1888141),
    (-8184783, -1837954),
    (-8195906, -1787699),
    (-8206721, -1737376),
    (-8217227, -1686988),
    (-8227423, -1636536),
    (-8237310, -1586023),
    (-8246887, -1535450),
    (-8256153, -1484819),
    (-8265108, -1434132),
    (-8273752, -1383392),
    (-8282085, -1332599),
    (-8290105, -1281756),
    (-8297814, -1230864),
    (-8305210, -1179927),
    (-8312294, -1128945),
    (-8319064, -1077920),
    (-8325522, -1026855),
    (-8331666, -975751),
    (-8337496, -924611),
    (-8343012, -873436),
    (-8348215, -822227),
    (-8353102, -770988),
    (-8357676, -719720),
    (-8361935, -668425),
    (-8365879, -617104),
    (-8369508, -565761),
    (-8372822, -514396),
    (-8375820, -463011),
    (-8378504, -411609),
    (-8380871, -360192),
    (-8382924, -308761),
    (-8384660, -257319),
    (-8386082, -205867),
    (-8387187, -154407),
    (-8387976, -102941),
    (-8388450, -51472),
];

/// Bit-reversal permutation table, `N` entries, `bits = N.trailing_zeros()`
/// wide -- pure integer arithmetic (unlike the twiddle tables above), so
/// this is computed for real at compile time as a `const fn`, not
/// generated offline and checked in: there's no drift risk to guard a
/// test against, and no runtime cost or heap allocation either way.
/// Entries are `u16` (both sizes are at most 1024) to halve the flash.
// [@ANCHOR: build_bit_reverse_table]
const fn build_bit_reverse_table<const N: usize>() -> [u16; N] {
    let bits = (N as u32).trailing_zeros();
    let mut table = [0u16; N];
    let mut i = 0;
    while i < N {
        table[i] = ((i as u32).reverse_bits() >> (32 - bits)) as u16;
        i += 1;
    }
    table
}

/// Twiddle table narrowed to `i32` pairs at compile time: every entry is
/// at most `2^23` in magnitude, so the `i64` source tables above (kept as
/// the checked-in, regenerated-and-diffed reference data) narrow exactly,
/// and the run-time table is half the size.
const fn narrow_twiddles<const M: usize>(src: &[(i64, i64); M]) -> [(i32, i32); M] {
    let mut out = [(0i32, 0i32); M];
    let mut i = 0;
    while i < M {
        assert!(src[i].0 == src[i].0 as i32 as i64 && src[i].1 == src[i].1 as i32 as i64);
        out[i] = (src[i].0 as i32, src[i].1 as i32);
        i += 1;
    }
    out
}

/// The table's second quadrant is the first rotated by a quarter turn:
/// `w[N/4 + m] == -i * w[m]`, i.e. `(wr, wif)` becomes `(wif, -wr)`, exactly
/// as integers (true of the decoder's tables, not of the pitch estimator's).
/// [`block_inv_i64`] can then multiply by a first-quadrant twiddle after an
/// exact rotation of the data by a quarter turn, which is cheaper.
const fn quadrant_symmetric<const M: usize>(t: &[(i32, i32); M]) -> bool {
    let q = M / 2;
    let mut m = 1;
    while m < q {
        if t[q + m].0 != t[m].1 || t[q + m].1 != -t[m].0 || t[m].0 <= 0 || t[m].1 >= 0 {
            return false;
        }
        m += 1;
    }
    true
}

const TW_512: [(i32, i32); FFT_ENC / 2] = narrow_twiddles(&TWIDDLES_512_Q23);
const _: () = assert!(quadrant_symmetric(&TW_512));
static BITREV_512: [u16; FFT_ENC] = build_bit_reverse_table::<FFT_ENC>();
#[cfg(feature = "codec2_16k_bridge")]
const TW_1024: [(i32, i32); FFT_ENC_SB / 2] = narrow_twiddles(&TWIDDLES_1024_Q23);
#[cfg(feature = "codec2_16k_bridge")]
const _: () = assert!(quadrant_symmetric(&TW_1024));
#[cfg(feature = "codec2_16k_bridge")]
static BITREV_1024: [u16; FFT_ENC_SB] = build_bit_reverse_table::<FFT_ENC_SB>();

/// One transform size. The codec runs at exactly two sample rates: 8 kHz
/// (`Size<512>`, [`Rate8k`]) and, with the spectral bridge, 16 kHz
/// (`Size<1024>`, [`Rate16k`]). Everything that depends only on the size
/// (twiddle table, bit-reversal table, stage count, block strides) is a
/// compile-time constant of the monomorphised code: there is no run-time
/// size dispatch and no generic slice length.
pub(crate) struct Size<const N: usize>;

/// 8 kHz transform (`FFT_ENC` = 512 points).
pub(crate) type Rate8k = Size<FFT_ENC>;
/// 16 kHz transform (`FFT_ENC_SB` = 1024 points).
#[cfg(feature = "codec2_16k_bridge")]
pub(crate) type Rate16k = Size<FFT_ENC_SB>;

/// Per-size constant tables. Only sizes with an implementation exist, so
/// asking for any other size is a compile error, not a run-time panic.
pub(crate) trait FftTables {
    /// `(cos, -sin)` of `2 pi k / N` in Q23 for `k < N / 2`.
    const TW: &'static [(i32, i32)];
    /// Bit-reversal permutation of `0..N`.
    const BITREV: &'static [u16];
}

impl FftTables for Rate8k {
    const TW: &'static [(i32, i32)] = &TW_512;
    const BITREV: &'static [u16] = &BITREV_512;
}

#[cfg(feature = "codec2_16k_bridge")]
impl FftTables for Rate16k {
    const TW: &'static [(i32, i32)] = &TW_1024;
    const BITREV: &'static [u16] = &BITREV_1024;
}

/// Twiddle table for size `N` (test reference and generic callers).
// [@ANCHOR: fft_twiddles_q23]
#[cfg(test)]
fn fft_twiddles_q23<const N: usize>() -> &'static [(i32, i32)]
where
    Size<N>: FftTables,
{
    <Size<N> as FftTables>::TW
}

// [@ANCHOR: fft_bit_reverse_table]
#[cfg(test)]
fn fft_bit_reverse_table<const N: usize>() -> &'static [u16]
where
    Size<N>: FftTables,
{
    <Size<N> as FftTables>::BITREV
}

const MODE_CHECKED: u8 = 0;
const MODE_I64: u8 = 1;
const MODE_I32: u8 = 2;

/// Largest value sum (of `|re| + |im|` over the whole transform) that still
/// allows each mode. Every intermediate value is a partial DFT of the
/// inputs, so it is bounded in magnitude by the sum of the input
/// magnitudes (plus a small rounding allowance).
const I32_SUM_LIMIT: u64 = 1 << 29;
///
/// The 64-bit kernel needs every value under 2^39: a twiddle product is
/// `|w| * |b|` with `|w| <= 2^23 (1 + 2^-23)` and `|b| <= sqrt(2) * 2^39`
/// (a complex value with both components under 2^39), which stays under
/// 2^62.6 and so fits `i64` with room for the rounding constant. (The
/// original 64-bit path stopped one bit lower, at 2^37.)
const I64_SUM_LIMIT: u64 = 1 << 39;

#[inline(always)]
fn mode_for_sum(sum: u64) -> u8 {
    if sum < I32_SUM_LIMIT {
        MODE_I32
    } else if sum < I64_SUM_LIMIT {
        MODE_I64
    } else {
        MODE_CHECKED
    }
}

/// `w * x` as the two 32-bit halves of the exact 64-bit product, for a
/// 24-bit twiddle `w` and an `i64` `x`: three 32-bit multiplies (a full
/// 32x32 signed-by-unsigned product for the low word of `x`, and the
/// cross term with its high word, which the 64-bit result truncates).
#[inline(always)]
fn mul_w_x(w: i32, x: i64) -> (u32, i32) {
    let xl = x as u32;
    let xh = (x >> 32) as i32;
    let p = w as i64 * xl as i64; // signed 32 x unsigned 32: one `mul` + one `mulhsu`
    ((p as u32), ((p >> 32) as i32).wrapping_add(w.wrapping_mul(xh)))
}

/// `(w1 * x1 + w2 * x2 + 2^22) >> 23` for 24-bit `w1`, `w2` and `i64`
/// `x1`, `x2` whose weighted sum is under 2^62 in magnitude, done on
/// 32-bit halves. Bit-identical to the plain `i64` expression.
#[inline(always)]
fn mul_round_i64(w1: i32, x1: i64, w2: i32, x2: i64) -> i64 {
    let (l1, h1) = mul_w_x(w1, x1);
    let (l2, h2) = mul_w_x(w2, x2);
    let (lo, c1) = l1.overflowing_add(l2);
    let hi = h1.wrapping_add(h2).wrapping_add(c1 as i32);
    let (lo, c2) = lo.overflowing_add(1 << (FRAC_BITS - 1));
    let hi = hi.wrapping_add(c2 as i32);
    let out_lo = (lo >> FRAC_BITS) | ((hi as u32) << (32 - FRAC_BITS));
    let out_hi = hi >> FRAC_BITS;
    ((out_hi as i64) << 32) | out_lo as i64
}

/// `c * x` for an unsigned 24-bit `c` and an `i64` `x`, as the (low, high)
/// 32-bit words of the low 64 bits of the product: one full 32x32->64
/// unsigned product (`mul` + `mulhu`) for the low word of `x` and one
/// 32-bit product for its high word. No sign corrections are needed
/// because `c` is unsigned.
#[inline(always)]
fn umul_x(c: u32, x: i64) -> (u32, u32) {
    let xl = x as u32;
    let xh = (x >> 32) as u32;
    let p = c as u64 * xl as u64;
    (p as u32, ((p >> 32) as u32).wrapping_add(c.wrapping_mul(xh)))
}

/// The two exact 64-bit sums of an inverse-transform twiddle multiply with
/// first-quadrant twiddle `(c, -s)` (`c`, `s` positive): `A = c*x - s*y`
/// and `B = c*y + s*x` (the real and imaginary parts of `w * (x + j y)`
/// before rounding), each as (low, high) words.
#[inline(always)]
fn twiddle_sums(c: u32, s: u32, x: i64, y: i64) -> ((u32, u32), (u32, u32)) {
    let (cxl, cxh) = umul_x(c, x);
    let (syl, syh) = umul_x(s, y);
    let (cyl, cyh) = umul_x(c, y);
    let (sxl, sxh) = umul_x(s, x);
    let al = cxl.wrapping_sub(syl);
    let ah = cxh.wrapping_sub(syh).wrapping_sub((cxl < syl) as u32);
    let (bl, bc) = cyl.overflowing_add(sxl);
    let bh = cyh.wrapping_add(sxh).wrapping_add(bc as u32);
    ((al, ah), (bl, bh))
}

/// `(v + 2^22) >> 23` for a 64-bit value in (low, high) words whose result
/// fits `i64` (arithmetic shift).
#[inline(always)]
fn round_shift((lo, hi): (u32, u32)) -> i64 {
    let (l, c) = lo.overflowing_add(1 << (FRAC_BITS - 1));
    let h = hi.wrapping_add(c as u32);
    let out_lo = (l >> FRAC_BITS) | (h << (32 - FRAC_BITS));
    let out_hi = (h as i32) >> FRAC_BITS;
    ((out_hi as i64) << 32) | out_lo as i64
}

/// `(2^22 - v) >> 23`, i.e. the rounded shift of `-v`.
#[inline(always)]
fn round_shift_neg((lo, hi): (u32, u32)) -> i64 {
    let l = (1u32 << (FRAC_BITS - 1)).wrapping_sub(lo);
    let h = 0u32.wrapping_sub(hi).wrapping_sub((1u32 << (FRAC_BITS - 1) < lo) as u32);
    let out_lo = (l >> FRAC_BITS) | (h << (32 - FRAC_BITS));
    let out_hi = (h as i32) >> FRAC_BITS;
    ((out_hi as i64) << 32) | out_lo as i64
}

/// [`block`] for the inverse direction in the 64-bit mode (all values under
/// 2^39), without sign corrections in the multiplies. The twiddle `(wr, wi)`
/// of the inverse transform has `wi = -wif > 0` everywhere, `wr > 0` for
/// `j < half/2` and `wr < 0` above it, so every product can use an unsigned
/// magnitude. For `j < q` the exact product is `(A, B)` of
/// [`twiddle_sums`] with `(c, s) = (wr, wi)`; for `j > q` it is `(-B', -A')`
/// where `(A', B')` are the sums for `(c, s) = (-wr, wi)` with the data
/// components swapped. Bit-identical to the generic butterfly, for any
/// table with those signs (the decoder's and the pitch estimator's). With
/// `SYM` the table must be [`quadrant_symmetric`] and the upper quadrant is
/// computed from the first-quadrant entries by an exact quarter-turn
/// rotation of the data, which needs no negated twiddle and one rounding
/// of each sign.
#[inline(always)]
fn block_inv_i64<const SYM: bool>(
    re: &mut [i64],
    im: &mut [i64],
    tw: &[(i32, i32)],
    base: usize,
    len: usize,
    step: usize,
) {
    let half = len / 2;
    let (lr, hr) = re[base..base + len].split_at_mut(half);
    let (li, hi) = im[base..base + len].split_at_mut(half);
    bf_one(&mut lr[0], &mut li[0], &mut hr[0], &mut hi[0]);
    if half >= 2 {
        let q = half / 2;
        bf_quarter::<false>(&mut lr[q], &mut li[q], &mut hr[q], &mut hi[q]);
        if SYM {
            // `j = q + m`: the inverse twiddle is `+i * w[m]`, so the exact
            // product is `w[m] * (i * b)` = `w[m] * (-bi + j br)`. With the
            // first-quadrant sums `(A, B)` of the unrotated data that is
            // `(-B, A)`, and the rounding is applied to the exact sums.
            // Both quadrants share one twiddle load per iteration.
            for m in 1..q {
                let (wr, wif) = tw[m * step];
                let (c, sn) = (wr as u32, (-wif) as u32);
                let (a, b) = twiddle_sums(c, sn, hr[m], hi[m]);
                let (vr, vi) = (round_shift(a), round_shift(b));
                let (xr, xi) = (lr[m], li[m]);
                lr[m] = xr + vr;
                li[m] = xi + vi;
                hr[m] = xr - vr;
                hi[m] = xi - vi;
                let j = q + m;
                let (a, b) = twiddle_sums(c, sn, hr[j], hi[j]);
                let (vr, vi) = (round_shift_neg(b), round_shift(a));
                let (xr, xi) = (lr[j], li[j]);
                lr[j] = xr + vr;
                li[j] = xi + vi;
                hr[j] = xr - vr;
                hi[j] = xi - vi;
            }
        } else {
            for j in 1..q {
                let (wr, wif) = tw[j * step];
                let (a, b) = twiddle_sums(wr as u32, (-wif) as u32, hr[j], hi[j]);
                let (vr, vi) = (round_shift(a), round_shift(b));
                let (xr, xi) = (lr[j], li[j]);
                lr[j] = xr + vr;
                li[j] = xi + vi;
                hr[j] = xr - vr;
                hi[j] = xi - vi;
            }
            for j in q + 1..half {
                let (wr, wif) = tw[j * step];
                let (a, b) = twiddle_sums((-wr) as u32, (-wif) as u32, hi[j], hr[j]);
                let (vr, vi) = (round_shift_neg(b), round_shift_neg(a));
                let (xr, xi) = (lr[j], li[j]);
                lr[j] = xr + vr;
                li[j] = xi + vi;
                hr[j] = xr - vr;
                hi[j] = xi - vi;
            }
        }
    }
}

/// One radix-2 butterfly on `(a, b)` with twiddle `(wr, wif)` (`wif` is the
/// forward-convention imaginary part; the inverse negates it). `MODE` says
/// how much the caller has proven about the whole transform: `MODE_I32`
/// every value is under 2^29 (32-bit data, 32x32->64 products), `MODE_I64`
/// every value is under 2^39 (64-bit products, |wr br - wi bi| < 2^63), `MODE_CHECKED` nothing (each butterfly uses the range-checked
/// [`twiddle_mul`]). Same integer arithmetic in every mode, so the same
/// result.
#[inline(always)]
fn bf<const FWD: bool, const MODE: u8>(
    ar: &mut i64,
    ai: &mut i64,
    br: &mut i64,
    bi: &mut i64,
    (wr, wif): (i32, i32),
) {
    let wi = if FWD { wif } else { -wif };
    if MODE == MODE_I32 {
        // Every value is under 2^29 in magnitude, so it fits `i32`; each
        // product is then a 32x32->64 multiply (two instructions on a
        // 32-bit core) instead of a full 64x64 one.
        let (wr, wi) = (wr as i64, wi as i64);
        let (yr, yi) = (*br as i32 as i64, *bi as i32 as i64);
        let vr = ((wr * yr - wi * yi + (1 << (FRAC_BITS - 1))) >> FRAC_BITS) as i32;
        let vi = ((wr * yi + wi * yr + (1 << (FRAC_BITS - 1))) >> FRAC_BITS) as i32;
        let (xr, xi) = (*ar as i32, *ai as i32);
        *ar = (xr + vr) as i64;
        *ai = (xi + vi) as i64;
        *br = (xr - vr) as i64;
        *bi = (xi - vi) as i64;
        return;
    }
    let (vr, vi) = if MODE == MODE_I64 {
        (
            mul_round_i64(wr, *br, -wi, *bi),
            mul_round_i64(wr, *bi, wi, *br),
        )
    } else {
        twiddle_mul(wr as i64, wi as i64, *br, *bi)
    };
    let (xr, xi) = (*ar, *ai);
    *ar = xr + vr;
    *ai = xi + vi;
    *br = xr - vr;
    *bi = xi - vi;
}

/// The two exactly representable twiddles need no multiply: `j == 0` is
/// `1` (`(x * 2^23 + 2^22) >> 23 == x`) and `j == half/2` is `-j`
/// (forward) / `+j` (inverse) whose table entry is exactly `(0, -2^23)`.
/// Those butterflies are add/subtract only, bit-identical to the generic
/// butterfly.
#[inline(always)]
fn bf_one(ar: &mut i64, ai: &mut i64, br: &mut i64, bi: &mut i64) {
    let (xr, xi, yr, yi) = (*ar, *ai, *br, *bi);
    *ar = xr + yr;
    *ai = xi + yi;
    *br = xr - yr;
    *bi = xi - yi;
}

#[inline(always)]
fn bf_quarter<const FWD: bool>(ar: &mut i64, ai: &mut i64, br: &mut i64, bi: &mut i64) {
    let (xr, xi, yr, yi) = (*ar, *ai, *br, *bi);
    let (vr, vi) = if FWD { (yi, -yr) } else { (-yi, yr) };
    *ar = xr + vr;
    *ai = xi + vi;
    *br = xr - vr;
    *bi = xi - vi;
}

/// One block of one stage: the `len` positions starting at `base` hold two
/// finished half-size transforms (`lo`, `hi`) that are merged in place.
/// `len`/`step` are literals in the hot monomorphisations (so every
/// address and trip count is a constant) and run-time values in the
/// compact fallback. `hi_zero` says the upper half is all zero (sparse
/// input), which makes every butterfly `(a, a)`.
#[inline(always)]
fn block<const FWD: bool, const MODE: u8>(
    re: &mut [i64],
    im: &mut [i64],
    tw: &[(i32, i32)],
    base: usize,
    len: usize,
    step: usize,
    hi_zero: bool,
) {
    let half = len / 2;
    let (lr, hr) = re[base..base + len].split_at_mut(half);
    let (li, hi) = im[base..base + len].split_at_mut(half);
    if hi_zero {
        hr.copy_from_slice(lr);
        hi.copy_from_slice(li);
        return;
    }
    bf_one(&mut lr[0], &mut li[0], &mut hr[0], &mut hi[0]);
    if half >= 2 {
        let q = half / 2;
        bf_quarter::<FWD>(&mut lr[q], &mut li[q], &mut hr[q], &mut hi[q]);
        for j in 1..q {
            bf::<FWD, MODE>(&mut lr[j], &mut li[j], &mut hr[j], &mut hi[j], tw[j * step]);
        }
        for j in q + 1..half {
            bf::<FWD, MODE>(&mut lr[j], &mut li[j], &mut hr[j], &mut hi[j], tw[j * step]);
        }
    }
}

/// One dense stage (every block). Test oracle only.
#[cfg(test)]
#[inline(always)]
fn stage_dense<const N: usize, const FWD: bool, const MODE: u8>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    len: usize,
) {
    let step = N / len;
    let mut i = 0;
    while i < N {
        block::<FWD, MODE>(re, im, tw, i, len, step, false);
        i += len;
    }
}

/// Length of the run of identical values that block `r` (residue `r`, at
/// the stage where blocks have `len` positions) holds while its upper half
/// is still all zero: the block's content is then just input `r` repeated,
/// so it is written directly instead of being copied up stage by stage. The
/// block exists (`r < N / len`) and has a zero upper half (`r + N / len >=
/// nz`) exactly while `len <= N / max(nz - r, r + 1)`; this is the largest
/// such power of two.
const fn prefix_fill_len(n: usize, nz: usize, r: usize) -> usize {
    let m = if nz - r > r + 1 { nz - r } else { r + 1 };
    let q = n / m;
    1 << (usize::BITS - 1 - q.leading_zeros())
}

struct FillLens<const N: usize, const NZ: usize>;

impl<const N: usize, const NZ: usize> FillLens<N, NZ> {
    const T: [u16; NZ] = {
        let mut t = [0u16; NZ];
        let mut r = 0;
        while r < NZ {
            t[r] = prefix_fill_len(N, NZ, r) as u16;
            r += 1;
        }
        t
    };
}

/// Places real input `v` for natural index `k` at its bit-reversed
/// position, filling the `fill` positions of the block that stay constant
/// (see [`prefix_fill_len`]); `im` is zero there.
#[inline(always)]
fn scatter_prefix<const N: usize>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    bitrev: &[u16],
    k: usize,
    fill: usize,
    v: i64,
) {
    let p = bitrev[k] as usize;
    re[p..p + fill].fill(v);
    im[p..p + fill].fill(0);
}

/// One sparse-prefix stage. The natural-order input is nonzero only in
/// `0..nz`; the block of `len` positions holding natural indices
/// `r, r + step, ...` (`step = N / len`) sits at bit-reversed position
/// `bitrev[r]` and is all zero exactly when `r >= nz`, and its upper half
/// (`r + step`) is all zero when `r + step >= nz`. Zero blocks are never
/// touched (nothing later reads them either, because their parent sees a
/// zero upper half), and a block with a zero upper half is a run of one
/// repeated input that [`scatter_prefix`] already wrote, so it is skipped
/// too; the arrays do not have to be cleared beforehand.
#[inline(always)]
fn stage_prefix<const N: usize, const FWD: bool, const MODE: u8>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    bitrev: &[u16],
    len: usize,
    nz: usize,
) {
    let step = N / len;
    let blocks = if nz < step { nz } else { step };
    let mut r = 0;
    while r < blocks {
        if r + step < nz {
            if !FWD && MODE == MODE_I64 {
                block_inv_i64::<false>(re, im, tw, bitrev[r] as usize, len, step);
            } else {
                block::<FWD, MODE>(re, im, tw, bitrev[r] as usize, len, step, false);
            }
        }
        r += 1;
    }
}

#[inline(always)]
fn stages_prefix_hot<const N: usize, const FWD: bool, const MODE: u8>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    bitrev: &[u16],
    nz: usize,
) {
    macro_rules! st {
        ($len:literal) => {
            if $len <= N {
                stage_prefix::<N, FWD, MODE>(re, im, tw, bitrev, $len, nz);
            }
        };
    }
    st!(2);
    st!(4);
    st!(8);
    st!(16);
    st!(32);
    st!(64);
    st!(128);
    st!(256);
    st!(512);
    st!(1024);
}

/// Compact fallbacks: one copy of the stage loop per (size, direction,
/// mode), the stage length a run-time variable. Used for the cold modes
/// (inputs too large for the fast paths, or the test-only forward dense
/// transform), where code size matters more than speed.
#[cfg(test)]
#[inline(never)]
fn stages_dense_loop<const N: usize, const FWD: bool, const MODE: u8>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
) {
    let mut len = 2usize;
    while len <= N {
        stage_dense::<N, FWD, MODE>(re, im, tw, len);
        len *= 2;
    }
}

#[inline(never)]
fn stages_prefix_loop<const N: usize, const FWD: bool, const MODE: u8>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    bitrev: &[u16],
    nz: usize,
) {
    let mut len = 2usize;
    while len <= N {
        stage_prefix::<N, FWD, MODE>(re, im, tw, bitrev, len, nz);
        len *= 2;
    }
}

/// In-place bit-reversal permutation of a dense input.
#[cfg(test)]
#[inline(always)]
fn bit_reverse_permute<const N: usize>(re: &mut [i64; N], im: &mut [i64; N], bitrev: &[u16]) {
    for (i, &j) in bitrev.iter().enumerate() {
        let j = j as usize;
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
}

#[cfg(test)]
fn sum_abs<const N: usize>(re: &[i64; N], im: &[i64; N]) -> u64 {
    let mut sum = 0u64;
    for i in 0..N {
        sum = sum.saturating_add(re[i].unsigned_abs().saturating_add(im[i].unsigned_abs()));
    }
    sum
}

/// In-place radix-2 decimation-in-time FFT of exactly `N` points, Q23
/// fixed-point throughout (no float inside the transform). No per-stage
/// rescaling: `i64`/`i128` headroom vastly exceeds this transform's real
/// dynamic range (LPC-spectrum and sparse-harmonic-spectrum inputs, not
/// full-scale noise).
///
/// `N` is a const generic: the 8 kHz decoder uses `N = 512`, the 16 kHz
/// spectral bridge `N = 1024`, and each gets its own monomorphised copy
/// with its own constant tables. `forward == true` matches `rustfft`'s
/// `plan_fft_forward`; `false` is the unnormalised inverse.
// [@ANCHOR: fft_fixed]
#[cfg(test)]
pub(crate) fn fft_fixed<const N: usize>(re: &mut [i64; N], im: &mut [i64; N], forward: bool)
where
    Size<N>: FftTables,
{
    let bitrev = <Size<N> as FftTables>::BITREV;
    let tw = <Size<N> as FftTables>::TW;
    bit_reverse_permute::<N>(re, im, bitrev);
    let mode = mode_for_sum(sum_abs::<N>(re, im));
    match (forward, mode) {
        (true, MODE_I32) => stages_dense_loop::<N, true, MODE_I32>(re, im, tw),
        (true, MODE_I64) => stages_dense_loop::<N, true, MODE_I64>(re, im, tw),
        (true, _) => stages_dense_loop::<N, true, MODE_CHECKED>(re, im, tw),
        (false, MODE_I32) => stages_dense_loop::<N, false, MODE_I32>(re, im, tw),
        (false, MODE_I64) => stages_dense_loop::<N, false, MODE_I64>(re, im, tw),
        (false, _) => stages_dense_loop::<N, false, MODE_CHECKED>(re, im, tw),
    }
}

/// Same transform as [`fft_fixed`] for an input whose only nonzero entries
/// are the real values `input[0..nz]` (`re[i] = input[i]`, `im` all zero):
/// bit-identical output, but the input is scattered straight to its
/// bit-reversed positions, blocks whose inputs are known zeros are skipped
/// or reduced to copies, and neither array needs clearing beforehand (both
/// are fully overwritten). See [`stage_prefix`].
// [@ANCHOR: fft_fixed_sparse_prefix]
pub(crate) fn fft_fixed_sparse_prefix<const N: usize>(
    input: &[i64],
    re: &mut [i64; N],
    im: &mut [i64; N],
    forward: bool,
) where
    Size<N>: FftTables,
{
    prefix_general::<N>(
        input,
        re,
        im,
        forward,
        <Size<N> as FftTables>::TW,
        <Size<N> as FftTables>::BITREV,
    );
}

/// The compact, any-mode form of the sparse-prefix transform for an
/// arbitrary twiddle table (the decoder's, or the pitch estimator's).
fn prefix_general<const N: usize>(
    input: &[i64],
    re: &mut [i64; N],
    im: &mut [i64; N],
    forward: bool,
    tw: &[(i32, i32)],
    bitrev: &[u16],
) {
    let nz = input.len();
    debug_assert!(nz >= 1 && nz <= N);
    let mut sum = 0u64;
    for (k, &v) in input.iter().enumerate() {
        scatter_prefix::<N>(re, im, bitrev, k, prefix_fill_len(N, nz, k), v);
        sum = sum.saturating_add(v.unsigned_abs());
    }
    match (forward, mode_for_sum(sum)) {
        (true, MODE_I32) => stages_prefix_loop::<N, true, MODE_I32>(re, im, tw, bitrev, nz),
        (true, MODE_I64) => stages_prefix_loop::<N, true, MODE_I64>(re, im, tw, bitrev, nz),
        (true, _) => stages_prefix_loop::<N, true, MODE_CHECKED>(re, im, tw, bitrev, nz),
        (false, MODE_I32) => stages_prefix_loop::<N, false, MODE_I32>(re, im, tw, bitrev, nz),
        (false, MODE_I64) => stages_prefix_loop::<N, false, MODE_I64>(re, im, tw, bitrev, nz),
        (false, _) => stages_prefix_loop::<N, false, MODE_CHECKED>(re, im, tw, bitrev, nz),
    }
}

/// [`fft_fixed_sparse_prefix`] with the number of nonzero inputs a
/// compile-time constant, forward direction: the production LPC-spectrum
/// call. When the input is small enough for the 32-bit kernel (always, for
/// real LPC coefficients) the whole transform runs in one fully specialised
/// instance: every block position, copy decision and twiddle stride is a
/// constant.
pub(crate) fn fft_fixed_sparse_prefix_forward<const N: usize, const NZ: usize>(
    input: &[i64; NZ],
    re: &mut [i64; N],
    im: &mut [i64; N],
) where
    Size<N>: FftTables,
{
    let bitrev = <Size<N> as FftTables>::BITREV;
    let tw = <Size<N> as FftTables>::TW;
    let mut sum = 0u64;
    for &v in input {
        sum = sum.saturating_add(v.unsigned_abs());
    }
    if sum < I32_SUM_LIMIT {
        for (k, &v) in input.iter().enumerate() {
            scatter_prefix::<N>(re, im, bitrev, k, FillLens::<N, NZ>::T[k] as usize, v);
        }
        prefix_forward_i32_hot::<N, NZ>(re, im, tw, bitrev);
    } else {
        fft_fixed_sparse_prefix::<N>(input, re, im, true);
    }
}

/// The pitch estimator's twiddles (`(cos, +sin)` of `2 pi k / 512`, its own
/// table, which differs from the decoder's in the last bit at some entries
/// and so must be kept) in this module's `(wr, wif)` layout, where the
/// inverse direction applies `wi = -wif`.
const PITCH_TW_512: [(i32, i32); FFT_ENC / 2] = {
    let src = &super::tables::NLP_TWIDDLES_Q23;
    let mut out = [(0i32, 0i32); FFT_ENC / 2];
    let mut i = 0;
    while i < FFT_ENC / 2 {
        assert!(src[i].0 == src[i].0 as i32 as i64 && src[i].1 == src[i].1 as i32 as i64);
        out[i] = (src[i].0 as i32, -(src[i].1 as i32));
        i += 1;
    }
    out
};

/// The pitch estimator's 512-point transform: real input `input[0..NZ]`
/// (natural order), everything else zero, the estimator's own twiddle table
/// and sign convention (the inverse direction in this module's terms).
/// Runs in the 64-bit kernel, fully specialised for `NZ`, whenever the
/// input's value sum allows it (always, for the estimator's windowed
/// samples), and falls back to the general exact path otherwise.
pub(crate) fn pitch_fft_512<const NZ: usize>(
    input: &[i64; NZ],
    re: &mut [i64; FFT_ENC],
    im: &mut [i64; FFT_ENC],
) {
    let bitrev = <Rate8k as FftTables>::BITREV;
    let mut sum = 0u64;
    for &v in input {
        sum = sum.saturating_add(v.unsigned_abs());
    }
    if sum < I64_SUM_LIMIT {
        for (k, &v) in input.iter().enumerate() {
            scatter_prefix::<FFT_ENC>(re, im, bitrev, k, FillLens::<FFT_ENC, NZ>::T[k] as usize, v);
        }
        pitch_prefix_i64_hot::<NZ>(re, im, bitrev);
    } else {
        prefix_general::<FFT_ENC>(input, re, im, false, &PITCH_TW_512, bitrev);
    }
}

#[inline(never)]
fn pitch_prefix_i64_hot<const NZ: usize>(re: &mut [i64; FFT_ENC], im: &mut [i64; FFT_ENC], bitrev: &[u16]) {
    stages_prefix_hot::<FFT_ENC, false, MODE_I64>(re, im, &PITCH_TW_512, bitrev, NZ);
}

#[inline(never)]
fn prefix_forward_i32_hot<const N: usize, const NZ: usize>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    bitrev: &[u16],
) {
    stages_prefix_hot::<N, true, MODE_I32>(re, im, tw, bitrev, NZ);
}

/// Real part of `(wr + j wi) * (br + j bi)` rounded back to Q23, for the
/// inverse direction (`wi = -wif`), in the given mode.
#[inline(always)]
fn twiddle_real_inverse<const MODE: u8>((wr, wif): (i32, i32), br: i64, bi: i64) -> i64 {
    if MODE == MODE_CHECKED {
        // Same range check as `twiddle_mul`, real part only.
        let (wr, wi) = (wr as i64, -(wif as i64));
        if (br.unsigned_abs() | bi.unsigned_abs()) < (1u64 << 38) {
            rshift_round_i64(wr * br - wi * bi, FRAC_BITS)
        } else {
            rshift_round_i128(wr as i128 * br as i128 - wi as i128 * bi as i128, FRAC_BITS)
        }
    } else {
        mul_round_i64(wr, br, wif, bi)
    }
}

/// Number of `u32` words of block-occupancy flags kept for a transform of
/// size `N` (`2N` bits: `N` for the inputs, then `N/2`, `N/4`, ... one
/// level per stage), sized for the largest supported transform.
const FLAG_WORDS: usize = 2 * FFT_MAX / 32;
const FFT_MAX: usize = 1024;

#[inline(always)]
fn flag(flags: &[u32; FLAG_WORDS], bit: usize) -> bool {
    (flags[bit >> 5] >> (bit & 31)) & 1 != 0
}

#[inline(always)]
fn set_flag(flags: &mut [u32; FLAG_WORDS], bit: usize) {
    flags[bit >> 5] |= 1 << (bit & 31);
}

/// Fills the flag levels above the input level: the block of stage `s`
/// (`step = N >> s` residues) with residue `r` is nonzero when either of
/// its halves (residues `r` and `r + step` of the previous level) is.
/// Level `s` starts at bit `2N - 2 * step`.
fn fold_flags<const N: usize>(flags: &mut [u32; FLAG_WORDS]) {
    let mut step = N / 2;
    while step >= 1 {
        let out = 2 * N - 2 * step;
        let inp = 2 * N - 4 * step;
        if step >= 32 {
            let (ow, iw, sw) = (out / 32, inp / 32, step / 32);
            for w in 0..sw {
                flags[ow + w] = flags[iw + w] | flags[iw + w + sw];
            }
        } else {
            for r in 0..step {
                if flag(flags, inp + r) || flag(flags, inp + r + step) {
                    set_flag(flags, out + r);
                }
            }
        }
        step /= 2;
    }
}

/// One stage of the sparse inverse: only blocks whose flag is set are
/// touched. A block whose upper half is all zero is a copy of its lower
/// half; one whose lower half is all zero gets that half cleared first and
/// then takes the ordinary butterfly (rare, so not worth its own kernel).
#[inline(always)]
fn stage_sparse<const N: usize, const MODE: u8>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    bitrev: &[u16],
    flags: &[u32; FLAG_WORDS],
    len: usize,
) {
    let step = N / len;
    let out = 2 * N - 2 * step;
    let inp = 2 * N - 4 * step;
    for (r, &pos) in bitrev.iter().enumerate().take(step) {
        if !flag(flags, out + r) {
            continue;
        }
        let base = pos as usize;
        let half = len / 2;
        if !flag(flags, inp + r + step) {
            block::<false, MODE>(re, im, tw, base, len, step, true);
            continue;
        }
        if !flag(flags, inp + r) {
            re[base..base + half].fill(0);
            im[base..base + half].fill(0);
        }
        if MODE == MODE_I64 {
            block_inv_i64::<true>(re, im, tw, base, len, step);
        } else {
            block::<false, MODE>(re, im, tw, base, len, step, false);
        }
    }
}

#[inline(always)]
fn stages_sparse_hot<const N: usize, const MODE: u8>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    bitrev: &[u16],
    flags: &[u32; FLAG_WORDS],
) {
    macro_rules! st {
        ($len:literal) => {
            if $len < N {
                stage_sparse::<N, MODE>(re, im, tw, bitrev, flags, $len);
            }
        };
    }
    st!(2);
    st!(4);
    st!(8);
    st!(16);
    st!(32);
    st!(64);
    st!(128);
    st!(256);
    st!(512);
}

#[inline(never)]
fn stages_sparse_loop<const N: usize, const MODE: u8>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    bitrev: &[u16],
    flags: &[u32; FLAG_WORDS],
) {
    let mut len = 2usize;
    while len < N {
        stage_sparse::<N, MODE>(re, im, tw, bitrev, flags, len);
        len *= 2;
    }
}

/// The last stage of the inverse transform, computing only what the
/// synthesis reads: the real parts of outputs `0..=NS` (lower half) and of
/// `N - NS + 1 .. N` (upper half, i.e. indices `N/2 - NS + 1 .. N/2` of the
/// upper half). Nothing else is written. Requires `NS < N / 4`.
#[inline(always)]
fn final_stage_real<const N: usize, const NS: usize, const MODE: u8>(
    re: &mut [i64; N],
    im: &[i64; N],
    tw: &[(i32, i32)],
) {
    let half = N / 2;
    let (lr, hr) = re.split_at_mut(half);
    let (_, hi) = im.split_at(half);
    // j == 0: twiddle 1.
    lr[0] += hr[0];
    if MODE == MODE_I64 {
        // Both ranges avoid the quarter-turn twiddle `j == N/4`: the lower
        // is below it, the upper above it (see `block_inv_i64`).
        const { assert!(NS < N / 4) };
        for j in 1..=NS {
            let (wr, wif) = tw[j];
            let (a, _) = twiddle_sums(wr as u32, (-wif) as u32, hr[j], hi[j]);
            lr[j] += round_shift(a);
        }
        for j in half - NS + 1..half {
            let (wr, wif) = tw[j - N / 4];
            let (_, b) = twiddle_sums(wr as u32, (-wif) as u32, hr[j], hi[j]);
            hr[j] = lr[j] - round_shift_neg(b);
        }
    } else {
        for j in 1..=NS {
            let v = twiddle_real_inverse::<MODE>(tw[j], hr[j], hi[j]);
            lr[j] += v;
        }
        for j in half - NS + 1..half {
            let v = twiddle_real_inverse::<MODE>(tw[j], hr[j], hi[j]);
            hr[j] = lr[j] - v;
        }
    }
}

#[inline(never)]
fn final_stage_real_hot<const N: usize, const NS: usize>(
    re: &mut [i64; N],
    im: &[i64; N],
    tw: &[(i32, i32)],
) {
    final_stage_real::<N, NS, MODE_I64>(re, im, tw);
}

#[inline(never)]
fn final_stage_real_checked<const N: usize, const NS: usize>(
    re: &mut [i64; N],
    im: &[i64; N],
    tw: &[(i32, i32)],
) {
    final_stage_real::<N, NS, MODE_CHECKED>(re, im, tw);
}

#[inline(never)]
fn sparse_inverse_hot<const N: usize, const NS: usize>(
    re: &mut [i64; N],
    im: &mut [i64; N],
    tw: &[(i32, i32)],
    bitrev: &[u16],
    flags: &[u32; FLAG_WORDS],
) {
    stages_sparse_hot::<N, MODE_I64>(re, im, tw, bitrev, flags);
    final_stage_real_hot::<N, NS>(re, im, tw);
}

/// Inverse transform of a sparse, conjugate-symmetric (real output)
/// spectrum, for the harmonic synthesis: the caller [`put`](Self::put)s the
/// bins `1..N/2` that are nonzero (the mirrored bin `N - b` is filled in
/// here), then [`run`](Self::run)s. Neither buffer needs clearing: inputs
/// are scattered straight to their bit-reversed positions, occupancy flags
/// record which blocks of each stage can be nonzero, and zero blocks are
/// never read. Only the outputs the overlap-add uses are computed: the real
/// parts at indices `0..=NS` and `N - NS + 1 .. N`; the rest of `re`, and
/// all of `im`, are left undefined.
///
/// Bit-identical, at those outputs, to filling both buffers densely
/// (including the mirror `X[N-k] = conj(X[k])`) and running [`fft_fixed`]
/// backward.
pub(crate) struct SparseInverse<'a, const N: usize> {
    re: &'a mut [i64; N],
    im: &'a mut [i64; N],
    flags: [u32; FLAG_WORDS],
    sum: u64,
}

impl<'a, const N: usize> SparseInverse<'a, N>
where
    Size<N>: FftTables,
{
    pub(crate) fn new(re: &'a mut [i64; N], im: &'a mut [i64; N]) -> Self {
        SparseInverse { re, im, flags: [0; FLAG_WORDS], sum: 0 }
    }

    /// Sets bin `b` (`1 <= b < N / 2`) and its mirror. A later `put` at the
    /// same bin overwrites the earlier one, as the dense fill did.
    #[inline(always)]
    pub(crate) fn put(&mut self, b: usize, v: ComplexQ23) {
        let bitrev = <Size<N> as FftTables>::BITREV;
        let (p, pm) = (bitrev[b] as usize, bitrev[N - b] as usize);
        self.re[p] = v.re;
        self.im[p] = v.im;
        self.re[pm] = v.re;
        self.im[pm] = -v.im;
        set_flag(&mut self.flags, b);
        set_flag(&mut self.flags, N - b);
        // Upper bound on the transform's value sum (counts overwritten
        // bins too, which only makes the bound looser).
        self.sum = self
            .sum
            .saturating_add(2 * (v.re.unsigned_abs().saturating_add(v.im.unsigned_abs())));
    }

    /// Runs the transform; see the type's documentation for what is valid
    /// afterwards.
    pub(crate) fn run<const NS: usize>(mut self) {
        let bitrev = <Size<N> as FftTables>::BITREV;
        let tw = <Size<N> as FftTables>::TW;
        fold_flags::<N>(&mut self.flags);
        // Halves of the last stage that are entirely zero: clear them so
        // the ordinary last stage applies. The last stage is level `s` with
        // `step == 1`, its halves are the two residues of the level below.
        let lvl = 2 * N - 4; // level of stage N/2: step == 2
        if !flag(&self.flags, lvl) {
            self.re[..N / 2].fill(0);
            self.im[..N / 2].fill(0);
        }
        if !flag(&self.flags, lvl + 1) {
            self.re[N / 2..].fill(0);
            self.im[N / 2..].fill(0);
        }
        if self.sum < I64_SUM_LIMIT {
            sparse_inverse_hot::<N, NS>(self.re, self.im, tw, bitrev, &self.flags);
        } else {
            stages_sparse_loop::<N, MODE_CHECKED>(self.re, self.im, tw, bitrev, &self.flags);
            final_stage_real_checked::<N, NS>(self.re, self.im, tw);
        }
    }
}

/// Scratch buffers for one 512-point transform. Kept in a long-lived
/// state struct (the decoder's synthesis state) and lent to whichever stage
/// needs a transform next, instead of being two 4 KB arrays on the stack of
/// every caller. Stages that use it start from whatever it holds, so each
/// one initialises what it reads.
pub(crate) struct FftScratch {
    pub(crate) re: [i64; FFT_ENC],
    pub(crate) im: [i64; FFT_ENC],
}

impl FftScratch {
    pub(crate) const fn new() -> Self {
        FftScratch { re: [0; FFT_ENC], im: [0; FFT_ENC] }
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

    /// Regenerates both twiddle tables from the real, live formula
    /// (`build_twiddles_q23`, `f32`-transcendental-based) and diffs them
    /// against the checked-in `const` data every test run -- the real
    /// guard against exactly the class of bug `lpc.rs`'s own
    /// `BW_GAMMA_Q23` doc comment records: an independently-computed or
    /// hand-copied table silently disagreeing with the function it's
    /// supposed to be a frozen snapshot of. A mismatch here means the
    /// tables need regenerating, not that this test is wrong.
    #[test]
    fn twiddle_tables_match_their_own_generating_formula() {
        let regenerated_512 = build_twiddles_q23(FFT_ENC);
        assert_eq!(
            regenerated_512.as_slice(),
            TWIDDLES_512_Q23.as_slice(),
            "TWIDDLES_512_Q23 has drifted from build_twiddles_q23(512)'s own real output -- regenerate it"
        );
        let regenerated_1024 = build_twiddles_q23(FFT_ENC_SB);
        assert_eq!(
            regenerated_1024.as_slice(),
            TWIDDLES_1024_Q23.as_slice(),
            "TWIDDLES_1024_Q23 has drifted from build_twiddles_q23(1024)'s own real output -- regenerate it"
        );
    }

    #[test]
    // Tests [@ANCHOR: fft_fixed]
    // Tests [@ANCHOR: fft_bit_reverse_table]
    // Tests [@ANCHOR: fft_twiddles_q23]
    // Tests [@ANCHOR: build_bit_reverse_table]
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
            max_abs_err = max_abs_err
                .max((got_re - buf[i].re).abs())
                .max((got_im - buf[i].im).abs());
            max_ref_mag = max_ref_mag.max(buf[i].re.abs()).max(buf[i].im.abs());
        }
        assert!(
            max_ref_mag > 1.0,
            "sanity: reference spectrum shouldn't be near-zero"
        );
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
            .map(|i| {
                Complex32::new(
                    re[i] as f32 / (1i64 << FRAC_BITS) as f32,
                    im[i] as f32 / (1i64 << FRAC_BITS) as f32,
                )
            })
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
            max_abs_err = max_abs_err
                .max((got_re - float_buf[i].re).abs())
                .max((got_im - float_buf[i].im).abs());
            max_ref_mag = max_ref_mag
                .max(float_buf[i].re.abs())
                .max(float_buf[i].im.abs());
        }
        assert!(
            max_ref_mag > 1.0,
            "sanity: reconstructed signal shouldn't be near-zero"
        );
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

    /// `fft_fixed`'s own doc comment on "`i64`/`i128` headroom vastly
    /// exceeds this transform's real dynamic range" was measured at
    /// `FFT_ENC`=512 with up to `MAX_AMP`=80 populated bins -- this
    /// re-verifies it at the doubled `FFT_ENC_SB`=1024 size with up to
    /// `MAX_AMP_SB`/2=80 *newly populated* bins on top of the base
    /// harmonics (`spectral_bridge::extrapolate_amplitudes`'s own real
    /// `l2` ceiling), one more butterfly stage and twice the summed
    /// bins than the existing 4-tone fixture stresses.
    #[test]
    fn inverse_fft_fixed_at_fft_enc_sb_matches_rustfft_on_a_real_extended_harmonic_spectrum() {
        use super::super::spectral_bridge::{FFT_ENC_SB, MAX_AMP_SB};
        let l2 = MAX_AMP_SB / 2;
        let mut re = [0i64; FFT_ENC_SB];
        let mut im = [0i64; FFT_ENC_SB];
        let mut re_f = vec![0.0f32; FFT_ENC_SB];
        let mut im_f = vec![0.0f32; FFT_ENC_SB];
        let mut seed = 42u32;
        for m in 1..=l2 {
            let bin = (m * (FFT_ENC_SB / 2) / l2).min(FFT_ENC_SB / 2 - 1);
            let amp = 20000.0 * 0.98f32.powi(m as i32);
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let phase = (seed >> 16) as i16 as f32 / 32768.0 * std::f32::consts::PI;
            let (s, c) = phase.sin_cos();
            let (vr, vi) = (amp * c, amp * s);
            re[bin] = f32_to_q23(vr);
            im[bin] = f32_to_q23(vi);
            re_f[bin] = vr;
            im_f[bin] = vi;
        }
        for k in 1..(FFT_ENC_SB / 2) {
            re[FFT_ENC_SB - k] = re[k];
            im[FFT_ENC_SB - k] = -im[k];
            re_f[FFT_ENC_SB - k] = re_f[k];
            im_f[FFT_ENC_SB - k] = -im_f[k];
        }

        fft_fixed(&mut re, &mut im, false);

        let mut planner = FftPlanner::<f32>::new();
        let ifft = planner.plan_fft_inverse(FFT_ENC_SB);
        let mut buf: Vec<Complex32> = re_f
            .iter()
            .zip(im_f.iter())
            .map(|(&r, &i)| Complex32::new(r, i))
            .collect();
        ifft.process(&mut buf);

        let mut max_abs_err = 0.0f32;
        let mut max_ref_mag = 0.0f32;
        for i in 0..FFT_ENC_SB {
            let got_re = re[i] as f32 / (1i64 << FRAC_BITS) as f32;
            let got_im = im[i] as f32 / (1i64 << FRAC_BITS) as f32;
            max_abs_err = max_abs_err
                .max((got_re - buf[i].re).abs())
                .max((got_im - buf[i].im).abs());
            max_ref_mag = max_ref_mag.max(buf[i].re.abs()).max(buf[i].im.abs());
        }
        println!(
            "FFT_ENC_SB inverse vs rustfft: max_abs_err={max_abs_err}, max_ref_mag={max_ref_mag}, ratio={}",
            max_abs_err / max_ref_mag
        );
        assert!(
            max_ref_mag > 1.0,
            "sanity: reconstructed signal shouldn't be near-zero"
        );
        assert!(
            max_abs_err / max_ref_mag < 1e-4,
            "FFT_ENC_SB inverse fft_fixed diverged from rustfft's plan_fft_inverse: max_abs_err={max_abs_err}, max_ref_mag={max_ref_mag}"
        );
    }

    /// Reference for the optimized butterflies: the original textbook
    /// stage loop with `i128` products and no shortcuts whatsoever.
    fn reference_fft<const N: usize>(re: &mut [i64; N], im: &mut [i64; N], forward: bool)
    where
        Size<N>: FftTables,
    {
        let n = N;
        let bitrev = fft_bit_reverse_table::<N>();
        for (i, &j) in bitrev.iter().enumerate() {
            let j = j as usize;
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let twiddles = fft_twiddles_q23::<N>();
        let mut len = 2usize;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            let mut i = 0;
            while i < n {
                for j in 0..half {
                    let (wr, wi_fwd) = twiddles[j * step];
                    let (wr, wi_fwd) = (wr as i64, wi_fwd as i64);
                    let wi = if forward { wi_fwd } else { -wi_fwd };
                    let (br, bi) = (re[i + j + half], im[i + j + half]);
                    let vr = rshift_round_i128(
                        wr as i128 * br as i128 - wi as i128 * bi as i128,
                        FRAC_BITS,
                    );
                    let vi = rshift_round_i128(
                        wr as i128 * bi as i128 + wi as i128 * br as i128,
                        FRAC_BITS,
                    );
                    let (ar, ai) = (re[i + j], im[i + j]);
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

    fn lcg(seed: &mut u64) -> i64 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (*seed >> 16) as i64
    }

    fn dense_case<const N: usize>(seed: &mut u64)
    where
        Size<N>: FftTables,
    {
        for &mag_bits in &[8u32, 17, 18, 19, 24, 28, 29, 30, 33, 34, 36, 37, 38, 40, 44] {
            for &forward in &[true, false] {
                let mut re = [0i64; N];
                let mut im = [0i64; N];
                for k in 0..N {
                    re[k] = (lcg(seed) % (1i64 << mag_bits)) * if lcg(seed) & 1 == 0 { 1 } else { -1 };
                    im[k] = lcg(seed) % (1i64 << mag_bits);
                }
                let (mut re_r, mut im_r) = (re, im);
                fft_fixed::<N>(&mut re, &mut im, forward);
                reference_fft::<N>(&mut re_r, &mut im_r, forward);
                assert_eq!(re, re_r, "n={N} bits={mag_bits} fwd={forward}");
                assert_eq!(im, im_r, "n={N} bits={mag_bits} fwd={forward}");
            }
        }
    }

    /// The optimized dense transform (unchecked-`i64` fast path, exact
    /// twiddle shortcuts) must equal the textbook `i128` loop bit for
    /// bit, at both sizes, in both directions, on inputs small enough
    /// for the fast path and large enough to force the checked path.
    #[test]
    // Tests [@ANCHOR: fft_fixed]
    fn optimized_fft_is_bit_identical_to_the_textbook_i128_loop() {
        let mut seed = 42u64;
        dense_case::<FFT_ENC>(&mut seed);
        dense_case::<FFT_ENC_SB>(&mut seed);
    }

    fn prefix_case<const N: usize>(seed: &mut u64)
    where
        Size<N>: FftTables,
    {
        for &nz in &[1usize, 2, 3, 11, 64, 100, 255, 257, N] {
            for &mag_bits in &[10u32, 18, 19, 26, 34] {
                for &forward in &[true, false] {
                    let mut input = [0i64; N];
                    for v in input.iter_mut().take(nz) {
                        *v = (lcg(seed) % (1i64 << mag_bits)) * if lcg(seed) & 1 == 0 { 1 } else { -1 };
                    }
                    let mut re = [0x5a5a_5a5ai64; N];
                    let mut im = [-0x1234_5678i64; N];
                    let (mut re_r, mut im_r) = (input, [0i64; N]);
                    fft_fixed_sparse_prefix::<N>(&input[..nz], &mut re, &mut im, forward);
                    reference_fft::<N>(&mut re_r, &mut im_r, forward);
                    assert_eq!(re, re_r, "n={N} nz={nz} bits={mag_bits} fwd={forward}");
                    assert_eq!(im, im_r, "n={N} nz={nz} bits={mag_bits} fwd={forward}");
                }
            }
        }
    }

    /// The compile-time-`NZ` forward form (the production LPC-spectrum
    /// call) equals the dense reference, for several `NZ`, on both the
    /// 32-bit-kernel path and the large-input fallback, and does not care
    /// what the output arrays held before.
    fn prefix_const_case<const N: usize, const NZ: usize>(seed: &mut u64)
    where
        Size<N>: FftTables,
    {
        for &mag_bits in &[10u32, 22, 24, 26, 34, 40] {
            let mut input = [0i64; NZ];
            for v in input.iter_mut() {
                *v = (lcg(seed) % (1i64 << mag_bits)) * if lcg(seed) & 1 == 0 { 1 } else { -1 };
            }
            let mut re = [0x5a5a_5a5ai64; N];
            let mut im = [-0x1234_5678i64; N];
            let (mut re_r, mut im_r) = ([0i64; N], [0i64; N]);
            re_r[..NZ].copy_from_slice(&input);
            fft_fixed_sparse_prefix_forward::<N, NZ>(&input, &mut re, &mut im);
            reference_fft::<N>(&mut re_r, &mut im_r, true);
            assert_eq!(re, re_r, "n={N} nz={NZ} bits={mag_bits}");
            assert_eq!(im, im_r, "n={N} nz={NZ} bits={mag_bits}");
        }
    }

    /// `fft_fixed_sparse_prefix` (skipped/copied blocks) equals the dense
    /// reference on real inputs with only the first `nz` entries set.
    #[test]
    // Tests [@ANCHOR: fft_fixed_sparse_prefix]
    fn sparse_prefix_fft_is_bit_identical_to_the_dense_reference() {
        let mut seed = 7u64;
        prefix_case::<FFT_ENC>(&mut seed);
        prefix_case::<FFT_ENC_SB>(&mut seed);
        prefix_const_case::<FFT_ENC, 1>(&mut seed);
        prefix_const_case::<FFT_ENC, 3>(&mut seed);
        prefix_const_case::<FFT_ENC, 11>(&mut seed);
        prefix_const_case::<FFT_ENC, 13>(&mut seed);
        prefix_const_case::<FFT_ENC, 100>(&mut seed);
        prefix_const_case::<FFT_ENC_SB, 11>(&mut seed);
    }

    /// One trial of the sparse harmonic inverse: build the dense spectrum
    /// the way the old synthesis fill did (later bins overwrite earlier
    /// ones, then mirror), run the textbook inverse, and compare the
    /// outputs the overlap-add reads.
    fn sparse_inverse_trial<const N: usize, const NS: usize>(bins: &[(usize, ComplexQ23)])
    where
        Size<N>: FftTables,
    {
        let mut re_ref = [0i64; N];
        let mut im_ref = [0i64; N];
        for &(b, v) in bins {
            re_ref[b] = v.re;
            im_ref[b] = v.im;
        }
        for k in 1..N / 2 {
            re_ref[N - k] = re_ref[k];
            im_ref[N - k] = -im_ref[k];
        }
        reference_fft::<N>(&mut re_ref, &mut im_ref, false);

        // Garbage in the buffers on entry: nothing may depend on it.
        let mut re = [0x1357_9bdfi64; N];
        let mut im = [-0x2468_ace0i64; N];
        let mut sp = SparseInverse::<N>::new(&mut re, &mut im);
        for &(b, v) in bins {
            sp.put(b, v);
        }
        sp.run::<NS>();
        for j in 0..=NS {
            assert_eq!(re[j], re_ref[j], "n={N} lower output {j}, {} bins", bins.len());
        }
        for j in N - NS + 1..N {
            assert_eq!(re[j], re_ref[j], "n={N} upper output {j}, {} bins", bins.len());
        }
    }

    fn sparse_inverse_case<const N: usize, const NS: usize>(seed: &mut u64)
    where
        Size<N>: FftTables,
    {
        let rnd = |seed: &mut u64, bits: u32| -> i64 {
            (lcg(seed) % (1i64 << bits)) * if lcg(seed) & 1 == 0 { 1 } else { -1 }
        };
        for &mag_bits in &[10u32, 24, 32, 34, 36, 37, 38, 40] {
            // Random harmonic counts, including none, one, and dense; the
            // bin clamp of the real callers is `N / 2 - 1`.
            for &count in &[0usize, 1, 2, 5, 17, 40, 80, 160] {
                let bins: Vec<(usize, ComplexQ23)> = (0..count)
                    .map(|_| {
                        let b = 1 + (lcg(seed) as usize) % (N / 2 - 1);
                        (b, ComplexQ23 { re: rnd(seed, mag_bits), im: rnd(seed, mag_bits) })
                    })
                    .collect();
                sparse_inverse_trial::<N, NS>(&bins);
            }
            // Structured layouts: only even bins, only odd bins (one whole
            // half of the last stage empty), regularly spaced harmonics
            // like the real callers, and a repeated bin.
            for parity in 0..2usize {
                let bins: Vec<(usize, ComplexQ23)> = (1..N / 2 - 1)
                    .filter(|b| b % 2 == parity && b % 6 == 1 + parity)
                    .map(|b| (b, ComplexQ23 { re: rnd(seed, mag_bits), im: rnd(seed, mag_bits) }))
                    .collect();
                sparse_inverse_trial::<N, NS>(&bins);
            }
            let comb: Vec<(usize, ComplexQ23)> = (1..)
                .map(|l| ((l * 37 + 8) / 10, l))
                .take_while(|&(b, _)| b < N / 2)
                .map(|(b, _)| (b, ComplexQ23 { re: rnd(seed, mag_bits), im: rnd(seed, mag_bits) }))
                .collect();
            sparse_inverse_trial::<N, NS>(&comb);
            let v = ComplexQ23 { re: rnd(seed, mag_bits), im: rnd(seed, mag_bits) };
            sparse_inverse_trial::<N, NS>(&[(3, v), (3, ComplexQ23 { re: v.im, im: v.re })]);
            sparse_inverse_trial::<N, NS>(&[(N / 2 - 1, v)]);
        }
    }

    /// Constructive worst case at the 64-bit-kernel limit: many bins with
    /// the same real value, so the transform's output at index 0 reaches
    /// the whole input sum. Value sums just below and just above the
    /// `I64_SUM_LIMIT` switch, at both sizes.
    fn sparse_inverse_peak_case<const N: usize, const NS: usize>()
    where
        Size<N>: FftTables,
    {
        for &count in &[3usize, 10, 40, 100] {
            for delta in [-64i64, -1, 0, 1, 64] {
                // sum over the puts = 2 * count * |v| (mirror included)
                let v = (((I64_SUM_LIMIT as i64) / (2 * count as i64)) + delta).max(1);
                let bins: Vec<(usize, ComplexQ23)> = (0..count)
                    .map(|k| (1 + k * ((N / 2 - 2) / count), ComplexQ23 { re: v, im: 0 }))
                    .collect();
                sparse_inverse_trial::<N, NS>(&bins);
                let bins_neg: Vec<(usize, ComplexQ23)> = bins
                    .iter()
                    .map(|&(b, c)| (b, ComplexQ23 { re: -c.re / 2, im: c.re / 2 }))
                    .collect();
                sparse_inverse_trial::<N, NS>(&bins_neg);
            }
        }
    }

    #[test]
    fn sparse_harmonic_inverse_is_exact_at_the_limit_of_the_64_bit_kernel() {
        sparse_inverse_peak_case::<FFT_ENC, 80>();
        sparse_inverse_peak_case::<FFT_ENC_SB, 160>();
    }

    /// The sparse harmonic inverse (occupancy-flag block skipping, scattered
    /// bit-reversed input, pruned last stage, uncleared buffers) equals the
    /// dense textbook inverse at every output the synthesis reads, at both
    /// sizes, across input magnitudes that select the fast and checked
    /// kernels.
    #[test]
    fn sparse_harmonic_inverse_is_bit_identical_to_the_dense_reference_at_the_used_outputs() {
        let mut seed = 2026u64;
        sparse_inverse_case::<FFT_ENC, 80>(&mut seed);
        sparse_inverse_case::<FFT_ENC_SB, 160>(&mut seed);
    }

    /// Fast-path complex multiply and squared magnitude equal the exact
    /// `i128` forms on both sides of every fast-path threshold.
    #[test]
    fn complex_mul_and_mag_sq_fast_paths_match_the_i128_forms() {
        let mut seed = 99u64;
        for &bits in &[8u32, 20, 29, 30, 31, 32, 36, 40] {
            for _ in 0..500 {
                let mut r = || (lcg(&mut seed) % (1i64 << bits)) * if lcg(&mut seed) & 1 == 0 { 1 } else { -1 };
                let (a, b) = (ComplexQ23 { re: r(), im: r() }, ComplexQ23 { re: r(), im: r() });
                let m = a.mul(b);
                let re = rshift_round_i128(a.re as i128 * b.re as i128 - a.im as i128 * b.im as i128, FRAC_BITS);
                let im = rshift_round_i128(a.re as i128 * b.im as i128 + a.im as i128 * b.re as i128, FRAC_BITS);
                assert_eq!((m.re, m.im), (re, im), "mul bits={bits}");
                assert_eq!(a.mag_sq_raw(), a.re as i128 * a.re as i128 + a.im as i128 * a.im as i128, "mag_sq bits={bits}");
            }
        }
    }
}
