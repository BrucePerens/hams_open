// SPDX-License-Identifier: AGPL-3.0-or-later
//! The optional baud-rate probe: finds the rate a serial radio answers at by asking it, at each
//! candidate rate in turn, the one question that changes nothing.
//!
//! Nothing probes unless the owner configures it (radioctld: `baud = "auto"` or `baud_probe =
//! [...]`; the default stays a fixed rate). The policy lives behind [`ProbePort`] so it is tested
//! without a port: for each rate in order the port is opened at that rate, the engine's own
//! encoding of `Op::GetFrequency(Vfo::A)` is written (CI-V command `03`; never a hand-written
//! frame, and the probe refuses to run if the engine's answer is not a read), the probe waits one
//! description timeout for an `Inbound::Observation`, and the port is closed. The first rate that
//! answers wins and the probe stops; no further rate is opened and no second request is sent.
//! When no rate answers the result is `LinkDown` naming every rate tried.
//!
//! The probe never touches the transmit gate and never keys anything: the only bytes it can put
//! on the wire are a frequency read. It runs before the rig actor exists, so the actor's start-up
//! unkey pass still runs, at the chosen rate.

use std::future::Future;
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Handle;

use crate::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use crate::error::{ErrorKind, RigError};
use crate::op::{Field, Op};
use crate::serial::{open_serial, LinkStatus, SerialConfig, SerialMonitor};
use crate::transport::Transport;
use crate::units::Vfo;

/// The rates `baud = "auto"` tries, in this order: the CI-V rates Icom radios offer, slowest first.
pub const DEFAULT_RATES: [u32; 6] = [4800, 9600, 19200, 38400, 57600, 115200];

/// How long the serial probe port waits for a closed link's supervisor to release the device
/// before opening it again at the next rate (the port is held exclusively).
const CLOSE_WAIT: Duration = Duration::from_secs(2);

/// A port the probe can open at a chosen rate. The in-memory fake of the tests and the serial
/// port of [`SerialProbePort`] both implement it.
pub trait ProbePort: Send {
    /// Opens the port at `baud` and returns its byte stream. An error here is not a baud problem
    /// (the device is missing or not permitted): it ends the probe.
    fn open(
        &mut self,
        baud: u32,
    ) -> impl Future<Output = Result<Box<dyn Transport>, RigError>> + Send;

    /// Called after the stream from the last `open` was dropped: waits until the device is
    /// released, so the next `open` (or the real link afterwards) does not find it busy.
    fn close(&mut self) -> impl Future<Output = ()> + Send;
}

/// What the probe found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    /// The rate the radio answered at.
    pub baud: u32,
    /// Every rate opened, in order, the winner last.
    pub tried: Vec<u32>,
}

// [@ANCHOR: radioctl_core_baud_probe_read_request]
/// The engine's frequency read, checked to be a read: the reply it expects must be the VFO A
/// frequency observation. Anything else (an acknowledgement, no reply) is refused, so the probe
/// can never send a command that changes the radio.
pub fn read_request<E: ProtocolEngine>(engine: &mut E) -> Result<Request, RigError> {
    let request = engine.encode(&Op::GetFrequency(Vfo::A))?;
    if request.reply != ReplyKind::Observation(Field::Frequency(Vfo::A)) {
        return Err(RigError::new(
            ErrorKind::Unsupported,
            format!(
                "the engine's frequency read expects {:?}, not a frequency observation: not probing",
                request.reply
            ),
        ));
    }
    Ok(request)
}

// [@ANCHOR: radioctl_core_baud_probe_wait_for_observation]
/// Reads `transport` for at most `timeout`, feeding the engine; true as soon as it decodes an
/// observation. An end of stream, a read error or the deadline is false.
async fn wait_for_observation<E: ProtocolEngine>(
    transport: &mut Box<dyn Transport>,
    engine: &mut E,
    timeout: Duration,
    epoch: Instant,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut buffer = [0u8; 256];
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return false;
        }
        let read = tokio::time::timeout(left, transport.read(&mut buffer)).await;
        let Ok(Ok(n)) = read else {
            return false;
        };
        if n == 0 {
            return false;
        }
        let inbound = engine.feed(buffer.get(..n).unwrap_or(&[]), epoch.elapsed());
        if inbound
            .iter()
            .any(|event| matches!(event, Inbound::Observation(_)))
        {
            return true;
        }
    }
}

// [@ANCHOR: radioctl_core_baud_probe_probe]
/// Tries `rates` in order on `port` with `engine`'s own frequency read, one request per rate and
/// one description timeout each, and stops at the first rate that answers. `Err(LinkDown)` names
/// every rate when none answers; an open error ends the probe with that error.
pub async fn probe<P: ProbePort, E: ProtocolEngine>(
    port: &mut P,
    engine: &mut E,
    rates: &[u32],
) -> Result<ProbeResult, RigError> {
    if rates.is_empty() {
        return Err(RigError::new(
            ErrorKind::Unsupported,
            "the baud probe needs at least one rate",
        ));
    }
    let request = read_request(engine)?;
    let timeout = engine.description().timing.timeout;
    let epoch = Instant::now();
    let mut tried = Vec::with_capacity(rates.len());
    for &baud in rates {
        tried.push(baud);
        let mut transport = port.open(baud).await?;
        let written = transport.write_all(&request.bytes).await;
        let answered = match written {
            Ok(()) => wait_for_observation(&mut transport, engine, timeout, epoch).await,
            Err(_) => false,
        };
        drop(transport);
        port.close().await;
        if answered {
            return Ok(ProbeResult { baud, tried });
        }
    }
    Err(RigError::new(
        ErrorKind::LinkDown,
        format!(
            "no answer to the frequency read at any of the rates tried: {}",
            rates_text(&tried)
        ),
    ))
}

// [@ANCHOR: radioctl_core_baud_probe_rates_text]
/// The rates as text for a log line or an error: `4800, 9600, 19200`.
pub fn rates_text(rates: &[u32]) -> String {
    rates
        .iter()
        .map(|rate| rate.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A real serial port as a [`ProbePort`]: each `open` is [`open_serial`] with the configured
/// port at the given rate (lines managed or not as configured, so DTR and RTS are made inactive
/// after every open exactly as for the real link), and `close` waits for the supervisor to
/// release the device.
pub struct SerialProbePort {
    runtime: Handle,
    config: SerialConfig,
    monitor: Option<SerialMonitor>,
}

impl SerialProbePort {
    // [@ANCHOR: radioctl_core_baud_probe_serial_port_new]
    /// A probe port for `config`'s device; `config.baud` is replaced at every open.
    pub fn new(runtime: Handle, config: SerialConfig) -> SerialProbePort {
        SerialProbePort {
            runtime,
            config,
            monitor: None,
        }
    }
}

impl ProbePort for SerialProbePort {
    // [@ANCHOR: radioctl_core_baud_probe_serial_port_open]
    /// Opens the configured device at `baud` with one open attempt.
    async fn open(&mut self, baud: u32) -> Result<Box<dyn Transport>, RigError> {
        self.close().await;
        let mut config = self.config.clone();
        config.baud = baud;
        config.first_open_attempts = 1;
        let link = open_serial(&self.runtime, config).await?;
        let (transport, monitor) = link.into_parts();
        self.monitor = Some(monitor);
        Ok(Box::new(transport))
    }

    // [@ANCHOR: radioctl_core_baud_probe_serial_port_close]
    /// Waits (bounded) until the last link's supervisor reports `Closed`, which is when the
    /// device is released.
    async fn close(&mut self) {
        let Some(monitor) = self.monitor.take() else {
            return;
        };
        let mut status = monitor.watch();
        let _ = tokio::time::timeout(
            CLOSE_WAIT,
            status.wait_for(|status| matches!(status, LinkStatus::Closed)),
        )
        .await;
    }
}
