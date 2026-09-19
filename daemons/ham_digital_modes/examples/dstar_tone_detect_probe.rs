// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Chip tone-detection calibration for D-STAR (encode direction): feeds synthetic DTMF digits and single tones at
//! several levels to the chip's encoder (whose `TD_ENABLE` tone detector is on by default) and prints, per stimulus,
//! how many of the emitted frames are tone frames (`b0` 126/127), the tone `index` and `volume` fields, so this
//! crate's tone detector and its level-to-`volume` mapping can be matched to the chip.
//!
//! Usage: `cargo run --release --example dstar_tone_detect_probe -- [host:port]`

use ham_digital_modes::ambe::float::dstar::decode::{classify_b0, decode_tone, extract_raw_parameters, parse_frame, FrameKind};
use ham_digital_modes::ambe::float::dstar::interleave::wire_bytes_to_frame;
use ham_digital_modes::ambe::float::tone_synthesis::{DTMF_COL_HZ, DTMF_ROW_HZ};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const RATEP_DSTAR: [u16; 6] = [0x0130, 0x0763, 0x4000, 0x0000, 0x0000, 0x0048];
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


fn run(sock: &UdpSocket, buf: &mut [u8; 1024], name: &str, freqs: &[f64], amp: f64) {
    let (mut tone_frames, mut total) = (0usize, 0usize);
    let mut seen = Vec::new();
    for f in 0..30usize {
        let frame: Vec<i16> = (0..FRAME_SAMPLES)
            .map(|i| {
                let t = (f * FRAME_SAMPLES + i) as f64 / 8000.0;
                freqs.iter().map(|&hz| amp * (2.0 * std::f64::consts::PI * hz * t).sin()).sum::<f64>() as i16
            })
            .collect();
        let payload = loop {
            let n = send_recv_retrying(sock, buf, &build_speech(&frame));
            match parse_packet(&buf[..n]) {
                Some((TYPE_CHANNEL, p)) => break p.to_vec(),
                other => eprintln!("discarding unexpected reply type {:?}", other.map(|o| o.0)),
            }
        };
        let mut wb = [0u8; 9];
        wb.copy_from_slice(&payload[2..11]);
        let d = parse_frame(wire_bytes_to_frame(&wb)).d;
        total += 1;
        if classify_b0(extract_raw_parameters(d).b0) == FrameKind::Tone {
            tone_frames += 1;
            let t = decode_tone(d);
            seen.push((f, t.index, t.volume));
        }
    }
    let first = seen.first().map(|x| x.0);
    let last = seen.last().map(|x| (x.1, x.2));
    println!("{name:14} amp {amp:6.0}: tone frames {tone_frames}/{total}, first at frame {first:?}, last (index, volume) {last:?}");
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    let mut body = Vec::new();
    for v in RATEP_DSTAR {
        body.extend_from_slice(&v.to_be_bytes());
    }
    sock.send(&control(FIELD_RATEP, &body)).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    while sock.recv(&mut buf).is_ok() {}
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    if std::env::var("SCAN").is_ok() {
        let mut hz = 100.0;
        while hz <= 3900.0 {
            run(&sock, &mut buf, &format!("scan {hz:.0} Hz"), &[hz], 4000.0);
            hz += 100.0;
        }
        return;
    }
    for amp in [500.0, 1000.0, 2000.0, 4000.0, 8000.0, 12000.0] {
        run(&sock, &mut buf, "DTMF 5", &[DTMF_ROW_HZ[1], DTMF_COL_HZ[1]], amp);
    }
    for (r, c, name) in [(0, 0, "DTMF 1"), (3, 1, "DTMF 0"), (3, 3, "DTMF D"), (2, 2, "DTMF 9")] {
        run(&sock, &mut buf, name, &[DTMF_ROW_HZ[r], DTMF_COL_HZ[c]], 4000.0);
    }
    for hz in [300.0, 500.0, 1000.0, 1500.0, 2000.0, 3000.0] {
        for amp in [1000.0, 4000.0, 12000.0] {
            run(&sock, &mut buf, &format!("tone {hz:.0} Hz"), &[hz], amp);
        }
    }
}
