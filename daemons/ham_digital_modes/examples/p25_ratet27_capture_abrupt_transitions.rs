// SPDX-License-Identifier: LGPL-3.0-or-later
//! Fourth follow-up targeting `g3`'s stubborn GF(2) rank-8 plateau (see
//! `p25_ratet27_capture_frames.rs`'s module doc and `AMBE_CHIP_VALIDATION_FINDINGS.md` section 23).
//! Reasoning: this crate's own encode pipeline includes a real frame-to-frame *prediction residual*
//! stage (`src/ambe/prediction.rs`, differential encoding of spectral amplitudes against the
//! previous frame's own reconstructed history) -- if the real chip does something similar and `g3`
//! carries part of that residual, every stimulus tried so far (steady tones, per-frame-independent
//! noise, and *smoothly*-varying real speech) may simply never produce a large enough frame-to-frame
//! jump to exercise it. This tool instead alternates abruptly between very different signal states
//! every single frame -- full-scale tone <-> silence, high-frequency <-> low-frequency, loud <->
//! quiet -- to maximize whatever a differential/predictive encoder would see as "residual".
//!
//! Usage: `cargo run --release --example p25_ratet27_capture_abrupt_transitions -- <host:port>`
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;

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
fn parse_packet(data: &[u8]) -> Option<(u8, &[u8])> {
    if data.len() < 4 || data[0] != 0x61 {
        return None;
    }
    let length = u16::from_be_bytes([data[1], data[2]]) as usize;
    let ptype = data[3];
    data.get(4..4 + length).map(|payload| (ptype, payload))
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
fn silence() -> Vec<i16> {
    vec![0i16; FRAME_SAMPLES]
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];
    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid DVSI packet");

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

    // Priming: get past initial settling with a steady tone first.
    let steady = sawtooth(200.0, 6000.0);
    for _ in 0..30 {
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(&steady));
        parse_packet(&buf[..n]).expect("valid packet");
    }

    // Build a long sequence of ABRUPTLY alternating frames -- every single frame is a maximally
    // different stimulus from the previous one, to stress any frame-to-frame differential/
    // predictive encoding path as hard as possible.
    let loud_high = sawtooth(400.0, 16000.0);
    let loud_low = sawtooth(70.0, 16000.0);
    let quiet_high = sawtooth(400.0, 300.0);
    let quiet_low = sawtooth(70.0, 300.0);
    let sil = silence();

    let mut frame_idx = 0usize;
    for cycle in 0..400 {
        let noise = lcg_noise(0xD1B54A32D192ED03_u64.wrapping_add(cycle as u64), 15000.0);
        let sequence: [&[i16]; 6] = [&loud_high, &sil, &loud_low, &quiet_high, &noise, &quiet_low];
        for samples in sequence {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(samples));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = &buf[..n];
            let bits = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let hex: String = bits.iter().map(|b| format!("{b:02x}")).collect();
            println!("abrupt\t{frame_idx}\t{hex}");
            frame_idx += 1;
        }
    }
}
