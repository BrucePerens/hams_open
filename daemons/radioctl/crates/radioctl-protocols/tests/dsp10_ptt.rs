// SPDX-License-Identifier: AGPL-3.0-or-later
//! DSP-10 PTT through the rig actor on the virtual twin: the timed key-on and unkey sequences
//! (UHFA.C t_r_toggle) run only behind the transmit gate. Time is paused, so the waits are exact.
#![allow(clippy::expect_used, clippy::panic)]

use std::io;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use radioctl_core::error::ErrorKind;
use radioctl_core::poll::PollConfig;
use radioctl_core::rig::{spawn_current, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::dsp10::{Dsp10Engine, Dsp10Twin};
use radioctl_protocols::serial_vendor::VendorTwin;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Instant;

/// The twin behind a transport: every write is fed to it.
#[derive(Clone)]
struct TwinWire {
    twin: Arc<Mutex<Dsp10Twin>>,
    bytes: Arc<Mutex<usize>>,
}

impl AsyncRead for TwinWire {
    fn poll_read(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        _: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

impl AsyncWrite for TwinWire {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        *self.bytes.lock().expect("n") += buf.len();
        self.twin.lock().expect("twin").feed(buf, Duration::ZERO);
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

// Tests [@ANCHOR: radioctl_dsp10_ptt_stages]
// Tests [@ANCHOR: radioctl_dsp10_shift_stages]
// Tests [@ANCHOR: radioctl_dsp10_shift_bytes]
// Tests [@ANCHOR: radioctl_dsp10_engine_encode_stages]
#[tokio::test(start_paused = true)]
async fn ptt_is_gated_staged_and_keys_the_virtual_dsp() {
    let path = format!("{}/../../rigs/tapr-dsp-10.toml", env!("CARGO_MANIFEST_DIR"));
    let desc = rigfile::parse(&std::fs::read_to_string(path).expect("rig file"))
        .expect("parses")
        .description;
    let wire = TwinWire {
        twin: Arc::new(Mutex::new(Dsp10Twin::new(&desc).expect("twin"))),
        bytes: Arc::default(),
    };
    let snap = |w: &TwinWire| w.twin.lock().expect("twin").snapshot().expect("snapshot");
    let options = RigOptions {
        poll: PollConfig {
            enabled: false,
            ..PollConfig::default()
        },
        ..RigOptions::default()
    };
    let handle = spawn_current(
        Dsp10Engine::new(desc).expect("engine"),
        wire.clone(),
        options,
    )
    .expect("spawn");
    handle
        .set_frequency(Vfo::A, Hz(145_000_000))
        .await
        .expect("frequency");
    let before = *wire.bytes.lock().expect("n");

    // Gate closed: nothing reaches the radio.
    let denied = handle.set_ptt(true).await.expect_err("gate closed");
    assert_eq!(denied.kind, ErrorKind::TransmitGateClosed);
    assert_eq!(*wire.bytes.lock().expect("n"), before);
    assert!(!snap(&wire).ptt);

    handle.open_transmit_gate(TransmitPermit::new("dsp10 test"));
    let start = Instant::now();
    handle.set_ptt(true).await.expect("key on");
    // Waits: 10+50, 10+75, 10+0, then 10 after the Command 84.
    assert_eq!(start.elapsed(), Duration::from_millis(60 + 85 + 10 + 10));
    assert!(snap(&wire).ptt, "relay, amplifier and DSP are in transmit");

    let start = Instant::now();
    handle.set_ptt(false).await.expect("unkey");
    assert_eq!(start.elapsed(), Duration::from_millis(35 + 35 + 12 + 10));
    assert!(!snap(&wire).ptt);
    // The unkey retuned to the receive frequency.
    assert_eq!(snap(&wire).frequency, Hz(145_000_000));

    // Unkeying needs no gate.
    handle.close_transmit_gate();
    handle.set_ptt(false).await.expect("unkey without the gate");
}
