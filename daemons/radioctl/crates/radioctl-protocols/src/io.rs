// SPDX-License-Identifier: AGPL-3.0-or-later
//! Runs a [`VirtualRig`] behind an in-memory duplex stream so an actor can talk to it like to a
//! serial port. The only tokio code in this crate; the rig itself stays sans-IO.
//!
//! A reply delay models a slow radio: the rig applies a command at once but its answer reaches the
//! stream only after the delay, in order. The other knobs are fault injection for the timing
//! policy: jitter on the delay, a bus echo of the controller's own bytes, replies split into
//! small writes, replies that lose their last byte, and replies lost altogether.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// Added to the delay at random, up to this much (fault injection: an uneven radio).
    jitter: Mutex<Duration>,
    /// State of the jitter's own generator (xorshift, seeded; the tests are repeatable).
    jitter_state: Mutex<u64>,
    received: AtomicUsize,
    /// Answers still to be lost on the way back (fault injection: a corrupted frame).
    drop_replies: AtomicUsize,
    /// Answers still to lose their last byte (fault injection: a frame cut short).
    truncate_replies: AtomicUsize,
    /// True when the controller's own bytes come back first (a single-wire bus echo).
    echo: AtomicBool,
    /// Largest write of an answer; 0 writes each answer whole.
    chunk: AtomicUsize,
    outbox: mpsc::UnboundedSender<(Instant, Vec<u8>)>,
    epoch: Instant,
}

/// Gap between the chunks of one answer when `chunk` is set.
const CHUNK_GAP: Duration = Duration::from_millis(1);

// [@ANCHOR: radioctl_virtual_io_next_jitter]
/// The next random share of `jitter` (0 to the whole of it), from the shared generator.
fn next_jitter(shared: &Shared) -> Duration {
    let jitter = *lock(&shared.jitter);
    if jitter.is_zero() {
        return Duration::ZERO;
    }
    let mut state = lock(&shared.jitter_state);
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    let fraction = (x % 1_000_001) as f64 / 1_000_000.0;
    jitter.mul_f64(fraction)
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
        jitter: Mutex::new(Duration::ZERO),
        jitter_state: Mutex::new(0x9E37_79B9_7F4A_7C15),
        received: AtomicUsize::new(0),
        drop_replies: AtomicUsize::new(0),
        truncate_replies: AtomicUsize::new(0),
        echo: AtomicBool::new(false),
        chunk: AtomicUsize::new(0),
        outbox,
        epoch: Instant::now(),
    });
    let chunk_shared = Arc::clone(&shared);
    let rx_shared = Arc::clone(&shared);
    runtime.spawn(async move {
        let mut buffer = [0u8; 256];
        while let Ok(n) = reader.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            rx_shared.received.fetch_add(n, Ordering::Relaxed);
            let now = Instant::now();
            let bytes = buffer.get(..n).unwrap_or(&[]);
            if rx_shared.echo.load(Ordering::Acquire) {
                let _ = rx_shared.outbox.send((now, bytes.to_vec()));
            }
            let delay = *lock(&rx_shared.delay) + next_jitter(&rx_shared);
            let mut reply =
                lock(&rx_shared.rig).feed(bytes, now.saturating_duration_since(rx_shared.epoch));
            if reply.is_empty() {
                continue;
            }
            let take_one = |counter: &AtomicUsize| {
                counter
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_sub(1))
                    .is_ok()
            };
            if take_one(&rx_shared.drop_replies) {
                continue;
            }
            if take_one(&rx_shared.truncate_replies) {
                reply.pop();
            }
            let _ = rx_shared.outbox.send((now + delay, reply));
        }
    });
    runtime.spawn(async move {
        while let Some((due, bytes)) = inbox.recv().await {
            sleep_until(due).await;
            let chunk = chunk_shared.chunk.load(Ordering::Acquire);
            if chunk == 0 {
                if writer.write_all(&bytes).await.is_err() {
                    break;
                }
                continue;
            }
            let mut pieces = bytes.chunks(chunk).peekable();
            while let Some(piece) = pieces.next() {
                if writer.write_all(piece).await.is_err() {
                    return;
                }
                if pieces.peek().is_some() {
                    sleep_until(Instant::now() + CHUNK_GAP).await;
                }
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

    // [@ANCHOR: radioctl_virtual_io_forget_session]
    /// The rig forgets the session set up on the old link (see `VirtualRig::forget_session`).
    pub fn forget_session(&self) {
        lock(&self.shared.rig).forget_session();
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

    // [@ANCHOR: radioctl_virtual_io_drop_replies]
    /// Loses the next `count` answers on the way back (as a corrupted frame would be), the
    /// commands themselves still taking effect.
    pub fn drop_replies(&self, count: usize) {
        self.shared.drop_replies.store(count, Ordering::Release);
    }

    // [@ANCHOR: radioctl_virtual_io_set_reply_jitter]
    /// Adds a random 0 to `jitter` to every answer's delay from now on (repeatable sequence).
    pub fn set_reply_jitter(&self, jitter: Duration) {
        *lock(&self.shared.jitter) = jitter;
    }

    // [@ANCHOR: radioctl_virtual_io_set_echo]
    /// Makes the controller's own bytes come back on the stream before the answer, as a
    /// single-wire CI-V bus does, or stops that.
    pub fn set_echo(&self, on: bool) {
        self.shared.echo.store(on, Ordering::Release);
    }

    // [@ANCHOR: radioctl_virtual_io_set_reply_chunk]
    /// Writes every answer in pieces of at most `bytes` with a short gap between them, so a
    /// frame arrives across several reads; 0 writes answers whole.
    pub fn set_reply_chunk(&self, bytes: usize) {
        self.shared.chunk.store(bytes, Ordering::Release);
    }

    // [@ANCHOR: radioctl_virtual_io_truncate_replies]
    /// Cuts the last byte off the next `count` answers (a frame that never ends), the
    /// commands themselves still taking effect.
    pub fn truncate_replies(&self, count: usize) {
        self.shared.truncate_replies.store(count, Ordering::Release);
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
