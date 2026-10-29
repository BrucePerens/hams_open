// SPDX-License-Identifier: AGPL-3.0-or-later
//! Transmit safety: releasing a rig that may be left keyed.

use super::proto::Client;
use std::time::Duration;

/// Called when a leftover rigctld of a dead server is about to be stopped: asks it to un-key the rig first. Best effort and silent
/// when the rig does not answer, because the daemon is stopped right after.
pub fn release_after_crash(_rig: &str, port: u16) -> bool {
    // [@ANCHOR: hamlib_release_after_crash]
    let Ok(mut c) = Client::connect("127.0.0.1", port, Duration::from_secs(2)) else {
        return false;
    };
    (0..3).any(|_| c.command("set_ptt 0").is_ok())
}
