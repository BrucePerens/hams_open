// SPDX-License-Identifier: AGPL-3.0-or-later
//! The rig description (TOML) and transcript parsers fed arbitrary text. A description is loaded
//! from files an operator may have edited, so a malformed one must be an error, never a panic.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = radioctl_core::rigfile::parse(text);
    let _ = radioctl_core::transcript::parse(text);
});
