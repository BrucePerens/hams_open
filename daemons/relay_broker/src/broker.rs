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
use crate::os::{Os, Peer};
use crate::policy::Policy;
use crate::ratelimit::RateLimiter;
use crate::wire::{DeviceClass, Group, Request, Response, Status};

/// The broker state shared by every connection.
pub struct Broker {
    /// Peers allowed in.
    pub allow: AllowList,
    /// The open policy.
    pub policy: Policy,
    /// Whether group changes are applied or only reported.
    pub apply_groups: bool,
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
        }
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
