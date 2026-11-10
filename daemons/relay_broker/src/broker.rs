// SPDX-License-Identifier: AGPL-3.0-or-later
//! The actions. `Broker::handle` is the one function that turns an authenticated, parsed
//! request into a response: it rate-limits, consults the policy, performs the action through
//! the `Os` trait and writes the audit record. There is no string anywhere that becomes a
//! command: `list_devices` reads sysfs, `open_device` opens one validated node and returns the
//! descriptor, `grant_group`/`provision_groups` report (and only with `apply_groups = true`
//! change) group membership.

use std::os::fd::{AsFd, OwnedFd};
use std::time::Instant;

use crate::audit::{AuditEvent, AuditLog};
use crate::auth::AllowList;
use crate::config::Config;
use crate::hub::{self, HubSettings};
use crate::os::{Os, Peer};
use crate::policy::Policy;
use crate::ratelimit::RateLimiter;
use crate::time_guard::{TimeGuard, BUILD_DATE_UNIX_SECS};
use crate::wire::{DeviceClass, Group, Request, Response, Status, TimeSource};

/// The broker state shared by every connection.
pub struct Broker {
    /// Peers allowed in.
    pub allow: AllowList,
    /// The open policy.
    pub policy: Policy,
    /// Whether group changes are applied or only reported.
    pub apply_groups: bool,
    /// UDP ports `bind_udp` may bind (default: 123, the time service).
    pub bind_udp_ports: Vec<u16>,
    /// Whether the hub requests are served at all (default: refused).
    pub hub_enabled: bool,
    /// Whether `update_package` is served (default: refused).
    pub update_package: bool,
    /// Whether `set_system_time` is served (default: refused).
    pub set_time: bool,
    time_guard: TimeGuard,
    rate: RateLimiter,
    audit: AuditLog,
    os: Box<dyn Os + Send>,
}

impl Broker {
    // [@ANCHOR: relay_broker_broker_new]
    /// Assembles a broker from a config, an audit log and an OS layer.
    pub fn new(config: &Config, audit: AuditLog, os: Box<dyn Os + Send>) -> Broker {
        Broker {
            allow: config.allow.clone(),
            policy: config.policy.clone(),
            apply_groups: config.apply_groups,
            bind_udp_ports: config.bind_udp_ports.clone(),
            hub_enabled: config.hub_enabled,
            update_package: config.update_package,
            set_time: config.set_time,
            time_guard: TimeGuard::new(
                config.max_unproven_step_secs,
                config.max_time_steps_per_hour,
            ),
            rate: RateLimiter::new(config.rate_per_minute),
            audit,
            os,
        }
    }

    // [@ANCHOR: relay_broker_broker_os]
    /// The OS layer (the server uses it for peer credentials).
    pub fn os(&self) -> &dyn Os {
        self.os.as_ref()
    }

    // [@ANCHOR: relay_broker_broker_log]
    /// Writes one audit record; a failed write is ignored here (the log is best effort once
    /// it has been opened; opening it is mandatory).
    pub fn log(&mut self, peer: Peer, action: &'static str, outcome: &'static str, detail: String) {
        let _ = self.audit.record(&AuditEvent {
            uid: peer.uid,
            pid: peer.pid,
            action,
            outcome,
            detail,
        });
    }

    // [@ANCHOR: relay_broker_broker_handle]
    /// Decides and performs one request for an already-authenticated peer. Returns the response
    /// and, for a successful open, the descriptor to pass.
    pub fn handle(
        &mut self,
        peer: Peer,
        req: &Request,
        now: Instant,
    ) -> (Response, Option<OwnedFd>) {
        let action = match req {
            Request::ListDevices { .. } => "list_devices",
            Request::OpenDevice { .. } => "open_device",
            Request::GrantGroup { .. } => "grant_group",
            Request::ProvisionGroups => "provision_groups",
            Request::BindUdp { .. } => "bind_udp",
            Request::HubStart(_) => "hub_start",
            Request::HubStop => "hub_stop",
            Request::HubStatus => "hub_status",
            Request::UpdatePackage => "update_package",
            Request::SetSystemTime { .. } => "set_system_time",
            Request::TimeStatus => "time_status",
        };
        if !self.rate.allow(peer.uid, now) {
            self.log(peer, action, "rate_limited", String::new());
            return (
                Response::error(Status::RateLimited, "too many requests"),
                None,
            );
        }
        match req {
            Request::ListDevices { class } => {
                let devs: Vec<_> = self
                    .os
                    .list_devices()
                    .into_iter()
                    .filter(|d| class.is_none_or(|c| c == d.class))
                    .filter(|d| !self.policy.deny_usb.contains(&(d.vid, d.pid)))
                    .collect();
                self.log(peer, action, "allowed", format!("{} devices", devs.len()));
                (Response::Devices(devs), None)
            }
            Request::OpenDevice {
                class,
                read_only,
                path,
            } => self.open_device(peer, *class, *read_only, path),
            Request::GrantGroup { group } => self.grant(peer, action, &[*group]),
            Request::ProvisionGroups => self.grant(peer, action, &[Group::Dialout, Group::Audio]),
            Request::BindUdp { port } => self.bind_udp(peer, *port),
            Request::HubStart(settings) => self.hub_start(peer, settings),
            Request::HubStop => self.hub_stop(peer),
            Request::HubStatus => self.hub_status(peer),
            Request::UpdatePackage => self.update_package(peer),
            Request::SetSystemTime {
                target_unix_secs,
                source,
                agreeing,
            } => self.set_system_time(peer, now, *target_unix_secs, *source, *agreeing),
            Request::TimeStatus => self.time_status(peer, now),
        }
    }

    // [@ANCHOR: relay_broker_broker_set_system_time]
    /// Steps the clock when the config enables it and the ceilings in `time_guard.rs` allow it. Every outcome is audited with the
    /// clock before, the target, the source and the evidence.
    fn set_system_time(
        &mut self,
        peer: Peer,
        now: Instant,
        target: u64,
        source: TimeSource,
        agreeing: u8,
    ) -> (Response, Option<OwnedFd>) {
        if !self.set_time {
            self.log(
                peer,
                "set_system_time",
                "denied",
                "clock steps not enabled".into(),
            );
            return (
                Response::error(Status::Denied, "clock steps not enabled"),
                None,
            );
        }
        let before = self.os.system_time_unix_secs();
        let detail =
            format!("before={before} target={target} source={source:?} agreeing={agreeing}");
        if let Err(denial) = self.time_guard.check(now, before, target, source, agreeing) {
            self.log(
                peer,
                "set_system_time",
                "denied",
                format!("{detail} {}", denial.reason()),
            );
            return (Response::error(Status::Denied, denial.reason()), None);
        }
        match self.os.set_system_time(target) {
            Ok(()) => {
                self.time_guard.record_step(now);
                let after = self.os.system_time_unix_secs();
                self.log(
                    peer,
                    "set_system_time",
                    "allowed",
                    format!("{detail} after={after}"),
                );
                (
                    Response::TimeSet {
                        before_unix_secs: before,
                        after_unix_secs: after,
                    },
                    None,
                )
            }
            Err(e) => {
                self.log(peer, "set_system_time", "failed", format!("{detail} {e}"));
                (
                    Response::error(Status::Failed, format!("set time: {e}")),
                    None,
                )
            }
        }
    }

    // [@ANCHOR: relay_broker_broker_time_status]
    /// The clock and the step limits; read-only, always served.
    fn time_status(&mut self, peer: Peer, now: Instant) -> (Response, Option<OwnedFd>) {
        let (steps_last_hour, breaker_open) = self.time_guard.status(now);
        self.log(peer, "time_status", "allowed", String::new());
        (
            Response::TimeStatus {
                now_unix_secs: self.os.system_time_unix_secs(),
                steps_last_hour,
                breaker_open,
                min_target_unix_secs: BUILD_DATE_UNIX_SECS,
            },
            None,
        )
    }

    /// The package update action: one fixed command (`apt-get install --only-upgrade hams-local-relay`, or the `dnf` equivalent) that the
    /// caller cannot add to or change, so a verified-signature repository the owner configured is the only source of what installs.
    fn update_package(&mut self, peer: Peer) -> (Response, Option<OwnedFd>) {
        if !self.update_package {
            self.log(
                peer,
                "update_package",
                "denied",
                "updates not enabled".into(),
            );
            return (Response::error(Status::Denied, "updates not enabled"), None);
        }
        match self.os.update_package() {
            Ok(()) => {
                self.log(peer, "update_package", "allowed", String::new());
                (Response::Updated, None)
            }
            Err(e) => {
                self.log(peer, "update_package", "failed", e.to_string());
                (
                    Response::error(Status::Failed, format!("update: {e}")),
                    None,
                )
            }
        }
    }

    /// The bind action: only a port the config lists. The descriptor is passed; the relay never needs a privilege to hold it.
    fn bind_udp(&mut self, peer: Peer, port: u16) -> (Response, Option<OwnedFd>) {
        if !self.bind_udp_ports.contains(&port) {
            self.log(peer, "bind_udp", "denied", format!("port {port}"));
            return (Response::error(Status::Denied, "port not allowed"), None);
        }
        match self.os.bind_udp(port) {
            Ok(fd) => {
                self.log(peer, "bind_udp", "allowed", format!("port {port}"));
                (Response::Opened, Some(fd))
            }
            Err(e) => {
                self.log(peer, "bind_udp", "failed", format!("port {port}: {e}"));
                (Response::error(Status::Failed, format!("bind: {e}")), None)
            }
        }
    }

    fn hub_answer(&self) -> Response {
        let (access_point, dhcp) = self.os.hub_status();
        Response::Hub { access_point, dhcp }
    }

    /// The hub start action: settings validated, both files written from the fixed templates, the programs started.
    fn hub_start(&mut self, peer: Peer, settings: &HubSettings) -> (Response, Option<OwnedFd>) {
        if !self.hub_enabled {
            self.log(peer, "hub_start", "denied", "hub not enabled".into());
            return (Response::error(Status::Denied, "hub not enabled"), None);
        }
        let texts =
            hub::hostapd_conf(settings).and_then(|h| hub::dnsmasq_conf(settings).map(|d| (h, d)));
        let (hostapd, dnsmasq) = match texts {
            Ok(t) => t,
            Err(why) => {
                self.log(peer, "hub_start", "denied", why.clone());
                return (Response::error(Status::Denied, why), None);
            }
        };
        match self.os.hub_start(
            &hostapd,
            &dnsmasq,
            &settings.interface,
            settings.hub_address,
        ) {
            Ok(()) => {
                self.log(peer, "hub_start", "allowed", settings.interface.clone());
                (self.hub_answer(), None)
            }
            Err(e) => {
                self.log(peer, "hub_start", "failed", e.to_string());
                (Response::error(Status::Failed, format!("hub: {e}")), None)
            }
        }
    }

    fn hub_stop(&mut self, peer: Peer) -> (Response, Option<OwnedFd>) {
        if !self.hub_enabled {
            self.log(peer, "hub_stop", "denied", "hub not enabled".into());
            return (Response::error(Status::Denied, "hub not enabled"), None);
        }
        match self.os.hub_stop() {
            Ok(()) => {
                self.log(peer, "hub_stop", "allowed", String::new());
                (self.hub_answer(), None)
            }
            Err(e) => {
                self.log(peer, "hub_stop", "failed", e.to_string());
                (Response::error(Status::Failed, format!("hub: {e}")), None)
            }
        }
    }

    fn hub_status(&mut self, peer: Peer) -> (Response, Option<OwnedFd>) {
        self.log(peer, "hub_status", "allowed", String::new());
        (self.hub_answer(), None)
    }

    // [@ANCHOR: relay_broker_broker_open_device]
    /// The open action: lexical check, node walk, open, post-open check. Any failure closes the
    /// descriptor (by drop) and answers `Denied` or `Failed`.
    fn open_device(
        &mut self,
        peer: Peer,
        class: DeviceClass,
        read_only: bool,
        path: &str,
    ) -> (Response, Option<OwnedFd>) {
        let checked = self.policy.check_path(path).and_then(|clean| {
            self.policy
                .check_node(self.os.as_ref(), &clean, class)
                .map(|()| clean)
        });
        let clean = match checked {
            Ok(c) => c,
            Err(d) => {
                self.log(
                    peer,
                    "open_device",
                    "denied",
                    format!("{path}: {}", d.reason()),
                );
                return (Response::error(Status::Denied, d.reason()), None);
            }
        };
        let fd = match self.os.open_device(&clean, read_only) {
            Ok(fd) => fd,
            Err(e) => {
                self.log(peer, "open_device", "failed", format!("{path}: {e}"));
                return (Response::error(Status::Failed, format!("open: {e}")), None);
            }
        };
        if let Err(d) = self.policy.check_fd(self.os.fd_kind(fd.as_fd()), class) {
            self.log(
                peer,
                "open_device",
                "denied",
                format!("{path}: post-open {}", d.reason()),
            );
            return (Response::error(Status::Denied, d.reason()), None);
        }
        self.log(peer, "open_device", "allowed", clean.display().to_string());
        (Response::Opened, Some(fd))
    }

    // [@ANCHOR: relay_broker_broker_grant]
    /// The provisioning action: which of `groups` the caller lacks, applied only when the
    /// config says so. The account is the peer's own UID; a peer can never name another user.
    fn grant(
        &mut self,
        peer: Peer,
        action: &'static str,
        groups: &[Group],
    ) -> (Response, Option<OwnedFd>) {
        let Some(user) = self.os.username(peer.uid) else {
            self.log(peer, action, "failed", "unknown uid".into());
            return (Response::error(Status::Failed, "unknown uid"), None);
        };
        let have = self.os.groups_of(&user);
        let missing: Vec<String> = groups
            .iter()
            .map(|g| g.name().to_string())
            .filter(|g| !have.contains(g))
            .collect();
        let mut applied = false;
        if self.apply_groups && !missing.is_empty() {
            for g in &missing {
                if let Err(e) = self.os.add_to_group(&user, g) {
                    self.log(peer, action, "failed", format!("{user} {g}: {e}"));
                    return (
                        Response::error(Status::Failed, format!("add to {g}: {e}")),
                        None,
                    );
                }
            }
            applied = true;
        }
        let outcome = if missing.is_empty() {
            "allowed"
        } else if applied {
            "applied"
        } else {
            "dry_run"
        };
        self.log(
            peer,
            action,
            outcome,
            format!("{user}: {}", missing.join(",")),
        );
        (
            Response::GroupReport {
                applied,
                user,
                groups: missing,
            },
            None,
        )
    }
}
