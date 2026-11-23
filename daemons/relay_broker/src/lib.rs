// SPDX-License-Identifier: AGPL-3.0-or-later
//! `hams-relay-broker`: the privileged broker of the relay's zero-privilege split
//! (`RELAY_ZERO_PRIVILEGE_SPLIT.md` sections 3.1, 3.5, 3.6). A small process that answers a
//! fixed set of typed requests from authenticated local peers over a Unix socket: enumerate
//! devices, open one device and pass the descriptor, report (or, when configured, fix) group
//! membership. No web, no async runtime, no FFI, no shell, no `unsafe` outside one commented
//! descriptor adoption. See `README.md` for the threat model and the integration path.
//!
//! Module map: `wire` (schema and strict parser), `transport` (frames and SCM_RIGHTS), `auth`
//! (SO_PEERCRED allow-list), `policy` (which nodes may be opened), `broker` (the actions),
//! `server` (the accept loop), `client` (the caller side), `config`, `audit`, `ratelimit`, and
//! `os` (the OS trait with its Linux implementation; the tests supply a fake).

#[cfg(unix)]
pub mod audit;
#[cfg(unix)]
pub mod auth;
#[cfg(unix)]
pub mod broker;
#[cfg(unix)]
pub mod cli;
#[cfg(unix)]
pub mod client;
#[cfg(unix)]
pub mod config;
pub mod hub;
#[cfg(target_os = "linux")]
pub mod hub_runner;
#[cfg(unix)]
pub mod os;
#[cfg(target_os = "macos")]
pub mod os_macos;
#[cfg(unix)]
pub mod policy;
#[cfg(unix)]
pub mod ratelimit;
#[cfg(unix)]
pub mod server;
#[cfg(test)]
mod testdir;
pub mod time_guard;
#[cfg(unix)]
pub mod transport;
#[cfg(windows)]
pub mod windows;
pub mod wire;
