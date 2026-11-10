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
    /// Ports `bind_udp` was asked for.
    pub binds: Arc<Mutex<Vec<u16>>>,
    /// The configuration texts and interface of a started hub.
    pub hub: Arc<Mutex<Option<(String, String, String)>>>,
    /// How many times the package manager was asked to upgrade the relay.
    pub updates: Arc<std::sync::atomic::AtomicUsize>,
    /// The fake clock (Unix seconds) and a switch that makes setting it fail.
    pub clock: Arc<std::sync::atomic::AtomicU64>,
    pub clock_fails: Arc<std::sync::atomic::AtomicBool>,
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
            binds: Arc::new(Mutex::new(Vec::new())),
            hub: Arc::new(Mutex::new(None)),
            updates: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            clock: Arc::new(std::sync::atomic::AtomicU64::new(1_793_500_000)),
            clock_fails: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
    fn bind_udp(&self, port: u16) -> io::Result<OwnedFd> {
        self.binds.lock().unwrap().push(port);
        LinuxOs::default().open_device(Path::new("/dev/null"), true)
    }
    fn hub_start(
        &self,
        hostapd_conf: &str,
        dnsmasq_conf: &str,
        interface: &str,
        _address: std::net::Ipv4Addr,
    ) -> io::Result<()> {
        *self.hub.lock().unwrap() = Some((
            hostapd_conf.to_string(),
            dnsmasq_conf.to_string(),
            interface.to_string(),
        ));
        Ok(())
    }
    fn hub_stop(&self) -> io::Result<()> {
        *self.hub.lock().unwrap() = None;
        Ok(())
    }
    fn update_package(&self) -> io::Result<()> {
        self.updates
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn system_time_unix_secs(&self) -> u64 {
        self.clock.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn set_system_time(&self, unix_secs: u64) -> io::Result<()> {
        if self.clock_fails.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(io::Error::other("no CAP_SYS_TIME"));
        }
        self.clock
            .store(unix_secs, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
    fn hub_status(&self) -> (bool, bool) {
        let up = self.hub.lock().unwrap().is_some();
        (up, up)
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

#[path = "../../src/testdir.rs"]
mod testdir;

pub fn temp_dir(tag: &str) -> testdir::TestDir {
    testdir::TestDir::new(&format!("it_{tag}"))
}
