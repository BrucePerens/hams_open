//! Encode-side quantization: the real inverse of `decode::dequantize`'s table lookups. Like
//! `ambe_dstar::quantize` (mbelib is decode-only, so there is no real reference encoder to check
//! this against), this module's own nearest-neighbor search -- plain Euclidean distance in the
//! log-domain gain space the tables are themselves defined in, or Hamming distance for the
//! bit-pattern voicing table -- is this project's own reasoned choice, flagged honestly as a
//! design decision rather than a known-correct reproduction of DVSI's own (unpublished) encoder.

use super::tables;

/// Finds the index of the table row nearest `target` by squared Euclidean distance -- the shared
/// core of every vector-quantization search below (identical in spirit to
/// `ambe_dstar::quantize::nearest_row`, duplicated rather than shared across modules since it's a
/// three-line generic utility, not the substantive logic).
fn nearest_row<const N: usize>(table: &[[f64; N]], target: &[f64; N]) -> u32 {
    table
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da: f64 = a.iter().zip(target).map(|(x, y)| (x - y).powi(2)).sum();
            let db: f64 = b.iter().zip(target).map(|(x, y)| (x - y).powi(2)).sum();
            da.total_cmp(&db)
        })
        .map(|(i, _)| i as u32)
        .expect("table must be non-empty")
}

/// `b0`: the pitch index nearest a target fundamental-frequency angle `w0`. Unlike D-STAR (which
/// only has a closed-form `f0` formula to invert), Annex A's own `W0_TABLE` stores `f0` directly,
/// so this is an ordinary nearest-scalar search over the real table, not a formula inversion --
/// clamped to `0..=119` (the 120 real pitch codes; 120-127 are the special erasure/silence/tone
/// ranges `decode::classify_b0` handles, never a real encoder target).
pub fn quantize_pitch(w0: f64) -> u32 {
    assert!(w0.is_finite() && w0 > 0.0, "pitch must be a finite positive frequency, got {w0}");
    let f0 = w0 / (2.0 * std::f64::consts::PI);
    tables::W0_TABLE
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (**a - f0).abs().total_cmp(&(**b - f0).abs()))
        .map(|(i, _)| i as u32)
        .expect("W0_TABLE is non-empty")
}

/// `b1`: the [`tables::VUV`] row nearest a target 8-entry voicing pattern, by Hamming distance
/// (the natural metric for a boolean pattern table, vs. the Euclidean distance the other,
/// real-valued tables use).
pub fn quantize_voicing(target: [bool; 8]) -> u32 {
    tables::VUV
        .iter()
        .enumerate()
        .min_by_key(|(_, row)| {
            row.iter()
                .zip(target.iter())
                .filter(|(a, b)| a != b)
                .count()
        })
        .map(|(i, _)| i as u32)
        .expect("VUV is non-empty")
}

/// `b2`: the gain-delta index nearest a target `delta_gamma`.
pub fn quantize_gain_delta(delta_gamma: f64) -> u32 {
    tables::DG
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (**a - delta_gamma)
                .abs()
                .total_cmp(&(**b - delta_gamma).abs())
        })
        .map(|(i, _)| i as u32)
        .expect("DG is non-empty")
}

/// `b3`: nearest [`tables::PRBA24`] row to `(G2, G3, G4)`.
pub fn quantize_prba24(target: [f64; 3]) -> u32 {
    nearest_row(&tables::PRBA24, &target)
}

/// `b4`: nearest [`tables::PRBA58`] row to `(G5, G6, G7, G8)`.
pub fn quantize_prba58(target: [f64; 4]) -> u32 {
    nearest_row(&tables::PRBA58, &target)
}

/// `b5..b8`: nearest higher-order-coefficient row in the given block's own table (callers pass
/// [`tables::HOC_B5`]/`HOC_B6`/`HOC_B7`/`HOC_B8` -- differing lengths, hence a slice rather than a
/// fixed-size array parameter, unlike `ambe_dstar::quantize::quantize_hoc`'s uniform 16-row
/// tables).
pub fn quantize_hoc(table: &[[f64; 4]], target: [f64; 4]) -> u32 {
    table
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let da: f64 = a.iter().zip(target).map(|(x, y)| (x - y).powi(2)).sum();
            let db: f64 = b.iter().zip(target).map(|(x, y)| (x - y).powi(2)).sum();
            da.total_cmp(&db)
        })
        .map(|(i, _)| i as u32)
        .expect("table must be non-empty")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same "dequantize then re-quantize recovers a numerically identical row" invariant
    /// `ambe_dstar::quantize`'s own tests use -- correct regardless of duplicate rows in the real
    /// tables, since it checks value equality, not index equality.
    #[test]
    fn quantize_prba24_recovers_a_row_matching_every_real_table_entry() {
        for (i, &row) in tables::PRBA24.iter().enumerate() {
            let recovered = quantize_prba24(row);
            assert_eq!(
                tables::PRBA24[recovered as usize],
                row,
                "PRBA24 row {i}: {row:?}, recovered index {recovered}"
            );
        }
    }

    #[test]
    fn quantize_prba58_recovers_a_row_matching_every_real_table_entry() {
        for (i, &row) in tables::PRBA58.iter().enumerate() {
            let recovered = quantize_prba58(row);
            assert_eq!(
                tables::PRBA58[recovered as usize],
                row,
                "PRBA58 row {i}: {row:?}, recovered index {recovered}"
            );
        }
    }

    #[test]
    fn quantize_hoc_recovers_a_row_matching_every_real_table_entry_in_all_four_blocks() {
        for table in [
            &tables::HOC_B5[..],
            &tables::HOC_B6[..],
            &tables::HOC_B7[..],
            &tables::HOC_B8[..],
        ] {
            for (i, &row) in table.iter().enumerate() {
                let recovered = quantize_hoc(table, row);
                assert_eq!(
                    table[recovered as usize], row,
                    "HOC row {i}: {row:?}, recovered index {recovered}"
                );
            }
        }
    }

    #[test]
    fn quantize_gain_delta_recovers_a_value_matching_every_real_table_entry() {
        for (i, &value) in tables::DG.iter().enumerate() {
            let recovered = quantize_gain_delta(value);
            assert_eq!(
                tables::DG[recovered as usize],
                value,
                "DG[{i}] = {value}, recovered index {recovered}"
            );
        }
    }

    #[test]
    fn quantize_voicing_recovers_a_row_matching_every_real_table_entry() {
        for (i, &row) in tables::VUV.iter().enumerate() {
            let recovered = quantize_voicing(row);
            assert_eq!(
                tables::VUV[recovered as usize],
                row,
                "VUV[{i}] = {row:?}, recovered index {recovered}"
            );
        }
    }

    /// `quantize_pitch` must recover exactly `b0` for every one of the 120 real table entries --
    /// unlike D-STAR's formula-based inversion, this is an exact nearest-scalar search over the
    /// same real table `decode::dequantize` reads, so there is no floating-point-boundary
    /// ambiguity to tolerate.
    #[test]
    fn quantize_pitch_exactly_recovers_every_real_b0() {
        for b0 in 0u32..120 {
            let f0 = tables::W0_TABLE[b0 as usize];
            let w0 = f0 * 2.0 * std::f64::consts::PI;
            let recovered = quantize_pitch(w0);
            assert_eq!(recovered, b0, "w0={w0} (from b0={b0}), recovered={recovered}");
        }
    }
}
