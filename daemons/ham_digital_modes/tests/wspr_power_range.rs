use ham_digital_modes::*;
// Tests [@ANCHOR: pack_grid4_power]
#[test]
fn wspr_out_of_range_power() {
    for p in [-1, 61, 63, -64] {
        assert!(
            wspr::wspr_encode_symbols("K6BP", "CM87", p).is_none(),
            "power {p}"
        );
    }
}
