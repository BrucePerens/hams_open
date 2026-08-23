// SPDX-License-Identifier: LGPL-3.0-or-later
//! Raw FFI declarations matching vendor/ft8_lib/shim.c exactly.
//! `Ft8Session` is deliberately opaque here -- Rust never constructs or
//! reads its fields, only holds and passes the pointer shim.c's own
//! allocator returns. See ft8.rs for the safe wrapper.
#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_float, c_int, c_uchar};

pub const FTX_MAX_MESSAGE_LENGTH: usize = 35;
pub const FT8_NN: usize = 79;

#[repr(C)]
pub struct Ft8Session {
    _private: [u8; 0],
}

extern "C" {
    pub fn ft8_session_new(sample_rate: c_int, f_min: c_float, f_max: c_float) -> *mut Ft8Session;
    pub fn ft8_session_free(s: *mut Ft8Session);
    pub fn ft8_session_process(s: *mut Ft8Session, frame: *const c_float);
    pub fn ft8_session_block_size(s: *const Ft8Session) -> c_int;
    pub fn ft8_session_reset(s: *mut Ft8Session);
    pub fn ft8_session_decode(
        s: *mut Ft8Session,
        out_messages: *mut [c_char; FTX_MAX_MESSAGE_LENGTH],
        out_snr: *mut c_int,
        max_messages: c_int,
    ) -> c_int;
    pub fn ft8_encode_message(text: *const c_char, tones_out: *mut c_uchar) -> c_int;
}
