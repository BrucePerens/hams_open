// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! What the chip's ENCODER emits for silence and low-level noise, in D-STAR and AMBE+2 half-rate: prints, for digital
//! silence and white noise of several amplitudes, each frame's raw parameters (`b0..b8`) or its classification, so
//! this crate's encoders can copy the chip's silence / unvoiced-frame conventions (for example D-STAR's `b0 = 34`).
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example ambe_chip_silence_probe -- <host:port>`

use ham_digital_modes::ambe::float::ambe_plus_2::decode as a2;
use ham_digital_modes::ambe::float::ambe_plus_2::interleave::interleaved_to_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame as a2_parse;
use ham_digital_modes::ambe::float::dstar::decode as ds;
use ham_digital_modes::ambe::float::dstar::interleave::wire_bytes_to_frame as ds_wire_to_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const FIELD_RATEP: u8 = 0x0A;
const RATEP_DSTAR: [u16; 6] = [0x0130, 0x0763, 0x4000, 0x0000, 0x0000, 0x0048];
const RATET_HALF_RATE_FEC: u8 = 33;
const FRAME_SAMPLES: usize = 160;

fn control(field: u8, body: &[u8]) -> Vec<u8> {
    let mut payload = vec![field];
    payload.extend_from_slice(body);
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
    data.get(4..4 + length).map(|p| (data[3], p))
}
fn parse_speech_payload(payload: &[u8]) -> Vec<i16> {
    let count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    payload[2..2 + count * 2].chunks_exact(2).map(|b| i16::from_be_bytes([b[0], b[1]])).collect()
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
            Err(e) => panic!("recv after retries: {e}"),
        }
    }
    unreachable!()
}
fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE");
    assert_eq!(&data[36..40], b"data");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}



fn wire_bytes_to_a2_frame(b: &[u8; 9]) -> u128 {
    let mut wire: u128 = 0;
    for &x in b {
        wire = (wire << 8) | x as u128;
    }
    interleaved_to_frame(wire)
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64 / (1u64 << 31) as f64) * 2.0 - 1.0
    }
}

fn encode_frames(sock: &UdpSocket, buf: &mut [u8; 1024], pcm: &[i16]) -> Vec<[u8; 9]> {
    let mut out = Vec::new();
    for chunk in pcm.chunks_exact(FRAME_SAMPLES) {
        let pkt = build_speech(chunk);
        let payload = loop {
            let n = send_recv_retrying(sock, buf, &pkt);
            match parse_packet(&buf[..n]) {
                Some((TYPE_CHANNEL, p)) => break p.to_vec(),
                _ => continue,
            }
        };
        let mut wb = [0u8; 9];
        wb.copy_from_slice(&payload[2..11]);
        out.push(wb);
    }
    out
}

fn describe_dstar(wb: &[u8; 9], st: &mut ds::DStarDecoderState) -> String {
    let f = ds_wire_to_frame(wb);
    let parsed = ds::parse_frame(f);
    let raw = ds::extract_raw_parameters(parsed.d);
    let kind = ds::classify_b0(raw.b0);
    let _ = ds::dequantize(parsed.d, st);
    format!("{kind:?} b0={} b1={} b2={} b3={} b4={} b5={} b6={} b7={} b8={} err={}+{}", raw.b0, raw.b1, raw.b2, raw.b3, raw.b4, raw.b5, raw.b6, raw.b7, raw.b8, parsed.epsilon_c0, parsed.epsilon_c1)
}

fn describe_a2(wb: &[u8; 9], st: &mut a2::DecoderState) -> String {
    let f = wire_bytes_to_a2_frame(wb);
    let parsed = a2_parse(f);
    let raw = a2::extract_raw_parameters(parsed.d);
    let kind = match a2::dequantize(&raw, st) {
        a2::DequantizedFrame::Speech(_) => "Speech".to_string(),
        a2::DequantizedFrame::Erasure => "Erasure".to_string(),
        a2::DequantizedFrame::Silence { .. } => "Silence".to_string(),
        a2::DequantizedFrame::Tone { .. } => "Tone".to_string(),
    };
    format!("{kind} b0={} b1={} b2={} b3={} b4={} b5={} b6={} b7={} b8={} err={}+{}", raw.b0, raw.b1, raw.b2, raw.b3, raw.b4, raw.b5, raw.b6, raw.b7, raw.b8, parsed.epsilon_c0, parsed.epsilon_c1)
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    let signals: Vec<(String, Vec<i16>)> = {
        let mut v = vec![("digital silence".to_string(), vec![0i16; FRAME_SAMPLES * 12])];
        for amp in [2.0, 8.0, 30.0, 150.0, 1000.0] {
            let mut rng = Lcg(12345);
            v.push((format!("white noise amplitude {amp}"), (0..FRAME_SAMPLES * 12).map(|_| (rng.next() * amp) as i16).collect()));
        }
        v
    };
    for mode in ["dstar", "ambe_plus_2"] {
        if mode == "dstar" {
            let mut body = Vec::new();
            for v in RATEP_DSTAR {
                body.extend_from_slice(&v.to_be_bytes());
            }
            sock.send(&control(FIELD_RATEP, &body)).unwrap();
        } else {
            sock.send(&control(FIELD_RATET, &[RATET_HALF_RATE_FEC])).unwrap();
        }
        let n = sock.recv(&mut buf).unwrap();
        parse_packet(&buf[..n]).unwrap();
        sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        while sock.recv(&mut buf).is_ok() {}
        sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        println!("=== {mode}");
        for (name, pcm) in &signals {
            println!("-- {name}");
            let frames = encode_frames(&sock, &mut buf, pcm);
            let (mut sd, mut sa) = (ds::DStarDecoderState::initial(), a2::DecoderState::initial());
            for (i, wb) in frames.iter().enumerate() {
                let text = if mode == "dstar" { describe_dstar(wb, &mut sd) } else { describe_a2(wb, &mut sa) };
                println!("  {i:2} {} {}", wb.iter().map(|b| format!("{b:02x}")).collect::<String>(), text);
            }
        }
    }
}
