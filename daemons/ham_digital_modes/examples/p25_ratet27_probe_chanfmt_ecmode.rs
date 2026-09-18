// SPDX-License-Identifier: LGPL-3.0-or-later
//! Empirical probe: DVSI's manual documents `PKT_CHANFMT` (field `0x15`) as able to make output
//! CHANNEL packets always include the `ECMODE_OUT` status word (`VOICE_ACTIVE`, `TONE_FRAME`
//! flags) -- ground truth for what the chip itself classified a frame as, which would be much more
//! direct evidence for `g3`'s behavior than inferring from stimulus type alone. The manual states
//! the *option* exists (`ecmode` bits 0-1 of the `PKT_CHANFMT` data word, value `0b01` = "always")
//! but doesn't show a worked example of the resulting output packet layout. This tool sends that
//! control field, then dumps the *raw* response packet bytes for both a loud tone and silence, to
//! empirically determine whether/where the packet grew relative to the normal 18-byte CHAND-only
//! response this investigation has relied on throughout.
//!
//! Usage: `cargo run --release --example p25_ratet27_probe_chanfmt_ecmode -- <host:port>`
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_ECMODE: u8 = 0x05;
const FIELD_CHANFMT: u8 = 0x15;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];

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
fn sawtooth(freq: f64, amp: f64) -> Vec<i16> {
    let period = 8000.0 / freq;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            (amp * (2.0 * phase - 1.0)) as i16
        })
        .collect()
}
fn dtmf_tone(row_hz: f64, col_hz: f64, amp: f64) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|n| {
            let t = n as f64 / 8000.0;
            let s = (2.0 * std::f64::consts::PI * row_hz * t).sin()
                + (2.0 * std::f64::consts::PI * col_hz * t).sin();
            (amp * 0.5 * s) as i16
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
    println!("RATEP response ({n} bytes): {:02x?}", &buf[..n]);

    // Reset ECMODE_IN to a known, clean baseline (TD_ENABLE on, bit 12, matching the chip's own
    // documented reset default; every other bit off) -- the earlier probe run inherited leftover
    // state from a previous DTX_ENABLE test, contaminating that reading.
    sock.send(&build_control_ecmode(1 << 12)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    println!("ECMODE reset response ({n} bytes): {:02x?}", &buf[..n]);

    // ecmode bits 0-1 = 0b01 ("always contain ecmode field"); all other bits 0 per the manual's
    // own explicit warning that reserved bits must be 0.
    sock.send(&build_control_chanfmt(0b01)).expect("send CHANFMT config");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    println!("CHANFMT response ({n} bytes): {:02x?}", &buf[..n]);

    let loud_tone = sawtooth(200.0, 16000.0);
    let silence = vec![0i16; FRAME_SAMPLES];
    let dtmf = dtmf_tone(697.0, 1209.0, 9000.0);
    let noise: Vec<i16> = {
        let mut state = 42u64;
        (0..FRAME_SAMPLES)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (((state >> 33) as f64 / (1u64 << 31) as f64 - 1.0) * 8000.0) as i16
            })
            .collect()
    };

    for (label, samples) in
        [("loud_tone", &loud_tone), ("silence", &silence), ("dtmf_1", &dtmf), ("noise", &noise)]
    {
        // Settle first so the classification reflects steady-state, not transition frames.
        for _ in 0..30 {
            sock.send(&build_speech(samples)).expect("send speech");
            sock.recv(&mut buf).expect("recv channel");
        }
        for i in 0..5 {
            sock.send(&build_speech(samples)).expect("send speech");
            let n = sock.recv(&mut buf).expect("recv channel");
            let ecmode_out = u16::from_be_bytes([buf[n - 2], buf[n - 1]]);
            println!(
                "{label} frame {i} ({n} bytes): ecmode_out=0x{ecmode_out:04x} (VOICE_ACTIVE={} TONE_FRAME={})  raw={:02x?}",
                (ecmode_out >> 1) & 1,
                (ecmode_out >> 15) & 1,
                &buf[..n]
            );
        }
    }
}
