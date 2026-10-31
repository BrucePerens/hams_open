// SPDX-License-Identifier: AGPL-3.0-or-later
//! A `kind = "serial"` rig on a pseudo-terminal that holds a virtual IC-7300: the bench-day
//! rehearsal path. A pty has no modem lines, so the default (`lines = "managed"`) fails the open
//! loudly and `lines = "unmanaged"` serves the radio.
#![allow(clippy::expect_used, clippy::panic)]

mod common;

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use common::{quiet_options, rigs_dir, Client};
use radioctl_protocols::virtual_rig::VirtualRig;
use radioctld::config::Config;
use radioctld::daemon;
use radioctld::rigs::load_description;
use serialport::{SerialPort, TTYPort};
use tokio::runtime::Handle;

/// A virtual IC-7300 behind a pty: returns the slave path, the stop flag, the thread and the
/// master (kept alive so the slave stays open for business).
fn radio_on_pty() -> (String, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let description = load_description(&rigs_dir().join("icom-ic-7300.toml")).expect("description");
    let rig = Arc::new(Mutex::new(
        VirtualRig::from_description(&description).expect("virtual rig"),
    ));
    let (mut master, slave) = TTYPort::pair().expect("pty pair");
    let slave_path = slave.name().expect("slave name");
    drop(slave);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_t = Arc::clone(&stop);
    let thread = thread::spawn(move || {
        master
            .set_timeout(Duration::from_millis(20))
            .expect("timeout");
        let started = Instant::now();
        let mut buffer = [0u8; 256];
        while !stop_t.load(Ordering::Acquire) {
            let Ok(n) = master.read(&mut buffer) else {
                continue;
            };
            let reply = rig
                .lock()
                .expect("lock")
                .feed(buffer.get(..n).unwrap_or(&[]), started.elapsed());
            if !reply.is_empty() {
                let _ = master.write_all(&reply);
            }
        }
    });
    (slave_path, stop, thread)
}

fn config_for(slave: &str, extra: &str) -> Config {
    let text = format!(
        "[[rig]]\nname = \"ic7300\"\ndescription = {:?}\nlisten = \"127.0.0.1:0\"\n\
         [rig.transport]\nkind = \"serial\"\npath = {slave:?}\nbaud = 19200\n{extra}\n",
        rigs_dir().join("icom-ic-7300.toml").display().to_string()
    );
    Config::parse(&text, Path::new(".")).expect("config")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pty_needs_lines_unmanaged_and_then_serves_the_radio() {
    let (slave, stop, thread) = radio_on_pty();
    let handle = Handle::current();

    // The default manages DTR and RTS, which a pty does not have: the daemon refuses to start
    // and says why, instead of serving a radio it cannot control.
    let config = config_for(&slave, "");
    let description = load_description(&config.rigs[0].description).expect("description");
    let refused = daemon::start(&handle, &config, vec![description], quiet_options).await;
    let message = refused.err().expect("the managed open of a pty fails");
    assert!(message.contains("DTR"), "unexpected refusal: {message}");

    let config = config_for(&slave, "lines = \"unmanaged\"");
    let description = load_description(&config.rigs[0].description).expect("description");
    let daemon = daemon::start(&handle, &config, vec![description], quiet_options)
        .await
        .expect("daemon on the pty");
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("f").await, "14074000\n");
    assert_eq!(client.ask("F 7074000").await, "RPRT 0\n");
    assert_eq!(client.ask("f").await, "7074000\n");
    assert_eq!(
        client.ask("T 1").await,
        "RPRT -9\n",
        "the gate stays closed"
    );
    daemon.stop().await;

    stop.store(true, Ordering::Release);
    thread.join().expect("radio thread");
}
