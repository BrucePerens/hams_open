// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Easycomm, Rot2Prog and DCU rotator engines over a real pty serial port against the
//! virtual rotator, and the same virtual rotator driven by Hamlib's own `rotctl` as an independent
//! client (skipped where `rotctl` is not installed). No hardware, nothing transmits.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use radioctl_core::serial::{open_serial, LineControl, PortTarget, SerialConfig};
use radioctl_station::error::StationErrorKind;
use radioctl_station::rotator::{Rotator, RotatorDescription};
use radioctl_station::virtual_rotator::VirtualRotator;
use serialport::{SerialPort, TTYPort};
use tokio::runtime::Handle;

fn desc(id: &str) -> RotatorDescription {
    let mut d = RotatorDescription::builtin()
        .into_iter()
        .find(|d| d.id == id)
        .unwrap();
    d.slew_deg_per_s = 400.0;
    d.reply_timeout_ms = 1000;
    d
}

/// A pty whose master side is served by the virtual rotator on a thread.
fn pty_rotator(d: &RotatorDescription) -> (PathBuf, Arc<Mutex<VirtualRotator>>) {
    let (mut master, slave) = TTYPort::pair().expect("pty");
    let path = PathBuf::from(slave.name().expect("name"));
    // Keep the slave end open for the life of the process so a client closing the port
    // does not hang up the master.
    std::mem::forget(slave);
    let rot = Arc::new(Mutex::new(VirtualRotator::new(d.clone())));
    let shared = Arc::clone(&rot);
    master
        .set_timeout(Duration::from_millis(20))
        .expect("timeout");
    let epoch = Instant::now();
    std::thread::spawn(move || {
        let mut buf = [0u8; 64];
        loop {
            match master.read(&mut buf) {
                Ok(n) if n > 0 => {
                    let reply = shared
                        .lock()
                        .unwrap()
                        .feed(buf.get(..n).unwrap_or(&[]), epoch.elapsed());
                    if !reply.is_empty() {
                        let _ = master.write_all(&reply);
                        let _ = master.flush();
                    }
                }
                _ => std::thread::sleep(Duration::from_millis(2)),
            }
        }
    });
    (path, rot)
}

async fn open(path: PathBuf, d: &RotatorDescription) -> Rotator {
    let mut config = SerialConfig::new(PortTarget::Path(path), 9600);
    config.lines = LineControl::Unmanaged;
    let link = open_serial(&Handle::current(), config).await.expect("open");
    Rotator::new(d.engine().unwrap(), link.transport, None)
}

fn now(
    rot: &Arc<Mutex<VirtualRotator>>,
    since: Instant,
) -> radioctl_station::virtual_rotator::RotatorState {
    rot.lock().unwrap().state(since.elapsed())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_new_engines_move_read_park_and_refuse_over_a_pty() {
    for id in ["easycomm2-azel", "easycomm3-satnogs", "spid-rot2prog"] {
        let d = desc(id);
        let (path, virt) = pty_rotator(&d);
        let rot = open(path, &d).await;
        rot.set_position(123.4, 45.6).await.unwrap();
        rot.wait_for(123.4, 45.6, 0.2, Duration::from_secs(5))
            .await
            .unwrap_or_else(|e| panic!("{id}: {}", e.detail));
        let (az, el) = rot.position().await.unwrap();
        assert!(
            (az - 123.4).abs() < 0.11 && (el - 45.6).abs() < 0.11,
            "{id} {az} {el}"
        );
        rot.park().await.unwrap();
        rot.wait_for(0.0, 0.0, 0.2, Duration::from_secs(5))
            .await
            .unwrap();
        // Mid-slew stop freezes it.
        rot.set_position(300.0, 10.0).await.unwrap();
        rot.stop().await.unwrap();
        let (a1, _) = rot.position().await.unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(rot.position().await.unwrap().0, a1, "{id} frozen");
        std::thread::sleep(Duration::from_millis(150));
        let before = virt.lock().unwrap().received().len();
        let e = rot.set_position(361.0, 0.0).await.unwrap_err();
        assert_eq!(e.kind, StationErrorKind::Refused);
        assert_eq!(
            virt.lock().unwrap().received().len(),
            before,
            "nothing written"
        );
    }
}

/// The sheet shows ASCII digits in a reply, Hamlib reads plain 0 to 9: both are understood.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rot2prog_reads_ascii_and_plain_digit_replies() {
    for raw in [true, false] {
        let d = desc("spid-rot2prog");
        let (path, virt) = pty_rotator(&d);
        virt.lock().unwrap().raw_digits = raw;
        let rot = open(path, &d).await;
        rot.set_position(215.7, 33.3).await.unwrap();
        rot.wait_for(215.7, 33.3, 0.05, Duration::from_secs(5))
            .await
            .unwrap();
        let (az, el) = rot.position().await.unwrap();
        assert!(
            (az - 215.7).abs() < 0.01 && (el - 33.3).abs() < 0.01,
            "{raw}: {az} {el}"
        );
        let log = virt.lock().unwrap().received().to_vec();
        // 0x57, "5757" 0x0A "3933" 0x0A, 0x2F, 0x20 : (360 + 215.7) * 10 and (360 + 33.3) * 10
        assert_eq!(log[0], "57 35 37 35 37 0A 33 39 33 33 0A 2F 20");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn easycomm_one_sets_but_cannot_read() {
    let d = desc("easycomm1-azel");
    let (path, virt) = pty_rotator(&d);
    let rot = open(path, &d).await;
    rot.set_position(90.0, 30.0).await.unwrap();
    rot.stop().await.unwrap();
    let e = rot.position().await.unwrap_err();
    assert_eq!(e.kind, StationErrorKind::Refused);
    std::thread::sleep(Duration::from_millis(150));
    let log = virt.lock().unwrap().received().to_vec();
    assert_eq!(log[0], "AZ90.0 EL30.0 UP000 XXX DN000 XXX");
    assert_eq!(log[1], "SA SE");
}

/// Hamlib's own `rotctl` as the client: `rotctl -m MODEL -r PTY -s 9600 ARGS...`.
fn hamlib(model: &str, path: &PathBuf, args: &[&str]) -> Option<String> {
    let out = Command::new("timeout")
        .args(["20", "rotctl", "-m", model, "-r"])
        .arg(path)
        .args(["-s", "9600"])
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() && out.status.code() == Some(127) {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dcu_sets_a_bearing_stops_and_cannot_be_read() {
    let d = desc("hygain-dcu3");
    let (path, virt) = pty_rotator(&d);
    let rot = open(path, &d).await;
    rot.set_position(270.0, 0.0).await.unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(virt.lock().unwrap().received().to_vec(), ["AP1270;", "AM1;"]);
    assert_eq!(virt.lock().unwrap().state(Duration::ZERO).target_az, 270.0);
    rot.stop().await.unwrap();
    assert_eq!(
        rot.position().await.unwrap_err().kind,
        StationErrorKind::Refused
    );
    assert_eq!(
        rot.set_position(361.0, 0.0).await.unwrap_err().kind,
        StationErrorKind::Refused
    );
}

#[test]
fn hamlibs_rotctl_drives_the_virtual_dcu() {
    if Command::new("rotctl").arg("--version").output().is_err() {
        eprintln!("rotctl not installed: oracle skipped");
        return;
    }
    let d = desc("hygain-dcu3");
    let (path, virt) = pty_rotator(&d);
    hamlib("406", &path, &["P", "123", "0"]).expect("rotctl runs");
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(virt.lock().unwrap().state(Duration::ZERO).target_az, 123.0);
    hamlib("406", &path, &["S"]).expect("rotctl runs");
    std::thread::sleep(Duration::from_millis(200));
    let log = virt.lock().unwrap().received().to_vec();
    assert_eq!(log, ["AP1123;", "AM1;", "AS1;"]);
}

#[test]
fn hamlibs_rotctl_drives_the_virtual_easycomm_rotators() {
    if Command::new("rotctl").arg("--version").output().is_err() {
        eprintln!("rotctl not installed: oracle skipped");
        return;
    }
    for (id, model) in [
        ("easycomm1-azel", "201"),
        ("easycomm2-azel", "202"),
        ("easycomm3-satnogs", "204"),
    ] {
        let mut d = desc(id);
        d.slew_deg_per_s = 100000.0;
        let (path, virt) = pty_rotator(&d);
        let epoch = Instant::now();
        hamlib(model, &path, &["P", "123.5", "45.5"]).expect("rotctl runs");
        std::thread::sleep(Duration::from_millis(100));
        let s = now(&virt, epoch);
        assert!(
            (s.target_az - 123.5).abs() < 0.06 && (s.target_el - 45.5).abs() < 0.06,
            "{id}: hamlib set moved the virtual rotator to {} {}",
            s.target_az,
            s.target_el
        );
        if model != "201" {
            let out = hamlib(model, &path, &["p"]).expect("rotctl runs");
            let nums: Vec<f32> = out.lines().filter_map(|l| l.trim().parse().ok()).collect();
            assert!(
                nums.len() == 2 && (nums[0] - 123.5).abs() < 0.06 && (nums[1] - 45.5).abs() < 0.06,
                "{id}: hamlib read {out:?}"
            );
            hamlib(model, &path, &["P", "10", "10"]).expect("rotctl runs");
            hamlib(model, &path, &["S"]).expect("rotctl runs");
        }
    }
}
