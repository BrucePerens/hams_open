// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctld`: a server for the Hamlib `rigctld` text protocol on top of radioctl, so WSJT-X,
//! fldigi, loggers and `rigctl -m 2` work unchanged against any radioctl rig, real or virtual.
//!
//! - The protocol is sans-IO ([`command`] parses, [`reply`] renders); only [`server`] and
//!   [`rigs`] touch sockets and the runtime, and neither creates one: the binary owns the one
//!   runtime and passes its `Handle`.
//! - Any number of clients share one rig through the rig's single actor queue ([`service`]); reads
//!   come from the actor's cache when fresh enough.
//! - PTT has one owner at a time with the splitter's arbiter, and transmit commands fail closed
//!   while the rig's transmit gate is closed (it is closed unless the configuration opens it).
//! - Loopback by default, an explicit option and an allow-list to serve a network, bounded line
//!   length, bounded client count, timeouts everywhere ([`config`], [`access`], [`server`]).
//! - Rotators (`[[rotator]]`) are served the same way in Hamlib's `rotctld` text protocol
//!   ([`rotctl`], [`rotators`]), through `radioctl-station`'s rotator engines; a position outside
//!   the description's travel is refused before anything is written.

pub mod access;
pub mod builtin;
pub mod candidates;
pub mod caps;
pub mod command;
pub mod config;
pub mod daemon;
pub mod discovery;
pub mod names;
pub mod reply;
pub mod rigs;
pub mod rotators;
pub mod rotctl;
pub mod selftest;
pub mod selftest_cli;
pub mod server;
pub mod service;
pub mod update;
pub mod upload;
