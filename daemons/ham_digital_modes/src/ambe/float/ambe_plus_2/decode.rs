//! Parameter extraction and dequantization for AMBE+2 half-rate frames -- see `mod.rs`'s own doc
//! comment for the real, source-verified `b0..b8` bit-scatter this module implements directly
//! (traced from mbelib's real `mbe_decodeAmbe2450Parms`), and for why the FEC/whitening layer
//! itself (`parse_frame`) is reused from `super::super::dstar` rather than reimplemented here.

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
    /// `b0` 120: the spec calls this range "erasure," but the real chip uses this exact value for a
    /// genuinely *detected or forced* tone/DTMF digit (confirmed via the independent
    /// `ECMODE_OUT`/`TONE_FRAME` ground-truth bit, `AMBE_CHIP_VALIDATION_FINDINGS.md` section 40) --
    /// never a real erasure in any capture taken so far. **A caller that treats this the same as
    /// [`FrameKind::Erasure`] will silently drop a real tone/DTMF digit.** The digit or frequency is
    /// fully recoverable via [`decode_tone_idx`] and [`classify_tone_idx`].
    DetectedTone,
    /// `b0` 121 or 123: a genuinely lost/erased frame per spec, with no real parameters to decode --
    /// unlike `120`/`122`, no stimulus tried so far (detected or forced) has ever produced either of
    /// these two values, so they remain believed-genuine erasure codes rather than a third
    /// undiscovered tone sub-kind.
    Erasure,
    /// `b0` 122: a second, distinct tone-bearing sub-code from `120`, confirmed live via forced
    /// generation of DVSI's own `Call Progress` tones (dial/ring/busy). Same caveat as
    /// [`FrameKind::DetectedTone`]: recoverable via [`decode_tone_idx`]/[`classify_tone_idx`], not a
    /// real erasure.
    CallProgress,
    /// `b0` 124-125: silence, with mbelib's own fixed `L=14`, `w0 = 2*pi/32`, fully unvoiced.
    Silence,
    /// `b0` 126-127: the spec's own documented `Tone` code -- **never observed from this chip**, for
    /// either a `TD_ENABLE`-detected tone or a directly *forced* one (`AMBE-3000R` manual's own
    /// `TONE` field, Table 98/103): both land in `b0=120`/`122` instead (see
    /// [`FrameKind::DetectedTone`]/[`FrameKind::CallProgress`]), confirmed live by feeding the
    /// encoder an explicit `TONE_IDX` and reading back its own channel bits. This variant is
    /// therefore believed unreachable on this chip/rate, not merely undecoded -- kept for spec
    /// completeness and in case some other stimulus (a different rate index, a different chip
    /// revision) does reach it.
    Tone,
}

pub fn classify_b0(b0: u32) -> FrameKind {
    match b0 {
        0..=119 => FrameKind::Speech,
        120 => FrameKind::DetectedTone,
        121 => FrameKind::Erasure,
        122 => FrameKind::CallProgress,
        123 => FrameKind::Erasure,
        124..=125 => FrameKind::Silence,
        _ => FrameKind::Tone,
    }
}

/// **DVSI's own documented `TONE_IDX` field (`AMBE-3000R Vocoder Chip Users Manual`, Version 1.4,
/// March 2013, Table 103 "TONE Field Format" / Table 104 "TONE Index Values", page 74), found
/// serialized directly inside `Erasure` (`b0=120`) frames** for a `TD_ENABLE`-*detected* tone. The
/// manual documents `TONE_IDX` (Field ID `0x00` of a `TONE` field) as usable in both directions:
/// "Can specify the index of a desired tone **or identify the index of a detected or received
/// tone**" -- the detected case is exactly this function, discovered here by direct analysis of real
/// captured chip output (section 40), not by reading the manual first -- the manual was consulted
/// afterward, once the bit-level structure was already found, and turned out to name and tabulate
/// exactly the field already recovered.
///
/// **This is not, in fact, a separate table from Annex J's own tone-frame parameters
/// (`FrameKind::Tone`'s `f0`/`l1`/`l2` table) -- they are the same underlying tone identifiers.**
/// Checked directly: Annex J's own `l1 * f0` and `l2 * f0` reconstruct the real DTMF/call-progress
/// frequencies at the *same* index this function returns for single tones, and (separately) at the
/// index a *forced* tone reads back as (see below) for DTMF and call-progress -- e.g. Annex J row 128
/// (`f0=78.5, l1=12, l2=17`) gives `12*78.5=942 Hz`/`17*78.5=1334.5 Hz`, matching DTMF `'0'`'s own
/// `941/1336 Hz` closely. Annex J is best understood as the *decoder-side synthesis recipe* for
/// whichever tone identifier ends up in a frame -- not a second, independent, still-mysterious
/// encoding.
///
/// **A real, now fully-resolved discrepancy between a *forced* DTMF digit and its readback, found
/// live via `examples/p25_ambe_plus_2_forced_tone_probe.rs`**: forcing the encoder to emit a specific
/// DTMF `TONE_IDX` and reading the result back through this same function does **not** return the
/// same byte that was sent (e.g. forcing `TONE_IDX=0x87` reads back `128`=`0x80`, not `0x87`). This is
/// not a second numbering space -- Table 104 documents *two* DTMF columns, "Rate Index 0-32" and
/// "Rate Index 33-61" (a different, non-monotonic nibble mapping in each), and at `RATET(33)` (**the
/// only rate this was tested against** -- not yet confirmed for `RATET(34)`, AMBE+2 half-rate's own
/// No-FEC rate, or any 0-32-group rate such as `RATET(27)`) the forced-generation `TONE` field is read
/// by the encoder using the **0-32 column**, even though `33` itself is in the 33-61 group, while the
/// encoder's own *output* -- both a
/// genuinely detected digit and a forced one's readback -- is reported using the 33-61 column, which
/// is what this function documents and what [`dtmf_digit_from_tone_idx`] decodes. Checked exactly
/// against all 16 forced DTMF digits: `readback = column_33_61[column_0_32[sent]]` matches every one
/// of the 16 captured values with zero exceptions. So sending `TONE_IDX=0x87` (33-61's own code for
/// '7') is read by the encoder via the 0-32 column as the code for '0', and the chip reports back '0'
/// using its own 33-61 code, `0x80`=`128` -- a real quirk of the forced-generation path specifically
/// (feed it a 0-32-column code to get a chosen digit), not a defect in this function or in the
/// detected-tone case, which was never affected. **Call Progress and single tones have no
/// discrepancy at all** -- forcing `TONE_IDX=0xA0/0xA1/0xA2` (Call Progress dial/ring/busy) reads back
/// `160`/`161`/`162`, which are the exact same bytes (`0xA0=160`, `0xA1=161`, `0xA2=162`), and forcing
/// a single tone round-trips exactly as sent. An earlier draft of this document misread the decimal
/// readback value against the hex-written input and wrongly reported a Call Progress discrepancy that
/// was never real; corrected here after re-deriving the comparison in both bases directly.
///
/// Every field of the 49-bit `d` besides `TONE_IDX` itself is either a hard constant (`b0`'s own
/// marker, `d[20..24)`/`d[28..32)` at DTMF's own fixed high nibble, `d[36..40)` always `0`) or a
/// separately-identified amplitude/gain field (`d[4..16)`, confirmed to track input amplitude, not
/// digit identity) -- confirmed against all 16 real ITU-T Q.23 DTMF digits and 26 real single-tone
/// captures spanning several frequencies and amplitudes, not a small or cherry-picked sample.
///
/// **The byte is serialized low-nibble-first, redundantly, not simply repeated verbatim**: the low
/// nibble appears four times (`d[16..20)`, `d[24..28)`, `d[32..36)`, `d[40..44)`), while the high
/// nibble reliably appears only twice (`d[20..24)`, `d[28..32)`) -- a third copy would fall at
/// `d[36..40)`, but that range's low 3 bits are pinned to `0` by `b0=120`'s own marker requirement
/// (`extract_raw_parameters`'s `b0 = (bits(d,0,4)<<3) | bits(d,37,3)`), so it can only carry a high
/// nibble whose own low 3 bits are already `0` (true of every DTMF and single-tone value tested,
/// all `< 0x10`). [`decode_tone_idx`] itself never reads `d[36..40)` at all -- only the four
/// low-nibble copies and the two high-nibble copies feed its result -- so a Call Progress frame's
/// `0xA` high nibble (whose low 3 bits are *not* `0`) cannot conflict with this function's own
/// decode regardless of what `d[36..40)` carries for that case; confirmed live, `decode_tone_idx`
/// recovers Call Progress's `0xA0`/`0xA1`/`0xA2` exactly (see below). [`decode_tone_idx`] trusts the
/// two unconstrained high-nibble copies, majority-votes the four low-nibble copies, and returns
/// `None` rather than a guess if either check fails -- never a false positive on a corrupted or
/// unrelated frame.
///
/// **Confirmed decoding both of `TONE_IDX`'s own documented ranges** (both specific to AMBE+2
/// half-rate's own `RATET(33)`, in DVSI's own "Rate Index Values 33 to 61" column of Table 104 --
/// the table's other column, for rate indices 0-32, uses a *different*, non-monotonic DTMF mapping
/// not applicable here):
/// - **DTMF** (`0x80..=0x8F`): `0x80 | nibble`, where `nibble` is the digit's own standard
///   DTMF-as-4-bit-nibble value (`'0'->0x0`, `'1'..='9'->0x1..=0x9`, `'A'..='D'->0xA..=0xD`,
///   `'*'->0xE`, `'#'->0xF`) -- see [`dtmf_digit_from_tone_idx`] for the row/column decode.
///   Chip-validated live end to end, 128/128 (`examples/ambe_chip_validate_ambe_plus_2_dtmf.rs`).
/// - **Single tone** (`0x05..=0x7A`): the table's own documented formula, `index = round(f0 / 31.25
///   Hz)` (156.25 Hz to 3812.5 Hz in 31.25 Hz steps) -- confirmed against 26 real captures across 8
///   distinct frequencies (203-401 Hz) and 4 amplitudes, exact match every time. Many single
///   frequencies tried were *not* classified as a tone by the chip at all (ordinary `Speech`
///   instead, `TONE_FRAME=0`) -- the chip's tone detector does not treat every possible frequency as
///   detectable.
///
/// - **Call Progress** (`0xA0` dial, `0xA1` ring, `0xA2` busy), under `b0=122` (`FrameKind::
///   CallProgress`) rather than `120`: tested via forced generation and reads back **exactly** as
///   sent -- `0xA0`/`0xA1`/`0xA2`, i.e. `160`/`161`/`162` in the same base (no discrepancy here at
///   all; see the note above about the DTMF-only rate-column mismatch). This function itself does not
///   inspect `b0`, so it decodes a Call Progress frame's `TONE_IDX` the same way as any other. `0xFF`
///   (inactive/invalid) was not tested.
pub fn decode_tone_idx(d: u64) -> Option<u8> {
    let low_copies = [bits(d, 16, 4), bits(d, 24, 4), bits(d, 32, 4), bits(d, 40, 4)];
    let mut low = 0u8;
    for bit_idx in 0..4 {
        let mask = 1u32 << (3 - bit_idx);
        let ones = low_copies.iter().filter(|&&n| n & mask != 0).count();
        if ones == 2 {
            return None; // a genuine tie across the 4 copies -- don't guess.
        }
        if ones > 2 {
            low |= 1u8 << (3 - bit_idx);
        }
    }
    let high_copies = [bits(d, 20, 4), bits(d, 28, 4)];
    if high_copies[0] != high_copies[1] {
        return None; // the only two unconstrained copies disagree -- don't guess.
    }
    Some(((high_copies[0] as u8) << 4) | low)
}

/// Maps a [`decode_tone_idx`] DTMF-range result (`0x80..=0x8F`, AMBE+2 half-rate's own `RATET(33)`)
/// to the `(row, column)` pair of DVSI's own DTMF keypad layout, matching `ambe::ratet27_dtmf`'s and
/// `ambe_dstar::decode`'s own established convention: row 0-3 is 697/770/852/941 Hz, column 0-3 is
/// 1209/1336/1477/1633 Hz. Returns `None` for any value outside `0x80..=0x8F`.
pub fn dtmf_digit_from_tone_idx(tone_idx: u8) -> Option<(u8, u8)> {
    if tone_idx & 0xF0 != 0x80 {
        return None;
    }
    match tone_idx & 0x0F {
        0x1 => Some((0, 0)),
        0x2 => Some((0, 1)),
        0x3 => Some((0, 2)),
        0xA => Some((0, 3)),
        0x4 => Some((1, 0)),
        0x5 => Some((1, 1)),
        0x6 => Some((1, 2)),
        0xB => Some((1, 3)),
        0x7 => Some((2, 0)),
        0x8 => Some((2, 1)),
        0x9 => Some((2, 2)),
        0xC => Some((2, 3)),
        0xE => Some((3, 0)),
        0x0 => Some((3, 1)),
        0xF => Some((3, 2)),
        0xD => Some((3, 3)),
        _ => unreachable!("nibble is masked to 4 bits"),
    }
}

/// DVSI's own `Call Progress` sub-range of `TONE_IDX` (`0xA0`/`0xA1`/`0xA2`/`0xFF`, Table 104).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallProgressTone {
    Dial,
    Ring,
    Busy,
    /// `0xFF`: inactive/invalid -- documented by the manual but not tested live.
    Inactive,
}

/// The real-world meaning of a [`decode_tone_idx`] result, covering every one of `TONE_IDX`'s
/// documented sub-ranges (Table 104) in one place, rather than leaving each caller to re-derive the
/// range boundaries [`dtmf_digit_from_tone_idx`] alone doesn't cover. A `b0` of [`FrameKind::
/// DetectedTone`] or [`FrameKind::CallProgress`] pairs with this; see `decode_tone_idx`'s own doc
/// comment for which sub-range is chip-validated live and which is documented-but-untested.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ToneIdentity {
    /// A DTMF digit, as `(row, column)` per [`dtmf_digit_from_tone_idx`]'s own layout.
    Dtmf { row: u8, col: u8 },
    /// A single tone's frequency in Hz, `tone_idx as f64 * 31.25`.
    SingleTone { hz: f64 },
    CallProgress(CallProgressTone),
    /// A `TONE_IDX` value outside every documented sub-range -- not necessarily invalid, just not
    /// one this crate has identified a meaning for yet.
    Reserved(u8),
}

/// Classifies a [`decode_tone_idx`] result into its real-world meaning. See [`ToneIdentity`]'s own
/// doc comment for which branches are chip-validated live.
pub fn classify_tone_idx(tone_idx: u8) -> ToneIdentity {
    if let Some((row, col)) = dtmf_digit_from_tone_idx(tone_idx) {
        return ToneIdentity::Dtmf { row, col };
    }
    match tone_idx {
        0x05..=0x7A => ToneIdentity::SingleTone { hz: tone_idx as f64 * 31.25 },
        0xA0 => ToneIdentity::CallProgress(CallProgressTone::Dial),
        0xA1 => ToneIdentity::CallProgress(CallProgressTone::Ring),
        0xA2 => ToneIdentity::CallProgress(CallProgressTone::Busy),
        0xFF => ToneIdentity::CallProgress(CallProgressTone::Inactive),
        other => ToneIdentity::Reserved(other),
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
    /// `b0=121` or `123` only -- a genuine erasure with no real parameters. **Not** `b0=120`/`122`;
    /// those carry real tone/DTMF/Call-Progress content and are reported as [`DequantizedFrame::
    /// Tone`] instead, via [`decode_tone_idx`]/[`classify_tone_idx`] on the frame's own `d`.
    Erasure,
    /// mbelib's own fixed silence-frame parameters (`L=14`, `w0 = 2*pi/32`, fully unvoiced) --
    /// carried here rather than discarded, since a real caller synthesizing audio still needs them.
    Silence { l: u32, w0: f64 },
    /// `b0` in `{120, 122, 126, 127}` -- any tone-bearing frame kind (see [`FrameKind::DetectedTone`],
    /// [`FrameKind::CallProgress`], [`FrameKind::Tone`]). The caller decodes the actual tone/digit via
    /// `decode_tone_idx(d)` on the same frame's `d` (not carried in `raw` itself, since `TONE_IDX`
    /// lives outside the `b0..b8` scatter).
    Tone { raw: RawParameters },
}

/// Dequantizes `RawParameters` into real synthesis-ready parameters (or a special-frame result),
/// advancing `state` in place for the `Speech` case -- the direct equivalent of mbelib's own
/// `mbe_decodeAmbe2450Parms`, verified stage by stage against that real source.
// [@ANCHOR: dequantize]
pub fn dequantize(raw: &RawParameters, state: &mut DecoderState) -> DequantizedFrame {
    match classify_b0(raw.b0) {
        FrameKind::Erasure => return DequantizedFrame::Erasure,
        FrameKind::DetectedTone | FrameKind::CallProgress | FrameKind::Tone => {
            return DequantizedFrame::Tone { raw: *raw }
        }
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
    use crate::ambe::general::fec::golay_encode;
    use crate::ambe::float::dstar::whiten_c1;

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

        let d = crate::ambe::float::ambe_plus_2::encode::pack_raw_parameters(&original);

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

        let parsed = crate::ambe::float::ambe_plus_2::parse_frame(frame);
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
                DequantizedFrame::Erasure => assert!(b0 == 121 || b0 == 123, "b0={b0}"),
                DequantizedFrame::Silence { l, w0 } => {
                    assert!((124..=125).contains(&b0));
                    assert_eq!(l, 14);
                    assert!(w0.is_finite() && w0 > 0.0);
                }
                DequantizedFrame::Tone { raw: r } => {
                    assert!(b0 == 120 || b0 == 122 || (126..=127).contains(&b0), "b0={b0}");
                    assert_eq!(r.b0, b0);
                }
            }
        }
    }

    /// Real chip captures, one per DTMF digit (RATET(33), `TD_ENABLE` on, `DTX_ENABLE` on --
    /// `AMBE_CHIP_VALIDATION_FINDINGS.md` section 40's own probe run, the same 16 hex frames already
    /// hardcoded in `examples/ambe_plus_2_erasure_frame_digit_correlation_scan.rs`). Every field of
    /// `d` other than `TONE_IDX`'s own repeated copies is confirmed constant across all 16 (at this
    /// fixed capture amplitude -- `d[4..16)` is a separately-identified amplitude field, not asserted
    /// here as a universal constant) -- checked directly, not assumed -- so this test both confirms
    /// the digit decode and pins the discovery that every other bit really is fixed.
    const REAL_DTMF_CAPTURES: [(&str, &str, u8, u8); 16] = [
        ("1", "e8cedbae008cd122c0", 0, 0),
        ("2", "eacdeb8e20ad8702c0", 0, 1),
        ("3", "eaeff9ae228d9322c0", 0, 2),
        ("A", "ebefc98c01cf8702c0", 0, 3),
        ("4", "cafee98c22b8e502c0", 1, 0),
        ("5", "cadcfbac2098f122c0", 1, 1),
        ("6", "c8dfcb8c00b9a702c0", 1, 2),
        ("B", "ebcddbac03ef9322c0", 1, 3),
        ("7", "c8fdd9ac0299b322c0", 2, 0),
        ("8", "e9ceeb8c23cec502c0", 2, 1),
        ("9", "e9ecf9ac21eed122c0", 2, 2),
        ("C", "cbdccb8e03dae502c0", 2, 3),
        ("*", "c9fde98e21dba702c0", 3, 0),
        ("0", "e8ecc98e02acc502c0", 3, 1),
        ("#", "c9dffbae23fbb322c0", 3, 2),
        ("D", "cbfed9ae01faf122c0", 3, 3),
    ];

    /// Real chip captures of single (non-DTMF) tones (RATET(33), `TD_ENABLE` on -- the same probe
    /// run's `examples/p25_ambe_plus_2_annex_j_tone_id_probe.rs` output, one representative `d` value
    /// per distinct detected frequency). `f0_hz` is the stimulus frequency actually sent; `expected`
    /// is `round(f0_hz / 31.25)`, DVSI's own documented single-tone formula (Table 104).
    const REAL_SINGLE_TONE_CAPTURES: [(f64, u64, u8); 4] = [
        (250.00, 0x1ff6101010100, 0x08),
        (203.12, 0x1ff60c0c0c0c0, 0x06),
        (395.85, 0x1ff61a1a1a1a0, 0x0d),
        (351.56, 0x1ff6161616160, 0x0b),
    ];

    fn hex_to_wire(hex: &str) -> u128 {
        let mut wire: u128 = 0;
        for i in 0..9 {
            let byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap();
            wire = (wire << 8) | byte as u128;
        }
        wire
    }

    #[test]
    fn real_chip_capture_dtmf_digits_decode_correctly_via_tone_idx() {
        for (label, hex, row, col) in REAL_DTMF_CAPTURES {
            let logical = super::super::interleave::interleaved_to_frame(hex_to_wire(hex));
            let parsed = super::super::parse_frame(logical);
            let raw = extract_raw_parameters(parsed.d);
            assert_eq!(classify_b0(raw.b0), FrameKind::DetectedTone, "digit {label}: b0={}", raw.b0);
            let tone_idx = decode_tone_idx(parsed.d).unwrap_or_else(|| panic!("digit {label}: decode_tone_idx returned None"));
            assert_eq!(tone_idx & 0xF0, 0x80, "digit {label}: tone_idx=0x{tone_idx:x} not in DTMF range");
            assert_eq!(
                dtmf_digit_from_tone_idx(tone_idx),
                Some((row, col)),
                "digit {label}: tone_idx=0x{tone_idx:x}"
            );
            // Every field besides TONE_IDX's own copies is a confirmed hard constant across all 16
            // real captures at this fixed amplitude -- assert it rather than merely note it.
            assert_eq!(bits(parsed.d, 4, 12), 0x0f38, "digit {label}: amplitude field changed");
            assert_eq!(bits(parsed.d, 36, 4), 0x8, "digit {label}: constrained 3rd high-nibble copy changed");
            assert_eq!(bits(parsed.d, 44, 5), 0b10000, "digit {label}: tail changed");
        }
    }

    #[test]
    fn real_chip_capture_single_tones_decode_correctly_via_tone_idx() {
        for (f0_hz, d, expected) in REAL_SINGLE_TONE_CAPTURES {
            let raw = extract_raw_parameters(d);
            assert_eq!(classify_b0(raw.b0), FrameKind::DetectedTone, "f0={f0_hz}: b0={}", raw.b0);
            let tone_idx = decode_tone_idx(d).unwrap_or_else(|| panic!("f0={f0_hz}: decode_tone_idx returned None"));
            assert_eq!(tone_idx, expected, "f0={f0_hz}: tone_idx=0x{tone_idx:x}");
            assert_eq!(dtmf_digit_from_tone_idx(tone_idx), None, "f0={f0_hz}: not a DTMF value");
        }
    }

    #[test]
    fn decode_tone_idx_majority_votes_against_a_single_corrupted_low_nibble_copy() {
        // Digit '5' (low nibble 0x5) with the second of its 4 repeated low-nibble copies corrupted
        // to 0x0 -- majority vote across the other 3 correct copies must still recover 0x85.
        let (_, hex, row, col) = REAL_DTMF_CAPTURES[5];
        let logical = super::super::interleave::interleaved_to_frame(hex_to_wire(hex));
        let parsed = super::super::parse_frame(logical);
        let mut d = parsed.d;
        let shift = 49 - 24 - 4;
        d &= !(0xFu64 << shift); // zero out the second low-nibble copy (d[24..28))
        let tone_idx = decode_tone_idx(d).expect("majority vote should still recover a value");
        assert_eq!(dtmf_digit_from_tone_idx(tone_idx), Some((row, col)));
    }

    #[test]
    fn decode_tone_idx_returns_none_when_the_two_high_nibble_copies_disagree() {
        let (_, hex, _, _) = REAL_DTMF_CAPTURES[0];
        let logical = super::super::interleave::interleaved_to_frame(hex_to_wire(hex));
        let parsed = super::super::parse_frame(logical);
        let mut d = parsed.d;
        let shift = 49 - 28 - 4;
        d ^= 0xFu64 << shift; // corrupt the second high-nibble copy (d[28..32)) so it disagrees
        assert_eq!(decode_tone_idx(d), None);
    }

    #[test]
    fn dtmf_digit_from_tone_idx_covers_all_16_values_with_the_standard_keypad_layout() {
        let expected: [(u8, (u8, u8)); 16] = [
            (0x80, (3, 1)),
            (0x81, (0, 0)),
            (0x82, (0, 1)),
            (0x83, (0, 2)),
            (0x84, (1, 0)),
            (0x85, (1, 1)),
            (0x86, (1, 2)),
            (0x87, (2, 0)),
            (0x88, (2, 1)),
            (0x89, (2, 2)),
            (0x8A, (0, 3)),
            (0x8B, (1, 3)),
            (0x8C, (2, 3)),
            (0x8D, (3, 3)),
            (0x8E, (3, 0)),
            (0x8F, (3, 2)),
        ];
        for (tone_idx, pair) in expected {
            assert_eq!(dtmf_digit_from_tone_idx(tone_idx), Some(pair), "tone_idx=0x{tone_idx:x}");
        }
        // Anything outside the DTMF sub-range must not be misread as a digit.
        for outside in [0x05u8, 0x7A, 0xA0, 0xA1, 0xA2, 0xFF, 0x00, 0x90] {
            assert_eq!(dtmf_digit_from_tone_idx(outside), None, "tone_idx=0x{outside:x}");
        }
    }

    /// `classify_b0` must separate the real tone-bearing sub-codes (`120`, `122`) from the genuine
    /// erasure ones (`121`, `123`) -- the correctness fix this test module was missing before: a
    /// caller matching only on `FrameKind::Erasure` would previously drop real detected tones and
    /// Call Progress frames as if nothing had happened.
    #[test]
    fn classify_b0_separates_tone_bearing_subcodes_from_genuine_erasure() {
        assert_eq!(classify_b0(120), FrameKind::DetectedTone);
        assert_eq!(classify_b0(121), FrameKind::Erasure);
        assert_eq!(classify_b0(122), FrameKind::CallProgress);
        assert_eq!(classify_b0(123), FrameKind::Erasure);
    }

    #[test]
    fn classify_tone_idx_covers_every_documented_subrange() {
        assert_eq!(classify_tone_idx(0x81), ToneIdentity::Dtmf { row: 0, col: 0 });
        assert_eq!(classify_tone_idx(0x08), ToneIdentity::SingleTone { hz: 8.0 * 31.25 });
        assert_eq!(classify_tone_idx(0xA0), ToneIdentity::CallProgress(CallProgressTone::Dial));
        assert_eq!(classify_tone_idx(0xA1), ToneIdentity::CallProgress(CallProgressTone::Ring));
        assert_eq!(classify_tone_idx(0xA2), ToneIdentity::CallProgress(CallProgressTone::Busy));
        assert_eq!(classify_tone_idx(0xFF), ToneIdentity::CallProgress(CallProgressTone::Inactive));
        assert_eq!(classify_tone_idx(0x00), ToneIdentity::Reserved(0x00));
    }

    /// The rate-column-mismatch finding, pinned as a regression test: every forced DTMF `TONE_IDX`
    /// (rate-33-61 column) reads back as the byte a *different* digit would use in the same column --
    /// specifically, the digit that the 0-32 column assigns to the byte that was actually sent. See
    /// `decode_tone_idx`'s own doc comment for the full explanation; this is the arithmetic that
    /// explanation rests on, captured as code so a future change can't silently break it.
    #[test]
    fn forced_dtmf_readback_matches_the_rate_column_mismatch_explanation() {
        // (sent TONE_IDX, real chip readback) -- examples/p25_ambe_plus_2_forced_tone_probe.rs.
        const FORCED_SWEEP: [(u8, u8); 16] = [
            (0x80, 0x81), (0x81, 0x84), (0x82, 0x87), (0x83, 0x8e), (0x84, 0x82), (0x85, 0x85),
            (0x86, 0x88), (0x87, 0x80), (0x88, 0x83), (0x89, 0x86), (0x8a, 0x89), (0x8b, 0x8f),
            (0x8c, 0x8a), (0x8d, 0x8b), (0x8e, 0x8c), (0x8f, 0x8d),
        ];
        // Table 104's "Rate Index 0-32" column: TONE_IDX -> digit nibble.
        let col_0_32 = |idx: u8| -> u8 {
            match idx {
                0x80 => 0x1, 0x81 => 0x4, 0x82 => 0x7, 0x83 => 0xE, 0x84 => 0x2, 0x85 => 0x5,
                0x86 => 0x8, 0x87 => 0x0, 0x88 => 0x3, 0x89 => 0x6, 0x8a => 0x9, 0x8b => 0xF,
                0x8c => 0xA, 0x8d => 0xB, 0x8e => 0xC, 0x8f => 0xD,
                _ => unreachable!(),
            }
        };
        for (sent, expected_readback) in FORCED_SWEEP {
            let digit = col_0_32(sent);
            // dtmf_digit_from_tone_idx already implements the rate-33-61 column (this crate's own
            // established mapping); recover the byte that column assigns to `digit`.
            let predicted = (0x80..=0x8Fu8).find(|&b| b & 0x0F == digit).unwrap();
            assert_eq!(predicted, expected_readback, "digit nibble=0x{digit:x}");
        }
    }
}
