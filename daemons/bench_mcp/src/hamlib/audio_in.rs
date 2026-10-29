// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rig's receive audio, taken from a sound input of the `audio` module. The two modules share names: the rig entry's `audio`
//! setting names an audio instrument of the bench config and the alias (or node name) of its input, and the device is found, and
//! reserved devices are refused, by the sound module's own code. The stream names its target node and does not fall back to the
//! system default.

use crate::audio::devices::{self, Dir};
use crate::audio::engine::Recorder;
use crate::audio::Process;
use crate::config::{Instrument, Kind};
use std::path::Path;
use std::time::Duration;

pub const RATE: u32 = 48_000;

/// Where samples come from: production reads a PipeWire input, tests give recordings.
pub trait AudioSource {
    /// `seconds` of mono samples at `rate()`.
    fn capture(&self, seconds: f64) -> Result<Vec<f32>, String>;
    fn rate(&self) -> u32;
    fn describe(&self) -> String;
}

pub struct PipeWireInput {
    node: String,
}

impl AudioSource for PipeWireInput {
    fn capture(&self, seconds: f64) -> Result<Vec<f32>, String> {
        // [@ANCHOR: hamlib_audio_capture]
        if !(0.1..=30.0).contains(&seconds) {
            return Err(format!(
                "{seconds} s is outside the 0.1 to 30 s a capture may last"
            ));
        }
        let n = (seconds * f64::from(RATE)) as usize;
        let mut rec = Recorder::start(&self.node, RATE, false)?;
        rec.wait_samples(n, Duration::from_secs_f64(seconds + 4.0))?;
        let mut all = rec.stop();
        all.truncate(n);
        Ok(all)
    }

    fn rate(&self) -> u32 {
        RATE
    }

    fn describe(&self) -> String {
        format!("PipeWire input {}", self.node)
    }
}

/// The audio input of a rig, or `None` when the rig has no `audio` setting. A configured input that cannot be found is an error.
pub fn resolve(insts: &[Instrument], rig: &Instrument) -> Result<Option<PipeWireInput>, String> {
    // [@ANCHOR: hamlib_audio_resolve]
    let Some(a) = &rig.hamlib.audio else {
        return Ok(None);
    };
    let sound = insts
        .iter()
        .find(|i| i.kind == Kind::Audio && i.name == a.instrument)
        .ok_or_else(|| {
            format!(
                "rig {:?} names the audio instrument {:?}, which is not in the bench config",
                rig.name, a.instrument
            )
        })?;
    let devs = devices::list(&Process, Path::new("/proc/asound"), &sound.audio)?;
    let dev = devices::resolve(&devs, &a.input, Dir::Input)?;
    Ok(Some(PipeWireInput {
        node: dev.node_name.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: hamlib_audio_capture]
    #[test]
    fn a_capture_has_bounds_and_a_missing_node_is_an_error_not_a_fall_back_to_the_default_input() {
        let src = PipeWireInput {
            node: "bench_mcp_no_such_node".into(),
        };
        assert!(src.capture(0.0).unwrap_err().contains("outside"));
        assert!(src.capture(31.0).unwrap_err().contains("outside"));
        // The stream names its target and refuses to move to another device when it is missing (pw-record, or its absence, is the error).
        assert!(src.capture(0.5).is_err());
        assert_eq!(
            (src.rate(), src.describe()),
            (RATE, "PipeWire input bench_mcp_no_such_node".to_string())
        );
    }
}
