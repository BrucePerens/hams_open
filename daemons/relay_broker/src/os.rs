// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OS layer behind a trait, so the policy and the server are tested with an in-process fake
//! and so a macOS (XPC, Mach ports) or Windows (named pipes, `DuplicateHandle`) implementation
//! can follow without touching the policy. `LinuxOs` is the only real implementation. Its sysfs
//! and group-file roots are parameters, so device enumeration is tested against a temporary
//! directory shaped like `/sys` and never needs root.

use std::fs;
use std::io;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};

use nix::fcntl::{fcntl, open, FcntlArg, OFlag};
use nix::sys::socket::{getsockopt, sockopt};
use nix::sys::stat::{fstat, lstat, major, minor, Mode, SFlag};

use crate::wire::{DeviceClass, DeviceDescriptor};

/// The credentials of the process at the other end of a Unix socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Peer {
    /// Effective UID.
    pub uid: u32,
    /// Effective GID.
    pub gid: u32,
    /// Process id (informational; a pid can be reused).
    pub pid: i32,
}

/// What a filesystem node is, as far as the policy cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    /// A character device with its major and minor numbers.
    Char {
        /// Device major.
        major: u32,
        /// Device minor.
        minor: u32,
    },
    /// A block device (disk): always refused.
    Block,
    /// A directory.
    Dir,
    /// A symbolic link (checked with lstat, so never followed).
    Symlink,
    /// Anything else (regular file, fifo, socket).
    Other,
    /// Does not exist or cannot be examined.
    Missing,
}

/// Everything the broker asks of the operating system.
pub trait Os {
    /// SO_PEERCRED (or the platform equivalent) of a connected socket.
    fn peer_credentials(&self, socket: BorrowedFd<'_>) -> io::Result<Peer>;
    /// The kind of the node at `path` without following a final symlink.
    fn node_kind(&self, path: &Path) -> NodeKind;
    /// Opens `path` with `O_NOFOLLOW | O_CLOEXEC | O_NOCTTY` and returns the descriptor.
    fn open_device(&self, path: &Path, read_only: bool) -> io::Result<OwnedFd>;
    /// The kind of an already-open descriptor (the post-open TOCTOU check).
    fn fd_kind(&self, fd: BorrowedFd<'_>) -> NodeKind;
    /// USB vendor and product ids and the product string of the device behind a node, if USB.
    fn usb_identity(&self, path: &Path) -> Option<(u16, u16, String)>;
    /// Read-only enumeration of serial and audio nodes.
    fn list_devices(&self) -> Vec<DeviceDescriptor>;
    /// The account name of a UID.
    fn username(&self, uid: u32) -> Option<String>;
    /// The supplementary groups of an account.
    fn groups_of(&self, user: &str) -> Vec<String>;
    /// Adds an account to a group. Only reached when the config enables apply.
    fn add_to_group(&self, user: &str, group: &str) -> io::Result<()>;
}

/// The Linux implementation: sysfs for enumeration, `/etc/group` for membership.
#[derive(Debug, Clone)]
pub struct LinuxOs {
    /// Normally `/sys`.
    pub sysfs_root: PathBuf,
    /// Normally `/etc/group`.
    pub group_file: PathBuf,
}

impl Default for LinuxOs {
    fn default() -> LinuxOs {
        LinuxOs {
            sysfs_root: PathBuf::from("/sys"),
            group_file: PathBuf::from("/etc/group"),
        }
    }
}

// [@ANCHOR: relay_broker_kind_of_mode]
/// Maps `st_mode`/`st_rdev` to a `NodeKind`.
fn kind_of_mode(mode: u32, rdev: u64) -> NodeKind {
    match SFlag::from_bits_truncate(mode) & SFlag::S_IFMT {
        SFlag::S_IFCHR => NodeKind::Char {
            major: major(rdev) as u32,
            minor: minor(rdev) as u32,
        },
        SFlag::S_IFBLK => NodeKind::Block,
        SFlag::S_IFDIR => NodeKind::Dir,
        SFlag::S_IFLNK => NodeKind::Symlink,
        _ => NodeKind::Other,
    }
}

// [@ANCHOR: relay_broker_read_hex_u16]
/// Reads a sysfs attribute holding a hexadecimal id (`idVendor`, `idProduct`).
fn read_hex_u16(path: &Path) -> Option<u16> {
    let text = fs::read_to_string(path).ok()?;
    u16::from_str_radix(text.trim(), 16).ok()
}

// [@ANCHOR: relay_broker_usb_identity_from_sysfs]
/// Walks up from a sysfs device directory (at most eight levels) to the USB device that holds
/// `idVendor`/`idProduct`, and returns them with the product string.
fn usb_identity_from_sysfs(device_dir: &Path) -> Option<(u16, u16, String)> {
    let mut dir = fs::canonicalize(device_dir).ok()?;
    for _ in 0..8 {
        if let (Some(vid), Some(pid)) = (
            read_hex_u16(&dir.join("idVendor")),
            read_hex_u16(&dir.join("idProduct")),
        ) {
            let product = fs::read_to_string(dir.join("product"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            return Some((vid, pid, product));
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

impl LinuxOs {
    // [@ANCHOR: relay_broker_linux_sysfs_dir_for]
    /// The sysfs class directory of a device node: `/dev/ttyUSB0` is `class/tty/ttyUSB0`,
    /// `/dev/snd/pcmC1D0c` is `class/sound/pcmC1D0c`. `None` for anything else.
    fn sysfs_dir_for(&self, path: &Path) -> Option<PathBuf> {
        let rel = path.strip_prefix("/dev").ok()?;
        let mut parts = rel.components().filter_map(|c| c.as_os_str().to_str());
        match (parts.next(), parts.next(), parts.next()) {
            (Some(name), None, _) if name.starts_with("tty") => {
                Some(self.sysfs_root.join("class/tty").join(name))
            }
            (Some("snd"), Some(name), None) => Some(self.sysfs_root.join("class/sound").join(name)),
            _ => None,
        }
    }

    // [@ANCHOR: relay_broker_linux_scan_class]
    /// Lists one sysfs class directory. `keep` decides by entry name; `dev_dir` is where the
    /// node lives. Only entries with a `device` link (real hardware, not virtual consoles) count.
    fn scan_class(
        &self,
        class: DeviceClass,
        sys_class: &str,
        dev_dir: &str,
        keep: fn(&str) -> bool,
    ) -> Vec<DeviceDescriptor> {
        let mut out = Vec::new();
        let Ok(entries) = fs::read_dir(self.sysfs_root.join(sys_class)) else {
            return out;
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| keep(n))
            .collect();
        names.sort();
        for name in names.into_iter().take(crate::wire::MAX_DEVICES) {
            let sys_dir = self.sysfs_root.join(sys_class).join(&name);
            if !sys_dir.join("device").exists() {
                continue;
            }
            let (vid, pid, product) =
                usb_identity_from_sysfs(&sys_dir.join("device")).unwrap_or((0, 0, String::new()));
            out.push(DeviceDescriptor {
                class,
                path: format!("{dev_dir}/{name}"),
                vid,
                pid,
                product,
            });
        }
        out
    }
}

impl Os for LinuxOs {
    // [@ANCHOR: relay_broker_linux_peer_credentials]
    fn peer_credentials(&self, socket: BorrowedFd<'_>) -> io::Result<Peer> {
        let cred = getsockopt(&socket, sockopt::PeerCredentials).map_err(io::Error::from)?;
        Ok(Peer {
            uid: cred.uid(),
            gid: cred.gid(),
            pid: cred.pid(),
        })
    }

    // [@ANCHOR: relay_broker_linux_node_kind]
    fn node_kind(&self, path: &Path) -> NodeKind {
        match lstat(path) {
            Ok(st) => kind_of_mode(st.st_mode, st.st_rdev),
            Err(_) => NodeKind::Missing,
        }
    }

    // [@ANCHOR: relay_broker_linux_open_device]
    fn open_device(&self, path: &Path, read_only: bool) -> io::Result<OwnedFd> {
        let access = if read_only {
            OFlag::O_RDONLY
        } else {
            OFlag::O_RDWR
        };
        // O_NONBLOCK so an open of a tty without carrier returns at once; cleared afterwards so
        // the worker receives an ordinary blocking descriptor.
        let flags =
            access | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NOCTTY | OFlag::O_NONBLOCK;
        let fd = open(path, flags, Mode::empty()).map_err(io::Error::from)?;
        fcntl(&fd, FcntlArg::F_SETFL(OFlag::empty())).map_err(io::Error::from)?;
        Ok(fd)
    }

    // [@ANCHOR: relay_broker_linux_fd_kind]
    fn fd_kind(&self, fd: BorrowedFd<'_>) -> NodeKind {
        match fstat(fd) {
            Ok(st) => kind_of_mode(st.st_mode, st.st_rdev),
            Err(_) => NodeKind::Missing,
        }
    }

    // [@ANCHOR: relay_broker_linux_usb_identity]
    fn usb_identity(&self, path: &Path) -> Option<(u16, u16, String)> {
        let dir = self.sysfs_dir_for(path)?;
        usb_identity_from_sysfs(&dir.join("device"))
    }

    // [@ANCHOR: relay_broker_linux_list_devices]
    fn list_devices(&self) -> Vec<DeviceDescriptor> {
        let mut out = self.scan_class(DeviceClass::Serial, "class/tty", "/dev", |n| {
            n.starts_with("ttyUSB") || n.starts_with("ttyACM") || n.starts_with("ttyS")
        });
        out.extend(
            self.scan_class(DeviceClass::Audio, "class/sound", "/dev/snd", |n| {
                n.starts_with("pcmC") || n.starts_with("controlC")
            }),
        );
        out
    }

    // [@ANCHOR: relay_broker_linux_username]
    fn username(&self, uid: u32) -> Option<String> {
        nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|u| u.name)
    }

    // [@ANCHOR: relay_broker_linux_groups_of]
    fn groups_of(&self, user: &str) -> Vec<String> {
        let Ok(text) = fs::read_to_string(&self.group_file) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| {
                let mut f = line.split(':');
                let name = f.next()?;
                let members = f.nth(2)?;
                members
                    .split(',')
                    .any(|m| m == user)
                    .then(|| name.to_string())
            })
            .collect()
    }

    // [@ANCHOR: relay_broker_linux_add_to_group]
    fn add_to_group(&self, user: &str, group: &str) -> io::Result<()> {
        // A fixed program with a fixed argv, no shell. Both names were validated upstream: the
        // group is one of the closed `Group` set and the user came from the UID lookup.
        let status = std::process::Command::new("/usr/sbin/gpasswd")
            .arg("-a")
            .arg(user)
            .arg(group)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("gpasswd exited with {status}")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsFd;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "relay_broker_os_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A sysfs tree with one USB serial port, one virtual console, and one USB sound card.
    fn fake_sysfs(root: &Path) {
        let usb = root.join("devices/usb1/1-1");
        fs::create_dir_all(usb.join("1-1:1.0/ttyUSB0")).unwrap();
        fs::write(usb.join("idVendor"), "08bb\n").unwrap();
        fs::write(usb.join("idProduct"), "29c0\n").unwrap();
        fs::write(usb.join("product"), "VNWA\n").unwrap();
        fs::create_dir_all(root.join("class/tty/ttyUSB0")).unwrap();
        std::os::unix::fs::symlink(usb.join("1-1:1.0"), root.join("class/tty/ttyUSB0/device"))
            .unwrap();
        fs::create_dir_all(root.join("class/tty/tty0")).unwrap();
        fs::create_dir_all(root.join("class/sound/pcmC1D0c")).unwrap();
        std::os::unix::fs::symlink(&usb, root.join("class/sound/pcmC1D0c/device")).unwrap();
        fs::create_dir_all(root.join("class/sound/timer")).unwrap();
    }

    // Tests [@ANCHOR: relay_broker_linux_list_devices]
    // Tests [@ANCHOR: relay_broker_linux_scan_class]
    // Tests [@ANCHOR: relay_broker_linux_usb_identity]
    // Tests [@ANCHOR: relay_broker_linux_sysfs_dir_for]
    // Tests [@ANCHOR: relay_broker_usb_identity_from_sysfs]
    // Tests [@ANCHOR: relay_broker_read_hex_u16]
    #[test]
    fn sysfs_enumeration_reads_usb_ids_through_the_device_link() {
        let root = temp_dir("sysfs");
        fake_sysfs(&root);
        let os = LinuxOs {
            sysfs_root: root.clone(),
            group_file: root.join("group"),
        };
        let devs = os.list_devices();
        assert_eq!(devs.len(), 2, "{devs:?}");
        assert_eq!(devs[0].path, "/dev/ttyUSB0");
        assert_eq!((devs[0].vid, devs[0].pid), (0x08bb, 0x29c0));
        assert_eq!(devs[0].product, "VNWA");
        assert_eq!(devs[1].path, "/dev/snd/pcmC1D0c");
        assert_eq!(devs[1].class, DeviceClass::Audio);
        assert_eq!(
            os.usb_identity(Path::new("/dev/ttyUSB0")),
            Some((0x08bb, 0x29c0, "VNWA".to_string()))
        );
        assert_eq!(
            os.usb_identity(Path::new("/dev/snd/pcmC1D0c")).map(|t| t.0),
            Some(0x08bb)
        );
        assert_eq!(os.usb_identity(Path::new("/dev/tty0")), None);
        assert_eq!(os.usb_identity(Path::new("/dev/sda")), None);
        assert_eq!(os.usb_identity(Path::new("/etc/passwd")), None);
        assert_eq!(read_hex_u16(&root.join("missing")), None);
        fs::remove_dir_all(&root).unwrap();
    }

    // Tests [@ANCHOR: relay_broker_linux_groups_of]
    // Tests [@ANCHOR: relay_broker_linux_username]
    #[test]
    fn group_membership_is_read_from_the_group_file() {
        let root = temp_dir("group");
        fs::write(
            root.join("group"),
            "root:x:0:\ndialout:x:20:ham,other\naudio:x:29:other\nsham:x:30:hamster\n",
        )
        .unwrap();
        let os = LinuxOs {
            sysfs_root: root.clone(),
            group_file: root.join("group"),
        };
        assert_eq!(os.groups_of("ham"), vec!["dialout".to_string()]);
        assert_eq!(os.groups_of("nobody_here"), Vec::<String>::new());
        let me = nix::unistd::getuid().as_raw();
        assert!(os.username(me).is_some());
        fs::remove_dir_all(&root).unwrap();
    }

    // Tests [@ANCHOR: relay_broker_linux_node_kind]
    // Tests [@ANCHOR: relay_broker_linux_fd_kind]
    // Tests [@ANCHOR: relay_broker_linux_open_device]
    // Tests [@ANCHOR: relay_broker_kind_of_mode]
    // Tests [@ANCHOR: relay_broker_linux_peer_credentials]
    #[test]
    fn real_nodes_are_classified_and_dev_null_opens_without_root() {
        let os = LinuxOs::default();
        assert_eq!(
            os.node_kind(Path::new("/dev/null")),
            NodeKind::Char { major: 1, minor: 3 }
        );
        assert_eq!(os.node_kind(Path::new("/dev")), NodeKind::Dir);
        assert_eq!(
            os.node_kind(Path::new("/dev/does-not-exist")),
            NodeKind::Missing
        );
        assert_eq!(os.node_kind(Path::new("/etc/hostname")), NodeKind::Other);
        assert_eq!(kind_of_mode(0o060000, 0), NodeKind::Block);
        assert_eq!(kind_of_mode(0o120000, 0), NodeKind::Symlink);
        let fd = os.open_device(Path::new("/dev/null"), false).unwrap();
        assert_eq!(
            os.fd_kind(fd.as_fd()),
            NodeKind::Char { major: 1, minor: 3 }
        );
        assert!(
            os.open_device(Path::new("/dev/stdin"), true).is_err(),
            "symlink refused by O_NOFOLLOW"
        );
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = os.peer_credentials(a.as_fd()).unwrap();
        assert_eq!(peer.uid, nix::unistd::getuid().as_raw());
        assert_eq!(peer.pid, std::process::id() as i32);
    }

    // Tests [@ANCHOR: relay_broker_linux_add_to_group]
    #[test]
    fn add_to_group_as_an_ordinary_user_fails_without_running_a_shell() {
        // Unprivileged: gpasswd refuses (or is absent). Either way the call returns an error
        // and nothing changes. The success path needs root and is covered only by review.
        let os = LinuxOs::default();
        assert!(os
            .add_to_group("nobody", "nogroup-relay-broker-test")
            .is_err());
    }
}
