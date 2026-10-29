// SPDX-License-Identifier: AGPL-3.0-or-later
//! The limits on anything audible: the volume cap (global and per output), quiet hours, and a guard that sets a device's volume
//! for the length of an operation and puts the volume and mute back afterwards.

use super::devices::Device;
use super::levels::{self, Level};
use super::{Config, Run};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// The highest wpctl volume allowed now.
    pub volume: f64,
    pub quiet: bool,
}

/// The limits for audible output on `dev` at `minute` of the local day. In quiet hours the limit is the quiet volume, except for
/// an urgent request, which gets the device's cap.
pub fn limits(cfg: &Config, dev: &Device, urgent: bool, minute: u32) -> Limits {
    // [@ANCHOR: audio_safety_limits]
    let cap = cfg.cap_for(dev);
    let quiet = cfg.quiet_hours.is_some_and(|q| q.contains(minute));
    Limits {
        volume: if quiet && !urgent {
            cap.min(cfg.quiet_volume)
        } else {
            cap
        },
        quiet,
    }
}

/// What to do with the volume before making sound.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Want {
    /// Leave it, but lower it to the limit if it is above.
    Keep,
    /// Set it to this volume (or the limit, if lower).
    Level(f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Applied {
    pub before: Level,
    pub volume: f64,
    pub muted: bool,
}

pub struct VolumeGuard<'a> {
    run: &'a dyn Run,
    id: u32,
    before: Level,
    done: bool,
}

impl<'a> VolumeGuard<'a> {
    pub fn apply(
        run: &'a dyn Run,
        dev: &Device,
        lim: &Limits,
        want: Want,
        unmute: bool,
    ) -> Result<(VolumeGuard<'a>, Applied), String> {
        // [@ANCHOR: audio_safety_guard]
        if let Some(why) = &dev.reserved {
            return Err(format!("{} is reserved: {why}", dev.node_name));
        }
        let before = levels::get(run, dev.id)?;
        let target = match want {
            Want::Keep => before.volume.min(lim.volume),
            Want::Level(v) => v.min(lim.volume),
        };
        let mut guard = VolumeGuard {
            run,
            id: dev.id,
            before,
            done: false,
        };
        let mut muted = before.muted;
        if (target - before.volume).abs() > 0.004 {
            levels::set_volume(run, dev.id, target)?;
        }
        if muted && unmute {
            if let Err(e) = levels::set_mute(run, dev.id, false) {
                let _ = guard.restore();
                return Err(e);
            }
            muted = false;
        }
        // A changed state needs a restore; an untouched one does not.
        guard.done = (target - before.volume).abs() <= 0.004 && muted == before.muted;
        Ok((
            guard,
            Applied {
                before,
                volume: target,
                muted,
            },
        ))
    }

    /// Sets the volume for the next step of an operation that raises it in stages; the caller keeps within the limit. The first
    /// `apply` recorded the original state, which `restore` still puts back.
    pub fn set(&mut self, volume: f64) -> Result<(), String> {
        self.done = false;
        levels::set_volume(self.run, self.id, volume)
    }

    /// Puts the volume and mute back. Idempotent.
    pub fn restore(&mut self) -> Result<(), String> {
        // [@ANCHOR: bench_mcp_audio_safety_volumeguard_restore]
        if self.done {
            return Ok(());
        }
        self.done = true;
        levels::set_volume(self.run, self.id, self.before.volume)?;
        levels::set_mute(self.run, self.id, self.before.muted)
    }
}

impl Drop for VolumeGuard<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.restore() {
            eprintln!(
                "audio: could not restore the volume of node {}: {e}",
                self.id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::testkit::{devbox_devices, Scripted};
    use crate::audio::QuietHours;

    fn speaker() -> Device {
        devbox_devices()
            .into_iter()
            .find(|d| d.description.ends_with("Speaker"))
            .unwrap()
    }

    // Tests [@ANCHOR: audio_safety_limits]
    #[test]
    fn quiet_hours_lower_the_limit_unless_urgent_and_a_port_cap_wins() {
        let mut cfg = Config::default();
        let d = speaker();
        let day = 12 * 60;
        let night = 23 * 60;
        assert_eq!(
            limits(&cfg, &d, false, day),
            Limits {
                volume: 0.5,
                quiet: false
            }
        );
        assert_eq!(
            limits(&cfg, &d, false, night),
            Limits {
                volume: 0.15,
                quiet: true
            }
        );
        assert_eq!(
            limits(&cfg, &d, true, night),
            Limits {
                volume: 0.5,
                quiet: true
            }
        );
        cfg.quiet_hours = None;
        assert!(!limits(&cfg, &d, false, night).quiet);
        cfg.outputs.insert(
            "desk".into(),
            crate::audio::Port {
                matcher: "Speaker".into(),
                volume_cap: Some(0.3),
            },
        );
        let mut d = d;
        d.alias = Some("desk".into());
        assert_eq!(limits(&cfg, &d, true, night).volume, 0.3);
        cfg.quiet_hours = Some(QuietHours {
            start: 1320,
            end: 420,
        });
        cfg.quiet_volume = 0.1;
        assert_eq!(limits(&cfg, &d, false, night).volume, 0.1);
    }

    // Tests [@ANCHOR: audio_safety_guard]
    #[test]
    fn the_guard_lowers_to_the_cap_unmutes_and_restores_both() {
        let d = speaker();
        let run = Scripted::new(&[(d.id, 0.9, true)]);
        let lim = Limits {
            volume: 0.2,
            quiet: false,
        };
        {
            let (_g, a) = VolumeGuard::apply(&run, &d, &lim, Want::Keep, true).unwrap();
            assert_eq!((a.volume, a.muted), (0.2, false));
            assert_eq!(run.volumes.borrow()[0].1, 0.2);
            assert!(!run.volumes.borrow()[0].2);
        }
        assert_eq!(*run.volumes.borrow(), vec![(d.id, 0.9, true)]);
    }

    // Tests [@ANCHOR: bench_mcp_audio_engine_playing_wait]
    // Tests [@ANCHOR: bench_mcp_audio_engine_recorder_shutdown]
    // Tests [@ANCHOR: bench_mcp_audio_safety_volumeguard_restore]
    #[test]
    fn keep_never_raises_and_a_level_request_never_passes_the_limit() {
        let d = speaker();
        let run = Scripted::new(&[(d.id, 0.1, false)]);
        let lim = Limits {
            volume: 0.3,
            quiet: false,
        };
        let (g, a) = VolumeGuard::apply(&run, &d, &lim, Want::Keep, false).unwrap();
        assert_eq!(a.volume, 0.1);
        drop(g);
        let (mut g, a) = VolumeGuard::apply(&run, &d, &lim, Want::Level(0.9), false).unwrap();
        assert_eq!(a.volume, 0.3);
        g.restore().unwrap();
        g.restore().unwrap();
        assert_eq!(run.volumes.borrow()[0].1, 0.1);
    }

    #[test]
    fn a_reserved_device_is_refused_before_anything_is_asked() {
        let mut d = speaker();
        d.reserved = Some("test".into());
        let run = Scripted::new(&[(d.id, 0.5, false)]);
        assert!(VolumeGuard::apply(
            &run,
            &d,
            &Limits {
                volume: 0.2,
                quiet: false
            },
            Want::Keep,
            false
        )
        .is_err());
        assert!(run.calls.borrow().is_empty());
    }
}
