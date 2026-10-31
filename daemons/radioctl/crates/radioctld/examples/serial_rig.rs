// SPDX-License-Identifier: AGPL-3.0-or-later
//! A real radio on a serial or USB-serial port. Compiled by the tests, never run by them: it
//! needs hardware. It reads the radio and never keys it: the transmit gate stays closed.
//!
//! Usage: `serial_rig PORT BAUD` (a path such as `/dev/ttyUSB0`), or `serial_rig usb VID PID
//! [SERIAL]` (hexadecimal ids, found again after every re-plug).
use std::error::Error;
use std::path::PathBuf;

use radioctl_core::rig::{spawn_current, Freshness, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::serial::{open_serial, PortTarget, SerialConfig};
use radioctl_core::units::Vfo;
use radioctl_core::usb::UsbMatcher;
use radioctl_protocols::elecraft::ElecraftEngine;
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
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;

    let mut config = SerialConfig::new(target, baud);
    config.first_open_attempts = 3;
    let link = open_serial(&Handle::current(), config).await?;
    println!("link: {:?}", link.status());

    // The actor watches the port: a reopen after a USB glitch unkeys the rig first. The monitor
    // (control lines, status, counters) stays alive here for the life of the rig.
    let options = RigOptions {
        link_watch: Some(link.watch()),
        ..RigOptions::default()
    };
    let (transport, _monitor) = link.into_parts();
    let rig = spawn_current(ElecraftEngine::new(description)?, transport, options)?;
    let frequency = rig.frequency(Vfo::A, Freshness::Fresh).await?;
    println!("VFO A: {} Hz", frequency.value.0);
    Ok(())
}
