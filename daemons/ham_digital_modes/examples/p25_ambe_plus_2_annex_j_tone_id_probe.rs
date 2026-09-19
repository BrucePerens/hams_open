// SPDX-License-Identifier: LGPL-3.0-or-later
//! Section 40 resolved AMBE+2 half-rate's Erasure-vs-Tone asymmetry as a genuine detection (the
//! chip's own `TONE_FRAME` ground-truth bit reads 1 for every tested tone/DTMF stimulus) with a
//! serialization difference (it lands in `b0=120`, the spec's own `Erasure` range, instead of
//! `126/127`). What's still open: DVSI's own Annex J documents a dedicated tone-frame parameter
//! table (`ID` 0-254 -> `f0`/`l1`/`l2`, transcribed in full in `src/ambe/AMBE_PLUS_2_NOTES.md`) that
//! plausibly carries exactly the missing per-tone identity -- but this codebase has never derived
//! where that `ID` field's bits live inside a real tone frame's 49-bit `d`, and neither does the
//! reference decoder (checked directly: mbelib's `mbe_decodeAmbe2450Parms` returns immediately on
//! `b0 in {126,127}` without reading any payload bits).
//!
//! This tool captures real chip output for a deliberately varied set of single-frequency tone
//! stimuli, chosen to span Annex J's own `ID` space (not just the 16 DTMF pairs already captured,
//! which cluster in one narrow sub-range if they land in 128-163 at all): 30 points across the
//! formula-driven ranges (`ID` 5-122, three points per range -- low/mid/high -- so a linear or
//! Gray-coded relationship between `ID` and some `d` window would show up the way section 9's pitch
//! field and D-STAR's own `128 + row + 4*col` tone index both did), 5 points from the individually
//! tabulated range (`ID` 128-163), and `ID` 255 (the fixed 250Hz special tone). Also varies
//! amplitude on 4 of the formula-driven points to separate `ID`-dependence from amplitude artifacts.
//!
//! This is a data-gathering + first-pass-analysis tool, not a validator -- it prints every captured
//! frame's raw hex and decoded `d`, requires `TONE_FRAME=1` on all 8 captures per stimulus (skipping
//! and flagging any stimulus the chip doesn't actually classify as a tone), then runs a variance
//! mask and an exact-bit-match search (plain and Gray-coded, all window widths up to 8 bits) against
//! the expected `ID` value -- the same techniques already proven in this document (§9, D-STAR's
//! tone index) to find a real field before falling back to anything more elaborate. A clean null
//! result (no exact match found) is still real data for a future session -- printed plainly, not
//! hidden.
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example p25_ambe_plus_2_annex_j_tone_id_probe -- <host:port>`
use ham_digital_modes::ambe_plus_2::decode::{classify_b0, extract_raw_parameters, FrameKind};
use ham_digital_modes::ambe_plus_2::interleave::interleaved_to_frame;
use ham_digital_modes::ambe_plus_2::parse_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATET: u8 = 0x09;
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
const D_BITS: usize = 49;

const TD_ENABLE_BIT: u16 = 1 << 12;
const RATET_HALF_RATE_FEC: u8 = 33;

/// One stimulus: Annex J `ID`, its formula/table-derived `f0` in Hz, and the amplitude to use.
struct Stimulus {
    id: u32,
    f0_hz: f64,
    amp: f64,
}

fn annex_j_stimuli() -> Vec<Stimulus> {
    // Formula-driven ranges from AMBE_PLUS_2_NOTES.md's own Annex J table -- low/mid/high `ID` per
    // range, all at a fixed loud amplitude, to span the full ID space.
    let ranges: [(u32, u32, f64); 10] = [
        (5, 12, 31.250),
        (13, 25, 15.625),
        (26, 38, 10.417),
        (39, 51, 7.8125),
        (52, 64, 6.2500),
        (65, 76, 5.2803),
        (77, 89, 4.4643),
        (90, 102, 3.9063),
        (103, 115, 3.4722),
        (116, 122, 3.1250),
    ];
    let mut out = Vec::new();
    for &(lo, hi, k) in &ranges {
        let mid = (lo + hi) / 2;
        for id in [lo, mid, hi] {
            out.push(Stimulus { id, f0_hz: k * id as f64, amp: 16000.0 });
        }
    }
    // Individually tabulated range, straight from the table.
    for &(id, f0) in &[(128u32, 78.5), (140, 71.0), (150, 67.7), (160, 87.78), (163, 70.0)] {
        out.push(Stimulus { id, f0_hz: f0, amp: 16000.0 });
    }
    // The fixed special tone.
    out.push(Stimulus { id: 255, f0_hz: 250.0, amp: 16000.0 });
    // Amplitude variants on 4 formula-driven points, to separate ID-dependence from amplitude
    // artifacts in the analysis below.
    for &(id, f0) in &[(8u32, 250.0), (38, 395.85), (76, 401.3), (115, 399.3)] {
        for amp in [6000.0, 11000.0, 20000.0] {
            out.push(Stimulus { id, f0_hz: f0, amp });
        }
    }
    out
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
fn sine_tone(freq: f64, amp: f64) -> Vec<i16> {
    let period = SAMPLE_RATE / freq;
    (0..FRAME_SAMPLES)
        .map(|n| (amp * (2.0 * std::f64::consts::PI * (n as f64 % period) / period).sin()) as i16)
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
fn window_value(d: u64, start: usize, width: usize) -> u32 {
    let shift = D_BITS - start - width;
    ((d >> shift) & ((1u64 << width) - 1)) as u32
}
fn gray_to_binary(g: u32) -> u32 {
    let mut b = g;
    let mut shift = 1;
    while (g >> shift) != 0 {
        b ^= g >> shift;
        shift += 1;
    }
    b
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
    sock.send(&build_control_ecmode(TD_ENABLE_BIT)).expect("send ECMODE config");
    let n = sock.recv(&mut buf).expect("ECMODE config response");
    parse_packet(&buf[..n]).expect("valid packet");
    sock.send(&build_control_chanfmt(0b01)).expect("send CHANFMT config");
    let n = sock.recv(&mut buf).expect("CHANFMT config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let stimuli = annex_j_stimuli();
    // (id, d value) for every captured frame that actually reads TONE_FRAME=1.
    let mut samples: Vec<(u32, u64)> = Vec::new();
    let mut skipped = 0usize;

    println!("id,f0_hz,amp,frame_idx,b0,kind,tone_frame,d_hex");
    for s in &stimuli {
        let wave = sine_tone(s.f0_hz, s.amp);
        let pkts = capture(&sock, &wave);
        for (i, pkt) in pkts.iter().enumerate() {
            let bits = channel_bits(pkt);
            let mut wire: u128 = 0;
            for &b in bits.iter().take(9) {
                wire = (wire << 8) | b as u128;
            }
            let logical = interleaved_to_frame(wire);
            let parsed = parse_frame(logical);
            let raw = extract_raw_parameters(parsed.d);
            let kind = classify_b0(raw.b0);
            let tf = tone_frame_bit(pkt);
            println!(
                "{},{:.2},{:.0},{i},{},{kind:?},{tf},{:013x}",
                s.id, s.f0_hz, s.amp, raw.b0, parsed.d
            );
            if tf == 1 && matches!(kind, FrameKind::Erasure | FrameKind::Tone) {
                samples.push((s.id, parsed.d));
            } else {
                skipped += 1;
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

    println!(
        "\n=== analysis: {} usable frames ({} skipped -- not classified as tone-with-TONE_FRAME=1) ===",
        samples.len(),
        skipped
    );

    if samples.is_empty() {
        println!("No usable frames captured -- cannot analyze.");
        return;
    }

    // Variance mask: which of the 49 `d` bits ever change across all usable samples.
    let mut any_one = 0u64;
    let mut any_zero = 0u64;
    for &(_, d) in &samples {
        any_one |= d;
        any_zero |= !d & ((1u64 << D_BITS) - 1);
    }
    let varying = any_one & any_zero;
    print!("variance mask (1=varies, MSB..LSB of d[0..49)): ");
    for bit in 0..D_BITS {
        let shift = D_BITS - 1 - bit;
        print!("{}", (varying >> shift) & 1);
    }
    println!();

    // Exact-match search: for each width 1..=8, each start position, does the window (plain or
    // Gray-coded) equal the expected Annex J `ID` for every single sample?
    println!("-- exact-match search against Annex J `ID` (plain and gray, widths 1..=8) --");
    let mut found_any = false;
    for width in 1..=8usize {
        for start in 0..=(D_BITS - width) {
            let mut plain_ok = true;
            let mut gray_ok = true;
            for &(id, d) in &samples {
                let expected = id & ((1u32 << width) - 1);
                let v = window_value(d, start, width);
                if v != expected {
                    plain_ok = false;
                }
                if gray_to_binary(v) != expected {
                    gray_ok = false;
                }
                if !plain_ok && !gray_ok {
                    break;
                }
            }
            if plain_ok {
                println!("  EXACT MATCH (plain): d[{start}..{}) == low {width} bits of ID for all {} samples", start + width, samples.len());
                found_any = true;
            }
            if gray_ok {
                println!("  EXACT MATCH (gray):  d[{start}..{}) == low {width} bits of ID for all {} samples", start + width, samples.len());
                found_any = true;
            }
        }
    }
    if !found_any {
        println!("  no exact match found for any window width 1..=8 -- a real null result, not skipped.");
    }
}
