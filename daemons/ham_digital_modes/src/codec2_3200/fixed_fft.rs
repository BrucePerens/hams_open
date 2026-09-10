// SPDX-License-Identifier: LGPL-3.0-or-later
//! Genuinely fixed-point, phase-correct radix-2 FFT for the decoder's
//! own `FFT_ENC`=512-point transforms: `envelope.rs`'s forward analysis
//! (`ak[]` -> `Aw[]`) and `synthesis.rs`'s inverse synthesis (a sparse
//! harmonic spectrum -> time-domain samples). Also serves
//! `spectral_bridge.rs`'s own doubled `FFT_ENC_SB`=1024-point inverse
//! synthesis (`fft_fixed` takes a runtime size, not just `FFT_ENC` --
//! see that function's own doc comment); this module imports
//! `spectral_bridge::FFT_ENC_SB` for its cached-table lookup, so it now
//! depends on that module even though this doc comment predates it.
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

#[cfg(test)]
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
        ComplexQ23 {
            re: self.re,
            im: -self.im,
        }
    }

    /// Complex multiply, `i128`-widened then rescaled back to Q23 --
    /// same accumulate-then-narrow pattern this port uses throughout.
    // [@ANCHOR: ComplexQ23::mul]
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
/// function itself is `f32`-transcendental-based (`cos`/`sin`) and is
/// therefore *not* itself part of the fixed-point production path -- see
/// the module doc comment's own no-heap-allocation rationale for why the
/// two tables below exist as `const` data rather than a lazily-built
/// `Vec` computed by calling this at runtime, which is what this
/// function replaced.
#[cfg(test)]
fn build_twiddles_q23(n: usize) -> Vec<(i64, i64)> {
    (0..n / 2)
        .map(|k| {
            let theta = -std::f32::consts::TAU * k as f32 / n as f32;
            (f32_to_q23(theta.cos()), f32_to_q23(theta.sin()))
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
    (8387977, -102941),
    (8386082, -205867),
    (8382924, -308761),
    (8378504, -411610),
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
    (8137212, -2038266),
    (8111587, -2137968),
    (8084740, -2237349),
    (8056675, -2336393),
    (8027398, -2435085),
    (7996911, -2533410),
    (7965220, -2631353),
    (7932329, -2728901),
    (7898244, -2826037),
    (7862970, -2922748),
    (7826511, -3019019),
    (7788874, -3114835),
    (7750063, -3210182),
    (7710086, -3305045),
    (7668947, -3399411),
    (7626654, -3493265),
    (7583212, -3586592),
    (7538628, -3679380),
    (7492909, -3771613),
    (7446061, -3863279),
    (7398092, -3954363),
    (7349009, -4044851),
    (7298819, -4134730),
    (7247530, -4223986),
    (7195149, -4312607),
    (7141685, -4400578),
    (7087146, -4487885),
    (7031539, -4574518),
    (6974873, -4660461),
    (6917157, -4745703),
    (6858399, -4830230),
    (6798608, -4914029),
    (6737793, -4997088),
    (6675964, -5079395),
    (6613129, -5160937),
    (6549299, -5241702),
    (6484482, -5321677),
    (6418688, -5400851),
    (6351928, -5479211),
    (6284212, -5556747),
    (6215549, -5633445),
    (6145950, -5709295),
    (6075425, -5784286),
    (6003985, -5858405),
    (5931642, -5931642),
    (5858405, -6003986),
    (5784285, -6075425),
    (5709295, -6145950),
    (5633445, -6215549),
    (5556746, -6284212),
    (5479211, -6351929),
    (5400851, -6418689),
    (5321677, -6484482),
    (5241701, -6549299),
    (5160937, -6613129),
    (5079395, -6675964),
    (4997088, -6737793),
    (4914029, -6798608),
    (4830229, -6858399),
    (4745703, -6917157),
    (4660461, -6974873),
    (4574518, -7031539),
    (4487886, -7087146),
    (4400577, -7141685),
    (4312606, -7195150),
    (4223987, -7247530),
    (4134730, -7298819),
    (4044850, -7349009),
    (3954362, -7398092),
    (3863279, -7446061),
    (3771613, -7492909),
    (3679379, -7538628),
    (3586592, -7583212),
    (3493264, -7626654),
    (3399410, -7668948),
    (3305044, -7710086),
    (3210181, -7750063),
    (3114834, -7788874),
    (3019018, -7826511),
    (2922748, -7862970),
    (2826037, -7898244),
    (2728900, -7932330),
    (2631353, -7965220),
    (2533410, -7996911),
    (2435084, -8027398),
    (2336392, -8056676),
    (2237349, -8084740),
    (2137968, -8111587),
    (2038265, -8137212),
    (1938256, -8161612),
    (1837954, -8184783),
    (1737376, -8206721),
    (1636536, -8227424),
    (1535450, -8246887),
    (1434132, -8265108),
    (1332598, -8282085),
    (1230865, -8297814),
    (1128945, -8312294),
    (1026855, -8325522),
    (924610, -8337496),
    (822227, -8348215),
    (719720, -8357676),
    (617104, -8365879),
    (514396, -8372822),
    (411609, -8378504),
    (308761, -8382924),
    (205866, -8386082),
    (102941, -8387977),
    (0, -8388608),
    (-102942, -8387977),
    (-205867, -8386082),
    (-308762, -8382924),
    (-411610, -8378504),
    (-514396, -8372822),
    (-617104, -8365879),
    (-719720, -8357676),
    (-822228, -8348215),
    (-924611, -8337496),
    (-1026855, -8325522),
    (-1128945, -8312294),
    (-1230865, -8297814),
    (-1332599, -8282085),
    (-1434133, -8265108),
    (-1535451, -8246887),
    (-1636536, -8227423),
    (-1737377, -8206721),
    (-1837955, -8184783),
    (-1938257, -8161612),
    (-2038266, -8137212),
    (-2137969, -8111587),
    (-2237350, -8084740),
    (-2336393, -8056675),
    (-2435085, -8027397),
    (-2533410, -7996911),
    (-2631353, -7965220),
    (-2728901, -7932329),
    (-2826038, -7898244),
    (-2922749, -7862969),
    (-3019019, -7826511),
    (-3114835, -7788874),
    (-3210182, -7750063),
    (-3305045, -7710086),
    (-3399411, -7668947),
    (-3493264, -7626654),
    (-3586592, -7583212),
    (-3679380, -7538628),
    (-3771614, -7492909),
    (-3863279, -7446061),
    (-3954363, -7398092),
    (-4044852, -7349008),
    (-4134730, -7298819),
    (-4223986, -7247530),
    (-4312607, -7195149),
    (-4400578, -7141685),
    (-4487886, -7087145),
    (-4574519, -7031538),
    (-4660462, -6974872),
    (-4745702, -6917157),
    (-4830229, -6858399),
    (-4914029, -6798608),
    (-4997089, -6737793),
    (-5079396, -6675964),
    (-5160938, -6613129),
    (-5241703, -6549298),
    (-5321677, -6484482),
    (-5400851, -6418688),
    (-5479211, -6351928),
    (-5556747, -6284211),
    (-5633446, -6215548),
    (-5709296, -6145949),
    (-5784287, -6075424),
    (-5858405, -6003986),
    (-5931642, -5931642),
    (-6003986, -5858405),
    (-6075426, -5784285),
    (-6145950, -5709295),
    (-6215550, -5633444),
    (-6284213, -5556745),
    (-6351928, -5479211),
    (-6418689, -5400851),
    (-6484482, -5321677),
    (-6549299, -5241701),
    (-6613130, -5160936),
    (-6675965, -5079394),
    (-6737794, -4997087),
    (-6798608, -4914029),
    (-6858399, -4830229),
    (-6917157, -4745702),
    (-6974873, -4660461),
    (-7031539, -4574517),
    (-7087146, -4487884),
    (-7141686, -4400576),
    (-7195149, -4312607),
    (-7247530, -4223986),
    (-7298819, -4134729),
    (-7349009, -4044850),
    (-7398093, -3954362),
    (-7446062, -3863278),
    (-7492909, -3771614),
    (-7538628, -3679380),
    (-7583212, -3586592),
    (-7626654, -3493264),
    (-7668948, -3399410),
    (-7710086, -3305044),
    (-7750064, -3210180),
    (-7788874, -3114835),
    (-7826511, -3019019),
    (-7862970, -2922748),
    (-7898245, -2826037),
    (-7932330, -2728900),
    (-7965220, -2631352),
    (-7996911, -2533408),
    (-8027398, -2435085),
    (-8056675, -2336393),
    (-8084740, -2237349),
    (-8111587, -2137968),
    (-8137212, -2038265),
    (-8161612, -1938255),
    (-8184783, -1837953),
    (-8206721, -1737376),
    (-8227424, -1636536),
    (-8246887, -1535450),
    (-8265108, -1434132),
    (-8282085, -1332598),
    (-8297814, -1230863),
    (-8312294, -1128943),
    (-8325522, -1026855),
    (-8337496, -924611),
    (-8348215, -822227),
    (-8357676, -719719),
    (-8365879, -617103),
    (-8372822, -514394),
    (-8378504, -411608),
    (-8382924, -308762),
    (-8386082, -205867),
    (-8387977, -102941),
];

/// Twiddle factors for `FFT_ENC_SB`=1024 (`spectral_bridge.rs`'s own
/// doubled-resolution transform) -- same generation/checked-in-data
/// convention as [`TWIDDLES_512_Q23`] above, same validating test.
const TWIDDLES_1024_Q23: [(i64, i64); 512] = [
    (8388608, 0),
    (8388450, -51472),
    (8387977, -102941),
    (8387187, -154407),
    (8386082, -205867),
    (8384661, -257319),
    (8382924, -308761),
    (8380872, -360192),
    (8378504, -411610),
    (8375820, -463011),
    (8372822, -514396),
    (8369508, -565761),
    (8365879, -617104),
    (8361935, -668425),
    (8357676, -719720),
    (8353103, -770988),
    (8348215, -822227),
    (8343013, -873436),
    (8337496, -924611),
    (8331666, -975751),
    (8325522, -1026855),
    (8319065, -1077920),
    (8312294, -1128945),
    (8305210, -1179927),
    (8297814, -1230864),
    (8290106, -1281756),
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
    (8149566, -1988298),
    (8137212, -2038266),
    (8124552, -2088156),
    (8111587, -2137968),
    (8098316, -2187700),
    (8084740, -2237349),
    (8070860, -2286914),
    (8056675, -2336393),
    (8042188, -2385784),
    (8027398, -2435085),
    (8012305, -2484294),
    (7996911, -2533410),
    (7981216, -2582430),
    (7965220, -2631353),
    (7948924, -2680177),
    (7932329, -2728901),
    (7915436, -2777521),
    (7898244, -2826037),
    (7880755, -2874447),
    (7862970, -2922748),
    (7844888, -2970939),
    (7826511, -3019019),
    (7807839, -3066984),
    (7788874, -3114835),
    (7769615, -3162568),
    (7750063, -3210182),
    (7730220, -3257674),
    (7710086, -3305045),
    (7689662, -3352291),
    (7668947, -3399411),
    (7647945, -3446403),
    (7626654, -3493265),
    (7605076, -3539995),
    (7583212, -3586592),
    (7561062, -3633055),
    (7538628, -3679380),
    (7515910, -3725567),
    (7492909, -3771613),
    (7469626, -3817518),
    (7446061, -3863279),
    (7422216, -3908894),
    (7398092, -3954363),
    (7373689, -3999682),
    (7349009, -4044851),
    (7324052, -4089867),
    (7298819, -4134730),
    (7273311, -4179437),
    (7247530, -4223986),
    (7221475, -4268377),
    (7195149, -4312607),
    (7168552, -4356674),
    (7141685, -4400578),
    (7114549, -4444315),
    (7087146, -4487885),
    (7059475, -4531287),
    (7031539, -4574518),
    (7003337, -4617577),
    (6974873, -4660461),
    (6946145, -4703171),
    (6917157, -4745703),
    (6887907, -4788056),
    (6858399, -4830230),
    (6828632, -4872221),
    (6798608, -4914029),
    (6768328, -4955652),
    (6737793, -4997088),
    (6707005, -5038337),
    (6675964, -5079395),
    (6644672, -5120262),
    (6613129, -5160937),
    (6581338, -5201417),
    (6549299, -5241702),
    (6517013, -5281789),
    (6484482, -5321677),
    (6451707, -5361365),
    (6418688, -5400851),
    (6385428, -5440134),
    (6351928, -5479211),
    (6318189, -5518083),
    (6284212, -5556747),
    (6249998, -5595201),
    (6215549, -5633445),
    (6180866, -5671477),
    (6145950, -5709295),
    (6110802, -5746899),
    (6075425, -5784286),
    (6039819, -5821455),
    (6003985, -5858405),
    (5967926, -5895134),
    (5931642, -5931642),
    (5895134, -5967926),
    (5858405, -6003986),
    (5821455, -6039819),
    (5784285, -6075425),
    (5746898, -6110803),
    (5709295, -6145950),
    (5671477, -6180866),
    (5633445, -6215549),
    (5595201, -6249998),
    (5556746, -6284212),
    (5518083, -6318189),
    (5479211, -6351929),
    (5440133, -6385429),
    (5400851, -6418689),
    (5361364, -6451707),
    (5321677, -6484482),
    (5281789, -6517013),
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
    (4788056, -6887908),
    (4745703, -6917157),
    (4703170, -6946146),
    (4660461, -6974873),
    (4617576, -7003338),
    (4574518, -7031539),
    (4531287, -7059475),
    (4487886, -7087146),
    (4444315, -7114549),
    (4400577, -7141685),
    (4356674, -7168552),
    (4312606, -7195150),
    (4268376, -7221476),
    (4223987, -7247530),
    (4179437, -7273311),
    (4134730, -7298819),
    (4089867, -7324052),
    (4044850, -7349009),
    (3999681, -7373690),
    (3954362, -7398092),
    (3908894, -7422216),
    (3863279, -7446061),
    (3817518, -7469626),
    (3771613, -7492909),
    (3725566, -7515910),
    (3679379, -7538628),
    (3633054, -7561063),
    (3586592, -7583212),
    (3539995, -7605076),
    (3493264, -7626654),
    (3446402, -7647945),
    (3399410, -7668948),
    (3352290, -7689662),
    (3305044, -7710086),
    (3257675, -7730220),
    (3210181, -7750063),
    (3162567, -7769615),
    (3114834, -7788874),
    (3066984, -7807840),
    (3019018, -7826511),
    (2970938, -7844888),
    (2922748, -7862970),
    (2874447, -7880755),
    (2826037, -7898244),
    (2777521, -7915436),
    (2728900, -7932330),
    (2680177, -7948925),
    (2631353, -7965220),
    (2582430, -7981216),
    (2533410, -7996911),
    (2484294, -8012305),
    (2435084, -8027398),
    (2385783, -8042188),
    (2336392, -8056676),
    (2286913, -8070860),
    (2237349, -8084740),
    (2187700, -8098316),
    (2137968, -8111587),
    (2088156, -8124553),
    (2038265, -8137212),
    (1988298, -8149566),
    (1938256, -8161612),
    (1888141, -8173351),
    (1837954, -8184783),
    (1787699, -8195906),
    (1737376, -8206721),
    (1686987, -8217227),
    (1636536, -8227424),
    (1586023, -8237310),
    (1535450, -8246887),
    (1484819, -8256153),
    (1434132, -8265108),
    (1383391, -8273752),
    (1332598, -8282085),
    (1281755, -8290106),
    (1230865, -8297814),
    (1179927, -8305210),
    (1128945, -8312294),
    (1077920, -8319065),
    (1026855, -8325522),
    (975751, -8331666),
    (924610, -8337496),
    (873436, -8343013),
    (822227, -8348215),
    (770988, -8353103),
    (719720, -8357676),
    (668424, -8361935),
    (617104, -8365879),
    (565760, -8369508),
    (514396, -8372822),
    (463011, -8375820),
    (411609, -8378504),
    (360192, -8380872),
    (308761, -8382924),
    (257318, -8384661),
    (205866, -8386082),
    (154407, -8387187),
    (102941, -8387977),
    (51471, -8388450),
    (0, -8388608),
    (-51472, -8388450),
    (-102942, -8387977),
    (-154408, -8387187),
    (-205867, -8386082),
    (-257319, -8384661),
    (-308762, -8382924),
    (-360193, -8380872),
    (-411610, -8378504),
    (-463012, -8375820),
    (-514396, -8372822),
    (-565761, -8369508),
    (-617104, -8365879),
    (-668425, -8361935),
    (-719720, -8357676),
    (-770989, -8353103),
    (-822228, -8348215),
    (-873436, -8343012),
    (-924611, -8337496),
    (-975752, -8331666),
    (-1026855, -8325522),
    (-1077921, -8319065),
    (-1128945, -8312294),
    (-1179928, -8305210),
    (-1230865, -8297814),
    (-1281756, -8290106),
    (-1332599, -8282085),
    (-1383392, -8273752),
    (-1434133, -8265108),
    (-1484820, -8256153),
    (-1535451, -8246887),
    (-1586024, -8237310),
    (-1636536, -8227423),
    (-1686988, -8217227),
    (-1737377, -8206721),
    (-1787699, -8195906),
    (-1837955, -8184783),
    (-1888142, -8173351),
    (-1938257, -8161612),
    (-1988298, -8149566),
    (-2038266, -8137212),
    (-2088157, -8124552),
    (-2137969, -8111587),
    (-2187700, -8098316),
    (-2237350, -8084740),
    (-2286914, -8070860),
    (-2336393, -8056675),
    (-2385784, -8042188),
    (-2435085, -8027397),
    (-2484294, -8012305),
    (-2533410, -7996911),
    (-2582431, -7981215),
    (-2631353, -7965220),
    (-2680178, -7948924),
    (-2728901, -7932329),
    (-2777522, -7915436),
    (-2826038, -7898244),
    (-2874447, -7880755),
    (-2922749, -7862969),
    (-2970939, -7844888),
    (-3019019, -7826511),
    (-3066985, -7807839),
    (-3114835, -7788874),
    (-3162568, -7769615),
    (-3210182, -7750063),
    (-3257675, -7730220),
    (-3305045, -7710086),
    (-3352291, -7689661),
    (-3399411, -7668947),
    (-3446403, -7647945),
    (-3493264, -7626654),
    (-3539995, -7605076),
    (-3586592, -7583212),
    (-3633054, -7561062),
    (-3679380, -7538628),
    (-3725567, -7515910),
    (-3771614, -7492909),
    (-3817518, -7469625),
    (-3863279, -7446061),
    (-3908895, -7422216),
    (-3954363, -7398092),
    (-3999683, -7373689),
    (-4044852, -7349008),
    (-4089869, -7324051),
    (-4134730, -7298819),
    (-4179437, -7273311),
    (-4223986, -7247530),
    (-4268377, -7221476),
    (-4312607, -7195149),
    (-4356674, -7168552),
    (-4400578, -7141685),
    (-4444316, -7114549),
    (-4487886, -7087145),
    (-4531288, -7059475),
    (-4574519, -7031538),
    (-4617577, -7003337),
    (-4660462, -6974872),
    (-4703170, -6946146),
    (-4745702, -6917157),
    (-4788056, -6887907),
    (-4830229, -6858399),
    (-4872221, -6828632),
    (-4914029, -6798608),
    (-4955652, -6768328),
    (-4997089, -6737793),
    (-5038337, -6707005),
    (-5079396, -6675964),
    (-5120263, -6644671),
    (-5160938, -6613129),
    (-5201418, -6581337),
    (-5241703, -6549298),
    (-5281788, -6517013),
    (-5321677, -6484482),
    (-5361365, -6451707),
    (-5400851, -6418688),
    (-5440133, -6385429),
    (-5479211, -6351928),
    (-5518083, -6318189),
    (-5556747, -6284211),
    (-5595202, -6249997),
    (-5633446, -6215548),
    (-5671478, -6180865),
    (-5709296, -6145949),
    (-5746900, -6110802),
    (-5784287, -6075424),
    (-5821455, -6039819),
    (-5858405, -6003986),
    (-5895134, -5967926),
    (-5931642, -5931642),
    (-5967926, -5895134),
    (-6003986, -5858405),
    (-6039819, -5821454),
    (-6075426, -5784285),
    (-6110803, -5746898),
    (-6145950, -5709295),
    (-6180866, -5671476),
    (-6215550, -5633444),
    (-6249999, -5595200),
    (-6284213, -5556745),
    (-6318189, -5518083),
    (-6351928, -5479211),
    (-6385429, -5440133),
    (-6418689, -5400851),
    (-6451707, -5361365),
    (-6484482, -5321677),
    (-6517013, -5281788),
    (-6549299, -5241701),
    (-6581339, -5201416),
    (-6613130, -5160936),
    (-6644673, -5120261),
    (-6675965, -5079394),
    (-6707006, -5038335),
    (-6737794, -4997087),
    (-6768328, -4955652),
    (-6798608, -4914029),
    (-6828632, -4872221),
    (-6858399, -4830229),
    (-6887908, -4788056),
    (-6917157, -4745702),
    (-6946146, -4703170),
    (-6974873, -4660461),
    (-7003338, -4617576),
    (-7031539, -4574517),
    (-7059476, -4531286),
    (-7087146, -4487884),
    (-7114550, -4444314),
    (-7141686, -4400576),
    (-7168552, -4356674),
    (-7195149, -4312607),
    (-7221476, -4268377),
    (-7247530, -4223986),
    (-7273311, -4179436),
    (-7298819, -4134729),
    (-7324052, -4089867),
    (-7349009, -4044850),
    (-7373690, -3999681),
    (-7398093, -3954362),
    (-7422217, -3908893),
    (-7446062, -3863278),
    (-7469626, -3817517),
    (-7492909, -3771614),
    (-7515910, -3725567),
    (-7538628, -3679380),
    (-7561062, -3633054),
    (-7583212, -3586592),
    (-7605076, -3539995),
    (-7626654, -3493264),
    (-7647945, -3446402),
    (-7668948, -3399410),
    (-7689662, -3352290),
    (-7710086, -3305044),
    (-7730221, -3257673),
    (-7750064, -3210180),
    (-7769615, -3162566),
    (-7788874, -3114835),
    (-7807839, -3066984),
    (-7826511, -3019019),
    (-7844888, -2970939),
    (-7862970, -2922748),
    (-7880756, -2874446),
    (-7898245, -2826037),
    (-7915436, -2777520),
    (-7932330, -2728900),
    (-7948925, -2680176),
    (-7965220, -2631352),
    (-7981216, -2582429),
    (-7996911, -2533408),
    (-8012306, -2484292),
    (-8027398, -2435085),
    (-8042188, -2385784),
    (-8056675, -2336393),
    (-8070860, -2286914),
    (-8084740, -2237349),
    (-8098316, -2187699),
    (-8111587, -2137968),
    (-8124553, -2088155),
    (-8137212, -2038265),
    (-8149566, -1988297),
    (-8161612, -1938255),
    (-8173352, -1888139),
    (-8184783, -1837953),
    (-8195907, -1787697),
    (-8206721, -1737376),
    (-8217227, -1686988),
    (-8227424, -1636536),
    (-8237310, -1586023),
    (-8246887, -1535450),
    (-8256153, -1484819),
    (-8265108, -1434132),
    (-8273752, -1383391),
    (-8282085, -1332598),
    (-8290106, -1281755),
    (-8297814, -1230863),
    (-8305211, -1179926),
    (-8312294, -1128943),
    (-8319065, -1077919),
    (-8325522, -1026855),
    (-8331666, -975751),
    (-8337496, -924611),
    (-8343013, -873435),
    (-8348215, -822227),
    (-8353103, -770988),
    (-8357676, -719719),
    (-8361935, -668424),
    (-8365879, -617103),
    (-8369508, -565760),
    (-8372822, -514394),
    (-8375821, -463010),
    (-8378504, -411608),
    (-8380872, -360191),
    (-8382924, -308762),
    (-8384661, -257319),
    (-8386082, -205867),
    (-8387187, -154407),
    (-8387977, -102941),
    (-8388450, -51471),
];

/// Bit-reversal permutation table, `N` entries, `bits = N.trailing_zeros()`
/// wide -- pure integer arithmetic (unlike the twiddle tables above), so
/// this is computed for real at compile time as a `const fn`, not
/// generated offline and checked in: there's no drift risk to guard a
/// test against, and no runtime cost or heap allocation either way.
// [@ANCHOR: build_bit_reverse_table]
const fn build_bit_reverse_table<const N: usize>() -> [usize; N] {
    let bits = (N as u32).trailing_zeros();
    let mut table = [0usize; N];
    let mut i = 0;
    while i < N {
        table[i] = ((i as u32).reverse_bits() >> (32 - bits)) as usize;
        i += 1;
    }
    table
}

const BIT_REVERSE_512: [usize; FFT_ENC] = build_bit_reverse_table::<FFT_ENC>();
const BIT_REVERSE_1024: [usize; FFT_ENC_SB] = build_bit_reverse_table::<FFT_ENC_SB>();

/// Twiddle/bit-reversal tables for the two real FFT sizes this port
/// ever needs (`FFT_ENC`=512, and `spectral_bridge.rs`'s own doubled
/// `FFT_ENC_SB`=1024). Plain `const` data, not a lazily-built `Vec`
/// behind an `OnceLock` (this function's own earlier form) -- that
/// pattern needs `std`/a heap allocator, which a genuinely FPU-less
/// embedded target (this whole fixed-point port's actual reason for
/// existing) may not have at all; a size this function hasn't been
/// built a table for is a programming error, not a runtime condition
/// to handle gracefully.
// [@ANCHOR: fft_twiddles_q23]
fn fft_twiddles_q23(n: usize) -> &'static [(i64, i64)] {
    match n {
        FFT_ENC => &TWIDDLES_512_Q23,
        FFT_ENC_SB => &TWIDDLES_1024_Q23,
        _ => panic!("fft_twiddles_q23: unsupported FFT size {n} (only {FFT_ENC} and {FFT_ENC_SB} have cached tables)"),
    }
}

// [@ANCHOR: fft_bit_reverse_table]
fn fft_bit_reverse_table(n: usize) -> &'static [usize] {
    match n {
        FFT_ENC => &BIT_REVERSE_512,
        FFT_ENC_SB => &BIT_REVERSE_1024,
        _ => panic!("fft_bit_reverse_table: unsupported FFT size {n} (only {FFT_ENC} and {FFT_ENC_SB} have cached tables)"),
    }
}

/// In-place radix-2 decimation-in-time FFT, Q23 fixed-point throughout
/// (no `f32` inside the transform itself -- only the one-time twiddle-
/// table construction above uses float, the same "table construction
/// isn't the hot path" convention this port uses elsewhere). No
/// per-stage rescaling: `i64`/`i128` headroom vastly exceeds this
/// transform's real dynamic range (LPC-spectrum and sparse-harmonic-
/// spectrum inputs, not full-scale noise), the same reasoning `nlp.rs`'s
/// own `fft_fixed` documents for its own, differently-scaled input --
/// re-verified, not just inherited, at the doubled `FFT_ENC_SB` size by
/// this module's own `spectral_bridge_size_matches_rustfft_on_a_real_
/// extended_harmonic_spectrum` test, which stresses a real extended
/// (up to `MAX_AMP_SB`-harmonic) spectrum rather than the 4-tone
/// fixture the original 512-point tests use.
///
/// Takes plain slices at a runtime size `n = re.len()` (a power of two,
/// `debug_assert`ed) rather than a `[i64; FFT_ENC]`-shaped array --
/// genuinely the same algorithm at two different sizes for the same
/// semantic use (phase-correct spectral synthesis), unlike `nlp.rs`'s
/// own separate `fft_fixed`, which exists apart from this one because
/// it serves a *different* consumer with different phase-correctness
/// needs, not merely a different size (see this module's own doc
/// comment above).
// [@ANCHOR: fft_fixed]
pub(crate) fn fft_fixed(re: &mut [i64], im: &mut [i64], forward: bool) {
    let n = re.len();
    debug_assert!(
        n.is_power_of_two(),
        "fft_fixed: n={n} must be a power of two"
    );
    debug_assert_eq!(im.len(), n, "fft_fixed: re/im length mismatch");

    let bitrev = fft_bit_reverse_table(n);
    for (i, &j) in bitrev.iter().enumerate() {
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let twiddles = fft_twiddles_q23(n);
    let mut len = 2usize;
    while len <= n {
        let half = len / 2;
        let step = n / len;
        let mut i = 0;
        while i < n {
            for j in 0..half {
                let (wr, wi_fwd) = twiddles[j * step];
                let wi = if forward { wi_fwd } else { -wi_fwd };
                let br = re[i + j + half];
                let bi = im[i + j + half];
                let vr =
                    rshift_round_i128(wr as i128 * br as i128 - wi as i128 * bi as i128, FRAC_BITS);
                let vi =
                    rshift_round_i128(wr as i128 * bi as i128 + wi as i128 * br as i128, FRAC_BITS);
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
        let mut re = vec![0i64; FFT_ENC_SB];
        let mut im = vec![0i64; FFT_ENC_SB];
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
}
