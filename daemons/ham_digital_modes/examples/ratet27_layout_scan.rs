// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Layout scan for a chip rate setting: captures encoder frames for harmonic signals and, for each of several
//! interpretations of the 18 payload bytes (the chip's own proprietary layout used by this crate; the standard P25
//! over-the-air dibit interleave read MSB-first, and LSB-first; each with and without the modulation/whitening
//! step), counts the FEC errors on `c0..c6` (Golay/Hamming distances) and how often `b0` is in the valid TIA range.
//! A layout that really is standard P25 IMBE shows ~0 errors and valid `b0` on every frame.
//!
//! **Result (live chip)**: with the P25-FEC RATEP words the crate's chip layout gives 0 FEC errors (the standard OTA
//! interleave, with or without modulation, gives random-level ~14/frame). With `RATET=27` and `RATET=59` (the index a
//! web-sourced note claims is "P25 Phase 1 IMBE"; unverified) every layout tried gives random-level errors, so those
//! settings emit a layout none of these interpretations decode; no chip setting has been found that yields standard
//! P25 IMBE frames.
//!
//! Usage: `RATET=<n> cargo run --release --example ratet27_layout_scan -- <host:port>` (without `RATET`, the P25-FEC
//! RATEP words are used).

use ham_digital_modes::ambe::float::tia_102_baba::interleave::deinterleave_from_dibit_symbols;
use ham_digital_modes::ambe::float::tia_102_baba::modulation::modulate_code_vectors;
use ham_digital_modes::ambe::dvsi_p25fec::fec::hamming_decode_chip;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::general::fec::{golay_decode, hamming_decode};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_FRAMES: usize = 200;

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
fn build_channel(payload: &[u8]) -> Vec<u8> {
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CHANNEL);
    pkt.extend_from_slice(payload);
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
fn parse_speech_payload(payload: &[u8]) -> Vec<i16> {
    let count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    payload[2..2 + count * 2]
        .chunks_exact(2)
        .map(|b| i16::from_be_bytes([b[0], b[1]]))
        .collect()
}
fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 1024], pkt: &[u8]) -> usize {
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
fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}
fn wire_bytes_to_c(bytes: &[u8; FRAME_BYTES]) -> [u32; 8] {
    let mut wire_frame_bits = [false; 144];
    for (byte_idx, &byte) in bytes.iter().enumerate() {
        for b in 0..8 {
            wire_frame_bits[byte_idx * 8 + b] = (byte >> (7 - b)) & 1 == 1;
        }
    }
    let raw = |block: Block| -> u32 {
        let members = block_wire_members(block);
        let mut received: u32 = 0;
        for (offset, &wire) in members.iter().enumerate() {
            if wire_frame_bits[wire] {
                received |= 1 << (members.len() - 1 - offset);
            }
        }
        received
    };
    [
        raw(Block::Golay { index: 0 }),
        raw(Block::Golay { index: 1 }),
        raw(Block::Golay { index: 2 }),
        raw(Block::Golay { index: 3 }),
        raw(Block::Hamming { index: 0 }),
        raw(Block::Hamming { index: 1 }),
        raw(Block::Hamming { index: 2 }),
        raw(Block::Raw),
    ]
}


fn errors(c: &[u32; 8], chip_hamming: bool) -> u32 {
    let mut e = 0;
    for &x in &c[..4] {
        e += golay_decode(x).1;
    }
    for &x in &c[4..7] {
        e += if chip_hamming { hamming_decode_chip(x as u16).1 } else { hamming_decode(x as u16).1 };
    }
    e
}

fn dibits(bytes: &[u8], msb_first: bool) -> [(bool, bool); 72] {
    let mut bits = Vec::new();
    for &b in bytes {
        for i in 0..8 {
            bits.push(if msb_first { (b >> (7 - i)) & 1 == 1 } else { (b >> i) & 1 == 1 });
        }
    }
    std::array::from_fn(|i| (bits[2 * i], bits[2 * i + 1]))
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    if let Ok(idx) = std::env::var("RATET") {
        let pkt = vec![0x61_u8, 0x00, 0x02, TYPE_CONTROL, 0x09, idx.parse().unwrap()];
        sock.send(&pkt).unwrap();
    } else {
        sock.send(&build_control_ratep(RATEP_P25_FEC)).unwrap();
    }
    let n = sock.recv(&mut buf).unwrap();
    println!("rate config reply: {:02x?}", &buf[..n]);
    let mut totals = [0u32; 8];
    let mut frames = 0;
    let names = ["chip layout (this crate)", "OTA msb-first, no demod", "OTA msb-first, demod", "OTA lsb-first, no demod", "OTA lsb-first, demod", "chip layout, TIA hamming", "-", "-"];
    for period in [45.0f64, 60.0, 80.0] {
        for f in 0..14usize {
            let frame: Vec<i16> = (0..FRAME_SAMPLES)
                .map(|i| (1..=8).map(|h| 1500.0 / h as f64 * (2.0 * std::f64::consts::PI * h as f64 * (f * FRAME_SAMPLES + i) as f64 / period).sin()).sum::<f64>() as i16)
                .collect();
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&frame));
            let (_, payload) = parse_packet(&buf[..n]).unwrap();
            if f < 6 {
                continue;
            }
            if f == 8 && period == 60.0 {
                println!("payload ({} bytes): {:02x?}", payload.len(), payload);
            }
            let bytes = &payload[payload.len() - FRAME_BYTES..];
            frames += 1;
            let mut wb = [0u8; FRAME_BYTES];
            wb.copy_from_slice(bytes);
            let c_chip = wire_bytes_to_c(&wb);
            totals[0] += errors(&c_chip, true);
            totals[5] += errors(&c_chip, false);
            for (k, msb) in [(1usize, true), (3, false)] {
                let c = deinterleave_from_dibit_symbols(dibits(bytes, msb));
                totals[k] += errors(&c, false);
                let u0 = golay_decode(c[0]).0 as u32;
                let d = modulate_code_vectors(c, u0);
                totals[k + 1] += errors(&d, false);
            }
        }
    }
    println!("total FEC errors over {frames} frames (a true layout gives ~0; random data gives ~{} per frame):", 3 * 4 + 3);
    for k in 0..6 {
        println!("  {:28} {:5}  ({:.2} per frame)", names[k], totals[k], totals[k] as f64 / frames as f64);
    }
}
