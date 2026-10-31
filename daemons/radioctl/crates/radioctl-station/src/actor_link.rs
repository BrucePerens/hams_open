// SPDX-License-Identifier: AGPL-3.0-or-later
//! The real rig actor behind the radio manager: [`ActorLinks`] is a [`RadioLink`] that spawns the
//! actor when the manager asks for a connection, respawns it when it is dead, and unkeys the radio
//! before it reports the link up.
//!
//! Why a respawn: a rig actor whose transport ended (a TCP stream that reached end of file, a
//! serial stream that closed) marks its link down and stays down; only a new transport and a new
//! actor help. A transport that reconnects by itself ([`radioctl_core::net::open_link`], the serial
//! link) keeps the actor alive and reports `Reconnecting`; that actor unkeys the radio itself when
//! the link returns (`Alarm::LinkRestored`), so [`ActorLinks`] never spawns a second actor on a
//! radio whose transport is still retrying.
//!
//! The unkey pass: a freshly spawned actor does not unkey on its first connection (it has no
//! reason to know the radio was ever keyed), but a radio that was keyed when the network or the
//! USB link died may still be. So after every spawn [`ActorLinks`] sends an unkey (the emergency
//! path: it needs no transmit gate and is never refused) and only then reports the link up. An
//! unkey that fails is a failed connection: the new actor is dropped and the manager's backoff
//! tries again. The radio's settings are only read, never restored.
//!
//! The transmit gate of a spawned actor is closed; the spawner decides whether to open it and on
//! whose authority. Nothing here transmits.

use crate::channel::BoxFuture;
use crate::error::{StationError, StationErrorKind};
use crate::manager::RadioLink;
use crate::model::DeviceId;
use crate::record::KnownState;
use crate::station::{KeyState, KeyedSource};
use radioctl_core::capability::Capability;
use radioctl_core::op::Op;
use radioctl_core::rig::{Freshness, RigHandle};
use radioctl_core::serial::LinkStatus;
use radioctl_core::units::Vfo;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::watch;

/// A freshly spawned rig actor and, when its transport reconnects by itself, that transport's
/// link status (the same watch the actor was given as `RigOptions::link_watch`).
pub struct RigConnection {
    /// The actor.
    pub handle: RigHandle,
    /// The transport's status, if it has one.
    pub watch: Option<watch::Receiver<LinkStatus>>,
}

/// Opens a radio's transport and spawns its actor. Implemented by the application, which knows
/// the engine, the transport and the options of each radio.
pub trait RigSpawner: Send + Sync {
    /// One attempt. It must not transmit, and must leave the transmit gate closed unless the
    /// owner's configuration says otherwise.
    fn spawn(&self, radio: &DeviceId) -> BoxFuture<'_, Result<RigConnection, StationError>>;
}

struct Slot {
    handle: RigHandle,
    watch: Option<watch::Receiver<LinkStatus>>,
}

#[derive(Default)]
struct Inner {
    slots: BTreeMap<DeviceId, Slot>,
    spawns: BTreeMap<DeviceId, u32>,
    journal: Vec<String>,
}

// [@ANCHOR: radioctl_station_actorlink_lock]
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// [@ANCHOR: radioctl_station_actorlink_transport_up]
/// Whether the transport status, when there is one, says the link is usable.
fn transport_up(watch: &Option<watch::Receiver<LinkStatus>>) -> bool {
    match watch {
        None => true,
        Some(w) => matches!(*w.borrow(), LinkStatus::Connected { .. }),
    }
}

/// The links of real rigs: spawns, respawns and unkeys. Share it as an `Arc`.
pub struct ActorLinks {
    spawner: Arc<dyn RigSpawner>,
    inner: Arc<Mutex<Inner>>,
    one_connect: tokio::sync::Mutex<()>,
    unkey_timeout: Duration,
}

impl ActorLinks {
    // [@ANCHOR: radioctl_station_actorlink_new]
    /// Links that spawn rigs through `spawner`; the unkey pass waits at most three seconds.
    pub fn new(spawner: Arc<dyn RigSpawner>) -> ActorLinks {
        ActorLinks {
            spawner,
            inner: Arc::new(Mutex::new(Inner::default())),
            one_connect: tokio::sync::Mutex::new(()),
            unkey_timeout: Duration::from_secs(3),
        }
    }

    // [@ANCHOR: radioctl_station_actorlink_with_unkey_timeout]
    /// Changes how long the unkey pass may take before the connection counts as failed.
    pub fn with_unkey_timeout(mut self, timeout: Duration) -> ActorLinks {
        self.unkey_timeout = timeout;
        self
    }

    // [@ANCHOR: radioctl_station_actorlink_handle]
    /// The current actor of a radio, for the application's own use (queries, the transmit gate).
    pub fn handle(&self, radio: &DeviceId) -> Option<RigHandle> {
        lock(&self.inner).slots.get(radio).map(|s| s.handle.clone())
    }

    // [@ANCHOR: radioctl_station_actorlink_key_source]
    /// A keyed-state source for the station's interlock that always follows the radio's current
    /// actor, so a respawn does not leave the interlock looking at a dead handle. A radio with no
    /// actor is unknown.
    pub fn key_source(&self, radio: &DeviceId) -> Arc<dyn KeyedSource> {
        Arc::new(LiveKey {
            inner: Arc::clone(&self.inner),
            radio: radio.clone(),
        })
    }

    // [@ANCHOR: radioctl_station_actorlink_spawns]
    /// How many actors were spawned for a radio.
    pub fn spawns(&self, radio: &DeviceId) -> u32 {
        lock(&self.inner).spawns.get(radio).copied().unwrap_or(0)
    }

    // [@ANCHOR: radioctl_station_actorlink_journal]
    /// What the links did, oldest first: spawns, unkey passes, refusals to double-spawn.
    pub fn journal(&self) -> Vec<String> {
        lock(&self.inner).journal.clone()
    }

    // [@ANCHOR: radioctl_station_actorlink_note]
    fn note(&self, line: String) {
        let mut g = lock(&self.inner);
        g.journal.push(line);
        let excess = g.journal.len().saturating_sub(500);
        g.journal.drain(..excess);
    }

    // [@ANCHOR: radioctl_station_actorlink_state]
    /// What the current slot of a radio says: up, retrying on its own, dead, or absent.
    fn slot_state(&self, radio: &DeviceId) -> SlotState {
        match lock(&self.inner).slots.get(radio) {
            None => SlotState::Absent,
            Some(s) if !transport_up(&s.watch) => {
                match s.watch.as_ref().map(|w| w.borrow().clone()) {
                    Some(LinkStatus::Reconnecting { .. }) => SlotState::Retrying,
                    _ => SlotState::Dead,
                }
            }
            Some(s) if s.handle.link_up() => SlotState::Up,
            Some(_) => SlotState::Dead,
        }
    }

    // [@ANCHOR: radioctl_station_actorlink_unkey_pass]
    /// The unkey pass on a new actor, then a read of what the radio is set to. The unkey is
    /// skipped (and said so) for a radio that cannot key at all.
    async fn unkey_pass(
        &self,
        radio: &DeviceId,
        handle: &RigHandle,
    ) -> Result<KnownState, StationError> {
        if handle.capabilities().has(Capability::SetPtt) {
            handle
                .execute_within(Op::SetPtt(false), self.unkey_timeout)
                .await
                .map_err(|e| {
                    StationError::new(
                        StationErrorKind::Driver,
                        format!("unkey pass on {} failed: {}", radio.0, e.detail),
                    )
                })?;
            self.note(format!("{}: unkey pass done", radio.0));
        } else {
            self.note(format!("{}: cannot transmit, unkey pass skipped", radio.0));
        }
        let mut known = KnownState::default();
        if let Ok(f) = handle.frequency(Vfo::A, Freshness::Fresh).await {
            known.frequency_hz = Some(f.value.0);
        }
        if let Ok(m) = handle.mode(Freshness::Fresh).await {
            known.mode = Some(m.value.name().to_string());
        }
        Ok(known)
    }
}

/// What a radio's slot says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Absent,
    Up,
    Retrying,
    Dead,
}

struct LiveKey {
    inner: Arc<Mutex<Inner>>,
    radio: DeviceId,
}

impl KeyedSource for LiveKey {
    // [@ANCHOR: radioctl_station_livekey_name]
    /// The radio's id.
    fn name(&self) -> String {
        self.radio.0.clone()
    }

    // [@ANCHOR: radioctl_station_livekey_key_state]
    /// The current actor's keyed state (see the `RigHandle` implementation); unknown with no
    /// actor, or while the transport is retrying.
    fn key_state(&self) -> BoxFuture<'_, KeyState> {
        Box::pin(async move {
            let current = lock(&self.inner)
                .slots
                .get(&self.radio)
                .map(|s| (s.handle.clone(), transport_up(&s.watch)));
            match current {
                Some((handle, true)) => handle.key_state().await,
                _ => KeyState::Unknown,
            }
        })
    }
}

impl RadioLink for ActorLinks {
    // [@ANCHOR: radioctl_station_actorlink_link_up]
    /// Up when the actor's link is up and its transport, if it has a status, is connected.
    fn link_up(&self, radio: &DeviceId) -> BoxFuture<'_, bool> {
        let up = self.slot_state(radio) == SlotState::Up;
        Box::pin(async move { up })
    }

    // [@ANCHOR: radioctl_station_actorlink_present]
    /// No hotplug or ping evidence from a bare actor.
    fn present(&self, _radio: &DeviceId) -> BoxFuture<'_, Option<bool>> {
        Box::pin(async { None })
    }

    // [@ANCHOR: radioctl_station_actorlink_connect]
    /// One connection attempt: nothing to do if the link is up; an error (and so the manager's
    /// backoff) while the transport is retrying on its own; otherwise a new actor, the unkey pass,
    /// and the state the radio reports. Attempts are serialised, so two ticks never spawn two
    /// actors for one radio.
    fn connect(&self, radio: &DeviceId) -> BoxFuture<'_, Result<Option<KnownState>, StationError>> {
        let radio = radio.clone();
        Box::pin(async move {
            let _one = self.one_connect.lock().await;
            match self.slot_state(&radio) {
                SlotState::Up => return Ok(None),
                SlotState::Retrying => {
                    self.note(format!(
                        "{}: transport is reconnecting by itself, not respawning",
                        radio.0
                    ));
                    return Err(StationError::new(
                        StationErrorKind::Driver,
                        "the transport is still reconnecting",
                    ));
                }
                SlotState::Absent | SlotState::Dead => {}
            }
            let conn = self.spawner.spawn(&radio).await?;
            {
                let mut g = lock(&self.inner);
                *g.spawns.entry(radio.clone()).or_insert(0) += 1;
            }
            self.note(format!("{}: actor spawned", radio.0));
            match self.unkey_pass(&radio, &conn.handle).await {
                Ok(known) => {
                    lock(&self.inner).slots.insert(
                        radio,
                        Slot {
                            handle: conn.handle,
                            watch: conn.watch,
                        },
                    );
                    Ok(Some(known))
                }
                Err(e) => {
                    self.note(format!("{}: {}", radio.0, e.detail));
                    Err(e)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests [@ANCHOR: radioctl_station_actorlink_transport_up]
    #[test]
    fn no_status_means_the_transport_is_usable() {
        assert!(transport_up(&None));
        let (tx, rx) = watch::channel(LinkStatus::Closed);
        assert!(!transport_up(&Some(rx)));
        drop(tx);
    }
}
