// SPDX-License-Identifier: AGPL-3.0-or-later
//! `temp_now`: the unit's firmware version and internal temperature counts.
use vnwa_open::usb::Vnwa;
fn main() -> Result<(), String> {
    let u = Vnwa::open()?;
    println!(
        "firmware {:?} temperature {:?}",
        u.firmware_version(),
        u.temperature_counts()
    );
    Ok(())
}
