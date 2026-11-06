// SPDX-License-Identifier: AGPL-3.0-or-later
//! A rig on a `radiod` transport, started through `daemon::start` as a program that embeds radioctld
//! starts it, keeps the channel's control (`Served::radiod`) so that program can take the channel's
//! audio. No `radiod` is needed: the bridge's sockets open on loopback multicast, and the channel would
//! be created when a `radiod` answered.

use std::path::Path;

use radioctl_core::rig::RigOptions;
use radioctld::config::Config;
use radioctld::daemon;
use radioctld::rigs::radiod_channel_description;
use tokio::runtime::Handle;

// Tests [@ANCHOR: radioctld_daemon_start_one]
// Tests [@ANCHOR: radioctld_rigs_radiod_channel_description]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_radiod_rig_keeps_its_channel_control_for_the_embedding_program() {
    let port = 31_000 + (std::process::id() % 8_000) as u16 * 2;
    let text = format!(
        "[[rig]]\nname = \"rx888\"\ndescription = \"builtin\"\nlisten = \"127.0.0.1:0\"\ntransmit = false\n\
         [rig.transport]\nkind = \"radiod\"\nstatus = \"239.77.1.1\"\nstatus_port = {port}\n\
         data = \"239.77.1.2\"\ndata_port = {}\nssrc = 7001\n",
        port + 1
    );
    let config = Config::parse(&text, Path::new(".")).expect("a radiod rig configuration");
    let description = radiod_channel_description().expect("the built-in description");
    let daemon = daemon::start(
        &Handle::current(),
        &config,
        vec![description],
        RigOptions::default,
    )
    .await
    .expect("the daemon starts with no radiod answering");
    let served = &daemon.rigs[0];
    let control = served
        .radiod
        .as_ref()
        .expect("a radiod rig keeps its control");
    assert!(
        control.take_audio().is_some(),
        "the audio queue is there to take"
    );
    assert!(control.take_audio().is_none(), "once");
    assert!(served.virtual_control.is_none() && served.net_link.is_none());
    daemon.stop().await;
}
