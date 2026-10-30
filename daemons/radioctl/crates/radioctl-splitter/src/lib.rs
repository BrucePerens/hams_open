// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-splitter`: virtual serial endpoints that speak the Icom IC-7300 CI-V or the Kenwood
//! TS-2000 CAT protocol to outside software (WSJT-X, fldigi, loggers) while every request becomes
//! an operation on the one rig actor that owns the real radio, whatever its make.
//!
//! - Every request goes through the rig's single queue ([`radioctl_core::rig::RigHandle`]); reads
//!   are answered from the actor's cache when it is fresh enough, so several programs polling
//!   the same radio cost the radio almost nothing and never contend for the port.
//! - Each client (a pty, a TCP connection, any stream) has its own personality state: CI-V echo
//!   semantics, TS-2000 auto-information flag, and unsolicited transceive frames pushed to it when
//!   the rig changes.
//! - One transmitter, one owner: the first client to key PTT owns it; another client's key is
//!   refused until the owner unkeys, or disconnects (which unkeys the rig). Unkeying is allowed to
//!   everyone. Transmit-capable requests still pass the rig's closed-by-default transmit gate.
//!
//! The personalities are sans-IO ([`personality::Personality`]); only `session` and `pty` use
//! tokio and threads. Windows has no pty: use the TCP endpoint there.

pub mod arbiter;
pub mod civ_personality;
pub mod personality;
#[cfg(unix)]
pub mod pty;
pub mod session;
pub mod ts2000_personality;
