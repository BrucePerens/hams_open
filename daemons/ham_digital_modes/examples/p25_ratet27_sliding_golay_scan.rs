// SPDX-License-Identifier: LGPL-3.0-or-later
//! Sliding 23-bit Golay-window scan across real RATET(27) captured frames -- not assuming the
//! textbook IMBE `c0..c7` block positions at all, since tonight's AMBE+2 investigation found the
//! real chip's parameter layout for its "half-rate" configuration is genuinely AMBE-family
//! (Gray-coded pitch, scattered positions), not textbook, even though the standard's own published
//! text describes something else. This scan checks every possible 23-bit starting position (both
//! byte-order/bit-direction hypotheses) across many real captured frames for a Golay-validity rate
//! dramatically above chance -- the same falsification-test logic used throughout this
//! investigation, but without assuming fixed field boundaries this time.
use ham_digital_modes::ambe::general::fec::golay_encode;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const TEST_FREQS_HZ: [f64; 8] = [50.0, 100.0, 200.0, 250.0, 400.0, 500.0, 800.0, 1000.0];
const SETTLING_FRAMES: usize = 40;
const CAPTURED_FRAMES: usize = 10;
const TOTAL_BITS: usize = 144;

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
fn test_tone(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (8000.0 * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
        .collect()
}

fn build_golay_bitmap() -> Vec<u64> {
    let mut bm = vec![0u64; (1 << 23) / 64];
    for data in 0u32..4096 {
        let cw = golay_encode(data as u16) as usize;
        bm[cw >> 6] |= 1u64 << (cw & 63);
    }
    bm
}
fn golay_valid(bm: &[u64], cw: u32) -> bool {
    let cw = cw as usize & 0x7F_FFFF;
    (bm[cw >> 6] >> (cw & 63)) & 1 == 1
}

fn frame_to_bits(bytes: &[u8; 18], reverse_bytes: bool, lsb_first: bool) -> [u8; TOTAL_BITS] {
    let mut b = *bytes;
    if reverse_bytes {
        b.reverse();
    }
    let mut bits = [0u8; TOTAL_BITS];
    for (i, &byte) in b.iter().enumerate() {
        for bit in 0..8 {
            bits[i * 8 + bit] = if lsb_first { (byte >> bit) & 1 } else { (byte >> (7 - bit)) & 1 };
        }
    }
    bits
}
fn extract(bits: &[u8; TOTAL_BITS], start: usize, len: usize) -> u32 {
    let mut v = 0u32;
    for i in 0..len {
        v = (v << 1) | bits[start + i] as u32;
    }
    v
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let mut buf = [0u8; 512];
    let n = sock.recv(&mut buf).expect("RATEP config response");
    let (ptype, payload) = parse_packet(&buf[..n]).expect("valid DVSI packet");
    println!("RATEP(P25 FEC) config ack: type={ptype:#04x} payload={payload:02x?}");

    let mut frames: Vec<[u8; 18]> = Vec::new();
    for &freq in &TEST_FREQS_HZ {
        let samples = test_tone(freq);
        for i in 0..(SETTLING_FRAMES + CAPTURED_FRAMES) {
            sock.send(&build_speech(&samples)).expect("send speech");
            let n = sock.recv(&mut buf).expect("recv channel");
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let num_bits = payload[1] as usize;
            assert_eq!(num_bits, 144, "expected 144 bits for RATEP P25 FEC");
            let nbytes = num_bits.div_ceil(8);
            let mut last = [0u8; 18];
            last[..nbytes].copy_from_slice(&payload[2..2 + nbytes]);
            if i >= SETTLING_FRAMES {
                frames.push(last);
            }
        }
    }
    let unique: Vec<[u8; 18]> = {
        let set: std::collections::HashSet<[u8; 18]> = frames.into_iter().collect();
        set.into_iter().collect()
    };
    println!("{} unique captured frames across {} frequencies", unique.len(), TEST_FREQS_HZ.len());

    let golay_bm = build_golay_bitmap();

    println!("\n--- Sliding 23-bit Golay-window scan (every start position, all 4 byte/bit hypotheses) ---");
    let mut best: Vec<(usize, bool, bool, usize)> = Vec::new();
    for &reverse_bytes in &[false, true] {
        for &lsb_first in &[false, true] {
            let bitstreams: Vec<[u8; TOTAL_BITS]> = unique.iter().map(|f| frame_to_bits(f, reverse_bytes, lsb_first)).collect();
            for start in 0..=(TOTAL_BITS - 23) {
                let count = bitstreams.iter().filter(|b| golay_valid(&golay_bm, extract(b, start, 23))).count();
                if count > unique.len() / 2 {
                    best.push((start, reverse_bytes, lsb_first, count));
                }
            }
        }
    }
    best.sort_by(|a, b| b.3.cmp(&a.3));
    if best.is_empty() {
        println!("No window position scored above 50% Golay-validity under any hypothesis -- clean negative.");
        // Report the single best score anyway, for context.
        let mut overall_best = (0usize, false, false, 0usize);
        for &reverse_bytes in &[false, true] {
            for &lsb_first in &[false, true] {
                let bitstreams: Vec<[u8; TOTAL_BITS]> = unique.iter().map(|f| frame_to_bits(f, reverse_bytes, lsb_first)).collect();
                for start in 0..=(TOTAL_BITS - 23) {
                    let count = bitstreams.iter().filter(|b| golay_valid(&golay_bm, extract(b, start, 23))).count();
                    if count > overall_best.3 {
                        overall_best = (start, reverse_bytes, lsb_first, count);
                    }
                }
            }
        }
        println!(
            "  best single position anyway: start={} reverse_bytes={} lsb_first={}: {}/{} ({:.1}%)",
            overall_best.0, overall_best.1, overall_best.2, overall_best.3, unique.len(),
            100.0 * overall_best.3 as f64 / unique.len() as f64
        );
    } else {
        for (start, reverse_bytes, lsb_first, count) in best.iter().take(20) {
            println!(
                "  start={start} reverse_bytes={reverse_bytes} lsb_first={lsb_first}: {count}/{} ({:.1}%)",
                unique.len(), 100.0 * *count as f64 / unique.len() as f64
            );
        }
    }
}
