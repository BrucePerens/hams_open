// SPDX-License-Identifier: AGPL-3.0-or-later
//! The eight SmartSDR slice descriptions (Hamlib models "SmartSDR Slice A" to "H") through the
//! FlexRadio engine, the real rig actor and the TCP twin: VFO A is slice 0 to 7, and the commands
//! on the wire name that slice. Nothing is keyed and no radio is involved.
#![allow(clippy::expect_used, clippy::panic)]

use std::time::Duration;

use radioctl_core::description::RigDescription;
use radioctl_core::rig::{spawn, Freshness, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::tcp::{connect_tcp, TcpConfig};
use radioctl_core::units::{Hz, Mode, Vfo};
use radioctl_protocols::flex::FlexEngine;
use radioctl_protocols::flex_io::serve_virtual_flex;
use radioctl_protocols::virtual_flex::VirtualFlex;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::time::timeout;

const FILES: [&str; 8] = [
    include_str!("../../../rigs/flex-smartsdr-slice-a.toml"),
    include_str!("../../../rigs/flex-smartsdr-slice-b.toml"),
    include_str!("../../../rigs/flex-smartsdr-slice-c.toml"),
    include_str!("../../../rigs/flex-smartsdr-slice-d.toml"),
    include_str!("../../../rigs/flex-smartsdr-slice-e.toml"),
    include_str!("../../../rigs/flex-smartsdr-slice-f.toml"),
    include_str!("../../../rigs/flex-smartsdr-slice-g.toml"),
    include_str!("../../../rigs/flex-smartsdr-slice-h.toml"),
];

fn description(text: &str) -> RigDescription {
    rigfile::parse(text).expect("flex file").description
}

// Tests [@ANCHOR: radioctl_flex_engine_new]
// Tests [@ANCHOR: radioctl_virtual_flex_new]
// Tests [@ANCHOR: radioctl_virtual_flex_serve]
// Tests [@ANCHOR: radioctl_virtual_flex_command]
// Tests [@ANCHOR: radioctl_flex_engine_encode]
#[tokio::test(flavor = "multi_thread")]
async fn each_slice_description_drives_its_own_slice_receiver() {
    for (index, text) in FILES.iter().enumerate() {
        let desc = description(text);
        assert_eq!(usize::from(desc.flex.as_ref().expect("flex").slice), index);
        let rig = VirtualFlex::from_description(&desc).expect("virtual flex");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let control = serve_virtual_flex(&Handle::current(), listener, rig);
        let stream = connect_tcp(addr, &TcpConfig::default())
            .await
            .expect("connect");
        let mut options = RigOptions::default();
        options.poll.enabled = false;
        options.default_timeout = Duration::from_secs(3);
        let engine = FlexEngine::new(desc).expect("engine");
        let handle = spawn(&Handle::current(), engine, stream, options).expect("spawn");
        let first = timeout(
            Duration::from_secs(5),
            handle.frequency(Vfo::A, Freshness::Fresh),
        )
        .await
        .expect("no hang")
        .expect("frequency");
        assert_eq!(first.value, Hz(14_074_000), "slice {index}");
        handle
            .set_frequency(Vfo::A, Hz(7_100_500))
            .await
            .expect("set");
        handle.set_mode(Mode::Lsb).await.expect("mode");
        let received = control.received();
        assert!(
            received.contains(&format!("slice t {index} 7.100500")),
            "{received:?}"
        );
        assert!(
            received.contains(&format!("slice s {index} mode=lsb")),
            "{received:?}"
        );
        assert_eq!(control.state().frequency, Hz(7_100_500));
    }
}
