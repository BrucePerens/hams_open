// SPDX-License-Identifier: AGPL-3.0-or-later
//! Read-only discovery: lists ELAD receivers, opens the first (or `--index N`) and prints what it reports. Sends only the
//! firmware-version and EEPROM reads; starts no stream, tunes nothing, transmits nothing. The serial number is shown as its
//! length only unless `--show-serial` is given.
use elad_usb::fpga::FpgaState;
use elad_usb::radio::{OpenOptions, Radio};
use elad_usb::usb::{find, UsbTransport};

fn main() {
    let show_serial = std::env::args().any(|a| a == "--show-serial");
    let found = match find() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("discovery failed: {e}");
            std::process::exit(1);
        }
    };
    if found.is_empty() {
        println!("no ELAD receiver on the bus");
        return;
    }
    for f in &found {
        println!(
            "bus {} address {}: pid {:#06x} {}",
            f.bus,
            f.address,
            f.pid,
            f.model.name()
        );
    }
    let f = &found[0];
    let opened = UsbTransport::open(f).and_then(|t| {
        Radio::open(
            t,
            f.model,
            OpenOptions {
                fpga: FpgaState::Unknown,
                sample_rate_hz: 192_000,
            },
        )
    });
    match opened {
        Ok(r) => {
            let i = r.info();
            println!("firmware bytes: {:?}", i.firmware);
            println!("hardware: {:?}", i.calibration.hardware);
            match (&i.calibration.serial, show_serial) {
                (Some(s), true) => println!("serial: {s}"),
                (Some(s), false) => println!("serial: present, {} characters", s.len()),
                (None, _) => println!("serial: not programmed"),
            }
            println!("rate correction Hz: {:?}", i.calibration.rate_correction_hz);
            println!(
                "offsets dB global/lp/att: {:?} {:?} {:?}",
                i.calibration.global_db, i.calibration.lp_db, i.calibration.att_db
            );
            println!(
                "needs an FPGA image before samples flow: {}",
                f.model.needs_fpga_image()
            );
        }
        Err(e) => {
            eprintln!("open failed: {e}");
            std::process::exit(2);
        }
    }
}
