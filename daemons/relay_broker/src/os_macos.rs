// SPDX-License-Identifier: AGPL-3.0-or-later
//! The macOS implementation of the `Os` trait. The broker's Unix socket, `LOCAL_PEERCRED` authentication, descriptor passing and policy are the
//! same code as on Linux; only the questions the kernel answers differently are here: the serial nodes are `/dev/cu.*` and `/dev/tty.*` (major 9),
//! their USB identity comes from the I/O Registry (`ioreg`, a fixed program with a fixed argument list, no shell), accounts come from the
//! directory service (`getgrouplist`, `dseditgroup`). The stand-alone Wi-Fi hub, the DHCP-client socket, the package upgrade, power-off and the
//! clock step have no macOS counterpart here and answer `Unsupported`; the policy refuses them by configuration anyway (`hub`, `update_package`
//! and `set_time` default to false). The I/O Registry text is parsed by a pure function that the tests also run on a fixed sample.

use std::io;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::path::Path;
use std::process::Command;

use nix::fcntl::{fcntl, open, FcntlArg, FdFlag, OFlag};
use nix::sys::stat::{fstat, lstat, Mode};

use crate::os::{kind_of_stat, socket_peer, NodeKind, Os, Peer, UsbIdentity};
use crate::wire::{DeviceClass, DeviceDescriptor};

/// The macOS implementation. `ioreg` and `dseditgroup` are parameters so a test can point them at another program.
// [@ANCHOR: os_macos_os_struct]
#[derive(Debug, Clone)]
pub struct MacOs {
    /// Normally `/usr/sbin/ioreg`.
    pub ioreg: std::path::PathBuf,
    /// Normally `/usr/sbin/dseditgroup`.
    pub dseditgroup: std::path::PathBuf,
}

impl Default for MacOs {
    fn default() -> MacOs {
        MacOs {
            ioreg: "/usr/sbin/ioreg".into(),
            dseditgroup: "/usr/sbin/dseditgroup".into(),
        }
    }
}

/// One serial node the I/O Registry lists, with the USB device it hangs under, if any.
// [@ANCHOR: os_macos_serial_node]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialNode {
    /// `/dev/cu.*`.
    pub callout: String,
    /// `/dev/tty.*`.
    pub dialin: String,
    /// Vendor id, product id, product string, vendor string of the nearest enclosing USB device.
    pub usb: Option<(u16, u16, String, String)>,
}

/// The value of an `ioreg` property line: `"key" = 1507` or `"key" = "text"`.
// [@ANCHOR: relay_broker_macos_property]
fn property(line: &str) -> Option<(&str, &str)> {
    let rest = line.trim_start_matches([' ', '|']).strip_prefix('"')?;
    let (key, value) = rest.split_once("\" = ")?;
    Some((key, value.trim()))
}

fn unquote(value: &str) -> String {
    value.trim_matches('"').to_string()
}

// [@ANCHOR: os_macos_parse_ioreg]
/// Reads `ioreg -r -c IOUSBHostDevice -l -w0` output (every USB device with its whole subtree and properties) and returns each serial node found
/// under a USB device, with that device's ids. Tree depth is the column of the `+-o` marker; the properties of a node follow its marker line and
/// precede its children, and only nodes of class `IOUSBHostDevice` supply ids. A callout path seen twice (a device printed under its hub and again
/// as a root of its own) is returned once, with the innermost device.
pub fn parse_ioreg(text: &str) -> Vec<SerialNode> {
    struct Frame {
        column: usize,
        is_device: bool,
        vendor: Option<u16>,
        product: Option<u16>,
        product_name: String,
        vendor_name: String,
        serial: bool,
        callout: String,
        dialin: String,
    }
    let mut stack: Vec<Frame> = Vec::new();
    let mut out: Vec<SerialNode> = Vec::new();
    for line in text.lines() {
        if let Some(column) = line.find("+-o ") {
            if line[..column].chars().all(|c| c == ' ' || c == '|') {
                stack.retain(|f| f.column < column);
                stack.push(Frame {
                    column,
                    is_device: line.contains("<class IOUSBHostDevice,"),
                    vendor: None,
                    product: None,
                    product_name: String::new(),
                    vendor_name: String::new(),
                    serial: line.contains("<class IOSerialBSDClient,"),
                    callout: String::new(),
                    dialin: String::new(),
                });
                continue;
            }
        }
        let Some((key, value)) = property(line) else {
            continue;
        };
        let Some(top) = stack.last_mut() else {
            continue;
        };
        if top.is_device {
            match key {
                "idVendor" => top.vendor = value.parse().ok(),
                "idProduct" => top.product = value.parse().ok(),
                "USB Product Name" => top.product_name = unquote(value),
                "USB Vendor Name" => top.vendor_name = unquote(value),
                _ => {}
            }
        } else if top.serial {
            match key {
                "IOCalloutDevice" => top.callout = unquote(value),
                "IODialinDevice" => top.dialin = unquote(value),
                _ => {}
            }
            if !top.callout.is_empty() && !top.dialin.is_empty() {
                let usb = stack.iter().rev().find_map(|f| {
                    Some((
                        f.vendor.filter(|_| f.is_device)?,
                        f.product?,
                        f.product_name.clone(),
                        f.vendor_name.clone(),
                    ))
                });
                if let Some(top) = stack.last() {
                    let node = SerialNode {
                        callout: top.callout.clone(),
                        dialin: top.dialin.clone(),
                        usb,
                    };
                    match out.iter_mut().find(|n| n.callout == node.callout) {
                        Some(existing) => *existing = node,
                        None => out.push(node),
                    }
                }
                if let Some(top) = stack.last_mut() {
                    top.serial = false;
                }
            }
        }
    }
    out
}

impl MacOs {
    /// Runs a fixed program with a fixed argument list, an empty environment and no shell; its standard output when it exits 0.
    // [@ANCHOR: relay_broker_macos_run]
    fn run(program: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
        let output = Command::new(program)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .output()?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(io::Error::other(format!(
                "{} exited with {}",
                program.display(),
                output.status
            )))
        }
    }

    /// The serial nodes under USB devices, plus the ones with no USB parent (`ioreg -r -c IOSerialBSDClient`, ids 0).
    // [@ANCHOR: relay_broker_macos_serial_nodes]
    fn serial_nodes(&self) -> io::Result<Vec<SerialNode>> {
        let usb = Self::run(&self.ioreg, &["-r", "-c", "IOUSBHostDevice", "-l", "-w0"])?;
        let mut nodes = parse_ioreg(&String::from_utf8_lossy(&usb));
        let all = Self::run(&self.ioreg, &["-r", "-c", "IOSerialBSDClient", "-l", "-w0"])?;
        for node in parse_ioreg(&String::from_utf8_lossy(&all)) {
            if !nodes.iter().any(|n| n.callout == node.callout) {
                nodes.push(node);
            }
        }
        Ok(nodes)
    }
}

// [@ANCHOR: os_macos_parse_dscl_search]
/// The group names of `dscl . -search /Groups GroupMembership <user>` output: lines `name<TAB>GroupMembership = (user)`.
fn parse_dscl_search(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| !line.starts_with([' ', '\t']))
        .filter_map(|line| {
            let (name, rest) = line.split_once(char::is_whitespace)?;
            rest.trim_start()
                .starts_with("GroupMembership")
                .then(|| name.to_string())
        })
        .collect()
}

// [@ANCHOR: relay_broker_macos_new_udp_socket]
fn new_udp_socket(family: nix::sys::socket::AddressFamily) -> nix::Result<OwnedFd> {
    use nix::sys::socket::{socket, SockFlag, SockType};
    let fd = socket(family, SockType::Datagram, SockFlag::empty(), None)?;
    fcntl(&fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
    Ok(fd)
}

fn unsupported(what: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("{what} is not available on macOS"),
    )
}

impl Os for MacOs {
    fn peer_credentials(&self, socket: BorrowedFd<'_>) -> io::Result<Peer> {
        socket_peer(socket)
    }

    // [@ANCHOR: relay_broker_macos_node_kind]
    fn node_kind(&self, path: &Path) -> NodeKind {
        match lstat(path) {
            Ok(st) => kind_of_stat(&st),
            Err(_) => NodeKind::Missing,
        }
    }

    // [@ANCHOR: relay_broker_macos_open_device]
    fn open_device(&self, path: &Path, read_only: bool) -> io::Result<OwnedFd> {
        let access = if read_only {
            OFlag::O_RDONLY
        } else {
            OFlag::O_RDWR
        };
        // O_NONBLOCK so a call-out open does not wait for carrier; cleared afterwards so the worker receives an ordinary blocking descriptor.
        let flags =
            access | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC | OFlag::O_NOCTTY | OFlag::O_NONBLOCK;
        let fd = open(path, flags, Mode::empty()).map_err(io::Error::from)?;
        fcntl(&fd, FcntlArg::F_SETFL(OFlag::empty())).map_err(io::Error::from)?;
        Ok(fd)
    }

    // [@ANCHOR: relay_broker_macos_fd_kind]
    fn fd_kind(&self, fd: BorrowedFd<'_>) -> NodeKind {
        match fstat(fd) {
            Ok(st) => kind_of_stat(&st),
            Err(_) => NodeKind::Missing,
        }
    }

    // [@ANCHOR: relay_broker_macos_usb_identity]
    fn usb_identity(&self, path: &Path) -> UsbIdentity {
        let Some(name) = path.to_str() else {
            return UsbIdentity::NotUsb;
        };
        if !name.starts_with("/dev/cu.") && !name.starts_with("/dev/tty.") {
            return UsbIdentity::NotUsb;
        }
        let Ok(nodes) = self.serial_nodes() else {
            return UsbIdentity::Unreadable;
        };
        match nodes
            .into_iter()
            .find(|n| n.callout == name || n.dialin == name)
        {
            Some(SerialNode {
                usb: Some((vid, pid, product, _)),
                ..
            }) => UsbIdentity::Usb(vid, pid, product),
            _ => UsbIdentity::NotUsb,
        }
    }

    // [@ANCHOR: relay_broker_macos_list_devices]
    fn list_devices(&self) -> Vec<DeviceDescriptor> {
        let mut out = Vec::new();
        for node in self.serial_nodes().unwrap_or_default() {
            let (vid, pid, product, manufacturer) = node.usb.unwrap_or_default();
            for path in [node.callout, node.dialin] {
                out.push(DeviceDescriptor {
                    class: DeviceClass::Serial,
                    path,
                    vid,
                    pid,
                    product: product.clone(),
                    manufacturer: manufacturer.clone(),
                });
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out.truncate(crate::wire::MAX_DEVICES);
        out
    }

    fn username(&self, uid: u32) -> Option<String> {
        nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|u| u.name)
    }

    /// The groups that list the account as a member, from the directory service (`dscl . -search /Groups GroupMembership <user>`): the same
    /// explicit membership Linux's `/etc/group` records, not the groups `id` computes (primary, `everyone`, nested).
    fn groups_of(&self, user: &str) -> Vec<String> {
        let Ok(out) = Self::run(
            Path::new("/usr/bin/dscl"),
            &[".", "-search", "/Groups", "GroupMembership", user],
        ) else {
            return Vec::new();
        };
        parse_dscl_search(&String::from_utf8_lossy(&out))
    }

    fn add_to_group(&self, user: &str, group: &str) -> io::Result<()> {
        Self::run(&self.dseditgroup, &Self::add_to_group_args(user, group)).map(|_| ())
    }

    // [@ANCHOR: relay_broker_macos_bind_udp]
    fn bind_udp(&self, port: u16) -> io::Result<OwnedFd> {
        use nix::sys::socket::{bind, setsockopt, sockopt, AddressFamily, SockaddrIn, SockaddrIn6};
        use std::os::fd::AsRawFd;
        let fd = new_udp_socket(AddressFamily::Inet6).map_err(io::Error::from)?;
        setsockopt(&fd, sockopt::Ipv6V6Only, &false).map_err(io::Error::from)?;
        let addr = SockaddrIn6::from(std::net::SocketAddrV6::new(
            std::net::Ipv6Addr::UNSPECIFIED,
            port,
            0,
            0,
        ));
        match bind(fd.as_raw_fd(), &addr) {
            Ok(()) => Ok(fd),
            Err(nix::errno::Errno::EADDRINUSE) => Err(io::Error::from(io::ErrorKind::AddrInUse)),
            Err(_) => {
                // No IPv6: plain IPv4.
                let fd = new_udp_socket(AddressFamily::Inet).map_err(io::Error::from)?;
                let addr = SockaddrIn::from(std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::UNSPECIFIED,
                    port,
                ));
                bind(fd.as_raw_fd(), &addr).map_err(io::Error::from)?;
                Ok(fd)
            }
        }
    }

    fn bind_dhcp_client(&self, _interface: &str) -> io::Result<OwnedFd> {
        Err(unsupported("a DHCP-client socket tied to one interface"))
    }

    fn hub_start(&self, _: &str, _: &str, _: &str, _: std::net::Ipv4Addr) -> io::Result<()> {
        Err(unsupported("the stand-alone hub"))
    }

    fn hub_stop(&self) -> io::Result<()> {
        Err(unsupported("the stand-alone hub"))
    }

    fn hub_bridged_ap_start(&self, _: &str, _: &str) -> io::Result<()> {
        Err(unsupported("the stand-alone hub"))
    }

    fn hub_takeover_start(&self, _: &str) -> io::Result<()> {
        Err(unsupported("the stand-alone hub"))
    }

    fn hub_takeover_stop(&self) -> io::Result<()> {
        Err(unsupported("the stand-alone hub"))
    }

    fn hub_status(&self) -> (bool, bool) {
        (false, false)
    }

    fn update_package(&self) -> io::Result<()> {
        Err(unsupported("the broker's package upgrade"))
    }

    fn poweroff(&self) -> io::Result<()> {
        Err(unsupported("the broker's power-off request"))
    }

    fn system_time_unix_secs(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    fn set_system_time(&self, _unix_secs: u64) -> io::Result<()> {
        Err(unsupported(
            "stepping the clock (the system's own time service sets it)",
        ))
    }
}

impl MacOs {
    /// The fixed `dseditgroup` argument list that adds a user account to a group.
    fn add_to_group_args<'a>(user: &'a str, group: &'a str) -> [&'a str; 7] {
        ["-o", "edit", "-a", user, "-t", "user", group]
    }
}

#[cfg(test)]
mod tests {
    // Tests [@ANCHOR: os_macos_os_struct]
    use super::*;
    use std::os::fd::AsFd;

    /// The text of a command that must exist on a Mac, trimmed.
    fn tool(program: &str, args: &[&str]) -> String {
        let out = Command::new(program).args(args).output().expect(program);
        assert!(out.status.success(), "{program} {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    const SAMPLE: &str = r#"+-o USB Serial Hub@00100000  <class IOUSBHostDevice, id 0x1, registered, matched, active, busy 0 (1 ms), retain 30>
  |   {
  |     "USB Product Name" = "Hub"
  |     "idVendor" = 1507
  |     "idProduct" = 1
  |   }
  |
  +-o FT232R USB UART@00110000  <class IOUSBHostDevice, id 0x2, registered, matched, active, busy 0 (1 ms), retain 20>
  | |   {
  | |     "USB Product Name" = "FT232R USB UART"
  | |     "USB Vendor Name" = "FTDI"
  | |     "idVendor" = 1027
  | |     "idProduct" = 24577
  | |   }
  | |
  | +-o IOUSBHostInterface@0  <class IOUSBHostInterface, id 0x3, registered>
  |   | {
  |   |   "idVendor" = 9999
  |   | }
  |   |
  |   +-o IOSerialBSDClient  <class IOSerialBSDClient, id 0x4, registered, matched, active>
  |         {
  |           "IOCalloutDevice" = "/dev/cu.usbserial-A1"
  |           "IODialinDevice" = "/dev/tty.usbserial-A1"
  |         }
  |
  +-o AppleUSBXHCI@2  <class AppleUSBXHCI, id 0x5, registered>
"#;

    // Tests [@ANCHOR: os_macos_parse_ioreg]
    // Tests [@ANCHOR: os_macos_serial_node]
    #[test]
    fn a_serial_node_takes_the_ids_of_its_nearest_usb_device_not_of_the_interface_or_the_hub() {
        assert_eq!(
            parse_ioreg(SAMPLE),
            vec![SerialNode {
                callout: "/dev/cu.usbserial-A1".into(),
                dialin: "/dev/tty.usbserial-A1".into(),
                usb: Some((1027, 24577, "FT232R USB UART".into(), "FTDI".into())),
            }]
        );
    }

    // Tests [@ANCHOR: os_macos_parse_ioreg]
    // Tests [@ANCHOR: relay_broker_macos_property]
    #[test]
    fn garbage_and_empty_text_parse_to_no_nodes() {
        assert!(parse_ioreg("").is_empty());
        assert!(parse_ioreg("\"idVendor\" = 5\n+-o x\n|||\n").is_empty());
    }

    // Tests [@ANCHOR: relay_broker_kind_of_stat]
    // Tests [@ANCHOR: relay_broker_macos_node_kind]
    // Tests [@ANCHOR: relay_broker_macos_fd_kind]
    #[test]
    fn node_numbers_equal_what_stat_prints_for_dev_null() {
        // Oracle: /usr/bin/stat prints the major (%Hr) and minor (%Lr) of the node.
        let major: u32 = tool("/usr/bin/stat", &["-f", "%Hr", "/dev/null"])
            .parse()
            .unwrap();
        let minor: u32 = tool("/usr/bin/stat", &["-f", "%Lr", "/dev/null"])
            .parse()
            .unwrap();
        let os = MacOs::default();
        assert_eq!(
            os.node_kind(Path::new("/dev/null")),
            NodeKind::Char { major, minor }
        );
        let fd = os.open_device(Path::new("/dev/null"), true).unwrap();
        assert_eq!(os.fd_kind(fd.as_fd()), NodeKind::Char { major, minor });
        assert_eq!(os.node_kind(Path::new("/dev/stdin")), NodeKind::Symlink);
        assert_eq!(os.node_kind(Path::new("/dev")), NodeKind::Dir);
        assert_eq!(
            os.node_kind(Path::new("/dev/nonexistent0")),
            NodeKind::Missing
        );
    }

    // Tests [@ANCHOR: relay_broker_macos_open_device]
    #[test]
    fn an_opened_descriptor_is_blocking_and_closes_on_exec() {
        let fd = MacOs::default()
            .open_device(Path::new("/dev/null"), false)
            .unwrap();
        let fdflags = fcntl(&fd, FcntlArg::F_GETFD).unwrap();
        assert!(FdFlag::from_bits_truncate(fdflags).contains(FdFlag::FD_CLOEXEC));
        let status = fcntl(&fd, FcntlArg::F_GETFL).unwrap();
        assert!(!OFlag::from_bits_truncate(status).contains(OFlag::O_NONBLOCK));
        assert!(
            MacOs::default()
                .open_device(Path::new("/dev/stdin"), true)
                .is_err(),
            "a symlink is never followed"
        );
    }

    // Tests [@ANCHOR: relay_broker_socket_peer]
    #[test]
    fn peer_credentials_name_this_process_as_the_oracle_id_does() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = MacOs::default().peer_credentials(a.as_fd()).unwrap();
        assert_eq!(peer.uid.to_string(), tool("/usr/bin/id", &["-u"]));
        assert_eq!(peer.gid.to_string(), tool("/usr/bin/id", &["-g"]));
        assert_eq!(i64::from(peer.pid), i64::from(std::process::id()));
    }

    // Tests [@ANCHOR: os_macos_parse_dscl_search]
    #[test]
    fn dscl_search_lines_give_group_names() {
        let text = "admin\t\t\tGroupMembership = (\n    root,\n    ai\n)\n_appserveradm  GroupMembership = (ai)\nnoise line\n";
        assert_eq!(parse_dscl_search(text), vec!["admin", "_appserveradm"]);
    }

    #[test]
    fn account_name_and_groups_agree_with_id_and_dseditgroup() {
        let os = MacOs::default();
        let uid: u32 = tool("/usr/bin/id", &["-u"]).parse().unwrap();
        let me = tool("/usr/bin/id", &["-un"]);
        assert_eq!(os.username(uid), Some(me.clone()));
        // Oracles: every group we report is one `id` reports for the account, and dseditgroup confirms the membership; root is in admin.
        let id_groups = tool("/usr/bin/id", &["-Gn", &me]);
        for group in os.groups_of(&me) {
            assert!(id_groups.split_whitespace().any(|g| g == group), "{group}");
            let check = Command::new("/usr/sbin/dseditgroup")
                .args(["-o", "checkmember", "-m", &me, &group])
                .output()
                .unwrap();
            assert!(
                check.status.success(),
                "dseditgroup says {me} is not in {group}"
            );
        }
        assert!(os.groups_of("root").contains(&"admin".to_string()));
        assert!(os.groups_of("no-such-account-xyz").is_empty());
    }

    // Tests [@ANCHOR: relay_broker_macos_run]
    #[test]
    fn adding_to_a_group_without_rights_or_for_a_missing_group_fails() {
        assert_eq!(
            MacOs::add_to_group_args("u", "g"),
            ["-o", "edit", "-a", "u", "-t", "user", "g"]
        );
        assert!(MacOs::default()
            .add_to_group("nobody", "no-such-group-xyz")
            .is_err());
    }

    // Tests [@ANCHOR: relay_broker_macos_new_udp_socket]
    // Tests [@ANCHOR: relay_broker_macos_bind_udp]
    #[test]
    fn a_bound_udp_descriptor_is_a_real_socket_on_the_port_and_the_port_is_then_taken() {
        let os = MacOs::default();
        let socket = std::net::UdpSocket::from(os.bind_udp(0).unwrap());
        let port = socket.local_addr().unwrap().port();
        assert_ne!(port, 0);
        assert!(os.bind_udp(port).is_err(), "a port in use is refused");
        // Oracle: lsof lists this process as holding that UDP port.
        let listed = tool(
            "/usr/sbin/lsof",
            &[
                "-nP",
                "-a",
                "-p",
                &std::process::id().to_string(),
                &format!("-iUDP:{port}"),
            ],
        );
        assert!(listed.contains("UDP"), "{listed}");
    }

    #[test]
    fn what_has_no_macos_counterpart_answers_unsupported_and_changes_nothing() {
        let os = MacOs::default();
        for result in [
            os.bind_dhcp_client("en0").map(|_| ()),
            os.hub_stop(),
            os.update_package(),
            os.poweroff(),
            os.set_system_time(0),
        ] {
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Unsupported);
        }
        assert_eq!(os.hub_status(), (false, false));
    }

    #[test]
    fn the_system_time_equals_the_date_tool() {
        let oracle: u64 = tool("/bin/date", &["+%s"]).parse().unwrap();
        let ours = MacOs::default().system_time_unix_secs();
        assert!(ours.abs_diff(oracle) <= 2);
    }

    // Tests [@ANCHOR: relay_broker_macos_serial_nodes]
    // Tests [@ANCHOR: relay_broker_macos_usb_identity]
    // Tests [@ANCHOR: relay_broker_macos_list_devices]
    #[test]
    fn every_listed_serial_node_exists_and_usb_ids_are_known_to_system_profiler() {
        let os = MacOs::default();
        let listed = os.list_devices();
        assert!(!listed.is_empty(), "every Mac has /dev/cu.debug-console");
        let profile = tool("/usr/sbin/system_profiler", &["SPUSBDataType"]).to_lowercase();
        for device in &listed {
            assert!(Path::new(&device.path).exists(), "{}", device.path);
            assert!(device.path.starts_with("/dev/cu.") || device.path.starts_with("/dev/tty."));
            if device.vid != 0 {
                assert!(
                    profile.contains(&format!("0x{:04x}", device.vid)),
                    "{} vendor {:04x}",
                    device.path,
                    device.vid
                );
                assert_eq!(
                    os.usb_identity(Path::new(&device.path)),
                    UsbIdentity::Usb(device.vid, device.pid, device.product.clone())
                );
            } else {
                assert_eq!(
                    os.usb_identity(Path::new(&device.path)),
                    UsbIdentity::NotUsb
                );
            }
        }
        assert_eq!(os.usb_identity(Path::new("/dev/null")), UsbIdentity::NotUsb);
        let broken = MacOs {
            ioreg: "/usr/bin/false".into(),
            ..MacOs::default()
        };
        assert_eq!(
            broken.usb_identity(Path::new("/dev/cu.debug-console")),
            UsbIdentity::Unreadable
        );
    }
}
