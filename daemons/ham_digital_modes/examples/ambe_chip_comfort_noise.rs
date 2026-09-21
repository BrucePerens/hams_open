// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Raw comfort-noise capture. (Structure of the chip's (deterministic after PKT_INIT) unvoiced noise: is the excitation independent of gain, pitch and
//! spectral shape? Captures the first frames after a decoder INIT for several all-unvoiced frames and prints their
//! pairwise sample-level correlations. (Copy of the determinism example's plumbing.)
//! Sends the same all-unvoiced frames twice with a full re-configuration in between and compares the PCM sample by
//! sample; then correlates the chip's first frames with this crate's decoder (fresh state, TIA-102.BABA's own
//! `u(-105) = 3147` noise sequence) at sample level over a lag search. A high correlation would show the noise
//! generator and synthesis window match the chip's.
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example ambe_chip_noise_determinism -- <host:port>`

use ham_digital_modes::ambe::float::ambe_plus_2::decode::RawParameters as A2Raw;
use ham_digital_modes::ambe::float::ambe_plus_2::encode::build_frame as a2_build;
use ham_digital_modes::ambe::float::ambe_plus_2::interleave::frame_to_interleaved;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
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




fn frame_to_wire_bytes(frame: u128) -> [u8; 9] {
    let wire = frame_to_interleaved(frame);
    std::array::from_fn(|i| ((wire >> (8 * (8 - i))) & 0xFF) as u8)
}

fn configure(sock: &UdpSocket, buf: &mut [u8; 1024]) {
    sock.send(&control(FIELD_RATET, &[RATET_HALF_RATE_FEC])).unwrap();
    let n = sock.recv(buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    if let Ok(v) = std::env::var("INIT") {
        // PKT_INIT (0x0B): 1 = encoder, 2 = decoder, 3 = both, 7 = both plus echo canceller.
        let flags: u8 = v.parse().unwrap_or(3);
        sock.send(&control(0x0B, &[flags])).unwrap();
        let n = sock.recv(buf).unwrap();
        println!("PKT_INIT {flags:#x} response: {:02x?}", &buf[..n.min(12)]);
    }
    sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    while sock.recv(buf).is_ok() {}
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
}

fn decode(sock: &UdpSocket, buf: &mut [u8; 1024], frames: &[u128]) -> Vec<Vec<f64>> {
    frames
        .iter()
        .map(|&f| {
            let mut payload = vec![0x01u8, 72];
            payload.extend_from_slice(&frame_to_wire_bytes(f));
            loop {
                let n = send_recv_retrying(sock, buf, &build_channel(&payload));
                if let Some((TYPE_SPEECH, p)) = parse_packet(&buf[..n]) {
                    return parse_speech_payload(p).iter().map(|&s| s as f64).collect();
                }
            }
        })
        .collect()
}

fn corr_at_best_lag(a: &[f64], b: &[f64], max_lag: i32) -> (f64, i32) {
    let mut best = (-1.0, 0);
    for lag in -max_lag..=max_lag {
        let (mut sab, mut saa, mut sbb) = (0.0, 0.0, 0.0);
        for i in 0..a.len() as i32 {
            let j = i + lag;
            if j < 0 || j >= b.len() as i32 {
                continue;
            }
            let (x, y) = (a[i as usize], b[j as usize]);
            sab += x * y;
            saa += x * x;
            sbb += y * y;
        }
        let c = sab / (saa * sbb).sqrt().max(1e-12);
        if c > best.0 {
            best = (c, lag);
        }
    }
    best
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    std::env::set_var("INIT", "2");
    let b0: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(124);
    let frames: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(40);
    let f = a2_build(&A2Raw { b0, b1: 16, b2: 12, b3: 200, b4: 60, b5: 6, b6: 6, b7: 6, b8: 3 });
    let mut runs = Vec::new();
    for _ in 0..2 {
        configure(&sock, &mut buf);
        let pcm: Vec<i32> = decode(&sock, &mut buf, &vec![f; frames]).into_iter().flatten().map(|x| x as i32).collect();
        runs.push(pcm);
    }
    println!("identical across two INIT runs: {}", runs[0] == runs[1]);
    let pcm = &runs[0];
    let mut hist = std::collections::BTreeMap::new();
    for &x in pcm { *hist.entry(x).or_insert(0usize) += 1; }
    println!("value histogram: {:?}", hist);
    for (i, fr) in pcm.chunks(160).enumerate().take(frames) {
        println!("frame {i}: {}", fr.iter().take(40).map(|x| x.to_string()).collect::<Vec<_>>().join(" "));
    }
    std::fs::write(std::env::temp_dir().join("comfort_noise.txt"), pcm.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
}
