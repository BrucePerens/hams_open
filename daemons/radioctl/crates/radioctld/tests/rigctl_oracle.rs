// SPDX-License-Identifier: AGPL-3.0-or-later
//! Differential tests: Hamlib's own `rigctl` client as the oracle against radioctld.
//!
//! `rigctl -m 2 -r host:port` is Hamlib's "NET rigctl" backend, the client WSJT-X, fldigi and the
//! rest of the ecosystem use when they talk to a `rigctld`. Each scenario starts radioctld (the
//! production path: configuration text, rig description file, real TCP) on a virtual Elecraft KX3
//! or a virtual Icom IC-7300 and drives it with the `rigctl` binary as a subprocess. What the
//! client prints, and the state the virtual rig ends up in, must agree. The `rigctl` binary is only
//! invoked; nothing of Hamlib is copied. Nothing is ever keyed on a real transmitter: every rig
//! here is a virtual rig in memory.
//!
//! Needs the `rigctl` binary (`RIGCTL=/path` to choose one) and the cargo feature `hamlib-oracle`;
//! with the feature on and no binary the tests fail loudly rather than skip.
#![cfg(feature = "hamlib-oracle")]
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{config_text, eventually, start};
use radioctl_core::units::{Hz, Level, Mode};
use radioctld::daemon::Daemon;

fn rigctl_binary() -> String {
    std::env::var("RIGCTL").unwrap_or_else(|_| "rigctl".to_string())
}

/// What one `rigctl` run printed.
struct Output {
    stdout: String,
    stderr: String,
}

/// Runs `rigctl -m 2 -r addr args...` with a deadline; fails the test if it does not finish.
async fn rigctl(daemon: &Daemon, args: &[&str]) -> Output {
    let addr = daemon.rigs[0].server.local_addr().to_string();
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    tokio::task::spawn_blocking(move || {
        let mut child = Command::new(rigctl_binary())
            .args(["-m", "2", "-r", &addr])
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("cannot run {}: {e}", rigctl_binary()));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match child.try_wait().expect("wait") {
                Some(_) => break,
                None if Instant::now() > deadline => {
                    let _ = child.kill();
                    panic!("rigctl {args:?} did not finish: radioctld left the client waiting");
                }
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        }
        let out = child.wait_with_output().expect("output");
        Output {
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    })
    .await
    .expect("join")
}

/// Everything the client printed, on both streams (it reports errors on either).
fn everything(output: &Output) -> String {
    format!("{}{}", output.stdout, output.stderr)
}

/// What the client prints on stdout, without the banner line it writes on stderr and stdout.
fn printed(output: &Output) -> String {
    output
        .stdout
        .lines()
        .filter(|l| !l.starts_with("rigctld: "))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn control(daemon: &Daemon) -> &radioctl_protocols::io::VirtualRigControl {
    daemon.rigs[0]
        .virtual_control
        .as_ref()
        .expect("virtual rig")
}

// Tests [@ANCHOR: radioctld_caps_dump_state]
// Tests [@ANCHOR: radioctld_caps_granularity]
// Tests [@ANCHOR: radioctld_caps_modes]
// Tests [@ANCHOR: radioctld_names_mode_mask]
// Tests [@ANCHOR: radioctld_names_level_mask]
// Tests [@ANCHOR: radioctld_names_func_mask]
// Tests [@ANCHOR: radioctld_service_execute]
// Tests [@ANCHOR: radioctld_service_get_level]
// Tests [@ANCHOR: radioctld_service_set_level]
// Tests [@ANCHOR: radioctld_service_get_func]
// Tests [@ANCHOR: radioctld_service_set_func]
// Tests [@ANCHOR: radioctld_service_rit_offset]
// Tests [@ANCHOR: radioctld_service_set_rit_offset]
// Tests [@ANCHOR: radioctld_service_served_level]
// Tests [@ANCHOR: radioctld_service_vfo_info]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hamlibs_rigctl_drives_a_virtual_kx3_through_radioctld() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    assert_eq!(printed(&rigctl(&daemon, &["f"]).await), "14060000\n");
    rigctl(&daemon, &["F", "14074000"]).await;
    let rig = control(&daemon).clone();
    eventually("VFO A", || rig.state().freq_a == Hz(14_074_000)).await;
    assert_eq!(printed(&rigctl(&daemon, &["f"]).await), "14074000\n");

    rigctl(&daemon, &["M", "USB", "2400"]).await;
    eventually("mode and width", || {
        let s = rig.state();
        (s.mode, s.filter_width) == (Mode::Usb, Hz(2400))
    })
    .await;
    assert_eq!(printed(&rigctl(&daemon, &["m"]).await), "USB\n2400\n");

    rigctl(&daemon, &["S", "1", "VFOB"]).await;
    eventually("split", || rig.state().split).await;
    assert_eq!(printed(&rigctl(&daemon, &["s"]).await), "1\nVFOB\n");
    rigctl(&daemon, &["I", "14075000"]).await;
    eventually("VFO B", || rig.state().freq_b == Hz(14_075_000)).await;
    assert_eq!(printed(&rigctl(&daemon, &["i"]).await), "14075000\n");

    rigctl(&daemon, &["J", "120"]).await;
    eventually("RIT", || {
        let s = rig.state();
        s.rit && s.rit_offset == 120
    })
    .await;
    assert_eq!(printed(&rigctl(&daemon, &["j"]).await), "120\n");
    assert_eq!(printed(&rigctl(&daemon, &["u", "RIT"]).await), "1\n");
    rigctl(&daemon, &["U", "RIT", "0"]).await;
    eventually("RIT off", || !rig.state().rit).await;

    rigctl(&daemon, &["L", "KEYSPD", "25"]).await;
    eventually("keyer speed", || {
        rig.state().levels.get(&Level::KeyerSpeed) == Some(&25)
    })
    .await;
    assert_eq!(printed(&rigctl(&daemon, &["l", "KEYSPD"]).await), "25\n");
    // The front end through Hamlib's own client: attenuator in decibels, preamplifier as a step.
    rigctl(&daemon, &["L", "ATT", "10"]).await;
    eventually("attenuator", || {
        rig.state().levels.get(&Level::Attenuator) == Some(&10)
    })
    .await;
    assert_eq!(printed(&rigctl(&daemon, &["l", "ATT"]).await), "10\n");
    rigctl(&daemon, &["L", "PREAMP", "1"]).await;
    eventually("preamplifier", || {
        rig.state().levels.get(&Level::Preamp) == Some(&1)
    })
    .await;
    assert_eq!(printed(&rigctl(&daemon, &["l", "PREAMP"]).await), "1\n");
    rigctl(&daemon, &["L", "AF", "0.5"]).await;
    let raw = control(&daemon)
        .state()
        .levels
        .get(&Level::AfGain)
        .copied()
        .expect("af");
    assert_eq!(raw, 128);
    let af: f64 = printed(&rigctl(&daemon, &["l", "AF"]).await)
        .trim()
        .parse()
        .expect("float");
    assert!((af - 0.5).abs() < 0.01, "{af}");

    let rejected = rigctl(&daemon, &["l", "STRENGTH"]).await;
    // The client reads the advertised levels from \dump_state and refuses one that was not.
    assert!(
        everything(&rejected).contains("Invalid parameter"),
        "{}",
        everything(&rejected)
    );
}

// Tests [@ANCHOR: radioctld_caps_dump_state]
// Tests [@ANCHOR: radioctld_caps_gran]
// Tests [@ANCHOR: radioctld_caps_wire_float]
// Tests [@ANCHOR: radioctld_caps_wire_int]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hamlibs_client_reads_back_the_capabilities_radioctld_advertises() {
    let kx3 = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    let caps = rigctl(&kx3, &["1"]).await.stdout;
    assert!(
        caps.contains("Mode list: AM CW USB LSB FM CWR PKTLSB PKTUSB"),
        "{caps}"
    );
    assert!(caps.contains("Get functions: RIT XIT"), "{caps}");
    assert!(caps.contains("KEYSPD(8..50/1)"), "{caps}");
    assert!(caps.contains("AF(0.000000..1.000000/0.003922)"), "{caps}");
    assert!(caps.contains("RFPOWER(0.000000..1.000000/"), "{caps}");
    assert!(caps.contains("SWR(0.000000..10.000000/0.100000)"), "{caps}");
    assert!(caps.contains("RAWSTR(0..255/1)"), "{caps}");
    assert!(caps.contains("Max RIT: -9.999kHz/+9.999kHz"), "{caps}");
    assert!(caps.contains("VFO list: VFOA VFOB"), "{caps}");
    assert!(caps.contains("Attenuator: 10dB"), "{caps}");
    assert!(caps.contains("Preamp: 1dB"), "{caps}");
    kx3.stop().await;

    let ic = start(&config_text(
        "",
        "ic7300",
        "icom-ic-7300.toml",
        false,
        false,
    ))
    .await;
    let caps = rigctl(&ic, &["1"]).await.stdout;
    assert!(
        caps.contains("Mode list: AM CW USB LSB RTTY FM CWR RTTYR"),
        "{caps}"
    );
    assert!(caps.contains("AF(0.000000..1.000000/0.003922)"), "{caps}");
    assert!(caps.contains("RAWSTR(0..255/1)"), "{caps}");
    assert!(caps.contains("KEYSPD(6..48/1)"), "{caps}");
    // The front-end lists are read back by Hamlib's own client from \dump_state: the preamplifier
    // as step numbers (Hamlib prints them with its dB unit), the attenuator in decibels.
    assert!(caps.contains("Preamp: 1dB 2dB"), "{caps}");
    assert!(caps.contains("Attenuator: 20dB"), "{caps}");
    assert!(caps.contains("ATT(0..20/1)"), "{caps}");
    assert!(caps.contains("PREAMP(0..2/1)"), "{caps}");
    assert!(caps.contains("VFO list: VFOA VFOB"), "{caps}");
    ic.stop().await;
}

// Tests [@ANCHOR: radioctld_service_key]
// Tests [@ANCHOR: radioctld_service_check_transmit]
// Tests [@ANCHOR: radioctld_service_refuse]
// Tests [@ANCHOR: radioctld_reply_error_code]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hamlibs_rigctl_is_refused_while_the_gate_is_closed_and_the_rig_hears_nothing() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, false)).await;
    rigctl(&daemon, &["f"]).await;
    let before = control(&daemon).bytes_received();
    let refused = rigctl(&daemon, &["T", "1"]).await;
    assert!(
        everything(&refused).contains("Command rejected by the rig"),
        "{}",
        everything(&refused)
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(control(&daemon).bytes_received(), before);
    assert!(!control(&daemon).state().ptt);
    assert_eq!(printed(&rigctl(&daemon, &["t"]).await), "0\n");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_service_key]
// Tests [@ANCHOR: radioctld_service_unkey]
// Tests [@ANCHOR: radioctld_service_disconnect]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hamlibs_rigctl_keys_a_virtual_rig_only_when_the_configuration_opened_the_gate() {
    use std::io::{BufRead, BufReader, Write};
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", true, false)).await;
    let addr = daemon.rigs[0].server.local_addr().to_string();
    let rig = control(&daemon).clone();
    // One session: a keyed rig stays keyed only while its owner is connected, so the client has
    // to stay up between "T 1" and "T 0". Interactive rigctl reads its commands from stdin.
    tokio::task::spawn_blocking(move || {
        let mut child = Command::new(rigctl_binary())
            .args(["-m", "2", "-r", &addr])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("rigctl");
        let mut stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let wait = |what: &str, done: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !done() {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        stdin.write_all(b"T 1\n").expect("write");
        wait("the virtual rig to be keyed", &|| rig.state().ptt);
        stdin.write_all(b"t\n").expect("write");
        stdin.write_all(b"T 0\n").expect("write");
        wait("the virtual rig to be unkeyed", &|| !rig.state().ptt);
        stdin.write_all(b"T 1\n").expect("write");
        wait("the virtual rig to be keyed again", &|| rig.state().ptt);
        drop(stdin);
        wait("an unkey when the owner's rigctl exits", &|| {
            !rig.state().ptt
        });
        // The client's stdout is a pipe, so C stdio holds its output until it exits.
        let _ = child.wait();
        let printed: Vec<String> = BufReader::new(stdout)
            .lines()
            .map(|l| l.expect("line"))
            .collect();
        assert!(
            printed.iter().any(|l| l.trim() == "PTT: 1"),
            "t never read back PTT: 1: {printed:?}"
        );
    })
    .await
    .expect("join");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_service_execute]
// Tests [@ANCHOR: radioctld_service_set_split_vfo]
// Tests [@ANCHOR: radioctld_service_split_vfo]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hamlibs_rigctl_drives_a_virtual_ic7300_and_sees_what_it_cannot_do() {
    let daemon = start(&config_text(
        "",
        "ic7300",
        "icom-ic-7300.toml",
        false,
        false,
    ))
    .await;
    let rig = control(&daemon).clone();
    assert_eq!(printed(&rigctl(&daemon, &["f"]).await), "14074000\n");
    rigctl(&daemon, &["F", "7074000"]).await;
    eventually("VFO A", || rig.state().freq_a == Hz(7_074_000)).await;
    assert_eq!(printed(&rigctl(&daemon, &["m"]).await), "USB\n500\n");
    rigctl(&daemon, &["M", "CW", "500"]).await;
    eventually("mode", || rig.state().mode == Mode::Cw).await;
    assert_eq!(printed(&rigctl(&daemon, &["m"]).await), "CW\n500\n");
    rigctl(&daemon, &["S", "1", "VFOB"]).await;
    eventually("split", || rig.state().split).await;
    assert_eq!(printed(&rigctl(&daemon, &["s"]).await), "1\nVFOB\n");
    // Hamlib reads the virtual rig's keyer speed (18 wpm) back through the `14 0C` table.
    assert_eq!(printed(&rigctl(&daemon, &["l", "KEYSPD"]).await), "18\n");
    assert_eq!(printed(&rigctl(&daemon, &["l", "RAWSTR"]).await), "30\n");
    assert_eq!(printed(&rigctl(&daemon, &["t"]).await), "0\n");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_service_get_level]
// Tests [@ANCHOR: radioctld_service_set_level]
// Tests [@ANCHOR: radioctld_names_level_range]
// Tests [@ANCHOR: radioctld_caps_granularity]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hamlibs_rigctl_scales_kenwood_levels_by_their_own_ranges() {
    let daemon = start(&config_text(
        "",
        "ts590sg",
        "kenwood-ts-590sg.toml",
        false,
        false,
    ))
    .await;
    let caps = rigctl(&daemon, &["1"]).await.stdout;
    assert!(caps.contains("KEYSPD(10..60/1)"), "{caps}");
    assert!(
        caps.contains("MICGAIN(0.000000..1.000000/0.010000)"),
        "{caps}"
    );
    let rig = control(&daemon).clone();
    rigctl(&daemon, &["L", "KEYSPD", "30"]).await;
    eventually("keyer speed", || {
        rig.state().levels.get(&Level::KeyerSpeed) == Some(&30)
    })
    .await;
    assert_eq!(printed(&rigctl(&daemon, &["l", "KEYSPD"]).await), "30\n");
    rigctl(&daemon, &["L", "MICGAIN", "0.5"]).await;
    eventually("mic gain", || {
        rig.state().levels.get(&Level::MicGain) == Some(&50)
    })
    .await;
    assert_eq!(printed(&rigctl(&daemon, &["l", "MICGAIN"]).await), "0.5\n");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_reply_render]
// Tests [@ANCHOR: radioctld_command_parse_line]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hamlibs_rigctl_works_against_a_server_in_vfo_mode() {
    let daemon = start(&config_text("", "kx3", "elecraft-kx3.toml", false, true)).await;
    assert_eq!(
        printed(&rigctl(&daemon, &["f", "VFOA"]).await),
        "14060000\n"
    );
    let rig = control(&daemon).clone();
    rigctl(&daemon, &["F", "VFOA", "14074000"]).await;
    eventually("VFO A", || rig.state().freq_a == Hz(14_074_000)).await;
    rigctl(&daemon, &["F", "VFOB", "14076000"]).await;
    eventually("VFO B", || rig.state().freq_b == Hz(14_076_000)).await;
    assert_eq!(printed(&rigctl(&daemon, &["m", "VFOA"]).await), "CW\n500\n");
    rigctl(&daemon, &["M", "VFOA", "USB", "2400"]).await;
    eventually("mode", || rig.state().mode == Mode::Usb).await;
    daemon.stop().await;
}
