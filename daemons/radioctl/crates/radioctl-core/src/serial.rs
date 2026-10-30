// SPDX-License-Identifier: AGPL-3.0-or-later
//! Serial transport: a real serial port (including a USB serial adapter or a pty) behind the
//! async [`crate::transport::Transport`] the rig actor uses.
//!
//! The port is driven by dedicated operating-system threads, never by a runtime worker: one
//! supervisor thread opens the port, writes to it and reconnects; one reader thread per
//! connection reads it. They talk to the async side through bounded channels and an in-memory
//! duplex stream that the caller's runtime (the `Handle` passed in) feeds. The library creates no
//! runtime and calls no `block_on`.
//!
//! Control lines are separate from the byte stream: [`SerialControl::set_line`] is an immediate
//! call on a second handle of the port, never queued behind reads or writes. Keying through a
//! line is gated by [`crate::keyer::LineKeyer`].
//!
//! Hot-plug: when the device disappears (read or write fails, or the port hangs up), the link goes
//! to [`LinkStatus::Reconnecting`], bytes written meanwhile are dropped and counted (a command
//! then ends in its own typed timeout), and the supervisor reopens the port with exponential
//! backoff, looking the port up again by USB id when the target is a [`UsbMatcher`], because the
//! device node usually changes. The byte stream the actor holds stays open throughout and ends
//! only when the caller drops it.
//!
//! Safety note: operating systems assert DTR and RTS when a port is opened. The lines are made
//! inactive immediately after the open, but a glitch of microseconds happens first; hardware that
//! keys on RTS or DTR needs an interface that tolerates it.

use std::io::{self, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self as std_mpsc, RecvTimeoutError, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serialport::SerialPort;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot, watch};

use crate::backoff::Backoff;
use crate::error::{ErrorKind, RigError};
use crate::keyer::{ControlLines, KeyLine};
use crate::usb::{list_ports, select_port, UsbMatcher};

const STREAM_BYTES: usize = 4096;
const READ_SLICE: Duration = Duration::from_millis(50);
const WRITE_QUEUE: usize = 64;
const READ_QUEUE: usize = 64;
const WRITE_RETRIES: u32 = 40;
const SUPERVISOR_TICK: Duration = Duration::from_millis(100);

/// Data bits per character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    /// Seven.
    Seven,
    /// Eight.
    Eight,
}

/// Parity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    /// None.
    None,
    /// Odd.
    Odd,
    /// Even.
    Even,
}

/// Stop bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    /// One.
    One,
    /// Two.
    Two,
}

/// Flow control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// None.
    None,
    /// RTS/CTS. Do not combine with RTS keying.
    Hardware,
}

/// Whether the port has modem control lines this library may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineControl {
    /// DTR and RTS exist: they are made inactive (or left at the configured idle level) after
    /// every open, and a failure to do that fails the open. This is the default and the safe
    /// choice for anything that could be wired to a transmitter.
    Managed,
    /// The port has no modem lines (a pseudo-terminal, a CDC ACM gadget, a network bridge): they
    /// are never touched and [`SerialControl::set_line`] reports them unsupported.
    Unmanaged,
}

/// Where the port is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortTarget {
    /// A device path such as `/dev/ttyUSB0` (or a symlink to one).
    Path(PathBuf),
    /// Found by USB ids at every (re)connect.
    Usb(UsbMatcher),
}

/// How to open and keep a serial port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SerialConfig {
    /// The port.
    pub target: PortTarget,
    /// Baud rate.
    pub baud: u32,
    /// Data bits.
    pub data_bits: DataBits,
    /// Parity.
    pub parity: Parity,
    /// Stop bits.
    pub stop_bits: StopBits,
    /// Flow control.
    pub flow: Flow,
    /// Whether DTR and RTS are managed.
    pub lines: LineControl,
    /// Level DTR is left at after opening (`false` is inactive).
    pub idle_dtr: bool,
    /// Level RTS is left at after opening (`false` is inactive).
    pub idle_rts: bool,
    /// Waits between reconnect attempts.
    pub backoff: Backoff,
    /// Attempts for the first open (at least one); a failure after that is an error to the
    /// caller. Later disconnects are retried without limit.
    pub first_open_attempts: u32,
}

impl SerialConfig {
    // [@ANCHOR: radioctl_core_serial_config_new]
    /// 8N1, no flow control, both control lines inactive, default backoff, one open attempt.
    pub fn new(target: PortTarget, baud: u32) -> SerialConfig {
        SerialConfig {
            target,
            baud,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
            flow: Flow::None,
            lines: LineControl::Managed,
            idle_dtr: false,
            idle_rts: false,
            backoff: Backoff::default(),
            first_open_attempts: 1,
        }
    }
}

/// The state of the physical link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkStatus {
    /// The port is open. `generation` counts opens, starting at 1.
    Connected {
        /// The device path in use.
        port: PathBuf,
        /// How many times the port has been opened.
        generation: u32,
    },
    /// The port is gone; the supervisor is trying to reopen it.
    Reconnecting {
        /// Failed attempts since the last connection.
        attempt: u32,
        /// Why the last attempt failed.
        last_error: String,
    },
    /// The supervisor has stopped (the byte stream was dropped).
    Closed,
}

/// Counters of a serial link.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SerialStats {
    /// Bytes read from the port.
    pub bytes_in: u64,
    /// Bytes written to the port.
    pub bytes_out: u64,
    /// Bytes written while the port was down and therefore dropped.
    pub bytes_dropped: u64,
    /// Successful reconnects (not counting the first open).
    pub reconnects: u64,
}

#[derive(Default)]
struct Counters {
    bytes_in: AtomicU64,
    bytes_out: AtomicU64,
    bytes_dropped: AtomicU64,
    reconnects: AtomicU64,
}

type SharedPort = Arc<Mutex<Option<Box<dyn SerialPort>>>>;

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Immediate access to the control lines of the port. Clone it freely.
#[derive(Clone)]
pub struct SerialControl {
    port: SharedPort,
    lines: LineControl,
}

impl ControlLines for SerialControl {
    // [@ANCHOR: radioctl_core_serial_control_set_line]
    fn set_line(&self, line: KeyLine, asserted: bool) -> Result<(), RigError> {
        if self.lines == LineControl::Unmanaged {
            return Err(RigError::new(
                ErrorKind::Unsupported,
                "this port has no modem control lines",
            ));
        }
        let mut guard = locked(&self.port);
        let Some(port) = guard.as_mut() else {
            return Err(RigError::new(
                ErrorKind::LinkDown,
                "serial port is not open",
            ));
        };
        let result = match line {
            KeyLine::Rts => port.write_request_to_send(asserted),
            KeyLine::Dtr => port.write_data_terminal_ready(asserted),
        };
        result.map_err(|e| RigError::new(ErrorKind::LinkDown, format!("control line: {e}")))
    }
}

/// A running serial link: the byte stream for the actor, the control lines, the link status.
pub struct SerialLink {
    /// The byte stream to hand to [`crate::rig::spawn`]. Dropping it closes the port.
    pub transport: DuplexStream,
    /// The control lines.
    pub control: SerialControl,
    status: watch::Receiver<LinkStatus>,
    counters: Arc<Counters>,
}

impl SerialLink {
    // [@ANCHOR: radioctl_core_serial_link_status]
    /// The current link status.
    pub fn status(&self) -> LinkStatus {
        self.status.borrow().clone()
    }

    // [@ANCHOR: radioctl_core_serial_link_watch]
    /// A receiver that wakes on every status change.
    pub fn watch(&self) -> watch::Receiver<LinkStatus> {
        self.status.clone()
    }

    // [@ANCHOR: radioctl_core_serial_link_stats]
    /// A snapshot of the counters.
    pub fn stats(&self) -> SerialStats {
        SerialStats {
            bytes_in: self.counters.bytes_in.load(Ordering::Relaxed),
            bytes_out: self.counters.bytes_out.load(Ordering::Relaxed),
            bytes_dropped: self.counters.bytes_dropped.load(Ordering::Relaxed),
            reconnects: self.counters.reconnects.load(Ordering::Relaxed),
        }
    }
}

enum Command {
    Write(Vec<u8>),
    Shutdown,
}

fn link_error(detail: impl Into<String>) -> RigError {
    RigError::new(ErrorKind::LinkDown, detail)
}

// [@ANCHOR: radioctl_core_serial_resolve]
/// The device path for a target: the path itself, or the one port that matches the USB ids.
pub fn resolve(target: &PortTarget) -> Result<PathBuf, RigError> {
    match target {
        PortTarget::Path(path) => Ok(path.clone()),
        PortTarget::Usb(matcher) => {
            let ports = list_ports();
            select_port(matcher, &ports).map(|p| p.path.clone())
        }
    }
}

// [@ANCHOR: radioctl_core_serial_open_port]
fn open_port(config: &SerialConfig) -> Result<(Box<dyn SerialPort>, PathBuf), RigError> {
    let path = resolve(&config.target)?;
    let name = path.to_string_lossy().into_owned();
    let mut port = serialport::new(name.clone(), config.baud)
        .data_bits(match config.data_bits {
            DataBits::Seven => serialport::DataBits::Seven,
            DataBits::Eight => serialport::DataBits::Eight,
        })
        .parity(match config.parity {
            Parity::None => serialport::Parity::None,
            Parity::Odd => serialport::Parity::Odd,
            Parity::Even => serialport::Parity::Even,
        })
        .stop_bits(match config.stop_bits {
            StopBits::One => serialport::StopBits::One,
            StopBits::Two => serialport::StopBits::Two,
        })
        .flow_control(match config.flow {
            Flow::None => serialport::FlowControl::None,
            Flow::Hardware => serialport::FlowControl::Hardware,
        })
        .timeout(READ_SLICE)
        .open()
        .map_err(|e| link_error(format!("open {name}: {e}")))?;
    if config.lines == LineControl::Managed {
        port.write_data_terminal_ready(config.idle_dtr)
            .map_err(|e| link_error(format!("DTR on {name}: {e}")))?;
        if config.flow == Flow::None {
            port.write_request_to_send(config.idle_rts)
                .map_err(|e| link_error(format!("RTS on {name}: {e}")))?;
        }
    }
    Ok((port, path))
}

// [@ANCHOR: radioctl_core_serial_write_fully]
/// Writes all of `bytes`, riding out short timeouts for up to about two seconds.
fn write_fully(port: &mut dyn SerialPort, mut bytes: &[u8]) -> io::Result<()> {
    let mut timeouts = 0;
    while !bytes.is_empty() {
        match port.write(bytes) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "port wrote zero")),
            Ok(n) => bytes = bytes.get(n..).unwrap_or(&[]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == io::ErrorKind::TimedOut && timeouts < WRITE_RETRIES => {
                timeouts += 1;
            }
            Err(e) => return Err(e),
        }
    }
    port.flush()
}

struct Supervisor {
    config: SerialConfig,
    commands: std_mpsc::Receiver<Command>,
    to_actor: mpsc::Sender<Vec<u8>>,
    status: watch::Sender<LinkStatus>,
    shared: SharedPort,
    counters: Arc<Counters>,
}

enum SessionEnd {
    Shutdown,
    Lost(String),
}

impl Supervisor {
    // [@ANCHOR: radioctl_core_serial_supervisor_run]
    /// The supervisor thread: first open, then sessions and reconnects until shutdown.
    fn run(self, first: oneshot::Sender<Result<(), RigError>>) {
        let attempts = self.config.first_open_attempts.max(1);
        let mut first = Some(first);
        let mut opened = None;
        let mut last = link_error("no attempt made");
        for attempt in 1..=attempts {
            match open_port(&self.config) {
                Ok(found) => {
                    opened = Some(found);
                    break;
                }
                Err(error) => last = error,
            }
            if attempt < attempts && !self.sleep_dropping(self.config.backoff.delay(attempt)) {
                break;
            }
        }
        let Some((mut port, mut path)) = opened else {
            if let Some(first) = first.take() {
                let _ = first.send(Err(last));
            }
            let _ = self.status.send(LinkStatus::Closed);
            return;
        };
        let mut generation = 1u32;
        loop {
            self.publish(port.as_ref(), &path, generation);
            if let Some(first) = first.take() {
                let _ = first.send(Ok(()));
            }
            let end = self.session(port);
            *locked(&self.shared) = None;
            let SessionEnd::Lost(mut why) = end else {
                break;
            };
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
                match open_port(&self.config) {
                    Ok(found) => break Some(found),
                    Err(error) => why = error.detail,
                }
            };
            match reopened {
                Some((next_port, next_path)) => {
                    port = next_port;
                    path = next_path;
                    generation += 1;
                    self.counters.reconnects.fetch_add(1, Ordering::Relaxed);
                }
                None => break,
            }
        }
        *locked(&self.shared) = None;
        let _ = self.status.send(LinkStatus::Closed);
    }

    fn publish(&self, port: &dyn SerialPort, path: &std::path::Path, generation: u32) {
        *locked(&self.shared) = port.try_clone().ok();
        let _ = self.status.send(LinkStatus::Connected {
            port: path.to_path_buf(),
            generation,
        });
    }

    // [@ANCHOR: radioctl_core_serial_sleep_dropping]
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

    // [@ANCHOR: radioctl_core_serial_session]
    /// One connection: a reader thread plus this thread writing, until shutdown or loss.
    fn session(&self, mut port: Box<dyn SerialPort>) -> SessionEnd {
        let reader_port = match port.try_clone() {
            Ok(clone) => clone,
            Err(e) => return SessionEnd::Lost(format!("clone for reading: {e}")),
        };
        let lost = Arc::new(Mutex::new(None::<String>));
        let stop = Arc::new(AtomicBool::new(false));
        let reader = {
            let (lost, stop) = (Arc::clone(&lost), Arc::clone(&stop));
            let (to_actor, counters) = (self.to_actor.clone(), Arc::clone(&self.counters));
            thread::Builder::new()
                .name("radioctl-serial-rx".into())
                .spawn(move || read_loop(reader_port, to_actor, counters, lost, stop))
        };
        let Ok(reader) = reader else {
            return SessionEnd::Lost("could not start the reader thread".into());
        };
        let end = loop {
            if let Some(why) = locked(&lost).clone() {
                break SessionEnd::Lost(why);
            }
            match self.commands.recv_timeout(SUPERVISOR_TICK) {
                Ok(Command::Write(bytes)) => match write_fully(port.as_mut(), &bytes) {
                    Ok(()) => {
                        self.counters
                            .bytes_out
                            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
                    }
                    Err(e) => break SessionEnd::Lost(format!("write: {e}")),
                },
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    break SessionEnd::Shutdown
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        };
        stop.store(true, Ordering::Release);
        let _ = reader.join();
        end
    }
}

// [@ANCHOR: radioctl_core_serial_read_loop]
/// The reader thread: bytes from the port to the async side until stopped or the port fails.
fn read_loop(
    mut port: Box<dyn SerialPort>,
    to_actor: mpsc::Sender<Vec<u8>>,
    counters: Arc<Counters>,
    lost: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
) {
    let mut buffer = [0u8; 256];
    while !stop.load(Ordering::Acquire) {
        match port.read(&mut buffer) {
            Ok(0) => {
                *locked(&lost) = Some("port closed".into());
                return;
            }
            Ok(n) => {
                counters.bytes_in.fetch_add(n as u64, Ordering::Relaxed);
                let chunk = buffer.get(..n).unwrap_or(&[]).to_vec();
                if to_actor.blocking_send(chunk).is_err() {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
                ) => {}
            Err(e) => {
                *locked(&lost) = Some(format!("read: {e}"));
                return;
            }
        }
    }
}

// [@ANCHOR: radioctl_core_serial_open]
/// Opens the port and starts its threads and bridge tasks on `runtime`.
///
/// Returns once the first open has succeeded, or with the error of the last attempt. The returned
/// stream goes to [`crate::rig::spawn`]; dropping it (with the rig) closes the port.
pub async fn open_serial(runtime: &Handle, config: SerialConfig) -> Result<SerialLink, RigError> {
    let (near, far) = duplex(STREAM_BYTES);
    let (mut far_reader, mut far_writer) = tokio::io::split(far);
    let (command_tx, command_rx) = std_mpsc::sync_channel::<Command>(WRITE_QUEUE);
    let (to_actor_tx, mut to_actor_rx) = mpsc::channel::<Vec<u8>>(READ_QUEUE);
    let (status_tx, status_rx) = watch::channel(LinkStatus::Reconnecting {
        attempt: 0,
        last_error: "opening".into(),
    });
    let shared: SharedPort = Arc::new(Mutex::new(None));
    let counters = Arc::new(Counters::default());
    let (first_tx, first_rx) = oneshot::channel();
    let lines = config.lines;
    let supervisor = Supervisor {
        config,
        commands: command_rx,
        to_actor: to_actor_tx,
        status: status_tx,
        shared: Arc::clone(&shared),
        counters: Arc::clone(&counters),
    };
    thread::Builder::new()
        .name("radioctl-serial".into())
        .spawn(move || supervisor.run(first_tx))
        .map_err(|e| link_error(format!("could not start the serial thread: {e}")))?;
    let uplink = command_tx.clone();
    runtime.spawn(async move {
        let mut buffer = [0u8; 256];
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
        Ok(Ok(())) => Ok(SerialLink {
            transport: near,
            control: SerialControl {
                port: shared,
                lines,
            },
            status: status_rx,
            counters,
        }),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(link_error("serial thread ended before opening the port")),
    }
}
