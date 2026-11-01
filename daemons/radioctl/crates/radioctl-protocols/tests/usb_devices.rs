// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every `usb-device` description (Si570 kits, FUNcube, FiFi-SDR, Elektor SDR-USB, DWT) against the
//! facts it must carry, and the refusal that stands in for the missing engine: radioctl has no USB
//! control or HID transport, so these models are described but cannot be opened. Nothing here
//! touches a radio.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;

use radioctl_core::description::{Family, RigDescription, UsbProtocol};
use radioctl_core::error::ErrorKind;
use radioctl_core::rigfile;
use radioctl_protocols::conformance::engine_for;

fn usb_descriptions() -> Vec<RigDescription> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).expect("rigs") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = std::fs::read_to_string(&path).expect("read");
            if text.contains("family = \"usb-device\"") {
                out.push(rigfile::parse(&text).expect("parses").description);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

// Tests [@ANCHOR: radioctl_core_rigfile_build_usb]
// Tests [@ANCHOR: radioctl_core_usbprotocol_name]
// Tests [@ANCHOR: radioctl_core_usbprotocol_from_name]
#[test]
fn every_usb_file_carries_its_ids_and_a_known_protocol() {
    let all = usb_descriptions();
    assert_eq!(
        all.len(),
        10,
        "{:?}",
        all.iter().map(|d| &d.id).collect::<Vec<_>>()
    );
    for d in &all {
        assert_eq!(d.family, Family::UsbDevice, "{}", d.id);
        let usb = d.usb.as_ref().expect("usb block");
        assert_eq!(
            UsbProtocol::from_name(usb.protocol.name()),
            Some(usb.protocol)
        );
        assert_ne!(usb.vendor_id, 0, "{}", d.id);
        assert!(
            d.civ.is_none() && d.vendor.is_none() && d.receiver.is_none(),
            "{}",
            d.id
        );
        // No wire protocol can be exercised, so no description may claim more than frequency and PTT.
        assert!(
            d.mode_codes.is_empty(),
            "{} has mode codes without an engine",
            d.id
        );
    }
    let by = |id: &str| {
        all.iter()
            .find(|d| d.id == id)
            .and_then(|d| d.usb.clone())
            .expect("present")
    };
    // The Si570 boards share the V-USB id and differ by strings and multiplier.
    let avr = by("softrock-si570-avr-usb");
    let pic = by("kth-si570-pic-usb");
    let v1 = by("ae9rb-peaberry-v1");
    assert_eq!((avr.vendor_id, avr.product_id), (0x16C0, 0x05DC));
    assert_eq!((pic.product.as_str(), pic.multiplier), ("KTH-SDR-KIT", 2));
    assert_eq!((v1.manufacturer.as_str(), v1.multiplier), ("AE9RB", 4));
    let funcube = by("amsat-uk-funcube-dongle");
    let plus = by("amsat-uk-funcube-dongle-pro-plus");
    assert_eq!((funcube.vendor_id, funcube.product_id), (0x04D8, 0xFB56));
    assert_eq!((plus.vendor_id, plus.product_id), (0x04D8, 0xFB31));
    assert_eq!(UsbProtocol::from_name("nope"), None);
}

// Tests [@ANCHOR: radioctl_conformance_engine_for]
#[test]
fn a_usb_device_has_no_engine_and_says_why() {
    for d in usb_descriptions() {
        let error = engine_for(&d)
            .map(|_| ())
            .expect_err("no engine for a USB device");
        assert_eq!(error.kind, ErrorKind::Unsupported, "{}", d.id);
        assert!(
            error.detail.contains("no USB control or HID transport"),
            "{}",
            error.detail
        );
    }
}
