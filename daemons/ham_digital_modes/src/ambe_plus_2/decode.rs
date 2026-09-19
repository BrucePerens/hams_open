//! Parameter extraction and dequantization for AMBE+2 half-rate frames -- see `mod.rs`'s own doc
//! comment for the real, source-verified `b0..b8` bit-scatter this module implements directly
//! (traced from mbelib's real `mbe_decodeAmbe2450Parms`), and for why the FEC/whitening layer
//! itself (`parse_frame`) is reused from `super::super::ambe_dstar` rather than reimplemented here.

use super::tables;

/// Reads `width` bits starting at direct (0-based, MSB-first) index `start` into the 49-bit `d[]`
/// field -- i.e. `bits(d, 0, 1)` is `d[0]`, the overall field's own first/most-significant bit,
/// matching mbelib's own `ambe_d[i]` indexing convention (see `mod.rs`'s doc comment for why this
/// differs from `ambe_dstar::decode`'s own MSB-relative `d[a..b)` notation).
fn bits(d: u64, start: usize, width: usize) -> u32 {
    let shift = 49 - start - width;
    ((d >> shift) & ((1u64 << width) - 1)) as u32
}

fn bit(d: u64, start: usize) -> u32 {
    bits(d, start, 1)
}

/// The nine raw parameter indices `b0..b8`, extracted from the 49 decoded data bits per the exact
/// bit mapping documented in `mod.rs`'s own doc comment (traced directly from mbelib's real
/// source, not guessed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawParameters {
    pub b0: u32,
    pub b1: u32,
    pub b2: u32,
    pub b3: u32,
    pub b4: u32,
    pub b5: u32,
    pub b6: u32,
    pub b7: u32,
    pub b8: u32,
}

// [@ANCHOR: extract_raw_parameters]
pub fn extract_raw_parameters(d: u64) -> RawParameters {
    RawParameters {
        b0: (bits(d, 0, 4) << 3) | bits(d, 37, 3),
        b1: (bits(d, 4, 4) << 1) | bit(d, 35),
        b2: (bits(d, 8, 4) << 1) | bit(d, 36),
        b3: (bits(d, 12, 8) << 1) | bit(d, 40),
        b4: (bits(d, 20, 4) << 3) | bits(d, 41, 3),
        b5: (bits(d, 24, 4) << 1) | bit(d, 44),
        b6: (bits(d, 28, 3) << 1) | bit(d, 45),
        b7: (bits(d, 31, 3) << 1) | bit(d, 46),
        b8: (bit(d, 34) << 2) | bits(d, 47, 2),
    }
}

/// `b0`'s own real special-frame ranges (120-127, beyond Annex A's 120 real pitch codes),
/// per mbelib's real `mbe_decodeAmbe2450Parms` branch on `b0`. See `mod.rs`'s own doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// `b0` 0-119: a real, voiced/unvoiced speech frame -- the common case.
    Speech,
    /// `b0` 120-123: a lost/erased frame per spec; no real parameters to decode. **Caveat, not just
    /// spec text**: the real chip also emits `b0=120` for a genuinely *detected* tone/DTMF digit
    /// under `TD_ENABLE` (confirmed via the independent `ECMODE_OUT`/`TONE_FRAME` ground-truth bit,
    /// `AMBE_CHIP_VALIDATION_FINDINGS.md` section 40) instead of its own spec-defined `Tone` range
    /// (126-127) -- a caller that treats every `Erasure` frame as "nothing real happened" will
    /// silently drop real detected tones on this rate. There is currently no known field that
    /// recovers the tone's identity from these frames (section 40's own open item).
    Erasure,
    /// `b0` 124-125: silence, with mbelib's own fixed `L=14`, `w0 = 2*pi/32`, fully unvoiced.
    Silence,
    /// `b0` 126-127: a tone frame (Annex J's own dedicated `f0`/`l1`/`l2` encoding) --
    /// **not decoded here**, a real, disclosed gap (see `mod.rs`'s doc comment), not a silent
    /// skip. `AMBE_PLUS_2_NOTES.md`'s own Annex J table has the real data for a future pass.
    Tone,
}

pub fn classify_b0(b0: u32) -> FrameKind {
    match b0 {
        0..=119 => FrameKind::Speech,
        120..=123 => FrameKind::Erasure,
        124..=125 => FrameKind::Silence,
        _ => FrameKind::Tone,
    }
}

/// Persistent decoder state across frames -- the previous frame's own `L`, its (post-inverse-DCT)
/// `log2` spectral-amplitude history, and `gamma`, mirroring mbelib's own `prev_mp` argument (and
/// `ambe_dstar::decode::DStarDecoderState`'s identical role for the sibling generation).
pub struct DecoderState {
    pub l: u32,
    pub log2_ml: Vec<f64>,
    pub gamma: f64,
}

impl DecoderState {
    /// A reasoned initial state for the very first frame -- see
    /// `ambe_dstar::decode::DStarDecoderState::initial`'s own doc comment for why a flat, constant
    /// starting point is low-stakes here (this recursion's own gain term is a frame-to-frame
    /// difference, not an absolute value).
    pub fn initial() -> Self {
        DecoderState {
            l: 9,
            log2_ml: vec![0.0; 10],
            gamma: 0.0,
        }
    }
}

/// One frame's own real, decoded semantic parameters.
#[derive(Debug, Clone)]
pub struct Parameters {
    pub l: u32,
    pub w0: f64,
    pub voiced: Vec<bool>,
    pub ml: Vec<f64>,
}

/// The real outcome of dequantizing one frame: a genuine speech frame's parameters, or one of the
/// three special frame kinds `mod.rs`'s doc comment describes -- never a panic on any of the 128
/// possible `b0` values.
pub enum DequantizedFrame {
    Speech(Parameters),
    Erasure,
    /// mbelib's own fixed silence-frame parameters (`L=14`, `w0 = 2*pi/32`, fully unvoiced) --
    /// carried here rather than discarded, since a real caller synthesizing audio still needs them.
    Silence { l: u32, w0: f64 },
    Tone { raw: RawParameters },
}

/// Dequantizes `RawParameters` into real synthesis-ready parameters (or a special-frame result),
/// advancing `state` in place for the `Speech` case -- the direct equivalent of mbelib's own
/// `mbe_decodeAmbe2450Parms`, verified stage by stage against that real source.
// [@ANCHOR: dequantize]
pub fn dequantize(raw: &RawParameters, state: &mut DecoderState) -> DequantizedFrame {
    match classify_b0(raw.b0) {
        FrameKind::Erasure => return DequantizedFrame::Erasure,
        FrameKind::Tone => return DequantizedFrame::Tone { raw: *raw },
        FrameKind::Silence => {
            // mbelib's own fixed silence-frame parameters: L=14, w0 = 2*pi/32, fully unvoiced.
            let l = 14u32;
            let w0 = 2.0 * std::f64::consts::PI / 32.0;
            state.l = l;
            state.gamma = 0.0;
            state.log2_ml = vec![0.0; l as usize + 1];
            return DequantizedFrame::Silence { l, w0 };
        }
        FrameKind::Speech => {}
    }

    let l = tables::L_TABLE[raw.b0 as usize];
    let f0 = tables::W0_TABLE[raw.b0 as usize];
    let w0 = f0 * 2.0 * std::f64::consts::PI;

    let mut voiced = vec![false; l as usize + 1];
    for (harmonic, slot) in voiced.iter_mut().enumerate().skip(1) {
        let jl = (harmonic as f64 * 16.0 * f0) as usize;
        *slot = tables::VUV[raw.b1 as usize][jl.min(7)];
    }

    let delta_gamma = tables::DG[raw.b2 as usize];
    let gamma = delta_gamma + 0.5 * state.gamma;

    let prba24 = tables::PRBA24[raw.b3 as usize];
    let prba58 = tables::PRBA58[raw.b4 as usize];
    let gm: [f64; 9] = [
        0.0, 0.0, prba24[0], prba24[1], prba24[2], prba58[0], prba58[1], prba58[2], prba58[3],
    ];
    let mut ri = [0.0f64; 9];
    for (i, slot) in ri.iter_mut().enumerate().skip(1) {
        let mut sum = 0.0;
        for (m, &gm_m) in gm.iter().enumerate().skip(1) {
            let am = if m == 1 { 1.0 } else { 2.0 };
            sum += am
                * gm_m
                * (std::f64::consts::PI * (m as f64 - 1.0) * (i as f64 - 0.5) / 8.0).cos();
        }
        *slot = sum;
    }

    let rconst = 1.0 / (2.0 * std::f64::consts::SQRT_2);
    let mut cik = [[0.0f64; 18]; 5];
    cik[1][1] = 0.5 * (ri[1] + ri[2]);
    cik[1][2] = rconst * (ri[1] - ri[2]);
    cik[2][1] = 0.5 * (ri[3] + ri[4]);
    cik[2][2] = rconst * (ri[3] - ri[4]);
    cik[3][1] = 0.5 * (ri[5] + ri[6]);
    cik[3][2] = rconst * (ri[5] - ri[6]);
    cik[4][1] = 0.5 * (ri[7] + ri[8]);
    cik[4][2] = rconst * (ri[7] - ri[8]);

    let ji = tables::LMPRBL[l as usize];
    let hoc_tables: [&[[f64; 4]]; 4] = [
        &tables::HOC_B5,
        &tables::HOC_B6,
        &tables::HOC_B7,
        &tables::HOC_B8,
    ];
    let hoc_indices = [raw.b5, raw.b6, raw.b7, raw.b8];
    for block in 0..4usize {
        for k in 3..=ji[block] {
            if k <= 6 {
                cik[block + 1][k as usize] = hoc_tables[block][hoc_indices[block] as usize][(k - 3) as usize];
            }
        }
    }

    // Inverse DCT each block's own Cik into Tl (log-domain per-harmonic residual).
    let mut tl = vec![0.0f64; l as usize + 1];
    let mut harmonic = 1usize;
    for block in 0..4 {
        let block_len = ji[block] as usize;
        for j in 1..=block_len {
            let mut sum = 0.0;
            for k in 1..=block_len {
                let ak = if k == 1 { 1.0 } else { 2.0 };
                sum += ak
                    * cik[block + 1][k]
                    * (std::f64::consts::PI * (k as f64 - 1.0) * (j as f64 - 0.5)
                        / block_len as f64)
                        .cos();
            }
            if harmonic <= l as usize {
                tl[harmonic] = sum;
            }
            harmonic += 1;
        }
    }

    let prev_l = state.l.max(1);
    let mut flokl = vec![0.0f64; l as usize + 1];
    let mut intkl = vec![0usize; l as usize + 1];
    let mut deltal = vec![0.0f64; l as usize + 1];
    let mut sum43 = 0.0;
    for h in 1..=l as usize {
        let f = (prev_l as f64 / l as f64) * h as f64;
        let ik = f.floor() as usize;
        flokl[h] = f;
        intkl[h] = ik;
        deltal[h] = f - ik as f64;
        let prev_at = |idx: usize| -> f64 {
            state
                .log2_ml
                .get(idx)
                .copied()
                .unwrap_or(*state.log2_ml.last().unwrap_or(&0.0))
        };
        sum43 += (1.0 - deltal[h]) * prev_at(ik) + deltal[h] * prev_at(ik + 1);
    }
    sum43 *= 0.65 / l as f64;

    let sum42: f64 = tl[1..=l as usize].iter().sum::<f64>() / l as f64;
    let big_gamma = gamma - 0.5 * (l as f64).log2() - sum42;

    let mut log2_ml = vec![0.0f64; l as usize + 1];
    let mut ml = vec![0.0f64; l as usize + 1];
    let unvc = 0.2046 / w0.sqrt();
    for h in 1..=l as usize {
        let prev_at = |idx: usize| -> f64 {
            state
                .log2_ml
                .get(idx)
                .copied()
                .unwrap_or(*state.log2_ml.last().unwrap_or(&0.0))
        };
        let c1 = 0.65 * (1.0 - deltal[h]) * prev_at(intkl[h]);
        let c2 = 0.65 * deltal[h] * prev_at(intkl[h] + 1);
        log2_ml[h] = tl[h] + c1 + c2 - sum43 + big_gamma;
        ml[h] = if voiced[h] {
            (0.693 * log2_ml[h]).exp()
        } else {
            unvc * (0.693 * log2_ml[h]).exp()
        };
    }

    state.l = l;
    state.log2_ml = log2_ml.clone();
    state.gamma = gamma;

    DequantizedFrame::Speech(Parameters { l, w0, voiced, ml })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ambe::fec::golay_encode;
    use crate::ambe_dstar::whiten_c1;

    /// The `b0..b8` scatter must be a genuine bijection over all 49 bits of `d[]` -- every index
    /// used by exactly one parameter, exactly once (`mod.rs`'s own doc comment claims this
    /// explicitly, unlike D-STAR's own scatter which leaves one bit unused). A missed or doubled
    /// index here is the exact class of transcription bug the D-STAR build caught twice.
    #[test]
    fn scatter_covers_every_bit_of_d_exactly_once() {
        // Build a d[] where bit i (MSB-relative, matching bits()/bit()'s own convention) is set
        // iff i is "claimed" by walking extract_raw_parameters' own bit-source list directly.
        let sources: Vec<(usize, usize)> = vec![
            (0, 4),
            (37, 3), // b0
            (4, 4),
            (35, 1), // b1
            (8, 4),
            (36, 1), // b2
            (12, 8),
            (40, 1), // b3
            (20, 4),
            (41, 3), // b4
            (24, 4),
            (44, 1), // b5
            (28, 3),
            (45, 1), // b6
            (31, 3),
            (46, 1), // b7
            (34, 1),
            (47, 2), // b8
        ];
        let mut seen = [false; 49];
        #[allow(clippy::needless_range_loop)] // `i` is a d[] bit index, not merely a seen[] cursor
        for (start, width) in sources {
            for i in start..start + width {
                assert!(!seen[i], "d[{i}] claimed by more than one parameter");
                seen[i] = true;
            }
        }
        assert!(
            seen.iter().all(|&s| s),
            "not every d[] bit is covered: {:?}",
            seen.iter().enumerate().filter(|(_, &s)| !s).map(|(i, _)| i).collect::<Vec<_>>()
        );
    }

    /// A real, self-consistent round trip through the shared (reused) FEC/whitening layer plus
    /// this module's own scatter: build a frame from known parameter indices, parse it back, and
    /// confirm both zero-error Golay decoding and an exact `RawParameters` match -- the same
    /// load-bearing check `ambe_dstar::decode`'s own tests use, applied to this generation's own
    /// scatter.
    #[test]
    fn extract_raw_parameters_round_trips_through_the_shared_frame_layer() {
        let original = RawParameters {
            b0: 0b101_0110,   // 7 bits, < 120 so a real speech frame
            b1: 0b1_0110,     // 5 bits
            b2: 0b0_1101,     // 5 bits
            b3: 0b1_0110_1100, // 9 bits
            b4: 0b101_1010,   // 7 bits
            b5: 0b0_1101,     // 5 bits
            b6: 0b0110,       // 4 bits
            b7: 0b1001,       // 4 bits
            b8: 0b101,        // 3 bits
        };
        assert!(original.b0 < 120);

        let d = crate::ambe_plus_2::encode::pack_raw_parameters(&original);

        let c0_data = ((d >> 37) & 0xFFF) as u16;
        let c1_data = ((d >> 25) & 0xFFF) as u16;
        let c2 = ((d >> 14) & 0x7FF) as u32;
        let c3 = (d & 0x3FFF) as u32;
        let c0_codeword = golay_encode(c0_data);
        let c1_codeword = golay_encode(c1_data);
        let c1_whitened = whiten_c1(c1_codeword, c0_data);
        let frame: u128 = ((c0_codeword as u128) << 49)
            | ((c1_whitened as u128) << 25)
            | ((c2 as u128) << 14)
            | (c3 as u128);

        let parsed = crate::ambe_plus_2::parse_frame(frame);
        assert_eq!(parsed.epsilon_c0, 0);
        assert_eq!(parsed.epsilon_c1, 0);

        let recovered = extract_raw_parameters(parsed.d);
        assert_eq!(recovered, original);
    }

    /// `dequantize` must never panic across the full 7-bit `b0` space, including every special
    /// range (erasure/silence/tone), and every produced speech-frame amplitude must be finite and
    /// non-negative -- the same full-range sanity sweep `ambe_dstar::decode`'s own tests apply.
    #[test]
    fn dequantize_never_panics_and_produces_sane_output_across_the_full_b0_range() {
        for b0 in 0u32..128 {
            let raw = RawParameters {
                b0,
                b1: 5,
                b2: 10,
                b3: 100,
                b4: 20,
                b5: 3,
                b6: 3,
                b7: 3,
                b8: 3,
            };
            let mut state = DecoderState::initial();
            match dequantize(&raw, &mut state) {
                DequantizedFrame::Speech(params) => {
                    assert!((9..=56).contains(&params.l), "b0={b0}: l={} out of range", params.l);
                    for (h, &m) in params.ml.iter().enumerate().skip(1) {
                        assert!(m.is_finite() && m >= 0.0, "b0={b0}, harmonic {h}: Ml={m}");
                    }
                }
                DequantizedFrame::Erasure => assert!((120..=123).contains(&b0)),
                DequantizedFrame::Silence { l, w0 } => {
                    assert!((124..=125).contains(&b0));
                    assert_eq!(l, 14);
                    assert!(w0.is_finite() && w0 > 0.0);
                }
                DequantizedFrame::Tone { raw: r } => {
                    assert!((126..=127).contains(&b0));
                    assert_eq!(r.b0, b0);
                }
            }
        }
    }
}
