// SPDX-License-Identifier: AGPL-3.0-or-later
//! Peer authentication: the broker answers only a peer whose SO_PEERCRED UID or GID is on the
//! allow-list from the root-owned config. The check runs on accept, before a single byte of the
//! request is read, so an unknown peer never reaches the parser.

use crate::os::Peer;

/// The allow-list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AllowList {
    /// UIDs allowed.
    pub uids: Vec<u32>,
    /// GIDs allowed (the peer's primary GID).
    pub gids: Vec<u32>,
}

impl AllowList {
    // [@ANCHOR: relay_broker_allow_list_permits]
    /// True when the peer's UID or primary GID is listed. An empty list permits no one: a
    /// broker with no config answers nobody, which is the safe default.
    pub fn permits(&self, peer: Peer) -> bool {
        self.uids.contains(&peer.uid) || self.gids.contains(&peer.gid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: relay_broker_allow_list_permits]
    #[test]
    fn only_listed_uids_or_gids_pass_and_an_empty_list_passes_nobody() {
        let peer = Peer {
            uid: 1000,
            gid: 1000,
            pid: 1,
        };
        assert!(!AllowList::default().permits(peer));
        assert!(AllowList {
            uids: vec![1000],
            gids: vec![]
        }
        .permits(peer));
        assert!(AllowList {
            uids: vec![],
            gids: vec![1000]
        }
        .permits(peer));
        assert!(!AllowList {
            uids: vec![0, 1001],
            gids: vec![0]
        }
        .permits(peer));
    }
}
