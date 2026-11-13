// SPDX-License-Identifier: LGPL-3.0-or-later
//! Property tests standing in for fuzzing: no input may panic or hang a decoder. Random Codec2
//! frames in long sequences (decoder state carries over), AMBE code vectors outside their bit
//! widths, FEC words, D-STAR headers, and arbitrary audio into the PSK31 and RTTY decoders.
//! Bounded for CI (16 cases); a long run: `PROPTEST_CASES=20000 cargo test --test fuzz_decoders`.
#![allow(clippy::expect_used)]

use ham_digital_modes::ambe::dvsi_p25fec::{fec, frame};
use ham_digital_modes::ambe::fixed::tia_102_baba::decode::DecoderState as FixedAmbe;
use ham_digital_modes::ambe::float::tia_102_baba::decode::DecoderState as FloatAmbe;
use ham_digital_modes::ambe::general::fec as general_fec;
use ham_digital_modes::codec2_1600;
use ham_digital_modes::codec2_3200;
use ham_digital_modes::dstar::{dstar_checksum, dstar_checksum_verify, DStarHeader};
use ham_digital_modes::psk31::{psk31_decode_bits, Psk31Decoder};
use ham_digital_modes::rtty::RttyDecoder;
use proptest::prelude::*;

fn config() -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    ProptestConfig {
        cases,
        ..ProptestConfig::default()
    }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn codec2_3200_decoders_survive_any_frames(
        frames in proptest::collection::vec(any::<[u8; 8]>(), 1..60),
    ) {
        let mut float = codec2_3200::Decoder::new();
        let mut fixed = codec2_3200::DecoderFixed::new();
        for f in &frames {
            let _ = float.decode(f);
            let _ = fixed.decode(f);
        }
        let _ = float.decode_16k(&frames[0]);
        let _ = fixed.decode_16k_fixed(&frames[0]);
    }

    #[test]
    fn codec2_1600_decoders_survive_any_frames(
        frames in proptest::collection::vec(any::<[u8; 8]>(), 1..60),
    ) {
        let mut float = codec2_1600::Decoder::new();
        let mut fixed = codec2_1600::DecoderFixed::new();
        for f in &frames {
            let _ = float.decode(f);
            let _ = fixed.decode(f);
        }
        let _ = float.decode_16k(&frames[0]);
        let _ = fixed.decode_16k_fixed(&frames[0]);
    }

    #[test]
    fn ambe_decoders_survive_any_code_vectors(
        frames in proptest::collection::vec(any::<[u32; 8]>(), 1..40),
    ) {
        let mut fixed = FixedAmbe::new();
        let mut float = FloatAmbe::new();
        let mut chip = FloatAmbe::new_chip_wire();
        for c in &frames {
            let _ = fixed.decode_frame(*c);
            let _ = float.decode_frame(*c);
            let _ = chip.decode_frame(*c);
        }
    }

    #[test]
    fn ambe_fec_survives_any_word(word in any::<u32>(), wire in any::<[u8; 18]>()) {
        let _ = general_fec::golay_decode(word);
        let _ = general_fec::hamming_decode(word as u16);
        let _ = fec::hamming_decode_chip(word as u16);
        let _ = fec::g3_decode(word);
        let _ = frame::decode_frame(&wire);
    }

    #[test]
    fn dstar_header_and_checksum_survive_any_bytes(
        body in any::<[u8; 39]>(),
        data in proptest::collection::vec(any::<u8>(), 0..300),
    ) {
        let data: &[u8] = &data;
        let header = DStarHeader::unpack(&body);
        let _ = header.pack();
        let _ = dstar_checksum(data);
        let _ = dstar_checksum_verify(data);
    }

    #[test]
    fn psk31_survives_any_audio(
        bits in proptest::collection::vec(any::<bool>(), 0..400),
        samples in proptest::collection::vec(any::<i16>(), 0..6000),
        carrier in 300.0f64..3000.0,
        rate in prop_oneof![Just(8000u32), Just(11025), Just(44100), Just(48000)],
    ) {
        let _ = psk31_decode_bits(&bits);
        let mut dec = Psk31Decoder::new(carrier, rate);
        let _ = dec.feed(&samples);
        let _ = dec.feed(&samples[..samples.len() / 2]);
    }

    #[test]
    fn rtty_survives_any_audio(
        samples in proptest::collection::vec(any::<i16>(), 0..6000),
        mark in 300.0f64..3000.0,
        rate in prop_oneof![Just(8000u32), Just(11025), Just(44100), Just(48000)],
    ) {
        let mut dec = RttyDecoder::new(mark, rate);
        let _ = dec.feed(&samples);
        let _ = dec.feed(&samples[..samples.len() / 3]);
    }
}
