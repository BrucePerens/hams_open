// SPDX-License-Identifier: LGPL-3.0-or-later
//! No frame of the four OSR speech files is a tone: the fixed tone detector (which the fixed D-STAR and AMBE+2
//! encoders consult for every 20 ms slot, `k*160..(k+1)*160`) rejects every slot of every file, like the float one.

mod common;

use ham_digital_modes::ambe::fixed::general::tone_detect as fx;
use ham_digital_modes::ambe::float::tone_detect as fl;

#[test]
fn no_speech_slot_of_the_four_fixtures_is_a_tone() {
    let mut slots = 0;
    for path in common::OSR_FILES {
        let pcm = common::read_wav_mono_i16(path);
        for (k, slot) in pcm.chunks_exact(160).enumerate() {
            slots += 1;
            assert!(fx::detect_tone(slot).is_none(), "{path} slot {k} detected as a tone by the fixed detector");
            let f: Vec<f64> = slot.iter().map(|&s| s as f64).collect();
            assert!(fl::detect_tone(&f).is_none(), "{path} slot {k} detected as a tone by the float detector");
        }
    }
    assert!(slots > 7000);
}
