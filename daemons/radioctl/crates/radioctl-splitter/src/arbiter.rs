// SPDX-License-Identifier: AGPL-3.0-or-later
//! PTT ownership: one transmitter, one owner at a time.

/// A client identifier assigned by the splitter.
pub type ClientId = u64;

/// Who holds the transmitter.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PttOwner {
    owner: Option<ClientId>,
}

impl PttOwner {
    // [@ANCHOR: radioctl_splitter_arbiter_try_key]
    /// Claims the transmitter for `client`. `Ok` when it was free or already theirs; `Err` with the
    /// current owner otherwise. The claim is made before the rig is asked, and given back with
    /// [`PttOwner::release`] if the rig refuses.
    pub fn try_key(&mut self, client: ClientId) -> Result<(), ClientId> {
        match self.owner {
            None => {
                self.owner = Some(client);
                Ok(())
            }
            Some(current) if current == client => Ok(()),
            Some(current) => Err(current),
        }
    }

    // [@ANCHOR: radioctl_splitter_arbiter_release]
    /// Gives the transmitter up if `client` holds it. Returns whether it did.
    pub fn release(&mut self, client: ClientId) -> bool {
        if self.owner == Some(client) {
            self.owner = None;
            true
        } else {
            false
        }
    }

    // [@ANCHOR: radioctl_splitter_arbiter_clear]
    /// Clears ownership because the transmitter was unkeyed by anyone (unkeying is always allowed).
    pub fn clear(&mut self) {
        self.owner = None;
    }

    // [@ANCHOR: radioctl_splitter_arbiter_owner]
    /// The current owner, if any.
    pub fn owner(&self) -> Option<ClientId> {
        self.owner
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_splitter_arbiter_try_key]
    // Tests [@ANCHOR: radioctl_splitter_arbiter_release]
    // Tests [@ANCHOR: radioctl_splitter_arbiter_clear]
    // Tests [@ANCHOR: radioctl_splitter_arbiter_owner]
    #[test]
    fn the_first_client_to_key_owns_the_transmitter() {
        let mut ptt = PttOwner::default();
        assert_eq!(ptt.owner(), None);
        assert_eq!(ptt.try_key(1), Ok(()));
        assert_eq!(ptt.try_key(1), Ok(()));
        assert_eq!(ptt.try_key(2), Err(1));
        assert!(!ptt.release(2));
        assert_eq!(ptt.owner(), Some(1));
        assert!(ptt.release(1));
        assert_eq!(ptt.try_key(2), Ok(()));
        ptt.clear();
        assert_eq!(ptt.owner(), None);
    }
}
