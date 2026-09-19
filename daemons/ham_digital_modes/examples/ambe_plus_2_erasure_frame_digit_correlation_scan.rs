// SPDX-License-Identifier: LGPL-3.0-or-later
//! Section 40 found AMBE+2 half-rate's `Erasure` frames (`b0=120`, produced by `TD_ENABLE` on a
//! detected tone/DTMF stimulus) are **not** byte-identical across the 16 DTMF digits, even though
//! the two fields `classify_b0`'s `Erasure` branch defines (`b1`/`b2` in the ordinary speech scatter)
//! are flat -- meaning real per-digit information exists somewhere in the frame's other bits, with no
//! known field to decode it. This offline tool (no chip needed -- uses 16 real hex frames already
//! captured live, hardcoded below) runs the same sliding 7-bit-window Spearman correlation scan
//! `examples/ambe_chip_validate_ambe_plus_2.rs` used to originally find P25 full-rate's own Gray-coded
//! pitch field, against DTMF row and column separately.
//!
//! **Scans the decoded 49-bit `d` (post-Golay, post-dewhiten), not the raw 72-bit deinterleaved
//! frame.** `C1`'s 23 raw bits are XOR-whitened with a PRBS seeded from `C0`'s own already-decoded
//! data (`ambe_plus_2::parse_frame`/`ambe_dstar::whiten_c1`) before transmission -- a fixed XOR does
//! not preserve a 7-bit window's ordinal value, so a real per-digit field living in `C1`'s data bits
//! would be invisible to this scan on the raw (still-whitened) frame. Scanning `d` instead means a
//! real signal in either Golay block's own data is actually visible to this technique.
//!
//! This is exploratory, not a validated finding either way: a real signal here would need a
//! considerably higher bar (e.g. a clean, small-integer-linear relationship, the way D-STAR's own
//! `index == 128 + row + 4*col` held exactly) before being written up as a finding, not just the
//! single best correlation coefficient out of many tested positions (multiple-comparisons risk with
//! only 16 data points and dozens of window positions -- see `scan`'s own doc comment for the actual
//! numbers).
//!
//! **Update, same section: the real field was found afterward** -- DVSI's own documented `TONE_IDX`
//! field (`AMBE-3000R Vocoder Chip Users Manual` Table 103/104; see
//! `ambe_plus_2::decode::decode_tone_idx`'s own doc comment for the full citation and bit layout),
//! whose low nibble (`d[16..20)`, repeated four times) holds the digit's own DTMF value directly for
//! this rate (`0x80 | nibble`). **Why this scan's own null result against `row`/`col` separately is
//! explained, not contradicted, by that finding, and it is a real, useful correction to this scan's
//! own methodology**: the scan tested the right bit region (`d[16..20)` sits inside several of the
//! sliding 7-bit windows tried) but the wrong hypothesis class. For the 3x3 sub-block of digits
//! 1-9 (`row`,`col` both 0-2), the nibble genuinely *is* `3*row + col + 1`, a clean linear relation
//! Spearman would have caught -- but the DTMF keypad's own historical layout puts `A/B/C` in column
//! 3 and `*/0/#/D` in row 3 at nibble values (`0xA-0xC`, `0xE/0x0/0xF/0xD`) that break that linear
//! rule, and those outliers alone are enough to pull `n=16` Spearman well below any reasonable
//! significance threshold. The lesson generalizes: **a monotone/linear correlation scan can miss a
//! real field that is a lookup table rather than a formula** -- it is not evidence against running
//! such scans in general (they found this project's own real Gray-coded pitch field and D-STAR's
//! own linear tone index), only evidence that a null result here specifically warranted an
//! exact-match search next (as used successfully elsewhere in this document), not a conclusion that
//! no field existed. Left in place as a real, useful record of the exploratory step that came first,
//! not superseded/deleted.
use ham_digital_modes::ambe_plus_2::interleave::interleaved_to_frame;
use ham_digital_modes::ambe_plus_2::parse_frame;

// (digit label, row 0-3, col 0-3, real hex captured live under RATET(33), TD_ENABLE on, DTX_ENABLE
// on -- from AMBE_CHIP_VALIDATION_FINDINGS.md section 40's own probe run).
const DIGITS: [(&str, f64, f64, &str); 16] = [
    ("1", 0.0, 0.0, "e8cedbae008cd122c0"),
    ("2", 0.0, 1.0, "eacdeb8e20ad8702c0"),
    ("3", 0.0, 2.0, "eaeff9ae228d9322c0"),
    ("A", 0.0, 3.0, "ebefc98c01cf8702c0"),
    ("4", 1.0, 0.0, "cafee98c22b8e502c0"),
    ("5", 1.0, 1.0, "cadcfbac2098f122c0"),
    ("6", 1.0, 2.0, "c8dfcb8c00b9a702c0"),
    ("B", 1.0, 3.0, "ebcddbac03ef9322c0"),
    ("7", 2.0, 0.0, "c8fdd9ac0299b322c0"),
    ("8", 2.0, 1.0, "e9ceeb8c23cec502c0"),
    ("9", 2.0, 2.0, "e9ecf9ac21eed122c0"),
    ("C", 2.0, 3.0, "cbdccb8e03dae502c0"),
    ("*", 3.0, 0.0, "c9fde98e21dba702c0"),
    ("0", 3.0, 1.0, "e8ecc98e02acc502c0"),
    ("#", 3.0, 2.0, "c9dffbae23fbb322c0"),
    ("D", 3.0, 3.0, "cbfed9ae01faf122c0"),
];

fn hex_to_bytes(hex: &str) -> [u8; 9] {
    let mut out = [0u8; 9];
    for i in 0..9 {
        out[i] = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
    }
    out
}

const D_BITS: usize = 49;

/// Reads a `width`-bit window starting at bit `start` (0 = `d`'s own MSB, bit 48) out of the
/// decoded 49-bit `d`, matching this crate's own `d[a..b)` convention (`ambe_dstar::mod`'s doc
/// comment).
fn window_value(d: u64, start: usize, width: usize) -> u32 {
    let shift = D_BITS - start - width;
    ((d >> shift) & ((1u64 << width) - 1)) as u32
}

fn gray_to_binary(g: u32) -> u32 {
    let mut b = g;
    let mut shift = 1;
    while (g >> shift) != 0 {
        b ^= g >> shift;
        shift += 1;
    }
    b
}

fn correlation(xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let cov: f64 = xs.iter().zip(ys).map(|(x, y)| (x - mx) * (y - my)).sum();
    let vx: f64 = xs.iter().map(|x| (x - mx).powi(2)).sum();
    let vy: f64 = ys.iter().map(|y| (y - my).powi(2)).sum();
    if vx == 0.0 || vy == 0.0 {
        return 0.0;
    }
    cov / (vx.sqrt() * vy.sqrt())
}

/// Fractional (average) rank, correctly handling ties -- **the naive version of this function
/// copied from `ambe_chip_validate_ambe_plus_2.rs` assigns distinct ranks 0..n-1 by stable-sort
/// order even when many values are exactly equal**, which silently produces spurious near-perfect
/// correlations whenever a bit window is constant or near-constant and the *input ordering itself*
/// correlates with the target (true here: digits were captured in strict row-major order, so row
/// correlates with capture sequence, and a stable sort of an all-equal column falls back to that
/// same input order) -- caught directly by noticing `bits[0..7)` printed as a literal constant
/// (127) across all 16 digits despite reporting `spearman=1.000` against `row` before this fix.
/// Real statistical software (and this project's own established rigor bar) requires tied values to
/// receive the *same* averaged rank; this version does that.
fn spearman(xs: &[f64], ys: &[f64]) -> f64 {
    fn ranks(v: &[f64]) -> Vec<f64> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|&a, &b| v[a].total_cmp(&v[b]));
        let mut r = vec![0.0; v.len()];
        let mut i = 0;
        while i < idx.len() {
            let mut j = i;
            while j + 1 < idx.len() && v[idx[j + 1]] == v[idx[i]] {
                j += 1;
            }
            let avg_rank = (i + j) as f64 / 2.0;
            for &k in &idx[i..=j] {
                r[k] = avg_rank;
            }
            i = j + 1;
        }
        r
    }
    correlation(&ranks(xs), &ranks(ys))
}

/// A window with fewer than this many distinct values across the 16 digits can't meaningfully
/// discriminate a 4-way row or column split -- reported separately from a genuine correlation, not
/// silently included (see `spearman`'s own doc comment for why a near-constant window is dangerous).
const MIN_DISTINCT_VALUES: usize = 4;

fn distinct_count(v: &[f64]) -> usize {
    let mut sorted = v.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    sorted.dedup();
    sorted.len()
}

/// This scan tests `(D_BITS - 6)` window start positions (7-bit windows over 49 bits = 43
/// positions) times 2 (plain/gray) = 86 tests per target, 172 total across both `row` and `col`.
/// For `n=16` samples, the uncorrected two-tailed `p~0.05` critical Spearman value is
/// approximately 0.50 -- meaning any single one of these ~172 tests, run in isolation, would call
/// 0.537/0.550 "significant." Run this many times, that is exactly the outcome pure chance
/// predicts (roughly 172 * 0.05 ~ 8-9 false positives expected at that threshold by chance alone) --
/// the actual reason 0.537/0.550 is not evidence of a real field, not merely "looks a bit low."
fn scan(label: &str, target: &[f64], d_values: &[u64; 16]) {
    println!("-- scanning against {label} --");
    let mut best: Option<(usize, bool, f64)> = None;
    for start in 0..(D_BITS - 6) {
        let plain: Vec<f64> = d_values.iter().map(|&d| window_value(d, start, 7) as f64).collect();
        let gray: Vec<f64> = d_values.iter().map(|&d| gray_to_binary(window_value(d, start, 7)) as f64).collect();
        if distinct_count(&plain) < MIN_DISTINCT_VALUES {
            continue;
        }
        let sp_plain = spearman(target, &plain);
        let sp_gray = spearman(target, &gray);
        for (is_gray, sp) in [(false, sp_plain), (true, sp_gray)] {
            if best.is_none_or(|(_, _, b)| sp.abs() > b.abs()) {
                best = Some((start, is_gray, sp));
            }
        }
    }
    if let Some((start, is_gray, sp)) = best {
        println!(
            "  best overall: bits[{start}..{}), {}, |spearman|={:.3}",
            start + 7,
            if is_gray { "gray" } else { "plain" },
            sp.abs()
        );
    }
}

fn main() {
    let mut d_values = [0u64; 16];
    let mut rows = [0.0f64; 16];
    let mut cols = [0.0f64; 16];
    for (i, (label, row, col, hex)) in DIGITS.iter().enumerate() {
        let bytes = hex_to_bytes(hex);
        let mut wire: u128 = 0;
        for &b in bytes.iter() {
            wire = (wire << 8) | b as u128;
        }
        let logical = interleaved_to_frame(wire);
        let parsed = parse_frame(logical);
        d_values[i] = parsed.d;
        rows[i] = *row;
        cols[i] = *col;
        println!(
            "{label}: row={row} col={col} hex={hex} epsilon_c0={} epsilon_c1={} d[0..7)={}",
            parsed.epsilon_c0,
            parsed.epsilon_c1,
            window_value(parsed.d, 0, 7)
        );
    }

    scan("row", &rows, &d_values);
    scan("col", &cols, &d_values);
}
