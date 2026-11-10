// SPDX-License-Identifier: AGPL-3.0-or-later
//! Starts, stops and reports the stand-alone hub's two programs (`hostapd`, `dnsmasq`). Fixed programs with fixed argument lists, no shell. Configuration files
//! and pid files live in one root-owned directory (`/run/hams-relay-broker/hub`); both programs daemonise themselves, so they outlive the broker's idle exit and
//! their pid files are the only state. A pid is trusted only while `/proc/<pid>/comm` still names the program.

use std::fs;
use std::io;
use std::net::Ipv4Addr;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

/// Where the hub's programs and files are.
#[derive(Debug, Clone)]
pub struct HubRunner {
    /// Configuration and pid files.
    pub dir: PathBuf,
    /// The access-point program.
    pub hostapd: PathBuf,
    /// The DHCP and DNS program.
    pub dnsmasq: PathBuf,
    /// The `ip` program, used to give the wifi interface the hub's address.
    pub ip: PathBuf,
}

impl Default for HubRunner {
    fn default() -> HubRunner {
        HubRunner {
            dir: PathBuf::from("/run/hams-relay-broker/hub"),
            hostapd: PathBuf::from("/usr/sbin/hostapd"),
            dnsmasq: PathBuf::from("/usr/sbin/dnsmasq"),
            ip: PathBuf::from("/usr/sbin/ip"),
        }
    }
}

/// The pid in a pid file, if the file holds a number.
fn read_pid(path: &Path) -> Option<i32> {
    fs::read_to_string(path)
        .ok()?
        .trim()
        .parse::<i32>()
        .ok()
        .filter(|p| *p > 1)
}

/// True when `pid` is a live process whose name is `comm`.
fn is_running(pid: i32, comm: &str) -> bool {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .map(|name| name.trim() == comm)
        .unwrap_or(false)
}

/// Writes a file readable by root only.
fn write_private(path: &Path, text: &str) -> io::Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}

impl HubRunner {
    fn pid_file(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.pid"))
    }

    fn run(&self, program: &Path, args: &[&std::ffi::OsStr]) -> io::Result<()> {
        let status = Command::new(program)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "{} exited with {status}",
                program.display()
            )))
        }
    }

    /// Whether `hostapd` and `dnsmasq` are running.
    pub fn status(&self) -> (bool, bool) {
        let up =
            |name: &str| read_pid(&self.pid_file(name)).is_some_and(|pid| is_running(pid, name));
        (up("hostapd"), up("dnsmasq"))
    }

    /// Writes the two configuration files, gives the interface the hub's /24 address and starts both programs. A failure stops what was started.
    pub fn start(
        &self,
        hostapd_conf: &str,
        dnsmasq_conf: &str,
        interface: &str,
        address: Ipv4Addr,
    ) -> io::Result<()> {
        self.stop()?;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)?;
        let hostapd_path = self.dir.join("hostapd.conf");
        let dnsmasq_path = self.dir.join("dnsmasq.conf");
        write_private(&hostapd_path, hostapd_conf)?;
        write_private(&dnsmasq_path, dnsmasq_conf)?;
        write_private(&self.dir.join("interface"), interface)?;
        let started = (|| {
            use std::ffi::OsStr;
            let cidr = format!("{address}/24");
            self.run(
                &self.ip,
                &["addr", "replace", &cidr, "dev", interface].map(OsStr::new),
            )?;
            self.run(&self.ip, &["link", "set", interface, "up"].map(OsStr::new))?;
            let hostapd_pid = self.pid_file("hostapd");
            self.run(
                &self.hostapd,
                &[
                    OsStr::new("-B"),
                    OsStr::new("-P"),
                    hostapd_pid.as_os_str(),
                    hostapd_path.as_os_str(),
                ],
            )?;
            let conf_arg = format!("--conf-file={}", dnsmasq_path.display());
            let pid_arg = format!("--pid-file={}", self.pid_file("dnsmasq").display());
            self.run(
                &self.dnsmasq,
                &[OsStr::new(&conf_arg), OsStr::new(&pid_arg)],
            )
        })();
        if let Err(e) = started {
            let _ = self.stop();
            return Err(e);
        }
        Ok(())
    }

    /// Stops both programs and removes the files and the interface address. Stopping a hub that is not running succeeds.
    pub fn stop(&self) -> io::Result<()> {
        for name in ["hostapd", "dnsmasq"] {
            if let Some(pid) = read_pid(&self.pid_file(name)).filter(|p| is_running(*p, name)) {
                let _ = kill(Pid::from_raw(pid), Signal::SIGTERM);
            }
        }
        if let Ok(interface) = fs::read_to_string(self.dir.join("interface")) {
            let interface = interface.trim();
            let plain = !interface.is_empty()
                && interface
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.');
            if plain {
                let _ = self.run(
                    &self.ip,
                    &["addr", "flush", "dev", interface].map(std::ffi::OsStr::new),
                );
            }
        }
        for file in [
            "hostapd.conf",
            "dnsmasq.conf",
            "interface",
            "hostapd.pid",
            "dnsmasq.pid",
        ] {
            match fs::remove_file(self.dir.join(file)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("relay_broker_hub_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn script(path: &Path, body: &str) {
        fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A runner whose "hostapd" and "dnsmasq" are scripts that start a copy of `sleep` named like the real program and write its pid file.
    fn fake(tag: &str) -> (HubRunner, PathBuf) {
        let root = temp_dir(tag);
        let sleep = ["/usr/bin/sleep", "/bin/sleep"]
            .into_iter()
            .find(|p| Path::new(p).exists())
            .unwrap();
        for name in ["hostapd", "dnsmasq"] {
            fs::create_dir_all(root.join(format!("d_{name}"))).unwrap();
            fs::copy(sleep, root.join(format!("d_{name}/{name}"))).unwrap();
        }
        script(
            &root.join("fake_hostapd"),
            &format!(
                "{}/d_hostapd/hostapd 300 >/dev/null 2>&1 &\necho $! > \"$3\"",
                root.display()
            ),
        );
        script(
            &root.join("fake_dnsmasq"),
            &format!(
                "{}/d_dnsmasq/dnsmasq 300 >/dev/null 2>&1 &\necho $! > \"${{2#--pid-file=}}\"",
                root.display()
            ),
        );
        script(
            &root.join("fake_ip"),
            "echo \"$@\" >> \"$(dirname \"$0\")/ip.log\"",
        );
        let runner = HubRunner {
            dir: root.join("hub"),
            hostapd: root.join("fake_hostapd"),
            dnsmasq: root.join("fake_dnsmasq"),
            ip: root.join("fake_ip"),
        };
        (runner, root)
    }

    #[test]
    fn start_runs_both_programs_and_stop_ends_them_and_removes_the_files() {
        let (hub, root) = fake("cycle");
        assert_eq!(hub.status(), (false, false));
        hub.stop().unwrap();
        hub.start(
            "interface=wlan0\n",
            "interface=wlan0\n",
            "wlan0",
            Ipv4Addr::new(192, 168, 50, 1),
        )
        .unwrap();
        // A backgrounded process may not have exec'd yet when its starter exits.
        for _ in 0..100 {
            if hub.status() == (true, true) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(hub.status(), (true, true));
        let mode = fs::metadata(hub.dir.join("hostapd.conf"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "configuration is private");
        let ip = fs::read_to_string(root.join("ip.log")).unwrap();
        assert!(
            ip.contains("addr replace 192.168.50.1/24 dev wlan0"),
            "{ip}"
        );
        hub.stop().unwrap();
        // The signalled processes may take a moment to leave /proc.
        for _ in 0..50 {
            if hub.status() == (false, false) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(!hub.dir.join("hostapd.conf").exists());
        let ip = fs::read_to_string(root.join("ip.log")).unwrap();
        assert!(ip.contains("addr flush dev wlan0"), "{ip}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_pid_that_is_not_the_program_is_never_signalled_or_reported() {
        let (hub, root) = fake("stale");
        fs::create_dir_all(&hub.dir).unwrap();
        // Our own pid is alive but is not "hostapd".
        fs::write(hub.pid_file("hostapd"), std::process::id().to_string()).unwrap();
        assert_eq!(hub.status(), (false, false));
        hub.stop().unwrap();
        assert!(!hub.pid_file("hostapd").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_program_that_fails_to_start_leaves_nothing_behind() {
        let (mut hub, root) = fake("fail");
        script(&root.join("fake_dnsmasq"), "exit 3");
        hub.dnsmasq = root.join("fake_dnsmasq");
        let err = hub
            .start("a\n", "b\n", "wlan0", Ipv4Addr::new(192, 168, 50, 1))
            .unwrap_err();
        assert!(err.to_string().contains("exited"), "{err}");
        assert!(!hub.dir.join("dnsmasq.conf").exists());
        // The hostapd that did start was stopped again.
        for _ in 0..50 {
            if !hub.status().0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(hub.status(), (false, false));
        fs::remove_dir_all(&root).unwrap();
    }
}
