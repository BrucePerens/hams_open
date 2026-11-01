// SPDX-License-Identifier: AGPL-3.0-or-later
//! The USB link: a USB device (an SDR board or dongle) behind the byte-stream
//! [`crate::transport::Transport`] the rig actor uses for every other link.
//!
//! The actor writes request frames ([`crate::usb_wire::UsbTransfer`]) and reads reply frames
//! ([`crate::usb_wire::UsbReply`]). A supervisor thread owns the open [`UsbDevice`], decodes the
//! request frames, performs the transfers one at a time and writes the replies back. It follows
//! the rules of the serial and network links:
//!
//! * When the device is lost (a transfer reports [`UsbFailure::NoDevice`]) the status becomes
//!   [`LinkStatus::Reconnecting`], the actor refuses commands with `LinkDown`, bytes it writes
//!   meanwhile are dropped and counted (never queued, so a stale command is not replayed into a
//!   device that comes back), and the supervisor reopens the device through the
//!   [`UsbBackend`] with exponential backoff. The device may have been re-enumerated, so it is
//!   looked up again by id and strings every time.
//! * Once reopened the status is `Connected` with a higher generation. The actor then unkeys
//!   the rig before it serves any other command ([`crate::rig::RigOptions::link_watch`]); the
//!   device may have been keyed when it was lost. Nothing in this module keys a device.
//! * A transfer that times out or stalls is reported to the engine in its reply and does not take
//!   the link down.
//! * Every transfer is bounded by [`UsbConfig::transfer_timeout`].
//!
//! The device is driven by a dedicated thread, never by a runtime worker, because libusb
//! transfers block.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self as std_mpsc, RecvTimeoutError, TrySendError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, watch};

use crate::backoff::Backoff;
use crate::error::{ErrorKind, RigError};
use crate::serial::{LinkStatus, SerialStats};
use crate::usb_device::{UsbBackend, UsbDevice, UsbTarget};
use crate::usb_wire::{execute, FrameError, UsbReply, UsbStatus, UsbTransfer, MAX_DATA};

const STREAM_BYTES: usize = 8192;
const WRITE_QUEUE: usize = 64;
const READ_QUEUE: usize = 64;
const SUPERVISOR_TICK: Duration = Duration::from_millis(100);

/// Counters of a USB link (the same shape as a serial link's: bytes are frame bytes).
pub type UsbStats = SerialStats;

/// How to reach a USB device and keep the link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbConfig {
    /// Which device.
    pub target: UsbTarget,
    /// Longest wait for one transfer.
    pub transfer_timeout: Duration,
    /// Waits between reopen attempts.
    pub backoff: Backoff,
    /// Attempts for the first open (at least one); later losses are retried without limit.
    pub first_open_attempts: u32,
}

impl UsbConfig {
    // [@ANCHOR: radioctl_core_usb_config_new]
    /// One-second transfers, the default backoff and one first attempt.
    pub fn new(target: UsbTarget) -> UsbConfig {
        UsbConfig {
            target,
            transfer_timeout: Duration::from_secs(1),
            backoff: Backoff::default(),
            first_open_attempts: 1,
        }
    }
}

#[derive(Default)]
struct Counters {
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    bytes_dropped: AtomicU64,
    reconnects: AtomicU64,
}

fn snapshot(counters: &Counters) -> UsbStats {
    SerialStats {
        bytes_in: counters.bytes_in.load(Ordering::Relaxed),
        bytes_out: counters.bytes_out.load(Ordering::Relaxed),
        bytes_dropped: counters.bytes_dropped.load(Ordering::Relaxed),
        reconnects: counters.reconnects.load(Ordering::Relaxed),
    }
}

/// A running USB link: the byte stream for the actor and the link status.
pub struct UsbLink {
    /// The byte stream to hand to [`crate::rig::spawn`]. Dropping it closes the device.
    pub transport: DuplexStream,
    status: watch::Receiver<LinkStatus>,
    counters: Arc<Counters>,
}

impl UsbLink {
    // [@ANCHOR: radioctl_core_usb_link_status]
    /// The current link status.
    pub fn status(&self) -> LinkStatus {
        self.status.borrow().clone()
    }

    // [@ANCHOR: radioctl_core_usb_link_watch]
    /// A receiver that wakes on every status change.
    pub fn watch(&self) -> watch::Receiver<LinkStatus> {
        self.status.clone()
    }

    // [@ANCHOR: radioctl_core_usb_link_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> UsbStats {
        snapshot(&self.counters)
    }

    // [@ANCHOR: radioctl_core_usb_link_into_parts]
    /// Splits the link into the byte stream (for [`crate::rig::spawn`]) and the monitor the
    /// owner keeps for the life of the rig. Dropping the monitor does not close the device;
    /// dropping the stream does.
    pub fn into_parts(self) -> (DuplexStream, UsbMonitor) {
        (
            self.transport,
            UsbMonitor {
                status: self.status,
                counters: self.counters,
            },
        )
    }
}

/// The part of a [`UsbLink`] that stays with the owner after the stream went to the actor.
pub struct UsbMonitor {
    status: watch::Receiver<LinkStatus>,
    counters: Arc<Counters>,
}

impl UsbMonitor {
    // [@ANCHOR: radioctl_core_usb_monitor_status]
    /// The current link status.
    pub fn status(&self) -> LinkStatus {
        self.status.borrow().clone()
    }

    // [@ANCHOR: radioctl_core_usb_monitor_watch]
    /// A receiver that wakes on every status change.
    pub fn watch(&self) -> watch::Receiver<LinkStatus> {
        self.status.clone()
    }

    // [@ANCHOR: radioctl_core_usb_monitor_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> UsbStats {
        snapshot(&self.counters)
    }
}

enum Command {
    Write(Vec<u8>),
    Shutdown,
}

fn link_error(detail: impl Into<String>) -> RigError {
    RigError::new(ErrorKind::LinkDown, detail)
}

enum SessionEnd {
    Shutdown,
    Lost(String),
}

struct Supervisor {
    backend: Arc<dyn UsbBackend>,
    config: UsbConfig,
    commands: std_mpsc::Receiver<Command>,
    to_actor: mpsc::Sender<Vec<u8>>,
    status: watch::Sender<LinkStatus>,
    counters: Arc<Counters>,
}

impl Supervisor {
    // [@ANCHOR: radioctl_core_usb_supervisor_run]
    /// The supervisor thread: first open, then sessions and reopens until shutdown.
    fn run(self, first: oneshot::Sender<Result<(), RigError>>) {
        let attempts = self.config.first_open_attempts.max(1);
        let mut first = Some(first);
        let mut opened = None;
        let mut last = link_error("no attempt made");
        for attempt in 1..=attempts {
            match self.backend.open(&self.config.target) {
                Ok(device) => {
                    opened = Some(device);
                    break;
                }
                Err(error) => last = error,
            }
            if attempt < attempts && !self.sleep_dropping(self.config.backoff.delay(attempt)) {
                break;
            }
        }
        let Some(mut device) = opened else {
            if let Some(first) = first.take() {
                let _ = first.send(Err(last));
            }
            let _ = self.status.send(LinkStatus::Closed);
            return;
        };
        let mut generation = 1u32;
        loop {
            let _ = self.status.send(LinkStatus::Connected {
                port: PathBuf::from(self.config.target.label()),
                generation,
            });
            if let Some(first) = first.take() {
                let _ = first.send(Ok(()));
            }
            let SessionEnd::Lost(mut why) = self.session(device.as_mut()) else {
                break;
            };
            drop(device);
            let mut failures = 0u32;
            let reopened = loop {
                failures += 1;
                let _ = self.status.send(LinkStatus::Reconnecting {
                    attempt: failures,
                    last_error: why.clone(),
                });
                if !self.sleep_dropping(self.config.backoff.delay(failures)) {
                    break None;
                }
                match self.backend.open(&self.config.target) {
                    Ok(found) => break Some(found),
                    Err(error) => why = error.detail,
                }
            };
            match reopened {
                Some(next) => {
                    device = next;
                    generation += 1;
                    self.counters.reconnects.fetch_add(1, Ordering::Relaxed);
                }
                None => break,
            }
        }
        let _ = self.status.send(LinkStatus::Closed);
    }

    // [@ANCHOR: radioctl_core_usb_sleep_dropping]
    /// Waits `delay`, dropping (and counting) bytes the actor writes meanwhile. Returns false if
    /// the link was asked to shut down.
    fn sleep_dropping(&self, delay: Duration) -> bool {
        let until = Instant::now() + delay;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return true;
            }
            match self.commands.recv_timeout(left) {
                Ok(Command::Write(bytes)) => {
                    self.counters
                        .bytes_dropped
                        .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                }
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => return false,
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    // [@ANCHOR: radioctl_core_usb_session]
    /// One open device: decode request frames, perform them, send the replies, until shutdown or
    /// loss.
    fn session(&self, device: &mut dyn UsbDevice) -> SessionEnd {
        let mut pending: Vec<u8> = Vec::new();
        loop {
            match self.commands.recv_timeout(SUPERVISOR_TICK) {
                Ok(Command::Write(bytes)) => {
                    self.counters
                        .bytes_out
                        .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    pending.extend_from_slice(&bytes);
                    if let Some(why) = self.serve(device, &mut pending) {
                        return SessionEnd::Lost(why);
                    }
                }
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    return SessionEnd::Shutdown
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    // [@ANCHOR: radioctl_core_usb_serve]
    /// Performs every complete request frame in `pending`; `Some(why)` when the device was lost.
    fn serve(&self, device: &mut dyn UsbDevice, pending: &mut Vec<u8>) -> Option<String> {
        loop {
            let reply = match UsbTransfer::decode(pending) {
                Ok((transfer, used)) => {
                    pending.drain(..used);
                    execute(device, &transfer, self.config.transfer_timeout)
                }
                Err(FrameError::Incomplete) => return None,
                Err(FrameError::Malformed) => {
                    // Out of step with the actor: drop the garbage and say so.
                    pending.clear();
                    UsbReply {
                        op: 0,
                        status: UsbStatus::Other,
                        data: Vec::new(),
                    }
                }
            };
            let bytes = reply.encode();
            self.counters
                .bytes_in
                .fetch_add(bytes.len() as u64, Ordering::Relaxed);
            let _ = self.to_actor.blocking_send(bytes);
            if reply.status == UsbStatus::NoDevice {
                pending.clear();
                return Some("device is gone".to_string());
            }
        }
    }
}

// [@ANCHOR: radioctl_core_usb_open]
/// Opens the device through `backend` and starts the supervisor thread and bridge tasks on
/// `runtime`.
///
/// Returns once the first open has succeeded, or with the error of the last attempt. The returned
/// stream goes to [`crate::rig::spawn`]; dropping it (with the rig) closes the device.
pub async fn open_usb(
    runtime: &Handle,
    backend: Arc<dyn UsbBackend>,
    config: UsbConfig,
) -> Result<UsbLink, RigError> {
    let (near, far) = duplex(STREAM_BYTES);
    let (mut far_reader, mut far_writer) = tokio::io::split(far);
    let (command_tx, command_rx) = std_mpsc::sync_channel::<Command>(WRITE_QUEUE);
    let (to_actor_tx, mut to_actor_rx) = mpsc::channel::<Vec<u8>>(READ_QUEUE);
    let (status_tx, status_rx) = watch::channel(LinkStatus::Reconnecting {
        attempt: 0,
        last_error: "opening".into(),
    });
    let counters = Arc::new(Counters::default());
    let (first_tx, first_rx) = oneshot::channel();
    let supervisor = Supervisor {
        backend,
        config,
        commands: command_rx,
        to_actor: to_actor_tx,
        status: status_tx,
        counters: Arc::clone(&counters),
    };
    thread::Builder::new()
        .name("radioctl-usb".into())
        .spawn(move || supervisor.run(first_tx))
        .map_err(|e| link_error(format!("could not start the USB thread: {e}")))?;
    let uplink = command_tx.clone();
    runtime.spawn(async move {
        let mut buffer = vec![0u8; MAX_DATA];
        while let Ok(n) = far_reader.read(&mut buffer).await {
            if n == 0 {
                break;
            }
            let mut command = Command::Write(buffer.get(..n).unwrap_or(&[]).to_vec());
            loop {
                match uplink.try_send(command) {
                    Ok(()) => break,
                    Err(TrySendError::Full(back)) => {
                        command = back;
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                    Err(TrySendError::Disconnected(_)) => return,
                }
            }
        }
        let _ = uplink.send(Command::Shutdown);
    });
    runtime.spawn(async move {
        while let Some(bytes) = to_actor_rx.recv().await {
            if far_writer.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = far_writer.shutdown().await;
    });
    match first_rx.await {
        Ok(Ok(())) => Ok(UsbLink {
            transport: near,
            status: status_rx,
            counters,
        }),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(link_error("USB thread ended before opening the device")),
    }
}
