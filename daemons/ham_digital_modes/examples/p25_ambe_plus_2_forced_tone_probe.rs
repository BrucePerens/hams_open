// SPDX-License-Identifier: LGPL-3.0-or-later
//! Decisive discriminator for the `b0=120` (`Erasure`, this rate's real `TD_ENABLE`-detected tone
//! serialization) vs. `b0=126/127` (`Tone`, the spec-defined range this document has never actually
//! observed the chip emit) question: DVSI's own manual documents a *forced*-tone-generation path,
//! independent of tone detection entirely -- a `TONE` field (`0x08`, `TONE_IDX` byte, amplitude
//! byte) appended to a `SPEECH` packet, with `ECMODE_IN`'s `TS_ENABLE` bit (14) set, "force[s] the
//! encoder to transmit a tone frame" for the specified `TONE_IDX` (`AMBE-3000R` manual Table 98/103,
//! page 72-74). This bypasses the tone *detector* entirely, so it answers a narrower, cleaner
//! question than any detection-based probe could: when the encoder is *told* to emit `TONE_IDX=0x81`
//! (this rate's own 33-61-column code for DTMF '4' -- see `ambe_plus_2::decode::decode_tone_idx`'s
//! doc comment for why this probe's readback doesn't match the digit `0x81` names in that column)
//! directly, does the resulting channel frame's `b0` read `120` (matching every
//! detection-triggered capture in this document) or `126/127` (the spec's own documented `Tone`
//! range, never yet observed)?
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example p25_ambe_plus_2_forced_tone_probe -- <host:port>`
use ham_digital_modes::ambe::float::ambe_plus_2::decode::{classify_b0, decode_tone_idx, extract_raw_parameters};
use ham_digital_modes::ambe::float::ambe_plus_2::interleave::interleaved_to_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::parse_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATET: u8 = 0x09;
const FIELD_ECMODE: u8 = 0x05;
const FIELD_CHANFMT: u8 = 0x15;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const FRAME_SAMPLES: usize = 160;
const BITS_OFFSET: usize = 6;

const TS_ENABLE_BIT: u16 = 1 << 14;
const RATET_HALF_RATE_FEC: u8 = 33;

fn build_control_ratet(index: u8) -> Vec<u8> {
    let payload = vec![FIELD_RATET, index];
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
/// A SPEECH packet carrying silence via SPEECHD (field 0x00) plus a forced TONE field (field 0x08:
/// TONE_IDX byte, amplitude byte) -- manual Table 98/103. Sending both SPEECHD and TONE in the same
/// packet matches the manual's own worked example (section 6.11.2).
fn build_speech_with_tone(tone_idx: u8, amp: u8) -> Vec<u8> {
    let mut payload = vec![0x00_u8, FRAME_SAMPLES as u8];
    for _ in 0..FRAME_SAMPLES {
        payload.extend_from_slice(&0i16.to_be_bytes());
    }
    payload.push(0x08);
    payload.push(tone_idx);
    payload.push(amp);
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

    sock.send(&build_control_ratet(RATET_HALF_RATE_FEC)).expect("send RATET config");
    let n = sock.recv(&mut buf).expect("RATET config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_ecmode(TS_ENABLE_BIT)).expect("send ECMODE config (TS_ENABLE)");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b01)).expect("send CHANFMT config");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");

    // Force DTMF '1' (TONE_IDX=0x81) and a single tone (TONE_IDX=0x08, matching earlier detection
    // captures) at max amplitude (0x00 = 0 dBm0, the loudest per Table 105).
    let stimuli: Vec<(String, u8)> = (0x80u8..=0x8Fu8)
        .map(|idx| (format!("DTMF 0x{idx:02x}"), idx))
        .chain([
            ("single tone (0x08)".to_string(), 0x08u8),
            ("single tone (0x0d)".to_string(), 0x0du8),
            ("Call Progress: Dial Tone (0xa0)".to_string(), 0xa0u8),
            ("Call Progress: Ring Tone (0xa1)".to_string(), 0xa1u8),
            ("Call Progress: Busy Tone (0xa2)".to_string(), 0xa2u8),
        ])
        .collect();
    for (label, tone_idx) in stimuli {
        println!("-- forcing TONE_IDX=0x{tone_idx:02x} ({label}) --");
        for frame in 0..2 {
            let n = send_recv_retrying(&sock, &mut buf, &build_speech_with_tone(tone_idx, 0x00));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = buf[..n].to_vec();
            let bits = channel_bits(&pkt);
            let mut wire: u128 = 0;
            for &b in bits.iter().take(9) {
                wire = (wire << 8) | b as u128;
            }
            let logical = interleaved_to_frame(wire);
            let parsed = parse_frame(logical);
            let raw = extract_raw_parameters(parsed.d);
            let kind = classify_b0(raw.b0);
            let decoded = decode_tone_idx(parsed.d);
            println!(
                "  frame {frame}: b0={} kind={kind:?} TONE_FRAME={} decode_tone_idx={decoded:?}",
                raw.b0,
                tone_frame_bit(&pkt)
            );
        }
    }

    // Reset to a known-clean state.
    sock.send(&build_control_ecmode(0)).expect("send ECMODE reset");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b00)).expect("send CHANFMT reset");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");
}
