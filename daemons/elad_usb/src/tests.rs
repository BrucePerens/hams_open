// SPDX-License-Identifier: AGPL-3.0-or-later
use crate::convert::{Decoder, Format};
use crate::fpga::FpgaState;
use crate::protocol::*;
use crate::radio::{OpenOptions, Radio};
use crate::transport::{FakeTransport, Op};
use crate::Error;
use std::time::Duration;

fn opts(fpga: FpgaState, rate: u32) -> OpenOptions {
    OpenOptions {
        fpga,
        sample_rate_hz: rate,
    }
}

fn fake_unit() -> FakeTransport {
    let t = FakeTransport::new();
    t.reply(REQ_FW_VERSION, 0, vec![3, 0]);
    t.reply(REQ_EEPROM, 0x4000, b"SN-0000\0".to_vec());
    t.reply(REQ_EEPROM, 0x404C, vec![1, 3]);
    t.reply(REQ_EEPROM, 0x4024, (-12i32).to_le_bytes().to_vec());
    t.reply(REQ_EEPROM, 0x4028, 1.5f32.to_le_bytes().to_vec());
    t.reply(REQ_EEPROM, 0x402C, vec![0xFF; 4]);
    t.reply(REQ_EEPROM, 0x4030, (-0.5f32).to_le_bytes().to_vec());
    t
}

#[test]
fn pids_map_to_models_and_unknown_is_refused() {
    // Tests [@ANCHOR: elad_model_from_pid]
    assert_eq!(Model::from_pid(0x061a), Ok(Model::Duo));
    assert_eq!(Model::from_pid(0x061c), Ok(Model::S2));
    assert_eq!(Model::from_pid(0x0610), Ok(Model::S1));
    assert_eq!(
        Model::from_pid(0x1234),
        Err(Error::UnknownDevice { pid: 0x1234 })
    );
}

#[test]
fn tuning_word_is_the_clock_fraction_and_clamps() {
    // Tests [@ANCHOR: elad_tuning_word]
    assert_eq!(tuning_word(0.0, 122_880_000.0), 0);
    assert_eq!(tuning_word(61_440_000.0, 122_880_000.0), 0x8000_0000);
    assert_eq!(tuning_word(30_720_000.0, 122_880_000.0), 0x4000_0000);
    assert_eq!(tuning_word(-5.0, 122_880_000.0), 0);
    assert_eq!(
        tuning_word(122_880_000.0 + 30_720_000.0, 122_880_000.0),
        0x4000_0000,
        "above the clock aliases"
    );
    assert_eq!(tuning_word(7_100_000.0, 0.0), 0);
    assert_eq!(tuning_word(f64::NAN, 122_880_000.0), 0);
}

#[test]
fn tune_requests_pack_the_word_for_each_family() {
    // Tests [@ANCHOR: elad_tune_request_sampler]
    let r = tune_request_sampler(0xAABB_CCDD);
    assert_eq!(
        (r.request, r.value, r.index, r.data),
        (REQ_TUNE, 0xCCDD, 0xAABB, vec![0xAA, 0])
    );
    // Tests [@ANCHOR: elad_tune_request_duo]
    let d = tune_request_duo(0xAABB_CCDD);
    assert_eq!(
        (d.request, d.value, d.index, d.data),
        (REQ_DUO_GATEWAY, 0xCCDD, 0xF2BB, vec![0xAA, 0])
    );
}

#[test]
fn front_end_codes_follow_band_and_attenuator() {
    // Tests [@ANCHOR: elad_front_end_code]
    assert_eq!(front_end_code(false, 7e6, false), 0x01);
    assert_eq!(front_end_code(true, 7e6, false), 0x03);
    assert_eq!(front_end_code(true, 70e6, false), 0x02);
    assert_eq!(front_end_code(true, 130e6, false), 0x04);
    assert_eq!(front_end_code(true, 7e6, true), 0x0B);
    assert_eq!(front_end_code(false, 7e6, true), 0x09);
}

#[test]
fn only_receive_requests_are_allowed() {
    // Tests [@ANCHOR: elad_is_receive_request]
    for (rq, ix, ok) in [
        (REQ_FW_VERSION, 0u16, true),
        (REQ_EEPROM, 0x0151, true),
        (REQ_TUNE, 0, true),
        (REQ_FRONT_END, 2, true),
        (REQ_FIFO, 0, true),
        (REQ_DUO_GATEWAY, 0xE800, true),
        (REQ_DUO_GATEWAY, 0xF200, true),
        (REQ_DUO_GATEWAY, 0xF500, true),
        (REQ_DUO_GATEWAY, 0xFC00, true),
        (REQ_DUO_GATEWAY, 0xF100, false), // CAT write
        (0xA3, 0, false),
        (0x00, 0, false),
    ] {
        let r = Request {
            request: rq,
            value: 0,
            index: ix,
            data: vec![],
        };
        assert_eq!(is_receive_request(&r), ok, "request {rq:#x} index {ix:#x}");
    }
}

#[test]
fn eeprom_fields_decode_and_unprogrammed_fields_are_absent() {
    // Tests [@ANCHOR: elad_calibration_from_eeprom]
    let f: [Vec<u8>; 6] = [
        b"SN-0000\0junk".to_vec(),
        vec![1, 3],
        (-12i32).to_le_bytes().to_vec(),
        1.5f32.to_le_bytes().to_vec(),
        vec![0xFF; 4],
        vec![],
    ];
    let c = Calibration::from_eeprom(&f);
    assert_eq!(c.serial.as_deref(), Some("SN-0000"));
    assert_eq!(c.hardware, Some((1, 3)));
    assert_eq!(c.rate_correction_hz, Some(-12));
    assert_eq!(c.global_db, Some(1.5));
    assert_eq!(c.lp_db, None, "all-ones is NaN and refused");
    assert_eq!(c.att_db, None, "short read");
    let blank: [Vec<u8>; 6] = [
        vec![0xFF; 32],
        vec![0xFF; 2],
        vec![0xFF; 4],
        vec![0xFF; 4],
        vec![0xFF; 4],
        vec![0xFF; 4],
    ];
    let b = Calibration::from_eeprom(&blank);
    assert_eq!(
        (b.hardware, b.rate_correction_hz, b.global_db),
        (None, None, None)
    );
}

#[test]
fn level_offset_adds_model_rate_and_front_end_terms() {
    // Tests [@ANCHOR: elad_level_offset_db]
    let c = Calibration {
        global_db: Some(1.0),
        lp_db: Some(0.25),
        att_db: Some(-0.5),
        ..Default::default()
    };
    assert!((c.level_offset_db(Model::S2, 192_000, false, false) - 1.0).abs() < 1e-9);
    assert!(
        (c.level_offset_db(Model::S2, 384_000, true, true) - (6.0 + 1.0 + 0.25 - 0.5)).abs() < 1e-9
    );
    assert!(
        (c.level_offset_db(Model::Duo, 6_144_000, false, false) - (21.4 - 0.7 + 1.0)).abs() < 1e-9
    );
}

#[test]
fn rate_picks_the_component_width() {
    // Tests [@ANCHOR: elad_format_for_rate]
    assert_eq!(Format::for_rate(192_000), Format::S32Le);
    assert_eq!(Format::for_rate(3_072_000), Format::S32Le);
    assert_eq!(Format::for_rate(6_144_000), Format::S16Le);
}

#[test]
fn decoder_converts_both_widths_and_carries_a_split_pair() {
    // Tests [@ANCHOR: elad_decoder_push]
    let mut d = Decoder::new(Format::S32Le);
    let mut bytes = Vec::new();
    for v in [i32::MIN, 0, 1 << 30, -(1 << 30)] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = Vec::new();
    assert_eq!(
        d.push(&bytes[..11], &mut out),
        1,
        "one whole pair in the first 11 bytes"
    );
    assert_eq!(
        d.push(&bytes[11..], &mut out),
        1,
        "the carried bytes complete the second pair"
    );
    assert_eq!(out, vec![-1.0, 0.0, 0.5, -0.5]);
    let mut d16 = Decoder::new(Format::S16Le);
    let mut o16 = Vec::new();
    let mut b16 = Vec::new();
    for v in [i16::MIN, 16384i16] {
        b16.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(d16.push(&b16, &mut o16), 1);
    assert_eq!(o16, vec![-1.0, 0.5]);
    d16.push(&[1], &mut o16);
    d16.reset();
    assert_eq!(d16.push(&b16, &mut o16), 1, "reset forgets the stray byte");
}

#[test]
fn fpga_gate_refuses_a_sampler_until_declared_loaded() {
    // Tests [@ANCHOR: elad_fpga_ready]
    assert!(FpgaState::Unknown.ready_for(Model::Duo));
    assert!(!FpgaState::Unknown.ready_for(Model::S2));
    assert!(!FpgaState::Unknown.ready_for(Model::S1));
    assert!(FpgaState::LoadedExternally.ready_for(Model::S2));
}

#[test]
fn open_reads_firmware_and_calibration_and_refuses_bad_input() {
    // Tests [@ANCHOR: elad_radio_open]
    let t = fake_unit();
    let r = Radio::open(t.clone(), Model::Duo, opts(FpgaState::Unknown, 192_000)).unwrap();
    assert_eq!(r.info().firmware, Some([3, 0]));
    assert_eq!(r.info().calibration.serial.as_deref(), Some("SN-0000"));
    assert_eq!(r.info().calibration.global_db, Some(1.5));
    assert!(
        t.ops().iter().all(|o| matches!(o, Op::In { .. })),
        "open only reads"
    );
    assert!(
        Radio::open(fake_unit(), Model::S2, opts(FpgaState::Unknown, 192_000)).is_ok(),
        "discovery works without an FPGA image"
    );
    assert!(matches!(
        Radio::open(
            fake_unit(),
            Model::S1,
            opts(FpgaState::LoadedExternally, 6_144_000)
        ),
        Err(Error::OutOfRange(_))
    ));
    assert!(Radio::open(
        fake_unit(),
        Model::S2,
        opts(FpgaState::LoadedExternally, 6_144_000)
    )
    .is_ok());
}

#[test]
fn tune_sends_the_word_and_the_s2_filter_code_and_refuses_out_of_range() {
    // Tests [@ANCHOR: elad_radio_tune]
    let t = fake_unit();
    let mut r = Radio::open(
        t.clone(),
        Model::S2,
        opts(FpgaState::LoadedExternally, 192_000),
    )
    .unwrap();
    r.tune(7_100_000.0).unwrap();
    let outs: Vec<Op> = t
        .ops()
        .into_iter()
        .filter(|o| matches!(o, Op::Out { .. }))
        .collect();
    let word = tuning_word(7_100_000.0, 122_880_000.0);
    assert_eq!(
        outs[0],
        Op::Out {
            request: REQ_TUNE,
            value: (word & 0xFFFF) as u16,
            index: (word >> 16) as u16,
            data: vec![(word >> 24) as u8, 0]
        }
    );
    assert_eq!(
        outs[1],
        Op::Out {
            request: REQ_FRONT_END,
            value: 0x03,
            index: 2,
            data: vec![]
        }
    );
    assert_eq!(r.tuned_hz(), Some(7_100_000.0));
    assert!(matches!(r.tune(60_000_000.0), Err(Error::OutOfRange(_))));
    assert!(matches!(r.tune(5_000.0), Err(Error::OutOfRange(_))));
    assert!(matches!(r.tune(f64::NAN), Err(Error::OutOfRange(_))));
    let td = fake_unit();
    let mut d = Radio::open(td.clone(), Model::Duo, opts(FpgaState::Unknown, 192_000)).unwrap();
    d.tune(14_200_000.0).unwrap();
    let outs: Vec<Op> = td
        .ops()
        .into_iter()
        .filter(|o| matches!(o, Op::Out { .. }))
        .collect();
    assert_eq!(outs.len(), 1, "the DUO has no front-end request");
    let word = tuning_word(14_200_000.0, 122_880_000.0);
    assert_eq!(
        outs[0],
        Op::Out {
            request: REQ_DUO_GATEWAY,
            value: (word & 0xFFFF) as u16,
            index: 0xF200 | ((word >> 16) & 0xFF) as u16,
            data: vec![(word >> 24) as u8, 0]
        }
    );
}

#[test]
fn filters_and_attenuator_change_the_front_end_code_on_the_s2_only() {
    // Tests [@ANCHOR: elad_radio_set_filters]
    // Tests [@ANCHOR: elad_radio_set_attenuator]
    let t = fake_unit();
    let mut r = Radio::open(
        t.clone(),
        Model::S2,
        opts(FpgaState::LoadedExternally, 192_000),
    )
    .unwrap();
    r.tune(14_000_000.0).unwrap();
    r.set_attenuator(true).unwrap();
    r.set_filters(false).unwrap();
    let codes: Vec<u16> = t
        .ops()
        .into_iter()
        .filter_map(|o| match o {
            Op::Out {
                request: REQ_FRONT_END,
                value,
                ..
            } => Some(value),
            _ => None,
        })
        .collect();
    assert_eq!(codes, vec![0x03, 0x0B, 0x09]);
    let mut d = Radio::open(fake_unit(), Model::Duo, opts(FpgaState::Unknown, 192_000)).unwrap();
    assert!(matches!(d.set_filters(false), Err(Error::OutOfRange(_))));
    assert!(matches!(d.set_attenuator(true), Err(Error::OutOfRange(_))));
}

#[test]
fn start_and_stop_use_the_right_fifo_requests_for_each_family() {
    // Tests [@ANCHOR: elad_radio_start]
    // Tests [@ANCHOR: elad_radio_stop]
    let t = fake_unit();
    let mut r = Radio::open(
        t.clone(),
        Model::S2,
        opts(FpgaState::LoadedExternally, 192_000),
    )
    .unwrap();
    r.stop().unwrap();
    assert!(
        t.ops().iter().all(|o| matches!(o, Op::In { .. })),
        "stop before start sends nothing"
    );
    r.start().unwrap();
    r.stop().unwrap();
    let outs: Vec<Op> = t
        .ops()
        .into_iter()
        .filter(|o| matches!(o, Op::Out { .. }))
        .collect();
    assert_eq!(
        outs,
        vec![
            Op::Out {
                request: REQ_FIFO,
                value: 1,
                index: 0,
                data: vec![]
            },
            Op::Out {
                request: REQ_FIFO,
                value: 0,
                index: 0,
                data: vec![]
            }
        ]
    );
    let td = fake_unit();
    let mut d = Radio::open(td.clone(), Model::Duo, opts(FpgaState::Unknown, 192_000)).unwrap();
    d.start().unwrap();
    drop(d); // Drop stops the stream
    let outs: Vec<(u8, u16, u16)> = td
        .ops()
        .into_iter()
        .filter_map(|o| match o {
            Op::Out {
                request,
                value,
                index,
                ..
            } => Some((request, value, index)),
            _ => None,
        })
        .collect();
    assert_eq!(
        outs,
        vec![
            (REQ_DUO_GATEWAY, 0, 0xE800),
            (REQ_DUO_GATEWAY, 1, 0xE900),
            (REQ_DUO_GATEWAY, 0, 0xE900)
        ]
    );
}

#[test]
fn read_decodes_counts_and_reports_silence_as_no_samples() {
    // Tests [@ANCHOR: elad_radio_read]
    let t = fake_unit();
    let mut r = Radio::open(
        t.clone(),
        Model::S2,
        opts(FpgaState::LoadedExternally, 6_144_000),
    )
    .unwrap();
    let mut out = Vec::new();
    t.queue_bulk(Ok(vec![0, 0x40, 0, 0xC0, 0, 0x20]));
    assert_eq!(r.read(&mut out, Duration::from_millis(1)).unwrap(), 1);
    assert_eq!(out, vec![0.5, -0.5]);
    assert_eq!(r.stats().bytes, 6);
    assert_eq!(r.stats().samples, 1);
    assert!(
        matches!(
            r.read(&mut out, Duration::from_millis(1)),
            Err(Error::Timeout)
        ),
        "silence after data is a plain timeout"
    );
    let mut s = Radio::open(
        fake_unit(),
        Model::S2,
        opts(FpgaState::LoadedExternally, 192_000),
    )
    .unwrap();
    let mut o = Vec::new();
    assert!(matches!(
        s.read(&mut o, Duration::from_millis(1)),
        Err(Error::Timeout)
    ));
    assert!(matches!(
        s.read(&mut o, Duration::from_millis(1)),
        Err(Error::Timeout)
    ));
    assert!(
        matches!(
            s.read(&mut o, Duration::from_millis(1)),
            Err(Error::NoSamples)
        ),
        "third silent read before any sample"
    );
}

#[test]
fn nothing_the_radio_can_do_is_a_transmit_or_cat_request() {
    // Tests [@ANCHOR: elad_is_receive_request]
    for model in [Model::Duo, Model::S2, Model::S1] {
        let t = fake_unit();
        let fpga = if model.needs_fpga_image() {
            FpgaState::LoadedExternally
        } else {
            FpgaState::Unknown
        };
        let mut r = Radio::open(t.clone(), model, opts(fpga, 192_000)).unwrap();
        r.tune(7_100_000.0).unwrap();
        let _ = r.set_filters(true);
        let _ = r.set_attenuator(true);
        r.start().unwrap();
        r.stop().unwrap();
        for op in t.ops() {
            if let Op::Out { request, index, .. } = op {
                assert!(
                    is_receive_request(&Request {
                        request,
                        value: 0,
                        index,
                        data: vec![]
                    }),
                    "{model:?} sent {request:#x}"
                );
            }
        }
    }
}

#[test]
fn a_real_fdm_s2_recording_opens_and_decodes() {
    // Tests [@ANCHOR: elad_radio_open]
    // Tests [@ANCHOR: elad_calibration_from_eeprom]
    let t = FakeTransport::new();
    for line in include_str!("../tests/fixtures/fdm_s2_fw3_0_open.txt")
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let mut parts = line.splitn(3, ' ');
        let req = u8::from_str_radix(parts.next().unwrap(), 16).unwrap();
        let val = u16::from_str_radix(parts.next().unwrap(), 16).unwrap();
        let bytes: Vec<u8> = parts
            .next()
            .unwrap()
            .split(' ')
            .map(|b| u8::from_str_radix(b, 16).unwrap())
            .collect();
        t.reply(req, val, bytes);
    }
    let r = Radio::open(t, Model::S2, opts(FpgaState::Unknown, 192_000)).unwrap();
    let i = r.info();
    assert_eq!(i.firmware, Some([3, 0]));
    assert_eq!(i.calibration.hardware, Some((1, 3)));
    assert_eq!(i.calibration.serial.as_deref(), Some("SN-0000"));
    assert_eq!(i.calibration.rate_correction_hz, Some(-784));
    assert_eq!(
        (
            i.calibration.global_db,
            i.calibration.lp_db,
            i.calibration.att_db
        ),
        (Some(-8.0), Some(0.0), Some(12.5))
    );
}
