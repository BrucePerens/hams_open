// SPDX-License-Identifier: AGPL-3.0-or-later
//! The splitter's CI-V and TS-2000 personalities (what outside programs say to the virtual
//! port) fed arbitrary bytes in two pieces, with a later reset.
#![no_main]

use std::time::Duration;

use libfuzzer_sys::fuzz_target;
use radioctl_splitter::civ_personality::CivPersonality;
use radioctl_splitter::personality::Personality;
use radioctl_splitter::ts2000_personality::Ts2000Personality;

fuzz_target!(|data: &[u8]| {
    let cut = data.len() / 2;
    let now = Duration::from_secs(5);
    let mut civ = CivPersonality::new(0x94, true);
    let mut ts2000 = Ts2000Personality::new();
    for personality in [&mut civ as &mut dyn Personality, &mut ts2000] {
        let _ = personality.parse(&data[..cut], now);
        let _ = personality.parse(&data[cut..], now);
        personality.reset();
        let _ = personality.parse(data, now);
    }
});
