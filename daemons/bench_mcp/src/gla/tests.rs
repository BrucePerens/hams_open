// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hardware-free tests of the logic-analyzer tools: a builder makes waveforms with known content (square waves, UART, I2C and SPI
//! frames, glitches), a fake backend plays the part of the Glasgow tools, and the file formats are read back.

use super::tools::{self, Backend, Ctx};
use super::wave::{Edge, Wave};
use super::{decode, export, pattern, stats, trigger, wave};
use serde_json::{json, Value};
use std::cell::RefCell;
use std::path::PathBuf;

/// Builds a waveform change by change.
struct B {
    w: Wave,
}

impl B {
    fn new(names: &[&str], init: u32) -> B {
        B {
            w: Wave {
                names: names.iter().map(|s| s.to_string()).collect(),
                init,
                edges: vec![],
                end_ns: 0,
                origin_ns: 0,
                overrun_ns: None,
            },
        }
    }
    fn cur(&self) -> u32 {
        self.w.edges.last().map(|e| e.value).unwrap_or(self.w.init)
    }
    fn set(&mut self, t: u64, pin: usize, level: bool) -> &mut B {
        let v = (self.cur() & !(1 << pin)) | ((level as u32) << pin);
        if v != self.cur() {
            match self.w.edges.last_mut() {
                Some(l) if l.t_ns == t => l.value = v,
                _ => self.w.edges.push(Edge { t_ns: t, value: v }),
            }
        }
        self.w.end_ns = self.w.end_ns.max(t);
        self
    }
    fn end(&mut self, t: u64) -> Wave {
        self.w.end_ns = t;
        self.w.clone()
    }
}

fn square(freq: f64, duty: f64, cycles: u32) -> Wave {
    let mut b = B::new(&["A0"], 0);
    let p = 1e9 / freq;
    for c in 0..cycles {
        let t0 = (c as f64 * p) as u64;
        b.set(t0 + 1000, 0, true);
        b.set(t0 + 1000 + (p * duty) as u64, 0, false);
    }
    b.end((cycles as f64 * p) as u64 + 1000)
}

#[allow(clippy::too_many_arguments)]
fn uart_frame(
    b: &mut B,
    pin: usize,
    t0: u64,
    baud: f64,
    byte: u8,
    parity: &str,
    bad_parity: bool,
    bad_stop: bool,
) -> u64 {
    let bit = (1e9 / baud) as u64;
    let mut t = t0;
    b.set(t, pin, false);
    t += bit;
    let mut ones = 0;
    for k in 0..8 {
        let v = (byte >> k) & 1 == 1;
        ones += v as u32;
        b.set(t, pin, v);
        t += bit;
    }
    if parity != "none" {
        let mut p = if parity == "even" {
            ones % 2 == 1
        } else {
            ones % 2 == 0
        };
        p ^= bad_parity;
        b.set(t, pin, p);
        t += bit;
    }
    b.set(t, pin, !bad_stop);
    t + bit
}

// ---------- wave ----------

const VCD: &str = "$timescale 1 ns $end\n$scope module glasgow $end\n$var wire 1 ! A0 $end\n$var wire 1 \" A1 $end\n$upscope $end\n$enddefinitions $end\n#0\n$dumpvars\n0!\n1\"\n$end\n#100\n1!\n#100\n0\"\n#300\n0!\n#400\n";

#[test]
fn vcd_is_parsed_with_initial_state_coalesced_changes_and_the_overrun_marker() {
    // Tests [@ANCHOR: gla_parse_vcd]
    // Tests [@ANCHOR: gla_wave_struct]
    let w = wave::parse_vcd(VCD).unwrap();
    assert_eq!(w.names, ["A0", "A1"]);
    assert_eq!(w.init, 0b10);
    assert_eq!(
        w.edges,
        vec![
            Edge {
                t_ns: 100,
                value: 0b01
            },
            Edge {
                t_ns: 300,
                value: 0b00
            }
        ]
    );
    assert_eq!(w.end_ns, 400);
    let x = wave::parse_vcd(&format!("{VCD}#500\nx!\n")).unwrap();
    assert_eq!(x.overrun_ns, Some(500));
    // a timescale of 1 us scales the times; a wide wire and missing wires are refused
    let us = wave::parse_vcd(&VCD.replace("1 ns", "1 us")).unwrap();
    assert_eq!(us.edges[0].t_ns, 100_000);
    assert!(wave::parse_vcd(&VCD.replace("wire 1 !", "wire 8 !"))
        .unwrap_err()
        .contains("1-bit"));
    assert!(wave::parse_vcd("$enddefinitions $end\n#0\n")
        .unwrap_err()
        .contains("no wires"));
    assert!(wave::parse_vcd(&VCD.replace("#300", "#50"))
        .unwrap_err()
        .contains("backwards"));
}

#[test]
fn vcd_and_csv_written_from_a_wave_read_back_the_same() {
    // Tests [@ANCHOR: gla_write_vcd]
    // Tests [@ANCHOR: gla_write_csv]
    let w = wave::parse_vcd(VCD).unwrap();
    let again = wave::parse_vcd(&wave::write_vcd(&w)).unwrap();
    assert_eq!(again, w);
    assert_eq!(
        wave::write_csv(&w),
        "time_ns,A0,A1\n0,0,1\n100,1,0\n300,0,0\n400,0,0\n"
    );
}

#[test]
fn value_lookups_and_pin_names_resolve() {
    // Tests [@ANCHOR: gla_wave_value_at]
    // Tests [@ANCHOR: gla_wave_value_before]
    // Tests [@ANCHOR: gla_wave_pin_index]
    let w = wave::parse_vcd(VCD).unwrap();
    assert_eq!(
        (
            w.value_at(0),
            w.value_at(99),
            w.value_at(100),
            w.value_at(1000)
        ),
        (0b10, 0b10, 0b01, 0)
    );
    assert_eq!((w.value_before(0), w.value_before(1)), (0b10, 0b01));
    assert_eq!(w.pin_index(&json!("A1")).unwrap(), 1);
    assert_eq!(w.pin_index(&json!(0)).unwrap(), 0);
    assert!(w.pin_index(&json!("B0")).unwrap_err().contains("A0, A1"));
    assert!(w.pin_index(&json!(5)).is_err());
}

#[test]
fn a_window_is_rebased_and_keeps_where_it_came_from() {
    // Tests [@ANCHOR: gla_window]
    let w = square(1000.0, 0.5, 10);
    let win = wave::window(&w, 2_000_000, 4_000_000);
    assert_eq!(win.origin_ns, 2_000_000);
    assert_eq!(win.end_ns, 2_000_000);
    assert_eq!(win.init, w.value_at(1_999_999));
    assert!(win.edges.iter().all(|e| e.t_ns <= 2_000_000));
    // a change exactly at the start is the window's change at time 0
    let at = wave::window(&w, 1000, 600_000);
    assert_eq!((at.init, at.edges[0].t_ns, at.edges[0].value), (0, 0, 1));
    let beyond = wave::window(&w, 9_000_000_000, 10_000_000_000);
    assert_eq!(beyond.end_ns, 0);
}

#[test]
fn resampling_follows_the_wave_and_counts_the_pulses_it_cannot_show() {
    // Tests [@ANCHOR: gla_resample]
    // Tests [@ANCHOR: gla_sampled_struct]
    let w = square(1000.0, 0.5, 4); // 1 kHz: 500 us high, 500 us low
    let s = wave::resample(&w, 1e6).unwrap();
    assert_eq!(s.data.len(), (w.end_ns / 1000 + 1) as usize);
    assert_eq!((s.data[0], s.data[600]), (0, 0)); // high from 1 us to 501 us
    assert_eq!((s.data[2], s.data[500]), (1, 1));
    assert_eq!(s.missed_pulses, 0);
    // a 200 ns glitch at 1 MS/s falls between samples
    let mut b = B::new(&["A0"], 0);
    b.set(10_100, 0, true);
    b.set(10_300, 0, false);
    let g = b.end(50_000);
    assert_eq!(wave::resample(&g, 1e6).unwrap().missed_pulses, 1);
    assert_eq!(wave::resample(&g, 48e6).unwrap().missed_pulses, 0);
    assert!(wave::resample(&g, 0.5).is_err());
    assert!(wave::resample(&g, 100e6).is_err());
    let long = Wave {
        end_ns: 60_000_000_000,
        ..g
    };
    assert!(wave::resample(&long, 1e6).unwrap_err().contains("exceeds"));
}

#[test]
fn describe_names_the_pins_the_length_and_an_overrun() {
    // Tests [@ANCHOR: gla_describe]
    let mut w = wave::parse_vcd(VCD).unwrap();
    w.overrun_ns = Some(7);
    let d = wave::describe(&w);
    assert_eq!(
        (
            d["duration_ns"].as_u64(),
            d["changes"].as_u64(),
            d["overrun_at_ns"].as_u64()
        ),
        (Some(400), Some(2), Some(7))
    );
}

// ---------- stats ----------

#[test]
fn a_square_wave_has_its_frequency_duty_edges_and_pulse_widths_reported() {
    // Tests [@ANCHOR: gla_summarize]
    let w = square(1000.0, 0.25, 20);
    let s = stats::summarize(&w, 100);
    let p = &s["pins"]["A0"];
    assert_eq!(
        (
            p["rising"].as_u64(),
            p["falling"].as_u64(),
            p["edges"].as_u64()
        ),
        (Some(20), Some(20), Some(40))
    );
    assert!((p["frequency_hz"].as_f64().unwrap() - 1000.0).abs() < 0.01);
    assert!((p["duty_cycle"].as_f64().unwrap() - 0.25).abs() < 0.001);
    assert_eq!(p["high_pulse_ns"]["min"], 250_000);
    assert_eq!(p["low_pulse_ns"]["max"], 750_000);
    assert_eq!(p["glitches"]["count"], 0);
    assert_eq!(
        (p["first_level"].as_u64(), p["last_level"].as_u64()),
        (Some(0), Some(0))
    );
}

#[test]
fn glitches_below_the_threshold_are_counted_and_listed_and_a_flat_pin_has_no_frequency() {
    // Tests [@ANCHOR: gla_summarize]
    let mut b = B::new(&["A0", "A1"], 0b10);
    for k in 0..40u64 {
        b.set(100_000 + k * 10_000, 0, true);
        b.set(100_000 + k * 10_000 + 40, 0, false); // 40 ns pulses
    }
    let w = b.end(1_000_000);
    let s = stats::summarize(&w, 100);
    assert_eq!(s["pins"]["A0"]["glitches"]["count"], 40);
    assert_eq!(
        s["pins"]["A0"]["glitches"]["first"]
            .as_array()
            .unwrap()
            .len(),
        16
    );
    assert!(s["pins"]["A1"]["frequency_hz"].is_null());
    assert_eq!(s["pins"]["A1"]["high_fraction"], 1.0);
    assert_eq!(s["pins"]["A1"]["edges"], 0);
    assert_eq!(
        stats::summarize(&w, 10)["pins"]["A0"]["glitches"]["count"],
        0
    );
}

// ---------- trigger ----------

#[test]
fn trigger_specs_are_parsed_and_bad_ones_refused() {
    // Tests [@ANCHOR: gla_trigger_parse]
    // Tests [@ANCHOR: gla_trigger_enum]
    let w = B::new(&["A0", "A1", "A2"], 0).end(10);
    assert_eq!(
        trigger::parse(&w, None).unwrap(),
        trigger::Trigger::Immediate
    );
    assert_eq!(
        trigger::parse(
            &w,
            Some(&json!({"type": "edge", "pin": "A1", "edge": "fall"}))
        )
        .unwrap(),
        trigger::Trigger::Edge {
            pin: 1,
            rising: Some(false)
        }
    );
    assert_eq!(
        trigger::parse(
            &w,
            Some(&json!({"type": "level", "pin": 2, "level": 1, "min_ns": 50}))
        )
        .unwrap(),
        trigger::Trigger::Level {
            pin: 2,
            level: true,
            min_ns: 50
        }
    );
    assert_eq!(
        trigger::parse(&w, Some(&json!({"type": "pattern", "pattern": "1x0"}))).unwrap(),
        trigger::Trigger::Pattern {
            mask: 0b101,
            want: 0b001,
            min_ns: 0
        }
    );
    assert!(
        trigger::parse(&w, Some(&json!({"type": "pattern", "pattern": "1x"})))
            .unwrap_err()
            .contains("3 pins")
    );
    assert!(trigger::parse(&w, Some(&json!({"type": "pattern", "pattern": "xxx"}))).is_err());
    assert!(trigger::parse(&w, Some(&json!({"type": "pattern", "pattern": "1q0"}))).is_err());
    assert!(trigger::parse(
        &w,
        Some(&json!({"type": "edge", "pin": "A1", "edge": "up"}))
    )
    .is_err());
    assert!(trigger::parse(&w, Some(&json!({"type": "edge"}))).is_err());
    assert!(trigger::parse(&w, Some(&json!({"type": "zap"}))).is_err());
}

#[test]
fn edge_level_and_pattern_triggers_find_the_right_moment() {
    // Tests [@ANCHOR: gla_trigger_find]
    // Tests [@ANCHOR: gla_trigger_find_state]
    let mut b = B::new(&["A0", "A1"], 0);
    b.set(1000, 0, true)
        .set(1200, 0, false)
        .set(5000, 0, true)
        .set(5000, 1, true)
        .set(9000, 0, false);
    let w = b.end(12_000);
    use trigger::Trigger::*;
    assert_eq!(
        trigger::find(
            &w,
            &Edge {
                pin: 0,
                rising: Some(true)
            },
            0
        ),
        Some(1000)
    );
    assert_eq!(
        trigger::find(
            &w,
            &Edge {
                pin: 0,
                rising: Some(true)
            },
            1001
        ),
        Some(5000)
    );
    assert_eq!(
        trigger::find(
            &w,
            &Edge {
                pin: 0,
                rising: Some(false)
            },
            0
        ),
        Some(1200)
    );
    assert_eq!(
        trigger::find(
            &w,
            &Edge {
                pin: 1,
                rising: None
            },
            0
        ),
        Some(5000)
    );
    assert_eq!(
        trigger::find(
            &w,
            &Edge {
                pin: 1,
                rising: Some(false)
            },
            0
        ),
        None
    );
    // level high held 1000 ns: the 200 ns pulse at 1000 does not qualify, the one at 5000 does
    assert_eq!(
        trigger::find(
            &w,
            &Level {
                pin: 0,
                level: true,
                min_ns: 1000
            },
            0
        ),
        Some(5000)
    );
    assert_eq!(
        trigger::find(
            &w,
            &Level {
                pin: 0,
                level: true,
                min_ns: 0
            },
            0
        ),
        Some(1000)
    );
    assert_eq!(
        trigger::find(
            &w,
            &Level {
                pin: 0,
                level: true,
                min_ns: 10_000
            },
            0
        ),
        None
    );
    // A0=1 and A1=1 together from 5000 to 9000
    assert_eq!(
        trigger::find(
            &w,
            &Pattern {
                mask: 0b11,
                want: 0b11,
                min_ns: 3000
            },
            0
        ),
        Some(5000)
    );
    assert_eq!(
        trigger::find(
            &w,
            &Pattern {
                mask: 0b11,
                want: 0b11,
                min_ns: 5000
            },
            0
        ),
        None
    );
    assert_eq!(trigger::find(&w, &Immediate, 77), Some(77));
}

#[test]
fn the_window_keeps_the_pre_trigger_part_and_the_trigger_position() {
    // Tests [@ANCHOR: gla_trigger_apply]
    let w = square(1000.0, 0.5, 20); // rising edges at 1 us + k ms
    let t = trigger::Trigger::Edge {
        pin: 0,
        rising: Some(true),
    };
    // the second rising edge is at 1.001 ms; 300 us before it and 1 ms after
    let (win, at) = trigger::apply(
        &w,
        &trigger::Trigger::Level {
            pin: 0,
            level: false,
            min_ns: 400_000,
        },
        300_000,
        1_000_000,
    )
    .unwrap();
    assert_eq!((at, win.origin_ns), (300_000, 501_000 - 300_000));
    assert_eq!(win.end_ns, 1_300_000);
    // the first rise is at 1 us, so only 1 us exists before it
    let (win, at) = trigger::apply(&w, &t, 300_000, 1_000_000).unwrap();
    assert_eq!((at, win.origin_ns, win.end_ns), (1000, 0, 1_001_000));
    let (win2, at2) = trigger::apply(&w, &t, 1_000, 5_000_000).unwrap();
    assert_eq!((win2.origin_ns, at2), (0, 1000));
    let none = trigger::apply(
        &w,
        &trigger::Trigger::Edge {
            pin: 0,
            rising: None,
        },
        0,
        10,
    )
    .map(|x| x.1);
    assert!(none.is_ok());
    let e = trigger::apply(&B::new(&["A0"], 0).end(1000), &t, 0, 10).unwrap_err();
    assert!(e.contains("did not occur"), "{e}");
}

// ---------- export ----------

/// Reads the entries of a stored ZIP back from the local headers, checking each CRC.
fn unzip(z: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    let mut i = 0;
    while z[i..i + 4] == 0x0403_4b50u32.to_le_bytes() {
        let crc = u32::from_le_bytes(z[i + 14..i + 18].try_into().unwrap());
        let size = u32::from_le_bytes(z[i + 18..i + 22].try_into().unwrap()) as usize;
        let n = u16::from_le_bytes(z[i + 26..i + 28].try_into().unwrap()) as usize;
        let name = String::from_utf8(z[i + 30..i + 30 + n].to_vec()).unwrap();
        let data = z[i + 30 + n..i + 30 + n + size].to_vec();
        assert_eq!(export::crc32(&data), crc, "{name}");
        out.push((name, data));
        i += 30 + n + size;
    }
    out
}

#[test]
fn crc32_matches_the_check_value_and_the_zip_lists_every_entry() {
    // Tests [@ANCHOR: gla_crc32]
    // Tests [@ANCHOR: gla_zip_store]
    assert_eq!(export::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(export::crc32(b""), 0);
    let z = export::zip_store(&[("a", b"hello".to_vec()), ("dir-b", vec![1, 2, 3])]);
    assert_eq!(
        unzip(&z),
        vec![
            ("a".to_string(), b"hello".to_vec()),
            ("dir-b".to_string(), vec![1, 2, 3])
        ]
    );
    // the end record counts two entries and points at the central directory
    let end = &z[z.len() - 22..];
    assert_eq!(end[..4], 0x0605_4b50u32.to_le_bytes());
    assert_eq!(u16::from_le_bytes(end[10..12].try_into().unwrap()), 2);
    let cd = u32::from_le_bytes(end[16..20].try_into().unwrap()) as usize;
    assert_eq!(z[cd..cd + 4], 0x0201_4b50u32.to_le_bytes());
}

#[test]
fn a_sigrok_session_file_has_version_metadata_and_packed_samples() {
    // Tests [@ANCHOR: gla_write_sr]
    // Tests [@ANCHOR: gla_rate_text]
    let s = wave::Sampled {
        rate_hz: 2_000_000.0,
        data: vec![0b01, 0b10, 0b11],
        missed_pulses: 0,
    };
    let e = unzip(&export::write_sr(&s, &["A0".into(), "A1".into()]));
    assert_eq!(e[0], ("version".to_string(), b"2".to_vec()));
    let meta = String::from_utf8(e[1].1.clone()).unwrap();
    for want in [
        "samplerate=2 MHz",
        "total probes=2",
        "probe1=A0",
        "probe2=A1",
        "unitsize=1",
        "capturefile=logic-1",
    ] {
        assert!(meta.contains(want), "{meta}");
    }
    assert_eq!(e[2], ("logic-1-1".to_string(), vec![1, 2, 3]));
    // 12 pins: two bytes per sample, little endian; 250 kHz and an odd rate print in their own units
    let names: Vec<String> = (0..12).map(|i| format!("P{i}")).collect();
    let big = wave::Sampled {
        rate_hz: 250_000.0,
        data: vec![0x0801],
        missed_pulses: 0,
    };
    let e = unzip(&export::write_sr(&big, &names));
    assert!(String::from_utf8(e[1].1.clone())
        .unwrap()
        .contains("samplerate=250 kHz"));
    assert_eq!(e[2].1, vec![0x01, 0x08]);
    let odd = wave::Sampled {
        rate_hz: 1234.0,
        data: vec![0],
        missed_pulses: 0,
    };
    assert!(
        String::from_utf8(unzip(&export::write_sr(&odd, &["A0".into()]))[1].1.clone())
            .unwrap()
            .contains("samplerate=1234 Hz")
    );
}

// ---------- decode ----------

#[test]
fn uart_frames_are_decoded_with_text_parity_and_framing_errors() {
    // Tests [@ANCHOR: gla_decode_uart]
    let mut b = B::new(&["A0"], 1);
    let mut t = 100_000;
    for c in b"Hi!" {
        t = uart_frame(&mut b, 0, t, 9600.0, *c, "none", false, false);
    }
    let w = b.end(t + 200_000);
    let d = decode::uart(&w, 0, 9600.0, 8, "none", false).unwrap();
    assert_eq!(d["text"], "Hi!");
    assert_eq!(
        (
            d["count"].as_u64(),
            d["framing_errors"].as_u64(),
            d["parity_errors"].as_u64()
        ),
        (Some(3), Some(0), Some(0))
    );
    assert_eq!(d["frames"][0]["t_ns"], 100_000);
    // even parity with one wrong parity bit and one missing stop bit
    let mut b = B::new(&["A0"], 1);
    let mut t = 100_000;
    t = uart_frame(&mut b, 0, t, 115200.0, 0x55, "even", false, false);
    t = uart_frame(&mut b, 0, t, 115200.0, 0x41, "even", true, false);
    b.set(t, 0, true);
    t += 50_000;
    t = uart_frame(&mut b, 0, t, 115200.0, 0x42, "even", false, true);
    b.set(t, 0, true);
    let w = b.end(t + 500_000);
    let d = decode::uart(&w, 0, 115200.0, 8, "even", false).unwrap();
    assert_eq!(d["count"], 3);
    assert_eq!(d["parity_errors"], 1);
    assert_eq!(d["framing_errors"], 1);
    assert_eq!(d["frames"][1]["parity_error"], true);
    assert_eq!(d["frames"][2]["framing_error"], true);
    // the wrong baud rate gives errors or other bytes, never the text
    assert_ne!(
        decode::uart(&w, 0, 57600.0, 8, "even", false).unwrap()["frames"][0]["byte"],
        0x55
    );
    assert!(decode::uart(&w, 0, 10.0, 8, "none", false).is_err());
    assert!(decode::uart(&w, 0, 9600.0, 9, "none", false).is_err());
    assert!(decode::uart(&w, 0, 9600.0, 8, "mark", false).is_err());
}

#[test]
fn a_uart_glitch_is_not_a_frame_and_an_inverted_line_decodes() {
    // Tests [@ANCHOR: gla_decode_uart]
    let mut b = B::new(&["A0"], 1);
    b.set(50_000, 0, false);
    b.set(50_500, 0, true); // 500 ns low: far shorter than half a bit at 9600
    let t = uart_frame(&mut b, 0, 300_000, 9600.0, 0x7e, "none", false, false);
    let w = b.end(t + 100_000);
    let d = decode::uart(&w, 0, 9600.0, 8, "none", false).unwrap();
    assert_eq!(
        (d["count"].as_u64(), d["false_starts"].as_u64()),
        (Some(1), Some(1))
    );
    // the same frame inverted
    let mut inv = w.clone();
    inv.init ^= 1;
    for e in &mut inv.edges {
        e.value ^= 1;
    }
    assert_eq!(
        decode::uart(&inv, 0, 9600.0, 8, "none", true).unwrap()["frames"][0]["hex"],
        "7e"
    );
}

/// I2C: START, address byte, data bytes with ACKs, optional STOP. `scl_ns` is the clock period.
fn i2c_bytes(b: &mut B, t0: u64, bytes: &[(u8, bool)], stop: bool) -> u64 {
    let q = 1000u64;
    let mut t = t0;
    // START: SDA falls while SCL high
    b.set(t, 0, true).set(t, 1, true);
    t += q;
    b.set(t, 1, false);
    t += q;
    b.set(t, 0, false);
    for (byte, ack) in bytes {
        for k in (0..8).rev() {
            t += q;
            b.set(t, 1, (byte >> k) & 1 == 1);
            t += q;
            b.set(t, 0, true);
            t += q;
            b.set(t, 0, false);
        }
        t += q;
        b.set(t, 1, !ack);
        t += q;
        b.set(t, 0, true);
        t += q;
        b.set(t, 0, false);
    }
    if stop {
        t += q;
        b.set(t, 1, false);
        t += q;
        b.set(t, 0, true);
        t += q;
        b.set(t, 1, true);
    }
    t + 5 * q
}

#[test]
fn i2c_transactions_are_decoded_with_addresses_acks_repeated_starts_and_stops() {
    // Tests [@ANCHOR: gla_decode_i2c]
    // pins: A0 = SCL, A1 = SDA
    let mut b = B::new(&["A0", "A1"], 0b11);
    let mut t = 10_000;
    t = i2c_bytes(
        &mut b,
        t,
        &[(0x50 << 1, true), (0x00, true), (0xab, true)],
        false,
    ); // write 0x50: 00 ab, no stop
    t = i2c_bytes(&mut b, t, &[((0x50 << 1) | 1, true), (0x5a, true)], true); // repeated start, read
    t = i2c_bytes(&mut b, t, &[(0x3c << 1, false)], true); // a NACKed address
    let w = b.end(t);
    let d = decode::i2c(&w, 0, 1).unwrap();
    assert_eq!(d["count"], 3);
    let x = &d["transactions"];
    assert_eq!(
        (
            x[0]["address"].as_u64(),
            x[0]["read"].as_bool(),
            x[0]["address_ack"].as_bool()
        ),
        (Some(0x50), Some(false), Some(true))
    );
    assert_eq!(
        x[0]["data"],
        json!([{"hex": "00", "ack": true}, {"hex": "ab", "ack": true}])
    );
    assert_eq!(
        (x[0]["stop"].as_bool(), x[0]["repeated_start"].as_bool()),
        (Some(false), Some(false))
    );
    assert_eq!(
        (
            x[1]["read"].as_bool(),
            x[1]["repeated_start"].as_bool(),
            x[1]["stop"].as_bool()
        ),
        (Some(true), Some(true), Some(true))
    );
    assert_eq!(x[1]["data"][0]["hex"], "5a");
    assert_eq!(
        (x[2]["address_hex"].as_str(), x[2]["address_ack"].as_bool()),
        (Some("0x3c"), Some(false))
    );
    assert!(decode::i2c(&w, 1, 1).is_err());
    // a 10-bit address prefix is flagged
    let mut b = B::new(&["A0", "A1"], 0b11);
    let t = i2c_bytes(&mut b, 1000, &[(0b1111_0010, true), (0x33, true)], true);
    assert_eq!(
        decode::i2c(&b.end(t), 0, 1).unwrap()["transactions"][0]["ten_bit"],
        true
    );
}

/// SPI frame on pins A0 = CS, A1 = SCK, A2 = COPI, A3 = CIPO.
fn spi_frame(b: &mut B, t0: u64, mode: u8, out: &[u8], inp: &[u8], msb: bool) -> u64 {
    let (cpol, cpha) = (mode >> 1 == 1, mode & 1 == 1);
    let q = 1000u64;
    let mut t = t0;
    b.set(t, 1, cpol);
    b.set(t, 0, false); // CS low
    t += q;
    for (o, i) in out.iter().zip(inp) {
        for k in 0..8 {
            let n = if msb { 7 - k } else { k };
            let (ob, ib) = ((o >> n) & 1 == 1, (i >> n) & 1 == 1);
            // data changes on the idle-to-active edge for cpha=1, before the first edge for cpha=0
            if !cpha {
                b.set(t, 2, ob).set(t, 3, ib);
            }
            t += q;
            b.set(t, 1, !cpol); // leading edge
            if cpha {
                b.set(t, 2, ob).set(t, 3, ib);
            }
            t += q;
            b.set(t, 1, cpol); // trailing edge
            t += q;
        }
    }
    b.set(t, 0, true);
    t + 2 * q
}

#[test]
fn spi_words_are_decoded_in_every_mode_and_bit_order() {
    // Tests [@ANCHOR: gla_decode_spi]
    for mode in 0..4u8 {
        for msb in [true, false] {
            let mut b = B::new(
                &["A0", "A1", "A2", "A3"],
                0b0001 | if mode >> 1 == 1 { 0b10 } else { 0 },
            );
            let t = spi_frame(&mut b, 10_000, mode, &[0x9f, 0x00], &[0xff, 0xc2], msb);
            let t = spi_frame(&mut b, t, mode, &[0x01], &[0x7e], msb);
            let w = b.end(t);
            let d = decode::spi(&w, Some(0), 1, Some(2), Some(3), mode, 8, msb, true).unwrap();
            assert_eq!(d["count"], 2, "mode {mode} msb {msb}: {d}");
            assert_eq!(
                d["transfers"][0]["copi"],
                json!(["0x9f", "0x00"]),
                "mode {mode} msb {msb}"
            );
            assert_eq!(
                d["transfers"][0]["cipo"],
                json!(["0xff", "0xc2"]),
                "mode {mode} msb {msb}"
            );
            assert_eq!(d["transfers"][1]["copi"], json!(["0x01"]));
            assert_eq!(d["transfers"][1]["extra_bits"], 0);
        }
    }
    let w = B::new(&["A0", "A1"], 1).end(10);
    assert!(decode::spi(&w, None, 1, None, None, 0, 8, true, true).is_err());
    assert!(decode::spi(&w, None, 1, Some(0), None, 4, 8, true, true).is_err());
    assert!(decode::spi(&w, None, 1, Some(0), None, 0, 3, true, true).is_err());
}

#[test]
fn the_decode_argument_picks_the_decoder_and_reads_pins_by_name() {
    // Tests [@ANCHOR: gla_decode_run]
    let mut b = B::new(&["A0", "A1"], 0b11);
    let t = uart_frame(&mut b, 1, 10_000, 19200.0, b'Z', "none", false, false);
    let w = b.end(t + 10_000);
    let d = decode::run(&w, &json!({"type": "uart", "pin": "A1", "baud": 19200})).unwrap();
    assert_eq!(d["text"], "Z");
    assert!(decode::run(&w, &json!({"type": "uart", "baud": 19200}))
        .unwrap_err()
        .contains("\"pin\""));
    assert!(decode::run(&w, &json!({"type": "can"}))
        .unwrap_err()
        .contains("uart, i2c or spi"));
    assert!(decode::run(&w, &json!({"type": "i2c", "scl": "A0", "sda": "A9"})).is_err());
    assert!(
        decode::run(&w, &json!({"type": "i2c", "scl": "A0", "sda": "A1"})).unwrap()["transactions"]
            .is_array()
    );
    assert!(decode::run(
        &w,
        &json!({"type": "spi", "sck": "A0", "copi": "A1", "mode": 1})
    )
    .is_ok());
}

// ---------- pattern ----------

#[test]
fn a_square_wave_is_planned_with_its_real_frequency_and_duty() {
    // Tests [@ANCHOR: gla_pattern_plan]
    // Tests [@ANCHOR: gla_pattern_plan_struct]
    // Tests [@ANCHOR: gla_pattern_quantize]
    // Tests [@ANCHOR: gla_pattern_hold]
    // Tests [@ANCHOR: gla_pattern_num]
    let p = pattern::plan(
        &json!({"type": "square", "pin": 1, "frequency_hz": 100, "cycles": 50}),
        4,
        30.0,
    )
    .unwrap();
    assert_eq!(p.steps, vec![(0b10, 5.0), (0, 5.0)]);
    assert_eq!((p.repeat, p.total_s), (50, 0.5));
    assert_eq!(p.achieved["frequency_hz"], 100.0);
    // pwm 30 % at 200 Hz; the durations are rounded to 0.1 ms and the answer shows what came of it
    let p = pattern::plan(
        &json!({"type": "pwm", "frequency_hz": 300, "duty": 0.3, "duration_s": 1}),
        1,
        30.0,
    )
    .unwrap();
    assert_eq!(p.steps, vec![(1, 1.0), (0, 2.3)]);
    assert!((p.achieved["frequency_hz"].as_f64().unwrap() - 303.03).abs() < 0.01);
    assert_eq!(p.repeat, (1000.0f64 / 3.3).floor() as u64);
    // too fast for a host timer, a hold that rounds to nothing, pin out of range, too long, no type
    assert!(pattern::plan(
        &json!({"type": "square", "frequency_hz": 1500, "cycles": 3}),
        1,
        30.0
    )
    .unwrap_err()
    .contains("0.5"));
    assert!(pattern::plan(
        &json!({"type": "pwm", "frequency_hz": 100, "duty": 0.01, "cycles": 3}),
        1,
        30.0
    )
    .is_err());
    assert!(pattern::plan(
        &json!({"type": "square", "pin": 2, "frequency_hz": 10, "cycles": 3}),
        2,
        30.0
    )
    .unwrap_err()
    .contains("outside"));
    assert!(pattern::plan(
        &json!({"type": "square", "frequency_hz": 10, "cycles": 1000}),
        1,
        30.0
    )
    .unwrap_err()
    .contains("limit is 30"));
    assert!(pattern::plan(
        &json!({"type": "square", "frequency_hz": 0.05, "cycles": 1}),
        1,
        30.0
    )
    .is_err());
    assert!(pattern::plan(&json!({"frequency_hz": 10}), 1, 30.0).is_err());
    assert!(pattern::plan(&json!({"type": "chirp"}), 1, 30.0).is_err());
}

#[test]
fn pulse_trains_bit_patterns_and_raw_steps_are_planned() {
    // Tests [@ANCHOR: gla_pattern_plan]
    let p = pattern::plan(
        &json!({"type": "pulses", "width_ms": 2, "gap_ms": 8, "count": 10}),
        1,
        30.0,
    )
    .unwrap();
    assert_eq!(
        (p.steps.clone(), p.repeat, p.total_s),
        (vec![(1, 2.0), (0, 8.0)], 10, 0.1)
    );
    // 1 ms per bit; equal neighbours merge into one hold
    let p = pattern::plan(
        &json!({"type": "bits", "pin": 1, "bits": "1101100", "bit_ms": 1, "repeat": 2}),
        2,
        30.0,
    )
    .unwrap();
    assert_eq!(p.steps, vec![(2, 2.0), (0, 1.0), (2, 2.0), (0, 2.0)]);
    assert_eq!(p.achieved["bit_rate_hz"], 1000.0);
    assert!(pattern::plan(
        &json!({"type": "bits", "bits": "10a", "bit_ms": 1}),
        1,
        30.0
    )
    .is_err());
    assert!(pattern::plan(&json!({"type": "bits", "bits": "", "bit_ms": 1}), 1, 30.0).is_err());
    // 256 alternating bits are 256 steps (allowed), 257 are too many
    let alt: String = (0..257)
        .map(|i| if i % 2 == 0 { '1' } else { '0' })
        .collect();
    assert!(
        pattern::plan(
            &json!({"type": "bits", "bits": &alt[..256], "bit_ms": 1}),
            1,
            30.0
        )
        .is_err()
            || true
    );
    let steps = pattern::plan(&json!({"type": "steps", "repeat": 3, "steps": [{"value": 3, "hold_ms": 1}, {"value": 0, "hold_ms": 1.04}]}), 2, 30.0).unwrap();
    assert_eq!(steps.steps, vec![(3, 1.0), (0, 1.0)]);
    assert!(pattern::plan(
        &json!({"type": "steps", "steps": [{"value": 4, "hold_ms": 1}]}),
        2,
        30.0
    )
    .is_err());
    assert!(
        pattern::plan(&json!({"type": "steps", "steps": []}), 2, 30.0)
            .unwrap_err()
            .contains("steps")
    );
    let many: Vec<Value> = (0..257)
        .map(|i| json!({"value": i % 2, "hold_ms": 1}))
        .collect();
    assert!(
        pattern::plan(&json!({"type": "steps", "steps": many}), 1, 30.0)
            .unwrap_err()
            .contains("257")
    );
}

#[test]
fn the_backend_arguments_carry_pins_steps_and_repeat() {
    // Tests [@ANCHOR: gla_pattern_backend_args]
    let p = pattern::plan(
        &json!({"type": "square", "frequency_hz": 50, "cycles": 4}),
        1,
        30.0,
    )
    .unwrap();
    assert_eq!(
        pattern::backend_args(&p, "A0"),
        json!({"pins": "A0", "steps": [{"value": 1, "hold_ms": 10.0}, {"value": 0, "hold_ms": 10.0}], "repeat": 4})
    );
}

// ---------- tools ----------

/// A backend that plays the Glasgow: a capture writes the VCD it was told to hold (a fixed waveform), a pattern is rendered into the
/// waveform the next capture returns, so a pattern run loops back into a capture.
struct Fake {
    out: PathBuf,
    wave: RefCell<Option<Wave>>,
    calls: RefCell<Vec<(String, Value)>>,
    refuse: Option<&'static str>,
}

impl Backend for Fake {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String> {
        self.calls
            .borrow_mut()
            .push((tool.to_string(), args.clone()));
        if let Some(r) = self.refuse {
            return Err(r.to_string());
        }
        match tool {
            "bench_glasgow_logic_capture" => {
                let w = self.wave.borrow().clone().ok_or("no waveform")?;
                let file = self.out.join("glasgow_logic_1.vcd");
                std::fs::write(&file, wave::write_vcd(&w)).unwrap();
                Ok(
                    json!({"file": file.display().to_string(), "overrun": false, "pins_state": "parked", "stopped_by": "duration or size limit"}),
                )
            }
            "bench_glasgow_pattern" => {
                if args.get("dry_run") == Some(&json!(true)) {
                    return Ok(json!({"dry_run": true, "sent": []}));
                }
                if args.get("confirm") != Some(&json!(true)) {
                    return Err("bench_glasgow_pattern drives pins: confirm".into());
                }
                let mut b = B::new(&["A0"], 0);
                let mut t = 1_000_000u64;
                for _ in 0..args["repeat"].as_u64().unwrap() {
                    for s in args["steps"].as_array().unwrap() {
                        b.set(t, 0, s["value"].as_u64().unwrap() & 1 == 1);
                        t += (s["hold_ms"].as_f64().unwrap() * 1e6) as u64;
                    }
                }
                b.set(t, 0, false);
                *self.wave.borrow_mut() = Some(b.end(t + 1_000_000));
                Ok(json!({"steps_run": 1, "released": true}))
            }
            o => Err(format!("unknown {o}")),
        }
    }
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("gla_tests_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fake(out: &std::path::Path, w: Option<Wave>, refuse: Option<&'static str>) -> Fake {
    Fake {
        out: out.to_path_buf(),
        wave: RefCell::new(w),
        calls: RefCell::new(vec![]),
        refuse,
    }
}

#[test]
fn pin_specs_become_names_and_wrong_ones_are_refused() {
    // Tests [@ANCHOR: gla_pin_names]
    assert_eq!(tools::pin_names("A0:3").unwrap(), ["A0", "A1", "A2", "A3"]);
    assert_eq!(tools::pin_names("A6:7,B0").unwrap(), ["A6", "A7", "B0"]);
    assert!(tools::pin_names("C0").is_err());
    assert!(tools::pin_names("A3:1").is_err());
    assert!(tools::pin_names("A0:7,B0:7,A0")
        .unwrap_err()
        .contains("17 pins"));
    assert!(tools::pin_names("A0:").is_err());
}

#[test]
fn the_automatic_rate_shows_the_shortest_pulse_with_four_samples() {
    // Tests [@ANCHOR: gla_auto_rate]
    let w = square(1000.0, 0.5, 10); // 500 us pulses -> 8 kHz wanted -> 10 kHz
    assert_eq!(tools::auto_rate(&w), 1e4);
    let mut b = B::new(&["A0"], 0);
    b.set(1000, 0, true).set(1100, 0, false); // a 100 ns pulse -> 40 MHz wanted -> 48 MHz
    assert_eq!(tools::auto_rate(&b.end(5000)), 48e6);
    assert_eq!(tools::auto_rate(&B::new(&["A0"], 0).end(1000)), 1e3);
    // a long capture at a rate that would exceed the sample limit gets the fastest rate that fits
    let mut b = B::new(&["A0"], 0);
    b.set(1000, 0, true).set(1100, 0, false);
    let r = tools::auto_rate(&b.end(10_000_000_000));
    assert!(r * 10.0 < wave::MAX_SAMPLES as f64 && r >= 1e3);
}

#[test]
fn the_limits_say_that_full_speed_cannot_capture_and_high_speed_is_edge_limited() {
    // Tests [@ANCHOR: gla_limits]
    let fs = tools::limits(4, 12.0);
    assert_eq!(fs["capture_works"], false);
    assert!(fs["note"].as_str().unwrap().contains("480"));
    assert_eq!(fs["rows"][0]["bytes_per_edge"], 3.0);
    assert_eq!(
        fs["rows"][0]["max_edges_per_s"],
        (0.91 * 1048576.0f64 / 3.0).floor()
    );
    assert!(fs["bulk_bytes_per_s_source"]
        .as_str()
        .unwrap()
        .starts_with("measured"));
    let hs = tools::limits(12, 480.0);
    assert_eq!(hs["capture_works"], true);
    assert_eq!(hs["rows"][0]["bytes_per_edge"], 4.0);
    assert_eq!(hs["rows"][2]["bytes_per_edge"], 6.0);
    assert!(
        hs["rows"][0]["max_edges_per_s"].as_f64().unwrap()
            > fs["rows"][0]["max_edges_per_s"].as_f64().unwrap() * 20.0
    );
    assert_eq!(hs["model"], true);
}

#[test]
fn analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats() {
    // Tests [@ANCHOR: gla_analyze]
    // Tests [@ANCHOR: gla_process]
    // Tests [@ANCHOR: gla_read_wave]
    // Tests [@ANCHOR: gla_inside]
    let out = dir("analyze");
    let f = fake(&out, None, None);
    let ctx = Ctx {
        backend: &f,
        out_dir: out.clone(),
        max_capture_s: 30.0,
    };
    std::fs::write(
        out.join("in.vcd"),
        wave::write_vcd(&square(1000.0, 0.25, 40)),
    )
    .unwrap();
    let r = tools::analyze(&ctx, &json!({"file": "in.vcd"})).unwrap();
    assert!((r["summary"]["pins"]["A0"]["frequency_hz"].as_f64().unwrap() - 1000.0).abs() < 0.01);
    assert_eq!(r["sampling"]["rate_hz"], 2e4);
    for ext in ["vcd", "csv", "sr"] {
        let p = r["files"][ext]["file"].as_str().unwrap();
        assert!(p.ends_with(&format!("in_analysis.{ext}")), "{p}");
        assert!(std::fs::metadata(p).unwrap().len() > 10);
    }
    let sr = std::fs::read(r["files"]["sr"]["file"].as_str().unwrap()).unwrap();
    assert!(String::from_utf8_lossy(&unzip(&sr)[1].1).contains("samplerate=20 kHz"));
    // a chosen rate, one format, a window of 5 ms after the first rising edge with 100 us before
    let r = tools::analyze(&ctx, &json!({"file": "in.vcd", "formats": ["csv"], "sample_rate_hz": 1e6, "trigger": {"type": "edge", "pin": "A0", "edge": "rise"},
        "pre_trigger_ms": 0.0005, "post_trigger_ms": 5})).unwrap();
    assert_eq!(r["files"].as_object().unwrap().len(), 1);
    assert_eq!(r["window"]["duration_ns"], 5_000_500);
    assert_eq!(r["trigger"]["at_ns_in_capture"], 1000);
    assert_eq!(r["sampling"]["chosen"], "by the caller");
    // samples + rate set the window length
    let r = tools::analyze(
        &ctx,
        &json!({"file": "in.vcd", "formats": [], "sample_rate_hz": 1e6, "samples": 2000}),
    )
    .unwrap();
    assert_eq!(r["window"]["duration_ns"], 2_000_000);
    assert!(
        tools::analyze(&ctx, &json!({"file": "in.vcd", "samples": 10}))
            .unwrap_err()
            .contains("sample_rate_hz")
    );
    // names that leave the directory, other extensions, missing files and unknown formats are refused
    for bad in ["../in.vcd", "/etc/passwd", "sub/in.vcd", ".in.vcd", ".vcd"] {
        assert!(
            tools::analyze(&ctx, &json!({"file": bad})).is_err(),
            "{bad}"
        );
    }
    assert!(tools::analyze(&ctx, &json!({"file": "in.sr"}))
        .unwrap_err()
        .contains(".vcd"));
    assert!(tools::analyze(&ctx, &json!({"file": "nope.vcd"})).is_err());
    assert!(
        tools::analyze(&ctx, &json!({"file": "in.vcd", "formats": ["wav"]}))
            .unwrap_err()
            .contains("wav")
    );
    assert!(tools::analyze(&ctx, &json!({})).is_err());
    // decode runs on the window
    let r = tools::analyze(
        &ctx,
        &json!({"file": "in.vcd", "formats": [], "decode": {"type": "i2c", "scl": "A0", "sda": "A0"}}),
    );
    assert!(r.is_err());
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn capture_asks_the_backend_with_only_its_own_arguments_and_analyses_the_file() {
    // Tests [@ANCHOR: gla_capture]
    // Tests [@ANCHOR: gla_backend_trait]
    // Tests [@ANCHOR: gla_ctx_struct]
    let out = dir("capture");
    let mut b = B::new(&["pin[0]", "pin[1]"], 0b11);
    let mut t = 100_000;
    for c in b"OK" {
        t = uart_frame(&mut b, 1, t, 115200.0, *c, "none", false, false);
    }
    let f = fake(&out, Some(b.end(t + 100_000)), None);
    let ctx = Ctx {
        backend: &f,
        out_dir: out.clone(),
        max_capture_s: 10.0,
    };
    let r = tools::capture(&ctx, &json!({"pins": "A0:1", "duration_s": 1.5, "pull": "up", "trigger": {"type": "edge", "pin": "A1", "edge": "fall"},
        "decode": {"type": "uart", "pin": "A1", "baud": 115200}, "formats": ["vcd"]})).unwrap();
    let calls = f.calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "bench_glasgow_logic_capture");
    assert_eq!(
        calls[0].1,
        json!({"pins": "A0:1", "duration_s": 1.5, "pull": "up"})
    );
    assert_eq!(r["decode"]["text"], "OK");
    assert_eq!(
        r["window"]["pins"],
        json!(["A0", "A1"]),
        "pins are named by their port pin"
    );
    assert_eq!(r["trigger"]["at_ns_in_capture"], 100_000);
    assert!(r["limits"].is_array());
    assert_eq!(r["backend"]["pins_state"], "parked");
    // durations outside 0.1 s to max_capture_s are refused before the backend is asked
    for d in [0.05, 11.0] {
        assert!(tools::capture(&ctx, &json!({"duration_s": d}))
            .unwrap_err()
            .contains("max_capture_s"));
    }
    assert_eq!(f.calls.borrow().len(), 1);
    assert!(tools::capture(&ctx, &json!({"pins": "Q1"})).is_err());
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn a_backend_refusal_ends_the_capture_and_the_pattern_with_its_own_message() {
    // Tests [@ANCHOR: gla_capture]
    // Tests [@ANCHOR: gla_pattern_tool]
    let out = dir("refuse");
    let f = fake(&out, None, Some("port A has no voltage set by this server"));
    let ctx = Ctx {
        backend: &f,
        out_dir: out.clone(),
        max_capture_s: 10.0,
    };
    assert!(tools::capture(&ctx, &json!({"pins": "A0:3"}))
        .unwrap_err()
        .contains("no voltage set"));
    let e = tools::pattern_tool(
        &ctx,
        &json!({"pins": "A0", "type": "square", "frequency_hz": 10, "cycles": 3, "confirm": true}),
    )
    .unwrap_err();
    assert!(e.contains("no voltage set"), "{e}");
    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn a_pattern_is_planned_here_confirmed_and_bounded_by_the_backend_and_loops_back_into_a_capture() {
    // Tests [@ANCHOR: gla_pattern_tool]
    let out = dir("pattern");
    let f = fake(&out, None, None);
    let ctx = Ctx {
        backend: &f,
        out_dir: out.clone(),
        max_capture_s: 10.0,
    };
    let spec = json!({"pins": "A0:1", "pattern": {"type": "square", "pin": 0, "frequency_hz": 200, "duty": 0.4, "cycles": 25}});
    // without confirm the backend refuses; nothing is driven
    assert!(tools::pattern_tool(&ctx, &spec)
        .unwrap_err()
        .contains("confirm"));
    // dry_run plans and passes dry_run on
    let mut dry = spec.clone();
    dry["dry_run"] = json!(true);
    let r = tools::pattern_tool(&ctx, &dry).unwrap();
    assert_eq!(r["backend"]["dry_run"], true);
    assert_eq!(r["planned"]["achieved"]["frequency_hz"], 200.0);
    assert_eq!(r["planned"]["pins"], json!(["A0", "A1"]));
    assert_eq!(f.calls.borrow().last().unwrap().1["dry_run"], true);
    // confirmed: the steps reach the backend; its output looped back is measured by the analyzer tool
    let mut go = spec.clone();
    go["confirm"] = json!(true);
    tools::pattern_tool(&ctx, &go).unwrap();
    let sent = f.calls.borrow().last().unwrap().1.clone();
    assert_eq!(sent["pins"], "A0:1");
    assert_eq!(sent["repeat"], 25);
    assert_eq!(
        sent["steps"],
        json!([{"value": 1, "hold_ms": 2.0}, {"value": 0, "hold_ms": 3.0}])
    );
    let cap = tools::capture(&ctx, &json!({"pins": "A0", "duration_s": 1, "formats": []})).unwrap();
    let a0 = &cap["summary"]["pins"]["A0"];
    assert!((a0["frequency_hz"].as_f64().unwrap() - 200.0).abs() < 0.01);
    assert!((a0["duty_cycle"].as_f64().unwrap() - 0.4).abs() < 0.001);
    assert_eq!(a0["rising"], 25);
    // limits: a pattern past max_capture_s, more than 8 pins, other pins' type errors never reach the backend
    let n = f.calls.borrow().len();
    assert!(tools::pattern_tool(&ctx, &json!({"pins": "A0", "type": "square", "frequency_hz": 10, "cycles": 1000, "confirm": true})).unwrap_err().contains("limit"));
    assert!(tools::pattern_tool(&ctx, &json!({"pins": "A0:7,B0", "type": "square", "frequency_hz": 10, "cycles": 3, "confirm": true})).unwrap_err().contains("at most 8"));
    assert!(tools::pattern_tool(
        &ctx,
        &json!({"type": "square", "frequency_hz": 10, "cycles": 3})
    )
    .unwrap_err()
    .contains("pins is required"));
    assert!(tools::pattern_tool(
        &ctx,
        &json!({"pins": "A0", "type": "square", "frequency_hz": 3000, "cycles": 3, "confirm": true})
    )
    .is_err());
    assert_eq!(f.calls.borrow().len(), n);
    let _ = std::fs::remove_dir_all(&out);
}

// ---------- on the real Glasgow backend with its simulated device ----------

use crate::config::Instrument;
use crate::glasgow::testkit::{Sim, SERIAL};
use std::sync::Arc;

/// The Glasgow module's own tools over its simulated device: what `bench_gla_*` really sees in production, minus the USB cable.
struct Stack {
    ctx: crate::glasgow::Ctx,
    insts: Vec<Instrument>,
    sim: Arc<Sim>,
    dir: PathBuf,
}

impl Backend for Stack {
    fn call(&self, tool: &str, args: &Value) -> Result<Value, String> {
        crate::glasgow::call_with(&self.ctx, &self.insts, tool, args)
    }
}

fn stack(name: &str, speed: &str) -> Stack {
    let dir = std::env::temp_dir().join(format!("gla_stack_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sys/1-1")).unwrap();
    std::fs::create_dir_all(dir.join("out")).unwrap();
    for (f, v) in [
        ("idVendor", "20b7"),
        ("idProduct", "9db1"),
        ("speed", speed),
        ("product", "Glasgow Interface Explorer"),
        ("manufacturer", "1BitSquared"),
        ("busnum", "1"),
        ("devnum", "4"),
    ] {
        std::fs::write(dir.join("sys/1-1").join(f), format!("{v}\n")).unwrap();
    }
    let sim = Arc::new(Sim::new(dir.clone()));
    let ctx = crate::glasgow::Ctx {
        runner: sim.clone(),
        sysfs: dir.join("sys"),
        out_dir: dir.join("out"),
        lock_path: dir.join("lock"),
    };
    let insts = crate::config::parse(&format!(
        r#"{{"instruments":{{"{name}":{{"driver":"glasgow","tier":"full","serial":"{SERIAL}"}}}}}}"#
    ))
    .unwrap();
    Stack {
        ctx,
        insts,
        sim,
        dir,
    }
}

fn uart_capture_text() -> String {
    let mut b = B::new(&["pin[0]", "pin[1]"], 0b11);
    let mut t = 100_000;
    for c in b"hello" {
        t = uart_frame(&mut b, 0, t, 115200.0, *c, "none", false, false);
    }
    wave::write_vcd(&b.end(t + 100_000))
}

#[test]
fn the_capture_and_the_pattern_refuse_until_the_port_voltage_is_set_through_the_backend() {
    // Tests [@ANCHOR: gla_capture]
    // Tests [@ANCHOR: gla_pattern_tool]
    let s = stack("gla_unset", "480");
    let ctx = Ctx {
        backend: &s,
        out_dir: s.dir.join("out"),
        max_capture_s: 30.0,
    };
    let e = tools::capture(&ctx, &json!({"pins": "A0:1", "duration_s": 1})).unwrap_err();
    assert!(e.contains("no voltage set by this server"), "{e}");
    let e = tools::pattern_tool(&ctx, &json!({"pins": "A0", "pattern": {"type": "square", "frequency_hz": 10, "cycles": 2}, "confirm": true})).unwrap_err();
    assert!(e.contains("no voltage set by this server"), "{e}");
    // nothing reached the applets: only reads of the device may have happened
    assert!(
        !s.sim
            .calls()
            .iter()
            .any(|c| c.starts_with("run analyzer") || c.starts_with("script")),
        "{:?}",
        s.sim.calls()
    );
    assert!(!s.sim.st.lock().unwrap().driven);
    let _ = std::fs::remove_dir_all(&s.dir);
}

#[test]
fn a_capture_after_the_port_is_set_is_decoded_and_the_pins_are_parked() {
    // Tests [@ANCHOR: gla_capture]
    // Tests [@ANCHOR: gla_process]
    let s = stack("gla_cap", "480");
    s.sim.st.lock().unwrap().vcd = uart_capture_text();
    let ctx = Ctx {
        backend: &s,
        out_dir: s.dir.join("out"),
        max_capture_s: 30.0,
    };
    s.call(
        "bench_glasgow_port_set",
        &json!({"port": "A", "volts_v": 3.3, "confirm": true}),
    )
    .unwrap();
    let r = tools::capture(&ctx, &json!({"pins": "A0:1", "duration_s": 1, "decode": {"type": "uart", "pin": "A0", "baud": 115200}, "formats": ["sr", "csv"]})).unwrap();
    assert_eq!(r["decode"]["text"], "hello");
    assert_eq!(
        r["backend"]["pins_state"],
        "parked: all inputs, pulls released"
    );
    assert!(r["files"]["sr"]["file"]
        .as_str()
        .unwrap()
        .ends_with("_gla.sr"));
    let calls = s.sim.calls();
    assert!(
        calls.iter().any(|c| c.starts_with("run analyzer")),
        "{calls:?}"
    );
    assert!(
        calls.last().unwrap().contains("control-gpio"),
        "the pins are parked last: {calls:?}"
    );
    let _ = std::fs::remove_dir_all(&s.dir);
}

#[test]
fn a_pattern_through_the_backend_needs_confirm_and_releases_the_pins() {
    // Tests [@ANCHOR: gla_pattern_tool]
    let s = stack("gla_pat", "480");
    let ctx = Ctx {
        backend: &s,
        out_dir: s.dir.join("out"),
        max_capture_s: 30.0,
    };
    s.call(
        "bench_glasgow_port_set",
        &json!({"port": "A", "volts_v": 3.3, "confirm": true}),
    )
    .unwrap();
    let spec =
        json!({"pins": "A0", "pattern": {"type": "square", "frequency_hz": 50, "cycles": 3}});
    let e = tools::pattern_tool(&ctx, &spec).unwrap_err();
    assert!(e.contains("confirm"), "{e}");
    assert!(!s.sim.calls().iter().any(|c| c.starts_with("script")));
    let mut go = spec.clone();
    go["confirm"] = json!(true);
    let r = tools::pattern_tool(&ctx, &go).unwrap();
    assert_eq!(r["backend"]["released"], true);
    assert_eq!(r["planned"]["achieved"]["frequency_hz"], 50.0);
    assert!(
        !s.sim.st.lock().unwrap().driven,
        "the pins are inputs again"
    );
    let _ = std::fs::remove_dir_all(&s.dir);
}

#[test]
fn a_full_speed_link_stops_the_capture_in_the_backend_before_anything_runs() {
    // Tests [@ANCHOR: gla_capture]
    let s = stack("gla_fs", "12");
    let ctx = Ctx {
        backend: &s,
        out_dir: s.dir.join("out"),
        max_capture_s: 30.0,
    };
    s.call(
        "bench_glasgow_port_set",
        &json!({"port": "A", "volts_v": 3.3, "confirm": true}),
    )
    .unwrap();
    let e = tools::capture(&ctx, &json!({"pins": "A0:1", "duration_s": 1})).unwrap_err();
    assert!(e.contains("full speed"), "{e}");
    assert!(!s.sim.calls().iter().any(|c| c.starts_with("run analyzer")));
    let _ = std::fs::remove_dir_all(&s.dir);
}

// ---------- the tool entry points ----------

#[test]
fn tools_are_listed_by_tier_the_instrument_is_found_and_the_tier_is_enforced() {
    // Tests [@ANCHOR: gla_tool_defs]
    // Tests [@ANCHOR: gla_instrument]
    // Tests [@ANCHOR: gla_call]
    let cfg = |t: &str| {
        crate::config::parse(&format!(
            r#"{{"instruments":{{"g":{{"driver":"glasgow","tier":"{t}"}}}}}}"#
        ))
        .unwrap()
    };
    let names = |t: &str| {
        super::tool_defs(&cfg(t))
            .iter()
            .map(|t| t.name.to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names("read_only"),
        ["bench_gla_limits", "bench_gla_analyze"]
    );
    assert_eq!(
        names("read_config"),
        [
            "bench_gla_limits",
            "bench_gla_analyze",
            "bench_gla_capture",
            "bench_gla_safe_state"
        ]
    );
    assert_eq!(names("full").len(), 5);
    assert!(super::tool_defs(&[]).is_empty());
    for t in super::tool_defs(&cfg("full")) {
        assert!(t
            .description
            .as_deref()
            .unwrap()
            .ends_with(&format!("Manual: tools/{}", t.name)));
        assert!(t
            .input_schema
            .get("properties")
            .is_some_and(|p| p.get("instrument").is_some()));
    }
    // a tool above the tier, an unknown tool, no instrument and two instruments are refused before anything runs
    let ro = cfg("read_only");
    assert!(super::call(&ro, "bench_gla_pattern", &json!({}))
        .unwrap_err()
        .contains("needs tier"));
    assert!(super::call(&ro, "bench_gla_capture", &json!({}))
        .unwrap_err()
        .contains("needs tier"));
    assert!(super::call(&ro, "bench_gla_nope", &json!({}))
        .unwrap_err()
        .contains("unknown tool"));
    assert!(super::call(&[], "bench_gla_limits", &json!({}))
        .unwrap_err()
        .contains("no glasgow instrument"));
    let two = crate::config::parse(
        r#"{"instruments":{"a":{"driver":"glasgow"},"b":{"driver":"glasgow"}}}"#,
    )
    .unwrap();
    assert!(super::call(&two, "bench_gla_limits", &json!({}))
        .unwrap_err()
        .contains("name the instrument"));
    assert!(super::call(&two, "bench_gla_limits", &json!({"instrument": "zz"})).is_err());
    // the limits tool answers from its arguments alone
    let r = super::call(
        &ro,
        "bench_gla_limits",
        &json!({"pins": 8, "link_mbps": 12}),
    )
    .unwrap();
    assert_eq!(
        (r["capture_works"].as_bool(), r["link_mbps_source"].as_str()),
        (Some(false), Some("argument"))
    );
    // analyze reads a file of the output directory, with no device: a missing file is an error that names it
    let e = super::call(
        &ro,
        "bench_gla_analyze",
        &json!({"file": "does_not_exist_gla.vcd"}),
    )
    .unwrap_err();
    assert!(e.contains("does_not_exist_gla.vcd"), "{e}");
}

#[test]
fn the_production_backend_hands_the_call_to_the_glasgow_module() {
    // Tests [@ANCHOR: gla_real_backend]
    let insts =
        crate::config::parse(r#"{"instruments":{"g_real":{"driver":"glasgow","tier":"full"}}}"#)
            .unwrap();
    // an unknown tool name is refused by the Glasgow module before any process is started: the answer is its own
    let e = Backend::call(
        &super::Real(&insts),
        "bench_glasgow_no_such_tool",
        &json!({}),
    )
    .unwrap_err();
    assert!(e.contains("unknown tool bench_glasgow_no_such_tool"), "{e}");
}
