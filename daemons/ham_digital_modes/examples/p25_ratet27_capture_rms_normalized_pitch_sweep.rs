// SPDX-License-Identifier: LGPL-3.0-or-later
//! Critical methodological correction to `p25_ratet27_capture_dense_pitch_sweep.rs`: that sweep
//! held the sawtooth's *peak* amplitude fixed across frequencies, but a discretely-sampled
//! sawtooth's actual RMS/energy is NOT frequency-independent when the period is a large fraction of
//! the 160-sample frame (boundary/incomplete-cycle effects) -- and a separate clean amplitude sweep
//! found `g0` correlates just as strongly (Spearman 1.000) with amplitude as it did with frequency.
//! This means the earlier "g0 tracks pitch" finding may have been partly or wholly an amplitude/RMS
//! confound, not a real pitch effect. This tool explicitly computes each generated buffer's actual
//! RMS and rescales it to a fixed target before sending, so amplitude is held genuinely constant
//! (not just nominally so) across the same 57-444Hz frequency sweep, isolating whether `g0` still
//! tracks frequency once RMS is properly controlled.
//!
//! Usage: `cargo run --release --example p25_ratet27_capture_rms_normalized_pitch_sweep -- <host:port>`
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 60;
const CAPTURE_FRAMES: usize = 8;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const TARGET_RMS: f64 = 3464.0; // ~6000/sqrt(3), the ideal continuous sawtooth RMS used previously

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
/// Sawtooth at `freq`, rescaled so this exact 160-sample buffer's own actual RMS equals
/// `TARGET_RMS` -- correcting for the boundary/incomplete-cycle effects a naive fixed-peak sawtooth
/// has at low frequencies, so amplitude is genuinely (not just nominally) held constant.
fn rms_normalized_sawtooth(freq: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    let raw: Vec<f64> = (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            2.0 * phase - 1.0
        })
        .collect();
    let rms: f64 = (raw.iter().map(|&x| x * x).sum::<f64>() / raw.len() as f64).sqrt();
    let scale = TARGET_RMS / rms;
    raw.iter().map(|&x| (x * scale).clamp(-32000.0, 32000.0) as i16).collect()
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

    let mut freq: f64 = 60.0;
    while freq <= 440.0 {
        let samples = rms_normalized_sawtooth(freq);
        let actual_rms: f64 =
            (samples.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / samples.len() as f64).sqrt();
        eprintln!("freq={freq} actual_rms={actual_rms:.1}");
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        for i in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech(&samples));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = &buf[..n];
            let bits = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let hex: String = bits.iter().map(|b| format!("{b:02x}")).collect();
            println!("rmsnorm_{freq}\t{i}\t{hex}");
        }
        freq += 20.0;
    }
}
