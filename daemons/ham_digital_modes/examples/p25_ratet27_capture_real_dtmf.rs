// SPDX-License-Identifier: LGPL-3.0-or-later
//! Follow-up to `p25_ratet27_capture_tone_send_forced.rs`: that tool forced `TS_ENABLE`
//! unconditionally with a plain sawtooth and got a constant, input-independent placeholder pattern,
//! suggesting the chip's own tone-frame substitution needs genuinely tone-classified input (a real
//! DTMF pair, KNOX tone, or call-progress tone), not just the flag alone. This tool instead uses
//! **default** `ECMODE_IN` (`TD_ENABLE` on, `TS_ENABLE` off, as the chip's own reset state already
//! has) and feeds **real standard DTMF digit tone pairs** (dual sinusoids at the actual ITU-T Q.23
//! frequency pairs, not the arbitrary pairs `p25_ratet27_capture_exotic_stimuli.rs` used earlier),
//! to see whether the chip's own tone-detection logic recognizes them and changes its channel
//! output differently from an arbitrary, non-DTMF dual-tone or single-tone signal.
//!
//! Usage: `cargo run --release --example p25_ratet27_capture_real_dtmf -- <host:port>`
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
const CAPTURE_FRAMES: usize = 10;
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
/// A real DTMF digit tone: sum of its row and column sinusoid, per ITU-T Q.23 -- not the arbitrary
/// dual-tone pairs used elsewhere in this investigation.
fn dtmf_tone(row_hz: f64, col_hz: f64, amp: f64) -> Vec<i16> {
    (0..FRAME_SAMPLES)
        .map(|n| {
            let t = n as f64 / SAMPLE_RATE;
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
    parse_packet(&buf[..n]).expect("valid DVSI packet");
    // ECMODE_IN left at its default (TD_ENABLE on, TS_ENABLE off) -- the chip's own reset state,
    // not forced -- so its own tone-classification logic decides.

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

    // All 16 real DTMF digits, ITU-T Q.23 row/column frequency pairs.
    let rows = [697.0, 770.0, 852.0, 941.0];
    let cols = [1209.0, 1336.0, 1477.0, 1633.0];
    let digit_names = [
        ["1", "2", "3", "A"],
        ["4", "5", "6", "B"],
        ["7", "8", "9", "C"],
        ["*", "0", "#", "D"],
    ];

    for (ri, &row) in rows.iter().enumerate() {
        for (ci, &col) in cols.iter().enumerate() {
            let samples = dtmf_tone(row, col, 9000.0);
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
                println!("dtmf_{}\t{i}\t{hex}", digit_names[ri][ci]);
            }
        }
    }
}
