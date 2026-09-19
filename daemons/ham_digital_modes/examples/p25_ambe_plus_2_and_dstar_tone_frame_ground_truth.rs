// SPDX-License-Identifier: LGPL-3.0-or-later
//! Section 40 left one real, unexplained cross-mode asymmetry: under `TD_ENABLE`, D-STAR reaches
//! its own real `Tone` code (`b0=126`) for a detected tone/DTMF stimulus, but AMBE+2 half-rate
//! reaches `Erasure` (`b0=120`) instead of its own spec-defined `Tone` code (`b0=126/127`) for the
//! identical kind of stimulus. DVSI's own manual documents an independent, chip-reported ground-
//! truth status bit for exactly this question -- `ECMODE_OUT`'s `TONE_FRAME` (bit 15): "The encoder
//! sets this bit if the output frame contains either a single frequency tone, a DTMF tone, a KNOX
//! tone, or a call progress tone" -- readable via `PKT_CHANFMT` (field `0x15`, data `0b01`) appended
//! to every `CHANNEL` packet, the same mechanism section 27/28 used for `VOICE_ACTIVE` ground truth.
//!
//! This tool checks `TONE_FRAME` directly against the same stimuli section 40 already captured, on
//! both rates: does AMBE+2 half-rate's chip genuinely *not* detect the tone at all (a real behavior
//! difference, matching its `Erasure` output), or does it detect the tone internally but simply
//! serialize that detection differently into `b0` for this rate (a labeling/encoding difference,
//! not a detection difference)? D-STAR's own already-confirmed `Tone`/`b0=126` result serves as a
//! positive control that this readback mechanism works correctly outside RATET(27).
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example p25_ambe_plus_2_and_dstar_tone_frame_ground_truth -- <host:port>`
use ham_digital_modes::ambe::float::dstar::decode::{
    classify_b0 as dstar_classify_b0, extract_raw_parameters as dstar_extract_raw,
    parse_frame as dstar_parse_frame,
};
use ham_digital_modes::ambe::float::dstar::interleave::wire_bytes_to_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::decode::{classify_b0, extract_raw_parameters};
use ham_digital_modes::ambe::float::ambe_plus_2::interleave::interleaved_to_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_RATET: u8 = 0x09;
const FIELD_ECMODE: u8 = 0x05;
const FIELD_CHANFMT: u8 = 0x15;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const SETTLING_FRAMES: usize = 80;
const CAPTURE_FRAMES: usize = 8;

const TD_ENABLE_BIT: u16 = 1 << 12;
const RATEP_DSTAR: [u16; 6] = [0x0130, 0x0763, 0x4000, 0x0000, 0x0000, 0x0048];
const RATET_HALF_RATE_FEC: u8 = 33;

fn build_control_ratep(rcw: [u16; 6]) -> Vec<u8> {
    let mut payload = vec![FIELD_RATEP];
    for v in rcw {
        payload.extend_from_slice(&v.to_be_bytes());
    }
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}
fn build_control_ratet(index: u8) -> Vec<u8> {
    let payload = vec![FIELD_RATET, index];
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}
fn build_control_ecmode(ecmode_in: u16) -> Vec<u8> {
    let mut payload = vec![FIELD_ECMODE];
    payload.extend_from_slice(&ecmode_in.to_be_bytes());
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}
fn build_control_chanfmt(data: u16) -> Vec<u8> {
    let mut payload = vec![FIELD_CHANFMT];
    payload.extend_from_slice(&data.to_be_bytes());
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}
fn build_speech(samples: &[i16]) -> Vec<u8> {
    let mut payload = vec![0x00_u8, samples.len() as u8];
    for &s in samples {
        payload.extend_from_slice(&s.to_be_bytes());
    }
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_SPEECH);
    pkt.extend_from_slice(&payload);
    pkt
}
fn parse_packet(data: &[u8]) -> Option<(u8, &[u8])> {
    if data.len() < 4 || data[0] != 0x61 {
        return None;
    }
    let length = u16::from_be_bytes([data[1], data[2]]) as usize;
    let ptype = data[3];
    data.get(4..4 + length).map(|payload| (ptype, payload))
}
fn sine_tone(freq: f64, amp: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (amp * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
        .collect()
}
fn dtmf_tone(row_hz: f64, col_hz: f64, amp: f64) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|n| {
            let t = n as f64 / SAMPLE_RATE;
            let s = (2.0 * std::f64::consts::PI * row_hz * t).sin()
                + (2.0 * std::f64::consts::PI * col_hz * t).sin();
            (amp * 0.5 * s) as i16
        })
        .collect()
}

fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 512], pkt: &[u8]) -> usize {
    for attempt in 0..8 {
        sock.send(pkt).expect("send");
        match sock.recv(buf) {
            Ok(n) => return n,
            Err(e) if attempt < 7 => {
                eprintln!("retrying after {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => panic!("recv channel after retries: {e}"),
        }
    }
    unreachable!()
}

/// Settles on `samples`, then returns the last `CAPTURE_FRAMES` full raw CHANNEL packets (with
/// `ECMODE_OUT` appended as the trailing 2 bytes, per `PKT_CHANFMT`'s own `0b01` request).
fn capture(sock: &UdpSocket, samples: &[i16]) -> Vec<Vec<u8>> {
    let mut buf = [0u8; 512];
    for _ in 0..SETTLING_FRAMES {
        let n = send_recv_retrying(sock, &mut buf, &build_speech(samples));
        parse_packet(&buf[..n]).expect("valid packet");
    }
    let mut out = Vec::with_capacity(CAPTURE_FRAMES);
    for _ in 0..CAPTURE_FRAMES {
        let n = send_recv_retrying(sock, &mut buf, &build_speech(samples));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        out.push(buf[..n].to_vec());
    }
    out
}

fn tone_frame_bit(pkt: &[u8]) -> u16 {
    let ecmode_out = u16::from_be_bytes([pkt[pkt.len() - 2], pkt[pkt.len() - 1]]);
    (ecmode_out >> 15) & 1
}
fn channel_bits(pkt: &[u8]) -> &[u8] {
    // Confirmed against this crate's own established BITS_OFFSET=6 convention (e.g.
    // ambe_chip_validate_ratet27.rs): pkt[0]=0x61 sync, pkt[1..3]=length, pkt[3]=ptype,
    // pkt[4]=0x00 (channel sub-field ID), pkt[5]=num_bits, pkt[6..]=bit data, then (only because
    // PKT_CHANFMT requested it with 0b01) a trailing 2-byte ECMODE_OUT appended after the bits.
    &pkt[6..pkt.len() - 2]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    let rows = [697.0, 770.0, 852.0, 941.0];
    let cols = [1209.0, 1336.0, 1477.0, 1633.0];
    let digit_names = [["1", "2", "3", "A"], ["4", "5", "6", "B"], ["7", "8", "9", "C"], ["*", "0", "#", "D"]];

    println!("=== AMBE+2 half-rate (RATET {RATET_HALF_RATE_FEC}): TONE_FRAME ground truth ===");
    sock.send(&build_control_ratet(RATET_HALF_RATE_FEC)).expect("send RATET config");
    let n = sock.recv(&mut buf).expect("RATET config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_ecmode(TD_ENABLE_BIT)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b01)).expect("send CHANFMT config");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let loud = sine_tone(200.0, 16000.0);
    println!("-- loud 200Hz tone --");
    for pkt in capture(&sock, &loud) {
        let bits = channel_bits(&pkt);
        let mut wire: u128 = 0;
        for &b in bits.iter().take(9) {
            wire = (wire << 8) | b as u128;
        }
        let logical = interleaved_to_frame(wire);
        let parsed = parse_frame(logical);
        let raw = extract_raw_parameters(parsed.d);
        println!("  b0={} kind={:?} TONE_FRAME={}", raw.b0, classify_b0(raw.b0), tone_frame_bit(&pkt));
    }
    println!("-- 16 DTMF digits (all {CAPTURE_FRAMES} captured frames each, not just the last) --");
    for (ri, &row) in rows.iter().enumerate() {
        for (ci, &col) in cols.iter().enumerate() {
            let samples = dtmf_tone(row, col, 9000.0);
            let pkts = capture(&sock, &samples);
            let decoded: Vec<(u32, ham_digital_modes::ambe::float::ambe_plus_2::decode::FrameKind, u16)> = pkts
                .iter()
                .map(|pkt| {
                    let bits = channel_bits(pkt);
                    let mut wire: u128 = 0;
                    for &b in bits.iter().take(9) {
                        wire = (wire << 8) | b as u128;
                    }
                    let logical = interleaved_to_frame(wire);
                    let parsed = parse_frame(logical);
                    let raw = extract_raw_parameters(parsed.d);
                    (raw.b0, classify_b0(raw.b0), tone_frame_bit(pkt))
                })
                .collect();
            println!("  digit {}: {:?}", digit_names[ri][ci], decoded);
        }
    }

    println!("\n=== D-STAR: TONE_FRAME ground truth (positive control -- b0 already known to reach Tone) ===");
    sock.send(&build_control_ratep(RATEP_DSTAR)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_ecmode(TD_ENABLE_BIT)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b01)).expect("send CHANFMT config");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let loud = sine_tone(200.0, 16000.0);
    println!("-- loud 200Hz tone --");
    for pkt in capture(&sock, &loud) {
        let bits = channel_bits(&pkt);
        let mut wire_bytes = [0u8; 9];
        wire_bytes.copy_from_slice(&bits[..9]);
        let frame = wire_bytes_to_frame(&wire_bytes);
        let parsed = dstar_parse_frame(frame);
        let raw = dstar_extract_raw(parsed.d);
        println!("  b0={} kind={:?} TONE_FRAME={}", raw.b0, dstar_classify_b0(raw.b0), tone_frame_bit(&pkt));
    }
    println!("-- one representative DTMF digit (row 1, col 1), all {CAPTURE_FRAMES} captured frames --");
    let samples = dtmf_tone(770.0, 1336.0, 9000.0);
    let pkts = capture(&sock, &samples);
    for pkt in &pkts {
        let bits = channel_bits(pkt);
        let mut wire_bytes = [0u8; 9];
        wire_bytes.copy_from_slice(&bits[..9]);
        let frame = wire_bytes_to_frame(&wire_bytes);
        let parsed = dstar_parse_frame(frame);
        let raw = dstar_extract_raw(parsed.d);
        println!("  digit 5: b0={} kind={:?} TONE_FRAME={}", raw.b0, dstar_classify_b0(raw.b0), tone_frame_bit(pkt));
    }

    // Reset to a known-clean state.
    sock.send(&build_control_ecmode(TD_ENABLE_BIT)).expect("send ECMODE reset");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b00)).expect("send CHANFMT reset");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");
}
