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
}
