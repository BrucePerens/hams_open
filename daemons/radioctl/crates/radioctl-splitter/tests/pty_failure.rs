// SPDX-License-Identifier: AGPL-3.0-or-later
//! A pty attach that fails after making its symlink must not leave the symlink behind. The
//! failure is forced by lowering the process limit, so this test lives alone in its binary.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::time::Duration;

use radioctl_core::op::Op;
use radioctl_core::rig::{spawn, RigOptions};
use radioctl_core::rigfile;
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctl_splitter::pty::attach_pty;
use radioctl_splitter::session::{Protocol, Splitter, SplitterConfig};
use tokio::runtime::Handle;

// Tests [@ANCHOR: radioctl_splitter_pty_attach]
// Tests [@ANCHOR: radioctl_splitter_pty_drop]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_attach_removes_the_symlink_it_made() {
    let desc = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))
        .expect("kx3 file")
        .description;
    let radio = VirtualRig::from_description(&desc).expect("virtual rig");
    let (stream, _control) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);
    let engine = ElecraftEngine::new(desc).expect("engine");
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    let rig = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    rig.execute(Op::SetTransceive(true)).await.expect("AI2");
    let splitter = Splitter::new(
        rig.clone(),
        SplitterConfig::new(Protocol::Civ {
            address: 0x94,
            echo: false,
        }),
    );
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("ptyfail-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let link = dir.join("port");
    let runtime = Handle::current();

    // With this process's soft thread limit at 1 no new thread can start, so the attach fails
    // after its pty and link exist. The limit stays for the rest of this one-test binary.
    let status = std::process::Command::new("prlimit")
        .arg(format!("--pid={}", std::process::id()))
        .arg("--nproc=1:")
        .status()
        .expect("prlimit");
    assert!(status.success());
    let outcome = attach_pty(&splitter, &runtime, Some(link.clone()));
    assert!(
        outcome.is_err(),
        "the attach must fail when no thread can start"
    );
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the failed attach left its symlink behind"
    );
}
