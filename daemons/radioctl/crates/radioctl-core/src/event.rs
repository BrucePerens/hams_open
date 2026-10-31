// SPDX-License-Identifier: AGPL-3.0-or-later
//! Status-change events: what the callback and the async stream receive.

use crate::op::{Field, Observation};

/// Where a change came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    /// An operation made through the library's API.
    Api,
    /// Found by the actor's polling.
    Poll,
    /// An unsolicited frame from the rig (transceive, auto-information), or a reply that
    /// arrived after its command had been abandoned.
    Transceive,
    /// An operation that arrived through the protocol splitter.
    Splitter,
    /// The actor's own transmit watchdog: a forced unkey, or an unkey that failed.
    Watchdog,
}

/// One changed field with its old and new value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    /// The field.
    pub field: Field,
    /// The cached value before, `None` the first time the field is seen.
    pub old: Option<Observation>,
    /// The value now.
    pub new: Observation,
}

/// A status change of one rig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigEvent {
    /// The rig's identifier.
    pub rig: String,
    /// What caused the change.
    pub source: Source,
    /// The changed fields.
    pub changes: Vec<Change>,
}

impl RigEvent {
    // [@ANCHOR: radioctl_core_event_merge]
    /// Folds a later event of the same rig into this one. Per field the earliest `old` and the
    /// latest `new` are kept. Returns how many intermediate values were lost by the merge.
    pub fn merge(&mut self, later: RigEvent) -> u64 {
        let mut lost = 0;
        self.source = later.source;
        for change in later.changes {
            match self.changes.iter_mut().find(|c| c.field == change.field) {
                Some(existing) => {
                    existing.new = change.new;
                    lost += 1;
                }
                None => self.changes.push(change),
            }
        }
        lost
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{Hz, Vfo};

    fn freq(old: Option<u64>, new: u64) -> Change {
        let field = Field::Frequency(Vfo::A);
        Change {
            field,
            old: old.map(|hz| Observation::Frequency(Vfo::A, Hz(hz))),
            new: Observation::Frequency(Vfo::A, Hz(new)),
        }
    }

    // Tests [@ANCHOR: radioctl_core_event_merge]
    #[test]
    fn merge_keeps_earliest_old_and_latest_new() {
        let mut first = RigEvent {
            rig: "r".into(),
            source: Source::Poll,
            changes: vec![freq(Some(1), 2)],
        };
        let second = RigEvent {
            rig: "r".into(),
            source: Source::Transceive,
            changes: vec![freq(Some(2), 3)],
        };
        assert_eq!(first.merge(second), 1);
        assert_eq!(first.changes, vec![freq(Some(1), 3)]);
        assert_eq!(first.source, Source::Transceive);
        let mode = Change {
            field: Field::Ptt,
            old: None,
            new: Observation::Ptt(true),
        };
        let third = RigEvent {
            rig: "r".into(),
            source: Source::Api,
            changes: vec![mode],
        };
        assert_eq!(first.merge(third), 0);
        assert_eq!(first.changes.len(), 2);
    }
}
