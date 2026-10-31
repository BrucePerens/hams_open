// SPDX-License-Identifier: AGPL-3.0-or-later
//! A real radio on a serial or USB-serial port. Compiled by the tests, never run by them: it
//! needs hardware. It reads the radio and never keys it: the transmit gate stays closed (the one transmitter command it sends is the start-up unkey).
//!
//! Usage: `serial_rig PORT BAUD` (a path such as `/dev/ttyUSB0`), or `serial_rig usb VID PID
//! [SERIAL]` (hexadecimal ids, found again after every re-plug).
use std::error::Error;
use std::path::PathBuf;

use radioctl_core::serial::{PortTarget, SerialConfig};
use radioctl_core::usb::UsbMatcher;
use radioctl_protocols::prelude::*;
use tokio::runtime::Handle;

fn target_and_baud(args: &[String]) -> Result<(PortTarget, u32), Box<dyn Error>> {
    match args {
        [path, baud] => Ok((PortTarget::Path(PathBuf::from(path)), baud.parse()?)),
        [usb, vid, pid, rest @ ..] if usb == "usb" => {
            let matcher = UsbMatcher {
                vid: u16::from_str_radix(vid, 16)?,
                pid: u16::from_str_radix(pid, 16)?,
                serial: rest.first().cloned(),
            };
            Ok((PortTarget::Usb(matcher), 38_400))
        }
        _ => Err("usage: serial_rig PORT BAUD | serial_rig usb VID PID [SERIAL]".into()),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (target, baud) = target_and_baud(&args)?;
    let description = rigfile::load("elecraft-kx3")?.description;

    // Opens the port, watches the link (a reopen after a USB glitch unkeys the rig first), and
    // sends the start-up unkey radioctld sends. The transmit gate stays closed. Keep `link`
    // (control lines, status, counters) alive for the life of the rig.
    let mut config = SerialConfig::new(target, baud);
    config.first_open_attempts = 3;
    let SerialRig { rig, link } = open_serial_rig_with(
        &Handle::current(),
        &description,
        config,
        RigOptions::default(),
    )
    .await?;
    println!("link: {:?}", link.status());
    let frequency = rig.frequency(Vfo::A, Freshness::Fresh).await?;
    println!("VFO A: {} Hz", frequency.value.0);
    Ok(())
}
