// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-capi`: the C ABI of radioctl (a sketch, version 1).
//!
//! What a C caller gets: opaque handles ([`ffi::RctlRuntime`], [`ffi::RctlRig`]), blocking calls
//! that return a status code, a per-thread error message, and the header `include/radioctl.h`
//! generated from this crate by cbindgen.
//!
//! Rules the API keeps:
//! - **Explicit runtime ownership.** There is no hidden global runtime. The caller creates a
//!   runtime object (`rctl_runtime_new`, which exists only with the `owned-runtime` feature, on by
//!   default) or a Rust host supplies one ([`ffi::runtime_from_handle`]); every rig is opened on
//!   one; a runtime cannot be freed while rigs are open on it. Calls block the calling thread and
//!   must not be made from inside the runtime's own tasks.
//! - **The transmit gate is closed by default.** A rig opened through this API cannot transmit.
//!   `rctl_rig_set_ptt(rig, 1)` is refused with `RCTL_ERR_TX_GATE_CLOSED` until the caller makes the
//!   explicit permit call `rctl_rig_permit_transmit(rig, who)`, which names who decided. Unkeying
//!   is always allowed. The API exposes no raw command, no Morse and no other way to transmit.
//! - **No panic crosses the boundary:** every entry point catches panics and returns
//!   `RCTL_ERR_INTERNAL`.
//! - **Strings** passed in are borrowed NUL-terminated UTF-8; strings out are written to
//!   caller buffers with a length; the one library-owned string, the last error, is per thread and
//!   valid until the next call on that thread.
//! - **Handles** must be used from one thread at a time per handle (calls on one rig are serialised
//!   by the library, but closing while another thread uses it is undefined). Freeing a handle twice,
//!   or using it after it is freed, is undefined behaviour as in any C API; a magic number turns
//!   most such mistakes into `RCTL_ERR_BAD_HANDLE` rather than a crash.

#![deny(unsafe_op_in_unsafe_fn)]

pub mod ffi;
