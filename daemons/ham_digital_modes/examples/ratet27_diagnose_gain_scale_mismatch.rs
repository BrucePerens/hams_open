// SPDX-License-Identifier: LGPL-3.0-or-later
//! Follow-up to `ambe_chip_pcm_vs_float_synthesis_ratet27.rs`'s post-demodulation-fix residual gap
//! (whole-buffer raw-sample correlation ~0.10, envelope correlation ~0.63, frame-to-frame RMS ratio
//! swinging both louder and quieter than the chip -- not a constant gain factor).
//!
//! **Result: `l_hat`/`b0` correct; `b2`'s weak correlation here turned out to be a clamp artifact,
//! not a layout bug -- see `ratet27_bit_flip_semantic_probe.rs`'s own doc comment for the full,
//! corrected story.** `l_hat` is stable and physically plausible across real voiced speech (e.g. 25
//! consecutive frames in 30-41 on one speaker), so `b0`'s own within-codeword bit order (`u0` bits
//! 11:6 and `u7` bits 2:1, per `bit_prioritization::extract_fundamental_frequency_quantizer`) is
//! correct. But `b2` (the 6-bit log-gain index) barely correlates with real chip loudness at all
//! (`0.18` Pearson on loud frames, `0.07` on moderate frames), and neither does `u0`'s own bits 5:3 --
//! the exact position TIA Fig. 22's `prioritize_bits` puts `b2`'s top 3 bits, right after `b0`'s.
//! **This first looked like evidence that DVSI's real chip uses a different bit-prioritization scheme
//! than TIA for everything past the fundamental frequency -- that conclusion was wrong.** The
//! bit-flip probe (`ratet27_bit_flip_semantic_probe.rs`), corrected to target a *moderate* rather
//! than loud frame (Eq. 115/116's own amplitude-smoothing clamp fires readily on loud frames and
//! decorrelates `b2` from RMS regardless of layout correctness -- the real mechanism behind the weak
//! correlation above), found the real chip's measured response matches this crate's own TIA-layout
//! prediction in sign on 5 of 6 bits, two in both sign and rough magnitude. `b2`'s layout is very
//! likely correct after all. Also checked and clean: `GAIN_QUANTIZER_LEVELS` (Annex E), and (in
//! `tables::tests::gain_and_higher_order_bit_allocation_match_annex_f_g_for_recurring_l_values`)
//! `GAIN_BIT_ALLOCATION`/`HIGHER_ORDER_BIT_ALLOCATION` (Annex F/G) for every `L` this session's real
//! speech sample actually exercised. `ratet27_compare_textbook_u_vectors_to_chip.rs`'s earlier
//! pure-tone `u0=g0` mismatch is also best read as a test-material confound (a pure sine tone is bad
//! input for a voice-tuned chip pitch tracker), not evidence against `u0`. **Net conclusion**: this
//! crate's bit layout and Annex F/G tables check out against the real chip; the residual chip/float
//! PCM gap (envelope correlation `~0.63`, not `1.0`) is most plausibly genuine inter-decoder variance
//! (phase dither, noise-generator differences, edge-case handling) rather than a further bug here.
//!
//! Usage: `cargo run --release --example ratet27_diagnose_gain_scale_mismatch -- <host:port>`

use ham_digital_modes::ambe::float::tia_102_baba::decode::{DecoderState, FrameOutcome};
use ham_digital_modes::ambe::float::tia_102_baba::enhancement::energy;
use ham_digital_modes::ambe::dvsi_p25fec::wire_format::{block_wire_members, Block};
use ham_digital_modes::ambe::general::fec::golay_decode;
use std::net::UdpSocket;
use std::time::Duration;

const FIELD_RATEP: u8 = 0x0A;
const TYPE_CONTROL: u8 = 0x00;
const TYPE_SPEECH: u8 = 0x02;
const TYPE_CHANNEL: u8 = 0x01;
const RATEP_P25_FEC: [u16; 6] = [0x0558, 0x086B, 0x1030, 0x0000, 0x0000, 0x0190];
const FRAME_SAMPLES: usize = 160;
const FRAME_BYTES: usize = 18;
const N_FRAMES: usize = 200;

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
fn wire_bytes_to_c(bytes: &[u8; FRAME_BYTES]) -> [u32; 8] {
    let mut wire_frame_bits = [false; 144];
    for (byte_idx, &byte) in bytes.iter().enumerate() {
        for b in 0..8 {
            wire_frame_bits[byte_idx * 8 + b] = (byte >> (7 - b)) & 1 == 1;
        }
    }
    let raw = |block: Block| -> u32 {
        let members = block_wire_members(block);
        let mut received: u32 = 0;
        for (offset, &wire) in members.iter().enumerate() {
            if wire_frame_bits[wire] {
                received |= 1 << (members.len() - 1 - offset);
            }
        }
        received
    };
    [
        raw(Block::Golay { index: 0 }),
        raw(Block::Golay { index: 1 }),
        raw(Block::Golay { index: 2 }),
        raw(Block::Golay { index: 3 }),
        raw(Block::Hamming { index: 0 }),
        raw(Block::Hamming { index: 1 }),
        raw(Block::Hamming { index: 2 }),
        raw(Block::Raw),
    ]
}

fn main() {
    let host = std::env::args().nth(1).unwrap_or_else(|| "192.168.10.189:2460".to_string());
    let sock = UdpSocket::bind("0.0.0.0:0").expect("bind local UDP socket");
    sock.connect(&host).unwrap_or_else(|e| panic!("connect to {host}: {e}"));
    sock.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 1024];

    sock.send(&build_control_ratep(RATEP_P25_FEC)).expect("send RATEP config");
    let n = sock.recv(&mut buf).expect("RATEP config response");
    parse_packet(&buf[..n]).expect("valid packet");

    let pcm = read_wav_mono_i16("tests/fixtures/osr_speech/OSR_us_000_0010_8k.wav");
    let n_frames = (pcm.len() / FRAME_SAMPLES).min(N_FRAMES);

    // Two independent decoders: one drives real decode_frame (PCM, for the chip/float RMS ratio),
    // the other calls decode_parameters directly to read b2/l_hat/voiced/R_M0 without re-deriving
    // them from decode_frame's own private state -- mirroring decode_frame's own history advance
    // exactly (advance_history) so both stay in lockstep frame to frame.
    let mut float_pcm_decoder = DecoderState::new_chip_wire();
    let mut float_param_decoder = DecoderState::new_chip_wire();

    let mut channel_payloads: Vec<Vec<u8>> = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let frame = &pcm[i * FRAME_SAMPLES..(i + 1) * FRAME_SAMPLES];
        let n = send_recv_retrying(&sock, &mut buf, &build_speech(frame));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_CHANNEL, "expected a CHANNEL response");
        channel_payloads.push(payload.to_vec());
    }

    struct Row {
        idx: usize,
        chip_rms: f64,
        float_rms: f64,
        ratio: f64,
        b2: u32,
        l_hat: u32,
        voiced_frac: f64,
        r_m0: f64,
        outcome: &'static str,
        header_bytes: Vec<u8>,
        u0_bits5_3: u32,
    }
    let mut rows: Vec<Row> = Vec::with_capacity(n_frames);

    for (i, channel_payload) in channel_payloads.iter().enumerate() {
        let mut wire_bytes = [0u8; FRAME_BYTES];
        wire_bytes.copy_from_slice(&channel_payload[channel_payload.len() - FRAME_BYTES..]);
        let c = wire_bytes_to_c(&wire_bytes);

        let n = send_recv_retrying(&sock, &mut buf, &build_channel(channel_payload));
        let (ptype, payload) = parse_packet(&buf[..n]).expect("valid packet");
        assert_eq!(ptype, TYPE_SPEECH, "expected a SPEECH (decoded PCM) response");
        let chip_frame_pcm = parse_speech_payload(payload);
        let chip_rms = (chip_frame_pcm.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>()
            / FRAME_SAMPLES as f64)
            .sqrt();

        let float_frame_pcm = float_pcm_decoder.decode_frame(c);
        let float_rms = match &float_frame_pcm {
            Some(pcm) => {
                (pcm.iter().map(|&s| s * s).sum::<f64>() / FRAME_SAMPLES as f64).sqrt()
            }
            None => 0.0,
        };
        let ratio = if chip_rms > 1.0 { float_rms / chip_rms } else { f64::NAN };

        // Per `prioritize_bits` (step 2): b2's own top 3 bits live at u0's bits 5..3, right after
        // b0's top 6 bits (11..6). If the TIA Fig. 22 layout holds for u0, this alone should show a
        // real step-function correlation with chip loudness -- the decisive, 5-minute discriminator
        // between "a narrow bug downstream of u0" and "DVSI's real prioritization differs from TIA's".
        let (u0, _) = golay_decode(c[0]);
        let u0_bits5_3 = (u0 as u32 >> 3) & 0b111;

        let (b2, l_hat, voiced_frac, r_m0, outcome) = match float_param_decoder.decode_parameters(c)
        {
            Some(FrameOutcome::Decoded(params)) => {
                let voiced_frac = if params.voiced.is_empty() {
                    0.0
                } else {
                    params.voiced.iter().filter(|&&v| v).count() as f64 / params.voiced.len() as f64
                };
                let r_m0 = energy(&params.reconstructed_amplitudes);
                let b2 = params.bits.b2;
                let l_hat = params.l_hat;
                float_param_decoder.advance_history(&params);
                (b2, l_hat, voiced_frac, r_m0, "Decoded")
            }
            Some(FrameOutcome::Repeat) => (0, 0, 0.0, 0.0, "Repeat"),
            Some(FrameOutcome::Mute) => (0, 0, 0.0, 0.0, "Mute"),
            None => (0, 0, 0.0, 0.0, "None"),
        };

        let header_bytes = channel_payload[..channel_payload.len() - FRAME_BYTES].to_vec();
        rows.push(Row {
            idx: i,
            chip_rms,
            float_rms,
            ratio,
            b2,
            l_hat,
            voiced_frac,
            r_m0,
            outcome,
            header_bytes,
            u0_bits5_3,
        });
    }

    println!(
        "{:>4} {:>10} {:>10} {:>8} {:>4} {:>6} {:>6} {:>12} {:>8}",
        "idx", "chip_rms", "float_rms", "ratio", "b2", "l_hat", "voi%", "R_M0", "outcome"
    );
    for r in &rows {
        println!(
            "{:>4} {:>10.1} {:>10.1} {:>8.2} {:>4} {:>6} {:>6.0} {:>12.1} {:>8}",
            r.idx,
            r.chip_rms,
            r.float_rms,
            r.ratio,
            r.b2,
            r.l_hat,
            r.voiced_frac * 100.0,
            r.r_m0,
            r.outcome
        );
    }

    // Group into "float much louder" (ratio > 2) vs "float much quieter" (ratio < 0.7, nonzero chip
    // RMS) among Decoded frames only, and compare each group's own mean b2/l_hat/voiced_frac/R_M0 --
    // the advisor's own discriminating test: if a field's mean flips between groups in a way that
    // tracks the ratio flip, that field is where the residual gap lives.
    let decoded: Vec<&Row> = rows.iter().filter(|r| r.outcome == "Decoded" && r.ratio.is_finite()).collect();
    let louder: Vec<&&Row> = decoded.iter().filter(|r| r.ratio > 2.0).collect();
    let quieter: Vec<&&Row> = decoded.iter().filter(|r| r.ratio < 0.7).collect();
    let mean = |xs: &[&&Row], f: fn(&Row) -> f64| -> f64 {
        if xs.is_empty() {
            return f64::NAN;
        }
        xs.iter().map(|r| f(r)).sum::<f64>() / xs.len() as f64
    };
    println!(
        "\n-- Float-louder frames (ratio>2, n={}): mean b2={:.1} l_hat={:.1} voi%={:.1} R_M0={:.1}",
        louder.len(),
        mean(&louder, |r| r.b2 as f64),
        mean(&louder, |r| r.l_hat as f64),
        mean(&louder, |r| r.voiced_frac * 100.0),
        mean(&louder, |r| r.r_m0),
    );
    println!(
        "-- Float-quieter frames (ratio<0.7, n={}): mean b2={:.1} l_hat={:.1} voi%={:.1} R_M0={:.1}",
        quieter.len(),
        mean(&quieter, |r| r.b2 as f64),
        mean(&quieter, |r| r.l_hat as f64),
        mean(&quieter, |r| r.voiced_frac * 100.0),
        mean(&quieter, |r| r.r_m0),
    );
    println!(
        "-- All decoded frames (n={}): mean b2={:.1} l_hat={:.1} voi%={:.1} R_M0={:.1}",
        decoded.len(),
        mean(&decoded.iter().collect::<Vec<_>>(), |r| r.b2 as f64),
        mean(&decoded.iter().collect::<Vec<_>>(), |r| r.l_hat as f64),
        mean(&decoded.iter().collect::<Vec<_>>(), |r| r.voiced_frac * 100.0),
        mean(&decoded.iter().collect::<Vec<_>>(), |r| r.r_m0),
    );

    // Does b2 (the 6-bit log-gain index) actually track chip energy at all? Pearson correlation of
    // b2 vs log2(chip_rms), restricted to real voiced/loud frames (chip_rms > 500) so near-silent
    // frames (where b2 is nearly meaningless either way) don't dilute it.
    fn pearson(xs: &[f64], ys: &[f64]) -> f64 {
        let n = xs.len() as f64;
        let mean_x = xs.iter().sum::<f64>() / n;
        let mean_y = ys.iter().sum::<f64>() / n;
        let mut cov = 0.0;
        let mut var_x = 0.0;
        let mut var_y = 0.0;
        for i in 0..xs.len() {
            let dx = xs[i] - mean_x;
            let dy = ys[i] - mean_y;
            cov += dx * dy;
            var_x += dx * dx;
            var_y += dy * dy;
        }
        cov / (var_x.sqrt() * var_y.sqrt())
    }
    let loud_decoded: Vec<&&Row> = decoded.iter().filter(|r| r.chip_rms > 500.0).collect();
    if loud_decoded.len() > 3 {
        let xs: Vec<f64> = loud_decoded.iter().map(|r| r.b2 as f64).collect();
        let ys: Vec<f64> = loud_decoded.iter().map(|r| r.chip_rms.log2()).collect();
        println!(
            "\n-- b2 vs log2(chip_rms) correlation on {} loud frames (chip_rms>500): {:.4}",
            loud_decoded.len(),
            pearson(&xs, &ys)
        );
        let xs_u0: Vec<f64> = loud_decoded.iter().map(|r| r.u0_bits5_3 as f64).collect();
        println!(
            "-- u0 bits[5:3] (TIA's own b2-top-3-bits position) vs log2(chip_rms) correlation on {} \
             loud frames: {:.4}",
            loud_decoded.len(),
            pearson(&xs_u0, &ys)
        );
        let b2_min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let b2_max = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        println!("-- b2 range on loud (chip_rms>500) frames: {b2_min}..={b2_max} (of 0..=63 possible)");
    }
    // The advisor's alternative hypothesis for the weak b2/chip_rms correlation above: Eq. 115/116's
    // own amplitude-smoothing clamp (tau_M resets to 20480 every clean frame, verified correct
    // against the spec) compresses b2's real range on loud frames, on *both* the chip's encoder and
    // this crate's decoder alike -- so correlating b2 against chip_rms on *moderate* frames, where
    // the clamp isn't active, should recover a real correlation if TIA's layout is actually correct.
    let moderate_decoded: Vec<&&Row> =
        decoded.iter().filter(|r| r.chip_rms > 200.0 && r.chip_rms < 800.0).collect();
    if moderate_decoded.len() > 3 {
        let xs: Vec<f64> = moderate_decoded.iter().map(|r| r.b2 as f64).collect();
        let ys: Vec<f64> = moderate_decoded.iter().map(|r| r.chip_rms.log2()).collect();
        let b2_min = xs.iter().cloned().fold(f64::INFINITY, f64::min);
        let b2_max = xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        println!(
            "\n-- b2 vs log2(chip_rms) correlation on {} MODERATE frames (200<chip_rms<800, clamp \
             likely inactive): {:.4} (b2 range {b2_min}..={b2_max})",
            moderate_decoded.len(),
            pearson(&xs, &ys)
        );
    }

    // DTX/comfort-noise check: print the channel payload's own header bytes (everything before the
    // 18-byte wire frame) for a few loud vs quiet frames, to see whether the chip flags near-silent
    // frames distinctly (a real DTX indicator these near-zero-RMS stretches might carry, which would
    // mean they're not comparable via the same correlation metric as real speech at all).
    println!("\n-- Channel payload header bytes, loud vs quiet frames --");
    for r in rows.iter().filter(|r| r.chip_rms > 1000.0).take(5) {
        println!("frame {} (chip_rms={:.0}, LOUD): header={:02x?}", r.idx, r.chip_rms, r.header_bytes);
    }
    for r in rows.iter().filter(|r| r.chip_rms < 100.0 && r.chip_rms > 0.0).take(5) {
        println!("frame {} (chip_rms={:.0}, QUIET): header={:02x?}", r.idx, r.chip_rms, r.header_bytes);
    }
}
