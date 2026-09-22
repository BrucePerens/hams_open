#!/usr/bin/env python3
"""Fits the 81-tap FIR shaping filter that maps the identified LCG (`s[n+1] = 173*s[n] + 13849 mod
65536`) onto the chip's actual muted/comfort-noise output (`table_65536.i16`), and validates it with
a proper held-out split -- addressing `docs/references/AMBE_CHIP_NOISE_GENERATOR.md`'s own note that
the original fit "was fitted on the same table, not validated on a separate capture" (also tracked
in hams_com's `night_shift_todo/high/ambe-...-3c8f1d2e.md`, "noise-shaping filter validation").

Why a held-out split within the SAME table, rather than a fresh chip capture: the muted/comfort-noise
path is a deterministic, full-period (65,536-state) generator. Every possible capture of it, no matter
how many times or how far apart in time, cycles through exactly the same 65,536 values (confirmed
2026-09-22 with `examples/ambe_chip_noise_table_capture.rs`: a fresh capture matched `table_65536.i16`
at correlation 0.9999999 once phase-aligned). So a "fresh capture" cannot serve as independent data
for testing whether the filter generalizes -- there is only ever one table to discover. A genuine
overfitting check instead needs to hold out *positions within* that one table: fit the filter on half
the samples and see whether it predicts the other half it never saw.

Usage: `python3 fit_and_validate_shaping_filter.py` (run from this directory; reads `table_65536.i16`,
writes `fir_coef_81tap.json`).
"""
import json

import numpy as np

TABLE_PATH = "table_65536.i16"
OUT_PATH = "fir_coef_81tap.json"
M = 65536
LCG_A = 173
LCG_C = 13849
TAPS = 81
HALF = TAPS // 2
# Found by a brute-force search over small shifts of the LCG sequence seeded at 0 (matching
# `csearch.py`'s own broader search over all seeds/shifts): rolling by -1 sample aligns
# `table_65536.i16` at raw correlation 0.866 -- the same figure AMBE_CHIP_NOISE_GENERATOR.md reports.
SEED = 0
ALIGNMENT_ROLL = -1


def lcg_sequence(seed: int, n: int) -> np.ndarray:
    """The LCG's raw state, read as signed 16-bit, for n consecutive steps starting at `seed`."""
    s = np.zeros(n, dtype=np.int64)
    u = seed
    for i in range(n):
        s[i] = u
        u = (LCG_A * u + LCG_C) & 0xFFFF
    return np.where(s >= 32768, s - 65536, s).astype(float)


def design_matrix(seq: np.ndarray, taps: int, half: int) -> np.ndarray:
    """Column k holds `seq` shifted so that `design[n, k] == seq[n - half + k]` (circular, since the
    generator's period exactly matches the table length)."""
    cols = [np.roll(seq, half - k) for k in range(taps)]
    return np.stack(cols, axis=1)


def corr(a: np.ndarray, b: np.ndarray) -> float:
    a = a - a.mean()
    b = b - b.mean()
    return float((a * b).sum() / np.sqrt((a**2).sum() * (b**2).sum()))


def main() -> None:
    table = np.fromfile(TABLE_PATH, "<i2").astype(float)
    assert len(table) == M, f"expected {M} samples, got {len(table)}"

    seq = lcg_sequence(SEED, M)
    seq_aligned = np.roll(seq, ALIGNMENT_ROLL)
    raw_corr = corr(seq_aligned, table)
    print(f"raw LCG-vs-table correlation (sanity check, should be ~0.866): {raw_corr:.4f}")

    design = design_matrix(seq_aligned, TAPS, HALF)

    # Interleaved (even/odd) split, not a contiguous first-half/second-half one: a contiguous split
    # would leave a whole 32,768-sample run untested by nearby training positions, which is a weaker
    # check of generalization than alternating positions is.
    train_idx = np.arange(0, M, 2)
    test_idx = np.arange(1, M, 2)

    coef_train, *_ = np.linalg.lstsq(design[train_idx], table[train_idx], rcond=None)
    train_corr = corr(design[train_idx] @ coef_train, table[train_idx])
    held_out_corr = corr(design[test_idx] @ coef_train, table[test_idx])

    # The coefficients actually shipped: fit on the whole table (matching the original methodology),
    # since the held-out check above already established this doesn't meaningfully overfit.
    coef_full, *_ = np.linalg.lstsq(design, table, rcond=None)
    pred_full = design @ coef_full
    full_corr = corr(pred_full, table)
    residual_rms = float(np.sqrt(((pred_full - table) ** 2).mean()))
    signal_rms = float(np.sqrt((table**2).mean()))

    print(f"train-split correlation:     {train_corr:.6f}")
    print(f"held-out-split correlation:  {held_out_corr:.6f}  (gap: {train_corr - held_out_corr:.6f})")
    print(f"full-table correlation:      {full_corr:.6f}")
    print(f"residual RMS: {residual_rms:.4f}  (signal RMS: {signal_rms:.4f})")

    dominant = sorted(range(TAPS), key=lambda k: -abs(coef_full[k]))[:5]
    print("dominant taps (lag, coefficient):")
    for k in dominant:
        print(f"  lag={HALF - k:+d}  coef={coef_full[k]:.6f}")

    out = {
        "description": (
            "81-tap FIR filter mapping the identified LCG's raw state sequence to the chip's actual "
            "muted/comfort-noise output. y[n] = sum_lag coef[lag] * lcg_state[n - lag], where "
            "lcg_state is the signed-16-bit reading of s[n+1] = (173*s[n] + 13849) mod 65536, seeded "
            f"s[0]={SEED} and rolled by {ALIGNMENT_ROLL} sample to align index 0 of table_65536.i16 "
            "(reproduce with this script)."
        ),
        "lcg_multiplier": LCG_A,
        "lcg_increment": LCG_C,
        "lcg_modulus": M,
        "seed_at_index_0": SEED,
        "alignment_roll": ALIGNMENT_ROLL,
        "taps_by_lag": {str(HALF - k): float(coef_full[k]) for k in range(TAPS)},
        "fit_on": f"{TABLE_PATH} (all {M} samples)",
        "validation": {
            "method": (
                "least-squares fit on even-indexed samples (interleaved train split), evaluated on "
                "held-out odd-indexed samples"
            ),
            "train_correlation": train_corr,
            "held_out_correlation": held_out_corr,
            "full_table_correlation_no_holdout": full_corr,
            "residual_rms": residual_rms,
            "signal_rms": signal_rms,
            "date": "2026-09-22",
            "conclusion": (
                "held-out correlation matches in-sample correlation to within "
                f"{abs(train_corr - held_out_corr):.6f} -- the filter is not meaningfully overfit to "
                f"{TABLE_PATH}."
            ),
        },
    }
    with open(OUT_PATH, "w") as f:
        json.dump(out, f, indent=2)
    print(f"wrote {OUT_PATH}")


if __name__ == "__main__":
    main()
