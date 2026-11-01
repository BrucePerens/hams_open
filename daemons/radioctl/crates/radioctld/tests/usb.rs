// SPDX-License-Identifier: AGPL-3.0-or-later
//! The USB transport as the daemon uses it: a virtual USB board (the twin on the fake bus, behind
//! the real USB link, frames and engine) served over rigctld's text protocol, the closed transmit
//! gate on a board with a PTT request, and the refusals of `kind = "usb"` that need no device: a
//! description that is not a USB device, and (in a build without the `usb` feature) the missing
//! libusb support. No test opens a USB device; nothing here keys anything.
#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

mod common;

use std::path::Path;

use common::{config_text, rigs_dir, start, Client};
use radioctl_core::description::Family;
use radioctld::config::Config;
use radioctld::daemon;
use radioctld::rigs::load_description;
use tokio::runtime::Handle;

// Tests [@ANCHOR: radioctld_rigs_build_rig]
// Tests [@ANCHOR: radioctld_daemon_start_one]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_virtual_si570_board_is_tuned_over_the_usb_link_and_the_gate_stays_closed() {
    let text = config_text("", "softrock", "softrock-si570-avr-usb.toml", false, false);
    let daemon = start(&text).await;
    assert!(daemon.rigs[0].usb_link.is_some(), "the rig runs over the USB link");
    assert!(daemon.rigs[0].serial_link.is_none() && daemon.rigs[0].net_link.is_none());
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("f").await, "7074000\n");
    assert_eq!(client.ask("F 14074000").await, "RPRT 0\n");
    assert_eq!(client.ask("f").await, "14074000\n");
    // The board has a PTT request, but the gate is closed unless the configuration opens it.
    assert_eq!(client.ask("T 1").await, "RPRT -9\n");
    assert_eq!(client.ask("t").await, "0\n", "still receiving");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_virtual_funcube_and_traveller_are_served_too() {
    let text = config_text("", "funcube", "amsat-uk-funcube-dongle-pro-plus.toml", false, false);
    let daemon = start(&text).await;
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("F 145800000").await, "RPRT 0\n");
    assert_eq!(client.ask("f").await, "145800000\n");
    let text = config_text("", "dwt", "digital-world-traveller.toml", false, false);
    let daemon = start(&text).await;
    let mut client = Client::connect(&daemon).await;
    assert_eq!(client.ask("F 9620000").await, "RPRT 0\n");
}

/// Configuration text with a `kind = "usb"` rig from description `file`.
fn usb_text(file: &str) -> String {
    format!(
        "[[rig]]\nname = \"board\"\ndescription = {:?}\nlisten = \"127.0.0.1:0\"\n[rig.transport]\nkind = \"usb\"\n",
        rigs_dir().join(file).display().to_string()
    )
}

async fn start_error(file: &str) -> String {
    let config = Config::parse(&usb_text(file), Path::new(".")).expect("config");
    let description = load_description(&config.rigs[0].description).expect("description");
    match daemon::start(&Handle::current(), &config, vec![description], common::quiet_options).await {
        Ok(_) => panic!("started"),
        Err(why) => why,
    }
}

// Tests [@ANCHOR: radioctld_rigs_real_usb_backend_missing]
#[cfg(not(feature = "usb"))]
#[tokio::test]
async fn a_build_without_the_usb_feature_says_so_before_it_looks_for_a_device() {
    let why = start_error("fifi-sdr.toml").await;
    assert!(why.contains("built without USB support"), "{why}");
}

#[tokio::test]
async fn a_usb_transport_needs_a_usb_device_description() {
    let why = start_error("elecraft-kx3.toml").await;
    assert!(why.contains("needs a usb-device description"), "{why}");
    assert_eq!(
        load_description(&rigs_dir().join("fifi-sdr.toml")).expect("fifi").family,
        Family::UsbDevice
    );
}

#[tokio::test]
async fn a_usb_board_without_an_engine_is_not_started() {
    let why = start_error("elektor-sdr-usb.toml").await;
    assert!(why.contains("no engine yet"), "{why}");
}

// Tests [@ANCHOR: radioctld_rigs_real_usb_backend]
#[cfg(feature = "usb")]
#[tokio::test]
async fn a_build_with_the_usb_feature_looks_for_the_board_and_reports_it_absent() {
    // Ids no device has, so libusb lists the bus and opens nothing.
    let text = usb_text("fifi-sdr.toml").replace("kind = \"usb\"", "kind = \"usb\"\nvid = 0xFFFF\npid = 0xFFFE");
    let config = Config::parse(&text, Path::new(".")).expect("config");
    let description = load_description(&config.rigs[0].description).expect("description");
    let why = daemon::start(&Handle::current(), &config, vec![description], common::quiet_options)
        .await
        .err()
        .expect("absent");
    assert!(why.contains("not plugged in"), "{why}");
}
