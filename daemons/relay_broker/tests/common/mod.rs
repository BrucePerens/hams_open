// SPDX-License-Identifier: AGPL-3.0-or-later
//! The in-process fake OS: a table of nodes, USB identities, accounts and groups, and a
//! configurable peer. `open_device` opens the real `/dev/null` so a genuine descriptor crosses
//! the socket, while `fd_kind` reports the kind of the node the fake was asked to open, which is
//! what a real `fstat` would say for the real device. Everything here runs without root.
#![allow(dead_code, clippy::expect_used, clippy::unwrap_used)]

use std::cell::Cell;
use std::collections::HashMap;
use std::io;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use relay_broker::audit::AuditLog;
use relay_broker::broker::Broker;
use relay_broker::config::Config;
use relay_broker::os::{LinuxOs, NodeKind, Os, Peer};
use relay_broker::wire::{DeviceClass, DeviceDescriptor};

pub const HAM_UID: u32 = 1000;

pub struct FakeOs {
    pub peer: Peer,
    pub nodes: HashMap<PathBuf, NodeKind>,
    pub usb: HashMap<PathBuf, (u16, u16, String)>,
    pub devices: Vec<DeviceDescriptor>,
    pub users: HashMap<u32, String>,
    pub groups: HashMap<String, Vec<String>>,
    pub added: Arc<Mutex<Vec<(String, String)>>>,
    last_opened: Cell<Option<NodeKind>>,
}

impl FakeOs {
    /// A machine with a USB serial port, a VNWA codec, a sound card, a disk, and the usual
    /// symlinks, seen by the `ham` account which is in no group yet.
    pub fn typical() -> FakeOs {
        let mut nodes = HashMap::new();
        let chr = |maj, min| NodeKind::Char {
            major: maj,
            minor: min,
        };
        nodes.insert("/dev".into(), NodeKind::Dir);
        nodes.insert("/dev/snd".into(), NodeKind::Dir);
        nodes.insert("/dev/ttyUSB0".into(), chr(188, 0));
        nodes.insert("/dev/ttyUSB1".into(), chr(188, 1));
        nodes.insert("/dev/ttyACM0".into(), chr(166, 0));
        nodes.insert("/dev/snd/pcmC1D0c".into(), chr(116, 24));
        nodes.insert("/dev/snd/controlC1".into(), chr(116, 32));
        nodes.insert("/dev/sda".into(), NodeKind::Block);
        nodes.insert("/dev/mem".into(), chr(1, 1));
        nodes.insert("/dev/stdin".into(), NodeKind::Symlink);
        nodes.insert("/dev/serial".into(), NodeKind::Symlink);
        nodes.insert("/dev/serial/by-id".into(), NodeKind::Dir);
        nodes.insert("/dev/serial/by-id/usb-FTDI-if00-port0".into(), chr(188, 0));
        nodes.insert("/dev/ttyUSB7".into(), NodeKind::Symlink);
        nodes.insert("/dev/vnwa".into(), NodeKind::Symlink);
        let mut usb = HashMap::new();
        usb.insert(
            "/dev/ttyUSB0".into(),
            (0x0403, 0x6001, "FT232R".to_string()),
        );
        usb.insert("/dev/ttyUSB1".into(), (0x08bb, 0x29c0, "VNWA".to_string()));
        usb.insert(
            "/dev/snd/pcmC1D0c".into(),
            (0x08bb, 0x2901, "USB Audio CODEC".to_string()),
        );
        let dev = |class, path: &str, vid, pid| DeviceDescriptor {
            class,
            path: path.into(),
            vid,
            pid,
            product: String::new(),
            manufacturer: String::new(),
        };
        FakeOs {
            peer: Peer {
                uid: HAM_UID,
                gid: HAM_UID,
                pid: 4321,
            },
            nodes,
            usb,
            devices: vec![
                dev(DeviceClass::Serial, "/dev/ttyUSB0", 0x0403, 0x6001),
                dev(DeviceClass::Serial, "/dev/ttyUSB1", 0x08bb, 0x29c0),
                dev(DeviceClass::Audio, "/dev/snd/pcmC1D0c", 0x08bb, 0x2901),
            ],
            users: HashMap::from([(HAM_UID, "ham".to_string()), (0, "root".to_string())]),
            groups: HashMap::from([
                ("dialout".to_string(), vec![]),
                ("audio".to_string(), vec![]),
            ]),
            added: Arc::new(Mutex::new(Vec::new())),
            last_opened: Cell::new(None),
        }
    }
}

impl Os for FakeOs {
    fn peer_credentials(&self, _socket: BorrowedFd<'_>) -> io::Result<Peer> {
        Ok(self.peer)
    }
    fn node_kind(&self, path: &Path) -> NodeKind {
        self.nodes.get(path).copied().unwrap_or(NodeKind::Missing)
    }
    fn open_device(&self, path: &Path, read_only: bool) -> io::Result<OwnedFd> {
        let kind = self.node_kind(path);
        if !matches!(kind, NodeKind::Char { .. }) {
            return Err(io::Error::from(io::ErrorKind::NotFound));
        }
        self.last_opened.set(Some(kind));
        LinuxOs::default().open_device(Path::new("/dev/null"), read_only)
    }
    fn fd_kind(&self, _fd: BorrowedFd<'_>) -> NodeKind {
        self.last_opened.get().unwrap_or(NodeKind::Missing)
    }
    fn usb_identity(&self, path: &Path) -> Option<(u16, u16, String)> {
        self.usb.get(path).cloned()
    }
    fn list_devices(&self) -> Vec<DeviceDescriptor> {
        self.devices.clone()
    }
    fn username(&self, uid: u32) -> Option<String> {
        self.users.get(&uid).cloned()
    }
    fn groups_of(&self, user: &str) -> Vec<String> {
        self.groups
            .iter()
            .filter(|(_, members)| members.iter().any(|m| m == user))
            .map(|(g, _)| g.clone())
            .collect()
    }
    fn add_to_group(&self, user: &str, group: &str) -> io::Result<()> {
        self.added
            .lock()
            .unwrap()
            .push((user.to_string(), group.to_string()));
        Ok(())
    }
}

/// A config that lets `HAM_UID` in, with no audit file.
pub fn test_config() -> Config {
    let mut cfg = Config::default();
    cfg.allow.uids.push(HAM_UID);
    cfg.audit_log = None;
    cfg
}

pub fn broker_with(cfg: &Config, os: FakeOs, audit: Option<&Path>) -> Broker {
    Broker::new(cfg, AuditLog::open(audit).expect("audit"), Box::new(os))
}

pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("relay_broker_it_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tempdir");
    dir
}
