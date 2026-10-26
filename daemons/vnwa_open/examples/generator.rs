// SPDX-License-Identifier: AGPL-3.0-or-later
//! `generator on <freq_hz> <attenuation_db>` | `generator retune <freq_hz>` | `generator level <attenuation_db>` | `generator off`:
//! drives the CW generator on the TX port, to check it against a receiver.
use vnwa_open::generator;
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize| -> Result<f64, String> {
        a.get(i)
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("argument {i} must be a number"))
    };
    let usb = Vnwa::open()?;
    match a.get(1).map(String::as_str) {
        Some("on") => generator::on(&usb, num(2)?, num(3)?),
        Some("retune") => generator::retune(&usb, num(2)?),
        Some("level") => generator::set_attenuation(&usb, num(2)?),
        Some("off") => generator::off(&usb),
        _ => Err("usage: generator on <hz> <db> | retune <hz> | level <db> | off".into()),
    }
}
