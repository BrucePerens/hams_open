// SPDX-License-Identifier: AGPL-3.0-or-later
//! Test helpers: a scratch directory, a writable copy of the recorded sysfs, and a scripted hub that removes and
//! restores a port's device in that copy the way a real power cycle does.

use super::uhubctl::Runner;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

pub struct Dir(PathBuf);

impl Dir {
    pub fn new() -> Dir {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "usbhub-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        Dir(p)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn recorded() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/usb_fixtures/devbox")
}

pub fn recorded_sys() -> PathBuf {
    recorded().join("sys")
}

fn copy_all(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let ft = e.file_type().unwrap();
        if ft.is_symlink() {
            std::os::unix::fs::symlink(std::fs::read_link(&src).unwrap(), &dst).unwrap();
        } else if ft.is_dir() {
            copy_all(&src, &dst);
        } else {
            std::fs::copy(&src, &dst).unwrap();
        }
    }
}

/// A writable copy of the recorded `/sys`.
pub fn writable_sys() -> (Dir, PathBuf) {
    let d = Dir::new();
    let sys = d.path().join("sys");
    copy_all(&recorded_sys(), &sys);
    (d, sys)
}

/// What the scripted hub does when asked to switch a port off.
#[derive(Clone, Copy, PartialEq)]
pub enum Behaviour {
    /// The device disappears and comes back with a new device number.
    Switches,
    /// The command succeeds but the device stays (a hub that does not really switch).
    Ignores,
    /// The command fails with this text.
    Fails,
}

/// A stand-in for `sudo -n uhubctl` that edits a copy of sysfs. It answers a plain `uhubctl` status request from the recording.
pub struct FakeHub {
    pub sys: PathBuf,
    pub behaviour: Behaviour,
    pub calls: RefCell<Vec<String>>,
    parked: RefCell<Option<(PathBuf, PathBuf, String)>>,
    pub next_devnum: RefCell<u32>,
}

impl FakeHub {
    pub fn new(sys: &Path, behaviour: Behaviour) -> FakeHub {
        FakeHub {
            sys: sys.to_path_buf(),
            behaviour,
            calls: RefCell::new(vec![]),
            parked: RefCell::new(None),
            next_devnum: RefCell::new(90),
        }
    }

    fn links(&self, path: &str) -> Vec<(String, PathBuf)> {
        let dir = self.sys.join("bus/usb/devices");
        let mut v = Vec::new();
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            let under = n == path
                || n.starts_with(&format!("{path}."))
                || n.starts_with(&format!("{path}:"));
            if under {
                v.push((n, std::fs::read_link(e.path()).unwrap()));
            }
        }
        v
    }

    fn off(&self, path: &str) {
        let links = self.links(path);
        let real = std::fs::canonicalize(self.sys.join("bus/usb/devices").join(path)).unwrap();
        for (n, _) in &links {
            std::fs::remove_file(self.sys.join("bus/usb/devices").join(n)).unwrap();
        }
        let park = self.sys.parent().unwrap().join(format!("parked-{path}"));
        std::fs::rename(&real, &park).unwrap();
        let saved = links
            .iter()
            .map(|(n, t)| format!("{n}\t{}", t.display()))
            .collect::<Vec<_>>()
            .join("\n");
        *self.parked.borrow_mut() = Some((park, real, saved));
    }

    fn on(&self, path: &str) {
        let Some((park, real, saved)) = self.parked.borrow_mut().take() else {
            return;
        };
        std::fs::rename(&park, &real).unwrap();
        let mut n = self.next_devnum.borrow_mut();
        *n += 1;
        std::fs::write(real.join("devnum"), format!("{}\n", *n)).unwrap();
        for line in saved.lines() {
            let (name, target) = line.split_once('\t').unwrap();
            std::os::unix::fs::symlink(target, self.sys.join("bus/usb/devices").join(name))
                .unwrap();
        }
        let _ = path;
    }
}

impl Runner for FakeHub {
    fn run(&self, program: &str, args: &[String]) -> Result<(bool, String), String> {
        self.calls
            .borrow_mut()
            .push(format!("{program} {}", args.join(" ")));
        let has = |a: &str| args.iter().any(|x| x == a);
        let after = |flag: &str| {
            args.iter()
                .position(|x| x == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        if !has("-a") {
            let text = std::fs::read_to_string(recorded().join("uhubctl_nodesc.txt")).unwrap();
            return Ok((true, text));
        }
        let (hub, port) = (after("-l").unwrap(), after("-p").unwrap());
        let path = super::sysfs::child_path(&hub, port.parse().unwrap());
        match (self.behaviour, after("-a").as_deref()) {
            (Behaviour::Fails, _) => Ok((false, "No compatible devices detected".into())),
            (Behaviour::Switches, Some("off")) => {
                self.off(&path);
                Ok((true, String::new()))
            }
            (Behaviour::Switches, Some("on")) => {
                self.on(&path);
                Ok((true, String::new()))
            }
            _ => Ok((true, String::new())),
        }
    }
}
