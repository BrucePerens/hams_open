// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one-call constructors of `radioctl_protocols::prelude`: a virtual rig of a named model, and
//! a serial rig on a pseudo-terminal that a virtual radio answers. No hardware.
#![allow(clippy::expect_used, clippy::panic)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use radioctl_core::poll::PollConfig;
use radioctl_core::serial::{LineControl, PortTarget, SerialConfig};
use radioctl_protocols::prelude::*;
use serialport::{SerialPort, TTYPort};
use tokio::runtime::Handle;

fn quiet() -> RigOptions {
    RigOptions {
        poll: PollConfig {
            enabled: false,
            ..PollConfig::default()
        },
        ..RigOptions::default()
    }
}

// Tests [@ANCHOR: radioctl_easy_virtual_rig_spawn]
#[tokio::test(start_paused = true)]
async fn a_named_virtual_rig_is_one_call_and_its_gate_is_closed() {
    let up = VirtualRig::spawn("elecraft-kx3").expect("spawn");
    let frequency = up
        .rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("read");
    assert_eq!(frequency.value, Hz(14_060_000));
    up.rig.set_mode(Mode::Usb).await.expect("mode");
    assert_eq!(up.control.state().mode, Mode::Usb);
    assert_eq!(up.rig.transmit_permit(), None);
    assert_eq!(
        up.rig.set_ptt(true).await.err().map(|e| e.kind),
        Some(ErrorKind::TransmitGateClosed)
    );
}

// Tests [@ANCHOR: radioctl_easy_virtual_rig_spawn]
#[tokio::test(start_paused = true)]
async fn every_family_with_a_virtual_stand_in_spawns_by_id() {
    for id in [
        "icom-ic-7300",
        "kenwood-ts-2000",
        "yaesu-ft-991a",
        "elecraft-kx3",
    ] {
        let up = VirtualRig::spawn(id).unwrap_or_else(|e| panic!("{id}: {e}"));
        up.rig
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .unwrap_or_else(|e| panic!("{id}: {e}"));
    }
}

// Tests [@ANCHOR: radioctl_easy_virtual_rig_spawn]
#[tokio::test(start_paused = true)]
async fn an_unknown_rig_is_an_error_naming_the_file() {
    let miss = VirtualRig::spawn("no-such-rig").err().expect("an error");
    assert!(miss.detail.contains("no-such-rig.toml"), "{}", miss.detail);
}

// Tests [@ANCHOR: radioctl_easy_virtual_rig_spawn]
#[test]
fn spawning_outside_a_runtime_is_an_error_not_a_panic() {
    let miss = VirtualRig::spawn("elecraft-kx3").err().expect("an error");
    assert_eq!(miss.kind, ErrorKind::Unsupported);
}

// Tests [@ANCHOR: radioctl_easy_spawn_virtual]
#[tokio::test(start_paused = true)]
async fn spawn_virtual_takes_the_options_and_the_reply_delay() {
    let description = rigfile::load("icom-ic-7300").expect("file").description;
    let up = spawn_virtual(
        &Handle::current(),
        description,
        quiet(),
        Duration::from_millis(300),
    )
    .expect("spawn");
    let started = tokio::time::Instant::now();
    up.rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("read");
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(up.rig.stats().polls, 0, "the options turned polling off");
}

/// A pty whose master end plays the radio: a virtual rig of `description` answers on it, or a
/// silent one when `silent`. Returns the slave path and a flag that stops the radio thread.
fn radio_on_a_pty(
    description: &RigDescription,
    silent: bool,
) -> (PathBuf, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let (mut master, slave) = TTYPort::pair().expect("pty pair");
    let path = PathBuf::from(slave.name().expect("slave name"));
    let mut radio = VirtualRig::from_description(description).expect("virtual rig");
    let done = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&done);
    let thread = std::thread::spawn(move || {
        // The slave end stays open until the thread ends, so the port exists for the whole test.
        let _slave = slave;
        master
            .set_timeout(Duration::from_millis(50))
            .expect("timeout");
        let mut buffer = [0u8; 128];
        let started = std::time::Instant::now();
        while !flag.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(20) {
            if let Ok(n) = master.read(&mut buffer) {
                let reply = radio.feed(buffer.get(..n).unwrap_or(&[]), started.elapsed());
                if !silent && !reply.is_empty() {
                    master.write_all(&reply).expect("reply");
                }
            }
        }
    });
    (path, done, thread)
}

fn pty_config(path: &std::path::Path) -> SerialConfig {
    let mut config = SerialConfig::new(PortTarget::Path(path.to_path_buf()), 38_400);
    config.lines = LineControl::Unmanaged;
    config
}

// Tests [@ANCHOR: radioctl_easy_open_serial_rig_with]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_serial_rig_is_unkeyed_first_reads_the_radio_and_has_a_closed_gate() {
    let description = rigfile::load("elecraft-kx3").expect("file").description;
    let (path, done, radio) = radio_on_a_pty(&description, false);
    let up = open_serial_rig_with(&Handle::current(), &description, pty_config(&path), quiet())
        .await
        .expect("open");
    let frequency = up
        .rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect("read");
    assert_eq!(frequency.value, Hz(14_060_000));
    assert_eq!(up.rig.transmit_permit(), None);
    assert!(up.rig.link_up());
    assert!(
        up.link.stats().bytes_out > 0,
        "the unkey and the read went out"
    );
    drop(up);
    done.store(true, Ordering::Release);
    radio.join().expect("radio thread");
}

// Tests [@ANCHOR: radioctl_easy_open_serial_rig_with]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_radio_that_cannot_be_unkeyed_is_not_handed_over() {
    let description = rigfile::load("icom-ic-7300").expect("file").description;
    let (path, done, radio) = radio_on_a_pty(&description, true);
    let mut options = quiet();
    options.default_timeout = Duration::from_millis(200);
    let miss = open_serial_rig_with(&Handle::current(), &description, pty_config(&path), options)
        .await
        .err()
        .expect("an error");
    assert!(miss.detail.contains("may be keyed"), "{}", miss.detail);
    done.store(true, Ordering::Release);
    radio.join().expect("radio thread");
}

// Tests [@ANCHOR: radioctl_easy_open_serial_rig]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn open_serial_rig_names_the_description_or_the_port_it_could_not_open() {
    let miss = open_serial_rig("no-such-rig", "/dev/null", 9600)
        .await
        .err()
        .expect("an error");
    assert!(miss.detail.contains("no-such-rig.toml"), "{}", miss.detail);
    let miss = open_serial_rig("elecraft-kx3", "/nonexistent/ttyUSB9", 38_400)
        .await
        .err()
        .expect("an error");
    assert!(
        miss.detail.contains("/nonexistent/ttyUSB9"),
        "{}",
        miss.detail
    );
}
