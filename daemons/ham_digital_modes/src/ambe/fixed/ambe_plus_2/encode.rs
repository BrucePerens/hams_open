// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point AMBE+2 encode side. `pack_raw_parameters`, `build_frame`, `pack_tone_parameters`, `build_tone_frame`
//! and `dtmf_tone_idx` are pure integer in the float sibling and are re-exported unchanged. Added here: the fixed
//! speech-quantizer table set and [`amplitude_field_q16`], the integer version of the float encoder's 12-bit tone
//! level mapping (interpolation in log2 over the seven chip-measured points).

use super::tables_q16::{DG_Q16_16, HOC_B5_Q16_16, HOC_B6_Q16_16, HOC_B7_Q16_16, HOC_B8_Q16_16, PRBA24_Q16_16, PRBA58_Q16_16};
use crate::ambe::fixed::general::explog::log2_q16_i64;
use crate::ambe::fixed::general::fixed_ops::div_q16;
use crate::ambe::fixed::general::mbe_encode::ModeTables;
use crate::ambe::float::ambe_plus_2::tables::{LMPRBL, VUV};

pub use crate::ambe::float::ambe_plus_2::decode::{dtmf_tone_idx, RawParameters};
pub use crate::ambe::float::ambe_plus_2::encode::{build_frame, build_tone_frame, pack_raw_parameters, pack_tone_parameters};

/// AMBE+2's Q16.16 quantizer tables.
pub fn mode_tables() -> ModeTables<'static> {
    ModeTables {
        vuv: &VUV,
        dg_q16: &DG_Q16_16,
        prba24_q16: &PRBA24_Q16_16,
        prba58_q16: &PRBA58_Q16_16,
        lmprbl: &LMPRBL,
        hoc_q16: [&HOC_B5_Q16_16, &HOC_B6_Q16_16, &HOC_B7_Q16_16, &HOC_B8_Q16_16],
        hoc_b8_even_only: false,
        rho_q16: crate::ambe::fixed::general::mbe_speech::POINT_65_Q16_16,
    }
}

/// The 12-bit tone level field for a per-tone amplitude (Q16.16 PCM units); integer port of the float
/// `amplitude_field` (same seven chip points, same clamping quirks below 250 and above 16000).
pub fn amplitude_field_q16(amplitude_q16: i64) -> u16 {
    const POINTS: [(i64, i32); 7] = [(250, 0x715), (500, 0x725), (1000, 0xea2), (2000, 0xed2), (4000, 0xf12), (8000, 0xf62), (16000, 0xfa2)];
    let x = log2_q16_i64(amplitude_q16.max(1 << 16)) as i64;
    let lg = |a: i64| log2_q16_i64(a << 16) as i64;
    let (mut lo, mut hi) = (POINTS[0], POINTS[POINTS.len() - 1]);
    for w in POINTS.windows(2) {
        if x >= lg(w[0].0) && x <= lg(w[1].0) {
            (lo, hi) = (w[0], w[1]);
            break;
        }
    }
    let span = lg(hi.0) - lg(lo.0);
    let t_q16 = (div_q16((x - lg(lo.0)) as i32, span as i32) as i64).clamp(0, 65536);
    let field = lo.1 as i64 * 65536 + t_q16 * (hi.1 - lo.1) as i64;
    ((field + 32768) >> 16) as u16
}
