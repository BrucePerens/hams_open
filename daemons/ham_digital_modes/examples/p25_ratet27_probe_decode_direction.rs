// SPDX-License-Identifier: LGPL-3.0-or-later
//! A bounded, one-shot probe: has any RATET(27) example in this codebase ever driven the DVSI
//! chip's own *decode* direction (send it a real 144-bit channel frame, ask it to synthesize PCM
//! back)? A grep across every existing `examples/*ratet27*.rs` file found none that ever construct
//! an outgoing `TYPE_CHANNEL` packet -- every one only sends `TYPE_SPEECH` (PCM) and reads back
//! `TYPE_CHANNEL` (the chip's own *encoded* bits). This probe captures one real `TYPE_CHANNEL`
//! response from real recorded speech, then sends that exact same payload straight back to the chip
//! as an *outgoing* packet, to see empirically whether the chip accepts it and replies with PCM (or
//! an error, or silence) -- before committing to building a real chip-PCM-vs-synthesized-PCM
//! validation harness on the assumption that this direction even works.
//!
//! Usage: `cargo run --release --example p25_ratet27_probe_decode_direction -- <host:port>`

use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;

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
/// Builds an *outgoing* `TYPE_CHANNEL` packet carrying `payload` verbatim -- the same bytes a real
/// `TYPE_CHANNEL` response carries (`[field_byte, bit_count, ...18 bytes of bits]`), to test whether
/// the chip accepts this shape as an input as well as an output.
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
fn read_wav_mono_i16(path: &str) -> Vec<i16> {
    let data = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(&data[8..12], b"WAVE", "{path}: not a RIFF/WAVE file");
    assert_eq!(&data[36..40], b"data", "{path}: not a standard 44-byte-header PCM WAV");
    data[44..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let pcm = read_wav_mono_i16("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
    // A few settling frames first (matches every other capture tool's own convention), then take a
    // handful of real, likely-voiced frames a bit into the recording.
    let start_frame = 60;
    let mut captured_channel_payloads: Vec<Vec<u8>> = Vec::new();
    for i in 0..(start_frame + 5) {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        if i >= start_frame {
            captured_channel_payloads.push(payload.to_vec());
        }
    }
    println!("Captured {} real TYPE_CHANNEL payloads from real speech.", captured_channel_payloads.len());
    println!("First payload ({} bytes): {:02x?}", captured_channel_payloads[0].len(), captured_channel_payloads[0]);

    println!("\n-- Sending each captured channel payload back to the chip as an outgoing TYPE_CHANNEL packet --");
    for (i, payload) in captured_channel_payloads.iter().enumerate() {
        let pkt = build_channel(payload);
        let n = send_recv_retrying(&sock, &mut buf, &pkt);
        match parse_packet(&buf[..n]) {
            Some((ptype, resp_payload)) => {
                println!(
                    "frame {i}: sent TYPE_CHANNEL ({} bytes) -> got type={ptype:02x}, {} byte payload: {:02x?}",
                    payload.len(),
                    resp_payload.len(),
                    &resp_payload[..resp_payload.len().min(24)]
                );
                if ptype == TYPE_SPEECH {
                    println!("  -> looks like PCM! declared sample count byte(s): {resp_payload:02x?}");
                }
            }
            None => println!("frame {i}: sent TYPE_CHANNEL -> got an unparseable response ({n} bytes): {:02x?}", &buf[..n.min(32)]),
        }
    }
}
