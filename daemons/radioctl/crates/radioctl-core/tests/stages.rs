// SPDX-License-Identifier: AGPL-3.0-or-later
//! Timed multi-stage sequences (`ProtocolEngine::encode_stages`): "write A, wait N ms, write B,
//! wait M ms, write C", with the transmit gate checked before the first key-on stage and an
//! unkey that always runs to the end. Time is paused, so the waits are measured exactly.
#![allow(clippy::expect_used, clippy::panic)]

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use radioctl_core::capability::{Capability, CapabilitySet};
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine, ReplyKind, Request, Stage};
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::op::Op;
use radioctl_core::poll::PollConfig;
use radioctl_core::rig::{spawn_current, RigOptions, TransmitPermit};
use radioctl_core::units::{Hz, Vfo};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Instant;

type Log = Arc<Mutex<Vec<(Duration, Vec<u8>)>>>;

/// Records every write with its time; the write numbered `fail_at` (0-based) fails.
#[derive(Clone)]
struct Wire {
    log: Log,
    start: Instant,
    fail_at: Option<usize>,
}

impl AsyncRead for Wire {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for Wire {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut log = self.log.lock().expect("log");
        let n = log.len();
        log.push((self.start.elapsed(), buf.to_vec()));
        if self.fail_at == Some(n) {
            return Poll::Ready(Err(io::Error::other("write failed")));
        }
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

/// Key-on is A, 50 ms, B, 75 ms, C; unkey is X, 20 ms, Y, 20 ms, Z. SetFrequency is a plain request.
struct Seq {
    desc: RigDescription,
}

impl ProtocolEngine for Seq {
    fn description(&self) -> &RigDescription {
        &self.desc
    }
    fn encode(&mut self, op: &Op) -> Result<Request, RigError> {
        match op {
            Op::SetFrequency(..) => Ok(Request {
                bytes: b"F".to_vec(),
                reply: ReplyKind::None,
            }),
            _ => Err(RigError::new(ErrorKind::Unsupported, "no")),
        }
    }
    fn encode_stages(&mut self, op: &Op) -> Result<Option<Vec<Stage>>, RigError> {
        let st = |b: &[u8], ms| Stage {
            bytes: b.to_vec(),
            wait: Duration::from_millis(ms),
        };
        Ok(match op {
            Op::SetPtt(true) => Some(vec![st(b"A", 50), st(b"B", 75), st(b"C", 0)]),
            Op::SetPtt(false) => Some(vec![st(b"X", 20), st(b"Y", 20), st(b"Z", 0)]),
            _ => None,
        })
    }
    fn feed(&mut self, _: &[u8], _: Duration) -> Vec<Inbound> {
        Vec::new()
    }
}

fn start(fail_at: Option<usize>) -> (radioctl_core::rig::RigHandle, Wire) {
    let mut desc = RigDescription::ic7300();
    desc.capabilities = CapabilitySet::of(&[Capability::SetPtt, Capability::SetFrequency]);
    let wire = Wire {
        log: Arc::default(),
        start: Instant::now(),
        fail_at,
    };
    let options = RigOptions {
        poll: PollConfig {
            enabled: false,
            ..PollConfig::default()
        },
        ..RigOptions::default()
    };
    let handle = spawn_current(
        Box::new(Seq { desc }) as Box<dyn ProtocolEngine>,
        wire.clone(),
        options,
    )
    .expect("spawn");
    (handle, wire)
}

fn writes(wire: &Wire) -> Vec<(u128, Vec<u8>)> {
    wire.log
        .lock()
        .expect("log")
        .iter()
        .map(|(t, b)| (t.as_millis(), b.clone()))
        .collect()
}

// Tests [@ANCHOR: radioctl_core_actor_run_stages]
// Tests [@ANCHOR: radioctl_core_actor_write_stages]
// Tests [@ANCHOR: radioctl_core_engine_box_encode_stages]
// Tests [@ANCHOR: radioctl_core_engine_stage]
#[tokio::test(start_paused = true)]
async fn the_stages_are_written_in_order_with_the_waits_between_them() {
    let (rig, wire) = start(None);
    rig.set_frequency(Vfo::A, Hz(145_000_000))
        .await
        .expect("freq");
    rig.open_transmit_gate(TransmitPermit::new("stages test"));
    let t0 = wire.start.elapsed().as_millis();
    rig.set_ptt(true).await.expect("key on");
    let w = writes(&wire);
    assert_eq!(
        w[1..].iter().map(|x| x.1.clone()).collect::<Vec<_>>(),
        [b"A", b"B", b"C"]
    );
    assert_eq!(w[2].0 - w[1].0, 50);
    assert_eq!(w[3].0 - w[2].0, 75);
    assert!(w[1].0 >= t0);
    rig.set_ptt(false).await.expect("unkey");
    let w = writes(&wire);
    assert_eq!(w[5].0 - w[4].0, 20);
    assert_eq!(w[6].0 - w[5].0, 20);
}

// Tests [@ANCHOR: radioctl_core_actor_run_command]
#[tokio::test(start_paused = true)]
async fn the_gate_is_checked_before_the_first_key_on_stage_and_unkey_needs_no_gate() {
    let (rig, wire) = start(None);
    rig.set_frequency(Vfo::A, Hz(145_000_000))
        .await
        .expect("freq");
    let denied = rig.set_ptt(true).await.expect_err("gate closed");
    assert_eq!(denied.kind, ErrorKind::TransmitGateClosed);
    assert_eq!(writes(&wire).len(), 1, "only the frequency write");
    rig.set_ptt(false).await.expect("unkey without the gate");
    assert_eq!(writes(&wire).len(), 4);
}

#[tokio::test(start_paused = true)]
async fn a_failed_key_on_stage_stops_the_sequence_and_unkeys() {
    // Writes: 0 frequency, 1 A, 2 B fails.
    let (rig, wire) = start(Some(2));
    rig.set_frequency(Vfo::A, Hz(145_000_000))
        .await
        .expect("freq");
    rig.open_transmit_gate(TransmitPermit::new("stages test"));
    let err = rig.set_ptt(true).await.expect_err("stage failed");
    assert_eq!(err.kind, ErrorKind::LinkDown);
    let sent: Vec<_> = writes(&wire).into_iter().map(|x| x.1).collect();
    assert_eq!(sent[..3], [b"F".to_vec(), b"A".to_vec(), b"B".to_vec()]);
    assert!(
        !sent.contains(&b"C".to_vec()),
        "C is never sent after B failed"
    );
}

#[tokio::test(start_paused = true)]
async fn an_unkey_runs_every_stage_even_when_one_fails() {
    // Writes: 0 frequency, 1 X fails; Y and Z must still go out, with their waits.
    let (rig, wire) = start(Some(1));
    let err = rig.set_ptt(false).await.expect_err("first stage failed");
    assert_eq!(err.kind, ErrorKind::LinkDown);
    let w = writes(&wire);
    let sent: Vec<_> = w.iter().map(|x| x.1.clone()).collect();
    assert_eq!(sent, vec![b"X".to_vec(), b"Y".to_vec(), b"Z".to_vec()]);
    assert_eq!(w[1].0 - w[0].0, 20);
}
