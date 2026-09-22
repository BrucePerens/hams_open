// SPDX-License-Identifier: LGPL-3.0-or-later
//! Captures a fresh, independent copy of the chip's deterministic muted/comfort-noise table
//! (`docs/references/AMBE_CHIP_NOISE_GENERATOR.md`), for validating the 81-tap shaping filter that
//! was fitted on `tools/chip_noise/table_65536.i16` but never checked against a separate capture --
//! see `night_shift_todo/high/ambe-...-3c8f1d2e.md` (hams_com), "noise-shaping filter validation".
//!
//! Sends `PKT_INIT` with the decoder flag (synchronizes the noise generator, per the doc's finding
//! 1), then an invalid AMBE+2 pitch code (`b0 = 124`, one of the reserved codes 120..127 the chip
//! treats as "repeat 3 frames then near-silence") for `FRAMES` frames, discards the first few
//! (fade-in from the repeat-then-mute path, matching the original capture's own note), and folds
//! the rest modulo 65536 by simple position, writing a little-endian i16 table to the path given
//! (or `tools/chip_noise/table_65536_fresh.i16` by default) in the exact same format as the
//! original `table_65536.i16` (`numpy.fromfile(path, '<i2')`).
//!
//! Usage: `cargo run --release --features ambe_plus_2 --example ambe_chip_noise_table_capture -- <host:port> [out_path]`

use ham_digital_modes::ambe::float::ambe_plus_2::decode::RawParameters;
use ham_digital_modes::ambe::float::ambe_plus_2::encode::build_frame;
use ham_digital_modes::ambe::float::ambe_plus_2::interleave::frame_to_interleaved;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATET: u8 = 0x09;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_CHANNEL: u8 = 0x01;
const TYPE_SPEECH: u8 = 0x02;
const RATET_HALF_RATE_FEC: u8 = 33;
const FRAME_SAMPLES: usize = 160;
const TABLE_LEN: usize = 65536;
// Two full periods plus a fade-in margin, matching the original capture's own "about 820 frames" --
// 900 * 160 = 144,000 samples, comfortably past 2 * 65,536 = 131,072.
const FRAMES: usize = 900;
// Skip the first few frames' worth of samples: the repeated-frame fade-in before the decoder
// settles into steady muted output (the original capture's own "positions before the fourth frame
// differ" note).
const SKIP_FRAMES: usize = 4;

fn control(field: u8, body: &[u8]) -> Vec<u8> {
    let mut payload = vec![field];
    payload.extend_from_slice(body);
    let mut pkt = vec![0x61_u8];
    pkt.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    pkt.push(TYPE_CONTROL);
    pkt.extend_from_slice(&payload);
    pkt
}
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
    data.get(4..4 + length).map(|p| (data[3], p))
}
fn parse_speech_payload(payload: &[u8]) -> Vec<i16> {
    let count = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    payload[2..2 + count * 2]
        .chunks_exact(2)
        .map(|b| i16::from_be_bytes([b[0], b[1]]))
        .collect()
}
fn send_recv_retrying(sock: &UdpSocket, buf: &mut [u8; 1024], pkt: &[u8]) -> usize {
    for attempt in 0..8 {
        sock.send(pkt).expect("send");
        match sock.recv(buf) {
            Ok(n) => return n,
            Err(e) if attempt < 7 => {
                eprintln!("retrying after {e}");
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => panic!("recv after retries: {e}"),
        }
    }
    unreachable!()
}
fn frame_to_wire_bytes(frame: u128) -> [u8; 9] {
    let wire = frame_to_interleaved(frame);
    std::array::from_fn(|i| ((wire >> (8 * (8 - i))) & 0xFF) as u8)
}

fn main() {
    let host = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let out_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "tools/chip_noise/table_65536_fresh.i16".to_string());

    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];

    sock.send(&control(FIELD_RATET, &[RATET_HALF_RATE_FEC])).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    parse_packet(&buf[..n]).unwrap();
    // PKT_INIT (0x0B), decoder flag (bit 1 -> value 2): synchronizes the noise generator (finding 1
    // in AMBE_CHIP_NOISE_GENERATOR.md). Not configurable via env here -- this tool always wants the
    // deterministic path, unlike the exploratory probes it borrows its plumbing from.
    sock.send(&control(0x0B, &[0x02])).unwrap();
    let n = sock.recv(&mut buf).unwrap();
    println!("PKT_INIT response: {:02x?}", &buf[..n.min(12)]);
    sock.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    while sock.recv(&mut buf).is_ok() {}
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();

    // b0 = 124 is one of the reserved AMBE+2 pitch codes (120..=124 invalid, 125 also invalid,
    // 126 = tone, 127 = invalid on the chip per this codebase's own ErrorPolicy::ChipCompatible
    // notes) -- the chip repeats 3 frames then falls into near-silence/comfort noise, which is
    // exactly the muted path the original capture used.
    // Other fields match examples/ambe_chip_comfort_noise.rs's own established pattern for this
    // probe (a typical-looking voiced frame's worth of non-zero fields, only b0 invalid) -- an
    // earlier version of this tool zeroed them all and captured near-silence (std ~0.6, range
    // [-1,1]) instead of the documented ~3.05 RMS comfort noise, confirming those other fields
    // really do matter for which muted/fallback behavior the chip takes.
    let muted = RawParameters {
        b0: 124,
        b1: 16,
        b2: 12,
        b3: 200,
        b4: 60,
        b5: 6,
        b6: 6,
        b7: 6,
        b8: 3,
    };
    let frame = build_frame(&muted);
    let wire = frame_to_wire_bytes(frame);

    let mut samples: Vec<i16> = Vec::with_capacity(FRAMES * FRAME_SAMPLES);
    for i in 0..FRAMES {
        let mut payload = vec![0x01u8, 72];
        payload.extend_from_slice(&wire);
        let pcm = loop {
            let n = send_recv_retrying(&sock, &mut buf, &build_channel(&payload));
            if let Some((TYPE_SPEECH, p)) = parse_packet(&buf[..n]) {
                break parse_speech_payload(p);
            }
        };
        samples.extend_from_slice(&pcm);
        if i % 100 == 0 {
            eprintln!("captured frame {i}/{FRAMES}");
        }
    }

    let usable = &samples[SKIP_FRAMES * FRAME_SAMPLES..];
    println!(
        "captured {} frames ({} samples), {} usable after skipping the first {SKIP_FRAMES}",
        FRAMES,
        samples.len(),
        usable.len()
    );
    assert!(
        usable.len() >= TABLE_LEN,
        "need at least one full period ({TABLE_LEN} samples) after skipping fade-in; got {}",
        usable.len()
    );

    // Fold by position modulo 65536, keeping the first occurrence of each position -- the doc's own
    // note that "the chip's own repeats differ by 1 in about 5% of samples" means later periods are
    // only a consistency check, not a better estimate; keeping the first occurrence matches how
    // table_65536.i16 itself was built (single period, per the doc's own description).
    let mut table = vec![0i16; TABLE_LEN];
    table.copy_from_slice(&usable[..TABLE_LEN]);

    let bytes: Vec<u8> = table.iter().flat_map(|s| s.to_le_bytes()).collect();
    std::fs::write(&out_path, &bytes).unwrap_or_else(|e| panic!("write {out_path}: {e}"));
    println!("wrote {} bytes to {out_path}", bytes.len());

    // Cheap self-check before anyone relies on this file: if a second period was captured, confirm
    // it substantially agrees with the first (the ~5% per-sample jitter the doc already documents).
    if usable.len() >= 2 * TABLE_LEN {
        let second = &usable[TABLE_LEN..2 * TABLE_LEN];
        let disagreements = table
            .iter()
            .zip(second.iter())
            .filter(|(a, b)| a != b)
            .count();
        println!(
            "second-period consistency check: {disagreements}/{TABLE_LEN} samples differ ({:.1}%)",
            100.0 * disagreements as f64 / TABLE_LEN as f64
        );
    }
}
