// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Encode-direction comparison of this crate's RATET(27) [`Encoder`] against the real chip's encoder, on real
//! speech. For each of a set of analysis-centre offsets it (1) encodes the input with our encoder, (2)
//! compares our frames' decoded parameters with the chip encoder's frames for the same input (pitch index `b0`
//! agreement, harmonic count, voiced fraction, gain index `b2`), and (3) sends our frames to the chip's decoder
//! to get chip-rendered PCM, reporting frame-RMS envelope correlation against the original input for: chip
//! encode->chip decode, our encode->chip decode, and our encode->our decode.
//!
//! Usage: `cargo run --release --example ratet27_chip_encode_compare -- <host:port> [wav] [n_frames]`

use ham_digital_modes::ambe::float::ratet27::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::ratet27::encoder::Encoder;
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


fn c_to_wire_bytes(c: &[u32; 8]) -> [u8; FRAME_BYTES] {
    let blocks = [
        Block::Golay { index: 0 },
        Block::Golay { index: 1 },
        Block::Golay { index: 2 },
        Block::Golay { index: 3 },
        Block::Hamming { index: 0 },
        Block::Hamming { index: 1 },
        Block::Hamming { index: 2 },
        Block::Raw,
    ];
    let mut bits = [false; 144];
    for (i, &block) in blocks.iter().enumerate() {
        let members = block_wire_members(block);
        for (offset, &wire) in members.iter().enumerate() {
            bits[wire] = (c[i] >> (members.len() - 1 - offset)) & 1 == 1;
        }
    }
    let mut bytes = [0u8; FRAME_BYTES];
    for (i, &b) in bits.iter().enumerate() {
        if b {
            bytes[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    bytes
}

fn corr(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    let (a, b) = (&a[..n], &b[..n]);
    let (ma, mb) = (a.iter().sum::<f64>() / n as f64, b.iter().sum::<f64>() / n as f64);
    let (mut c, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        c += (x - ma) * (y - mb);
        va += (x - ma).powi(2);
        vb += (y - mb).powi(2);
    }
    c / (va.sqrt() * vb.sqrt()).max(1e-12)
}
fn env(x: &[f64]) -> Vec<f64> {
    x.chunks_exact(FRAME_SAMPLES).map(|c| (c.iter().map(|s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt()).collect()
}

struct Params {
    b0: i32,
    l_hat: u32,
    voiced_frac: f64,
    b2: u32,
}
fn params_of(frames: &[[u32; 8]]) -> Vec<Option<Params>> {
    let mut d = DecoderState::new();
    frames
        .iter()
        .map(|c| match d.decode_parameters(*c) {
            Some(FrameOutcome::Decoded(p)) => {
                let v = p.voiced.iter().filter(|&&x| x).count() as f64 / p.voiced.len().max(1) as f64;
                let r = Some(Params { b0: p.bits.b0 as i32, l_hat: p.l_hat, voiced_frac: v, b2: p.bits.b2 });
                d.advance_history(&p);
                r
            }
            _ => None,
        })
        .collect()
}

fn chip_decode(sock: &UdpSocket, buf: &mut [u8; 1024], header: &[u8], frames: &[[u32; 8]]) -> Vec<f64> {
    let mut pcm = Vec::new();
    for c in frames {
        let mut payload = header.to_vec();
        payload.extend_from_slice(&c_to_wire_bytes(c));
        let n = send_recv_retrying(sock, buf, &build_channel(&payload));
        let (t, p) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(t, TYPE_SPEECH, "expected decoded speech");
        pcm.extend(parse_speech_payload(p).iter().map(|&s| s as f64));
    }
    pcm
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let wav = std::env::args().nth(2).unwrap_or_else(|| "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav".to_string());
    let n_frames: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(150);
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();

    let pcm = read_wav_mono_i16(&wav);
    let n_frames = n_frames.min(pcm.len() / FRAME_SAMPLES);
    let input: Vec<f64> = pcm[..n_frames * FRAME_SAMPLES].iter().map(|&s| s as f64).collect();

    // Chip encode, then chip decode of its own frames.
    let mut chip_frames: Vec<[u32; 8]> = Vec::new();
    let mut header = Vec::new();
    for i in 0..n_frames {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES]));
        let (_, payload) = parse_packet(&buf[..n]).unwrap();
        let mut wb = [0u8; FRAME_BYTES];
        wb.copy_from_slice(&payload[payload.len() - FRAME_BYTES..]);
        header = payload[..payload.len() - FRAME_BYTES].to_vec();
        chip_frames.push(wire_bytes_to_c(&wb));
    }
    let chip_chip = chip_decode(&sock, &mut buf, &header, &chip_frames);
    let chip_params = params_of(&chip_frames);
    let env_in = env(&input);
    println!("chip encode -> chip decode: envelope corr vs input {:.4}", corr(&env_in, &env(&chip_chip)));

    for offset in [-160i32, -120, -80, -40, 0, 40, 80, 120, 160] {
        let mut enc = Encoder::new();
        enc.set_center_offset(offset);
        enc.push_samples(&input);
        let mut ours: Vec<[u32; 8]> = Vec::new();
        while let Some(f) = enc.next_frame() {
            ours.push(f);
        }
        ours.extend(enc.finish());
        ours.truncate(n_frames);
        let our_params = params_of(&ours);
        let (mut both, mut b0_close, mut l_eq, mut b2_close, mut vf_sum) = (0usize, 0usize, 0usize, 0usize, 0.0);
        for (a, b) in our_params.iter().zip(chip_params.iter()) {
            if let (Some(a), Some(b)) = (a, b) {
                both += 1;
                b0_close += ((a.b0 - b.b0).abs() <= 2) as usize;
                l_eq += (a.l_hat == b.l_hat) as usize;
                b2_close += ((a.b2 as i32 - b.b2 as i32).abs() <= 3) as usize;
                vf_sum += (a.voiced_frac - b.voiced_frac).abs();
            }
        }
        if std::env::var("SHOW_B0").is_ok() && offset == 80 {
            let pairs: Vec<String> = our_params.iter().zip(chip_params.iter()).enumerate().filter_map(|(i, (a, b))| Some(format!("{i}:{}/{}", a.as_ref()?.b0, b.as_ref()?.b0))).collect();
            println!("our_b0/chip_b0 at offset 80: {}", pairs.join(" "));
        }
        let ours_chip = chip_decode(&sock, &mut buf, &header, &ours);
        let mut d = DecoderState::new();
        let ours_ours: Vec<f64> = ours.iter().flat_map(|c| d.decode_frame(*c).unwrap_or([0.0; 160])).collect();
        println!(
            "offset {offset:5}: frames {} both-decoded {both}; b0 within 2: {:.2}, L equal: {:.2}, b2 within 3: {:.2}, mean |voiced-fraction diff| {:.2}; envelope corr vs input: our-enc->chip-dec {:.4}, our-enc->our-dec {:.4}",
            ours.len(),
            b0_close as f64 / both.max(1) as f64,
            l_eq as f64 / both.max(1) as f64,
            b2_close as f64 / both.max(1) as f64,
            vf_sum / both.max(1) as f64,
            corr(&env_in, &env(&ours_chip)),
            corr(&env_in, &env(&ours_ours)),
        );
    }
}
