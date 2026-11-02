// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rotators served by radioctld in the `rotctld` text protocol, the production path (configuration
//! text, `daemon::start`, real TCP) against the virtual rotator in both of its protocols: the
//! `\dump_state` a Hamlib client reads on connect, a move and its arrival, a stop in mid-slew, a
//! position outside the travel refused before anything is written, the park position, the extended
//! format, unserved and unknown commands, two clients on one rotator, `q`, and a rotator next to a
//! rig in one file. With the `hamlib-oracle` feature, Hamlib's own `rotctl -m 2` drives it too.
//! Nothing is verified against a real rotator; nothing here keys anything.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::time::Duration;

use common::{config_text, start, Client};
use radioctld::daemon::Daemon;
use tokio::time::sleep;

/// Configuration text for one virtual rotator of `model` named `name`.
fn rotator_text(name: &str, model: &str) -> String {
    format!(
        "[[rotator]]\nname = \"{name}\"\nmodel = \"{model}\"\nlisten = \"127.0.0.1:0\"\n\
         [rotator.transport]\nkind = \"virtual\"\n"
    )
}

/// Connects to the first rotator of `daemon`.
async fn rotator_client(daemon: &Daemon) -> Client {
    Client::connect_to(daemon.rotators[0].server.local_addr()).await
}

/// Asks `p` until the rotator reports within a degree of `az`/`el`, or fails after five seconds.
async fn arrives(client: &mut Client, az: f32, el: f32) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let reply = client.ask("p").await;
        let mut lines = reply.lines();
        let got_az: f32 = lines.next().unwrap().parse().unwrap();
        let got_el: f32 = lines.next().unwrap().parse().unwrap();
        if (got_az - az).abs() <= 1.0 && (got_el - el).abs() <= 1.0 {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "did not arrive at {az} {el}: last {got_az} {got_el}"
        );
        sleep(Duration::from_millis(50)).await;
    }
}

// Tests [@ANCHOR: radioctld_daemon_start_rotator]
// Tests [@ANCHOR: radioctld_rotators_build_rotator]
// Tests [@ANCHOR: radioctld_rotctl_service_new]
// Tests [@ANCHOR: radioctld_rotctl_service_rotator]
// Tests [@ANCHOR: radioctld_rotctl_service_execute]
// Tests [@ANCHOR: radioctld_rotctl_dump_state]
// Tests [@ANCHOR: radioctld_rotctl_answer]
// Tests [@ANCHOR: radioctld_rotctl_service_answer]
// Tests [@ANCHOR: radioctld_rotctl_service_disconnect]
// Tests [@ANCHOR: radioctld_daemon_stop]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rotctld_rotator_moves_stops_and_refuses_what_is_out_of_travel() {
    // The generic rotctld model slews at 3 degrees a second; too slow for a test, so the
    // served description is what the configuration says and the virtual rotator behind it is
    // the same description: a 90-degree move takes 30 s at that rate. Use the position checks
    // on short moves and the stop on a long one.
    let daemon = start(&rotator_text("rot", "generic-rotctld")).await;
    assert_eq!(daemon.rotators[0].name, "rot");
    assert_eq!(
        daemon.rotators[0].service.rotator().description().id,
        "generic-rotctld"
    );
    let control = daemon.rotators[0].virtual_control.clone().expect("virtual");
    let mut client = rotator_client(&daemon).await;

    // What rotctl -m 2 reads on connect: the shape rotctld 4.6.2 prints.
    assert_eq!(
        client.ask("\\dump_state").await,
        "1\n1\nmin_az=0.000000\nmax_az=360.000000\nmin_el=0.000000\nmax_el=90.000000\n\
         south_zero=0\nrot_type=AzEl\ndone\n"
    );
    assert_eq!(client.ask("p").await, "0.00\n0.00\n");
    assert_eq!(
        client.ask("_").await,
        "Any rotator behind a Hamlib rotctld (TCP port 4533)\n"
    );
    // A short move arrives; the extended format names the fields.
    assert_eq!(client.ask("P 1.5 1").await, "RPRT 0\n");
    arrives(&mut client, 1.5, 1.0).await;
    let extended = client.ask("+p").await;
    assert!(extended.starts_with("get_pos:\nAzimuth: "), "{extended}");
    assert!(extended.contains("\nElevation: ") && extended.ends_with("RPRT 0\n"));
    assert_eq!(client.ask("+P 2 1").await, "set_pos: 2 1\nRPRT 0\n");
    // A long move, stopped in mid-slew: it freezes short of the target.
    assert_eq!(client.ask("P 300 45").await, "RPRT 0\n");
    sleep(Duration::from_millis(400)).await;
    assert_eq!(client.ask("S").await, "RPRT 0\n");
    let frozen = control.state();
    assert!(
        frozen.az > 2.0 && frozen.az < 300.0,
        "stopped en route at {}",
        frozen.az
    );
    sleep(Duration::from_millis(200)).await;
    assert_eq!(control.state().az, frozen.az, "still frozen");
    // Out of travel: refused as -1 and nothing reaches the rotator. No park position: -11.
    let before = control.received().len();
    for bad in ["P 361 0", "P -1 0", "P 10 91", "P 10 -0.5"] {
        assert_eq!(client.ask(bad).await, "RPRT -1\n", "{bad}");
    }
    assert_eq!(client.ask("K").await, "RPRT -11\n");
    assert_eq!(control.received().len(), before, "nothing was written");
    // Hamlib commands radioctld does not serve, unknown words, raw, a non-text line.
    assert_eq!(client.ask("M 16 0").await, "RPRT -4\n");
    assert_eq!(
        client.ask("+\\get_level SPEED").await,
        "get_level: SPEED\nRPRT -4\n"
    );
    assert_eq!(client.ask("\\bogus").await, "RPRT -1\n");
    assert_eq!(client.ask("w P 0 0").await, "RPRT -11\n");
    client.send_raw(b"p\x01\n").await;
    assert_eq!(client.read_reply().await, "RPRT -1\n");
    // A second client shares the rotator; q closes only its own connection.
    let mut second = rotator_client(&daemon).await;
    assert_eq!(second.ask("q").await, "RPRT 0\n");
    assert!(second.is_closed(Duration::from_secs(2)).await);
    assert!(client.ask("p").await.ends_with('\n'));
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_rotctl_error_code]
// Tests [@ANCHOR: radioctld_config_rotator]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_gs232_rotator_parks_and_an_azimuth_only_one_reports_its_type() {
    let two = format!(
        "{}{}",
        rotator_text("g5500", "yaesu-g-5500-gs232b"),
        rotator_text("g450", "yaesu-g-450-gs232a")
    );
    let daemon = start(&two).await;
    assert_eq!(daemon.rotators.len(), 2);
    let control = daemon.rotators[0].virtual_control.clone().expect("virtual");
    let mut client = rotator_client(&daemon).await;
    assert!(client.ask("\\dump_state").await.contains(
        "max_az=450.000000\nmin_el=0.000000\nmax_el=180.000000\nsouth_zero=0\nrot_type=AzEl\n"
    ));
    // GS-232 sets have no reply; the position read shows the controller took it. The virtual
    // G-5500 slews at 2 degrees a second, so a 1-degree move arrives within the wait.
    assert_eq!(client.ask("P 1 1").await, "RPRT 0\n");
    arrives(&mut client, 1.0, 1.0).await;
    assert_eq!(client.ask("K").await, "RPRT 0\n");
    arrives(&mut client, 0.0, 0.0).await;
    assert!(control.received().iter().any(|l| l == "W000 000"));
    assert_eq!(client.ask("P 451 0").await, "RPRT -1\n");
    // The azimuth-only rotator.
    let mut az_only = Client::connect_to(daemon.rotators[1].server.local_addr()).await;
    assert!(az_only
        .ask("\\dump_state")
        .await
        .contains("max_el=0.000000\nsouth_zero=0\nrot_type=Az\n"));
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_config_parse]
// Tests [@ANCHOR: radioctld_daemon_start]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rig_and_a_rotator_are_served_from_one_file() {
    let text = format!(
        "{}{}",
        config_text("", "kx3", "elecraft-kx3.toml", false, false),
        rotator_text("rot", "generic-rotctld")
    );
    let daemon = start(&text).await;
    assert_eq!((daemon.rigs.len(), daemon.rotators.len()), (1, 1));
    let mut rig = Client::connect(&daemon).await;
    assert!(rig.ask("f").await.trim().parse::<u64>().is_ok());
    // The rig's listener does not know rotator commands, and the rotator's does not know rig ones.
    assert_eq!(rig.ask("\\set_pos 10 10").await, "RPRT -1\n");
    let mut rotator = rotator_client(&daemon).await;
    assert_eq!(rotator.ask("f").await, "RPRT -1\n");
    daemon.stop().await;
}

/// Hamlib's own `rotctl -m 2` (NET rotctl) against the served rotator: the connect-time
/// `\dump_state` read, a position set and read. Needs the `rotctl` binary (`ROTCTL=/path`).
#[cfg(feature = "hamlib-oracle")]
// Tests [@ANCHOR: radioctld_rotctl_dump_state]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hamlib_rotctl_reads_and_moves_the_served_rotator() {
    let daemon = start(&rotator_text("rot", "yaesu-g-5500-gs232b")).await;
    let addr = daemon.rotators[0].server.local_addr().to_string();
    let binary = std::env::var("ROTCTL").unwrap_or_else(|_| "rotctl".to_string());
    let run = |args: Vec<String>| {
        let binary = binary.clone();
        tokio::task::spawn_blocking(move || {
            let output = std::process::Command::new(&binary)
                .args(&args)
                .stdin(std::process::Stdio::null())
                .output()
                .expect("rotctl runs (ROTCTL=/path to choose the binary)");
            (
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
                output.status.success(),
            )
        })
    };
    let (out, err, ok) = run(vec![
        "-m".into(),
        "2".into(),
        "-r".into(),
        addr.clone(),
        "P".into(),
        "3".into(),
        "2".into(),
    ])
    .await
    .unwrap();
    assert!(ok && out.is_empty(), "set_pos: {out} {err}");
    let (out, err, ok) = run(vec![
        "-m".into(),
        "2".into(),
        "-r".into(),
        addr.clone(),
        "p".into(),
    ])
    .await
    .unwrap();
    assert!(ok, "get_pos: {out} {err}");
    let values: Vec<f32> = out.split_whitespace().map(|v| v.parse().unwrap()).collect();
    assert_eq!(values.len(), 2, "{out}");
    assert!(values[0] >= 0.0 && values[0] <= 3.0, "{out}");
    daemon.stop().await;
}

// Tests [@ANCHOR: radioctld_daemon_stop]
// Tests [@ANCHOR: radioctl_station_vrot_serve]
// Tests [@ANCHOR: radioctl_station_vrot_control_stop]
// Tests [@ANCHOR: radioctl_station_vrot_control_address]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stopping_the_daemon_stops_the_virtual_rotators_listener() {
    let daemon = start(&rotator_text("rot", "generic-rotctld")).await;
    let control = daemon.rotators[0].virtual_control.clone().expect("virtual");
    let address = control
        .address()
        .expect("the virtual rotator reports its socket");
    // Served now: a connection is accepted by the listener task.
    let probe = tokio::net::TcpStream::connect(address).await;
    assert!(
        probe.is_ok(),
        "the virtual rotator is served before the stop"
    );
    drop(probe);
    daemon.stop().await;
    // The listener task is gone and its socket closed: nothing accepts, the port is refused.
    assert!(
        tokio::net::TcpStream::connect(address).await.is_err(),
        "the virtual rotator's listener outlived Daemon::stop"
    );
    // Stopping again, through the clone, is harmless.
    control.stop().await;
}
