# Data structures of the chip-probe examples, part 2

As a developer changing the DVSI chip probe examples, I want every data structure in `examples/ambe_bit_sensitivity.rs`, `examples/ambe_chip_validate_p25_wireformat.rs`, `examples/p25_ambe_plus_2_and_dstar_dtx_dtmf_probe.rs`, `examples/p25_ratet27_ecmode_bit_sweep.rs`, `examples/ratet27_diagnose_gain_scale_mismatch.rs` and `examples/ratet27_probe_bit_layout.rs` explained in full (ADR 0113): what it is for, its fields and units, who creates and changes it, and how it is stored or sent. Each entry sits under the anchor on the definition in code; the tests (each example's `tests` module, run by `cargo test --examples`) cite the same anchors. These are developer probes that talk to a DVSI chip over UDP or run offline analyses; none keys a transmitter and none holds secrets.

## `Decoder` [@ANCHOR: ham_digital_modes:ambe_bit_sensitivity_decoder_struct]

**Purpose.** The type of the decode step in `ambe_bit_sensitivity.rs`: a boxed closure that takes a sequence of 128-bit wire frames (`&[u128]`, one frame per element, the packed AMBE bits) and returns the decoded PCM as one `Vec<f64>` for the whole sequence. It lets the example pick the D-STAR or the AMBE+2 synthesis decoder at run time and use one code path for the flip-one-bit sweep.

**Definition.** `type Decoder = Box<dyn Fn(&[u128]) -> Vec<f64>>;` (type alias, example-private).

**Notes.** The closure owns its decoder state (it builds a fresh `DStarSynthesisDecoder` or `AmbePlus2SynthesisDecoder` inside), so every call decodes from a clean state; it is not `Send`. An empty slice gives an empty vector. Test: the example's `tests` module.

## `PermHit` [@ANCHOR: ham_digital_modes:ambe_chip_validate_p25_wireformat_perm_hit_struct]

**Purpose.** One surviving candidate of the permutation search in `ambe_chip_validate_p25_wireformat.rs`: a way of reading the chip's 18 wire bytes (byte order, bit order, and a permutation of the eight blocks) together with how many frames it explained. The search keeps a candidate only when at least two of the first four blocks match raw or after de-whitening.

**Definition.** `struct PermHit { reverse_bytes: bool, lsb_first: bool, perm: [u8; 8], raw4: usize, dew4: usize, dew7: usize, total: usize }`.

| Field | Type | Meaning |
|---|---|---|
| `reverse_bytes` | `bool` | The candidate reads the 18 bytes in reverse order. |
| `lsb_first` | `bool` | The candidate takes the bits of each byte least significant first. |
| `perm` | `[u8; 8]` | Which wire block feeds each of the eight FEC blocks. |
| `raw4` | `usize` | Frames (of `total`) whose first four blocks decode as valid without de-whitening. |
| `dew4` | `usize` | The same count after de-whitening. |
| `dew7` | `usize` | Frames whose seven-block check passes after de-whitening. |
| `total` | `usize` | The number of frames tried (every field above is at most this). |

**Lifetime.** Built by `permutation_search`, returned sorted by `(dew4, raw4)` descending, printed (top ten) by `main`. Plain data, not serialized.

## `DstarToneCapture` [@ANCHOR: ham_digital_modes:p25_ambe_plus_2_and_dstar_dtx_dtmf_probe_dstar_tone_capture_struct]

**Purpose.** What the DTX/DTMF probe records for one D-STAR frame decoded by the chip: a 7-tuple of `u32` in this order: `b0`, the ordinary-speech `b1`, the ordinary-speech `b2`, the real tone index, the real tone volume (meaningful only when `b0` is a tone frame, see `ambe_dstar::decode::decode_tone`), `epsilon_c0`, `epsilon_c1` (the two error counts).

**Definition.** `type DstarToneCapture = (u32, u32, u32, u32, u32, u32, u32);` (tuple alias, example-private).

**Notes.** Produced by the `decode_full` closure, which gives `None` for a frame it cannot decode; collected as `Vec<Option<DstarToneCapture>>`. The field order is positional, so a change to it must change every destructuring. Test: the example's `tests` module.

## `Blocks` [@ANCHOR: ham_digital_modes:p25_ratet27_ecmode_bit_sweep_blocks_struct]

**Purpose.** The eight decoded FEC blocks of one P25 wire frame, as the ecmode bit sweep (`p25_ratet27_ecmode_bit_sweep.rs`) sees them for one value of the chip's `ecmode` word: four Golay (23,12) payloads (`g0` to `g3`), three Hamming (15,11) payloads (`u4` to `u6`) and the 7 unprotected raw bits (`c7`); the layout is `Block` in `wire_format.rs`.

**Definition.** `#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)] struct Blocks { g0: u16, g1: u16, g2: u16, g3: u8, u4: u16, u5: u16, u6: u16, c7: u16 }`.

| Field | Type | Meaning |
|---|---|---|
| `g0`, `g1`, `g2` | `u16` | Decoded Golay (23,12) blocks 0 to 2. |
| `g3` | `u8` | Decoded Golay block 3, narrowed to `u8` by the example (`as u8`). |
| `u4`, `u5`, `u6` | `u16` | Decoded Hamming (15,11) blocks 4 to 6. |
| `c7` | `u16` | The 7 unprotected raw bits, taken as they are. |

**Notes.** `Default` is all zero; `Copy` and `Eq` let the sweep compare the frames of two `ecmode` values for equality. One value per frame, returned as a `Vec<Blocks>` by `run_with_ecmode`. Test: the example's `tests` module.

## `Row` [@ANCHOR: ham_digital_modes:ratet27_diagnose_gain_scale_mismatch_row_struct]

**Purpose.** One analysed frame in `ratet27_diagnose_gain_scale_mismatch.rs`, which compares the chip's decoder output level with this crate's float decoder over a capture: the two RMS levels and their ratio, the float decoder's parameters for the frame and the chip-side header bytes. The tables and the louder/quieter groups (ratio above 2.0, below 0.7) are built from the rows.

**Definition.** `struct Row { idx, chip_rms, float_rms, ratio, b2, l_hat, voiced_frac, r_m0, outcome, header_bytes, u0_bits5_3 }`, module level (it used to be declared inside `main`).

| Field | Type | Meaning |
|---|---|---|
| `idx` | `usize` | Frame number in the capture. |
| `chip_rms`, `float_rms` | `f64` | RMS of the chip's and of the float decoder's PCM (pulse-code modulation) for the frame (sample units). |
| `ratio` | `f64` | `chip_rms / float_rms`. |
| `b2` | `u32` | Quantised gain index of the float decoder; 0 unless decoded. |
| `l_hat` | `u32` | Number of harmonics; 0 unless decoded. |
| `voiced_frac` | `f64` | Fraction of voiced bands, 0.0 to 1.0. |
| `r_m0` | `f64` | Energy of the reconstructed amplitudes. |
| `outcome` | `&'static str` | `Decoded`, `Repeat`, `Mute` or `None`. |
| `header_bytes` | `Vec<u8>` | The chip's channel-payload bytes before the 18 data bytes. |
| `u0_bits5_3` | `u32` | Bits 5 to 3 of the first decoded Golay block. |

**Lifetime.** Built once per frame in `main`, kept in a `Vec<Row>`, read only. Test: the example's `tests` module.

## `Chip` [@ANCHOR: ham_digital_modes:ratet27_probe_bit_layout_chip_struct]

**Purpose.** A handle on the DVSI chip's UDP channel for `ratet27_probe_bit_layout.rs`, with the request buffer and the RATEP header bytes the chip expects in front of the 18 channel bytes. `decode_repeated(&[u32; 8])` sends one set of channel parameters `REPS` (8) times and returns the last two decoded PCM frames (as `Vec<Vec<f64>>` of `FRAME_SAMPLES` = 160 samples), so the chip's decoder history has settled.

**Definition.** `struct Chip<'a> { sock: &'a UdpSocket, buf: [u8; 1024], header: Vec<u8> }`.

| Field | Type | Meaning |
|---|---|---|
| `sock` | `&UdpSocket` | A connected socket to the chip; borrowed, not closed by the handle. |
| `buf` | `[u8; 1024]` | Receive buffer reused for every reply. |
| `header` | `Vec<u8>` | Header bytes probed from a real chip response, copied in front of the wire bytes. |

**Special concerns.** Blocking: each send waits for a reply (retrying up to 8 times with 200 ms pauses, then panics); a non-speech reply is printed and counted as a silent frame. Single-threaded use. Test: the example's `tests` module runs it against a fake chip on loopback.
