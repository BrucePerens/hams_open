// SPDX-License-Identifier: LGPL-3.0-or-later
//! Closes the caveat left by `p25_ratet27_probe_dtx_voice_active.rs`: that tool's 3
//! `VOICE_ACTIVE=1` test points were all the same noise realization merely rescaled, so `g3`'s
//! observed constancy there could reflect shared signal shape rather than a genuine "always
//! constant during confirmed voice" property. This tool tests `g3` against ground-truth
//! `VOICE_ACTIVE` across genuinely varied loud content -- several independent noise seeds, real
//! tones at different frequencies, and real recorded speech -- all well above the confirmed
//! silence/voice amplitude threshold (peak 50-75, section 28).
//!
//! Usage: `cargo run --release --example p25_ratet27_probe_g3_during_varied_voice -- <host:port>`
use ham_digital_modes::ambe::general::fec::golay_decode;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_ECMODE: u8 = 0x05;
const FIELD_CHANFMT: u8 = 0x15;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const DTX_ENABLE_BIT: u16 = 1 << 11;
const TD_ENABLE_BIT: u16 = 1 << 12;
const SETTLING_FRAMES: usize = 40;
const CAPTURE_FRAMES: usize = 10;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
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
fn lcg_noise(seed: u64, peak: f64) -> Vec<i16> {
    let mut state = seed;
    (0..FRAME_SAMPLES)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let unit = ((state >> 33) as f64 / (1u64 << 31) as f64) - 1.0;
            (unit * peak) as i16
        })
        .collect()
}
fn sawtooth(freq: f64, amp: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            (amp * (2.0 * phase - 1.0)) as i16
        })
        .collect()
}
fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_ecmode(DTX_ENABLE_BIT | TD_ENABLE_BIT)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b01)).expect("send CHANFMT config");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let send_recv_retrying = |sock: &UdpSocket, buf: &mut [u8; 512], pkt: &[u8]| -> usize {
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
    };

    let mut stimuli: Vec<(String, Vec<i16>)> = vec![];
    for seed in [1u64, 2, 3, 4, 5] {
        stimuli.push((format!("noise_seed{seed}"), lcg_noise(seed, 9000.0)));
    }
    for freq in [100.0, 200.0, 300.0, 400.0] {
        stimuli.push((format!("tone_{freq}"), sawtooth(freq, 9000.0)));
    }
    let speech = read_wav_mono_i16("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
    for chunk in 0..5 {
        let start = chunk * 2000;
        stimuli.push((format!("speech_chunk{chunk}"), speech[start..start + FRAME_SAMPLES].to_vec()));
    }

    for (label, samples) in &stimuli {
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        for i in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(samples));
            let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            assert_eq!(payload[1] as usize, TOTAL_BITS);
            let pkt = &buf[..n];
            let ecmode_out = u16::from_be_bytes([pkt[n - 2], pkt[n - 1]]);
            let voice_active = (ecmode_out >> 1) & 1;
            let bits_bytes = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let mut wire_frame_bits = [false; TOTAL_BITS];
            for (byte_idx, &byte) in bits_bytes.iter().enumerate() {
                for bit_idx in 0..8 {
                    wire_frame_bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
                }
            }
            let g3_members = block_wire_members(Block::Golay { index: 3 });
            let mut g3_received: u32 = 0;
            for (offset, &wire) in g3_members.iter().enumerate() {
                if wire_frame_bits[wire] {
                    g3_received |= 1 << (g3_members.len() - 1 - offset);
                }
            }
            let (g3, _) = golay_decode(g3_received);
            println!("{label} frame={i} VOICE_ACTIVE={voice_active} g3={g3}");
        }
    }
}
