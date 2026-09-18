// SPDX-License-Identifier: LGPL-3.0-or-later
//! Falsification test for the Hamming-block hypothesis found by
//! `p25_ratet27_hamming_block_convention_search.rs`: under convention (dibit_swap=true,
//! reverse_bytes=true, lsb_first=false), wire bits {68, 92, 103, 127} all land in TIA_BLOCK 6 (a
//! 15-bit Hamming(15,11) block), but bit 8 lands in TIA_BLOCK 3 (a 23-bit Golay block) -- which
//! contradicts the confirmed {8, 92, 127} triple (Golay's minimum distance is 7, so a 2-bit error
//! is never "corrected" onto a third bit the way Hamming's distance-3 code allows).
//!
//! Per advisor: that triple was originally verified via sequential decodes within ONE connection,
//! which this investigation has since shown is NOT reliable (a single prior flip-decode measurably
//! changes the next result). This tool re-tests the three relevant pairs -- {92,127}, {8,92},
//! {8,127} -- each from its OWN fresh process (one flip-decode per process, matching the only
//! protocol shown reliable), and dumps the resulting PCM for exact byte-for-byte comparison across
//! runs. If {8,92} and {8,127} land back at (near-)baseline while {92,127} alone shows the large
//! deviation, bit 8's earlier "triple" membership was a same-connection artifact, and the Hamming-
//! block convention survives. If all three are identical (as originally reported), the triple is
//! real and this specific convention is falsified.
//!
//! Usage: `cargo run --release --example p25_ratet27_hamming_block_falsification_test -- <host:port> <pos1,pos2,...>`
//! Prints one line: `<positions>\t<distance_from_baseline_dB>\t<pcm_hex_sha_like_checksum>`
use rustfft::{num_complex::Complex64, FftPlanner};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const TEST_FREQ_HZ: f64 = 200.0;
const SETTLING_FRAMES: usize = 80;
const TOTAL_BITS: usize = 144;
const PRIME_REPEATS: usize = 4;
const SILENCE_CONDITION_REPEATS: usize = 20;

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
const BITS_OFFSET: usize = 6;
fn flip_bits_in_packet(raw_packet: &[u8], positions: &[usize]) -> Vec<u8> {
    let mut pkt = raw_packet.to_vec();
    for &k in positions {
        pkt[BITS_OFFSET + k / 8] ^= 1 << (7 - (k % 8));
    }
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
fn digital_silence() -> Vec<i16> {
    vec![0i16; FRAME_SAMPLES]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let positions: Vec<usize> = args
        .get(2)
        .expect("usage: <host:port> <pos1,pos2,...>")
        .split(',')
        .map(|s| s.parse().expect("integer bit position"))
        .collect();

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid DVSI packet");

    let samples = test_tone(TEST_FREQ_HZ);
    let mut r: Vec<u8> = Vec::new();
    for i in 0..SETTLING_FRAMES {
        sock.send(&build_speech(&samples)).expect("send speech");
        let n = sock.recv(&mut buf).expect("recv channel");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        assert_eq!(payload[1] as usize, TOTAL_BITS);
        if i == SETTLING_FRAMES - 1 {
            r = buf[..n].to_vec();
        }
    }

    let silence_samples = digital_silence();
    let mut silence_r: Vec<u8> = Vec::new();
    for i in 0..SETTLING_FRAMES {
        sock.send(&build_speech(&silence_samples)).expect("send speech");
        let n = sock.recv(&mut buf).expect("recv channel");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        assert_eq!(payload[1] as usize, TOTAL_BITS);
        if i == SETTLING_FRAMES - 1 {
            silence_r = buf[..n].to_vec();
        }
    }

    let send_channel_get_pcm = |sock: &UdpSocket, buf: &mut [u8; 512], raw_packet: &[u8]| -> Vec<i16> {
        sock.send(raw_packet).expect("send channel");
        let n = sock.recv(buf).expect("recv speech");
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decode) response");
        payload[2..].chunks_exact(2).map(|b| i16::from_be_bytes([b[0], b[1]])).collect()
    };
    let send_speech_get_channel = |sock: &UdpSocket, buf: &mut [u8; 512], samples: &[i16]| {
        sock.send(&build_speech(samples)).expect("send speech");
        let n = sock.recv(buf).expect("recv channel");
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL (encode) response");
    };

    let condition = |sock: &UdpSocket, buf: &mut [u8; 512]| {
        for _ in 0..SILENCE_CONDITION_REPEATS {
            send_speech_get_channel(sock, buf, &silence_samples);
        }
        for _ in 0..SILENCE_CONDITION_REPEATS {
            send_channel_get_pcm(sock, buf, &silence_r);
        }
        for _ in 0..PRIME_REPEATS {
            send_channel_get_pcm(sock, buf, &r);
        }
    };

    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(FRAME_SAMPLES);
    const DB_FLOOR: f64 = 1.0;
    let db_spectrum = |samples: &[i16]| -> Vec<f64> {
        let mut buf: Vec<Complex64> = samples.iter().map(|&s| Complex64::new(s as f64, 0.0)).collect();
        fft.process(&mut buf);
        buf[..FRAME_SAMPLES / 2 + 1].iter().map(|c| 20.0 * (c.norm().max(DB_FLOOR)).log10()).collect()
    };
    let spectral_distance = |a: &[f64], b: &[f64]| -> f64 {
        let sum_sq: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum();
        (sum_sq / a.len() as f64).sqrt()
    };

    // ONE flip-decode per fresh process -- the only protocol shown reliable this session.
    condition(&sock, &mut buf);
    let baseline = send_channel_get_pcm(&sock, &mut buf, &r);
    let baseline_spectrum = db_spectrum(&baseline);

    condition(&sock, &mut buf);
    let flipped = flip_bits_in_packet(&r, &positions);
    let pcm = send_channel_get_pcm(&sock, &mut buf, &flipped);
    let distance = spectral_distance(&baseline_spectrum, &db_spectrum(&pcm));

    // Simple order-sensitive checksum of the full PCM frame for cross-process exact comparison.
    let mut checksum: u64 = 0xcbf29ce484222325;
    for &s in &pcm {
        checksum ^= s as u16 as u64;
        checksum = checksum.wrapping_mul(0x100000001b3);
    }

    let pos_str = positions.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    println!("{pos_str}\t{distance:.2}\t{checksum:016x}\t{pcm:?}");
}
