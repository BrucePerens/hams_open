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

pub mod audit;
pub mod auth;
pub mod broker;
pub mod client;
pub mod config;
pub mod os;
pub mod policy;
pub mod ratelimit;
pub mod server;
pub mod transport;
pub mod wire;
