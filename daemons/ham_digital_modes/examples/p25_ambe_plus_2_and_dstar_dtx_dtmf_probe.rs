// SPDX-License-Identifier: LGPL-3.0-or-later
//! Closes a real coverage gap this document's own executive summary implied but never checked:
//! `ECMODE_IN`'s `DTX_ENABLE`/`TS_ENABLE` bits (and the resulting DTX-silence / DTMF-tone special
//! frames `ambe::ratet27_dtx`/`ambe::ratet27_dtmf` classify) were only ever tested under
//! `RATET(27)` (P25 full-rate FEC). `ECMODE_IN` is documented as a global encoder-side control
//! (DVSI's own manual, section 39), so the same DTX/DTMF behavior should appear under D-STAR and
//! AMBE+2 half-rate too -- but this project's own D-STAR and AMBE+2 decoders had never actually been
//! pointed at a silence or DTMF capture to check.
//!
//! This matters for a reason sharper than "untested": `ambe_plus_2::decode::classify_b0` already
//! implements DVSI/TIA's own published special-value convention for the half-rate family --
//! `b0 in 0..=119` is `Speech`, `120..=123` is `Erasure`, `124..=125` is `Silence`, `126..=127` is
//! `Tone` -- straight from the spec, not reverse-engineered. `ambe_dstar::decode::dequantize`, by
//! contrast, **had no such check at all**: it ran every `b0` value straight through the speech
//! dequantization path (clamping only to stay in `L_TABLE`'s bounds).
//!
//! **Checked directly against real mbelib source (`ambe3600x2400.c`, this project's own primary
//! reference for the whole `ambe_dstar` module), not assumed**: D-STAR's real special-value trigger
//! is narrower than a naive reading of `L_TABLE`'s 126-entry length suggests. mbelib's own decoder
//! only special-cases a frame when `(b0 & 0x7E) == 0x7E`, i.e. `b0` is *exactly* 126 or 127 -- not a
//! broader 120..127 block the way AMBE+2 half-rate reserves. `ambe_dstar::decode::dequantize`
//! therefore only had a real gap against its own primary source for those two exact values, not the
//! wider range an earlier draft of this file's own doc comment incorrectly inferred from table size
//! alone (caught in this same session before that inference reached the findings doc).
//!
//! **The gap is now closed in `ambe_dstar::decode`** (`FrameKind`/`classify_b0`, `TonePayload`/
//! `decode_tone`, `dtmf_digit_from_tone_index`) -- this probe's own `decode_full` calls that shipped
//! implementation directly rather than a private copy of the same logic.
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example p25_ambe_plus_2_and_dstar_dtx_dtmf_probe -- <host:port>`
use ham_digital_modes::ambe_dstar::decode::{
    decode_tone as dstar_decode_tone, extract_raw_parameters as dstar_extract_raw,
    parse_frame as dstar_parse_frame,
};
use ham_digital_modes::ambe_dstar::interleave::wire_bytes_to_frame;
use ham_digital_modes::ambe_plus_2::decode::{classify_b0, extract_raw_parameters, FrameKind};
use ham_digital_modes::ambe_plus_2::interleave::interleaved_to_frame;
use ham_digital_modes::ambe_plus_2::parse_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_RATET: u8 = 0x09;
const FIELD_ECMODE: u8 = 0x05;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const SAMPLE_RATE: f64 = 8000.0;
const FRAME_SAMPLES: usize = 160;
const SETTLING_FRAMES: usize = 80;
const CAPTURE_FRAMES: usize = 8;

const DTX_ENABLE_BIT: u16 = 1 << 11;
const TD_ENABLE_BIT: u16 = 1 << 12; // on-by-default per section 25/36 -- must be re-set explicitly
                                     // whenever this tool writes ECMODE_IN at all, since a full
                                     // 16-bit register write with DTX_ENABLE alone silently clears it.
const RATEP_DSTAR: [u16; 6] = [0x0130, 0x0763, 0x4000, 0x0000, 0x0000, 0x0048];
const RATET_HALF_RATE_FEC: u8 = 33;

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
fn sine_tone(freq: f64, amp: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (amp * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
        .collect()
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

/// (`b0`, ordinary-speech `b1`, ordinary-speech `b2`, real tone index, real tone volume [only
/// meaningful when `b0` is a tone frame -- see `ambe_dstar::decode::decode_tone`], `epsilon_c0`,
/// `epsilon_c1`).
type DstarToneCapture = (u32, u32, u32, u32, u32, u32, u32);

fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 256], pkt: &[u8]) -> usize {
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

/// Settles on `samples`, then returns the last `CAPTURE_FRAMES` raw CHANNEL payloads (bits + count).
fn capture(sock: &UdpSocket, samples: &[i16]) -> Vec<(Vec<u8>, usize)> {
    let mut buf = [0u8; 256];
    for _ in 0..SETTLING_FRAMES {
        let n = send_recv_retrying(sock, &mut buf, &build_speech(samples));
        parse_packet(&buf[..n]).expect("valid packet");
    }
    let mut out = Vec::with_capacity(CAPTURE_FRAMES);
    for _ in 0..CAPTURE_FRAMES {
        let n = send_recv_retrying(sock, &mut buf, &build_speech(samples));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL);
        let num_bits = payload[1] as usize;
        let nbytes = num_bits.div_ceil(8);
        out.push((payload[2..2 + nbytes].to_vec(), num_bits));
    }
    out
}

fn hex(frame: &(Vec<u8>, usize)) -> String {
    frame.0.iter().map(|b| format!("{b:02x}")).collect()
}

fn set_ecmode(sock: &UdpSocket, buf: &mut [u8; 256], ecmode_in: u16) {
    sock.send(&build_control_ecmode(ecmode_in)).expect("send ECMODE config");
    let n = sock.recv(buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
}

fn probe_ambe_plus_2(sock: &UdpSocket) {
    println!("\n=== AMBE+2 half-rate (RATET {RATET_HALF_RATE_FEC}), DTX/DTMF probe ===");
    sock.send(&build_control_ratet(RATET_HALF_RATE_FEC)).expect("send RATET config");
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).expect("RATET config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let decode_full = |frame: &(Vec<u8>, usize)| -> Option<(u32, FrameKind, u32, u32)> {
        let (bytes, num_bits) = frame;
        if *num_bits != 72 || bytes.len() < 9 {
            return None;
        }
        let mut wire: u128 = 0;
        for &byte in bytes.iter().take(9) {
            wire = (wire << 8) | byte as u128;
        }
        let logical = interleaved_to_frame(wire);
        let parsed = parse_frame(logical);
        let raw = extract_raw_parameters(parsed.d);
        Some((raw.b0, classify_b0(raw.b0), raw.b1, raw.b2))
    };

    let silence = vec![0i16; FRAME_SAMPLES];
    set_ecmode(sock, &mut buf, TD_ENABLE_BIT); // DTX off, TD at its real default -- control capture
    println!("-- silence, DTX_ENABLE off (control) --");
    for f in capture(sock, &silence) {
        println!("  {:?}", decode_full(&f));
    }

    set_ecmode(sock, &mut buf, DTX_ENABLE_BIT | TD_ENABLE_BIT);
    println!("-- silence, DTX_ENABLE on --");
    for f in capture(sock, &silence) {
        println!("  {:?}", decode_full(&f));
    }

    let loud = sine_tone(200.0, 16000.0);
    println!("-- loud 200Hz tone, DTX_ENABLE on -- (b0, kind, b1, b2)");
    for f in capture(sock, &loud) {
        println!("  {:?}", decode_full(&f));
    }

    let rows = [697.0, 770.0, 852.0, 941.0];
    let cols = [1209.0, 1336.0, 1477.0, 1633.0];
    let digit_names = [["1", "2", "3", "A"], ["4", "5", "6", "B"], ["7", "8", "9", "C"], ["*", "0", "#", "D"]];
    println!("-- 16 DTMF digits, DTX_ENABLE + TD_ENABLE both explicitly on, all {CAPTURE_FRAMES} captured frames each --");
    for (ri, &row) in rows.iter().enumerate() {
        for (ci, &col) in cols.iter().enumerate() {
            let samples = dtmf_tone(row, col, 9000.0);
            let frames = capture(sock, &samples);
            let decoded: Vec<Option<(u32, FrameKind, u32, u32)>> = frames.iter().map(decode_full).collect();
            println!("  digit {}: {:?} hex={}", digit_names[ri][ci], decoded, hex(&frames[0]));
        }
    }

    // Isolate TD_ENABLE's own effect from a possible DTX x TD interaction: DTX off this time.
    set_ecmode(sock, &mut buf, TD_ENABLE_BIT);
    println!("-- loud 200Hz tone, DTX_ENABLE OFF, TD_ENABLE only (isolates TD from any DTX interaction) --");
    for f in capture(sock, &loud) {
        println!("  {:?}", decode_full(&f));
    }
    println!("-- 16 DTMF digits, DTX_ENABLE OFF, TD_ENABLE only --");
    for (ri, &row) in rows.iter().enumerate() {
        for (ci, &col) in cols.iter().enumerate() {
            let samples = dtmf_tone(row, col, 9000.0);
            let frames = capture(sock, &samples);
            let decoded: Vec<Option<(u32, FrameKind, u32, u32)>> = frames.iter().map(decode_full).collect();
            println!("  digit {}: {:?}", digit_names[ri][ci], decoded);
        }
    }

    set_ecmode(sock, &mut buf, TD_ENABLE_BIT); // reset to true default (DTX off, TD on), not 0x0000
}

fn probe_dstar(sock: &UdpSocket) {
    println!("\n=== D-STAR (RATEP custom), DTX/DTMF probe ===");
    sock.send(&build_control_ratep(RATEP_DSTAR)).expect("send RATEP config");
    let mut buf = [0u8; 256];
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");

    // (b0, ordinary-speech b1, ordinary-speech b2, real mbelib tone index, real mbelib tone volume
    // [only meaningful when b0 is a tone frame], epsilon_c0, epsilon_c1).
    let decode_full = |frame: &(Vec<u8>, usize)| -> Option<DstarToneCapture> {
        let (bytes, num_bits) = frame;
        if *num_bits != 72 || bytes.len() < 9 {
            return None;
        }
        let mut wire_bytes = [0u8; 9];
        wire_bytes.copy_from_slice(&bytes[..9]);
        let frame = wire_bytes_to_frame(&wire_bytes);
        let parsed = dstar_parse_frame(frame);
        let raw = dstar_extract_raw(parsed.d);
        let tone = dstar_decode_tone(parsed.d);
        Some((raw.b0, raw.b1, raw.b2, tone.index, tone.volume, parsed.epsilon_c0, parsed.epsilon_c1))
    };

    let silence = vec![0i16; FRAME_SAMPLES];
    set_ecmode(sock, &mut buf, TD_ENABLE_BIT); // DTX off, TD at its real default -- control capture
    println!("-- silence, DTX_ENABLE off (control) -- (b0, b1, b2, epsilon_c0, epsilon_c1); real mbelib's");
    println!("   own ambe3600x2400.c only special-cases b0 when (b0 & 0x7E) == 0x7E, i.e. b0 in {{126,127}}");
    println!("   exactly -- not the broader 120..127 this tool's own earlier revision assumed");
    for f in capture(sock, &silence) {
        println!("  {:?}", decode_full(&f));
    }

    set_ecmode(sock, &mut buf, DTX_ENABLE_BIT | TD_ENABLE_BIT);
    println!("-- silence, DTX_ENABLE on --");
    for f in capture(sock, &silence) {
        println!("  {:?} hex={}", decode_full(&f), hex(&f));
    }

    let loud = sine_tone(200.0, 16000.0);
    println!("-- loud 200Hz tone, DTX_ENABLE on --");
    for f in capture(sock, &loud) {
        println!("  {:?} hex={}", decode_full(&f), hex(&f));
    }

    let rows = [697.0, 770.0, 852.0, 941.0];
    let cols = [1209.0, 1336.0, 1477.0, 1633.0];
    let digit_names = [["1", "2", "3", "A"], ["4", "5", "6", "B"], ["7", "8", "9", "C"], ["*", "0", "#", "D"]];
    println!("-- 16 DTMF digits, DTX_ENABLE + TD_ENABLE both explicitly on, all {CAPTURE_FRAMES} captured frames each --");
    for (ri, &row) in rows.iter().enumerate() {
        for (ci, &col) in cols.iter().enumerate() {
            let samples = dtmf_tone(row, col, 9000.0);
            let frames = capture(sock, &samples);
            let decoded: Vec<Option<DstarToneCapture>> = frames.iter().map(decode_full).collect();
            println!("  digit {}: {:?} hex={}", digit_names[ri][ci], decoded, hex(&frames[0]));
        }
    }

    // Isolate TD_ENABLE's own effect from a possible DTX x TD interaction: DTX off this time.
    set_ecmode(sock, &mut buf, TD_ENABLE_BIT);
    println!("-- loud 200Hz tone, DTX_ENABLE OFF, TD_ENABLE only (isolates TD from any DTX interaction) --");
    for f in capture(sock, &loud) {
        println!("  {:?}", decode_full(&f));
    }
    println!("-- 16 DTMF digits, DTX_ENABLE OFF, TD_ENABLE only --");
    for (ri, &row) in rows.iter().enumerate() {
        for (ci, &col) in cols.iter().enumerate() {
            let samples = dtmf_tone(row, col, 9000.0);
            let frames = capture(sock, &samples);
            let decoded: Vec<Option<DstarToneCapture>> = frames.iter().map(decode_full).collect();
            println!("  digit {}: {:?}", digit_names[ri][ci], decoded);
        }
    }

    set_ecmode(sock, &mut buf, TD_ENABLE_BIT); // reset to true default (DTX off, TD on), not 0x0000
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let host = args.get(1).cloned().unwrap_or_else(|| "192.168.10.189:2460".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    probe_ambe_plus_2(&sock);
    probe_dstar(&sock);
}
