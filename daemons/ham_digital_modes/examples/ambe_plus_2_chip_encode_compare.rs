// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Encode-direction comparison of this crate's AMBE+2 half-rate [`Encoder`] against the real chip's encoder on real speech,
//! the sibling of `ratet27_chip_encode_compare`. For several analysis-centre offsets: (1) encode the input with our
//! encoder, (2) compare our frames' decoded parameters (pitch index `b0`, harmonic count, V/UV pattern index `b1`,
//! gain index `b2`) with the chip encoder's frames for the same input, (3) send our frames to the chip's decoder and
//! report frame-RMS envelope correlation against the input for chip encode->chip decode, our encode->chip decode
//! and our encode->our decode.
//!
//! **Result (live chip, 150 frames of OSR_us_000_0010_8k.wav)**: chip encode->chip decode reaches envelope
//! correlation 0.762 against the input; our encode->chip decode reaches 0.973 at analysis offset +160 (rising with
//! offset up to one frame, the decoder's latency), and our encode->our decode 0.981. Pitch index agreement with
//! the chip encoder's own frames is ~30-48% within 2 steps.
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example ambe_plus_2_chip_encode_compare -- [host:port] [wav] [n_frames]`

use ham_digital_modes::ambe::float::ambe_plus_2::decode::{dequantize, extract_raw_parameters, DecoderState, DequantizedFrame};
use ham_digital_modes::ambe::float::ambe_plus_2::encoder::Encoder;
use ham_digital_modes::ambe::float::ambe_plus_2::interleave::{frame_to_interleaved, interleaved_to_frame};
use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder;
use std::net::UdpSocket;
use std::time::Duration;

const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const FIELD_RATET: u8 = 0x09;
const RATET_HALF_RATE_FEC: u8 = 33;

fn frame_to_wire_bytes(frame: u128) -> [u8; 9] {
    let wire = frame_to_interleaved(frame);
    std::array::from_fn(|i| ((wire >> (8 * (8 - i))) & 0xFF) as u8)
}
fn wire_bytes_to_frame(b: &[u8; 9]) -> u128 {
    let mut wire: u128 = 0;
    for &x in b {
        wire = (wire << 8) | x as u128;
    }
    interleaved_to_frame(wire)
}
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
/// Best envelope correlation over lags of up to +-4 frames (hop 20 samples), so alignment choices are judged by quality
/// rather than by how much decoder delay happens to line up with the input.
fn best_lag_corr(input: &[f64], decoded: &[f64]) -> (f64, i32) {
    let e = |x: &[f64]| -> Vec<f64> { x.windows(FRAME_SAMPLES).step_by(20).map(|w| (w.iter().map(|s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt()).collect() };
    let (a, b) = (e(input), e(decoded));
    let mut best = (f64::NEG_INFINITY, 0);
    for lag in -32i32..=32 {
        let (x, y): (Vec<f64>, Vec<f64>) = a.iter().enumerate().filter_map(|(i, &v)| { let j = i as i32 + lag; (j >= 0 && (j as usize) < b.len()).then(|| (v, b[j as usize])) }).unzip();
        if x.len() > 50 { let c = corr(&x, &y); if c > best.0 { best = (c, lag * 20); } }
    }
    best
}

fn env(x: &[f64]) -> Vec<f64> {
    x.chunks_exact(FRAME_SAMPLES).map(|c| (c.iter().map(|s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt()).collect()
}

struct P {
    b0: u32,
    b1: u32,
    b2: u32,
}
fn params(frames: &[u128]) -> Vec<Option<P>> {
    let mut st = DecoderState::initial();
    frames
        .iter()
        .map(|&f| {
            let d = parse_frame(f).d;
            let raw = extract_raw_parameters(d);
            match dequantize(&raw, &mut st) {
                DequantizedFrame::Speech(_) => Some(P { b0: raw.b0, b1: raw.b1, b2: raw.b2 }),
                _ => None,
            }
        })
        .collect()
}

fn chip_decode(sock: &UdpSocket, buf: &mut [u8; 1024], frames: &[u128]) -> Vec<f64> {
    let mut pcm = Vec::new();
    for &f in frames {
        let mut payload = vec![0x01u8, 72];
        payload.extend_from_slice(&frame_to_wire_bytes(f));
        let reply = loop {
            let n = send_recv_retrying(sock, buf, &build_channel(&payload));
            match parse_packet(&buf[..n]) {
                Some((TYPE_SPEECH, p)) => break p.to_vec(),
                other => eprintln!("discarding unexpected reply type {:?}", other.map(|o| o.0)),
            }
        };
        pcm.extend(parse_speech_payload(&reply).iter().map(|&s| s as f64));
    }
    pcm
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let wav = std::env::args().nth(2).unwrap_or_else(|| "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav".to_string());
    let n_frames: usize = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(150);
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];
    sock.send(&control(FIELD_RATET, &[RATET_HALF_RATE_FEC])).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    while sock.recv(&mut buf).is_ok() {}
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    let pcm = read_wav_mono_i16(&wav);
    let n_frames = n_frames.min(pcm.len() / FRAME_SAMPLES);
    let input: Vec<f64> = pcm[..n_frames * FRAME_SAMPLES].iter().map(|&s| s as f64).collect();

    let mut chip_frames: Vec<u128> = Vec::new();
    for i in 0..n_frames {
        let pkt = build_speech(&pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES]);
        let payload = loop {
            let n = send_recv_retrying(&sock, &mut buf, &pkt);
            match parse_packet(&buf[..n]) {
                Some((TYPE_CHANNEL, p)) => break p.to_vec(),
                other => eprintln!("discarding unexpected reply type {:?}", other.map(|o| o.0)),
            }
        };
        let mut wb = [0u8; 9];
        wb.copy_from_slice(&payload[2..11]);
        chip_frames.push(wire_bytes_to_frame(&wb));
    }
    let chip_chip = chip_decode(&sock, &mut buf, &chip_frames);
    let chip_params = params(&chip_frames);
    let env_in = env(&input);
    println!("chip encode -> chip decode: envelope corr vs input {:.4}", corr(&env_in, &env(&chip_chip)));

    for offset in [-160i32, -120, -80, -40, 0, 40, 80, 120, 160] {
        let mut enc = Encoder::new();
        enc.set_center_offset(offset);
        enc.push_samples(&input);
        let mut ours: Vec<u128> = Vec::new();
        while let Some(f) = enc.next_frame() {
            ours.push(f);
        }
        ours.extend(enc.finish());
        ours.truncate(n_frames);
        let our_params = params(&ours);
        if let Ok(path) = std::env::var("DUMP_B0") {
            let lines: Vec<String> = our_params.iter().zip(chip_params.iter()).map(|(a, b)| format!("{} {}", a.as_ref().map_or(-1, |x| x.b0 as i32), b.as_ref().map_or(-1, |x| x.b0 as i32))).collect();
            std::fs::write(format!("{path}.{offset}"), lines.join("\n")).unwrap();
        }
        let (mut both, mut b0_close, mut b1_eq, mut b2_close) = (0usize, 0usize, 0usize, 0usize);
        for (a, b) in our_params.iter().zip(chip_params.iter()) {
            if let (Some(a), Some(b)) = (a, b) {
                both += 1;
                b0_close += ((a.b0 as i32 - b.b0 as i32).abs() <= 2) as usize;
                b1_eq += (a.b1 == b.b1) as usize;
                b2_close += ((a.b2 as i32 - b.b2 as i32).abs() <= 2) as usize;
            }
        }
        let ours_chip = chip_decode(&sock, &mut buf, &ours);
        let mut d = AmbePlus2SynthesisDecoder::new();
        if std::env::var("DEBUG_NONE").is_ok() {
            let mut dd = AmbePlus2SynthesisDecoder::new();
            let nones = ours.iter().filter(|&&f| dd.decode_frame(f).is_none()).count();
            let mut dd = AmbePlus2SynthesisDecoder::new();
            let rms: f64 = (ours.iter().flat_map(|&f| dd.decode_frame(f).unwrap_or([0.0; 160])).map(|x| x * x).sum::<f64>() / (ours.len() * 160) as f64).sqrt();
            let tone_frames = ours.iter().filter(|&&f| matches!(dequantize(&extract_raw_parameters(parse_frame(f).d), &mut DecoderState::initial()), DequantizedFrame::Tone { .. })).count();
            eprintln!("offset {offset}: tone frames {tone_frames}; our decoder returned None for {nones} of {} frames, rms {rms:.0}", ours.len());
        }
        let ours_ours: Vec<f64> = ours.iter().flat_map(|&f| d.decode_frame(f).unwrap_or([0.0; 160])).collect();
        if std::env::var("DEBUG_NONE").is_ok() {
            let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
            eprintln!("offset {offset}: rms chip-dec {:.0} our-dec {:.0}; env corr chip-dec vs our-dec {:.4}", rms(&ours_chip), rms(&ours_ours), corr(&env(&ours_chip), &env(&ours_ours)));
        }
        println!(
            "offset {offset:5}: lag-searched corr our-enc->chip-dec {:.4} (lag {} samples), our-enc->our-dec {:.4} (lag {}); frames {} both-speech {both}; b0 within 2: {:.2}, b1 equal: {:.2}, b2 within 2: {:.2}; envelope corr vs input: our-enc->chip-dec {:.4}, our-enc->our-dec {:.4}",
            best_lag_corr(&input, &ours_chip).0,
            best_lag_corr(&input, &ours_chip).1,
            best_lag_corr(&input, &ours_ours).0,
            best_lag_corr(&input, &ours_ours).1,
            ours.len(),
            b0_close as f64 / both.max(1) as f64,
            b1_eq as f64 / both.max(1) as f64,
            b2_close as f64 / both.max(1) as f64,
            corr(&env_in, &env(&ours_chip)),
            corr(&env_in, &env(&ours_ours)),
        );
    }
}
