# The DVSI chip's noise generator: what is known (2026-09-19)

Measured on the AMBE-3000R chip in its AMBE+2 half-rate setting (`RATET` 33), decoder only. Reproduce with
`examples/ambe_chip_noise_determinism.rs`, `ambe_chip_noise_structure.rs` and `ambe_chip_comfort_noise.rs`
(all need `--features ambe_plus_2` and the chip's UDP bridge address as the first argument).

## What the documentation says

The AMBE-3000R Users Manual (version 1.4, `hams_com/docs/references/dvsi_ambe/`) says nothing about a random
number generator, a seed, a synchronization mode, deterministic output or test vectors. It documents comfort noise
only as "the decoder synthesizes noise from a transmitted level". The one relevant packet is `PKT_INIT` (field
`0x0B`; bit 0 encoder, bit 1 decoder, bit 2 echo canceller), described only as initializing mode flags.
TIA-102.BABA defines its own generator (`u(n+1) = (171 u(n) + 11213) mod 53125`, `u(-105) = 3147`) and, for the
half-rate addendum, uniform noise in [-5, 5] for muted frames; TIA-102.BABC (reference test) does not require
bit-exact output and has no synchronization vectors.

## Findings

1. **`PKT_INIT` with the decoder flag synchronizes the noise.** After a normal reconfiguration the unvoiced noise
   differs from run to run (0 of 6 frames identical). After `PKT_INIT` (data byte `0x02`, `0x03` or `0x07`) sent
   before each run, the output is bit-identical (6 of 6 frames, and 40-900 frame runs, maximum difference 0).
2. **One noise sequence serves every parameter set.** For unvoiced frames after `PKT_INIT`, changing only the gain
   gives sample correlation 0.996-1.0 with the original; changing the pitch (harmonic count) gives 0.69-0.89 and
   changing the spectral shape 0.72-0.93. Parameters only shape one fixed sequence.
3. **The generator has period exactly 65,536 samples.** Muted output (an invalid pitch code such as `b0 = 124`,
   after the decoder's three repeated frames) was captured for 900 frames (142,400 usable samples). The
   autocorrelation at lag 65,536 is 0.539 (chance level 0.012) and 43,275 exact 12-sample windows repeat at an
   offset of exactly 65,536. So the source is a full-period recurrence over 16 bits, stepped once per output
   sample, and the whole sequence is 16 bits of state: two periods of capture (about 820 frames, 16 seconds of
   audio) contain everything. Lag 32,768 shows a strong negative autocorrelation (-0.38), the usual signature of a
   power-of-two-modulus recurrence.
4. **The muted output is shaped and small.** Values are integers of root mean square about 3.05 (histogram from
   -10 to +8), roughly bell shaped (kurtosis 2.3-2.5 after whitening; uniform would be 1.8, Gaussian 3.0),
   with clear coloring (autocorrelation +0.18 at lag 2, -0.38 at lag 6, band powers uneven by 5x). Linear
   prediction of order 8-32 only reduces the root mean square to 0.76-0.86, so the shaping filter is mild or
   time-varying and the underlying values are not white uniform samples.

## What does not match (so nobody repeats it)

* The TIA generator (`171u + 11213 mod 53125`): circular cross-correlation against every offset of its whole
  period, raw and after whitening: best 0.065 and 0.058 (chance about 0.065 for that search).
* Full-period 16-bit recurrences `s = a s + c mod 65536` with `(a, c)` = (31821, 13849) (ITU G.729 and the ITU
  test-tool library), (25173, 13849), (69069, 1), (4093, 1), (5, 1), (9821, 1), and a 32-bit constant pair
  truncated to 16 bits: best correlation 0.017-0.030 over all 65,536 offsets, raw, with the chip's estimated
  coloring applied to the candidate (an order-16 all-pole filter fitted to the chip stream), and as the sum of
  two consecutive values. Applying the coloring raised the best correlations only marginally (for example 0.021
  to 0.028), all at noise level, so **matching the noise shape did not produce a match.**

## How to get the most usable bits out of the chip

* Bits per sample: the comfort-noise path gives about 3.7 bits of apparent entropy per sample, but the sequence
  is a deterministic function of one 16-bit state, so its true information content is 16 bits. More capture adds
  nothing after two periods. Unvoiced synthesis output has larger samples but is a filtered version of the same
  sequence.
* The cheapest way to the raw table is the muted path after `PKT_INIT`: send an invalid pitch-code frame for
  about 820 frames and fold the capture modulo 65,536 (positions before the fourth frame differ because of the
  repeated-frame fade-in).
* With the full 65,536-sample table the state-to-output map can be studied directly: divide out the coloring
  with a high-order filter fitted to the whole table, then test the residual for the bit-level signature of
  power-of-two recurrences (period 2, 4, 8, ... in the low bits) and for the sum-of-two-uniform hypothesis
  suggested by the kurtosis.

## Status

Algorithm not identified. The tools and the determinism result are enough to continue if the recurrence ever
matters (for example for sample-exact conformance tests of unvoiced synthesis); the previous judgment was that it
does not, because the impact is inaudible phase and about 1.4 dB of average unvoiced level.
