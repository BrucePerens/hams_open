// SPDX-License-Identifier: LGPL-3.0-or-later
//! FT8 decode, built on vendored ft8_lib (Kārlis Goba, MIT-licensed --
//! see vendor/ft8_lib/LICENSE-MIT) via a thin C shim (vendor/ft8_lib/
//! shim.c) rather than a from-scratch reimplementation of its LDPC/
//! Costas-sync decode algorithms. Real, tested end to end against the
//! actual K1JT reference tools this session installed (`wsjtx` 2.7.0):
//! a noisy signal generated with `ft8sim` at a real SNR, decoded
//! correctly by both `jt9` (the reference) and this module. See
//! `tests/reference_decode.rs` for the harness that proves it, run
//! against a swept range of SNR values, not just one clean case.
//!
//! FT8 is a slotted protocol (~15s transmit windows): feed accumulates
//! audio into ft8_lib's internal waterfall; call `decode()` once per
//! slot boundary (the caller's job to track, same as any FT8 client),
//! then `reset()` before the next slot.
//!
//! `encode_message()` covers message-packing (77-bit payload) and tone
//! generation (the discrete 0..7 tone-index sequence a real FT8
//! transmission consists of) -- both are the vendored `ft8_lib`'s own
//! real code (`ftx_message_encode`/`ft8_encode`), not reimplemented
//! here, and verified against `ft8sim`'s own printed tone sequence (see
//! `tests/reference_decode.rs`).
//!
//! `synthesize_waveform()` turns that tone sequence into an actual
//! transmittable audio waveform: continuous-phase GFSK synthesis with
//! the real Gaussian pulse shape (BT=2.0) and raised-cosine start/end
//! amplitude ramp, per the protocol's own published spec -- see
//! `docs/references/FT8_FT4_GFSK_WAVEFORM_SPEC.md` (Franke/Somerville/
//! Taylor, "The FT4 and FT8 Communication Protocols," QEX, July/August
//! 2020) for the exact equations and why this was deliberately left
//! unbuilt until that primary source was available. Verified by real
//! round-trip: synthesizing a known message and feeding the result back
//! into this module's own already-reference-verified `Ft8Decoder`
//! recovers the original message (see the tests below).

mod ffi;

pub struct Ft8Decoder {
    session: *mut ffi::Ft8Session,
    pending: Vec<f32>,
    block_size: usize,
}

// The underlying C session holds no thread-local or global state of its
// own (aside from the ftx_lib callsign hashtable documented in shim.c,
// which is a known, deliberate process-wide limitation, not a
// thread-safety issue for a single session's own decode calls) -- Send
// is safe as long as one session is only ever used from one thread at a
// time, which the &mut self API already enforces.
unsafe impl Send for Ft8Decoder {}

impl Ft8Decoder {
    /// `sample_rate` must match the actual input audio (12000 Hz is
    /// FT8's own standard rate; ft8_lib itself does not resample).
    /// `f_min`/`f_max` bound the analysis frequency range in Hz within
    /// the audio passband -- 200.0/3000.0 matches WSJT-X's own default
    /// FT8 analysis window.
    pub fn new(sample_rate: u32, f_min: f32, f_max: f32) -> Option<Self> {
        let session = unsafe { ffi::ft8_session_new(sample_rate as i32, f_min, f_max) };
        if session.is_null() {
            return None;
        }
        let block_size = unsafe { ffi::ft8_session_block_size(session) } as usize;
        Some(Ft8Decoder {
            session,
            pending: Vec::with_capacity(block_size),
            block_size,
        })
    }

    /// Accumulates audio samples, feeding one analysis block at a time
    /// into ft8_lib's waterfall as enough samples arrive. Does not
    /// itself attempt to decode -- call `decode()` at the end of a
    /// ~15s FT8 slot.
    pub fn feed(&mut self, samples: &[f32]) {
        self.pending.extend_from_slice(samples);
        let mut offset = 0;
        while self.pending.len() - offset >= self.block_size {
            let block = &self.pending[offset..offset + self.block_size];
            unsafe {
                ffi::ft8_session_process(self.session, block.as_ptr());
            }
            offset += self.block_size;
        }
        self.pending.drain(0..offset);
    }

    /// Attempts to decode every candidate found in the waterfall
    /// accumulated so far. Returns (message_text, snr_estimate) pairs.
    /// Does not clear the waterfall -- call `reset()` before the next
    /// slot's audio.
    pub fn decode(&mut self) -> Vec<(String, i32)> {
        const MAX_MESSAGES: usize = 50;
        let mut out_messages = [[0 as std::os::raw::c_char; ffi::FTX_MAX_MESSAGE_LENGTH]; MAX_MESSAGES];
        let mut out_snr = [0i32; MAX_MESSAGES];

        let count = unsafe {
            ffi::ft8_session_decode(
                self.session,
                out_messages.as_mut_ptr(),
                out_snr.as_mut_ptr(),
                MAX_MESSAGES as i32,
            )
        };

        (0..count as usize)
            .map(|i| {
                let cstr = unsafe { std::ffi::CStr::from_ptr(out_messages[i].as_ptr()) };
                (cstr.to_string_lossy().trim().to_string(), out_snr[i])
            })
            .collect()
    }

    /// Clears the accumulated waterfall for the next FT8 slot.
    pub fn reset(&mut self) {
        self.pending.clear();
        unsafe {
            ffi::ft8_session_reset(self.session);
        }
    }
}

/// Packs `text` into an FT8 message and generates its real 79-symbol
/// tone sequence (each element a tone index 0..7), via the vendored
/// `ft8_lib`'s own `ftx_message_encode`/`ft8_encode` -- not
/// reimplemented. Returns `None` if `text` can't be packed as a valid
/// FT8 message (see `ftx_message_rc_t` in message.h -- e.g. an invalid
/// callsign). Does not produce audio; pass the result to
/// `synthesize_waveform()` for that.
pub fn encode_message(text: &str) -> Option<[u8; ffi::FT8_NN]> {
    let c_text = std::ffi::CString::new(text).ok()?;
    let mut tones = [0u8; ffi::FT8_NN];
    let result = unsafe { ffi::ft8_encode_message(c_text.as_ptr(), tones.as_mut_ptr()) };
    if result != ffi::FT8_NN as i32 {
        return None;
    }
    Some(tones)
}

/// FT8 symbol/signaling interval, seconds -- docs/references/
/// FT8_FT4_GFSK_WAVEFORM_SPEC.md, Table 4.
const FT8_SYMBOL_PERIOD_S: f64 = 0.160;
/// GFSK smoothing filter's bandwidth-time product for FT8 (BT=2.0;
/// FT4 uses BT=1.0, not implemented here). Same source, Table 4.
const FT8_BT: f64 = 2.0;
/// Modulation index h -- same source, Table 4. FT8 and FT4 both use 1.
const FT8_MODULATION_INDEX: f64 = 1.0;

/// The GFSK Gaussian-smoothed frequency-deviation pulse shape, normalized
/// to unit area. `t` and the returned deviation are both in units of the
/// symbol period (i.e. call with `t / FT8_SYMBOL_PERIOD_S`, and the
/// result already has the `1/(2T)` normalization baked in via the
/// `symbol_period_s` parameter). Equation 3 in
/// docs/references/FT8_FT4_GFSK_WAVEFORM_SPEC.md:
///   p(t) = (1/2T) * [erf(kBT(t/T + 0.5)) - erf(kBT(t/T - 0.5))]
/// with k = pi*sqrt(2/ln2).
fn gfsk_pulse(t_over_symbol_period: f64, symbol_period_s: f64) -> f64 {
    // k = pi * sqrt(2 / ln 2) = 5.336... (matches the paper's own stated
    // value exactly -- computed directly, not a hardcoded literal, so it
    // can't silently drift from a transcription error).
    let k = std::f64::consts::PI * (2.0 / std::f64::consts::LN_2).sqrt();
    let kbt = k * FT8_BT;
    let a = libm::erf(kbt * (t_over_symbol_period + 0.5));
    let b = libm::erf(kbt * (t_over_symbol_period - 0.5));
    (a - b) / (2.0 * symbol_period_s)
}

/// Synthesizes a real, transmittable FT8 audio waveform from a 79-symbol
/// tone sequence (as produced by `encode_message()`), at `sample_rate`
/// Hz, with the FT8 tone-space carrier centered at `base_freq_hz`
/// (WSJT-X's own convention places the lowest tone near the low edge of
/// the audio passband, commonly ~1000-1500 Hz -- the caller picks the
/// actual dial-frequency offset). Returns real f32 PCM samples, roughly
/// 79 * 0.160s = 12.64s worth (plus the ramp, negligible).
///
/// Implements docs/references/FT8_FT4_GFSK_WAVEFORM_SPEC.md directly:
/// continuous-phase FSK (`s(t) = A*cos(2*pi*f_c*t + phi(t))`), frequency
/// deviation as the superposition of one Gaussian-smoothed pulse per
/// symbol (`f_d(t) = h * sum(b_n * p(t - nT))`), phase as the running
/// integral of that deviation, and a raised-cosine amplitude ramp over
/// the first/last T/8 = 20ms so the signal doesn't key on/off with a
/// hard edge.
pub fn synthesize_waveform(tones: &[u8; ffi::FT8_NN], sample_rate: u32, base_freq_hz: f32) -> Vec<f32> {
    let t_sym = FT8_SYMBOL_PERIOD_S;
    let n_symbols = ffi::FT8_NN;
    let total_duration_s = n_symbols as f64 * t_sym;
    let n_samples = (total_duration_s * sample_rate as f64).round() as usize;
    let dt = 1.0 / sample_rate as f64;
    let ramp_duration_s = t_sym / 8.0; // 20ms for FT8

    let mut samples = Vec::with_capacity(n_samples);
    let mut phase = 0.0f64; // running integral of 2*pi*f_d(t), i.e. phi(t)

    for i in 0..n_samples {
        let t = i as f64 * dt;

        // f_d(t) = h * sum_n( b_n * p(t - n*T) ), summed only over the
        // symbols whose pulse actually reaches this sample -- the paper's
        // own note that only the nearest symbol and its immediate
        // neighbors contribute meaningfully (a 3-symbol-wide window).
        let center_symbol = (t / t_sym).floor() as i64;
        let mut f_d = 0.0f64;
        for n in (center_symbol - 1)..=(center_symbol + 1) {
            if n < 0 || n as usize >= n_symbols {
                continue;
            }
            let symbol_center_t = n as f64 * t_sym;
            let t_over_period = (t - symbol_center_t) / t_sym;
            // The pulse's own support is effectively [-1.5T, 1.5T];
            // outside that erf(x)-erf(x) is numerically ~0 anyway, but
            // skip the libm calls for symbols clearly out of range.
            if t_over_period.abs() > 1.5 {
                continue;
            }
            let b_n = tones[n as usize] as f64;
            f_d += b_n * gfsk_pulse(t_over_period, t_sym);
        }
        f_d *= FT8_MODULATION_INDEX;

        // phi(t) = 2*pi * integral_0^t f_d(tau) d(tau) -- rectangular
        // integration; dt is ~1/48000s, tiny relative to the pulse's own
        // timescale (T=0.16s), so accumulated error is negligible.
        phase += 2.0 * std::f64::consts::PI * f_d * dt;

        // Raised-cosine amplitude ramp: 0->1 over the first 20ms,
        // constant 1 in the middle, 1->0 over the last 20ms. Equation
        // A(t) = 0.5*(1 - cos(8*pi*t/T)), 0 <= t <= T/8, applied at both
        // ends (time-reversed at the end).
        let time_from_end = total_duration_s - t;
        let amplitude = if t < ramp_duration_s {
            0.5 * (1.0 - (8.0 * std::f64::consts::PI * t / t_sym).cos())
        } else if time_from_end < ramp_duration_s {
            0.5 * (1.0 - (8.0 * std::f64::consts::PI * time_from_end / t_sym).cos())
        } else {
            1.0
        };

        let carrier_phase = 2.0 * std::f64::consts::PI * base_freq_hz as f64 * t + phase;
        samples.push((amplitude * carrier_phase.cos()) as f32);
    }

    samples
}

impl Drop for Ft8Decoder {
    fn drop(&mut self) {
        unsafe {
            ffi::ft8_session_free(self.session);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_drop_do_not_crash() {
        let decoder = Ft8Decoder::new(12000, 200.0, 3000.0);
        assert!(decoder.is_some());
    }

    #[test]
    fn decoding_silence_yields_no_messages() {
        let mut decoder = Ft8Decoder::new(12000, 200.0, 3000.0).unwrap();
        // Feed a full ~15s slot of silence -- 12000 Hz * 15s.
        decoder.feed(&vec![0.0f32; 12000 * 15]);
        let messages = decoder.decode();
        assert!(messages.is_empty(), "silence must never produce a phantom decode");
    }

    /// The real correctness proof for synthesize_waveform(): if the GFSK
    /// pulse shape, symbol timing, or phase integration were wrong, this
    /// decoder (already reference-verified against the real jt9 decoder
    /// in tests/reference_decode.rs) would not lock onto the Costas sync
    /// pattern and would not recover the original message -- a much
    /// stronger check than "the math matches the paper's equations on
    /// paper," which this test makes unnecessary to trust blindly.
    #[test]
    fn synthesized_waveform_round_trips_through_the_real_decoder_at_12khz() {
        let message = "K1ABC W9XYZ EN37";
        let tones = encode_message(message).expect("a valid standard-format message must encode");

        let samples = synthesize_waveform(&tones, 12000, 1500.0);
        // 79 symbols * 0.160s = 12.64s, at 12000 Hz.
        assert_eq!(samples.len(), (79.0_f64 * 0.160 * 12000.0).round() as usize);
        assert!(
            samples.iter().any(|&s| s.abs() > 0.01),
            "synthesized waveform must not be silence"
        );
        assert!(
            samples.iter().all(|&s| s.abs() <= 1.0 + 1e-6),
            "constant-envelope signal must never exceed unit amplitude"
        );

        let mut decoder = Ft8Decoder::new(12000, 200.0, 3000.0).unwrap();
        decoder.feed(&samples);
        let decoded = decoder.decode();
        assert!(
            decoded.iter().any(|(text, _)| text.contains("K1ABC") && text.contains("W9XYZ")),
            "expected to decode '{message}' back out of its own synthesized waveform, got {decoded:?}"
        );
    }

    /// Same round trip at 48kHz (hams_local_relay's own mic/speaker
    /// rate, not FT8's conventional 12kHz) -- confirms synthesis, not
    /// just decode, is correct at this daemon's real operating rate.
    #[test]
    fn synthesized_waveform_round_trips_through_the_real_decoder_at_48khz() {
        let message = "CQ K6BP CM87";
        let tones = encode_message(message).expect("a valid standard-format message must encode");

        let samples = synthesize_waveform(&tones, 48000, 1500.0);
        let mut decoder = Ft8Decoder::new(48000, 200.0, 3000.0).unwrap();
        decoder.feed(&samples);
        let decoded = decoder.decode();
        assert!(
            decoded.iter().any(|(text, _)| text.contains("K6BP") && text.contains("CM87")),
            "expected to decode '{message}' back out of its own synthesized waveform at 48kHz, got {decoded:?}"
        );
    }

    /// The amplitude ramp is the one part of the spec that ISN'T tested
    /// by the round trip above (the decoder doesn't care about clean
    /// keying edges) -- verify it directly against the paper's own
    /// equation: A(0)=0, A(T/8)=1 (ramp-up complete), and the signal
    /// envelope actually starts near zero rather than jumping to full
    /// amplitude instantly.
    #[test]
    fn synthesis_ramps_up_smoothly_instead_of_keying_on_instantly() {
        let tones = encode_message("K1ABC W9XYZ EN37").unwrap();
        let sample_rate = 12000u32;
        let samples = synthesize_waveform(&tones, sample_rate, 1500.0);

        // First sample (t=0): A(t) = 0.5*(1-cos(0)) = 0.
        assert!(samples[0].abs() < 1e-6, "signal must start at exactly zero amplitude, got {}", samples[0]);

        // A few samples in (still well inside the 20ms ramp), amplitude
        // should be small but the carrier is already oscillating -- not
        // a hard step to full scale.
        let one_ms_in = sample_rate as usize / 1000;
        assert!(
            samples[one_ms_in].abs() < 0.5,
            "amplitude 1ms into a 20ms ramp should still be well under full scale, got {}",
            samples[one_ms_in]
        );

        // Well past the 20ms ramp (at symbol 2, comfortably inside
        // steady-state), the envelope should be reaching full scale
        // somewhere in a local neighborhood (checked via max, since cos()
        // itself passes through zero regardless of envelope).
        let mid_start = (2.5 * 0.160 * sample_rate as f64) as usize;
        let mid_window = &samples[mid_start..mid_start + sample_rate as usize / 10];
        let mid_peak = mid_window.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
        assert!(mid_peak > 0.9, "steady-state envelope should reach near-unit amplitude, got peak {mid_peak}");
    }
}
