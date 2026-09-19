// SPDX-License-Identifier: LGPL-3.0-or-later
//! Captures real chip channel frames and the chip's own decoded PCM, and dumps them to files so an
//! independent decoder (mbelib, built from `/home/bruce/workspace/tmp/mbelib_research/mbelib`) can be run
//! on the identical bits: `<out>/ratet27_frames.txt` (one line per frame: eight hex words `c0..c7`, the
//! raw pre-FEC codewords, plus this crate's own decoded `b0 l_hat` for cross-checking bit-order
//! conventions), and `<out>/ratet27_chip.raw` (chip PCM, little-endian i16), `<out>/ratet27_float.raw`
//! (this crate's float PCM, little-endian f32).
//!
//! Usage: `cargo run --release --example ratet27_dump_frames_for_mbelib -- <host:port> [out_dir] [wav]`

use ham_digital_modes::ambe::float::ratet27::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::ratet27::ratet27_wire_format::{block_wire_members, Block};
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
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let out_dir = std::env::args().nth(2).unwrap_or_else(|| "/tmp".to_string());
    let wav = std::env::args()
        .nth(3)
        .unwrap_or_else(|| "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let pcm = read_wav_mono_i16(&wav);
    let n_frames = (pcm.len() / FRAME_SAMPLES).min(N_FRAMES);
    let mut payloads: Vec<Vec<u8>> = Vec::new();
    for i in 0..n_frames {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        payloads.push(payload.to_vec());
    }

    let mut params_decoder = DecoderState::new();
    let mut float_decoder = DecoderState::new();
    let mut lines = String::new();
    let mut chip_bytes: Vec<u8> = Vec::new();
    let mut float_bytes: Vec<u8> = Vec::new();
    for payload in &payloads {
        let mut wire = [0u8; FRAME_BYTES];
        wire.copy_from_slice(&payload[payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire);
        let info = match params_decoder.decode_parameters(c) {
            Some(FrameOutcome::Decoded(p)) => {
                let s = format!("{} {}", p.bits.b0, p.l_hat);
                params_decoder.advance_history(&p);
                s
            }
            Some(FrameOutcome::Repeat) => "R 0".to_string(),
            Some(FrameOutcome::Mute) => "M 0".to_string(),
            None => "N 0".to_string(),
        };
        lines.push_str(&format!(
            "{:x} {:x} {:x} {:x} {:x} {:x} {:x} {:x} {info}\n",
            c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]
        ));
        let n = send_recv_retrying(&sock, &mut buf, &build_channel(payload));
        let (_, sp) = parse_packet(&buf[..n]).expect("valid packet");
        for s in parse_speech_payload(sp) {
            chip_bytes.extend_from_slice(&s.to_le_bytes());
        }
        match float_decoder.decode_frame(c) {
            Some(f) => f.iter().for_each(|&x| float_bytes.extend_from_slice(&(x as f32).to_le_bytes())),
            None => (0..FRAME_SAMPLES).for_each(|_| float_bytes.extend_from_slice(&0f32.to_le_bytes())),
        }
    }
    std::fs::write(format!("{out_dir}/ratet27_frames.txt"), lines).unwrap();
    std::fs::write(format!("{out_dir}/ratet27_chip.raw"), chip_bytes).unwrap();
    std::fs::write(format!("{out_dir}/ratet27_float.raw"), float_bytes).unwrap();
    println!("wrote {n_frames} frames to {out_dir}");
}
