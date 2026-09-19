// SPDX-License-Identifier: LGPL-3.0-or-later
//! Live chip validation for `ambe_dstar::decode`'s tone/DTMF path: feeds all 16 real ITU-T Q.23
//! DTMF digit tones to the chip under D-STAR with `TD_ENABLE` set, and confirms that
//! `classify_b0` reports `Tone`, `ECMODE_OUT`'s `TONE_FRAME` ground-truth bit reads 1, and
//! `dtmf_digit_from_tone_index` recovers the exact row/column pair for every one -- straight from
//! the chip's own live response, not the frozen fixtures `decode.rs`'s own unit tests check
//! against. Every other module with a live-fixture pair (`ratet27_dtmf`, `ambe_plus_2`) already has
//! a pass/fail validator beside its frozen fixtures; this closes that same gap for D-STAR's tone
//! decode, shipped in the same round as section 40's D-STAR tone/DTMF findings.
//!
//! Usage: `cargo run --release --example ambe_chip_validate_dstar_tone -- <host:port>`
use ham_digital_modes::ambe::float::dstar::decode::{
    classify_b0, dtmf_digit_from_tone_index, extract_raw_parameters, parse_frame, FrameKind,
};
use ham_digital_modes::ambe::float::dstar::interleave::wire_bytes_to_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_ECMODE: u8 = 0x05;
const FIELD_CHANFMT: u8 = 0x15;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const SETTLING_FRAMES: usize = 80;
const CAPTURE_FRAMES: usize = 8;
const BITS_OFFSET: usize = 6;

const TD_ENABLE_BIT: u16 = 1 << 12;
const RATEP_DSTAR: [u16; 6] = [0x0130, 0x0763, 0x4000, 0x0000, 0x0000, 0x0048];

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
fn parse_packet(data: &[u8]) -> Option<(u8, &[u8])> {
    if data.len() < 4 || data[0] != 0x61 {
        return None;
    }
    let length = u16::from_be_bytes([data[1], data[2]]) as usize;
    let ptype = data[3];
    data.get(4..4 + length).map(|payload| (ptype, payload))
}
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
fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 512], pkt: &[u8]) -> usize {
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
}
/// Settles on `samples`, then returns the last `CAPTURE_FRAMES` full raw CHANNEL packets (with
/// `ECMODE_OUT` appended as the trailing 2 bytes, per `PKT_CHANFMT`'s own `0b01` request).
fn capture(sock: &UdpSocket, samples: &[i16]) -> Vec<Vec<u8>> {
    let mut buf = [0u8; 512];
    for _ in 0..SETTLING_FRAMES {
        let n = send_recv_retrying(sock, &mut buf, &build_speech(samples));
        parse_packet(&buf[..n]).expect("valid packet");
    }
    let mut out = Vec::with_capacity(CAPTURE_FRAMES);
    for _ in 0..CAPTURE_FRAMES {
        let n = send_recv_retrying(sock, &mut buf, &build_speech(samples));
        let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        out.push(buf[..n].to_vec());
    }
    out
}
fn tone_frame_bit(pkt: &[u8]) -> u16 {
    let ecmode_out = u16::from_be_bytes([pkt[pkt.len() - 2], pkt[pkt.len() - 1]]);
    (ecmode_out >> 15) & 1
}
fn channel_bits(pkt: &[u8]) -> &[u8] {
    &pkt[BITS_OFFSET..pkt.len() - 2]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratep(RATEP_DSTAR)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_ecmode(TD_ENABLE_BIT)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b01)).expect("send CHANFMT config");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let rows = [697.0, 770.0, 852.0, 941.0];
    let cols = [1209.0, 1336.0, 1477.0, 1633.0];
    let digit_names = [["1", "2", "3", "A"], ["4", "5", "6", "B"], ["7", "8", "9", "C"], ["*", "0", "#", "D"]];

    let mut all_ok = true;
    for (ri, &row) in rows.iter().enumerate() {
        for (ci, &col) in cols.iter().enumerate() {
            let samples = dtmf_tone(row, col, 9000.0);
            let pkts = capture(&sock, &samples);
            // Check every captured frame, not just the last -- a transient/dropped frame among
            // the 8 would otherwise hide behind a `last()`-only check.
            let mut digit_ok = true;
            for pkt in &pkts {
                let bits = channel_bits(pkt);
                let mut wire_bytes = [0u8; 9];
                wire_bytes.copy_from_slice(&bits[..9]);
                let frame = wire_bytes_to_frame(&wire_bytes);
                let parsed = parse_frame(frame);
                let raw = extract_raw_parameters(parsed.d);
                let kind = classify_b0(raw.b0);
                let is_tone = matches!(kind, FrameKind::Tone);
                let tone_frame = tone_frame_bit(pkt) == 1;
                let decoded = if is_tone {
                    let tone = ham_digital_modes::ambe::float::dstar::decode::decode_tone(parsed.d);
                    dtmf_digit_from_tone_index(tone.index)
                } else {
                    None
                };
                let frame_ok = is_tone && tone_frame && decoded == Some((ri as u8, ci as u8));
                if !frame_ok {
                    digit_ok = false;
                }
                println!(
                    "  digit {}: b0={} kind={kind:?} TONE_FRAME={} decoded={decoded:?} expected=Some(({ri},{ci})) {}",
                    digit_names[ri][ci],
                    raw.b0,
                    tone_frame_bit(pkt),
                    if frame_ok { "OK" } else { "MISMATCH" }
                );
            }
            if !digit_ok {
                all_ok = false;
            }
        }
    }

    // Reset to a known-clean state.
    sock.send(&build_control_ecmode(TD_ENABLE_BIT)).expect("send ECMODE reset");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b00)).expect("send CHANFMT reset");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");

    if all_ok {
        println!(
            "\nPASS: all 16 DTMF digits, all {CAPTURE_FRAMES} captured frames each, decode correctly via \
             ambe_dstar::decode (classify_b0=Tone, TONE_FRAME=1, dtmf_digit_from_tone_index matches)."
        );
    } else {
        eprintln!("\nFAIL: at least one captured frame did not decode correctly.");
        std::process::exit(1);
    }
}
