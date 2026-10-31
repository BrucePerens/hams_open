// SPDX-License-Identifier: AGPL-3.0-or-later
//! Runs a [`VirtualRig`] behind an in-memory duplex stream so an actor can talk to it like to a
//! serial port. The only tokio code in this crate; the rig itself stays sans-IO.
//!
//! A reply delay models a slow radio: the rig applies a command at once but its answer reaches the
//! stream only after the delay, in order.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};

use crate::virtual_rig::{PanelChange, RigState, VirtualRig};

const STREAM_BYTES: usize = 4096;

struct Shared {
    rig: Mutex<VirtualRig>,
    delay: Mutex<Duration>,
    received: AtomicUsize,
    outbox: mpsc::UnboundedSender<(Instant, Vec<u8>)>,
    epoch: Instant,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Control of a running virtual rig: inspect its state, turn its knobs, change its speed.
#[derive(Clone)]
pub struct VirtualRigControl {
    shared: Arc<Shared>,
}

// [@ANCHOR: radioctl_virtual_io_spawn]
/// Starts `rig` on `runtime`; returns the stream to hand to an actor and a control handle.
pub fn spawn_virtual_rig(
    runtime: &Handle,
    rig: VirtualRig,
    reply_delay: Duration,
) -> (DuplexStream, VirtualRigControl) {
    let (near, far) = duplex(STREAM_BYTES);
    let (mut reader, mut writer) = tokio::io::split(far);
    let (outbox, mut inbox) = mpsc::unbounded_channel::<(Instant, Vec<u8>)>();
    let shared = Arc::new(Shared {
        rig: Mutex::new(rig),
        delay: Mutex::new(reply_delay),
        received: AtomicUsize::new(0),
        outbox,
        epoch: Instant::now(),
    });
    let rx_shared = Arc::clone(&shared);
    runtime.spawn(async move {
        let mut buffer = [0u8; 256];
        while let Ok(n) = reader.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            rx_shared.received.fetch_add(n, Ordering::Relaxed);
            let now = Instant::now();
            let delay = *lock(&rx_shared.delay);
            let reply = lock(&rx_shared.rig).feed(
                buffer.get(..n).unwrap_or(&[]),
                now.saturating_duration_since(rx_shared.epoch),
            );
            if !reply.is_empty() {
                let _ = rx_shared.outbox.send((now + delay, reply));
            }
        }
    });
    runtime.spawn(async move {
        while let Some((due, bytes)) = inbox.recv().await {
            sleep_until(due).await;
            if writer.write_all(&bytes).await.is_err() {
                break;
            }
        }
    });
    (near, VirtualRigControl { shared })
}

impl VirtualRigControl {
    // [@ANCHOR: radioctl_virtual_io_state]
    /// A copy of the rig's current state.
    pub fn state(&self) -> RigState {
        lock(&self.shared.rig).state().clone()
    }

    // [@ANCHOR: radioctl_virtual_io_front_panel]
    /// Turns a knob on the rig; any unsolicited bytes go out without delay.
    pub fn front_panel(&self, change: PanelChange) {
        let bytes = lock(&self.shared.rig).front_panel(change);
        if !bytes.is_empty() {
            let _ = self.shared.outbox.send((Instant::now(), bytes));
        }
    }

    // [@ANCHOR: radioctl_virtual_io_scope_sweep]
    /// Makes the rig send one scope sweep of its Main receiver on its own, as a rig with waveform
    /// output on does; nothing goes out when the model has no scope or output is off.
    pub fn scope_sweep(&self) {
        let bytes = lock(&self.shared.rig).scope_sweep();
        if !bytes.is_empty() {
            let _ = self.shared.outbox.send((Instant::now(), bytes));
        }
    }

    // [@ANCHOR: radioctl_virtual_io_set_reply_delay]
    /// Changes how long answers take from now on.
    pub fn set_reply_delay(&self, delay: Duration) {
        *lock(&self.shared.delay) = delay;
    }

    // [@ANCHOR: radioctl_virtual_io_bytes_received]
    /// How many bytes the controller has written to the rig.
    pub fn bytes_received(&self) -> usize {
        self.shared.received.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::description::RigDescription;
    use radioctl_core::units::Hz;

    // Tests [@ANCHOR: radioctl_virtual_io_spawn]
    // Tests [@ANCHOR: radioctl_virtual_io_state]
    // Tests [@ANCHOR: radioctl_virtual_io_bytes_received]
    #[tokio::test(start_paused = true)]
    async fn answers_after_the_reply_delay_in_order() {
        let rig = VirtualRig::from_description(&RigDescription::ic7300()).expect("rig");
        let (mut stream, control) =
            spawn_virtual_rig(&Handle::current(), rig, Duration::from_millis(300));
        let started = Instant::now();
        stream
            .write_all(&[0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD])
            .await
            .expect("write");
        let mut reply = [0u8; 11];
        stream.read_exact(&mut reply).await.expect("read");
        assert_eq!(
            reply,
            [0xFE, 0xFE, 0xE0, 0x94, 0x03, 0x00, 0x40, 0x07, 0x14, 0x00, 0xFD]
        );
        assert!(Instant::now() - started >= Duration::from_millis(300));
        assert_eq!(control.bytes_received(), 6);
        assert_eq!(control.state().freq_a, Hz(14_074_000));
    }

    // Tests [@ANCHOR: radioctl_virtual_io_front_panel]
    // Tests [@ANCHOR: radioctl_virtual_io_set_reply_delay]
    #[tokio::test(start_paused = true)]
    async fn front_panel_bytes_arrive_without_delay() {
        let rig = VirtualRig::from_description(&RigDescription::ic7300()).expect("rig");
        let (mut stream, control) =
            spawn_virtual_rig(&Handle::current(), rig, Duration::from_secs(5));
        control.set_reply_delay(Duration::from_secs(9));
        let started = Instant::now();
        control.front_panel(PanelChange::Frequency(Hz(7_074_000)));
        let mut frame = [0u8; 11];
        stream.read_exact(&mut frame).await.expect("read");
        assert_eq!(frame.get(5..8), Some(&[0x00, 0x40, 0x07][..]));
        assert!(Instant::now() - started < Duration::from_secs(1));
    }
}
