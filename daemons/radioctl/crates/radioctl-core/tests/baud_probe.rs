// SPDX-License-Identifier: AGPL-3.0-or-later
//! The baud probe against an in-memory port: rates in order, one read-only request per rate, one
//! description timeout per silent rate, stop at the first answer, nothing but the frequency read
//! on the wire; and the serial probe port against a device that is not there. No hardware.
#![allow(clippy::expect_used, clippy::panic)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use radioctl_core::baud_probe::{
    probe, rates_text, read_request, ProbePort, SerialProbePort, DEFAULT_RATES,
};
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::{Field, Op};
use radioctl_core::serial::{PortTarget, SerialConfig};
use radioctl_core::transport::Transport;
use radioctl_core::units::Vfo;
use radioctl_protocols::civ::CivEngine;
use radioctl_protocols::virtual_rig::VirtualRig;
use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt, DuplexStream};
use tokio::runtime::Handle;

const IC7300_READ: [u8; 6] = [0xFE, 0xFE, 0x94, 0xE0, 0x03, 0xFD];

/// An in-memory port: a virtual IC-7300 that answers at one rate only. Every byte written at
/// any rate is recorded, so a test can prove the wire carried nothing but the read.
struct FakePort {
    answers_at: Option<u32>,
    fails_at: Option<u32>,
    opens: Vec<u32>,
    closes: usize,
    wire: Arc<Mutex<Vec<u8>>>,
    rig: Arc<Mutex<VirtualRig>>,
}

impl FakePort {
    fn new(answers_at: Option<u32>) -> FakePort {
        FakePort {
            answers_at,
            fails_at: None,
            opens: Vec::new(),
            closes: 0,
            wire: Arc::new(Mutex::new(Vec::new())),
            rig: Arc::new(Mutex::new(
                VirtualRig::from_description(&RigDescription::ic7300()).expect("virtual rig"),
            )),
        }
    }

    fn wire(&self) -> Vec<u8> {
        self.wire.lock().expect("wire").clone()
    }
}

impl ProbePort for FakePort {
    async fn open(&mut self, baud: u32) -> Result<Box<dyn Transport>, RigError> {
        self.opens.push(baud);
        if self.fails_at == Some(baud) {
            return Err(RigError::new(ErrorKind::Unsupported, "no such device"));
        }
        let (near, far): (DuplexStream, DuplexStream) = duplex(256);
        let answers = self.answers_at == Some(baud);
        let (wire, rig) = (Arc::clone(&self.wire), Arc::clone(&self.rig));
        tokio::spawn(async move {
            let mut far = far;
            let mut buffer = [0u8; 64];
            // The far end stays open while the probe waits: a silent rate is silence, not
            // a closed stream.
            while let Ok(n) = far.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                let bytes = buffer.get(..n).unwrap_or(&[]);
                wire.lock().expect("wire").extend_from_slice(bytes);
                if answers {
                    let reply = rig.lock().expect("rig").feed(bytes, Duration::ZERO);
                    if far.write_all(&reply).await.is_err() {
                        break;
                    }
                }
            }
        });
        Ok(Box::new(near))
    }

    async fn close(&mut self) {
        self.closes += 1;
    }
}

fn engine() -> CivEngine {
    CivEngine::new(RigDescription::ic7300()).expect("engine")
}

async fn settle() {
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
}

// Tests [@ANCHOR: radioctl_core_baud_probe_probe]
// Tests [@ANCHOR: radioctl_core_baud_probe_read_request]
// Tests [@ANCHOR: radioctl_core_baud_probe_wait_for_observation]
#[tokio::test(start_paused = true)]
async fn the_first_rate_that_answers_wins_after_one_request_and_one_timeout_per_rate() {
    let mut port = FakePort::new(Some(19200));
    let mut engine = engine();
    let timeout = engine.description().timing.timeout;
    let started = tokio::time::Instant::now();
    let found = probe(&mut port, &mut engine, &DEFAULT_RATES)
        .await
        .expect("an answer");
    settle().await;
    assert_eq!(found.baud, 19200);
    assert_eq!(found.tried, vec![4800, 9600, 19200]);
    // Rates are tried in order and the probe stops at the winner: 38400 and up never open.
    assert_eq!(port.opens, vec![4800, 9600, 19200]);
    assert_eq!(
        port.closes, 3,
        "every opened port is closed, the winner too"
    );
    // The two silent rates each cost exactly one description timeout; the answer cost none.
    assert_eq!(started.elapsed(), timeout * 2);
    // One request per rate, and the wire never carried anything but the frequency read.
    assert_eq!(port.wire(), IC7300_READ.repeat(3));
}

// Tests [@ANCHOR: radioctl_core_baud_probe_probe]
// Tests [@ANCHOR: radioctl_core_baud_probe_rates_text]
#[tokio::test(start_paused = true)]
async fn no_answer_at_any_rate_is_link_down_naming_every_rate() {
    let mut port = FakePort::new(None);
    let mut engine = engine();
    let timeout = engine.description().timing.timeout;
    let started = tokio::time::Instant::now();
    let error = probe(&mut port, &mut engine, &[9600, 115200])
        .await
        .expect_err("silence");
    settle().await;
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(error.detail.contains("9600, 115200"), "{error}");
    assert_eq!(port.opens, vec![9600, 115200]);
    assert_eq!(started.elapsed(), timeout * 2);
    assert_eq!(port.wire(), IC7300_READ.repeat(2));
    assert_eq!(rates_text(&[]), "");
    assert_eq!(rates_text(&[4800]), "4800");
}

// Tests [@ANCHOR: radioctl_core_baud_probe_probe]
#[tokio::test(start_paused = true)]
async fn an_open_error_ends_the_probe_with_that_error_not_with_no_answer() {
    let mut port = FakePort::new(Some(38400));
    port.fails_at = Some(9600);
    let error = probe(&mut port, &mut engine(), &[4800, 9600, 38400])
        .await
        .expect_err("the device vanished");
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert_eq!(error.detail, "no such device");
    assert_eq!(port.opens, vec![4800, 9600], "38400 is never reached");
}

// Tests [@ANCHOR: radioctl_core_baud_probe_probe]
#[tokio::test(start_paused = true)]
async fn an_empty_rate_list_is_refused_before_anything_opens() {
    let mut port = FakePort::new(Some(4800));
    let error = probe(&mut port, &mut engine(), &[])
        .await
        .expect_err("nothing to try");
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(port.opens.is_empty());
}

/// An engine whose frequency read is not a read: its request expects an acknowledgement.
struct SetsInstead(RigDescription);

impl ProtocolEngine for SetsInstead {
    fn description(&self) -> &RigDescription {
        &self.0
    }
    fn encode(&mut self, _op: &Op) -> Result<Request, RigError> {
        Ok(Request {
            bytes: vec![0xFE, 0xFE, 0x94, 0xE0, 0x1C, 0x00, 0x01, 0xFD],
            reply: ReplyKind::Ack,
        })
    }
    fn feed(&mut self, _bytes: &[u8], _now: Duration) -> Vec<Inbound> {
        Vec::new()
    }
}

// Tests [@ANCHOR: radioctl_core_baud_probe_read_request]
// Tests [@ANCHOR: radioctl_core_baud_probe_probe]
#[tokio::test(start_paused = true)]
async fn an_engine_whose_frequency_read_is_not_a_read_is_refused_and_nothing_is_sent() {
    let mut port = FakePort::new(Some(4800));
    let mut engine = SetsInstead(RigDescription::ic7300());
    let error = read_request(&mut engine).expect_err("not a read");
    assert_eq!(error.kind, ErrorKind::Unsupported);
    let error = probe(&mut port, &mut engine, &DEFAULT_RATES)
        .await
        .expect_err("not a read");
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert!(port.opens.is_empty(), "the port is never opened");
    assert!(port.wire().is_empty(), "nothing reaches the wire");
}

// Tests [@ANCHOR: radioctl_core_baud_probe_read_request]
#[test]
fn the_request_is_the_engines_own_civ_frequency_read() {
    let request = read_request(&mut engine()).expect("a read");
    assert_eq!(request.bytes, IC7300_READ);
    assert_eq!(
        request.reply,
        ReplyKind::Observation(Field::Frequency(Vfo::A))
    );
}

// Tests [@ANCHOR: radioctl_core_baud_probe_serial_port_new]
// Tests [@ANCHOR: radioctl_core_baud_probe_serial_port_open]
// Tests [@ANCHOR: radioctl_core_baud_probe_serial_port_close]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_serial_probe_port_reports_a_missing_device_and_closes_idempotently() {
    let config = SerialConfig::new(
        PortTarget::Path("/nonexistent/radioctl-baud-probe".into()),
        9600,
    );
    let mut port = SerialProbePort::new(Handle::current(), config);
    port.close().await;
    let error = probe(&mut port, &mut engine(), &[4800, 9600])
        .await
        .expect_err("no device");
    assert_eq!(error.kind, ErrorKind::LinkDown);
    assert!(error.detail.contains("open"), "{error}");
    port.close().await;
}
