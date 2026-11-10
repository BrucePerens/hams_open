// SPDX-License-Identifier: AGPL-3.0-or-later
//! Every `usb-device` description (Si570 kits, FUNcube, FiFi-SDR, Elektor SDR-USB, DWT) against the
//! facts it must carry; the Si570 and FUNcube engines against their virtual twins over the fake USB
//! backend, through the real rig actor and the USB link; and the two models that still have no engine.
//! Nothing here touches a USB device or a radio.
#![allow(clippy::expect_used, clippy::panic)]

use radioctl_core::description::{Family, RigDescription, UsbProtocol};
use std::path::Path;

#[path = "../../../testdir.rs"]
mod testdir;
use std::sync::Arc;
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_core::error::ErrorKind;
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn, Freshness, RigOptions, TransmitPermit};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_core::usb_device::{FakeTransfer, FakeUsbBackend};
use radioctl_core::usb_link::{open_usb, UsbConfig};
use radioctl_protocols::conformance::engine_for;
use radioctl_protocols::funcube::{command_report, FuncubeEngine, FuncubeTwin};
use radioctl_protocols::si570_usb::{
    hz_from_registers, hz_from_word, registers_from_hz, word_from_hz, Si570UsbEngine,
};
use radioctl_protocols::usb_family::{target_for, twin_for, UsbFrameStandIn};
use tokio::runtime::Handle;

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
        12,
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
        // The wire protocols carry frequency and PTT only: no description may claim a mode.
        assert!(d.mode_codes.is_empty(), "{} has mode codes", d.id);
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

fn by_id(id: &str) -> RigDescription {
    usb_descriptions()
        .into_iter()
        .find(|d| d.id == id)
        .expect("description")
}

fn fast(desc: &RigDescription) -> UsbConfig {
    let mut config = UsbConfig::new(target_for(desc).expect("target"));
    config.transfer_timeout = Duration::from_millis(500);
    config.backoff = Backoff {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(80),
        factor: 2,
    };
    config
}

/// A rig over the USB link and the fake bus carrying the description's own twin.
async fn rig_on_fake_bus(desc: &RigDescription) -> (radioctl_core::rig::RigHandle, FakeUsbBackend) {
    let bus = FakeUsbBackend::new(twin_for(desc).expect("twin"));
    let link = open_usb(&Handle::current(), Arc::new(bus.clone()), fast(desc))
        .await
        .expect("link");
    let (stream, monitor) = link.into_parts();
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.link_watch = Some(monitor.watch());
    let engine = radioctl_protocols::usb_family::engine_for(desc).expect("engine");
    // The monitor stays alive for the life of the rig.
    std::mem::forget(monitor);
    (
        spawn(&Handle::current(), engine, stream, options).expect("spawn"),
        bus,
    )
}

// Tests [@ANCHOR: radioctl_si570_usb_word_from_hz]
// Tests [@ANCHOR: radioctl_si570_usb_hz_from_word]
#[test]
fn the_si570_word_is_eleven_dot_twenty_one_fixed_point_of_the_oscillator_frequency() {
    // 7.074 MHz on a x4 board is 28.296 MHz at the oscillator: 59341013 units of 2^-21 MHz.
    assert_eq!(word_from_hz(Hz(7_074_000), 4), Some(59_341_013));
    assert_eq!(word_from_hz(Hz(14_074_000), 2), Some(59_030_634));
    assert_eq!(hz_from_word(59_341_013, 4), Some(Hz(7_074_000)));
    assert_eq!(hz_from_word(1, 0), None);
    // The word is 32 bits: the oscillator tops out at 2047 MHz.
    assert!(word_from_hz(Hz(511_000_000), 4).is_some());
    assert_eq!(word_from_hz(Hz(512_000_000), 4), None);
    // Every kilohertz (and some odd hertz) of the coverage survives the round trip.
    for hz in (800_000..=53_700_000u64)
        .step_by(1_000)
        .chain([7_074_123, 14_199_999])
    {
        for multiplier in [2, 4] {
            let word = word_from_hz(Hz(hz), multiplier).expect("fits");
            assert_eq!(
                hz_from_word(word, multiplier),
                Some(Hz(hz)),
                "{hz} x{multiplier}"
            );
        }
    }
}

// Tests [@ANCHOR: radioctl_si570_usb_registers_from_hz]
// Tests [@ANCHOR: radioctl_si570_usb_hz_from_registers]
#[test]
fn the_legacy_si570_registers_hold_the_solved_dividers() {
    // 7.074 MHz on a x4 board is 28.296 MHz: the lowest DCO in range is 4980.096 MHz with
    // HS_DIV 4 and N1 44, so the bytes are code 0, N1-1 = 43, RFREQ = DCO / 114.285 MHz in 10.28
    // fixed point (11697373590). Worked out by hand, not by the code under test.
    assert_eq!(
        registers_from_hz(Hz(7_074_000), 4),
        Some([0x0A, 0xC2, 0xB9, 0x37, 0xC1, 0x96])
    );
    assert_eq!(
        hz_from_registers(&[0x0A, 0xC2, 0xB9, 0x37, 0xC1, 0x96], 4),
        Some(Hz(7_074_000))
    );
    // Nothing solves a frequency whose DCO cannot reach the window, or a zero multiplier.
    assert_eq!(registers_from_hz(Hz(1_000), 4), None);
    assert_eq!(registers_from_hz(Hz(0), 4), None);
    assert_eq!(hz_from_registers(&[0; 6], 0), None);
    // The unused HS_DIV code 4 decodes to nothing.
    assert_eq!(hz_from_registers(&[0x80, 0, 0, 0, 0, 1], 4), None);
    for hz in (1_800_000..=53_700_000u64)
        .step_by(1_000)
        .chain([7_074_123, 14_199_999])
    {
        for multiplier in [2, 4] {
            let regs = registers_from_hz(Hz(hz), multiplier).expect("solvable");
            let back = hz_from_registers(&regs, multiplier).expect("decodes").0;
            // Below about 3.4 MHz at the oscillator the divider chain cannot reach the DCO
            // window, so the range starts at 1.8 MHz. RFREQ has 2^-28 resolution on a 114 MHz crystal: well under a hertz.
            assert!(back.abs_diff(hz) <= 1, "{hz} x{multiplier} -> {back}");
        }
    }
}

// Tests [@ANCHOR: radioctl_si570_usb_engine_new]
// Tests [@ANCHOR: radioctl_si570_usb_set_value]
// Tests [@ANCHOR: radioctl_si570_usb_engine_encode]
// Tests [@ANCHOR: radioctl_si570_usb_engine_feed]
// Tests [@ANCHOR: radioctl_usb_family_engine_for]
#[test]
fn the_si570_engine_encodes_the_documented_requests() {
    use radioctl_core::engine::ProtocolEngine;
    let mut avr = Si570UsbEngine::new(by_id("softrock-si570-avr-usb")).expect("engine");
    let set = avr
        .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_000)))
        .expect("set");
    // Control out, request 0x32, wValue 0x755 (0x700 + the Si570's I2C address), wIndex 0, the word LE.
    assert_eq!(
        set.bytes,
        vec![2, 0x40, 0x32, 0x55, 0x07, 0, 0, 4, 0, 0xD5, 0x78, 0x89, 0x03]
    );
    let get = avr.encode(&Op::GetFrequency(Vfo::A)).expect("get");
    assert_eq!(get.bytes, vec![1, 0xC0, 0x3A, 0, 0, 0, 0, 4, 0]);
    // PTT is request 0x50, direction in, wValue 1 or 0, three bytes back.
    let key = avr.encode(&Op::SetPtt(true)).expect("ptt");
    assert_eq!(key.bytes, vec![1, 0xC0, 0x50, 1, 0, 0, 0, 3, 0]);
    // The FiFi-SDR sets with wValue 0 and has no PTT; the KTH board multiplies by two.
    let mut fifi = Si570UsbEngine::new(by_id("fifi-sdr")).expect("engine");
    let set = fifi
        .encode(&Op::SetFrequency(Vfo::A, Hz(7_074_000)))
        .expect("set");
    assert_eq!(&set.bytes[..7], &[2, 0x40, 0x32, 0, 0, 0, 0]);
    assert!(fifi.encode(&Op::SetPtt(true)).is_err());
    let mut kth = Si570UsbEngine::new(by_id("kth-si570-pic-usb")).expect("engine");
    assert!(
        kth.encode(&Op::SetPtt(true)).is_err(),
        "the KTH description lists no PTT"
    );
    // No mode, and a frequency the oscillator cannot reach, never reach the wire.
    assert!(avr.encode(&Op::GetMode).is_err());
    assert!(avr
        .encode(&Op::SetFrequency(Vfo::A, Hz(600_000_000)))
        .is_err());
    // A FUNcube description is not an Si570 one.
    assert!(Si570UsbEngine::new(by_id("amsat-uk-funcube-dongle")).is_err());
    assert!(radioctl_protocols::usb_family::engine_for(&by_id("fifi-sdr")).is_ok());
}

// Tests [@ANCHOR: radioctl_si570_usb_twin_new]
// Tests [@ANCHOR: radioctl_si570_usb_twin_ptt]
// Tests [@ANCHOR: radioctl_si570_usb_twin_control]
// Tests [@ANCHOR: radioctl_usb_family_twin_for]
// Tests [@ANCHOR: radioctl_usb_family_usb_of]
// Tests [@ANCHOR: radioctl_si570_usb_twin_control_out]
// Tests [@ANCHOR: radioctl_usb_family_target_for]
// Tests [@ANCHOR: radioctl_core_fake_usb_handle_transfers]
#[tokio::test]
async fn every_si570_board_tunes_through_the_link_and_the_closed_gate_keeps_the_ptt_request_back() {
    let boards = [
        "softrock-si570-avr-usb",
        "softrock-si570-avr-usb-legacy",
        "kth-si570-pic-usb",
        "ae9rb-peaberry-v1",
        "ae9rb-peaberry-v2",
        "funkamateur-fa-sdr",
        "fifi-sdr",
    ];
    for id in boards {
        let desc = by_id(id);
        let (rig, bus) = rig_on_fake_bus(&desc).await;
        assert_eq!(
            rig.frequency(Vfo::A, Freshness::Fresh)
                .await
                .expect("read")
                .value,
            desc.default_frequency,
            "{id}"
        );
        rig.set_frequency(Vfo::A, Hz(14_205_000))
            .await
            .expect("set");
        let read = rig
            .frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("read")
            .value;
        // The legacy register word has finite resolution; the by-value word is exact here.
        assert!(read.0.abs_diff(14_205_000) <= 1, "{id}: {read:?}");
        if id.ends_with("-legacy") {
            assert!(
                bus.log()
                    .iter()
                    .any(|t| matches!(t, FakeTransfer::ControlOut(0x40, 0x30, 0x755, 0, _))),
                "{id}: no request 0x30"
            );
            assert!(
                !bus.log()
                    .iter()
                    .any(|t| matches!(t, FakeTransfer::ControlOut(_, 0x32, ..))),
                "{id}: legacy firmware got request 0x32"
            );
        }
        if desc
            .capabilities
            .has(radioctl_core::capability::Capability::SetPtt)
        {
            let refused = rig.set_ptt(true).await.expect_err("gate closed");
            assert_eq!(refused.kind, ErrorKind::TransmitGateClosed, "{id}");
            assert!(
                !bus.log()
                    .iter()
                    .any(|t| matches!(t, FakeTransfer::ControlIn(_, 0x50, 1, _, _))),
                "{id}: a key request reached the device"
            );
            rig.open_transmit_gate(TransmitPermit::new("usb test"));
            rig.set_ptt(true).await.expect("keys");
            rig.set_ptt(false).await.expect("unkeys");
            assert!(
                bus.log()
                    .contains(&FakeTransfer::ControlIn(0xC0, 0x50, 1, 0, 3)),
                "{id}"
            );
        }
    }
}

// Tests [@ANCHOR: radioctl_funcube_command_report]
// Tests [@ANCHOR: radioctl_funcube_engine_new]
// Tests [@ANCHOR: radioctl_funcube_exchange]
// Tests [@ANCHOR: radioctl_funcube_engine_encode]
#[test]
fn the_funcube_engine_exchanges_sixty_four_byte_reports() {
    use radioctl_core::engine::ProtocolEngine;
    let report = command_report(0x65, &[1, 2, 3, 4]);
    assert_eq!(report.len(), 64);
    assert_eq!(&report[..6], &[0x65, 1, 2, 3, 4, 0]);
    let mut engine = FuncubeEngine::new(by_id("amsat-uk-funcube-dongle")).expect("engine");
    let set = engine
        .encode(&Op::SetFrequency(Vfo::A, Hz(145_800_000)))
        .expect("set");
    // Interrupt exchange: OUT 0x02, IN 0x82, 64 bytes each way, 0x65 and the hertz LE.
    assert_eq!(&set.bytes[..9], &[7, 0x02, 0x82, 64, 0, 0, 0, 64, 0]);
    assert_eq!(&set.bytes[9..14], &[0x65, 0x40, 0xBB, 0xB0, 0x08]);
    let get = engine.encode(&Op::GetFrequency(Vfo::A)).expect("get");
    assert_eq!(set.bytes.len(), get.bytes.len());
    assert_eq!(get.bytes[9], 0x66);
    assert!(engine.encode(&Op::SetPtt(true)).is_err());
    assert!(engine
        .encode(&Op::SetFrequency(Vfo::A, Hz(5_000_000_000)))
        .is_err());
    assert!(FuncubeEngine::new(by_id("fifi-sdr")).is_err());
}

// Tests [@ANCHOR: radioctl_funcube_engine_feed]
// Tests [@ANCHOR: radioctl_funcube_twin_new]
// Tests [@ANCHOR: radioctl_funcube_twin_interrupt]
// Tests [@ANCHOR: radioctl_funcube_twin_interrupt_read]
// Tests [@ANCHOR: radioctl_funcube_twin_frequency]
#[tokio::test]
async fn both_funcube_dongles_tune_and_read_back_through_the_link() {
    for id in [
        "amsat-uk-funcube-dongle",
        "amsat-uk-funcube-dongle-pro-plus",
    ] {
        let desc = by_id(id);
        assert_eq!(
            target_for(&desc).expect("target").interface,
            Some(2),
            "{id}"
        );
        let (rig, bus) = rig_on_fake_bus(&desc).await;
        rig.set_frequency(Vfo::A, Hz(145_800_000))
            .await
            .expect("set");
        assert_eq!(
            rig.frequency(Vfo::A, Freshness::Fresh)
                .await
                .expect("read")
                .value,
            Hz(145_800_000),
            "{id}"
        );
        assert!(bus.log().iter().any(
            |t| matches!(t, FakeTransfer::InterruptWrite(0x02, d) if d.first() == Some(&0x65))
        ));
        assert!(bus
            .log()
            .iter()
            .any(|t| matches!(t, FakeTransfer::InterruptRead(0x82, 64))));
    }
    let twin = FuncubeTwin::new(&by_id("amsat-uk-funcube-dongle"), false).expect("twin");
    assert_eq!(twin.frequency(), Hz(7_074_000));
}

#[tokio::test]
async fn an_older_funcube_refuses_the_hertz_command_and_the_engine_falls_back_to_kilohertz() {
    let desc = by_id("amsat-uk-funcube-dongle");
    let bus = FakeUsbBackend::new(Box::new(FuncubeTwin::new(&desc, true).expect("twin")));
    let link = open_usb(&Handle::current(), Arc::new(bus.clone()), fast(&desc))
        .await
        .expect("link");
    let (stream, monitor) = link.into_parts();
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    let rig = spawn(
        &Handle::current(),
        FuncubeEngine::new(desc).expect("engine"),
        stream,
        options,
    )
    .expect("spawn");
    let first = rig
        .set_frequency(Vfo::A, Hz(145_800_000))
        .await
        .expect_err("refused");
    assert_eq!(first.kind, ErrorKind::Rejected);
    rig.set_frequency(Vfo::A, Hz(145_800_000))
        .await
        .expect("kilohertz command");
    assert!(bus.log().iter().any(|t| matches!(t, FakeTransfer::InterruptWrite(0x02, d) if d.get(..4) == Some(&[0x64, 0x88, 0x39, 0x02]))));
    // Old firmware cannot be read back.
    let read = rig
        .frequency(Vfo::A, Freshness::Fresh)
        .await
        .expect_err("no read");
    assert_eq!(read.kind, ErrorKind::Unsupported);
    drop(monitor);
}

// Tests [@ANCHOR: radioctl_usb_family_stand_in_new]
// Tests [@ANCHOR: radioctl_usb_family_stand_in_feed]
// Tests [@ANCHOR: radioctl_conformance_engine_for]
#[test]
fn the_conformance_stand_in_answers_the_frames_the_engines_encode() {
    use radioctl_core::engine::{Inbound, ProtocolEngine};
    use radioctl_core::op::Observation;
    for d in usb_descriptions() {
        let Ok(mut engine) = engine_for(&d) else {
            continue;
        };
        let mut device = UsbFrameStandIn::new(&d).expect("stand-in");
        let set = engine
            .encode(&Op::SetFrequency(Vfo::A, Hz(10_136_000)))
            .expect("set");
        let ack = device.feed(&set.bytes);
        assert_eq!(
            engine.feed(&ack, Duration::ZERO),
            vec![Inbound::Ack],
            "{}",
            d.id
        );
        if matches!(
            d.id.as_str(),
            "digital-world-traveller" | "microtelecom-perseus-usb" | "elektor-sdr-usb"
        ) {
            // Set only: there is no read on the wire.
            assert!(engine.encode(&Op::GetFrequency(Vfo::A)).is_err());
            continue;
        }
        let get = engine.encode(&Op::GetFrequency(Vfo::A)).expect("get");
        let reply = device.feed(&get.bytes);
        assert_eq!(
            engine.feed(&reply, Duration::ZERO),
            vec![Inbound::Observation(Observation::Frequency(
                Vfo::A,
                Hz(10_136_000)
            ))],
            "{}",
            d.id
        );
        // Garbage in the request stream gets a failure reply, not silence or a panic.
        let bad = device.feed(&[99, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(!bad.is_empty());
    }
}

// Tests [@ANCHOR: radioctl_dwt_engine_new]
// Tests [@ANCHOR: radioctl_dwt_engine_encode]
// Tests [@ANCHOR: radioctl_dwt_engine_feed]
// Tests [@ANCHOR: radioctl_dwt_twin_khz]
// Tests [@ANCHOR: radioctl_dwt_twin_control_out]
#[tokio::test]
async fn the_traveller_is_tuned_in_whole_kilohertz_and_cannot_be_read() {
    use radioctl_core::engine::ProtocolEngine;
    let desc = by_id("digital-world-traveller");
    let mut engine = radioctl_protocols::dwt::DwtEngine::new(desc.clone()).expect("engine");
    let set = engine
        .encode(&Op::SetFrequency(Vfo::A, Hz(9_620_500)))
        .expect("set");
    // 9620 kHz = 0x2594; request type 0, request 0, nine bytes.
    assert_eq!(
        set.bytes,
        vec![2, 0, 0, 0, 0, 0, 0, 9, 0, 0x4A, 0, 3, 0, 0xFF, 0xFF, 0x32, 0x25, 0x94]
    );
    assert!(
        engine
            .encode(&Op::SetFrequency(Vfo::A, Hz(98_000_000)))
            .is_err(),
        "FM band does not fit 16 bits"
    );
    assert!(engine.encode(&Op::GetFrequency(Vfo::A)).is_err());
    assert!(radioctl_protocols::dwt::DwtEngine::new(by_id("fifi-sdr")).is_err());
    let (rig, bus) = rig_on_fake_bus(&desc).await;
    rig.set_frequency(Vfo::A, Hz(15_300_000))
        .await
        .expect("set");
    assert!(bus.log().contains(&FakeTransfer::ControlOut(
        0,
        0,
        0,
        0,
        vec![0x4A, 0, 3, 0, 0xFF, 0xFF, 0x32, 0x3B, 0xC4]
    )));
    assert!(radioctl_protocols::dwt::DwtTwin::default().khz().is_none());
}

#[test]
fn a_bare_si570_twin_follows_the_ptt_request() {
    use radioctl_core::usb_device::UsbDevice;
    use radioctl_protocols::si570_usb::Si570Twin;
    let mut twin = Si570Twin::new(&by_id("softrock-si570-avr-usb")).expect("twin");
    assert!(!twin.ptt());
    twin.control_in(0xC0, 0x50, 1, 0, 3, Duration::from_millis(10))
        .expect("key");
    assert!(twin.ptt());
    twin.control_in(0xC0, 0x50, 0, 0, 3, Duration::from_millis(10))
        .expect("unkey");
    assert!(!twin.ptt());
    // The FiFi-SDR has no PTT request.
    let mut fifi = Si570Twin::new(&by_id("fifi-sdr")).expect("twin");
    assert!(fifi
        .control_in(0xC0, 0x50, 1, 0, 3, Duration::from_millis(10))
        .is_err());
}

fn synthetic_firmware(name: &str, fpga: bool) -> testdir::TestDir {
    // Synthetic stand-ins, nothing from Microtelecom: two valid Intel HEX records and an
    // end-of-file record, and a few arbitrary bytes for the bitstream.
    let dir = testdir::TestDir::new(&format!("perseus-it-{name}"));
    std::fs::write(
        dir.join("fx2.hex"),
        ":0400000001020304F2\n:02001000AABB89\n:00000001FF\n",
    )
    .expect("hex");
    if fpga {
        std::fs::write(dir.join("fpga.rbs"), [1u8, 2, 3, 4, 5]).expect("rbs");
    }
    dir
}

// Tests [@ANCHOR: radioctl_perseus_engine_new]
// Tests [@ANCHOR: radioctl_perseus_engine_transfer]
// Tests [@ANCHOR: radioctl_perseus_engine_encode]
// Tests [@ANCHOR: radioctl_perseus_engine_feed]
// Tests [@ANCHOR: radioctl_perseus_session_new]
// Tests [@ANCHOR: radioctl_perseus_session_failure]
// Tests [@ANCHOR: radioctl_perseus_session_drop]
// Tests [@ANCHOR: radioctl_perseus_session_control_out]
// Tests [@ANCHOR: radioctl_perseus_from_device]
// Tests [@ANCHOR: radioctl_perseus_ready_virtual_new]
// Tests [@ANCHOR: radioctl_perseus_ready_virtual_board]
// Tests [@ANCHOR: radioctl_perseus_ready_virtual_open]
#[tokio::test]
async fn a_perseus_is_tuned_and_its_front_end_set_through_the_actor_and_the_usb_link() {
    use radioctl_core::units::Level;
    use radioctl_protocols::perseus::{preselector_for, tuning_word, ReadyVirtualPerseus};
    let desc = by_id("microtelecom-perseus-usb");
    let board = ReadyVirtualPerseus::new(7);
    let link = open_usb(&Handle::current(), Arc::new(board.clone()), fast(&desc))
        .await
        .expect("link");
    let (stream, monitor) = link.into_parts();
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.link_watch = Some(monitor.watch());
    let engine = radioctl_protocols::usb_family::engine_for(&desc).expect("engine");
    let rig = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    rig.set_frequency(Vfo::A, Hz(14_074_000))
        .await
        .expect("tune");
    let state = board.board().state();
    assert_eq!(state.sio.1, tuning_word(14_074_000.0));
    // The preselector follows the frequency: low nibble of Port E.
    assert_eq!(state.port_e & 0x0F, preselector_for(14_074_000.0, true));
    rig.execute(Op::SetLevel(Level::Attenuator, 20))
        .await
        .expect("att");
    assert_eq!(board.board().state().port_e >> 4, 2);
    assert!(rig
        .execute(Op::SetLevel(Level::Attenuator, 15))
        .await
        .is_err());
    rig.execute(Op::SetLevel(Level::Dither, 1))
        .await
        .expect("dither");
    rig.execute(Op::SetLevel(Level::Preamp, 1))
        .await
        .expect("preamp");
    let control = board.board().state().sio.0;
    assert_eq!(control & 0x06, 0x06, "dither and preamp bits");
    rig.execute(Op::SetLevel(Level::Dither, 0))
        .await
        .expect("dither off");
    assert_eq!(board.board().state().sio.0 & 0x06, 0x04, "preamp stays");
    // Wide-band path when the preselector is off.
    rig.execute(Op::SetLevel(Level::Preselector, 0))
        .await
        .expect("preselector");
    assert_eq!(board.board().state().port_e & 0x0F, 10);
    assert!(rig.execute(Op::SetLevel(Level::Dither, 2)).await.is_err());
    assert!(rig.execute(Op::SetLevel(Level::Preamp, 2)).await.is_err());
    assert!(rig.set_frequency(Vfo::A, Hz(41_000_000)).await.is_err());
}

// Tests [@ANCHOR: radioctl_perseus_engine_encode]
#[tokio::test]
async fn a_perseus_has_no_transmit_path_and_its_gate_stays_closed() {
    use radioctl_core::capability::Capability;
    use radioctl_core::engine::ProtocolEngine;
    let desc = by_id("microtelecom-perseus-usb");
    for capability in [Capability::SetPtt, Capability::ReadPtt, Capability::SendCw] {
        assert!(!desc.capabilities.has(capability), "{capability:?}");
    }
    let mut engine =
        radioctl_protocols::perseus_engine::PerseusEngine::new(desc.clone()).expect("engine");
    for op in [
        Op::SetPtt(true),
        Op::SetPtt(false),
        Op::SendCw("CQ".into()),
        Op::StopCw,
    ] {
        let error = engine.encode(&op).map(|_| ()).expect_err("refused");
        assert_eq!(error.kind, ErrorKind::Unsupported, "{op:?}");
    }
    let (rig, bus) = rig_on_fake_bus(&desc).await;
    assert!(rig.set_ptt(true).await.is_err(), "gate is closed");
    assert!(rig.execute(Op::SendCw("CQ".into())).await.is_err());
    // Nothing reached the device.
    assert!(bus
        .log()
        .iter()
        .all(|t| !matches!(t, FakeTransfer::BulkWrite(..))));
    assert!(radioctl_protocols::perseus_engine::PerseusEngine::new(by_id("fifi-sdr")).is_err());
}

// Tests [@ANCHOR: radioctl_perseus_loader_new]
// Tests [@ANCHOR: radioctl_perseus_loader_open]
// Tests [@ANCHOR: radioctl_perseus_virtual_new]
// Tests [@ANCHOR: radioctl_perseus_virtual_state]
#[tokio::test]
async fn the_loader_backend_loads_the_users_firmware_on_open_and_names_a_missing_file() {
    use radioctl_protocols::perseus::{PerseusFiles, PerseusLoaderBackend, VirtualPerseusBackend};
    let desc = by_id("microtelecom-perseus-usb");
    let dir = synthetic_firmware("ok", true);
    let board = VirtualPerseusBackend::new(false, 3, true);
    let loader =
        PerseusLoaderBackend::new(Arc::new(board.clone()), PerseusFiles::in_dir(&dir), false);
    let link = open_usb(&Handle::current(), Arc::new(loader), fast(&desc))
        .await
        .expect("link opens after the load");
    let state = board.state();
    assert!(state.firmware_running);
    assert_eq!(state.ram_writes.len(), 2);
    assert_eq!(state.fpga, vec![1, 2, 3, 4, 5]);
    drop(link);

    // No bitstream: the start fails and the error names the path and says where to get it.
    let dir = synthetic_firmware("nofpga", false);
    let loader = PerseusLoaderBackend::new(
        Arc::new(VirtualPerseusBackend::new(false, 3, true)),
        PerseusFiles::in_dir(&dir),
        false,
    );
    let error = open_usb(&Handle::current(), Arc::new(loader), fast(&desc))
        .await
        .map(|_| ())
        .expect_err("no fpga.rbs");
    let shown = error.detail.clone();
    assert!(shown.contains("fpga.rbs"), "{shown}");
    assert!(shown.contains(&dir.display().to_string()), "{shown}");
    assert!(shown.contains("RADIOCTL_PERSEUS_FIRMWARE_DIR"), "{shown}");
    assert!(shown.contains("libperseus-sdr"), "{shown}");

    // No firmware at all on a blank board.
    let empty = testdir::TestDir::new("perseus-it-empty");
    let loader = PerseusLoaderBackend::new(
        Arc::new(VirtualPerseusBackend::new(false, 3, true)),
        PerseusFiles::in_dir(&empty),
        false,
    );
    let error = open_usb(&Handle::current(), Arc::new(loader), fast(&desc))
        .await
        .map(|_| ())
        .expect_err("no fx2.hex");
    assert!(error.detail.contains("fx2.hex"), "{}", error.detail);
}

// Tests [@ANCHOR: radioctl_si570_usb_fifi_answer]
// Tests [@ANCHOR: radioctl_si570_usb_engine_open_requests]
#[tokio::test]
async fn the_fifisdr_takes_its_vco_factor_from_the_board_and_carries_mode_levels_and_meter() {
    use radioctl_core::op::Observation;
    use radioctl_core::units::{Level, Mode};
    use radioctl_protocols::si570_usb::Si570Twin;
    let desc = by_id("fifi-sdr");
    // A board whose VCO factor is 2 (the description says 4) and whose S-meter reads 70000.
    let mut twin = Si570Twin::new(&desc).expect("twin");
    twin.set_fifisdr_factor_and_signal(2, 70_000);
    let bus = FakeUsbBackend::new(Box::new(twin));
    let link = open_usb(&Handle::current(), Arc::new(bus.clone()), fast(&desc))
        .await
        .expect("link");
    let (stream, monitor) = link.into_parts();
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.link_watch = Some(monitor.watch());
    let engine = radioctl_protocols::usb_family::engine_for(&desc).expect("engine");
    std::mem::forget(monitor);
    let rig = spawn(&Handle::current(), engine, stream, options).expect("spawn");

    // Opening reads the VCO factor (request 0xAB, wIndex 11, four bytes) before anything else.
    rig.set_frequency(Vfo::A, Hz(14_205_000))
        .await
        .expect("set");
    let log = bus.log();
    assert_eq!(
        log.first(),
        Some(&FakeTransfer::ControlIn(0xC0, 0xAB, 0, 11, 4))
    );
    // The word on the wire is the oscillator frequency with the board's factor: 14.205 MHz x2.
    let expected = word_from_hz(Hz(14_205_000), 2)
        .expect("fits")
        .to_le_bytes()
        .to_vec();
    assert!(log.contains(&FakeTransfer::ControlOut(0x40, 0x32, 0, 0, expected)));
    assert_eq!(
        rig.frequency(Vfo::A, Freshness::Fresh)
            .await
            .expect("read")
            .value,
        Hz(14_205_000)
    );

    rig.set_mode(Mode::Am).await.expect("mode");
    assert!(log_has(
        &bus,
        FakeTransfer::ControlOut(0x40, 0xAC, 0, 15, vec![2])
    ));
    assert_eq!(
        rig.mode(Freshness::Fresh).await.expect("mode").value,
        Mode::Am
    );
    assert!(rig.set_mode(Mode::Cw).await.is_err());

    rig.execute(Op::SetFilterWidth(Hz(2_700)))
        .await
        .expect("width");
    assert!(log_has(
        &bus,
        FakeTransfer::ControlOut(0x40, 0xAC, 0, 16, 2_700u32.to_le_bytes().to_vec())
    ));
    assert_eq!(
        rig.execute(Op::GetFilterWidth).await.expect("width"),
        Some(Observation::FilterWidth(Hz(2_700)))
    );

    // Volume is a percentage, clamped at 100; the preamp is step 0 or 1 (+6 dB).
    rig.execute(Op::SetLevel(Level::AfGain, 250))
        .await
        .expect("volume");
    assert!(log_has(
        &bus,
        FakeTransfer::ControlOut(0x40, 0xAC, 0, 14, 100i16.to_le_bytes().to_vec())
    ));
    assert_eq!(
        rig.execute(Op::GetLevel(Level::AfGain))
            .await
            .expect("volume"),
        Some(Observation::Level(Level::AfGain, 100))
    );
    rig.execute(Op::SetLevel(Level::Preamp, 1))
        .await
        .expect("preamp");
    assert!(log_has(
        &bus,
        FakeTransfer::ControlOut(0x40, 0xAC, 0, 19, vec![1])
    ));
    assert_eq!(
        rig.execute(Op::GetLevel(Level::Preamp))
            .await
            .expect("preamp"),
        Some(Observation::Level(Level::Preamp, 1))
    );
    assert!(rig.execute(Op::SetLevel(Level::Preamp, 2)).await.is_err());
    assert!(rig.execute(Op::SetLevel(Level::RfGain, 1)).await.is_err());

    // The raw S-meter is capped at 65535.
    assert_eq!(
        rig.signal(Freshness::Fresh).await.expect("signal").value,
        u16::MAX
    );
}

fn log_has(bus: &FakeUsbBackend, transfer: FakeTransfer) -> bool {
    bus.log().contains(&transfer)
}

// Tests [@ANCHOR: radioctl_funcube_gain_value]
// Tests [@ANCHOR: radioctl_funcube_gain_read]
#[tokio::test]
async fn the_funcube_gain_commands_follow_the_dongle_model() {
    use radioctl_core::op::Observation;
    use radioctl_core::units::Level;
    // The original Dongle: one LNA code, steps map through Hamlib's tables, RSSI is raw.
    let desc = by_id("amsat-uk-funcube-dongle");
    let mut twin = FuncubeTwin::new(&desc, false).expect("twin");
    twin.set_rssi(83);
    let bus = FakeUsbBackend::new(Box::new(twin));
    let link = open_usb(&Handle::current(), Arc::new(bus.clone()), fast(&desc))
        .await
        .expect("link");
    let (stream, monitor) = link.into_parts();
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    std::mem::forget(monitor);
    let engine = radioctl_protocols::usb_family::engine_for(&desc).expect("engine");
    let rig = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    rig.execute(Op::SetLevel(Level::Preamp, 4))
        .await
        .expect("preamp");
    assert!(bus.log().iter().any(
        |t| matches!(t, FakeTransfer::InterruptWrite(0x02, d) if d.get(..2) == Some(&[0x6E, 12]))
    ));
    assert_eq!(
        rig.execute(Op::GetLevel(Level::Preamp))
            .await
            .expect("read"),
        Some(Observation::Level(Level::Preamp, 4))
    );
    // The attenuator (decibels) shares the one LNA setting: 5 dB is code 0, which no preamp step has.
    rig.execute(Op::SetLevel(Level::Attenuator, 5))
        .await
        .expect("att");
    assert_eq!(
        rig.execute(Op::GetLevel(Level::Attenuator))
            .await
            .expect("read"),
        Some(Observation::Level(Level::Attenuator, 5))
    );
    assert_eq!(
        rig.execute(Op::GetLevel(Level::Preamp))
            .await
            .expect("read"),
        Some(Observation::Level(Level::Preamp, 0))
    );
    assert!(rig.execute(Op::SetLevel(Level::Preamp, 7)).await.is_err());
    assert!(rig
        .execute(Op::SetLevel(Level::Attenuator, 3))
        .await
        .is_err());
    assert!(rig.execute(Op::SetLevel(Level::RfGain, 1)).await.is_err());
    assert_eq!(rig.signal(Freshness::Fresh).await.expect("rssi").value, 83);

    // The Pro+: LNA switch, mixer (20 dB of attenuator is the mixer gain off), IF gain 0 to 59.
    let desc = by_id("amsat-uk-funcube-dongle-pro-plus");
    let (rig, bus) = rig_on_fake_bus(&desc).await;
    rig.execute(Op::SetLevel(Level::Preamp, 1))
        .await
        .expect("lna");
    rig.execute(Op::SetLevel(Level::Attenuator, 20))
        .await
        .expect("mixer");
    rig.execute(Op::SetLevel(Level::RfGain, 200))
        .await
        .expect("if");
    let writes: Vec<Vec<u8>> = bus
        .log()
        .iter()
        .filter_map(|t| match t {
            FakeTransfer::InterruptWrite(0x02, d) => d.get(..2).map(<[u8]>::to_vec),
            _ => None,
        })
        .collect();
    assert_eq!(writes, vec![vec![0x6E, 1], vec![0x72, 0], vec![0x75, 59]]);
    for (level, want) in [
        (Level::Preamp, 1),
        (Level::Attenuator, 20),
        (Level::RfGain, 59),
    ] {
        assert_eq!(
            rig.execute(Op::GetLevel(level)).await.expect("read"),
            Some(Observation::Level(level, want))
        );
    }
    rig.execute(Op::SetLevel(Level::Attenuator, 0))
        .await
        .expect("mixer on");
    assert_eq!(
        rig.execute(Op::GetLevel(Level::Attenuator))
            .await
            .expect("read"),
        Some(Observation::Level(Level::Attenuator, 0))
    );
    assert!(rig.execute(Op::SetLevel(Level::Preamp, 2)).await.is_err());
    assert!(rig
        .execute(Op::SetLevel(Level::Attenuator, 10))
        .await
        .is_err());
    assert!(rig.execute(Op::GetSignal).await.is_err());
}
