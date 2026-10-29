// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test helpers: the recorded sound graph of the development machine (`tests/audio_fixtures/devbox`) and a program runner
//! that answers `pw-dump` from it and scripts the rest.

use super::devices::{self, Device};
use super::{Config, Run};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub fn recorded() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/audio_fixtures/devbox")
}

pub fn devbox_devices() -> Vec<Device> {
    let dump = std::fs::read_to_string(recorded().join("pw-dump.json")).unwrap();
    devices::parse(&dump, &recorded(), &Config::default()).unwrap()
}

/// Answers `pw-dump` with the recording and `wpctl get-volume` from a table, and records every other call.
pub struct Scripted {
    pub volumes: RefCell<Vec<(u32, f64, bool)>>,
    pub calls: RefCell<Vec<String>>,
}

impl Scripted {
    pub fn new(volumes: &[(u32, f64, bool)]) -> Scripted {
        Scripted {
            volumes: RefCell::new(volumes.to_vec()),
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl Run for Scripted {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
        self.calls
            .borrow_mut()
            .push(format!("{program} {}", args.join(" ")));
        match (program, args.first().map(|s| s.as_str())) {
            ("pw-dump", _) => Ok((
                true,
                std::fs::read_to_string(recorded().join("pw-dump.json")).unwrap(),
            )),
            ("wpctl", Some("get-volume")) => {
                let id: u32 = args[1].parse().unwrap();
                let v = self.volumes.borrow();
                match v.iter().find(|(i, _, _)| *i == id) {
                    Some((_, vol, m)) => Ok((
                        true,
                        format!("Volume: {vol:.2}{}\n", if *m { " [MUTED]" } else { "" }),
                    )),
                    None => Ok((false, "Failed to find node".into())),
                }
            }
            ("wpctl", Some("set-volume")) => {
                let id: u32 = args[1].parse().unwrap();
                let vol: f64 = args[2].parse().unwrap();
                for e in self.volumes.borrow_mut().iter_mut().filter(|e| e.0 == id) {
                    e.1 = vol;
                }
                Ok((true, String::new()))
            }
            ("pkill", _) => Ok((false, String::new())),
            ("wpctl", Some("set-mute")) => {
                let id: u32 = args[1].parse().unwrap();
                let m = args[2] == "1";
                for e in self.volumes.borrow_mut().iter_mut().filter(|e| e.0 == id) {
                    e.2 = m;
                }
                Ok((true, String::new()))
            }
            _ => Err(format!("unscripted {program}")),
        }
    }
}

/// The directory of stand-in programs that `fake_programs` put at the front of `PATH`.
static FAKES: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Puts stand-ins for `espeak-ng`, `spd-say`, `pw-play` and `pw-record` at the front of `PATH` (once per test process) so the
/// code that starts those programs by name runs unchanged and nothing reaches the sound server. `espeak-ng -w FILE` copies
/// `espeak-<voice>.wav` from the returned directory (written by the test) and `espeak-ng --voices` prints the recorded list;
/// `pw-record --target NODE` prints the raw f32 samples of `rec-NODE.f32` there and then stays open like a live stream;
/// `pw-play` and `spd-say` read their input and succeed.
pub fn fake_programs() -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    FAKES
        .get_or_init(|| {
            let dir = std::env::temp_dir().join(format!("audio-fakes-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let d = dir.display();
            let scripts = [
                (
                    "espeak-ng",
                    format!(
                        "#!/bin/sh\nif [ \"$1\" = \"--voices\" ]; then cat '{}'; exit 0; fi\n\
                         while [ $# -gt 0 ]; do case \"$1\" in -v) v=\"$2\";; -w) o=\"$2\";; esac; shift; done\n\
                         cat > /dev/null\ncp '{d}'/espeak-\"$v\".wav \"$o\"\n",
                        recorded().join("espeak-voices.txt").display()
                    ),
                ),
                ("spd-say", "#!/bin/sh\ncat > /dev/null\n".to_string()),
                ("pw-play", "#!/bin/sh\nexit 0\n".to_string()),
                (
                    "pw-record",
                    format!(
                        "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in --target) t=\"$2\";; esac; shift; done\n\
                         cat '{d}'/rec-\"$t\".f32\nexec sleep 60\n"
                    ),
                ),
            ];
            for (name, body) in scripts {
                let p = dir.join(name);
                std::fs::write(&p, body).unwrap();
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
            let old = std::env::var_os("PATH").unwrap_or_default();
            let mut paths = vec![dir.clone()];
            paths.extend(std::env::split_paths(&old));
            std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
            dir
        })
        .clone()
}

/// A scratch directory unique to the test process and `tag`.
pub fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("audio-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}
