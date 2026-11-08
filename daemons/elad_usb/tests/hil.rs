// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-in-the-loop tests: run only with an ELAD receiver attached. Read-only: discovery and control reads; no test starts a
//! stream or tunes. Run with `cargo test --test hil -- --ignored --nocapture --test-threads=1`.

use elad_usb::fpga::FpgaState;
use elad_usb::radio::{OpenOptions, Radio};
use elad_usb::usb::{find, UsbTransport};

#[test]
#[ignore = "needs an ELAD receiver on the bus"]
fn hil_find_and_open_reads_firmware_and_calibration() {
    // Tests [@ANCHOR: elad_usb_find]
    // Tests [@ANCHOR: elad_usb_open]
    let found = find().unwrap();
    let Some(f) = found.first() else {
        eprintln!("no ELAD receiver on the bus; nothing to test");
        return;
    };
    let t = UsbTransport::open(f).unwrap();
    let r = Radio::open(
        t,
        f.model,
        OpenOptions {
            fpga: FpgaState::Unknown,
            sample_rate_hz: 192_000,
        },
    )
    .unwrap();
    assert!(
        r.info().firmware.is_some(),
        "a unit that answers a firmware request"
    );
}
