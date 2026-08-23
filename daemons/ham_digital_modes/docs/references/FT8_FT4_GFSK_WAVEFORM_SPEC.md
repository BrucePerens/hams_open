# FT8/FT4 GFSK transmit waveform: protocol facts, cited

**Source:** Steve Franke (K9AN), Bill Somerville (G4WJS), Joe Taylor (K1JT), "The FT4 and FT8
Communication Protocols," *QEX*, July/August 2020, pp. 7-11. Published by WSJT-X's own project at
<https://wsjt.sourceforge.io/FT4_FT8_QEX.pdf> ("Reprinted with permission; copyright ARRL").

This file restates the protocol facts needed to implement FT8's transmit waveform generation --
extracted by reading the paper, not a copy of its text or figures, matching this crate's existing
convention for WSPR/PSK31 (protocol facts belong to nobody; the paper's own exposition does). Read
the actual paper at the URL above for the full derivation, error-correction/message-encoding
sections (already implemented elsewhere in this crate), and the decode-side discussion.

## Why this exists

`ft8.rs`'s `encode_message()` (this session) produces a real 79-symbol tone sequence via the
vendored `ft8_lib`, verified against `ft8sim`'s own reference output. Turning that tone sequence
into an actual transmittable audio waveform was deliberately left unbuilt: getting the pulse-shaping
wrong causes real out-of-band spectral splatter on an actual transmission, and no verifiable primary
source was available in-session to check the exact constant against. This paper is that source.

## The waveform

Both FT4 and FT8 use continuous-phase frequency-shift keying (CPFSK):

```
s(t) = A cos(2*pi*f_c*t + phi(t))
```

`A` is constant-envelope (except during the start/end ramp below -- this is what keeps the signal
free of intermodulation products under nonlinear amplification). `f_c` is the carrier (dial)
frequency. `phi(t)` is the integral of the instantaneous frequency deviation `f_d(t)`:

```
phi(t) = 2*pi * integral_0^t f_d(tau) d(tau)
```

`f_d(t)` is the weighted sum of one pulse per symbol:

```
f_d(t) = h * sum_n( b_n * p(t - n*T) )
```

- `h` = modulation index = **1** for FT8 (and FT4).
- `b_n` = the channel symbol value at position `n` -- for FT8 this is exactly the tone index
  (0..7) `encode_message()` already produces per symbol. No remapping needed.
- `T` = symbol/signaling interval = **0.160 s** for FT8 (0.048 s for FT4). Tone spacing is `1/T`
  = 6.25 Hz for FT8.
- `p(t)` = the frequency-deviation pulse shape, normalized to unit area (`integral p(t) dt = 1`).

## The Gaussian pulse (this is the part that was unverified)

FT8 and FT4 smooth `p(t)` with a Gaussian low-pass filter response instead of using a rectangular
pulse (which is what JT4/JT9/JT65/MSK144 use, and what caused their comparatively wide sidelobes --
Figure 3 in the paper shows FT4's GFSK spectrum next to unfiltered FSK for direct comparison). This
deliberate pulse overlap (inter-symbol interference, on purpose) is what narrows the occupied
bandwidth. The pulse shape, in terms of the error function `erf`:

```
p(t) = (1 / (2*T)) * [ erf(k*B*T*(t/T + 0.5)) - erf(k*B*T*(t/T - 0.5)) ]
```

- `k = pi * sqrt(2 / ln(2)) = 5.336...`
- `B` = the smoothing filter's -3 dB bandwidth.
- **For FT8: `B*T = 2.0`** (i.e. `B = 2/T`). This is the exact constant this session had no
  verified source for -- confirmed here, directly from the protocol's own authors, not guessed.
  (FT4 uses `B*T = 1.0`, a more heavily smoothed pulse -- not needed for FT8 but noted for
  completeness since this crate may implement FT4 later.)
- `erf(x) = (2/sqrt(pi)) * integral_0^x e^(-t^2) dt` -- standard error function, available in any
  competent numerics library (Rust: `libm::erf` or `statrs`, or a direct series/rational
  approximation if avoiding a new dependency).

In the limit `B*T -> infinity` this becomes a rectangular pulse (Figure 1 in the paper plots
`BT=1`, `BT=2`, and `BT=99` overlaid -- `BT=99` is visually indistinguishable from rectangular).

Practically: `p(t)` has non-negligible support outside `[-T/2, T/2]` (that's the whole point --
deliberate ISI). The paper notes: "it is only necessary to include contributions from the pulse
whose center is closest to t and those immediately before and after" -- i.e. a 3-symbol-wide
window (one pulse table spanning `[-1.5T, 1.5T]`) is sufficient to sum at any given output sample;
this matches the general shape of a lookup-table approach, not a per-sample Gaussian recompute.

## Start/end amplitude ramp (a *different*, separate mechanism -- raised cosine, not Gaussian)

Constant envelope A holds throughout the transmission except at the very start and end, which use
a raised-cosine amplitude taper -- explicitly a **different** shaping function from the Gaussian
frequency-deviation pulse above; don't conflate the two:

```
A(t) = 0.5 * (1 - cos(8*pi*t / T)),   0 <= t <= T/8
```

For FT8 this ramp spans the first `T/8` = **20 ms** of the first sync symbol (ramping 0 -> 1), and
the time-reversed form of the same function over the last 20 ms (ramping 1 -> 0). (FT4 handles
this differently -- a dedicated ramp symbol `R` with the same raised-cosine shape applied over its
full 48 ms duration -- not relevant to FT8.)

## Symbol sequence

FT8's 79 transmitted symbols (`b_0 .. b_78`) already come out of `ft8_lib`'s real encoder via
`encode_message()`, including the three 7-tone Costas sync arrays (`3,1,4,0,6,5,2`) at positions
0, 36, and 72 -- this crate doesn't need to reconstruct that placement itself, only synthesize
audio from the tone sequence it's already given.

## Verification strategy

The paper's own equations are the spec; the actual proof that an implementation is correct is a
real round-trip: synthesize a waveform for a known message, feed it into this crate's own
`Ft8Decoder` (already reference-verified against `jt9`), and confirm it decodes back to the
original message -- not just "the math looks like the paper," which round-trip decode makes
unnecessary to trust blindly.
