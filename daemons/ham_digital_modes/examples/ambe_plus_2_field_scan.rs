// SPDX-License-Identifier: LGPL-3.0-or-later
#![allow(dead_code)]
//! Field-by-field oracle scan of the chip's AMBE+2 half-rate decoder against this crate's decoder (the mbelib-derived model).
//! Starting from a base frame with every harmonic voiced, it sweeps ONE quantizer field at a time through all its
//! values (`b2` gain delta, `b3` PRBA24, `b4` PRBA58, `b5..b8` higher-order coefficients), sends each resulting frame
//! to the chip's decoder repeatedly (so the predictor settles), and measures the amplitude of every harmonic line in
//! the chip's steady-state output. The same is measured on this crate's own decoded PCM. A field whose table matches
//! the chip's gives a chip/ours harmonic-amplitude ratio that is flat across the field's values; a differing table
//! shows up as a ratio that varies with the value. Writes TSV `field value harmonic chip_db ours_db` to the path given.
//!
//! Usage: `cargo run --release --example dstar_field_scan -- <host:port> <out.tsv> [b0=44]`

use ham_digital_modes::ambe::float::ambe_plus_2::decode::RawParameters;
use ham_digital_modes::ambe::float::ambe_plus_2::encode::build_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::interleave::{frame_to_interleaved, interleaved_to_frame};
use ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder as DStarSynthesisDecoder;
use ham_digital_modes::ambe::float::ambe_plus_2::tables::{DG, HOC_B5, HOC_B6, HOC_B7, HOC_B8, L_TABLE, PRBA24, PRBA58, VUV, W0_TABLE};
use rustfft::{num_complex::Complex64, FftPlanner};
use std::net::UdpSocket;
use std::time::Duration;

fn frame_to_wire_bytes(frame: u128) -> [u8; 9] {
    let wire = frame_to_interleaved(frame);
    std::array::from_fn(|i| ((wire >> (8 * (8 - i))) & 0xFF) as u8)
}
#[allow(dead_code)]
fn wire_bytes_to_frame(b: &[u8; 9]) -> u128 {
    let mut wire: u128 = 0;
    for &x in b {
        wire = (wire << 8) | x as u128;
    }
    interleaved_to_frame(wire)
}
fn f0_from_b0(b0: u32) -> f64 {
    W0_TABLE[b0 as usize]
}
fn pack_raw_parameters(raw: &RawParameters) -> RawParameters {
    RawParameters { b0: raw.b0, b1: raw.b1, b2: raw.b2, b3: raw.b3, b4: raw.b4, b5: raw.b5, b6: raw.b6, b7: raw.b7, b8: raw.b8 }
}

const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const RATET_HALF_RATE_FEC: u8 = 33;
const FRAME_SAMPLES: usize = 160;
const REPS: usize = 12;
const FFT_LEN: usize = 4096;

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


fn line_amplitudes_db(pcm: &[f64], f0_hz: f64, harmonics: usize) -> Vec<f64> {
    // Hann-windowed 3 frames (480 samples), zero padded.
    let seg = &pcm[pcm.len() - 480..];
    let n = seg.len();
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(FFT_LEN);
    let mut buf: Vec<Complex64> = seg
        .iter()
        .enumerate()
        .map(|(i, &s)| Complex64::new(s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n as f64 - 1.0)).cos()), 0.0))
        .collect();
    buf.resize(FFT_LEN, Complex64::new(0.0, 0.0));
    fft.process(&mut buf);
    let mag: Vec<f64> = buf[..FFT_LEN / 2].iter().map(|c| c.norm()).collect();
    (1..=harmonics)
        .map(|k| {
            let centre = k as f64 * f0_hz * FFT_LEN as f64 / 8000.0;
            let (lo, hi) = ((centre - 0.25 * f0_hz * FFT_LEN as f64 / 8000.0).max(1.0) as usize, (centre + 0.25 * f0_hz * FFT_LEN as f64 / 8000.0) as usize);
            let peak = mag[lo..=hi.min(FFT_LEN / 2 - 1)].iter().cloned().fold(0.0, f64::max);
            20.0 * (peak + 1e-9).log10()
        })
        .collect()
}

fn chip_steady(sock: &UdpSocket, buf: &mut [u8; 1024], frame: u128) -> Vec<f64> {
    let mut pcm = Vec::new();
    for _ in 0..REPS {
        let mut payload = vec![0x01u8, 72];
        payload.extend_from_slice(&frame_to_wire_bytes(frame));
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

fn ours_steady(frame: u128) -> Vec<f64> {
    let mut dec = DStarSynthesisDecoder::new();
    let mut pcm = Vec::new();
    for _ in 0..REPS {
        pcm.extend(dec.decode_frame(frame).unwrap_or([0.0; 160]));
    }
    pcm
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let out_path = std::env::args().nth(2).unwrap_or_else(|| "/tmp/dstar_field_scan.tsv".to_string());
    let b0: u32 = std::env::args().nth(3).and_then(|s| s.parse().ok()).unwrap_or(44);
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

    let l = L_TABLE[b0 as usize] as usize;
    let f0_hz = f0_from_b0(b0) * 8000.0;
    let harmonics = l.min(((3800.0 / f0_hz) as usize).max(1));
    // Base frame: every harmonic voiced (b1 = 15), moderate gain, mid-range coefficients.
    fn min_norm<const N: usize>(t: &[[f64; N]], even: bool) -> u32 {
        (0..t.len()).filter(|i| !even || i % 2 == 0).min_by(|&a, &b| t[a].iter().map(|x| x * x).sum::<f64>().total_cmp(&t[b].iter().map(|x| x * x).sum::<f64>())).unwrap() as u32
    }
    let flat = (min_norm(&PRBA24, false), min_norm(&PRBA58, false), min_norm(&HOC_B5, false), min_norm(&HOC_B6, false), min_norm(&HOC_B7, false), min_norm(&HOC_B8, false));
    let b2_mid = (0..DG.len()).min_by(|&a, &b| (DG[a] - 0.0).abs().total_cmp(&(DG[b] - 0.0).abs())).unwrap() as u32;
    eprintln!("flat base: b2={b2_mid} b3..b8={flat:?}");
    let base = || RawParameters { b0, b1: VUV.iter().position(|r| r.iter().all(|&v| v)).unwrap() as u32, b2: b2_mid, b3: flat.0, b4: flat.1, b5: flat.2, b6: flat.3, b7: flat.4, b8: flat.5 };
    if std::env::args().nth(4).as_deref() == Some("f0scan") {
        // The chip's true fundamental for every b0: least-squares slope of the measured harmonic peak frequencies
        // against harmonic number, over a long steady all-voiced flat frame.
        for b0 in 10u32..118 {
            let mut raw = base();
            raw.b0 = b0;
            let frame = build_frame(&pack_raw_parameters(&raw));
            let mut pcm = Vec::new();
            for _ in 0..30 {
                pcm.extend(chip_steady(&sock, &mut buf, frame).into_iter().skip(160 * 8).take(160 * 4));
            }
            let seg = &pcm[..pcm.len().min(1280)];
            const NFFT: usize = 32768;
            let mut planner = FftPlanner::<f64>::new();
            let fft = planner.plan_fft_forward(NFFT);
            let n = seg.len();
            let mut b: Vec<Complex64> = seg.iter().enumerate().map(|(i, &s)| Complex64::new(s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (n as f64 - 1.0)).cos()), 0.0)).collect();
            b.resize(NFFT, Complex64::new(0.0, 0.0));
            fft.process(&mut b);
            let mag: Vec<f64> = b[..NFFT / 2].iter().map(|c| c.norm()).collect();
            let nominal = f0_from_b0(b0) * 8000.0;
            let bin = |hz: f64| hz * NFFT as f64 / 8000.0;
            let (mut sk, mut skf, mut skk) = (0.0, 0.0, 0.0);
            let kmax = ((3300.0 / nominal) as usize).max(3);
            let mut est = nominal;
            for _pass in 0..2 {
                let (mut sxy, mut sxx) = (0.0, 0.0);
                for k in 1..=kmax {
                    let c = bin(k as f64 * est);
                    let (lo, hi) = ((c - bin(est) * 0.35) as usize, (c + bin(est) * 0.35) as usize);
                    let pk = (lo..=hi).max_by(|&x, &y| mag[x].total_cmp(&mag[y])).unwrap();
                    let (a, m, cc) = (mag[pk - 1].ln(), mag[pk].ln(), mag[pk + 1].ln());
                    let off = 0.5 * (a - cc) / (a - 2.0 * m + cc);
                    let hz = (pk as f64 + off) * 8000.0 / NFFT as f64;
                    sxy += k as f64 * hz;
                    sxx += (k * k) as f64;
                    sk += 1.0;
                    skf += hz;
                    skk += k as f64;
                }
                est = sxy / sxx;
            }
            let _ = (sk, skf, skk);
            println!("b0={b0} nominal={nominal:.3} chip={est:.3} ratio={:.4} L_ours={}", est / nominal, L_TABLE[b0 as usize]);
        }
        return;
    }
    if std::env::args().nth(4).as_deref() == Some("uvspec") {
        // All-unvoiced flat frame: band energy (dB, 200 Hz bands) of chip versus ours, to see where the chip's noise ends.
        let mut raw = base();
        raw.b1 = 0;
        let frame = build_frame(&pack_raw_parameters(&raw));
        let (chip, ours) = (chip_steady(&sock, &mut buf, frame), ours_steady(frame));
        for (label, pcm) in [("chip", chip), ("ours", ours)] {
            let seg = &pcm[pcm.len() - 1600..];
            let mut planner = FftPlanner::<f64>::new();
            let fft = planner.plan_fft_forward(1600);
            let mut power = vec![0.0f64; 801];
            for w in seg.chunks(1600).filter(|c| c.len() == 1600) {
                let mut b: Vec<Complex64> = w.iter().map(|&s| Complex64::new(s, 0.0)).collect();
                fft.process(&mut b);
                for (p, c) in power.iter_mut().zip(&b) {
                    *p += c.norm_sqr();
                }
            }
            let bands: Vec<String> = (0..20).map(|k| format!("{:.0}", 10.0 * power[k * 40..(k + 1) * 40].iter().sum::<f64>().log10())).collect();
            println!("{label} unvoiced band dB (200 Hz steps from 0): {}", bands.join(" "));
        }
        return;
    }
    if std::env::args().nth(4).as_deref() == Some("repeatstate") {
        // Does a chip repeat (eps_c0 == 3 on an otherwise intact louder frame B) leave the predictor state untouched?
        // Sequence: base x10, corrupted B (forced repeat) x1..3, then base x4. If the state were updated from B, the
        // following base frames would show a loud excursion; ours (state untouched) will not.
        use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
        let clean = build_frame(&pack_raw_parameters(&base()));
        let mut loud = base();
        loud.b2 = (b2_mid + 14).min(31);
        let loud_frame = build_frame(&pack_raw_parameters(&loud));
        let one = |sock: &UdpSocket, buf: &mut [u8; 1024], frame: u128| -> Vec<f64> {
            let mut payload = vec![0x01u8, 72];
            payload.extend_from_slice(&frame_to_wire_bytes(frame));
            loop {
                let n = send_recv_retrying(sock, buf, &build_channel(&payload));
                if let Some((TYPE_SPEECH, p)) = parse_packet(&buf[..n]) {
                    return parse_speech_payload(p).iter().map(|&s| s as f64).collect();
                }
            }
        };
        let rms_db = |v: &[f64]| 10.0 * (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64 + 1e-9).log10();
        let clean_data = parse_frame(loud_frame).d;
        // Find three C0 error patterns that give eps_c0 == 3 with the data intact.
        let mut bad_frames = Vec::new();
        'outer: for a in 49..72u32 {
            for b in (a + 1)..72 {
                for c in (b + 1)..72 {
                    let f = loud_frame ^ (1u128 << a) ^ (1u128 << b) ^ (1u128 << c);
                    let p = parse_frame(f);
                    if p.epsilon_c0 == 3 && p.epsilon_c1 == 0 && p.d == clean_data {
                        bad_frames.push(f);
                        if bad_frames.len() == 3 {
                            break 'outer;
                        }
                    }
                }
            }
        }
        for repeats in 1..=3usize {
            let mut seq = vec![clean; 10];
            seq.extend(bad_frames[..repeats].iter().copied());
            seq.extend(vec![clean; 4]);
            let c: Vec<f64> = seq.iter().map(|&f| rms_db(&one(&sock, &mut buf, f))).collect();
            println!("{repeats} repeated frame(s): chip dB relative to settled: {}", c[10..].iter().map(|x| format!("{:+.1}", x - c[9])).collect::<Vec<_>>().join(" "));
        }
        let mut seq = vec![clean; 10];
        seq.extend(bad_frames.iter().copied());
        seq.extend(vec![loud_frame; 1]);
        seq.extend(vec![clean; 4]);
        let c: Vec<f64> = seq.iter().map(|&f| rms_db(&one(&sock, &mut buf, f))).collect();
        println!("3 repeats then clean B then base: {}", c[10..].iter().map(|x| format!("{:+.1}", x - c[9])).collect::<Vec<_>>().join(" "));
        // Reference: the same clean B frames sent as real frames (state definitely updated).
        let mut seq = vec![clean; 10];
        seq.extend(vec![loud_frame; 1]);
        seq.extend(vec![clean; 4]);
        let c: Vec<f64> = seq.iter().map(|&f| rms_db(&one(&sock, &mut buf, f))).collect();
        println!("reference: ONE clean B frame then base: {}", c[10..].iter().map(|x| format!("{:+.1}", x - c[9])).collect::<Vec<_>>().join(" "));
        return;
    }
    if std::env::args().nth(4).as_deref() == Some("errclass") {
        // How the chip decides to repeat: corrupt a distinctly LOUDER frame B (b2 raised) with k Golay-region bit errors after
        // settling on the base frame. Chip level at the corrupted frame: ~base => repeated the previous parameters; ~clean B =>
        // decoded B; anything else => decoded garbage. Tabulated against our own Golay error counts (eps_c0, eps_c1).
        use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
        let clean = build_frame(&pack_raw_parameters(&base()));
        let mut loud = base();
        loud.b2 = (b2_mid + 14).min(31);
        let loud_frame = build_frame(&pack_raw_parameters(&loud));
        let one = |sock: &UdpSocket, buf: &mut [u8; 1024], frame: u128| -> Vec<f64> {
            let mut payload = vec![0x01u8, 72];
            payload.extend_from_slice(&frame_to_wire_bytes(frame));
            loop {
                let n = send_recv_retrying(sock, buf, &build_channel(&payload));
                if let Some((TYPE_SPEECH, p)) = parse_packet(&buf[..n]) {
                    return parse_speech_payload(p).iter().map(|&s| s as f64).collect();
                }
            }
        };
        let rms_db = |v: &[f64]| 10.0 * (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64 + 1e-9).log10();
        // Reference levels: settled base, and the frame right after switching to clean B.
        let mut seq = vec![clean; 10];
        seq.push(loud_frame);
        let lv: Vec<f64> = seq.iter().map(|&f| rms_db(&one(&sock, &mut buf, f))).collect();
        let (base_db, loud_db) = (lv[9], lv[10]);
        println!("base {base_db:.1} dB, clean louder frame {loud_db:.1} dB");
        let mut seed = 0x0dd_ba11_5eedu64;
        let mut rnd = |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        let mut table: std::collections::BTreeMap<(u32, u32, bool, &'static str), usize> = Default::default();
        let clean_data = parse_frame(loud_frame).d;
        for k in 0..=9usize {
            for _ in 0..24 {
                let mut positions: Vec<u32> = Vec::new();
                while positions.len() < k {
                    let p = if rnd(2) == 0 { 49 + rnd(23) as u32 } else { 25 + rnd(23) as u32 };
                    if !positions.contains(&p) {
                        positions.push(p);
                    }
                }
                let bad = positions.iter().fold(loud_frame, |f, &p| f ^ (1u128 << p));
                let parsed = parse_frame(bad);
                let mut seq = vec![clean; 10];
                seq.push(bad);
                seq.extend(vec![clean; 2]);
                let c: Vec<f64> = seq.iter().map(|&f| rms_db(&one(&sock, &mut buf, f))).collect();
                let d = c[10] - base_db;
                let class = if d.abs() < 2.5 { "repeat" } else if (c[10] - loud_db).abs() < 2.5 { "decoded-B" } else { "garbage" };
                *table.entry((parsed.epsilon_c0, parsed.epsilon_c1, parsed.d == clean_data, class)).or_default() += 1;
            }
        }
        println!("eps_c0 eps_c1 data_intact class : count");
        for ((e0, e1, ok, class), n) in &table {
            println!("{e0} {e1} {ok} {class} : {n}");
        }
        return;
    }
    if std::env::args().nth(4).as_deref() == Some("errs") {
        // Corrupted-frame handling. Settle on the base frame, inject `k` bit errors into the Golay-protected part (C0 and C1)
        // of one frame, then send clean frames. Prints, per k and trial, the frame RMS (dB) of the corrupted frame and the
        // next two frames relative to the settled level, for the chip and for ours. Then a burst of 6 consecutive corrupted
        // frames (mbelib: repeat 3 times, then mute).
        let clean = build_frame(&pack_raw_parameters(&base()));
        let one = |sock: &UdpSocket, buf: &mut [u8; 1024], frame: u128| -> Vec<f64> {
            let mut payload = vec![0x01u8, 72];
            payload.extend_from_slice(&frame_to_wire_bytes(frame));
            loop {
                let n = send_recv_retrying(sock, buf, &build_channel(&payload));
                if let Some((TYPE_SPEECH, p)) = parse_packet(&buf[..n]) {
                    return parse_speech_payload(p).iter().map(|&s| s as f64).collect();
                }
            }
        };
        let rms_db = |v: &[f64]| 10.0 * (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64 + 1e-9).log10();
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut rnd = |n: u64| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        let corrupt = |frame: u128, k: usize, rnd: &mut dyn FnMut(u64) -> u64| -> u128 {
            let mut positions: Vec<u32> = Vec::new();
            while positions.len() < k {
                // Golay(23,12) codeword bits of C0 (frame bits 49..71) and C1 (bits 25..47).
                let p = if rnd(2) == 0 { 49 + rnd(23) as u32 } else { 25 + rnd(23) as u32 };
                if !positions.contains(&p) {
                    positions.push(p);
                }
            }
            positions.iter().fold(frame, |f, &p| f ^ (1u128 << p))
        };
        let run = |seq: &[u128], sock: &UdpSocket, buf: &mut [u8; 1024]| -> (Vec<f64>, Vec<f64>) {
            let chip: Vec<f64> = seq.iter().map(|&f| rms_db(&one(sock, buf, f))).collect();
            let mut dec = DStarSynthesisDecoder::new();
            let ours: Vec<f64> = seq.iter().map(|&f| rms_db(&dec.decode_frame(f).unwrap_or([0.0; 160]))).collect();
            (chip, ours)
        };
        println!("single corrupted frame: dB relative to the settled level (corrupted frame, next, next+1); chip | ours");
        for k in 0..=8usize {
            for trial in 0..3 {
                let bad = corrupt(clean, k, &mut rnd);
                let mut seq = vec![clean; 10];
                seq.push(bad);
                seq.extend(vec![clean; 2]);
                let (c, o) = run(&seq, &sock, &mut buf);
                let (cb, ob) = (c[9], o[9]);
                println!("k={k} trial {trial}: chip {:+.1} {:+.1} {:+.1} | ours {:+.1} {:+.1} {:+.1}", c[10] - cb, c[11] - cb, c[12] - cb, o[10] - ob, o[11] - ob, o[12] - ob);
            }
        }
        println!("burst of 6 corrupted frames with k=8 errors, then 3 clean: dB relative to settled");
        for trial in 0..3 {
            let mut seq = vec![clean; 10];
            for _ in 0..6 {
                seq.push(corrupt(clean, 8, &mut rnd));
            }
            seq.extend(vec![clean; 3]);
            let (c, o) = run(&seq, &sock, &mut buf);
            let (cb, ob) = (c[9], o[9]);
            println!("trial {trial}: chip {} | ours {}", c[10..].iter().map(|x| format!("{:+.0}", x - cb)).collect::<Vec<_>>().join(" "), o[10..].iter().map(|x| format!("{:+.0}", x - ob)).collect::<Vec<_>>().join(" "));
        }
        return;
    }
    if std::env::args().nth(4).as_deref() == Some("ljump") {
        // Pitch (harmonic count) jump: settle on the flat base, switch b0 to a very different value for 8 frames, then
        // back. Frame RMS (dB) of chip and ours, to find how the chip's predictor handles a change of L.
        let alt_b0: u32 = std::env::args().nth(5).and_then(|s| s.parse().ok()).unwrap_or(90);
        let mut alt = base();
        alt.b0 = alt_b0;
        if std::env::var("UNVOICED").is_ok() {
            alt.b1 = 0;
        }
        let (fb, fa) = (build_frame(&pack_raw_parameters(&base())), build_frame(&pack_raw_parameters(&alt)));
        let mut seq = vec![fb; 12];
        seq.extend(vec![fa; 8]);
        seq.extend(vec![fb; 8]);
        let one = |sock: &UdpSocket, buf: &mut [u8; 1024], frame: u128| -> Vec<f64> {
            let mut payload = vec![0x01u8, 72];
            payload.extend_from_slice(&frame_to_wire_bytes(frame));
            loop {
                let n = send_recv_retrying(sock, buf, &build_channel(&payload));
                if let Some((TYPE_SPEECH, p)) = parse_packet(&buf[..n]) {
                    return parse_speech_payload(p).iter().map(|&s| s as f64).collect();
                }
            }
        };
        let rms_db = |v: &[f64]| 10.0 * (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64 + 1e-9).log10();
        let chip: Vec<f64> = seq.iter().map(|&f| rms_db(&one(&sock, &mut buf, f))).collect();
        let mut dec = DStarSynthesisDecoder::new();
        let ours: Vec<f64> = seq.iter().map(|&f| rms_db(&dec.decode_frame(f).unwrap_or([0.0; 160]))).collect();
        println!("frame RMS dB, chip-minus-ours by frame (12 base, 8 alt b0={alt_b0}, 8 base):");
        println!("{}", chip.iter().zip(&ours).map(|(c, o)| format!("{:+.1}", c - o)).collect::<Vec<_>>().join(" "));
        println!("chip: {}", chip.iter().map(|c| format!("{c:.0}")).collect::<Vec<_>>().join(" "));
        println!("ours: {}", ours.iter().map(|c| format!("{c:.0}")).collect::<Vec<_>>().join(" "));
        return;
    }
    if std::env::args().nth(4).as_deref() == Some("rho") {
        // Step response: settle on the flat base, switch b5 to a strong row for 8 frames, then back for 6. The chip's and our
        // per-frame level (dB, relative to the settled base) at each harmonic give the predictor's coefficient.
        let row: u32 = std::env::args().nth(5).and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut alt = base();
        if std::env::var("FIELD").as_deref() == Ok("b2") {
            alt.b2 = row;
        } else {
            alt.b5 = row;
        }
        let (fb, fa) = (build_frame(&pack_raw_parameters(&base())), build_frame(&pack_raw_parameters(&alt)));
        let mut seq = vec![fb; 12];
        seq.extend(vec![fa; 8]);
        seq.extend(vec![fb; 6]);
        let one = |sock: &UdpSocket, buf: &mut [u8; 1024], frame: u128| -> Vec<f64> {
            let mut payload = vec![0x01u8, 72];
            payload.extend_from_slice(&frame_to_wire_bytes(frame));
            loop {
                let n = send_recv_retrying(sock, buf, &build_channel(&payload));
                if let Some((TYPE_SPEECH, p)) = parse_packet(&buf[..n]) {
                    return parse_speech_payload(p).iter().map(|&s| s as f64).collect();
                }
            }
        };
        let mut chip: Vec<Vec<f64>> = Vec::new();
        for &f in &seq {
            chip.push(one(&sock, &mut buf, f));
        }
        let mut dec = DStarSynthesisDecoder::new();
        let ours: Vec<Vec<f64>> = seq.iter().map(|&f| dec.decode_frame(f).unwrap_or([0.0; 160]).to_vec()).collect();
        let meas = |frames: &[Vec<f64>], i: usize, k: usize| -> f64 {
            // 3-frame window centred on frame i to give the line enough resolution.
            let mut w: Vec<f64> = Vec::new();
            for frame in &frames[i.saturating_sub(1)..=(i + 1).min(frames.len() - 1)] {
                w.extend(frame);
            }
            let a = line_amplitudes_db(&[vec![0.0; 480 - w.len().min(480)], w.clone()].concat(), f0_hz * 1.008, k);
            a[k - 1]
        };
        for k in [1usize, 2, 3] {
            print!("h{k} chip :");
            let base_c = meas(&chip, 11, k);
            for i in 12..seq.len() {
                print!(" {:+.1}", meas(&chip, i, k) - base_c);
            }
            println!();
            print!("h{k} ours :");
            let base_o = meas(&ours, 11, k);
            for i in 12..seq.len() {
                print!(" {:+.1}", meas(&ours, i, k) - base_o);
            }
            println!();
        }
        return;
    }
    if std::env::args().nth(4).as_deref() == Some("lscan") {
        // For every b0, find the chip's last harmonic that responds to b8 (its top block) and the chip's actual f0.
        for b0 in 20u32..118 {
            let l_ours = L_TABLE[b0 as usize] as usize;
            let f0 = f0_from_b0(b0) * 8000.0;
            let mut lo = base();
            lo.b0 = b0;
            let mut hi = base();
            hi.b0 = b0;
            hi.b8 = 1;
            lo.b8 = 6;
            let a = line_amplitudes_db(&chip_steady(&sock, &mut buf, build_frame(&pack_raw_parameters(&lo))), f0, (l_ours + 2).min((3950.0 / f0) as usize));
            let b = line_amplitudes_db(&chip_steady(&sock, &mut buf, build_frame(&pack_raw_parameters(&hi))), f0, (l_ours + 2).min((3950.0 / f0) as usize));
            let diffs: Vec<String> = a.iter().zip(&b).map(|(x, y)| format!("{:.0}", (x - y).abs())).collect();
            println!("b0={b0} L_ours={l_ours} f0={f0:.1} b8_delta_dB_by_harmonic: {}", diffs.join(" "));
        }
        return;
    }
    if std::env::args().nth(4).as_deref() == Some("peaks") {
        let frame = build_frame(&pack_raw_parameters(&base()));
        for (label, pcm) in [("chip", chip_steady(&sock, &mut buf, frame)), ("ours", ours_steady(frame))] {
            let seg = &pcm[pcm.len() - 480..];
            let mut planner = FftPlanner::<f64>::new();
            let fft = planner.plan_fft_forward(FFT_LEN);
            let mut b: Vec<Complex64> = seg.iter().enumerate().map(|(i, &s)| Complex64::new(s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / 479.0).cos()), 0.0)).collect();
            b.resize(FFT_LEN, Complex64::new(0.0, 0.0));
            fft.process(&mut b);
            let mag: Vec<f64> = b[..FFT_LEN / 2].iter().map(|c| c.norm()).collect();
            let mut peaks = Vec::new();
            for i in 2..FFT_LEN / 2 - 2 {
                if mag[i] > mag[i - 1] && mag[i] >= mag[i + 1] && mag[i] > 1e-3 * mag.iter().cloned().fold(0.0, f64::max) {
                    peaks.push(format!("{:.0}Hz:{:.0}dB", i as f64 * 8000.0 / FFT_LEN as f64, 20.0 * mag[i].log10()));
                }
            }
            println!("{label} f0={f0_hz:.1} L={l}: {}", peaks.join(" "));
        }
        return;
    }
    let fields: [(&str, usize); 7] = [("b2", 32), ("b3", 512), ("b4", 128), ("b5", 32), ("b6", 16), ("b7", 16), ("b8", 8)];
    let mut out = String::new();
    let only = std::env::args().nth(4);
    for (name, count) in fields {
        if only.as_deref().is_some_and(|o| !o.split(',').any(|x| x == name)) {
            continue;
        }
        for v in 0..count {
            
            let mut raw = base();
            match name {
                "b2" => raw.b2 = v as u32,
                "b3" => raw.b3 = v as u32,
                "b4" => raw.b4 = v as u32,
                "b5" => raw.b5 = v as u32,
                "b6" => raw.b6 = v as u32,
                "b7" => raw.b7 = v as u32,
                _ => raw.b8 = v as u32,
            }
            let frame = build_frame(&pack_raw_parameters(&raw));
            let chip = line_amplitudes_db(&chip_steady(&sock, &mut buf, frame), f0_hz, harmonics);
            let ours = line_amplitudes_db(&ours_steady(frame), f0_hz, harmonics);
            for k in 0..harmonics {
                out.push_str(&format!("{name}\t{v}\t{}\t{:.2}\t{:.2}\n", k + 1, chip[k], ours[k]));
            }
        }
        eprintln!("scanned {name}");
    }
    std::fs::write(out_path, out).unwrap();
}
