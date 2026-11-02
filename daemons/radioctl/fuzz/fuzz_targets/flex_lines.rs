// SPDX-License-Identifier: AGPL-3.0-or-later
//! The FlexRadio line and discovery parsers fed arbitrary bytes, as bytes and as (lossy) text.
#![no_main]

use libfuzzer_sys::fuzz_target;
use radioctl_protocols::flex;

fuzz_target!(|data: &[u8]| {
    let _ = flex::parse_discovery(data);
    let text = String::from_utf8_lossy(data);
    let _ = flex::parse_reply(&text);
    let _ = flex::parse_pairs(&text);
    let _ = flex::parse_info(&text);
    let _ = flex::parse_firmware(&text);
    let _ = flex::parse_mhz(&text);
    let _ = flex::parse_mhz_signed(&text);
    let _ = flex::mode_from_name(&text);
    let _ = flex::line_may_transmit(&text);
    let _ = flex::interlock_keyed(&text);
});
