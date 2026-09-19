// SPDX-License-Identifier: LGPL-3.0-or-later
//! Cross-checks `ambe::fixed::tia_102_baba::enhancement`/`error_estimation` against their floating-point
//! siblings. `realistic_amplitude_sweep` sweeps peak harmonic amplitude from `0.5` to `3000` --
//! spanning the real `R_M0` range `examples/ambe_fixed_chip_validate_ratet27.rs`'s own live-chip run
//! measured (roughly 9 to 4x10^8, i.e. peak amplitudes from well under 1 to the low thousands), not
//! a single arbitrarily-picked envelope -- since `enhancement.rs`'s own energy-domain quantities are
//! specifically designed (see that module's doc comment) around this measured dynamic range.

use ham_digital_modes::ambe::fixed::tia_102_baba::enhancement as fixed_enh;
use ham_digital_modes::ambe::fixed::tia_102_baba::error_estimation::{
    estimate_errors_q16, should_mute_frame_q16, should_repeat_frame_q16,
};
use ham_digital_modes::ambe::float::tia_102_baba::enhancement as float_enh;
use ham_digital_modes::ambe::float::tia_102_baba::error_estimation::{estimate_errors, should_mute_frame, should_repeat_frame, FrameErrors};

const RELATIVE_TOLERANCE: f64 = 0.01;

fn to_q16(v: f64) -> i32 {
    (v * 65536.0).round() as i32
}
fn to_q16_i64(v: f64) -> i64 {
    (v * 65536.0).round() as i64
}
fn from_q16(v: i32) -> f64 {
    v as f64 / 65536.0
}
fn from_q16_i64(v: i64) -> f64 {
    v as f64 / 65536.0
}
fn assert_close(label: &str, float_val: f64, fixed_val_q16: i32) {
    let fixed_val = from_q16(fixed_val_q16);
    let rel_err = if float_val.abs() > 1e-9 {
        ((fixed_val - float_val) / float_val).abs()
    } else {
        fixed_val.abs()
    };
    assert!(
        rel_err <= RELATIVE_TOLERANCE,
        "{label}: float={float_val}, fixed={fixed_val}, rel_err={rel_err}"
    );
}
fn assert_close_i64(label: &str, float_val: f64, fixed_val_q16: i64) {
    let fixed_val = from_q16_i64(fixed_val_q16);
    let rel_err = if float_val.abs() > 1e-9 {
        ((fixed_val - float_val) / float_val).abs()
    } else {
        fixed_val.abs()
    };
    assert!(
        rel_err <= RELATIVE_TOLERANCE,
        "{label}: float={float_val}, fixed={fixed_val}, rel_err={rel_err}"
    );
}

/// A realistic spectral envelope at a given peak amplitude: harmonics decaying-ish, not all-equal
/// or degenerate. `peak` sets the overall scale -- swept across several decades in the tests below
/// to exercise `enhancement.rs`'s own wide `R_M0` range (real chip data: roughly 9 to 4x10^8).
fn realistic_amplitudes(l: usize, peak: f64) -> Vec<f64> {
    (1..=l).map(|i| peak * (0.4 + 0.6 * ((i as f64) * 0.7).sin().abs())).collect()
}

/// `(l, peak)` pairs whose `R_M0` spans the real chip-measured range (peak amplitudes from `0.5`
/// -- near the measured minimum -- up to `3000`, close to the measured maximum's own implied peak).
fn sweep_cases() -> Vec<(usize, f64)> {
    let mut cases = Vec::new();
    for &l in &[9usize, 20, 40, 56] {
        for &peak in &[0.5f64, 5.0, 50.0, 500.0, 3000.0] {
            cases.push((l, peak));
        }
    }
    cases
}

#[test]
fn energy_and_scaled_energy_match() {
    for (l, peak) in sweep_cases() {
        let amps = realistic_amplitudes(l, peak);
        let amps_q16: Vec<i32> = amps.iter().map(|&v| to_q16(v)).collect();
        let omega0 = 0.05;
        let omega0_q16 = to_q16(omega0);

        assert_close_i64(
            &format!("energy l={l} peak={peak}"),
            float_enh::energy(&amps),
            fixed_enh::energy_q16(&amps_q16),
        );
        assert_close_i64(
            &format!("scaled_energy l={l} peak={peak}"),
            float_enh::scaled_energy(&amps, omega0),
            fixed_enh::scaled_energy_q16(&amps_q16, omega0_q16),
        );
    }
}

#[test]
fn enhance_spectral_amplitudes_matches() {
    for (l, peak) in sweep_cases() {
        let amps = realistic_amplitudes(l, peak);
        let amps_q16: Vec<i32> = amps.iter().map(|&v| to_q16(v)).collect();
        let omega0 = 0.05;
        let omega0_q16 = to_q16(omega0);

        let float_result = float_enh::enhance_spectral_amplitudes(&amps, omega0);
        let fixed_result = fixed_enh::enhance_spectral_amplitudes_q16(&amps_q16, omega0_q16);
        assert_eq!(float_result.len(), fixed_result.len());
        for (h, (&fv, &fxv)) in float_result.iter().zip(fixed_result.iter()).enumerate() {
            assert_close(&format!("enhance l={l} peak={peak} h={h}"), fv, fxv);
        }
    }
}

#[test]
fn update_local_energy_matches() {
    // Real chip data: R_M0 from ~9 to ~4e8, S_E floored at 10000 -- covers the low, middle, and
    // high end of that measured range.
    for (prev, r_m0) in [
        (10000.0, 500.0),
        (50000.0, 100.0),
        (12000.0, 30000.0),
        (400_000_000.0, 850_000.0),
        (9.2, 400_000_000.0),
    ] {
        assert_close_i64(
            "update_local_energy",
            float_enh::update_local_energy(prev, r_m0),
            fixed_enh::update_local_energy_q16(to_q16_i64(prev), to_q16_i64(r_m0)),
        );
    }
}

#[test]
fn adaptive_voicing_threshold_matches_all_three_branches() {
    // Real chip data shows S_E can range from the 10000 floor up past 4e8 -- exercise both ends,
    // not just one representative value.
    for &s_e in &[15000.0, 4.0e8] {
        let s_e_i64 = to_q16_i64(s_e);

        // Branch 1: rate <= 0.005 && total <= 4 -> infinity. Compared by sentinel behavior, not value.
        let errors_f = FrameErrors { total: 2, rate: 0.001, golay_init: 0, hamming_init: 0 };
        let errors_q = ham_digital_modes::ambe::fixed::tia_102_baba::error_estimation::FrameErrorsQ16 {
            total: 2,
            rate_q16: to_q16(0.001),
            golay_init: 0,
            hamming_init: 0,
        };
        let float_v = float_enh::adaptive_voicing_threshold(&errors_f, s_e);
        let fixed_v = fixed_enh::adaptive_voicing_threshold_q16(&errors_q, s_e_i64);
        assert!(float_v.is_infinite() && fixed_v == i32::MAX, "branch 1 s_e={s_e}: float={float_v}, fixed={fixed_v}");

        // Branch 2: rate <= 0.0125 && hamming_init == 0.
        let errors_f = FrameErrors { total: 8, rate: 0.01, golay_init: 0, hamming_init: 0 };
        let errors_q = ham_digital_modes::ambe::fixed::tia_102_baba::error_estimation::FrameErrorsQ16 {
            total: 8,
            rate_q16: to_q16(0.01),
            golay_init: 0,
            hamming_init: 0,
        };
        let float_v = float_enh::adaptive_voicing_threshold(&errors_f, s_e);
        let fixed_v = fixed_enh::adaptive_voicing_threshold_q16(&errors_q, s_e_i64);
        if float_v > 32767.0 {
            // Past a plain Q16.16 i32's own real-valued ceiling: V_M's saturation to i32::MAX is
            // the documented, correct behavior (see `powf_wide_input_q16`'s own doc comment), not a
            // precision bug -- real M_l never reaches this range either, so V_M > M_l stays false.
            assert_eq!(fixed_v, i32::MAX, "branch 2 s_e={s_e} (saturation case): float={float_v}");
        } else {
            assert_close(&format!("branch 2 s_e={s_e}"), float_v, fixed_v);
        }

        // Branch 3: neither of the above.
        let errors_f = FrameErrors { total: 20, rate: 0.05, golay_init: 1, hamming_init: 2 };
        let errors_q = ham_digital_modes::ambe::fixed::tia_102_baba::error_estimation::FrameErrorsQ16 {
            total: 20,
            rate_q16: to_q16(0.05),
            golay_init: 1,
            hamming_init: 2,
        };
        let float_v = float_enh::adaptive_voicing_threshold(&errors_f, s_e);
        let fixed_v = fixed_enh::adaptive_voicing_threshold_q16(&errors_q, s_e_i64);
        if float_v > 32767.0 {
            assert_eq!(fixed_v, i32::MAX, "branch 3 s_e={s_e} (saturation case): float={float_v}");
        } else {
            assert_close(&format!("branch 3 s_e={s_e}"), float_v, fixed_v);
        }
    }
}

#[test]
fn smooth_voicing_decision_matches() {
    assert_eq!(
        float_enh::smooth_voicing_decision(10.0, false, 5.0),
        fixed_enh::smooth_voicing_decision_q16(to_q16(10.0), false, to_q16(5.0))
    );
    assert_eq!(
        float_enh::smooth_voicing_decision(3.0, false, 5.0),
        fixed_enh::smooth_voicing_decision_q16(to_q16(3.0), false, to_q16(5.0))
    );
    assert_eq!(
        float_enh::smooth_voicing_decision(3.0, true, 5.0),
        fixed_enh::smooth_voicing_decision_q16(to_q16(3.0), true, to_q16(5.0))
    );
}

#[test]
fn amplitude_sum_matches() {
    for &peak in &[0.5f64, 5.0, 50.0, 500.0, 3000.0] {
        let amps = realistic_amplitudes(30, peak);
        let amps_q16: Vec<i32> = amps.iter().map(|&v| to_q16(v)).collect();
        assert_close_i64(
            &format!("amplitude_sum peak={peak}"),
            float_enh::amplitude_sum(&amps),
            fixed_enh::amplitude_sum_q16(&amps_q16),
        );
    }
}

#[test]
fn update_amplitude_threshold_matches_both_branches() {
    for &prev in &[15000.0f64, 200_000.0] {
        // Branch 1: rate <= 0.005 && total <= 6.
        let errors_f = FrameErrors { total: 3, rate: 0.001, golay_init: 0, hamming_init: 0 };
        let errors_q = ham_digital_modes::ambe::fixed::tia_102_baba::error_estimation::FrameErrorsQ16 {
            total: 3,
            rate_q16: to_q16(0.001),
            golay_init: 0,
            hamming_init: 0,
        };
        assert_close_i64(
            &format!("amplitude threshold branch 1 prev={prev}"),
            float_enh::update_amplitude_threshold(&errors_f, prev),
            fixed_enh::update_amplitude_threshold_q16(&errors_q, to_q16_i64(prev)),
        );

        // Branch 2.
        let errors_f = FrameErrors { total: 15, rate: 0.05, golay_init: 1, hamming_init: 1 };
        let errors_q = ham_digital_modes::ambe::fixed::tia_102_baba::error_estimation::FrameErrorsQ16 {
            total: 15,
            rate_q16: to_q16(0.05),
            golay_init: 1,
            hamming_init: 1,
        };
        assert_close_i64(
            &format!("amplitude threshold branch 2 prev={prev}"),
            float_enh::update_amplitude_threshold(&errors_f, prev),
            fixed_enh::update_amplitude_threshold_q16(&errors_q, to_q16_i64(prev)),
        );
    }
}

#[test]
fn amplitude_smoothing_scale_matches_both_branches() {
    for &(tau, a) in &[(100.0f64, 50.0), (30.0, 50.0), (200_000.0, 150_000.0), (150_000.0, 200_000.0)] {
        assert_close(
            &format!("smoothing scale tau={tau} a={a}"),
            float_enh::amplitude_smoothing_scale(tau, a),
            fixed_enh::amplitude_smoothing_scale_q16(to_q16_i64(tau), to_q16_i64(a)),
        );
    }
}

#[test]
fn error_estimation_matches() {
    let counts = [1u32, 0, 2, 1, 0, 3, 1];
    let previous_rate = 0.02;
    let float_errors = estimate_errors(&counts, previous_rate);
    let fixed_errors = estimate_errors_q16(&counts, to_q16(previous_rate));

    assert_eq!(float_errors.total, fixed_errors.total);
    assert_close("error rate", float_errors.rate, fixed_errors.rate_q16);
    assert_eq!(float_errors.golay_init, fixed_errors.golay_init);
    assert_eq!(float_errors.hamming_init, fixed_errors.hamming_init);
    assert_eq!(should_repeat_frame(&float_errors), should_repeat_frame_q16(&fixed_errors));
    assert_eq!(should_mute_frame(&float_errors), should_mute_frame_q16(&fixed_errors));

    // A second, higher-error case exercising the other side of both thresholds.
    let counts2 = [3u32, 5, 4, 6, 5, 4, 3];
    let float_errors2 = estimate_errors(&counts2, float_errors.rate);
    let fixed_errors2 = estimate_errors_q16(&counts2, fixed_errors.rate_q16);
    assert_close("error rate 2", float_errors2.rate, fixed_errors2.rate_q16);
    assert_eq!(should_repeat_frame(&float_errors2), should_repeat_frame_q16(&fixed_errors2));
    assert_eq!(should_mute_frame(&float_errors2), should_mute_frame_q16(&fixed_errors2));
}
