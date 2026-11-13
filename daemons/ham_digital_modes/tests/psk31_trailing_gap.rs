use ham_digital_modes::psk31::*;

// Tests [@ANCHOR: psk31_decode_bits]
#[test]
fn last_character_survives_a_stream_cut_after_the_first_gap_zero() {
    let mut bits = psk31_encode_bits("ab");
    bits.pop(); // leaves the codeword plus one provisional gap zero
    assert_eq!(psk31_decode_bits(&bits), "ab");
    bits.pop(); // codeword only, no gap at all
    assert_eq!(psk31_decode_bits(&bits), "ab");
}
