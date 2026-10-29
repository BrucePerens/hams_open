// SPDX-License-Identifier: AGPL-3.0-or-later
//! Volume and mute through `wpctl`. The volume scale is wpctl's (0.0 to 1.0 and above, the cube root of the signal amplitude
//! factor, so 0.5 is about -18 dB); the volume cap and quiet volume of the config are in the same scale.

use super::Run;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub volume: f64,
    pub muted: bool,
}

/// Parses `wpctl get-volume` output: `Volume: 0.52` or `Volume: 0.40 [MUTED]`.
pub fn parse_get_volume(s: &str) -> Result<Level, String> {
    // [@ANCHOR: audio_levels_parse]
    let rest = s
        .trim()
        .strip_prefix("Volume:")
        .ok_or_else(|| format!("unexpected wpctl output {:?}", s.trim()))?;
    let mut it = rest.split_whitespace();
    let volume: f64 = it
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("no volume number in {:?}", s.trim()))?;
    Ok(Level {
        volume,
        muted: it.any(|w| w == "[MUTED]"),
    })
}

/// The gain in dB for a wpctl volume: the signal amplitude factor is the volume cubed, so 60 log10(volume).
pub fn gain_db(volume: f64) -> f64 {
    60.0 * volume.log10()
}

pub fn get(run: &dyn Run, node_id: u32) -> Result<Level, String> {
    // [@ANCHOR: bench_mcp_audio_levels_get]
    let (ok, text) = run.run("wpctl", &["get-volume".into(), node_id.to_string()])?;
    if !ok {
        return Err(format!("wpctl get-volume {node_id}: {}", text.trim()));
    }
    parse_get_volume(&text)
}

pub fn set_volume(run: &dyn Run, node_id: u32, volume: f64) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_audio_levels_set_volume]
    let (ok, text) = run.run(
        "wpctl",
        &[
            "set-volume".into(),
            node_id.to_string(),
            format!("{volume:.3}"),
        ],
    )?;
    if ok {
        Ok(())
    } else {
        Err(format!("wpctl set-volume {node_id}: {}", text.trim()))
    }
}

pub fn set_mute(run: &dyn Run, node_id: u32, mute: bool) -> Result<(), String> {
    // [@ANCHOR: bench_mcp_audio_levels_set_mute]
    let (ok, text) = run.run(
        "wpctl",
        &[
            "set-mute".into(),
            node_id.to_string(),
            (mute as u8).to_string(),
        ],
    )?;
    if ok {
        Ok(())
    } else {
        Err(format!("wpctl set-mute {node_id}: {}", text.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: audio_levels_parse]
    #[test]
    fn wpctl_volume_lines_parse_with_and_without_mute() {
        assert_eq!(
            parse_get_volume("Volume: 0.52\n").unwrap(),
            Level {
                volume: 0.52,
                muted: false
            }
        );
        assert_eq!(
            parse_get_volume("Volume: 0.40 [MUTED]\n").unwrap(),
            Level {
                volume: 0.40,
                muted: true
            }
        );
        assert_eq!(parse_get_volume("Volume: 1.00").unwrap().volume, 1.0);
    }

    #[test]
    fn unexpected_wpctl_output_is_an_error() {
        assert!(parse_get_volume("").is_err());
        assert!(parse_get_volume("Failed to get volume").is_err());
        assert!(parse_get_volume("Volume: loud").is_err());
    }

    #[test]
    fn the_gain_is_sixty_log_volume() {
        assert!((gain_db(0.5) - 20.0 * 0.125f64.log10()).abs() < 1e-9);
        assert_eq!(gain_db(1.0), 0.0);
    }

    struct Failing;
    impl Run for Failing {
        fn run(&self, _p: &str, _a: &[String]) -> Result<(bool, String), String> {
            Ok((false, "Failed to find node\n".into()))
        }
    }

    // Tests [@ANCHOR: bench_mcp_audio_levels_get]
    // Tests [@ANCHOR: bench_mcp_audio_levels_set_volume]
    // Tests [@ANCHOR: bench_mcp_audio_levels_set_mute]
    #[test]
    fn levels_round_trip_through_wpctl_and_failures_are_errors() {
        let run = super::super::testkit::Scripted::new(&[(7, 0.5, false)]);
        assert_eq!(get(&run, 7).unwrap().volume, 0.5);
        set_volume(&run, 7, 0.25).unwrap();
        set_mute(&run, 7, true).unwrap();
        assert_eq!(
            get(&run, 7).unwrap(),
            Level {
                volume: 0.25,
                muted: true
            }
        );
        assert!(run
            .calls
            .borrow()
            .contains(&"wpctl set-volume 7 0.250".to_string()));
        assert!(run
            .calls
            .borrow()
            .contains(&"wpctl set-mute 7 1".to_string()));
        assert!(get(&Failing, 7).unwrap_err().contains("get-volume 7"));
        assert!(set_volume(&Failing, 7, 0.1)
            .unwrap_err()
            .contains("set-volume 7"));
        assert!(set_mute(&Failing, 7, false)
            .unwrap_err()
            .contains("set-mute 7"));
    }
}
