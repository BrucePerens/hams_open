// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fixed-point D-STAR encode side. The bit layout (`pack_raw_parameters`, `build_frame`, `pack_tone_parameters`,
//! `build_tone_frame`) is already pure integer in the float sibling (checked: no `f32`/`f64` anywhere in those four
//! functions or in `RawParameters`), so it is re-exported unchanged instead of duplicated. What this module adds is
//! the fixed-point speech quantizer's D-STAR table set.

use super::tables_q16::{DG_Q16_16, HOC_B5_Q16_16, HOC_B6_Q16_16, HOC_B7_Q16_16, HOC_B8_Q16_16, PRBA24_Q16_16, PRBA58_Q16_16};
use crate::ambe::fixed::general::mbe_encode::ModeTables;
use crate::ambe::float::dstar::tables::{LMPRBL, VUV};

pub use crate::ambe::float::dstar::decode::RawParameters;
pub use crate::ambe::float::dstar::encode::{build_frame, build_tone_frame, pack_raw_parameters, pack_tone_parameters};

/// D-STAR's Q16.16 quantizer tables (`b8` even-only).
pub fn mode_tables() -> ModeTables<'static> {
    ModeTables {
        vuv: &VUV,
        dg_q16: &DG_Q16_16,
        prba24_q16: &PRBA24_Q16_16,
        prba58_q16: &PRBA58_Q16_16,
        lmprbl: &LMPRBL,
        hoc_q16: [&HOC_B5_Q16_16, &HOC_B6_Q16_16, &HOC_B7_Q16_16, &HOC_B8_Q16_16],
        hoc_b8_even_only: true,
    }
}
