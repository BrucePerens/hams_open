# `codec2_1600` test fixtures

Reference data for cross-validating `src/codec2_1600/`'s independent Rust
implementation against plain upstream Codec2 (not `vendor/codec2-mod`,
which has been stripped down to 3200bps only and has no 1600bps mode at
all).

- `synthetic_c_encoded_bits.bin`, `synthetic_c_decoded_pcm.bin`: a locally
  synthesized non-speech signal (swept sinusoid plus deterministic
  pseudo-noise, alternating ~1s voiced-dominant / ~1s noisy blocks so the
  fixture exercises both excitation paths), run through a local unmodified
  build of plain upstream Codec2's own `c2enc 1600` / `c2dec 1600`.
  `synthetic_c_encoded_bits.bin` is 200 real packed frames (raw bytes, 8
  bytes each, back-to-back, 40ms/frame); `synthetic_c_decoded_pcm.bin` is
  the real reference decoder's own real PCM output for them (raw
  little-endian i16, no WAV header). Used by
  `codec2_1600::tests::decoder_matches_the_real_reference_decoder_on_a_real_captured_synthetic_signal_bitstream`
  to check `Decoder`'s real output against the real reference decoder's
  own, automatically and reproducibly. Same "synthetic, not real donated
  speech" reasoning `tests/fixtures/codec2_3200/README.md` documents for
  its own equivalent fixture: the reference decoder's own PCM output of
  real speech is still recognizable speech even lossily coded, which a
  synthesized non-speech signal has no such concern about.

## Regenerating

Build plain upstream Codec2 (`cmake` + the `c2enc`/`c2dec` targets --
`vendor/codec2-mod` won't work here, it has no 1600bps mode), synthesize
the same swept-sinusoid-plus-noise signal (any 8kHz mono 16-bit raw PCM,
a multiple of 320 samples so it divides evenly into 40ms frames), then:

```
c2enc 1600 synthetic.raw synthetic_c_encoded_bits.bin
c2dec 1600 synthetic_c_encoded_bits.bin synthetic_c_decoded_pcm.bin
```
