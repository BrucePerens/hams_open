// SPDX-License-Identifier: LGPL-3.0-or-later
//! Pins `DecoderFixed`'s exact output on 30 000 pseudo-random bitstream
//! frames (garbage in, as a noisy radio channel produces; this drives the
//! decoder through extreme line spectral pair, energy and pitch values and
//! through the slow range-checked arithmetic paths).
//!
//! The expected hash was measured on 2026-09-20 with the decoder as it
//! stood *before* the 32-bit-core optimisation work (commit a93ea2ad: `i128`
//! arithmetic, lazily built float tables, textbook FFT), and the optimised
//! decoder reproduces it bit for bit. A change that alters any decoded
//! sample on any of these frames fails here; if a change is meant to alter
//! decoder output, re-measure and record why.

use ham_digital_modes::codec2_3200::DecoderFixed;

#[test]
fn decoder_fixed_output_on_pseudo_random_bitstreams_is_unchanged() {
    let mut seed = 12345u64;
    let mut h = 0xcbf29ce484222325u64;
    let mut d = DecoderFixed::new();
    for k in 0..30_000u32 {
        let mut b = [0u8; 8];
        for x in b.iter_mut() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *x = (seed >> 56) as u8;
        }
        if k % 5 == 0 {
            b[0] |= 0xC0; // both voiced bits
        }
        if k % 11 == 0 {
            b[1] = 0xFF;
        }
        for s in d.decode(&b) {
            h = (h ^ (s as u16 as u64)).wrapping_mul(0x100000001b3);
        }
        if k % 997 == 0 {
            d = DecoderFixed::new();
        }
    }
    assert_eq!(h, 0xaafb64f670c425b9, "DecoderFixed output changed: hash {h:016x}");
}
