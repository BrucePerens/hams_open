// SPDX-License-Identifier: AGPL-3.0-or-later
//! `am_replay <freq_hz> <level_word> <v92> <v94_value> <v94_index> <v9c> <v9d>`: replays the control transfers the vendor program sent when
//! amplitude modulation was switched on (usbmon capture 2026-10-28, "sg_mod am" at 10 MHz, 500 Hz, 100 %), with the parameter fields
//! given on the command line, to find out what each field does by looking at the output on a scope. The unit must already be in
//! generator mode (`generator on`). After this the unit refuses further requests (stalls them) until it is power cycled.
use vnwa_open::protocol::generator_frequency;
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize| -> Result<f64, String> {
        let s = a.get(i).ok_or("missing argument")?;
        if let Some(h) = s.strip_prefix("0x") {
            u32::from_str_radix(h, 16)
                .map(f64::from)
                .map_err(|e| e.to_string())
        } else {
            s.parse().map_err(|_| format!("argument {i}: not a number"))
        }
    };
    let (f, level) = (num(1)?, num(2)? as u16);
    let (v92, v94, i94, v9c, v9d) = (
        num(3)? as u16,
        num(4)? as u16,
        num(5)? as u16,
        num(6)? as u16,
        num(7)? as u16,
    );
    let usb = Vnwa::open()?;
    // AM_SKIP_91=1: the unit stalls 0x91 while modulating, so a second replay without a power cycle leaves it out.
    if std::env::var("AM_SKIP_91").is_err() {
        usb.read(0x91, 0, 0, 64)?;
    }
    usb.read(0x0e, 0, 0, 64)?;
    usb.send(&generator_frequency(f, 0))?;
    usb.read(0x04, 0x090a, 0, 8)?;
    usb.read(0x92, v92, 0, 64)?;
    usb.read(0x93, level, 0, 64)?;
    usb.read(0x58, level, 0, 64)?;
    usb.read(0x94, v94, i94, 64)?;
    usb.read(0x94, v94, i94, 64)?;
    usb.read(0x95, 0, 0, 64)?;
    usb.read(0x9f, 0, 0, 64)?;
    usb.read(0x9c, v9c, 0, 64)?;
    usb.read(0x9d, v9d, 0, 64)?;
    usb.read(0x0e, 0, 0, 64)?;
    usb.send(&generator_frequency(f, 0x0080))?;
    usb.read(0x04, 0x090a, 0, 8)?;
    usb.read(0x92, v92, 0, 64)?;
    usb.read(0x94, v94, i94, 64)?;
    usb.read(0x95, 0, 0, 64)?;
    usb.read(0x58, level, 0, 64)?;
    usb.read(0x66, 0, 0, 64)?;
    usb.read(0x9e, 0, 0, 64)?;
    // This request starts the modulation; the unit then often stalls its status stage, which is not an error here.
    if let Err(e) = usb.read(0x90, 0x000a, 0, 64) {
        eprintln!("start request: {e} (expected while the unit modulates)");
    }
    Ok(())
}
