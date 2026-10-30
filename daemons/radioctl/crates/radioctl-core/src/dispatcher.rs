// SPDX-License-Identifier: AGPL-3.0-or-later
//! The callback dispatcher.
//!
//! Application callbacks are synchronous and may block, so they run on a dedicated OS thread,
//! never on a runtime worker and never on the rig's actor. The actor hands events over through a
//! bounded queue under a mutex held only for a push; the actor never waits for the callback. When
//! the queue is full a new event is merged into the newest queued one (latest value per field
//! wins) and the loss is counted. A panicking callback is caught and counted; the thread lives on.

use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

use crate::error::{ErrorKind, RigError};
use crate::event::RigEvent;

/// An application callback for status changes.
pub type Callback = Arc<dyn Fn(RigEvent) + Send + Sync>;

/// Counters of one dispatcher.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DispatcherCounters {
    /// Events handed to the callback.
    pub delivered: u64,
    /// Times an event arrived at a full queue.
    pub queue_full: u64,
    /// Intermediate values lost by merging events (the drop counter).
    pub values_coalesced: u64,
    /// Callbacks that panicked.
    pub callback_panics: u64,
}

#[derive(Default)]
struct Queue {
    items: VecDeque<RigEvent>,
    closed: bool,
}

struct Inner {
    queue: Mutex<Queue>,
    wake: Condvar,
    capacity: usize,
    delivered: AtomicU64,
    queue_full: AtomicU64,
    values_coalesced: AtomicU64,
    callback_panics: AtomicU64,
}

/// A running dispatcher. Dropping it closes the queue; the thread finishes what is queued and
/// exits, and is not joined (a callback may be blocked).
pub struct Dispatcher {
    inner: Arc<Inner>,
}

fn lock(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    queue
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Dispatcher {
    // [@ANCHOR: radioctl_core_dispatcher_start]
    /// Starts the dispatcher thread for `callback` with a queue of `capacity` events.
    pub fn start(name: &str, capacity: usize, callback: Callback) -> Result<Dispatcher, RigError> {
        let inner = Arc::new(Inner {
            queue: Mutex::new(Queue::default()),
            wake: Condvar::new(),
            capacity: capacity.max(1),
            delivered: AtomicU64::new(0),
            queue_full: AtomicU64::new(0),
            values_coalesced: AtomicU64::new(0),
            callback_panics: AtomicU64::new(0),
        });
        let worker = Arc::clone(&inner);
        thread::Builder::new()
            .name(format!("radioctl-callbacks-{name}"))
            .spawn(move || worker.run(&callback))
            .map_err(|e| RigError::new(ErrorKind::LinkDown, format!("callback thread: {e}")))?;
        Ok(Dispatcher { inner })
    }

    // [@ANCHOR: radioctl_core_dispatcher_submit]
    /// Queues an event without ever waiting for the callback.
    pub fn submit(&self, event: RigEvent) {
        let mut queue = lock(&self.inner.queue);
        if queue.items.len() >= self.inner.capacity {
            self.inner.queue_full.fetch_add(1, Ordering::Relaxed);
            if let Some(last) = queue.items.back_mut() {
                let lost = last.merge(event);
                self.inner
                    .values_coalesced
                    .fetch_add(lost, Ordering::Relaxed);
                return;
            }
        }
        queue.items.push_back(event);
        self.inner.wake.notify_one();
    }

    // [@ANCHOR: radioctl_core_dispatcher_counters]
    /// A snapshot of the counters.
    pub fn counters(&self) -> DispatcherCounters {
        DispatcherCounters {
            delivered: self.inner.delivered.load(Ordering::Relaxed),
            queue_full: self.inner.queue_full.load(Ordering::Relaxed),
            values_coalesced: self.inner.values_coalesced.load(Ordering::Relaxed),
            callback_panics: self.inner.callback_panics.load(Ordering::Relaxed),
        }
    }
}

impl Drop for Dispatcher {
    fn drop(&mut self) {
        lock(&self.inner.queue).closed = true;
        self.inner.wake.notify_all();
    }
}

impl Inner {
    fn run(&self, callback: &Callback) {
        loop {
            let event = {
                let mut queue = lock(&self.queue);
                loop {
                    if let Some(event) = queue.items.pop_front() {
                        break event;
                    }
                    if queue.closed {
                        return;
                    }
                    queue = self
                        .wake
                        .wait(queue)
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                }
            };
            match catch_unwind(AssertUnwindSafe(|| callback(event))) {
                Ok(()) => self.delivered.fetch_add(1, Ordering::Relaxed),
                Err(_) => self.callback_panics.fetch_add(1, Ordering::Relaxed),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Change, Source};
    use crate::op::{Field, Observation};
    use std::sync::mpsc;
    use std::time::Duration;

    fn event(level: u16) -> RigEvent {
        let change = Change {
            field: Field::Signal,
            old: None,
            new: Observation::Signal(level),
        };
        RigEvent {
            rig: "r".into(),
            source: Source::Poll,
            changes: vec![change],
        }
    }

    // Tests [@ANCHOR: radioctl_core_dispatcher_start]
    // Tests [@ANCHOR: radioctl_core_dispatcher_submit]
    // Tests [@ANCHOR: radioctl_core_dispatcher_counters]
    #[test]
    fn delivers_in_order_and_survives_a_panicking_callback() {
        let (tx, rx) = mpsc::channel();
        let callback: Callback = Arc::new(move |event: RigEvent| {
            if event.changes.first().map(|c| c.new) == Some(Observation::Signal(13)) {
                panic!("callback bug");
            }
            let _ = tx.send(event);
        });
        let dispatcher = Dispatcher::start("test", 16, callback).expect("start");
        for level in [1, 13, 2] {
            dispatcher.submit(event(level));
        }
        let first = rx.recv_timeout(Duration::from_secs(5)).expect("first");
        let second = rx.recv_timeout(Duration::from_secs(5)).expect("second");
        assert_eq!(
            first.changes.first().map(|c| c.new),
            Some(Observation::Signal(1))
        );
        assert_eq!(
            second.changes.first().map(|c| c.new),
            Some(Observation::Signal(2))
        );
        let counters = dispatcher.counters();
        assert_eq!(counters.callback_panics, 1);
        assert_eq!(counters.delivered, 2);
    }

    // Tests [@ANCHOR: radioctl_core_dispatcher_submit]
    #[test]
    fn a_blocked_callback_never_blocks_submit_and_full_queues_coalesce() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel::<()>();
        let release_rx = Mutex::new(release_rx);
        let (out_tx, out_rx) = mpsc::channel();
        let callback: Callback = Arc::new(move |event: RigEvent| {
            let _ = started_tx.send(());
            let _ = lock_unit(&release_rx).recv_timeout(Duration::from_secs(10));
            let _ = out_tx.send(event);
        });
        let dispatcher = Dispatcher::start("slow", 2, callback).expect("start");
        dispatcher.submit(event(0));
        started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("callback entered");
        for level in 1..=10 {
            dispatcher.submit(event(level));
        }
        let counters = dispatcher.counters();
        assert_eq!(counters.queue_full, 8);
        assert_eq!(counters.values_coalesced, 8);
        for _ in 0..3 {
            release_tx.send(()).expect("release");
        }
        let mut last = None;
        for _ in 0..3 {
            last = Some(out_rx.recv_timeout(Duration::from_secs(5)).expect("event"));
        }
        let final_value = last.and_then(|e| e.changes.first().map(|c| c.new));
        assert_eq!(final_value, Some(Observation::Signal(10)));
    }

    fn lock_unit<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
