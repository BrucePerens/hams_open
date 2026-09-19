// SPDX-License-Identifier: LGPL-3.0-or-later
//! Section 21's own record leaves an explicit, named, untried lever for `g3`'s stubborn rank-8
//! plateau (2687 real captured frames of every audio stimulus tried all land in the same GF(2)
//! subspace): "an encoder feature/mode this investigation's SPEECH-packet-only testing never
//! engages (frame-repeat, DTX, or another documented `ECMODE`/`DCMODE` flag)." `DTX_ENABLE` (bit 11)
//! and `TD_ENABLE` (bit 12) were both tested directly and ruled out (section 24). Every *other*
//! `ECMODE_IN` bit (0-10, 13-15) has never been tried -- this project doesn't have DVSI's own manual
//! bit-name table stored locally, but that isn't needed to test the actual question empirically:
//! does setting each individual bit change the chip's real output at all, on the same fixed content
//! (`ECMODE_IN=0` as baseline)? This tool decodes every block including `g3` via this crate's own
//! real `g3_decode` (not an assumed-Golay stand-in, now that section 29 derived the real generator)
//! and reports which bits, if any, move anything -- not just `g3`.
//!
//! **Caveat for reuse**: each bit's 60-frame settling period runs immediately after the *previous*
//! bit's 8 capture frames of the same fixed stimulus, with no explicit reset of the chip's own
//! adaptive state (section 32) in between. This was fine for the actual run this tool produced --
//! the null result on 13 of 14 bits reflects ample settling at an unchanging stimulus, and bit 8's
//! large, immediately-reversible effect (bit 9 promptly returned every block to baseline) isn't an
//! artifact of carried-over state -- but a future stimulus chosen near one of section 32's own
//! adaptive/contrast-sensitive boundaries could see cross-bit contamination. Re-settle explicitly
//! (e.g. re-run the baseline stimulus for a full settling period) before reusing this tool with a
//! stimulus that isn't simply "far from any known threshold," as this one was.
//!
//! Usage: `cargo run --release --example p25_ratet27_ecmode_bit_sweep -- <host:port>`
use ham_digital_modes::ambe::ratet27_fec::decode_block;
use ham_digital_modes::ambe::ratet27_wire_format::Block;
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
const TOTAL_BITS: usize = 144;
const ALREADY_TESTED_BITS: [u32; 2] = [11, 12]; // DTX_ENABLE, TD_ENABLE (section 24)

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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Blocks {
    g0: u16,
    g1: u16,
    g2: u16,
    g3: u8,
    u4: u16,
    u5: u16,
    u6: u16,
    c7: u16,
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

    let samples = sawtooth(200.0, 6000.0);

    let run_with_ecmode = |sock: &UdpSocket, buf: &mut [u8; 512], ecmode_in: u16| -> Vec<Blocks> {
        sock.send(&build_control_ecmode(ecmode_in)).expect("send ECMODE config");
        let n = sock.recv(buf).expect("ECMODE config response");
        parse_packet(&buf[..n]).expect("valid packet");
        for _ in 0..SETTLING_FRAMES {
            let n = send_recv_retrying(sock, buf, &build_speech(&samples));
            parse_packet(&buf[..n]).expect("valid packet");
        }
        let mut results = Vec::with_capacity(CAPTURE_FRAMES);
        for _ in 0..CAPTURE_FRAMES {
            let n = send_recv_retrying(sock, buf, &build_speech(&samples));
            let (ptype, _payload) = parse_packet(&buf[..n]).expect("valid packet");
            assert_eq!(ptype, TYPE_CHANNEL);
            let pkt = &buf[..n];
            let bits_bytes = &pkt[BITS_OFFSET..BITS_OFFSET + FRAME_BYTES];
            let mut wire_frame_bits = [false; TOTAL_BITS];
            for (byte_idx, &byte) in bits_bytes.iter().enumerate() {
                for bit_idx in 0..8 {
                    wire_frame_bits[byte_idx * 8 + bit_idx] = (byte >> (7 - bit_idx)) & 1 == 1;
                }
            }
            let (g0, _) = decode_block(&wire_frame_bits, Block::Golay { index: 0 });
            let (g1, _) = decode_block(&wire_frame_bits, Block::Golay { index: 1 });
            let (g2, _) = decode_block(&wire_frame_bits, Block::Golay { index: 2 });
            let (g3, _) = decode_block(&wire_frame_bits, Block::Golay { index: 3 });
            let (u4, _) = decode_block(&wire_frame_bits, Block::Hamming { index: 0 });
            let (u5, _) = decode_block(&wire_frame_bits, Block::Hamming { index: 1 });
            let (u6, _) = decode_block(&wire_frame_bits, Block::Hamming { index: 2 });
            let (c7, _) = decode_block(&wire_frame_bits, Block::Raw);
            results.push(Blocks { g0, g1, g2, g3: g3 as u8, u4, u5, u6, c7 });
        }
        results
    };

    println!("-- Baseline: ECMODE_IN=0x0000 --");
    let baseline = run_with_ecmode(&sock, &mut buf, 0x0000);
    for (i, b) in baseline.iter().enumerate() {
        println!("  frame {i}: {b:?}");
    }

    for bit in 0..16u32 {
        if ALREADY_TESTED_BITS.contains(&bit) {
            println!("\n-- Bit {bit}: already tested (DTX_ENABLE/TD_ENABLE), skipping --");
            continue;
        }
        let ecmode_in = 1u16 << bit;
        println!("\n-- Bit {bit}: ECMODE_IN=0x{ecmode_in:04x} --");
        let result = run_with_ecmode(&sock, &mut buf, ecmode_in);
        for (i, b) in result.iter().enumerate() {
            println!("  frame {i}: {b:?}");
        }
        let differs_from_baseline = result != baseline;
        println!(
            "  bit {bit}: {}",
            if differs_from_baseline {
                "*** DIFFERS FROM BASELINE ***"
            } else {
                "identical to baseline"
            }
        );
    }
    // Reset to a known-clean state for whoever/whatever uses the chip next.
    run_with_ecmode(&sock, &mut buf, 0x0000);
}
