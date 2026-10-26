// SPDX-License-Identifier: AGPL-3.0-or-later
//! The measurement set-up block: the nine vendor requests the vendor program sends for each measurement
//! point (always in this order: 0x40, 0x41, 0x42, 0x43, 0x44, 0x47, 0x4b, 0x49, 0x64), built from a frequency, the
//! IF and the time per point. Checked byte for byte against blocks captured from the vendor program (`VECTORS`).
//!
//! Fields whose meaning is not decoded are written as the constants seen on the wire (see `docs/PROTOCOL.md`).

use crate::protocol::{
    if_scaled_fields, if_word, point_time_byte, sweep_offset_tail, sweep_word, Request, VENDOR_OUT,
};

/// What the set-up block depends on.
#[derive(Clone, Copy, Debug)]
pub struct SetupParams {
    /// The frequency of this point and the frequency before the sweep started.
    pub freq_hz: f64,
    pub previous_hz: f64,
    /// The two chips' clock multipliers (the observed unit: 10 and 9).
    pub n_rf: u8,
    pub n_lo: u8,
    /// The measurement IF (12, 6 or 4 kHz seen) and the time per point.
    pub if_hz: f64,
    pub point_ms: f64,
    /// The first byte of the 0x49 payload (3 or 4 for measurement points, 2 for the slow sweeps seen).
    pub counter: u8,
    /// `wValue` of requests 0x40 and 0x41 (0, 1, 2, 0x0302, 0x0303 and 8 were seen; the meaning is unknown).
    pub mode: u16,
    /// The RF level (14 bits) carried in `wIndex` of the first seven requests, or 0. (The vendor program sometimes
    /// sends it on fewer of them, apparently a race inside the program; that is not reproduced.)
    pub rf_level: u16,
}

/// Bytes 2 and 5 of request 0x40 for a time per point. Only the four settings that were captured are known:
/// 1.33 ms (8, 8), 8 ms (2, 1), 30 ms (1, 1) and 60 ms (1, 1).
pub fn point_time_fields(point_ms: f64) -> Result<(u8, u8), String> {
    // [@ANCHOR: vnwa_open_point_time_fields]
    let near = |t: f64| (point_ms - t).abs() < 0.01 * t;
    if near(4.0 / 3.0) {
        Ok((8, 8))
    } else if near(8.0) {
        Ok((2, 1))
    } else if near(30.0) || near(60.0) {
        Ok((1, 1))
    } else {
        Err(format!("time per point {point_ms} ms: bytes 2 and 5 of request 0x40 are only known for 1.33, 8, 30 and 60 ms"))
    }
}

fn out(request: u8, value: u16, index: u16, data: Vec<u8>) -> Request {
    Request {
        request_type: VENDOR_OUT,
        request,
        value,
        index,
        data,
    }
}

/// The nine requests for one measurement point.
pub fn setup_block(p: &SetupParams) -> Result<Vec<Request>, String> {
    // [@ANCHOR: vnwa_open_setup_block]
    if p.rf_level > 0x3fff {
        return Err(format!(
            "RF level {:#x} is above the 14-bit maximum 0x3fff",
            p.rf_level
        ));
    }
    if p.freq_hz != p.previous_hz && !(3..=4).contains(&p.counter) {
        return Err(format!(
            "step counter {}: the offset divisor is only verified for counters 3 and 4 (or an unchanged frequency)",
            p.counter
        ));
    }
    let (b2, b5) = point_time_fields(p.point_ms)?;
    let word = sweep_word(p.freq_hz, p.n_rf, p.n_lo);
    if word >> 40 != 0 {
        return Err(format!(
            "the sweep word for {} Hz does not fit in 40 bits",
            p.freq_hz
        ));
    }
    let (w41, f41) = if_scaled_fields(p.if_hz);
    let lvl = p.rf_level;

    let mut r40 = vec![point_time_byte(p.point_ms), 0, b2, 1, 0x7d, b5, 0];
    let mut r41 = w41.to_le_bytes().to_vec();
    r41.extend_from_slice(&f41.to_le_bytes());
    r41.push(0x02);
    let mut r42 = if_word(p.if_hz).to_le_bytes().to_vec();
    r42.push(0);
    let mut r43 = if_word(18_000.0).to_le_bytes().to_vec();
    r43.push(0);
    let mut r44 = if_word(300_000.0).to_le_bytes().to_vec();
    r44.push(0);
    let r47 = vec![0, 0, 0, 0, 0x01, 0x40];
    let r4b = vec![0x5b, 0xf3, 0xc3, 0x22, 0x65, 0x00, 0x03];
    let mut r49 = vec![p.counter, 0, p.n_rf, p.n_lo];
    r49.extend_from_slice(&word.to_le_bytes()[..5]);
    r49.extend_from_slice(&sweep_offset_tail(p.previous_hz, p.freq_hz, p.counter));
    r40.shrink_to_fit();

    Ok(vec![
        out(0x40, p.mode, lvl, r40),
        out(0x41, p.mode, lvl, r41),
        out(0x42, 7, lvl, r42),
        out(0x43, 5, lvl, r43),
        out(0x44, 5, lvl, r44),
        out(0x47, 5, lvl, r47),
        out(0x4b, 6, lvl, r4b),
        out(0x49, 0, 0, r49),
        out(0x64, 0, 0, Vec::new()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    pub struct Vector {
        pub point_ms: f64,
        pub if_hz: f64,
        pub counter: u8,
        pub freq_mhz: f64,
        pub previous_mhz: f64,
        pub mode: u16,
        pub rf_level: u16,
        pub time_fields: (u8, u8),
        pub expected: [(u8, u16, u16, &'static str); 9],
    }

    fn hex(d: &[u8]) -> String {
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    include!("setup_vectors.rs");

    // Tests [@ANCHOR: vnwa_open_setup_block]
    // Tests [@ANCHOR: vnwa_open_point_time_fields]
    #[test]
    fn the_block_reproduces_every_captured_block_byte_for_byte() {
        for (n, v) in VECTORS.iter().enumerate() {
            assert_eq!(
                point_time_fields(v.point_ms).unwrap(),
                v.time_fields,
                "vector {n}"
            );
            let p = SetupParams {
                freq_hz: v.freq_mhz * 1e6,
                previous_hz: v.previous_mhz * 1e6,
                n_rf: 10,
                n_lo: 9,
                if_hz: v.if_hz,
                point_ms: v.point_ms,
                counter: v.counter,
                mode: v.mode,
                rf_level: v.rf_level,
            };
            let block = setup_block(&p).unwrap();
            assert_eq!(block.len(), 9);
            for (i, (r, e)) in block.iter().zip(v.expected.iter()).enumerate() {
                let got = (r.request, r.value, r.index, hex(&r.data));
                assert_eq!(
                    got,
                    (e.0, e.1, e.2, e.3.to_string()),
                    "vector {n} request {i}"
                );
            }
        }
    }

    #[test]
    fn unverified_combinations_are_refused() {
        let base = SetupParams {
            freq_hz: 10.0e6,
            previous_hz: 25.0e6,
            n_rf: 10,
            n_lo: 9,
            if_hz: 12_000.0,
            point_ms: 4.0 / 3.0,
            counter: 5,
            mode: 0,
            rf_level: 0,
        };
        assert!(setup_block(&base).unwrap_err().contains("counter"));
        assert!(setup_block(&SetupParams {
            counter: 3,
            point_ms: 17.0,
            ..base
        })
        .unwrap_err()
        .contains("17"));
        assert!(setup_block(&SetupParams {
            counter: 3,
            rf_level: 0x4000,
            ..base
        })
        .is_err());
        assert!(setup_block(&SetupParams { counter: 3, ..base }).is_ok());
        // An unchanged frequency needs no offset, so any counter is fine.
        assert!(setup_block(&SetupParams {
            counter: 25,
            previous_hz: 10.0e6,
            ..base
        })
        .is_ok());
    }
}
