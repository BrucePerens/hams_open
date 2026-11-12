// SPDX-License-Identifier: AGPL-3.0-or-later
//! The Elektor SDR-USB engine against its virtual twin: the FTDI set-up transfers, the I2C pin
//! samples that program the clock chip, tuning, the attenuator, through the real rig actor and the
//! USB link over the fake bus. Nothing here touches a USB device or a radio.
#![allow(clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use radioctl_core::backoff::Backoff;
use radioctl_core::description::RigDescription;
use radioctl_core::engine::{Inbound, ProtocolEngine};
use radioctl_core::op::Op;
use radioctl_core::rig::{spawn, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Level, Vfo};
use radioctl_core::usb_device::{FakeTransfer, FakeUsbBackend, UsbDevice};
use radioctl_core::usb_link::{open_usb, UsbConfig};
use radioctl_core::usb_wire::{UsbReply, UsbStatus};
use radioctl_protocols::elektor507::{
    Elektor507Engine, Elektor507Twin, Pll, BAUD_VALUE, BITMODE_VALUE, BULK_OUT, FTDI_BAUD,
    FTDI_BITMODE, FTDI_RESET, OSCILLATOR_HZ, REG_CLKOE, REG_DIV1, REG_DIV2, REG_PUMP, VENDOR_OUT,
};
use radioctl_protocols::usb_family::{target_for, twin_for};
use tokio::runtime::Handle;

fn description() -> RigDescription {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/elektor-sdr-usb.toml");
    let text = std::fs::read_to_string(path).expect("read");
    rigfile::parse(&text).expect("parses").description
}

const T: Duration = Duration::from_millis(500);

/// Opens a twin the way the engine's open requests do, by running them on the device.
fn opened() -> (Elektor507Engine, Elektor507Twin) {
    let mut engine = Elektor507Engine::new(description()).expect("engine");
    let mut twin = Elektor507Twin::new(&description()).expect("twin");
    for request in engine.open_requests() {
        let reply = radioctl_core::usb_wire::execute(
            &mut twin,
            &radioctl_core::usb_wire::UsbTransfer::decode(&request.bytes)
                .expect("frame")
                .0,
            T,
        );
        assert_eq!(reply.status, UsbStatus::Ok);
    }
    (engine, twin)
}

fn run(engine: &mut Elektor507Engine, twin: &mut Elektor507Twin, op: &Op) {
    let request = engine.encode(op).expect("encode");
    let transfer = radioctl_core::usb_wire::UsbTransfer::decode(&request.bytes)
        .expect("frame")
        .0;
    let reply = radioctl_core::usb_wire::execute(twin, &transfer, T);
    assert_eq!(
        engine.feed(&reply.encode(), Duration::ZERO),
        vec![Inbound::Ack]
    );
}

// Tests [@ANCHOR: radioctl_elektor507_engine_new]
// Tests [@ANCHOR: radioctl_elektor507_engine_setup]
// Tests [@ANCHOR: radioctl_elektor507_engine_open_requests]
// Tests [@ANCHOR: radioctl_elektor507_engine_link_restored]
// Tests [@ANCHOR: radioctl_elektor507_twin_new]
// Tests [@ANCHOR: radioctl_elektor507_twin_control_out]
// Tests [@ANCHOR: radioctl_elektor507_twin_bulk_write]
// Tests [@ANCHOR: radioctl_elektor507_twin_register]
#[test]
fn opening_sets_up_the_ftdi_chip_then_the_clock_chip() {
    let mut engine = Elektor507Engine::new(description()).expect("engine");
    let open = engine.open_requests();
    assert_eq!(open.len(), 4);
    // The three vendor requests: reset, bit-bang with every pin an output, the baud rate.
    let control = |request: u8, value: u16| {
        radioctl_core::usb_wire::UsbTransfer::ControlOut {
            request_type: VENDOR_OUT,
            request,
            value,
            index: 0,
            data: Vec::new(),
        }
        .encode()
    };
    assert_eq!(open[0].bytes, control(FTDI_RESET, 0));
    assert_eq!(open[1].bytes, control(FTDI_BITMODE, BITMODE_VALUE));
    assert_eq!(open[2].bytes, control(FTDI_BAUD, BAUD_VALUE));
    assert_eq!(BITMODE_VALUE, 0x01FF);
    assert_eq!(BAUD_VALUE, 49230);
    // The clock chip's initial registers, as the twin decodes them.
    let (_, twin) = opened();
    assert_eq!(twin.register(REG_CLKOE), 0x20);
    assert_eq!(twin.register(REG_DIV1), 8);
    assert_eq!(
        [
            twin.register(0x44),
            twin.register(0x45),
            twin.register(0x46)
        ],
        [0x02, 0x8E, 0x47]
    );
    assert_eq!(twin.register(REG_DIV2), 0x88);
    assert_eq!(twin.frequency(), None);
    // A restored link sets everything up again.
    assert_eq!(engine.link_restored().len(), 4);
    assert!(Elektor507Engine::new(
        rigfile::parse(
            &std::fs::read_to_string(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rigs/fifi-sdr.toml")
            )
            .expect("read")
        )
        .expect("parses")
        .description
    )
    .is_err());
    // The twin refuses what the board does not take.
    let mut stray = Elektor507Twin::new(&description()).expect("twin");
    assert!(stray.bulk_write(0x81, &[0], T).is_err());
    assert!(stray.control_out(VENDOR_OUT, 9, 0, 0, &[], T).is_err());
}

// Tests [@ANCHOR: radioctl_elektor507_pll_solve]
// Tests [@ANCHOR: radioctl_elektor507_pll_frequency]
#[test]
fn the_solver_lands_within_a_few_hertz_of_every_band() {
    for hz in [
        30_000u64, 100_000, 1_600_000, 3_573_000, 7_074_000, 10_136_000, 14_074_000, 28_074_000,
        29_999_000,
    ] {
        let pll = Pll::SAFE.solve(hz);
        assert!((2..=127).contains(&pll.div), "{hz}: {pll:?}");
        assert!((2..=40).contains(&pll.q), "{hz}: {pll:?}");
        let vco = f64::from(pll.p) * f64::from(OSCILLATOR_HZ) / f64::from(pll.q);
        // Hamlib truncates the pump bounds, so the low end may fall one reference step short.
        assert!((95e6..=400e6).contains(&vco), "{hz}: {pll:?}");
        // Above 200 kHz the quadrature frequency is within 0.01 percent.
        if hz >= 200_000 {
            let error = (pll.frequency() - hz as f64).abs();
            assert!(
                error <= hz as f64 / 10_000.0,
                "{hz}: {pll:?} off by {error}"
            );
        }
    }
}

// Tests [@ANCHOR: radioctl_elektor507_pll_registers]
#[test]
fn the_register_values_follow_the_pump_ranges_and_the_fixed_dividers() {
    // P = 1001 (odd, pump range 4), Q = 7, divider 2 and 3 use the fixed options.
    let [pump, div1, source] = (Pll {
        p: 1001,
        q: 7,
        div: 2,
    })
    .registers();
    assert_eq!(
        pump,
        (
            REG_PUMP,
            vec![
                ((496u32 >> 8) as u8) | (4 << 2) | 0xC0,
                (496 & 0xFF) as u8,
                5 | 0x80
            ]
        )
    );
    assert_eq!(div1, (REG_DIV1, vec![8]));
    assert_eq!(source, (0x46, vec![0x87]));
    let [_, div1, source] = (Pll {
        p: 100,
        q: 2,
        div: 3,
    })
    .registers();
    assert_eq!((div1.1, source.1), (vec![6], vec![0xC7]));
    let [pump, div1, source] = (Pll {
        p: 40,
        q: 2,
        div: 50,
    })
    .registers();
    assert_eq!(pump.1, vec![0xC0, 16, 0]);
    assert_eq!((div1.1, source.1), (vec![50], vec![0x47]));
    // The pump range table's boundaries.
    for (p, range) in [
        (44u32, 0u8),
        (45, 1),
        (479, 1),
        (480, 2),
        (639, 2),
        (640, 3),
        (799, 3),
        (800, 4),
    ] {
        let [pump, _, _] = (Pll { p, q: 2, div: 10 }).registers();
        assert_eq!((pump.1[0] >> 2) & 7, range, "p = {p}");
    }
}

// Tests [@ANCHOR: radioctl_elektor507_samples_sda]
// Tests [@ANCHOR: radioctl_elektor507_samples_scl]
// Tests [@ANCHOR: radioctl_elektor507_samples_byte]
// Tests [@ANCHOR: radioctl_elektor507_samples_write]
// Tests [@ANCHOR: radioctl_elektor507_engine_samples]
// Tests [@ANCHOR: radioctl_elektor507_twin_sample]
// Tests [@ANCHOR: radioctl_elektor507_twin_finish]
// Tests [@ANCHOR: radioctl_elektor507_engine_encode]
// Tests [@ANCHOR: radioctl_elektor507_twin_frequency]
// Tests [@ANCHOR: radioctl_elektor507_twin_attenuation]
// Tests [@ANCHOR: radioctl_elektor507_twin_multiplexer]
#[test]
fn tuning_programs_the_chip_and_picks_the_filter_and_the_attenuator_is_two_pins() {
    let (mut engine, mut twin) = opened();
    run(
        &mut engine,
        &mut twin,
        &Op::SetFrequency(Vfo::A, Hz(7_074_000)),
    );
    let tuned = twin.frequency().expect("tuned");
    assert!(tuned.0.abs_diff(7_074_000) <= 707, "{tuned:?}");
    assert_eq!(twin.multiplexer(), 2, "high-pass above 1.6 MHz");
    run(
        &mut engine,
        &mut twin,
        &Op::SetFrequency(Vfo::A, Hz(1_600_000)),
    );
    assert_eq!(twin.multiplexer(), 1, "low-pass up to 1.6 MHz");
    for (db, pins) in [(10u16, 1u8), (20, 2), (0, 0)] {
        run(&mut engine, &mut twin, &Op::SetLevel(Level::Attenuator, db));
        assert_eq!(twin.attenuation_db(), db);
        assert_eq!(
            twin.multiplexer(),
            1,
            "the attenuator leaves the multiplexer alone"
        );
        assert_eq!(pins, (db / 10).min(2) as u8);
    }
    // The attenuator survives a retune, the new frequency lands, both lines end high.
    run(&mut engine, &mut twin, &Op::SetLevel(Level::Attenuator, 20));
    run(
        &mut engine,
        &mut twin,
        &Op::SetFrequency(Vfo::A, Hz(14_074_000)),
    );
    assert_eq!(twin.attenuation_db(), 20);
    assert!(twin.frequency().expect("tuned").0.abs_diff(14_074_000) <= 1_408);
}

// Tests [@ANCHOR: radioctl_elektor507_engine_feed]
#[test]
fn the_engine_refuses_what_the_board_cannot_do() {
    let mut engine = Elektor507Engine::new(description()).expect("engine");
    for op in [
        Op::GetFrequency(Vfo::A),
        Op::GetLevel(Level::Attenuator),
        Op::SetLevel(Level::Attenuator, 15),
        Op::SetLevel(Level::Preamp, 1),
        Op::SetFrequency(Vfo::A, Hz(29_000)),
        Op::SetFrequency(Vfo::A, Hz(30_000_000)),
        Op::SetPtt(true),
    ] {
        assert!(engine.encode(&op).is_err(), "{op:?}");
    }
    // A failed transfer is a refusal; silence is silence.
    let stall = UsbReply {
        op: 3,
        status: UsbStatus::Stall,
        data: Vec::new(),
    };
    assert_eq!(
        engine.feed(&stall.encode(), Duration::ZERO),
        vec![Inbound::Nak]
    );
    let timeout = UsbReply {
        op: 3,
        status: UsbStatus::Timeout,
        data: Vec::new(),
    };
    assert!(engine.feed(&timeout.encode(), Duration::ZERO).is_empty());
}

// Tests [@ANCHOR: radioctl_usb_family_engine_for]
// Tests [@ANCHOR: radioctl_usb_family_twin_for]
#[tokio::test]
async fn a_rig_over_the_link_and_the_fake_bus_sets_up_and_tunes() {
    let desc = description();
    let bus = FakeUsbBackend::new(twin_for(&desc).expect("twin"));
    let mut config = UsbConfig::new(target_for(&desc).expect("target"));
    config.transfer_timeout = T;
    config.backoff = Backoff {
        initial: Duration::from_millis(20),
        max: Duration::from_millis(80),
        factor: 2,
    };
    let link = open_usb(&Handle::current(), Arc::new(bus.clone()), config)
        .await
        .expect("link");
    let (stream, monitor) = link.into_parts();
    let mut options = RigOptions::default();
    options.poll.enabled = false;
    options.link_watch = Some(monitor.watch());
    let engine = radioctl_protocols::usb_family::engine_for(&desc).expect("engine");
    std::mem::forget(monitor);
    let rig = spawn(&Handle::current(), engine, stream, options).expect("spawn");
    rig.set_frequency(Vfo::A, Hz(14_200_000))
        .await
        .expect("set");
    let log = bus.log();
    assert_eq!(
        log.first(),
        Some(&FakeTransfer::ControlOut(
            VENDOR_OUT,
            FTDI_RESET,
            0,
            0,
            Vec::new()
        ))
    );
    assert!(log.contains(&FakeTransfer::ControlOut(
        VENDOR_OUT,
        FTDI_BITMODE,
        BITMODE_VALUE,
        0,
        Vec::new()
    )));
    let writes = log
        .iter()
        .filter(|t| matches!(t, FakeTransfer::BulkWrite(e, _) if *e == BULK_OUT))
        .count();
    assert_eq!(writes, 2, "the chip set-up, then the tuning");
}
