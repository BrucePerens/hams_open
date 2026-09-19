// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Tone-level calibration against the real chip's decoder for D-STAR (and, with `--features ambe_plus_2`, AMBE+2
//! half-rate): builds tone frames with this crate's tone encoders across the level field's range, sends each to the
//! chip repeatedly, and reports the chip's steady-state output peak and RMS next to what this crate's own tone
//! synthesis produces for the same frame, for a single tone (1 kHz) and a DTMF digit ('5').
//!
//! **Results (live chip)**: D-STAR tone level is exponential in the 8-bit volume field (per-tone amplitude
//! 3268 at volume 180, x1.8435 per 15 steps, saturating near 238), the same per tone for single tones and each
//! tone of a DTMF pair. AMBE+2 half-rate only synthesizes tone frames when `DCMODE_IN` bit 14 (`TS_ENABLE`,
//! `DCMODE=0x4000`, PKT_DCMODE field 0x06) is set, and then always at ~24000 rms in total regardless of the frame's
//! level field (a real chip-detected tone frame decodes at the same level whatever its input amplitude was).
//! `CAPTURE=1` records those detected frames.
//!
//! Usage: `cargo run --release --example ambe_tone_level_probe -- <dstar|ambe_plus_2> [host:port]`

use ham_digital_modes::ambe::float::dstar::encode::build_tone_frame as dstar_tone_frame;
use ham_digital_modes::ambe::float::dstar::interleave::frame_to_wire_bytes as dstar_wire;
use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_RATET: u8 = 0x09;
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


fn chip_frame_stats(sock: &UdpSocket, buf: &mut [u8; 1024], wire: [u8; 9]) -> (f64, f64) {
    let mut last = Vec::new();
    for r in 0..6 {
        let mut payload = vec![0x01u8, 72];
        payload.extend_from_slice(&wire);
        let reply = loop {
            let n = send_recv_retrying(sock, buf, &build_channel(&payload));
            match parse_packet(&buf[..n]) {
                Some((TYPE_SPEECH, p)) => break p.to_vec(),
                other => eprintln!("discarding unexpected reply type {:?}", other.map(|o| o.0)),
            }
        };
        if r == 5 {
            last = parse_speech_payload(&reply).iter().map(|&s| s as f64).collect();
        }
    }
    let peak = last.iter().fold(0.0f64, |m, &s| m.max(s.abs()));
    let rms = (last.iter().map(|s| s * s).sum::<f64>() / last.len().max(1) as f64).sqrt();
    (peak, rms)
}

fn ours_stats(frames: &[u128], mut decode: impl FnMut(u128) -> Option<[f64; 160]>) -> (f64, f64) {
    let mut last = [0.0; 160];
    for &f in frames.iter().cycle().take(6) {
        last = decode(f).unwrap_or([0.0; 160]);
    }
    let peak = last.iter().fold(0.0f64, |m, &s| m.max(s.abs()));
    let rms = (last.iter().map(|s| s * s).sum::<f64>() / 160.0).sqrt();
    (peak, rms)
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "dstar".to_string());
    let host = std::env::args().nth(2).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    let config = if mode == "dstar" {
        let mut body = Vec::new();
        for v in RATEP_DSTAR {
            body.extend_from_slice(&v.to_be_bytes());
        }
        control(FIELD_RATEP, &body)
    } else {
        control(FIELD_RATET, &[33])
    };
    sock.send(&config).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    while sock.recv(&mut buf).is_ok() {}
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    // Optional PKT_DCMODE (field 0x06) word, e.g. DCMODE=0x4000 to set DCMODE_IN's TS_ENABLE (bit 14, force tone synthesis).
    if let Ok(v) = std::env::var("DCMODE") {
        let word = u16::from_str_radix(v.trim_start_matches("0x"), 16).unwrap();
        sock.send(&control(0x06, &word.to_be_bytes())).unwrap();
        let n = sock.recv(&mut buf).unwrap();
        println!("DCMODE reply: {:02x?}", &buf[..n]);
    }
    #[cfg(feature = "ambe_plus_2")]
    if std::env::var("CAPTURE").is_ok() && mode == "ambe_plus_2" {
        use ham_digital_modes::ambe::float::ambe_plus_2::interleave::interleaved_to_frame;
        use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
        for amp in [250.0f64, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0] {
            let mut last_payload = Vec::new();
            for f in 0..8usize {
                let frame: Vec<i16> = (0..160).map(|i| (amp * (2.0 * std::f64::consts::PI * 1000.0 * (f * 160 + i) as f64 / 8000.0).sin()) as i16).collect();
                let n = send_recv_retrying(&sock, &mut buf, &build_speech(&frame));
                let (_, p) = parse_packet(&buf[..n]).unwrap();
                last_payload = p.to_vec();
            }
            let mut wire: u128 = 0;
            for &b in &last_payload[2..11] {
                wire = (wire << 8) | b as u128;
            }
            let d = parse_frame(interleaved_to_frame(wire)).d;
            let (cp, cr) = chip_frame_stats(&sock, &mut buf, last_payload[2..11].try_into().unwrap());
            println!("input sine amp {amp:6.0}: d[4..16)={:#05x} tone_idx-ish d[16..20)={:#x}; chip decode peak/rms {cp:.0}/{cr:.0}", (d >> 33) & 0xFFF, (d >> 29) & 0xF);
        }
    }
    println!("tone | level field | chip peak / rms | ours peak / rms");
    if mode == "dstar" {
        for (name, index) in [("1kHz", 32u32), ("DTMF 5", 128 + 1 + 4)] {
            for volume in (0u32..=255).step_by(15).chain([255]) {
                let frame = dstar_tone_frame(index, volume);
                let (cp, cr) = chip_frame_stats(&sock, &mut buf, dstar_wire(frame));
                let mut dec = DStarSynthesisDecoder::new();
                let (op, or) = ours_stats(&[frame], |f| dec.decode_frame(f));
                println!("{name:7} | {volume:3} | {cp:8.0} / {cr:7.0} | {op:8.0} / {or:7.0}");
            }
        }
    }
    #[cfg(feature = "ambe_plus_2")]
    if mode == "ambe_plus_2" {
        use ham_digital_modes::ambe::float::ambe_plus_2::encode::build_tone_frame as ap2_tone_frame;
        use ham_digital_modes::ambe::float::ambe_plus_2::interleave::frame_to_interleaved;
        use ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder;
        let wire_of = |f: u128| -> [u8; 9] {
            let w = frame_to_interleaved(f);
            std::array::from_fn(|i| ((w >> (8 * (8 - i))) & 0xFF) as u8)
        };
        for (name, idx, cp) in [("1kHz", 32u8, false), ("DTMF 5", 0x85, false), ("dial", 0xA0, true)] {
            for amp in (0u16..=4095).step_by(256).chain([4095]) {
                let frame = ap2_tone_frame(idx, cp, amp);
                let (cpk, cr) = chip_frame_stats(&sock, &mut buf, wire_of(frame));
                let mut dec = AmbePlus2SynthesisDecoder::new();
                let (op, or) = ours_stats(&[frame], |f| dec.decode_frame(f));
                println!("{name:7} | {amp:4} | {cpk:8.0} / {cr:7.0} | {op:8.0} / {or:7.0}");
            }
        }
    }
}
