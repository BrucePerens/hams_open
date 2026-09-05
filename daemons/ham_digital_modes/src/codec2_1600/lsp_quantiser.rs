// SPDX-License-Identifier: LGPL-3.0-or-later
//! The 1600bps mode's own LSP quantizer: ten independent per-dimension
//! scalar codebooks (dimension 1 = k = 1 each), a genuinely different
//! design from `codec2_3200::quantise`'s delta-scalar quantizer. Table
//! values (in Hz) are the real, published Codec2 quantizer tables
//! (`src/codebook/lsp1.txt` .. `lsp10.txt` in a plain upstream Codec2
//! checkout) -- not creative expression, the actual numbers any
//! interoperable decoder must reproduce exactly, the same reasoning
//! `codec2_3200`'s own module doc comment gives for its bitstream-format
//! constants. Checked directly against upstream's own plain-text
//! codebook source (`lsp1.txt` .. `lsp10.txt`): every one of the ten
//! real tables turns out to be evenly spaced, so each reduces to a
//! `(start, step, levels)` triple -- the same "closed-form, not opaque
//! trained data" situation `codec2_3200::bw_gamma`'s own doc comment
//! documents for that quantizer, verified here rather than assumed, so
//! no literal per-level float array is stored (and no separate
//! LGPL-2.1-only data file was needed).

/// One dimension's scalar codebook: evenly spaced from `start` in steps
/// of `step`, `levels` entries (`2^log2m == levels` in every real
/// dimension here, confirmed against each `lspN.txt`'s own `1 <m>`
/// header). Every one of the ten real tables happens to be evenly
/// spaced, so this is `start + step*index`, not a lookup array.
struct LspDim {
    start_hz: f32,
    step_hz: f32,
    levels: u32,
    log2m: u32,
}

/// The ten real per-dimension codebooks, `lsp1.txt` .. `lsp10.txt`.
const LSP_CB: [LspDim; super::LPC_ORD] = [
    LspDim { start_hz: 225.0, step_hz: 25.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 325.0, step_hz: 25.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 500.0, step_hz: 50.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 700.0, step_hz: 100.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 950.0, step_hz: 100.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 1100.0, step_hz: 100.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 1500.0, step_hz: 100.0, levels: 16, log2m: 4 },
    LspDim { start_hz: 2300.0, step_hz: 100.0, levels: 8, log2m: 3 },
    LspDim { start_hz: 2500.0, step_hz: 100.0, levels: 8, log2m: 3 },
    LspDim { start_hz: 2900.0, step_hz: 200.0, levels: 4, log2m: 2 },
];

const RAD_PER_HZ: f32 = std::f32::consts::PI / 4000.0;
const HZ_PER_RAD: f32 = 4000.0 / std::f32::consts::PI;

/// Bit width of the `i`th LSP dimension's own index -- 4,4,4,4,4,4,4,3,3,2
/// (36 bits total), matching upstream's own `lsp_bits(i)`.
pub fn lsp_bits(i: usize) -> u32 {
    LSP_CB[i].log2m
}

/// Encodes one dimension's LSP (radians) to its nearest codebook index.
/// The real codebooks are evenly spaced, so nearest-index is a direct
/// rounded division, not a linear scan -- behaviorally identical to the
/// reference's own linear-scan `quantise()` for a 1-D codebook (nearest
/// by absolute difference is the same as nearest by squared error for a
/// scalar), just without the O(m) search.
fn quantise_dim(dim: &LspDim, target_hz: f32) -> u32 {
    let idx = ((target_hz - dim.start_hz) / dim.step_hz).round();
    idx.clamp(0.0, (dim.levels - 1) as f32) as u32
}

/// Scalar LSP quantiser. From a vector of unquantised LSPs (radians)
/// finds the quantised LSP indexes -- one call per dimension, each
/// dimension's own independent codebook.
pub fn encode_lsps_scalar(lsp: &[f32; super::LPC_ORD]) -> [u32; super::LPC_ORD] {
    std::array::from_fn(|i| quantise_dim(&LSP_CB[i], lsp[i] * HZ_PER_RAD))
}

/// From a vector of quantised LSP indexes, returns the quantised LSPs
/// (radians) -- the real quantized value any compliant decoder must
/// reproduce exactly, since this *is* the table, not a design choice.
pub fn decode_lsps_scalar(indexes: &[u32; super::LPC_ORD]) -> [f32; super::LPC_ORD] {
    std::array::from_fn(|i| {
        let dim = &LSP_CB[i];
        let hz = dim.start_hz + dim.step_hz * indexes[i] as f32;
        hz * RAD_PER_HZ
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_bits_sums_to_36_total_bits() {
        let total: u32 = (0..super::super::LPC_ORD).map(lsp_bits).sum();
        assert_eq!(total, 36, "1600bps LSP field is documented as 36 bits total");
    }

    #[test]
    fn encode_then_decode_recovers_the_same_quantised_value_for_every_real_index() {
        for i in 0..super::super::LPC_ORD {
            let dim = &LSP_CB[i];
            for level in 0..dim.levels {
                let indexes: [u32; super::super::LPC_ORD] = std::array::from_fn(|j| {
                    if j == i {
                        level
                    } else {
                        0
                    }
                });
                let decoded = decode_lsps_scalar(&indexes);
                let re_encoded = encode_lsps_scalar(&decoded);
                assert_eq!(
                    re_encoded[i], level,
                    "dimension {i} level {level}: decode-then-encode didn't round-trip"
                );
            }
        }
    }

    /// Every real table value from `lspN.txt` (N=1..10), spot-checked
    /// directly against the plain upstream Codec2 source text -- not
    /// just the evenly-spaced reconstruction's own internal consistency.
    #[test]
    fn decode_matches_the_real_upstream_codebook_tables_at_specific_indices() {
        let cases: [(usize, u32, f32); 10] = [
            (0, 0, 225.0),
            (0, 15, 600.0),
            (1, 8, 525.0),
            (2, 0, 500.0),
            (3, 15, 2200.0),
            (4, 7, 1650.0),
            (5, 15, 2600.0),
            (6, 0, 1500.0),
            (7, 7, 3000.0),
            (8, 0, 2500.0),
        ];
        for (dim, level, expected_hz) in cases {
            let indexes: [u32; super::super::LPC_ORD] =
                std::array::from_fn(|j| if j == dim { level } else { 0 });
            let decoded = decode_lsps_scalar(&indexes);
            let got_hz = decoded[dim] * HZ_PER_RAD;
            assert!(
                (got_hz - expected_hz).abs() < 1e-3,
                "dim={dim} level={level}: got {got_hz}Hz, expected {expected_hz}Hz"
            );
        }
        // lsp10.txt's own 4 entries: 2900, 3100, 3300, 3500
        for (level, expected_hz) in [(0, 2900.0), (1, 3100.0), (2, 3300.0), (3, 3500.0)] {
            let indexes: [u32; super::super::LPC_ORD] =
                std::array::from_fn(|j| if j == 9 { level } else { 0 });
            let decoded = decode_lsps_scalar(&indexes);
            let got_hz = decoded[9] * HZ_PER_RAD;
            assert!(
                (got_hz - expected_hz).abs() < 1e-3,
                "dim=9 level={level}: got {got_hz}Hz, expected {expected_hz}Hz"
            );
        }
    }
}
