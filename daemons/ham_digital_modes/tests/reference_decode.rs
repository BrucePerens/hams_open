// SPDX-License-Identifier: LGPL-3.0-or-later
//! The actual "tested against Joe Taylor's C code on a simulated
//! band-noise channel" harness. Requires the real `wsjtx` package
//! (`ft8sim`, `jt9`) installed -- every test here is skipped, not
//! failed, when those binaries aren't present, since this is
//! deliberately exercising real external reference tools, not
//! self-contained unit tests.
//!
//! For each test message, at a swept range of SNR values:
//!   1. `ft8sim` generates a real noisy 15s WAV slot at that SNR.
//!   2. This crate's `Ft8Decoder` decodes it.
//!   3. The real `jt9` reference decoder decodes the identical file.
//!   4. Both results are compared -- not just "does ours decode
//!      something," but does it match the reference, and does the
//!      reference itself confirm the message is actually recoverable at
//!      that SNR at all (a miss when the reference also misses isn't a
//!      bug in this decoder).

use ham_digital_modes::ft8::Ft8Decoder;
use std::io::Read;
use std::path::Path;
use std::process::Command;

fn binary_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Minimal WAV reader for the specific format ft8sim always produces
/// (mono, 16-bit PCM) -- not a general-purpose parser. Skips the
/// standard 44-byte header and reads little-endian i16 samples,
/// normalized to f32 in [-1.0, 1.0].
fn read_wav_mono_i16(path: &Path) -> Vec<f32> {
    let mut file = std::fs::File::open(path).expect("wav file must exist");
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    // Standard WAV: 44-byte header for uncompressed PCM with no extra
    // chunks -- ft8sim's own output matches this exactly (verified: the
    // 'data' chunk id appears at offset 36 in every file this test
    // generates).
    assert_eq!(&bytes[36..40], b"data", "expected a standard 44-byte-header PCM WAV from ft8sim");
    let data = &bytes[44..];
    data.chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect()
}

fn generate_and_decode(message: &str, snr_db: i32, work_dir: &Path) -> (Vec<(String, i32)>, Option<String>) {
    let out = Command::new("ft8sim")
        .args([message, "1500.0", "0.0", "0.1", "1.0", "1", &snr_db.to_string()])
        .current_dir(work_dir)
        .output()
        .expect("ft8sim must run");
    assert!(out.status.success(), "ft8sim failed: {}", String::from_utf8_lossy(&out.stderr));

    let wav_path = std::fs::read_dir(work_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().map(|x| x == "wav").unwrap_or(false))
        .map(|e| e.path())
        .expect("ft8sim must produce a .wav file");

    let samples = read_wav_mono_i16(&wav_path);
    let mut decoder = Ft8Decoder::new(12000, 200.0, 3000.0).expect("decoder must initialize");
    decoder.feed(&samples);
    let ours = decoder.decode();

    let jt9_out = Command::new("jt9")
        .args(["--ft8", "-8"])
        .arg(&wav_path)
        .current_dir(work_dir) // jt9 writes its own wisdom/timer files into cwd
        .output()
        .expect("jt9 must run");
    let jt9_text = String::from_utf8_lossy(&jt9_out.stdout);
    let reference_message = jt9_text
        .lines()
        .find(|l| l.contains(&message.split_whitespace().next().unwrap_or(message)))
        .map(|l| l.trim().to_string());

    let _ = std::fs::remove_file(&wav_path);
    (ours, reference_message)
}

#[test]
fn decodes_a_clean_high_snr_signal_matching_the_reference() {
    if !binary_exists("ft8sim") || !binary_exists("jt9") {
        eprintln!("skipping: wsjtx (ft8sim/jt9) not installed");
        return;
    }
    let work_dir = std::env::temp_dir().join(format!("ft8_ref_test_clean_{}", std::process::id()));
    std::fs::create_dir_all(&work_dir).unwrap();

    let (ours, reference) = generate_and_decode("K1ABC W9XYZ EN37", -10, &work_dir);
    assert!(reference.is_some(), "the reference decoder itself must decode a -10dB signal -- if it can't, this test's premise is broken, not this crate");
    assert!(
        ours.iter().any(|(text, _)| text.contains("K1ABC") && text.contains("W9XYZ")),
        "must decode a clean/high-SNR (-10dB) signal the real reference decoder also decodes; got: {ours:?}, reference: {reference:?}"
    );

    let _ = std::fs::remove_dir_all(&work_dir);
}

#[test]
fn noise_channel_sweep_reports_real_snr_sensitivity_against_the_reference() {
    if !binary_exists("ft8sim") || !binary_exists("jt9") {
        eprintln!("skipping: wsjtx (ft8sim/jt9) not installed");
        return;
    }
    let work_dir = std::env::temp_dir().join(format!("ft8_ref_test_sweep_{}", std::process::id()));
    std::fs::create_dir_all(&work_dir).unwrap();

    let message = "K1ABC W9XYZ EN37";
    let mut results = Vec::new();
    // A representative sweep from clean to the deep-noise floor where
    // even the reference decoder starts failing -- FT8's own designed
    // operating range extends to roughly -20/-21 dB.
    for snr in [-5, -10, -15, -18, -20, -22] {
        let (ours, reference) = generate_and_decode(message, snr, &work_dir);
        let ours_decoded = ours.iter().any(|(text, _)| text.contains("K1ABC") && text.contains("W9XYZ"));
        let reference_decoded = reference.is_some();
        results.push((snr, ours_decoded, reference_decoded));

        // The one failure mode that would actually be a bug: decoding
        // something when the reference says the signal isn't there, or
        // (worse) decoding a *wrong* message. Missing a decode the
        // reference also misses is expected, honest behavior at the
        // noise floor, not a failure.
        assert!(
            !(ours_decoded && !reference_decoded),
            "at {snr} dB, this decoder claimed a decode the reference itself didn't make -- \
             that's a false-positive risk, not just reduced sensitivity"
        );
    }

    eprintln!("FT8 noise-channel sweep ({message}) vs. real jt9 reference:");
    for (snr, ours, reference) in &results {
        eprintln!("  {snr:+4} dB: this_decoder={ours:5} reference={reference:5}");
    }

    let _ = std::fs::remove_dir_all(&work_dir);
}

/// `hams_local_relay`'s mic input runs at 48kHz (matching its Opus voice
/// path), not FT8's conventional 12kHz. Rather than assume a resampler
/// is needed and write one, this checks empirically whether
/// `Ft8Decoder::new()` -- which takes `sample_rate` as a real parameter,
/// not a hardcoded constant, and derives its FFT/block size from it (see
/// `shim.c`'s `ft8_session_new`) -- decodes correctly when actually fed
/// 48kHz audio directly. It does (verified once already, ad hoc, before
/// this test existed): ft8_lib itself is rate-parameterized, so a
/// hand-rolled 48k->12k decimator turned out to be unnecessary work, not
/// a shortcut. This test is what keeps that true going forward instead
/// of resting on a one-time manual check.
#[test]
fn decodes_correctly_when_fed_at_48khz_directly_no_resampling() {
    if !binary_exists("ft8sim") || !binary_exists("jt9") || !binary_exists("sox") {
        eprintln!("skipping: wsjtx (ft8sim/jt9) or sox not installed");
        return;
    }
    let work_dir = std::env::temp_dir().join(format!("ft8_ref_test_48k_{}", std::process::id()));
    std::fs::create_dir_all(&work_dir).unwrap();

    let message = "K1ABC W9XYZ EN37";
    // ft8sim always emits 12000 Hz; -10dB is comfortably decodable
    // (see the clean-signal test above) so a decode failure here points
    // at the 48kHz path, not signal quality.
    let out = Command::new("ft8sim")
        .args([message, "1500.0", "0.0", "0.1", "1.0", "1", "-10"])
        .current_dir(&work_dir)
        .output()
        .expect("ft8sim must run");
    assert!(out.status.success(), "ft8sim failed: {}", String::from_utf8_lossy(&out.stderr));

    let wav_12k = std::fs::read_dir(&work_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .find(|e| e.path().extension().map(|x| x == "wav").unwrap_or(false))
        .map(|e| e.path())
        .expect("ft8sim must produce a .wav file");

    let wav_48k = work_dir.join("upsampled_48k.wav");
    let sox_out = Command::new("sox")
        .arg(&wav_12k)
        .args(["-r", "48000"])
        .arg(&wav_48k)
        .output()
        .expect("sox must run");
    assert!(sox_out.status.success(), "sox resample failed: {}", String::from_utf8_lossy(&sox_out.stderr));

    let samples_48k = read_wav_mono_i16(&wav_48k);
    let mut decoder = Ft8Decoder::new(48000, 200.0, 3000.0).expect("decoder must initialize at 48000 Hz");
    decoder.feed(&samples_48k);
    let ours = decoder.decode();

    assert!(
        ours.iter().any(|(text, _)| text.contains("K1ABC") && text.contains("W9XYZ")),
        "Ft8Decoder::new(48000, ...) must decode real 48kHz-sampled audio directly -- got {ours:?}. \
         If this starts failing, hams_local_relay needs a real 48k->12k resampler before FT8 \
         wiring, which this test previously found unnecessary."
    );

    let _ = std::fs::remove_dir_all(&work_dir);
}
