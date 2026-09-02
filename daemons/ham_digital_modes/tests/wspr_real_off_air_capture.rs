// SPDX-License-Identifier: LGPL-3.0-or-later
//! `WSPR_DECODE_IMPLEMENTATION_PLAN.md` step 6's real remaining half: not
//! synthetic AWGN on a clean signal (already closed by `wspr_sync.rs`'s
//! own `own_decoder_noise_ladder_matches_the_recorded_wsprd_reference_
//! boundary`), but a genuine off-air recording -- real frequency drift,
//! real non-Gaussian noise, and (since a real capture always contains
//! more than one transmission at once) a real test of the "adjacent
//! WSPR transmission in the same band is an untested false-sync risk"
//! gap that module's own doc comment carries forward as unaddressed.
//!
//! The fixture is the official WSJT-X project's own published WSPR
//! sample recording (`150426_0918.wav`, from
//! <https://sourceforge.net/projects/wsjt/files/samples/WSPR/>) -- the
//! same K1JT/WSJT-X project this codebase already treats as the
//! reference implementation for `wsprsim`/`wsprd`. Deliberately NOT
//! committed to this repo: it's a real third-party recording of real
//! amateur transmissions, published for exactly this kind of decoder
//! testing but without a stated redistribution license, and this crate
//! lives in the public `hams_open` repository -- redistributing it here
//! isn't this project's call to make unilaterally. Skipped, not failed,
//! when the file isn't present locally; download it yourself from the
//! URL above and place it at the path `fixture_path()` names to run
//! this for real.
//!
//! Ground truth recorded by running the real `wsprd` reference decoder
//! (Debian's `wsjtx` package) directly against this file, 2026-09-01:
//!
//! | audio freq (Hz) | SNR (dB) | call    | grid | power (dBm) |
//! |---|---|---|---|---|
//! | 1446 | -9  | ND6P   | DM04 | 30 |
//! | 1460 | -15 | W5BIT  | EL09 | 17 |
//! | 1465 | -23 | G8VDQ  | IO91 | 37 |
//! | 1489 | -6  | WD4LHT | EL89 | 30 |
//! | 1503 | -1  | NM7J   | DM26 | 30 |  <- strongest signal
//! | 1517 | -21 | KI7CI  | DM09 | 37 |
//! | 1530 | -18 | DJ6OL  | JO52 | 37 |
//! | 1587 | -11 | W3HH   | EL89 | 30 |
//! | 1594 | -25 | W3BI   | FN20 | 30 |
//!
//! This crate's own decoder is deliberately single-strongest-signal
//! scoped (`wspr_sync.rs`'s own "Scope" doc comment) -- it was never
//! designed to extract all nine like `wsprd` does. The real test this
//! fixture enables is narrower and more targeted: does it find *any*
//! one of these nine real messages correctly (ideally the strongest,
//! NM7J), or does it stay silent -- and, the one actual failure mode
//! that would matter, does the presence of eight *other* real
//! transmissions in the same band ever cause it to lock onto a false
//! sync and report a message that ISN'T one of these nine known-real
//! ones.

use ham_digital_modes::wspr_sync::{required_window_samples, sync_search_and_decode_message, MIN_SYNC_SCORE};
use ham_digital_modes::wspr_decode::MIN_ACCEPTABLE_METRIC;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Real ground truth from the real `wsprd` reference decoder, recorded
/// in this file's own doc comment above -- kept here as data so the
/// test can check membership without re-deriving it.
const KNOWN_REAL_MESSAGES: &[(&str, &str, i32)] = &[
    ("ND6P", "DM04", 30),
    ("W5BIT", "EL09", 17),
    ("G8VDQ", "IO91", 37),
    ("WD4LHT", "EL89", 30),
    ("NM7J", "DM26", 30),
    ("KI7CI", "DM09", 37),
    ("DJ6OL", "JO52", 37),
    ("W3HH", "EL89", 30),
    ("W3BI", "FN20", 30),
];

fn fixture_path() -> PathBuf {
    // Documented, not committed -- see this file's own module doc
    // comment for why and where to get it.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wspr_real_off_air_150426_0918.wav")
}

/// Same 44-byte-header mono 16-bit PCM assumption `wspr.rs`'s and
/// `digital_decoder.rs`'s own `read_wav_mono_i16()` test helpers make
/// -- confirmed directly against this exact file (`file(1)`: "RIFF...
/// WAVE audio, Microsoft PCM, 16 bit, mono 12000 Hz").
fn read_wav_mono_i16(path: &Path) -> Vec<i16> {
    let mut file = std::fs::File::open(path).expect("wav fixture must exist");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(&bytes[36..40], b"data", "expected a standard 44-byte-header PCM WAV");
    bytes[44..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect()
}

#[test]
fn decodes_a_real_off_air_capture_correctly_or_stays_silent_never_wrong() {
    let path = fixture_path();
    if !path.exists() {
        eprintln!(
            "skipping: real off-air WSPR fixture not present at {} -- see this file's own module doc comment for how to get it",
            path.display()
        );
        return;
    }

    let sample_rate = 12000u32;
    let samples = read_wav_mono_i16(&path);
    let max_start_sample = samples.len().saturating_sub(required_window_samples(sample_rate));

    let result = sync_search_and_decode_message(
        &samples,
        sample_rate,
        1400.0,
        1600.0,
        max_start_sample,
        MIN_SYNC_SCORE,
        2_000_000,
        MIN_ACCEPTABLE_METRIC,
    );

    match result {
        Some(Ok((callsign, grid, power, base_hz))) => {
            let is_known_real = KNOWN_REAL_MESSAGES
                .iter()
                .any(|&(c, g, p)| c == callsign && g == grid && p == power);
            assert!(
                is_known_real,
                "decoded {callsign} {grid} {power} -- not one of the 9 real messages the reference \
                 decoder (wsprd) found in this real capture. This is the actual failure mode that \
                 would matter: a confident WRONG decode caused by cross-talk between the 8 other \
                 real transmissions sharing this band, the exact 'adjacent transmission false-sync \
                 risk' wspr_sync.rs's own doc comment already carries forward as untested. A \
                 no-decode is fine; this specific outcome is not."
            );
            let is_strongest = callsign == "NM7J" && grid == "DM26" && power == 30;
            eprintln!(
                "real off-air capture: decoded {callsign} {grid} {power} at base_hz={base_hz:.1} \
                 (a real, known-correct message from this capture{}).",
                if is_strongest { ", the strongest of the 9" } else { "" }
            );
        }
        Some(Err(e)) => {
            eprintln!(
                "real off-air capture: no decode ({e:?}) -- honest, non-failing outcome. The \
                 reference decoder found 9 real signals here (strongest: NM7J at -1dB); this \
                 crate's own decoder is not guaranteed to match wsprd's sensitivity or its \
                 multi-signal extraction (it is deliberately single-strongest-signal scoped)."
            );
        }
        None => {
            eprintln!("real off-air capture: no sync found at all -- honest, non-failing outcome.");
        }
    }
}
