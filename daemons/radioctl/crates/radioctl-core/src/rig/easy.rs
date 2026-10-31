// SPDX-License-Identifier: AGPL-3.0-or-later
//! One-call helpers on a rig handle for the activities the manual shows again and again: read or
//! set a level, read a meter, wait for a field to change, run a scope stream, and hold the
//! transmitter for a bounded time.
//!
//! Every helper is built from the public operations and obeys the same gates. None opens the
//! transmit gate on its own: the only helper that touches the gate, [`RigHandle::transmit_lease`],
//! takes the [`TransmitPermit`] as an argument, so the decision stays visible at the call site.

use std::time::Duration;

use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Instant};

use super::{Cached, EventStream, Freshness, RigHandle, ScopeStream, Source, TransmitPermit};
use crate::error::{ErrorKind, RigError};
use crate::event::Change;
use crate::op::{Field, Observation, Op};
use crate::units::{Level, Meter};

impl RigHandle {
    // [@ANCHOR: radioctl_core_easy_level]
    /// A numeric level in the rig's own units, from the cache or the radio as `freshness` says.
    pub async fn level(&self, level: Level, freshness: Freshness) -> Result<Cached<u16>, RigError> {
        let read = self
            .read(Field::Level(level), Op::GetLevel(level), freshness)
            .await?;
        match read.value {
            Observation::Level(got, value) if got == level => Ok(Cached {
                value,
                age: read.age,
            }),
            other => Err(RigError::new(
                ErrorKind::Malformed,
                format!("expected level {level:?}, got {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_core_easy_set_level]
    /// Sets a numeric level in the rig's own units.
    pub async fn set_level(&self, level: Level, value: u16) -> Result<(), RigError> {
        self.execute(Op::SetLevel(level, value)).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_core_easy_meter]
    /// A raw meter (SWR, ALC, COMP, Vd, Id) in the rig's own scale. Read-only.
    pub async fn meter(&self, meter: Meter, freshness: Freshness) -> Result<Cached<u16>, RigError> {
        let read = self
            .read(Field::Meter(meter), Op::GetMeter(meter), freshness)
            .await?;
        match read.value {
            Observation::Meter(got, value) if got == meter => Ok(Cached {
                value,
                age: read.age,
            }),
            other => Err(RigError::new(
                ErrorKind::Malformed,
                format!("expected meter {meter:?}, got {other:?}"),
            )),
        }
    }

    // [@ANCHOR: radioctl_core_easy_start_scope]
    /// Subscribes to the scope sweeps and then tells the rig to send them, in that order, so the
    /// first sweep is not missed. Receive-only. [`RigHandle::stop_scope`] ends the output.
    pub async fn start_scope(&self) -> Result<ScopeStream, RigError> {
        let stream = self.scope_frames();
        self.execute(Op::SetScopeOutput(true)).await?;
        Ok(stream)
    }

    // [@ANCHOR: radioctl_core_easy_stop_scope]
    /// Tells the rig to stop sending scope sweeps.
    pub async fn stop_scope(&self) -> Result<(), RigError> {
        self.execute(Op::SetScopeOutput(false)).await.map(|_| ())
    }

    // [@ANCHOR: radioctl_core_easy_transmit_lease]
    /// Opens this rig's transmit gate under `permit` for at most `max`, and returns the lease
    /// that holds it. The gate is opened here, by the caller's explicit permit; no other helper
    /// opens it. When the lease ends (released, dropped, or `max` elapsed) the gate is closed and
    /// the rig is unkeyed. A lease needs a closed gate and a runtime to time it: it is refused
    /// with `Rejected` if the gate is already open (someone else decided that), with
    /// `Rejected` for a zero `max`, and with `Unsupported` outside a tokio runtime.
    pub fn transmit_lease(
        &self,
        permit: TransmitPermit,
        max: Duration,
    ) -> Result<TransmitLease, RigError> {
        if max.is_zero() {
            return Err(RigError::new(
                ErrorKind::Rejected,
                "a transmit lease must last longer than zero",
            ));
        }
        let runtime = Handle::try_current()
            .map_err(|e| RigError::new(ErrorKind::Unsupported, format!("no tokio runtime: {e}")))?;
        if self.transmit_permit().is_some() {
            return Err(RigError::new(
                ErrorKind::Rejected,
                "the transmit gate is already open; a lease needs a closed gate",
            ));
        }
        self.open_transmit_gate(permit);
        let deadline = Instant::now() + max;
        let rig = self.clone();
        let timer = runtime.spawn(async move {
            sleep(max).await;
            rig.end_lease();
        });
        Ok(TransmitLease {
            rig: self.clone(),
            deadline,
            timer,
            ended: false,
        })
    }

    /// Closes the gate and fires an unkey. Synchronous: an unkey takes the emergency path and
    /// is never refused, so this works from `Drop` and from a timer without awaiting anything.
    fn end_lease(&self) {
        self.close_transmit_gate();
        // The answer is not awaited: the radio is unkeyed by the actor either way, and a rig
        // without a PTT command has nothing to unkey.
        drop(self.submit(Op::SetPtt(false), self.shared.default_timeout, Source::Api));
    }
}

impl EventStream {
    // [@ANCHOR: radioctl_core_easy_next_change]
    /// Waits for the next event that changes `field` and returns that change with the event's
    /// source. Other events are skipped. `None` once the rig has stopped. Wrap it in
    /// `tokio::time::timeout` to bound the wait.
    pub async fn next_change(&mut self, field: Field) -> Option<(Change, Source)> {
        loop {
            let event = self.next().await?;
            if let Some(change) = event.changes.into_iter().find(|c| c.field == field) {
                return Some((change, event.source));
            }
        }
    }
}

/// The transmit gate held open for a bounded time. Created by [`RigHandle::transmit_lease`].
/// Dropping it ends the lease: the gate closes and the rig is unkeyed.
pub struct TransmitLease {
    rig: RigHandle,
    deadline: Instant,
    timer: JoinHandle<()>,
    ended: bool,
}

impl TransmitLease {
    // [@ANCHOR: radioctl_core_easy_lease_remaining]
    /// How long the lease has left; zero once it has ended.
    pub fn remaining(&self) -> Duration {
        if self.ended || self.timer.is_finished() {
            return Duration::ZERO;
        }
        self.deadline.saturating_duration_since(Instant::now())
    }

    // [@ANCHOR: radioctl_core_easy_lease_key]
    /// Keys the rig. Refused with `TransmitGateClosed` once the lease has ended, like any
    /// transmit command with the gate closed.
    pub async fn key(&self) -> Result<(), RigError> {
        self.rig.set_ptt(true).await
    }

    // [@ANCHOR: radioctl_core_easy_lease_unkey]
    /// Unkeys the rig. Always allowed; the lease and its gate stay as they are.
    pub async fn unkey(&self) -> Result<(), RigError> {
        self.rig.set_ptt(false).await
    }

    // [@ANCHOR: radioctl_core_easy_lease_transmit_for]
    /// Keys the rig, holds the key for `duration` (cut short if the lease ends first), and
    /// unkeys. If this future is dropped while keyed the lease's `Drop` unkeys.
    pub async fn transmit_for(&self, duration: Duration) -> Result<(), RigError> {
        self.key().await?;
        sleep(duration.min(self.remaining())).await;
        self.unkey().await
    }

    // [@ANCHOR: radioctl_core_easy_lease_release]
    /// Ends the lease now: closes the gate, unkeys, and reports whether the unkey succeeded.
    pub async fn release(mut self) -> Result<(), RigError> {
        self.ended = true;
        self.timer.abort();
        self.rig.close_transmit_gate();
        self.rig.set_ptt(false).await
    }
}

impl Drop for TransmitLease {
    // [@ANCHOR: radioctl_core_easy_lease_drop]
    /// Ends the lease if it has not ended (the timer ends it otherwise): the gate closes and the
    /// rig is unkeyed.
    fn drop(&mut self) {
        self.timer.abort();
        if !self.ended && !self.timer.is_finished() {
            self.rig.end_lease();
        }
    }
}
