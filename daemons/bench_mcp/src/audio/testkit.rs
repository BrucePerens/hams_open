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
