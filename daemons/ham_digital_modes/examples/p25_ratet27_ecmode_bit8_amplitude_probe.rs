// SPDX-License-Identifier: LGPL-3.0-or-later
//! Section 36 found `ECMODE_IN` bit 8 is a real, previously undocumented feature (shifts
//! `g1`/`g2`/`u4`/`u5`/`u6`/`c7` on identical content, leaves `g0`/`g3` unaffected), most plausibly
//! one of noise suppression, echo cancellation, or companding -- named as existing-but-untested
//! features, not distinguished from each other. This probe adds one discriminating test: **does bit
//! 8's effect size scale systematically with input amplitude?**
//!
//! The three candidates make different predictions on a clean, noise-free sawtooth (no actual noise
//! to suppress, no actual echo to cancel):
//! - **Companding** is a static nonlinear amplitude transform applied unconditionally to any input
//!   -- it should show a smooth, systematic, amplitude-dependent effect even on a perfectly clean
//!   tone (that's the whole point of a compander: it always acts on amplitude).
//! - **Noise suppression** should show little to no effect on a genuinely clean synthetic tone
//!   (nothing to suppress), regardless of amplitude.
//! - **Echo cancellation** should show little to no effect with no actual echo path present,
//!   regardless of amplitude.
//!
//! Section 36's own bit-8 test already showed a *large* effect on one plain sawtooth -- this probe
//! checks whether that effect's *size* tracks amplitude in a systematic way (supporting companding)
//! or looks amplitude-independent/erratic (arguing against it), across the same 16-point amplitude
//! sweep `p25_ratet27_capture_g0_long_settling_amplitude.rs` used for `g0`'s own confirmed curve.
//!
//! **Known measurement flaws in this tool's own output, found and corrected in section 38 -- read
//! that section before trusting this tool's raw numbers**: (1) this tool's "baseline" column
//! (`ECMODE_IN=0`) is contaminated by insufficient settling (60 frames) between each amplitude's
//! alternating baseline/bit-8 captures -- section 32's own adaptive-baseline effect doesn't fully
//! clear in that gap, so most baseline readings after the first don't match the clean, independently
//! re-verified values in `g0_long_settling_amplitude_sweep.tsv`. Use `p25_ratet27_ecmode_
//! default_state_probe.rs` for a properly-settled single-point measurement instead. (2) `capture()`
//! below only keeps the *last* of 8 captured frames (`last = (...)` is overwritten each iteration),
//! so the `u4` columns are single-frame samples of a dithering value, not a real distribution --
//! this is why `u4_base` shows an apparent cyclic pattern (steps of exactly 128) that is dither-phase
//! aliasing from under-sampling, not a real signal. `g0`'s own bit-8 column (a flat, unchanging
//! constant regardless of contamination) is unaffected by either flaw and remains a real finding.
//!
//! **Bit 8 is now confirmed as `CP_ENABLE` (Compand Enable) directly from DVSI's own manual (section
//! 39)** -- not merely narrowed to "companding" by this probe's own reasoning below. The mechanism is
//! also corrected in section 39: `CP_ENABLE` doesn't apply a gain curve, it tells the chip the
//! incoming samples' *format* (linear vs. µ-law/A-law); this tool always sent linear PCM, so setting
//! it made the chip misinterpret those samples as µ-law bytes and expand them, producing a garbled,
//! consistently-loud-reading signal rather than a genuine amplitude-dependent boost. The "does the
//! effect scale with amplitude" reasoning immediately below predates that correction and is kept for
//! its own historical record, not as the operative explanation.
//!
//! Usage: `cargo run --release --example p25_ratet27_ecmode_bit8_amplitude_probe -- <host:port>`
use ham_digital_modes::ambe::float::ratet27::ratet27_frame::decode_frame;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const FIELD_ECMODE: u8 = 0x05;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const FRAME_SAMPLES: usize = 160;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const SETTLING_FRAMES: usize = 60;
const CAPTURE_FRAMES: usize = 8;
const BITS_OFFSET: usize = 6;
const FRAME_BYTES: usize = 18;
const ECMODE_BIT8: u16 = 1 << 8;

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
    let period = 8000.0 / freq;
    (0..FRAME_SAMPLES)
        .map(|n| {
            let phase = (n as f64 % period) / period;
            (amp * (2.0 * phase - 1.0)) as i16
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
    parse_packet(&buf[..n]).expect("valid packet");

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

    let capture = |sock: &UdpSocket, buf: &mut [u8; 512], ecmode_in: u16, samples: &[i16]| -> (u16, u16) {
        sock.send(&build_control_ecmode(ecmode_in)).expect("send ECMODE config");
        let n = sock.recv(buf).expect("ECMODE config response");
        parse_packet(&buf[..n]).expect("valid packet");
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(sock, buf, &build_speech(samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        let mut last = (0u16, 0u16);
        for _ in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(sock, buf, &build_speech(samples));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = &buf[..n];
            let bits_bytes: &[u8; FRAME_BYTES] =
                pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES].try_into().unwrap();
            let frame = decode_frame(bits_bytes);
            last = (frame.g0.value, frame.u4.value);
        }
        last
    };

    // Same 16-point amplitude sweep as p25_ratet27_capture_g0_long_settling_amplitude.rs, so this
    // probe's results sit on the exact same amplitude scale as g0's own confirmed curve.
    let amps: Vec<f64> = (0..16).map(|i| 100.0 * (2.0_f64).powf(i as f64 * 7.0 / 15.0)).collect();

    println!("{:>10}  {:>8}  {:>8}  {:>8}  {:>8}  {:>10}  {:>10}", "amp", "g0_base", "g0_bit8", "u4_base", "u4_bit8", "g0_delta", "u4_delta");
    for &amp in &amps {
        let samples = sawtooth(200.0, amp);
        let (g0_base, u4_base) = capture(&sock, &mut buf, 0x0000, &samples);
        let (g0_bit8, u4_bit8) = capture(&sock, &mut buf, ECMODE_BIT8, &samples);
        let g0_delta = g0_bit8 as i32 - g0_base as i32;
        let u4_delta = u4_bit8 as i32 - u4_base as i32;
        println!("{amp:10.1}  {g0_base:8}  {g0_bit8:8}  {u4_base:8}  {u4_bit8:8}  {g0_delta:10}  {u4_delta:10}");
    }
    // Reset to a known-clean state.
    capture(&sock, &mut buf, 0x0000, &sawtooth(200.0, 100.0));
}
