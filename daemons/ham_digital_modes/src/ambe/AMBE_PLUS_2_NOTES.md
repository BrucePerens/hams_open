# AMBE+2 (DMR / Yaesu System Fusion generation) -- documentation only, NOT IMPLEMENTED

Per `docs/proposals/AMBE_CODEC_AND_DSTAR_IMPLEMENTATION_PLAN.md`: DMR and Yaesu System Fusion both use
AMBE+2, the half-rate codec covered by the 2009 addendum to TIA-102.BABA-1, which names 12 specific
patents. This file exists to record what's known about how AMBE+2 differs from the AMBE generation
`ambe/mod.rs` implements (D-STAR's own, from the 2003 base standard), so a future implementer doesn't
have to re-derive that context from scratch -- **it is not an invitation to implement AMBE+2**. Do not
add AMBE+2 encode/decode code to this crate until the patent-clearance question in the plan document
above has been revisited and resolved.

## 2026-09-07: the half-rate addendum's own Annexes extracted and verified

Bruce's own direct instruction: extract the tables from the half-rate annex (TIA-102.BABA-1's own
Annexes A through J) and record them here. Applied the same "confirm it's real text, parse
programmatically, check completeness/invariants" discipline `tables.rs`'s own doc comments describe
for the base standard -- this document is real reference data, not a placeholder, even though nothing
here gets built into code.

**A real, useful surprise found first**: unlike the 2003 base standard's own custom Type 3 font
encoding (`fec.rs`'s own doc comment covers that puzzle), this half-rate PDF uses ordinary, standard
embedded fonts (Computer Modern / TeX fonts: `CMR10`, `CMMI10`, `CMSY10`, plus `NimbusSanL`) -- its
plain extracted text (via PyMuPDF's `get_text()`, no custom decoding needed) is already clean, correct,
and in genuine linear reading order, page after page, with no watermark or layout-reconstruction
interference at all. Every table below was extracted this way and checked for completeness (every
index present exactly once, no gaps, no duplicate-conflicting values) before being trusted.

### Real structural differences from the base standard, confirmed by direct comparison

- **Only 4 frequency blocks (Annex C), not 6** (base standard's Annex J). `J1+J2+J3+J4 == L` for every
  row, `J1<=J2<=J3<=J4` -- the same shape of invariant as the base standard's own block-length table,
  just a smaller partition.
- **Smaller quantizer index widths throughout**: the gain quantizer (Annex D) is 32 entries/5 bits here
  vs. the base standard's 64 entries/6 bits (Annex E); higher-order coefficient quantizers (Annex G)
  run from 32 entries (`b5`) down to just 8 (`b8`), vs. the base standard's larger, more uniform Annex
  G. Consistent with half-rate needing to fit the same information into roughly half the bits.
- **A real new mechanism absent from the base standard entirely: vector quantization.** Annexes E and F
  (`PRBA24`/`PRBA58`) quantize *multiple* spectral-amplitude-related coefficients jointly as vectors
  (3 and 4 dimensions respectively) against a trained codebook, rather than the base standard's
  scalar-per-coefficient approach throughout. This is a materially different, more modern coding
  technique -- likely a real driver of AMBE+2's improved quality-per-bit over the original AMBE, and a
  genuinely separate algorithm class from anything `ambe/tables.rs` currently implements.
- **A tone-frame mode with its own dedicated parameter table (Annex J)**, entirely absent from the base
  standard covered by `ambe/mod.rs` -- a special encoding for pure/near-pure tones (DTMF-like signals,
  cues), not a speech-model feature at all.

### Annex A: Fundamental Frequency Quantization Table (`b0` -> `L`, `omega0`)

120 entries (`b0` 0-119, 7-bit index). `omega0` strictly decreasing and `L` non-decreasing
as `b0` increases -- both confirmed during extraction.

```
  b0   L     omega0
   0   9   0.049971      1   9   0.049215      2   9   0.048471
   3   9   0.047739      4   9   0.047010      5   9   0.046299
   6  10   0.045601      7  10   0.044905      8  10   0.044226
   9  10   0.043558     10  10   0.042900     11  10   0.042246
  12  11   0.041609     13  11   0.040979     14  11   0.040356
  15  11   0.039747     16  11   0.039148     17  11   0.038559
  18  12   0.037971     19  12   0.037399     20  12   0.036839
  21  12   0.036278     22  12   0.035732     23  13   0.035198
  24  13   0.034672     25  13   0.034145     26  13   0.033636
  27  13   0.033133     28  14   0.032635     29  14   0.032148
  30  14   0.031670     31  14   0.031122     32  15   0.030647
  33  15   0.030184     34  15   0.029728     35  15   0.029272
  36  16   0.028831     37  16   0.028395     38  16   0.027966
  39  16   0.027538     40  17   0.027122     41  17   0.026712
  42  17   0.026304     43  17   0.025906     44  18   0.025515
  45  18   0.025129     46  18   0.024746     47  18   0.024372
  48  19   0.024002     49  19   0.023636     50  19   0.023279
  51  20   0.022926     52  20   0.022581     53  20   0.022236
  54  21   0.021900     55  21   0.021570     56  21   0.021240
  57  22   0.020920     58  22   0.020605     59  22   0.020294
  60  23   0.019983     61  23   0.019684     62  23   0.019386
  63  24   0.019094     64  24   0.018805     65  24   0.018520
  66  25   0.018242     67  25   0.017965     68  26   0.017696
  69  26   0.017431     70  26   0.017170     71  27   0.016911
  72  27   0.016657     73  28   0.016409     74  28   0.016163
  75  29   0.015923     76  29   0.015686     77  30   0.015411
  78  30   0.015177     79  30   0.014946     80  31   0.014721
  81  31   0.014496     82  32   0.014277     83  32   0.014061
  84  33   0.013847     85  33   0.013636     86  34   0.013430
  87  34   0.013227     88  35   0.013025     89  36   0.012829
  90  36   0.012634     91  37   0.012444     92  37   0.012253
  93  38   0.012068     94  38   0.011887     95  39   0.011703
  96  40   0.011528     97  40   0.011353     98  41   0.011183
  99  42   0.011011    100  42   0.010845    101  43   0.010681
 102  43   0.010517    103  44   0.010359    104  45   0.010202
 105  46   0.010050    106  46   0.009895    107  47   0.009747
 108  48   0.009600    109  48   0.009453    110  49   0.009312
 111  50   0.009172    112  51   0.009033    113  52   0.008896
 114  52   0.008762    115  53   0.008633    116  54   0.008501
 117  55   0.008375    118  56   0.008249    119  56   0.008125
```

### Annex B: V/UV Quantization Vectors (`b1` -> 8-bit voicing vector `v0..v7`)

32 entries (`b1` 0-31, 5-bit index).

```
 b1  v0 v1 v2 v3 v4 v5 v6 v7
  0  1  1  1  1  1  1  1  1
  1  1  1  1  1  1  1  1  1
  2  1  1  1  1  1  1  1  0
  3  1  1  1  1  1  1  1  1
  4  1  1  1  1  1  1  0  0
  5  1  1  0  1  1  1  1  1
  6  1  1  1  0  1  1  1  1
  7  1  1  1  1  1  0  1  1
  8  1  1  1  1  0  0  0  0
  9  1  1  1  1  1  0  0  0
 10  1  1  1  0  0  0  0  0
 11  1  1  1  0  0  0  0  1
 12  1  1  0  0  0  0  0  0
 13  1  1  1  0  0  0  0  0
 14  1  0  0  0  0  0  0  0
 15  1  1  1  0  0  0  0  0
 16  0  0  0  0  0  0  0  0
 17  0  0  0  0  0  0  0  0
 18  0  0  0  0  0  0  0  0
 19  0  0  0  0  0  0  0  0
 20  0  0  0  0  0  0  0  0
 21  0  0  0  0  0  0  0  0
 22  0  0  0  0  0  0  0  0
 23  0  0  0  0  0  0  0  0
 24  0  0  0  0  0  0  0  0
 25  0  0  0  0  0  0  0  0
 26  0  0  0  0  0  0  0  0
 27  0  0  0  0  0  0  0  0
 28  0  0  0  0  0  0  0  0
 29  0  0  0  0  0  0  0  0
 30  0  0  0  0  0  0  0  0
 31  0  0  0  0  0  0  0  0
```

### Annex C: Log Magnitude Prediction Residual Block Lengths (`L` -> `J1..J4`)

48 entries (`L` 9-56). Only 4 frequency blocks here, vs. the base standard's 6 -- a real
structural difference (see above). `J1+J2+J3+J4 == L` for every row, and `J1<=J2<=J3<=J4` --
both confirmed during extraction, the same invariant shape as the base standard's own Annex J.

```
  L  J1  J2  J3  J4
  9   2   2   2   3
 10   2   2   3   3
 11   2   3   3   3
 12   2   3   3   4
 13   3   3   3   4
 14   3   3   4   4
 15   3   3   4   5
 16   3   4   4   5
 17   3   4   5   5
 18   4   4   5   5
 19   4   4   5   6
 20   4   4   6   6
 21   4   5   6   6
 22   4   5   6   7
 23   5   5   6   7
 24   5   5   7   7
 25   5   6   7   7
 26   5   6   7   8
 27   5   6   8   8
 28   6   6   8   8
 29   6   6   8   9
 30   6   7   8   9
 31   6   7   9   9
 32   6   7   9  10
 33   7   7   9  10
 34   7   8   9  10
 35   7   8  10  10
 36   7   8  10  11
 37   8   8  10  11
 38   8   9  10  11
 39   8   9  11  11
 40   8   9  11  12
 41   8   9  11  13
 42   8   9  12  13
 43   8  10  12  13
 44   9  10  12  13
 45   9  10  12  14
 46   9  10  13  14
 47   9  11  13  14
 48  10  11  13  14
 49  10  11  13  15
 50  10  11  14  15
 51  10  12  14  15
 52  10  12  14  16
 53  11  12  14  16
 54  11  12  15  16
 55  11  12  15  17
 56  11  13  15  17
```

### Annex D: Gain Quantizer Levels (`b2` -> `delta_gamma`)

32 entries (`b2` 0-31, 5-bit index -- half the base standard's 64-entry, 6-bit Annex E).
Strictly increasing, confirmed during extraction.

```
 b2  delta_gamma
  0    -2.000000
  1    -0.670000
  2     0.297941
  3     0.663728
  4     1.036829
  5     1.438136
  6     1.890077
  7     2.227970
  8     2.478289
  9     2.667544
 10     2.793619
 11     2.893261
 12     3.020630
 13     3.138586
 14     3.237579
 15     3.322570
 16     3.432367
 17     3.571863
 18     3.696650
 19     3.814917
 20     3.920932
 21     4.022503
 22     4.123569
 23     4.228291
 24     4.370569
 25     4.543700
 26     4.707695
 27     4.848879
 28     5.056757
 29     5.326468
 30     5.777581
 31     6.874496
```

### Annex E: PRBA24 Vector Quantizer Levels (`b3` -> 3-dim vector `G2,G3,G4`)

512 entries (`b3` 0-511, 9-bit index), each a 3-dimensional codeword -- a real vector quantizer, not
a scalar table (see "real structural differences" above). Full 512x3 data (1536 numbers) is too large
for this notes file to stay readable inline; extracted to `ambe/half_rate_reference/
half_rate_prba24_b3.csv` (header: `b3,G2,G3,G4`, with its own leading `#`-comment header naming its
source/structure), completeness-checked (all 512 indices present exactly once) before being trusted.

### Annex F: PRBA58 Vector Quantizer Levels (`b4` -> 4-dim vector `G5,G6,G7,G8`)

128 entries (`b4` 0-127, 7-bit index), each a 4-dimensional codeword. Extracted to
`ambe/half_rate_reference/half_rate_prba58_b4.csv` (header: `b4,G5,G6,G7,G8`), same completeness
check.

### Annex G: Quantization Tables for Higher Order Coefficients (`b5`..`b8` -> 4-dim vectors)

Four separate vector-quantizer sub-tables, one per higher-order coefficient group, each 4-dimensional:

| Index | Entries | Bits | Dims | CSV |
|---|---|---|---|---|
| `b5` | 32 | 5 | `H1,1..H1,4` | `half_rate_reference/half_rate_higher_order_b5.csv` |
| `b6` | 16 | 4 | `H2,1..H2,4` | `half_rate_reference/half_rate_higher_order_b6.csv` |
| `b7` | 16 | 4 | `H3,1..H3,4` | `half_rate_reference/half_rate_higher_order_b7.csv` |
| `b8` | 8  | 3 | `H4,1..H4,4` | `half_rate_reference/half_rate_higher_order_b8.csv` |

All four in `ambe/half_rate_reference/` (alongside this notes file's own directory, not in
`hams_com`'s `reference/` -- moved there so the real codec code and its own reference data for the
generation it describes live in the same repository), same completeness check (all indices present
exactly once, per table) applied before being trusted. Each CSV carries its own leading `#`-comment
header naming its annex, structure, and the standing "not implemented, pending patent clearance"
scope, so a reader opening one directly (not via this notes file) still gets the same context.

### Annex H: Bit Frame Format (`symbol` -> codeword bit sources for Bit 1 / Bit 0)

36 symbols (0-35), each carrying 2 bits drawn from codewords `c0..c3` -- e.g. `c0(23)` means
bit 23 of codeword `c0`. 72 total bit positions.

```
symbol      Bit1      Bit0
     0    c0(23)     c0(5)
     1    c1(10)     c2(3)
     2    c0(22)     c0(4)
     3     c1(9)     c2(2)
     4    c0(21)     c0(3)
     5     c1(8)     c2(1)
     6    c0(20)     c0(2)
     7     c1(7)     c2(0)
     8    c0(19)     c0(1)
     9     c1(6)    c3(13)
    10    c0(18)     c0(0)
    11     c1(5)    c3(12)
    12    c0(17)    c1(22)
    13     c1(4)    c3(11)
    14    c0(16)    c1(21)
    15     c1(3)    c3(10)
    16    c0(15)    c1(20)
    17     c1(2)     c3(9)
    18    c0(14)    c1(19)
    19     c1(1)     c3(8)
    20    c0(13)    c1(18)
    21     c1(0)     c3(7)
    22    c0(12)    c1(17)
    23    c2(10)     c3(6)
    24    c0(11)    c1(16)
    25     c2(9)     c3(5)
    26    c0(10)    c1(15)
    27     c2(8)     c3(4)
    28     c0(9)    c1(14)
    29     c2(7)     c3(3)
    30     c0(8)    c1(13)
    31     c2(6)     c3(2)
    32     c0(7)    c1(12)
    33     c2(5)     c3(1)
    34     c0(6)    c1(11)
    35     c2(4)     c3(0)
```

### Annex J: Tone Frame Parameters (`ID` -> `f0`, `l1`, `l2`)

50 rows. A special encoding mode for pure/near-pure tones, absent from the base standard entirely
(see "real structural differences" above). IDs 0-4, 123-127, and 164-254 are N/A (reserved); IDs
5-122 are formula-driven ranges (`f0 = K * ID` for a per-range constant `K`, with fixed `l1`/`l2`);
IDs 128-163 are individually tabulated tones; ID 255 is a fixed special tone.

```
        ID          f0    l1    l2
     0 - 4         N/A   N/A   N/A
    5 - 12    31.250ID     1     1
   13 - 25    15.625ID     2     2
   26 - 38    10.417ID     3     3
   39 - 51    7.8125ID     4     4
   52 - 64    6.2500ID     5     5
   65 - 76    5.2803ID     6     6
   77 - 89    4.4643ID     7     7
  90 - 102    3.9063ID     8     8
 103 - 115    3.4722ID     9     9
 116 - 122    3.1250ID    10    10
 123 - 127         N/A   N/A   N/A
       128        78.5    12    17
       129      173.48     4     7
       130        70.0    10    19
       131        87.0     8    17
       132      109.95     7    11
       133      191.68     4     7
       134       70.17    11    21
       135       71.06    12    17
       136      121.58     7    11
       137       212.0     4     7
       138      116.41     6    14
       139       96.15     8    17
       140        71.0    12    23
       141      234.26     4     7
       142      134.38     7     9
       143      134.35     7    11
       144       68.33    12    17
       145      150.89     4     7
       146       67.82     9    17
       147        86.5     7    15
       148       95.79     7    11
       149      166.92     4     7
       150        67.7    10    19
       151       74.74    10    14
       152      105.90     7    11
       153       92.78     8    14
       154      101.55     6    14
       155       84.02     8    17
       156       67.83    11    21
       157       102.3     8    14
       158       117.0     7     9
       159      117.49     7    11
       160       87.78     4     5
       161       70.83     6     7
       162       122.0     4     5
       163        70.0     5     7
 164 - 254         N/A   N/A   N/A
       255       250.0     0     0
```

## Real next step, if this is ever picked up

Read the 2009 addendum's own encoder/decoder text (sections, not just annexes) with the same
implementation-focused care `ambe/mod.rs`'s own pitch-estimation read applied to the base standard --
not attempted here, since this pass was scoped to the tables Bruce specifically asked for. In
particular, the vector-quantization mechanism (Annexes E/F/G) needs its own real algorithm description
(nearest-codeword search, most likely, but confirm rather than assume) before any of this becomes
buildable, separate from the patent-clearance gate this whole file already exists to enforce.
