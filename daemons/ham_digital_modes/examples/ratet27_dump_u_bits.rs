// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Dumps the chip's RATET(27) encoder output for an entire speech file as FEC-decoded data bits, one frame per line
//! (`u0..u7` as binary strings of widths 12,12,12,12,11,11,11,7, space separated), for offline statistical analysis of
//! which bits form fields (mutual information clustering, correlation with input features).
//!
//! Usage: `cargo run --release --example ratet27_dump_u_bits -- <host:port> <wav> <out.txt>`

use ham_digital_modes::ambe::dvsi_p25fec::fec::hamming_decode_chip;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::general::fec::golay_decode;
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


fn main() {
    let host = std::env::args().nth(1).unwrap();
    let wav = std::env::args().nth(2).unwrap();
    let out_path = std::env::args().nth(3).unwrap();
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    let pcm = read_wav_mono_i16(&wav);
    let widths = [12usize, 12, 12, 12, 11, 11, 11, 7];
    let mut out = String::new();
    for i in 0..pcm.len() / FRAME_SAMPLES {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES]));
        let (_, payload) = parse_packet(&buf[..n]).unwrap();
        let mut wb = [0u8; FRAME_BYTES];
        wb.copy_from_slice(&payload[payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wb);
        let u = [
            golay_decode(c[0]).0 as u32,
            golay_decode(c[1]).0 as u32,
            golay_decode(c[2]).0 as u32,
            golay_decode(c[3]).0 as u32,
            hamming_decode_chip(c[4] as u16).0 as u32,
            hamming_decode_chip(c[5] as u16).0 as u32,
            hamming_decode_chip(c[6] as u16).0 as u32,
            c[7],
        ];
        let line: Vec<String> = (0..8).map(|k| format!("{:0w$b}", u[k], w = widths[k])).collect();
        out.push_str(&line.join(" "));
        out.push('\n');
    }
    std::fs::write(out_path, out).unwrap();
}
