// SPDX-License-Identifier: AGPL-3.0-or-later
//! Generated test signals: sine, logarithmic sweep, two-tone and noise, with a level in dBFS and short raised-cosine edges so
//! that no click is added.

use serde_json::Value;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tone {
    Sine { hz: f64 },
    Sweep { from_hz: f64, to_hz: f64 },
    Dual { hz1: f64, hz2: f64 },
    Noise,
}

/// The loudest generated level a tool accepts, in dBFS.
pub const MAX_LEVEL_DBFS: f64 = -1.0;

pub fn db_to_amp(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Generates `seconds` of the tone at `level_dbfs` (the peak of the signal). Two tones share the level, each 6 dB below it.
pub fn generate(t: Tone, rate: u32, seconds: f64, level_dbfs: f64) -> Vec<f32> {
    // [@ANCHOR: audio_tones]
    let n = (seconds * f64::from(rate)).round() as usize;
    let amp = db_to_amp(level_dbfs);
    let r = f64::from(rate);
    let edge = ((0.01 * r) as usize).min(n / 2).max(1);
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..n)
        .map(|i| {
            let time = i as f64 / r;
            let raw = match t {
                Tone::Sine { hz } => (2.0 * PI * hz * time).sin(),
                Tone::Sweep { from_hz, to_hz } => {
                    let k = (to_hz / from_hz).ln() / seconds.max(1e-9);
                    // The instantaneous frequency is from_hz * e^(k t); the phase is its integral.
                    (2.0 * PI * from_hz * ((k * time).exp() - 1.0) / k).sin()
                }
                Tone::Dual { hz1, hz2 } => {
                    0.5 * ((2.0 * PI * hz1 * time).sin() + (2.0 * PI * hz2 * time).sin())
                }
                Tone::Noise => {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state >> 11) as f64 / (1u64 << 52) as f64 - 1.0
                }
            };
            let fade = if i < edge {
                0.5 - 0.5 * (PI * i as f64 / edge as f64).cos()
            } else if i + edge >= n {
                0.5 - 0.5 * (PI * (n - 1 - i) as f64 / edge as f64).cos()
            } else {
                1.0
            };
            (raw * amp * fade) as f32
        })
        .collect()
}

fn hz(args: &Value, k: &str, default: Option<f64>, rate: u32) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_audio_tones_hz]
    let v = args.get(k).and_then(|x| x.as_f64()).or(default);
    let v = v.ok_or_else(|| format!("{k} is required for this tone"))?;
    let top = f64::from(rate) * 0.45;
    if !(20.0..=top).contains(&v) {
        return Err(format!("{k} {v} Hz is outside 20 Hz to {top:.0} Hz"));
    }
    Ok(v)
}

/// Reads a tone from tool arguments: `tone` (sine, sweep, two_tone, noise), `freq_hz`, `freq2_hz` (sweep end, second tone).
pub fn parse(args: &Value, rate: u32) -> Result<Tone, String> {
    // [@ANCHOR: bench_mcp_audio_tones_parse]
    match args.get("tone").and_then(|t| t.as_str()).unwrap_or("sine") {
        "sine" => Ok(Tone::Sine {
            hz: hz(args, "freq_hz", Some(1000.0), rate)?,
        }),
        "sweep" => Ok(Tone::Sweep {
            from_hz: hz(args, "freq_hz", Some(200.0), rate)?,
            to_hz: hz(args, "freq2_hz", Some(4000.0), rate)?,
        }),
        "two_tone" => Ok(Tone::Dual {
            hz1: hz(args, "freq_hz", Some(700.0), rate)?,
            hz2: hz(args, "freq2_hz", Some(1900.0), rate)?,
        }),
        "noise" => Ok(Tone::Noise),
        o => Err(format!("tone {o:?} is not sine, sweep, two_tone or noise")),
    }
}

/// The level argument, `level_dbfs` (default -12), limited to [`MAX_LEVEL_DBFS`].
pub fn level(args: &Value, default: f64) -> Result<f64, String> {
    // [@ANCHOR: bench_mcp_audio_tones_level]
    let v = args
        .get("level_dbfs")
        .and_then(|x| x.as_f64())
        .unwrap_or(default);
    if !(-90.0..=MAX_LEVEL_DBFS).contains(&v) {
        return Err(format!(
            "level_dbfs {v} is outside -90 to {MAX_LEVEL_DBFS} dBFS"
        ));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Tests [@ANCHOR: audio_tones]
    // Tests [@ANCHOR: bench_mcp_audio_tones_hz]
    #[test]
    fn a_sine_has_the_requested_peak_and_fades_at_its_edges() {
        let s = generate(Tone::Sine { hz: 1000.0 }, 48000, 0.5, -12.0);
        assert_eq!(s.len(), 24000);
        let peak = s.iter().fold(0f32, |m, x| m.max(x.abs()));
        assert!((f64::from(peak) - db_to_amp(-12.0)).abs() < 0.01, "{peak}");
        assert!(s[0].abs() < 1e-3 && s[s.len() - 1].abs() < 0.01);
    }

    #[test]
    fn a_two_tone_stays_under_its_level_and_noise_is_deterministic_and_bounded() {
        let t = generate(
            Tone::Dual {
                hz1: 700.0,
                hz2: 1900.0,
            },
            48000,
            0.3,
            -6.0,
        );
        assert!(t
            .iter()
            .all(|x| f64::from(x.abs()) <= db_to_amp(-6.0) + 1e-6));
        let a = generate(Tone::Noise, 48000, 0.1, -20.0);
        assert_eq!(a, generate(Tone::Noise, 48000, 0.1, -20.0));
        assert!(a
            .iter()
            .all(|x| f64::from(x.abs()) <= db_to_amp(-20.0) + 1e-6));
        let mean: f64 = a.iter().map(|x| f64::from(*x)).sum::<f64>() / a.len() as f64;
        assert!(mean.abs() < 0.01);
    }

    // Tests [@ANCHOR: bench_mcp_audio_tones_level]
    // Tests [@ANCHOR: bench_mcp_audio_tones_parse]
    #[test]
    fn tone_arguments_are_validated() {
        assert_eq!(parse(&json!({}), 48000).unwrap(), Tone::Sine { hz: 1000.0 });
        assert!(parse(&json!({"freq_hz": 5}), 48000).is_err());
        assert!(parse(&json!({"freq_hz": 30000}), 48000).is_err());
        assert!(parse(&json!({"tone": "square"}), 48000).is_err());
        assert!(matches!(
            parse(&json!({"tone": "sweep"}), 48000).unwrap(),
            Tone::Sweep { .. }
        ));
        assert_eq!(level(&json!({}), -12.0).unwrap(), -12.0);
        assert!(level(&json!({"level_dbfs": 0}), -12.0).is_err());
        assert!(level(&json!({"level_dbfs": -120}), -12.0).is_err());
    }
}
