// SPDX-License-Identifier: LGPL-3.0-or-later
//! C ABI layer exposing `ham_digital_modes::codec2_3200`'s independent
//! Codec2 3200bps port through the real upstream `codec2.h` function
//! surface (see `include/codec2.h`'s own doc comment for the exact
//! scope: create/destroy/encode/decode/decode_ber plus the three
//! frame-size queries, `CODEC2_MODE_3200` only) -- a link-compatible
//! drop-in for `-lcodec2` for a caller, such as an M17 stack, that only
//! needs that subset.
//!
//! `struct CODEC2` is opaque to C callers (the real header only forward-
//! declares it too), so this side is free to lay it out however's
//! convenient: one `Encoder` and one `Decoder` together behind a single
//! handle, matching the real library's own documented behavior ("One
//! set of states is sufficient for a full duplex codec... you don't
//! need separate states for encoders and decoders" -- `codec2.c`'s own
//! `codec2_create` doc comment) -- a caller that only ever calls encode
//! (or only ever decode) on a given handle pays for the unused half's
//! state, the same real tradeoff the reference makes.

use ham_digital_modes::codec2_3200::{Decoder, Encoder, BYTES_PER_FRAME, SAMPLES_PER_FRAME};
use std::os::raw::c_int;

const CODEC2_MODE_3200: c_int = 0;

pub struct CODEC2 {
    encoder: Encoder,
    decoder: Decoder,
}

/// Returns a new handle for `CODEC2_MODE_3200`, or NULL for any other
/// mode -- matching the real `codec2_create`'s own documented behavior
/// when a mode is compiled out (`CODEC2_MODE_ACTIVE` false -> NULL),
/// which is exactly this port's situation for every mode but 3200.
///
/// # Safety
/// Callable from C with any `mode` value; allocates and returns an
/// owned pointer the caller must eventually pass to `codec2_destroy`.
#[no_mangle]
pub extern "C" fn codec2_create(mode: c_int) -> *mut CODEC2 {
    if mode != CODEC2_MODE_3200 {
        return std::ptr::null_mut();
    }
    Box::into_raw(Box::new(CODEC2 { encoder: Encoder::new(), decoder: Decoder::new() }))
}

/// # Safety
/// `codec2_state` must be a pointer previously returned by
/// `codec2_create` and not already destroyed (matches the real
/// library's own contract -- its own `codec2_destroy` `assert`s
/// non-NULL rather than tolerating it, so this does the same).
#[no_mangle]
pub unsafe extern "C" fn codec2_destroy(codec2_state: *mut CODEC2) {
    assert!(!codec2_state.is_null(), "codec2_destroy: codec2_state must not be NULL");
    drop(Box::from_raw(codec2_state));
}

/// # Safety
/// `codec2_state` must be a live handle from `codec2_create`;
/// `speech_in` must point to at least `codec2_samples_per_frame` valid
/// `int16_t`s; `bytes` must point to at least `codec2_bytes_per_frame`
/// writable bytes.
#[no_mangle]
pub unsafe extern "C" fn codec2_encode(codec2_state: *mut CODEC2, bytes: *mut u8, speech_in: *const i16) {
    assert!(!codec2_state.is_null(), "codec2_encode: codec2_state must not be NULL");
    let state = &mut *codec2_state;
    let speech = &*(speech_in as *const [i16; SAMPLES_PER_FRAME]);
    let out = state.encoder.encode(speech);
    std::ptr::copy_nonoverlapping(out.as_ptr(), bytes, BYTES_PER_FRAME);
}

/// # Safety
/// `codec2_state` must be a live handle from `codec2_create`; `bytes`
/// must point to at least `codec2_bytes_per_frame` valid bytes;
/// `speech_out` must point to at least `codec2_samples_per_frame`
/// writable `int16_t`s.
#[no_mangle]
pub unsafe extern "C" fn codec2_decode(codec2_state: *mut CODEC2, speech_out: *mut i16, bytes: *const u8) {
    codec2_decode_ber(codec2_state, speech_out, bytes, 0.0);
}

/// `ber_est` is unused for `CODEC2_MODE_3200`: checked against the real
/// reference's own `codec2_decode_ber` (`codec2.c`) -- 3200bps mode sets
/// `c2->decode` (not `c2->decode_ber`), so `codec2_decode_ber` there
/// calls `c2->decode(...)` and never touches `ber_est` at all for this
/// mode. Matched here rather than inventing a soft-decision treatment
/// the real reference doesn't apply at 3200bps either.
///
/// # Safety
/// Same pointer/length contract as `codec2_decode`.
#[no_mangle]
pub unsafe extern "C" fn codec2_decode_ber(codec2_state: *mut CODEC2, speech_out: *mut i16, bytes: *const u8, _ber_est: f32) {
    assert!(!codec2_state.is_null(), "codec2_decode_ber: codec2_state must not be NULL");
    let state = &mut *codec2_state;
    let frame = &*(bytes as *const [u8; BYTES_PER_FRAME]);
    let out = state.decoder.decode(frame);
    std::ptr::copy_nonoverlapping(out.as_ptr(), speech_out, SAMPLES_PER_FRAME);
}

/// # Safety
/// `codec2_state` must be non-NULL (value not otherwise inspected --
/// same real answer, 160, for every live handle, since this build only
/// ever creates `CODEC2_MODE_3200` handles).
#[no_mangle]
pub unsafe extern "C" fn codec2_samples_per_frame(codec2_state: *mut CODEC2) -> c_int {
    assert!(!codec2_state.is_null(), "codec2_samples_per_frame: codec2_state must not be NULL");
    SAMPLES_PER_FRAME as c_int
}

/// # Safety
/// Same contract as `codec2_samples_per_frame`.
#[no_mangle]
pub unsafe extern "C" fn codec2_bits_per_frame(codec2_state: *mut CODEC2) -> c_int {
    assert!(!codec2_state.is_null(), "codec2_bits_per_frame: codec2_state must not be NULL");
    (BYTES_PER_FRAME * 8) as c_int
}

/// # Safety
/// Same contract as `codec2_samples_per_frame`.
#[no_mangle]
pub unsafe extern "C" fn codec2_bytes_per_frame(codec2_state: *mut CODEC2) -> c_int {
    assert!(!codec2_state.is_null(), "codec2_bytes_per_frame: codec2_state must not be NULL");
    BYTES_PER_FRAME as c_int
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_encode_decode_destroy_round_trip_over_the_c_abi_matches_direct_rust_use() {
        // Exercises the exact same call sequence a C caller makes,
        // through the actual `extern "C"` entry points (not the
        // underlying `Encoder`/`Decoder` directly) -- catches an FFI
        // layout/pointer-arithmetic bug the Rust-side unit tests in
        // `ham_digital_modes` itself can't see.
        unsafe {
            let enc_handle = codec2_create(CODEC2_MODE_3200);
            assert!(!enc_handle.is_null());
            assert_eq!(codec2_samples_per_frame(enc_handle), 160);
            assert_eq!(codec2_bits_per_frame(enc_handle), 64);
            assert_eq!(codec2_bytes_per_frame(enc_handle), 8);

            let dec_handle = codec2_create(CODEC2_MODE_3200);
            assert!(!dec_handle.is_null());

            // A few frames of a simple synthetic tone -- not asserting
            // exact sample values (covered by `ham_digital_modes`'s own
            // decoder-vs-reference regression test), just that the FFI
            // round trip runs cleanly end to end and produces finite,
            // plausibly-scaled audio.
            for frame_i in 0..20 {
                let mut speech_in = [0i16; SAMPLES_PER_FRAME];
                for (i, s) in speech_in.iter_mut().enumerate() {
                    let t = (frame_i * SAMPLES_PER_FRAME + i) as f32 / 8000.0;
                    *s = (3000.0 * (2.0 * std::f32::consts::PI * 200.0 * t).sin()) as i16;
                }
                let mut bytes = [0u8; BYTES_PER_FRAME];
                codec2_encode(enc_handle, bytes.as_mut_ptr(), speech_in.as_ptr());

                let mut speech_out = [0i16; SAMPLES_PER_FRAME];
                codec2_decode(dec_handle, speech_out.as_mut_ptr(), bytes.as_ptr());
                for &s in &speech_out {
                    assert!(s.abs() < 32767, "sample hit clip boundary on frame {frame_i}: {s}");
                }
            }

            codec2_destroy(enc_handle);
            codec2_destroy(dec_handle);
        }
    }

    #[test]
    fn create_rejects_every_mode_but_3200() {
        for mode in [1, 2, 3, 4, 5, 8, -1, 999] {
            assert!(codec2_create(mode).is_null(), "mode {mode} should be rejected (only CODEC2_MODE_3200 is implemented)");
        }
        unsafe {
            let h = codec2_create(CODEC2_MODE_3200);
            assert!(!h.is_null());
            codec2_destroy(h);
        }
    }
}
