// SPDX-License-Identifier: LGPL-3.0-or-later
//! Chip-PCM-vs-float-synthesis comparison for D-STAR and AMBE+2 half-rate, the sibling of
//! `ambe_chip_pcm_vs_float_synthesis_ratet27.rs`. Captures real channel frames from the chip's
//! encoder, sends the same bits back to get the chip's own decoded PCM (two separate passes, so the
//! chip's decoder state is never reset by an interleaved encode), decodes them with this crate's
//! float `decode_frame`, and reports envelope correlation, best lag, and a per-band long-term
//! spectrum table (input / chip / float).
//!
//! **Results (live chip, 200 frames of OSR_us_000_0010_8k.wav)**: AMBE+2 half-rate envelope
//! correlation `0.9689`, long-term spectrum within ~2.5 dB of the chip in every band. D-STAR envelope
//! correlation `0.8355`, but float is 10-14 dB too quiet below 700 Hz (chip tracks the input within
//! ~3 dB) -- open. Run under `flock /tmp/dvsi_chip.lock` (the chip is shared between sessions).
//!
//! Set `AMBE_WAV_IN` to use a different speech file and `AMBE_WAV_OUT_DIR` for the output directory.
//!
//! Usage: `cargo run --release --example ambe_chip_pcm_vs_float_synthesis_half_rate -- <dstar|ambe_plus_2> [host:port]`
//! (`ambe_plus_2` needs `--features ambe_plus_2`).

use ham_digital_modes::ambe::float::dstar::interleave::wire_bytes_to_frame;
use ham_digital_modes::ambe::float::dstar::synthesis::DStarSynthesisDecoder;
use rustfft::{num_complex::Complex64, FftPlanner};
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const RATEP_DSTAR: [u16; 6] = [0x0130, 0x0763, 0x4000, 0x0000, 0x0000, 0x0048];
const RATET_HALF_RATE_FEC: u8 = 33;
const FRAME_SAMPLES: usize = 160;
const N_FRAMES: usize = 200;
const MAX_LAG_SAMPLES: i32 = 2400;

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
fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        cov += (x - ma) * (y - mb);
        va += (x - ma) * (x - ma);
        vb += (y - mb) * (y - mb);
    }
    if va <= 1e-9 || vb <= 1e-9 { 0.0 } else { cov / (va.sqrt() * vb.sqrt()) }
}
fn frame_rms(pcm: &[f64]) -> Vec<f64> {
    pcm.chunks_exact(FRAME_SAMPLES)
        .map(|c| (c.iter().map(|&s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt())
        .collect()
}
fn psd_add(psd: &mut [f64], frame: &[f64]) {
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(512);
    let n = frame.len() as f64;
    let mut buf: Vec<Complex64> = frame
        .iter()
        .enumerate()
        .map(|(k, &s)| Complex64::new(s * (0.5 - 0.5 * (2.0 * std::f64::consts::PI * k as f64 / (n - 1.0)).cos()), 0.0))
        .collect();
    buf.resize(512, Complex64::new(0.0, 0.0));
    fft.process(&mut buf);
    for (b, p) in psd.iter_mut().enumerate() {
        *p += buf[b].norm_sqr();
    }
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "dstar".to_string());
    let host = std::env::args().nth(2).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];

    let config = match mode.as_str() {
        "dstar" => {
            let mut body = Vec::new();
            for v in RATEP_DSTAR {
                body.extend_from_slice(&v.to_be_bytes());
            }
            control(FIELD_RATEP, &body)
        }
        "ambe_plus_2" => control(FIELD_RATET, &[RATET_HALF_RATE_FEC]),
        other => panic!("unknown mode {other}; use dstar or ambe_plus_2"),
    };
    sock.send(&config).expect("send rate config");
    let n = sock.recv(&mut buf).expect("rate config response");
    parse_packet(&buf[..n]).expect("valid packet");
    // Drain any stale replies left over from an earlier (killed or concurrent) session.
    sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    while sock.recv(&mut buf).is_ok() {}
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    let pcm = read_wav_mono_i16(&std::env::var("AMBE_WAV_IN").unwrap_or_else(|_| "tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav".to_string()));
    let n_frames = (pcm.len() / FRAME_SAMPLES).min(N_FRAMES);
    let mut payloads: Vec<Vec<u8>> = Vec::new();
    for i in 0..n_frames {
        let pkt = build_speech(&pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES]);
        let payload = loop {
            let n = send_recv_retrying(&sock, &mut buf, &pkt);
            match parse_packet(&buf[..n]) {
                Some((TYPE_CHANNEL, p)) => break p.to_vec(),
                other => eprintln!("discarding unexpected reply type {:?}", other.map(|o| o.0)),
            }
        };
        payloads.push(payload);
    }

    let mut dstar_dec = DStarSynthesisDecoder::new();
    let mut param_state = ham_digital_modes::ambe::float::dstar::decode::DStarDecoderState::initial();
    let param_range: Option<(usize, usize)> = std::env::var("PARAMS_RANGE").ok().and_then(|v| { let (a, b) = v.split_once(':')?; Some((a.parse().ok()?, b.parse().ok()?)) });
    #[cfg(feature = "ambe_plus_2")]
    let mut ap2_dec = ham_digital_modes::ambe::float::ambe_plus_2::synthesis::AmbePlus2SynthesisDecoder::new();
    let (mut chip_pcm, mut float_pcm) = (Vec::new(), Vec::new());
    let mut failures = 0usize;
    let (mut chip_psd, mut float_psd, mut in_psd) = (vec![0.0; 256], vec![0.0; 256], vec![0.0; 256]);
    for (i, payload) in payloads.iter().enumerate() {
        let pkt = build_channel(payload);
        let reply = loop {
            let n = send_recv_retrying(&sock, &mut buf, &pkt);
            match parse_packet(&buf[..n]) {
                Some((TYPE_SPEECH, p)) => break p.to_vec(),
                other => eprintln!("discarding unexpected reply type {:?}", other.map(|o| o.0)),
            }
        };
        let chip: Vec<f64> = parse_speech_payload(&reply).iter().map(|&s| s as f64).collect();

        let num_bits = payload[1] as usize;
        let frame_bytes = &payload[2..2 + 9];
        let float = match mode.as_str() {
            "dstar" if num_bits == 72 => {
                let mut wb = [0u8; 9];
                wb.copy_from_slice(frame_bytes);
                dstar_dec.decode_frame(wire_bytes_to_frame(&wb))
            }
            #[cfg(feature = "ambe_plus_2")]
            "ambe_plus_2" if num_bits == 72 => {
                let mut wire: u128 = 0;
                for &b in frame_bytes {
                    wire = (wire << 8) | b as u128;
                }
                ap2_dec.decode_frame(ham_digital_modes::ambe::float::ambe_plus_2::interleave::interleaved_to_frame(wire))
            }
            _ => None,
        };
        let float: Vec<f64> = match float {
            Some(f) => f.to_vec(),
            None => {
                failures += 1;
                vec![0.0; FRAME_SAMPLES]
            }
        };
        if let (Some((a, b)), "dstar") = (param_range, mode.as_str()) {
            let mut wb = [0u8; 9];
            wb.copy_from_slice(frame_bytes);
            let parsed = ham_digital_modes::ambe::float::dstar::decode::parse_frame(wire_bytes_to_frame(&wb));
            let raw = ham_digital_modes::ambe::float::dstar::decode::extract_raw_parameters(parsed.d);
            let rms = |v: &[f64]| (v.iter().map(|x| x * x).sum::<f64>() / v.len() as f64).sqrt();
            let dq = ham_digital_modes::ambe::float::dstar::decode::dequantize(parsed.d, &mut param_state);
            if i >= a && i < b {
                if let ham_digital_modes::ambe::float::dstar::decode::DequantizedFrame::Speech(p) = dq {
                    let voiced = p.voiced[1..].iter().filter(|&&v| v).count();
                    let l = p.ml.len() - 1;
                    let mean_db = 20.0 * (p.ml[1..].iter().map(|m| m * m).sum::<f64>() / l as f64).sqrt().log10();
                    println!("frame {i}: b0={} b1={} b2={} b3={} b4={} b5={} b6={} b7={} b8={} L={l} voiced={voiced}/{l} meanMl={mean_db:.1}dB chip_rms={:.0} ours_rms={:.0} err={}+{}", raw.b0, raw.b1, raw.b2, raw.b3, raw.b4, raw.b5, raw.b6, raw.b7, raw.b8, rms(&chip), rms(&float), parsed.epsilon_c0, parsed.epsilon_c1);
                }
            }
        }
        psd_add(&mut chip_psd, &chip);
        psd_add(&mut float_psd, &float);
        let input: Vec<f64> = pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES].iter().map(|&s| s as f64).collect();
        psd_add(&mut in_psd, &input);
        chip_pcm.extend(chip);
        float_pcm.extend(float);
    }
    println!("mode={mode}: {n_frames} frames, {failures} float decode failures");

    let search_len = chip_pcm.len().min(float_pcm.len()) - 2 * MAX_LAG_SAMPLES as usize;
    let (mut best_lag, mut best_corr) = (0i32, f64::NEG_INFINITY);
    for lag in -MAX_LAG_SAMPLES..=MAX_LAG_SAMPLES {
        let (a, b) = if lag >= 0 {
            (&chip_pcm[lag as usize..lag as usize + search_len], &float_pcm[..search_len])
        } else {
            (&chip_pcm[..search_len], &float_pcm[(-lag) as usize..(-lag) as usize + search_len])
        };
        let c = correlation(a, b);
        if c > best_corr {
            best_corr = c;
            best_lag = lag;
        }
    }
    println!("best sample alignment lag={best_lag} ({:.2} ms), correlation {best_corr:.4}", best_lag as f64 / 8.0);
    let (ce, fe) = (frame_rms(&chip_pcm), frame_rms(&float_pcm));
    let len = ce.len().min(fe.len());
    println!("frame RMS envelope correlation: {:.4}", correlation(&ce[..len], &fe[..len]));

    println!("{:>11} {:>8} {:>8} {:>8} {:>10} {:>10}", "band Hz", "input", "chip", "float", "chip/float", "chip/input");
    let edges = [0.0, 200.0, 400.0, 700.0, 1000.0, 1500.0, 2000.0, 3000.0, 4000.0];
    let db = |x: f64| 10.0 * x.max(1e-12).log10();
    for w in edges.windows(2) {
        let (a, b) = ((w[0] / 8000.0 * 512.0) as usize, (w[1] / 8000.0 * 512.0) as usize);
        let sum = |p: &[f64]| p[a..b.min(256)].iter().sum::<f64>();
        let (i, c, f) = (sum(&in_psd), sum(&chip_psd), sum(&float_psd));
        println!("{:>4.0}-{:<5.0} {:>8.1} {:>8.1} {:>8.1} {:>+10.1} {:>+10.1}", w[0], w[1], db(i), db(c), db(f), db(c) - db(f), db(c) - db(i));
    }
    let dir = std::env::var("AMBE_WAV_OUT_DIR").unwrap_or_else(|_| "/tmp".to_string());
    // One hex-encoded captured channel payload per line, so offline analysis can replay the exact
    // frames without touching the (shared) chip again.
    let hex: String = payloads.iter().map(|p| p.iter().map(|b| format!("{b:02x}")).collect::<String>() + "\n").collect();
    std::fs::write(format!("{dir}/{mode}_channel_payloads.hex"), hex).expect("write payloads");
    for (name, data) in [("chip", &chip_pcm), ("float", &float_pcm)] {
        let pcm16: Vec<i16> = data.iter().map(|&s| s.round().clamp(-32768.0, 32767.0) as i16).collect();
        let mut out = Vec::new();
        let dl = (pcm16.len() * 2) as u32;
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + dl).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&8000u32.to_le_bytes());
        out.extend_from_slice(&16000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&dl.to_le_bytes());
        for s in pcm16 {
            out.extend_from_slice(&s.to_le_bytes());
        }
        std::fs::write(format!("{dir}/{mode}_{name}_decoded.wav"), out).expect("write wav");
    }
}
