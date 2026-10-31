// SPDX-License-Identifier: AGPL-3.0-or-later
//! The open policy: which device nodes a worker may be handed. Three checks in order, each
//! closing a different hole: `check_path` is lexical (absolute, under the device root, no `.`
//! or `..`, not on the deny list); `check_node` walks the real nodes (no symlink anywhere in the
//! chain, final node is a character device whose major belongs to the requested class, USB
//! identity not on the protected list); `check_fd` repeats the kind test on the opened descriptor
//! so a swap between lstat and open (TOCTOU) still fails closed.

use std::path::{Path, PathBuf};

use crate::os::{NodeKind, Os};
use crate::wire::{DeviceClass, MAX_STRING};

/// Why an open was refused. Short, fixed, loggable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denial {
    /// Not an absolute path.
    NotAbsolute,
    /// Not under the device root.
    OutsideRoot,
    /// Contains `.`/`..` or an empty component.
    Traversal,
    /// Longer than the wire allows or contains NUL.
    TooLong,
    /// Explicitly on the deny list.
    DenyListed,
    /// A symlink was found in the chain.
    Symlink,
    /// Not a character device (a disk, a file, a directory, a missing node).
    NotCharDevice,
    /// A character device of another class than requested.
    WrongClass,
    /// The USB vendor:product is protected (for example the VNWA codec).
    ProtectedUsb,
}

impl Denial {
    // [@ANCHOR: relay_broker_denial_reason]
    /// A stable short reason for the audit log and the response.
    pub fn reason(self) -> &'static str {
        match self {
            Denial::NotAbsolute => "path not absolute",
            Denial::OutsideRoot => "path outside device root",
            Denial::Traversal => "path traversal",
            Denial::TooLong => "path too long",
            Denial::DenyListed => "device on deny list",
            Denial::Symlink => "symlink in device path",
            Denial::NotCharDevice => "not a character device",
            Denial::WrongClass => "device class mismatch",
            Denial::ProtectedUsb => "protected usb device",
        }
    }
}

/// The policy table. Loaded from the root-owned config; defaults are the Linux conventions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Every allowed node lives under this directory (`/dev`).
    pub device_root: PathBuf,
    /// Character-device majors that are serial ttys.
    pub serial_majors: Vec<u32>,
    /// Character-device majors that are ALSA nodes.
    pub audio_majors: Vec<u32>,
    /// USB vendor:product pairs never handed out.
    pub deny_usb: Vec<(u16, u16)>,
    /// Exact paths never handed out, whatever they are.
    pub deny_paths: Vec<PathBuf>,
}

/// The VNWA's USB codec: never a radio, and opening it disturbs the instrument.
pub const VNWA_CODEC: (u16, u16) = (0x08bb, 0x29c0);

impl Default for Policy {
    fn default() -> Policy {
        Policy {
            device_root: PathBuf::from("/dev"),
            // 4 ttyS (legacy), 166 ttyACM, 188 ttyUSB, 204 and 207 vendor serial ranges.
            serial_majors: vec![4, 166, 188, 204, 207],
            // 116 is ALSA.
            audio_majors: vec![116],
            deny_usb: vec![VNWA_CODEC],
            deny_paths: [
                "/dev/mem",
                "/dev/kmem",
                "/dev/port",
                "/dev/console",
                "/dev/tty",
            ]
            .iter()
            .map(PathBuf::from)
            .collect(),
        }
    }
}

impl Policy {
    // [@ANCHOR: relay_broker_policy_check_path]
    /// The lexical check. Returns the normalised path on success. No filesystem access.
    pub fn check_path(&self, path: &str) -> Result<PathBuf, Denial> {
        if path.len() > MAX_STRING || path.contains('\0') {
            return Err(Denial::TooLong);
        }
        let p = Path::new(path);
        if !p.is_absolute() {
            return Err(Denial::NotAbsolute);
        }
        // Textual, not `Path::components()`: that API silently drops `.` segments, and a path
        // the policy accepts must be byte-for-byte the one that is opened.
        if path
            .split('/')
            .skip(1)
            .any(|seg| seg.is_empty() || seg == "." || seg == "..")
        {
            return Err(Denial::Traversal);
        }
        let clean = PathBuf::from(path);
        if !clean.starts_with(&self.device_root) || clean == self.device_root {
            return Err(Denial::OutsideRoot);
        }
        if self.deny_paths.contains(&clean) {
            return Err(Denial::DenyListed);
        }
        Ok(clean)
    }

    // [@ANCHOR: relay_broker_policy_class_of_major]
    /// The class a character-device major belongs to under this policy.
    pub fn class_of_major(&self, major: u32) -> Option<DeviceClass> {
        if self.serial_majors.contains(&major) {
            Some(DeviceClass::Serial)
        } else if self.audio_majors.contains(&major) {
            Some(DeviceClass::Audio)
        } else {
            None
        }
    }

    // [@ANCHOR: relay_broker_policy_check_kind]
    /// Accepts a node kind only when it is a character device of `class`.
    pub fn check_kind(&self, kind: NodeKind, class: DeviceClass) -> Result<(), Denial> {
        match kind {
            NodeKind::Char { major, .. } => {
                if self.class_of_major(major) == Some(class) {
                    Ok(())
                } else {
                    Err(Denial::WrongClass)
                }
            }
            NodeKind::Symlink => Err(Denial::Symlink),
            _ => Err(Denial::NotCharDevice),
        }
    }

    // [@ANCHOR: relay_broker_policy_check_node]
    /// The filesystem check: every directory on the way from the device root is a real
    /// directory (not a symlink), the final node is a character device of `class`, and its USB
    /// identity is not protected. `clean` comes from `check_path`.
    pub fn check_node(&self, os: &dyn Os, clean: &Path, class: DeviceClass) -> Result<(), Denial> {
        let Ok(rel) = clean.strip_prefix(&self.device_root) else {
            return Err(Denial::OutsideRoot);
        };
        let mut cursor = self.device_root.clone();
        let parts: Vec<_> = rel.components().collect();
        let last = parts.len().saturating_sub(1);
        for (i, c) in parts.iter().enumerate() {
            cursor.push(c);
            let kind = os.node_kind(&cursor);
            if i < last {
                match kind {
                    NodeKind::Dir => {}
                    NodeKind::Symlink => return Err(Denial::Symlink),
                    _ => return Err(Denial::NotCharDevice),
                }
            } else {
                self.check_kind(kind, class)?;
            }
        }
        if let Some((vid, pid, _)) = os.usb_identity(clean) {
            if self.deny_usb.contains(&(vid, pid)) {
                return Err(Denial::ProtectedUsb);
            }
        }
        Ok(())
    }

    // [@ANCHOR: relay_broker_policy_check_fd]
    /// The post-open check on the descriptor itself.
    pub fn check_fd(&self, kind: NodeKind, class: DeviceClass) -> Result<(), Denial> {
        self.check_kind(kind, class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: relay_broker_policy_check_path]
    // Tests [@ANCHOR: relay_broker_denial_reason]
    #[test]
    fn lexical_check_refuses_traversal_relative_and_deny_listed_paths() {
        let p = Policy::default();
        assert_eq!(
            p.check_path("/dev/ttyUSB0"),
            Ok(PathBuf::from("/dev/ttyUSB0"))
        );
        assert_eq!(
            p.check_path("/dev/snd/pcmC1D0c"),
            Ok(PathBuf::from("/dev/snd/pcmC1D0c"))
        );
        assert_eq!(p.check_path("ttyUSB0"), Err(Denial::NotAbsolute));
        assert_eq!(p.check_path("/dev/../etc/passwd"), Err(Denial::Traversal));
        assert_eq!(p.check_path("/dev/./ttyUSB0"), Err(Denial::Traversal));
        assert_eq!(p.check_path("/dev//ttyUSB0"), Err(Denial::Traversal));
        assert_eq!(p.check_path("/dev/ttyUSB0/"), Err(Denial::Traversal));
        assert_eq!(p.check_path("/devices/x"), Err(Denial::OutsideRoot));
        assert_eq!(p.check_path("/etc/passwd"), Err(Denial::OutsideRoot));
        assert_eq!(p.check_path("/dev"), Err(Denial::OutsideRoot));
        assert_eq!(p.check_path("/dev/mem"), Err(Denial::DenyListed));
        assert_eq!(p.check_path("/dev/tty"), Err(Denial::DenyListed));
        assert_eq!(p.check_path("/dev/tty\0"), Err(Denial::TooLong));
        assert_eq!(
            p.check_path(&format!("/dev/{}", "a".repeat(300))),
            Err(Denial::TooLong)
        );
        for d in [
            Denial::NotAbsolute,
            Denial::OutsideRoot,
            Denial::Traversal,
            Denial::TooLong,
            Denial::DenyListed,
            Denial::Symlink,
            Denial::NotCharDevice,
            Denial::WrongClass,
            Denial::ProtectedUsb,
        ] {
            assert!(!d.reason().is_empty());
        }
    }

    // Tests [@ANCHOR: relay_broker_policy_check_kind]
    // Tests [@ANCHOR: relay_broker_policy_check_fd]
    // Tests [@ANCHOR: relay_broker_policy_class_of_major]
    #[test]
    fn kinds_are_matched_to_the_requested_class_and_disks_refused() {
        let p = Policy::default();
        let tty = NodeKind::Char {
            major: 188,
            minor: 0,
        };
        let snd = NodeKind::Char {
            major: 116,
            minor: 5,
        };
        assert_eq!(p.check_kind(tty, DeviceClass::Serial), Ok(()));
        assert_eq!(
            p.check_kind(tty, DeviceClass::Audio),
            Err(Denial::WrongClass)
        );
        assert_eq!(p.check_fd(snd, DeviceClass::Audio), Ok(()));
        assert_eq!(
            p.check_kind(NodeKind::Block, DeviceClass::Serial),
            Err(Denial::NotCharDevice)
        );
        assert_eq!(
            p.check_kind(NodeKind::Symlink, DeviceClass::Serial),
            Err(Denial::Symlink)
        );
        assert_eq!(
            p.check_kind(NodeKind::Missing, DeviceClass::Serial),
            Err(Denial::NotCharDevice)
        );
        assert_eq!(
            p.check_kind(NodeKind::Char { major: 1, minor: 1 }, DeviceClass::Serial),
            Err(Denial::WrongClass),
            "/dev/mem's major is nobody's class"
        );
        assert_eq!(p.class_of_major(166), Some(DeviceClass::Serial));
        assert_eq!(p.class_of_major(116), Some(DeviceClass::Audio));
        assert_eq!(p.class_of_major(8), None);
    }

    // Tests [@ANCHOR: relay_broker_policy_check_node]
    #[test]
    fn node_walk_uses_the_real_dev_null_without_root() {
        let p = Policy {
            serial_majors: vec![1],
            ..Policy::default()
        };
        let os = crate::os::LinuxOs::default();
        let clean = p.check_path("/dev/null").unwrap();
        assert_eq!(p.check_node(&os, &clean, DeviceClass::Serial), Ok(()));
        assert_eq!(
            p.check_node(&os, &clean, DeviceClass::Audio),
            Err(Denial::WrongClass)
        );
        // /dev/stdin is a symlink on every Linux; /dev/fd is a symlink to /proc/self/fd.
        let link = p.check_path("/dev/stdin").unwrap();
        assert_eq!(
            p.check_node(&os, &link, DeviceClass::Serial),
            Err(Denial::Symlink)
        );
        let via_link_dir = p.check_path("/dev/fd/0").unwrap();
        assert_eq!(
            p.check_node(&os, &via_link_dir, DeviceClass::Serial),
            Err(Denial::Symlink)
        );
        assert_eq!(
            p.check_node(&os, Path::new("/etc/x"), DeviceClass::Serial),
            Err(Denial::OutsideRoot)
        );
    }
}
