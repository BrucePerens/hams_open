// SPDX-License-Identifier: AGPL-3.0-or-later
//! `raw58 <word>`: the generator level request (0x58) with a raw 14-bit amplitude word, for scanning the amplitude response of a unit. The generator must be on (`generator on`).
use vnwa_open::usb::Vnwa;

fn main() -> Result<(), String> {
    let w: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or("usage: raw58 <word 0..16383>")?;
    Vnwa::open()?.read(0x58, w, 0, 64).map(|_| ())
}
