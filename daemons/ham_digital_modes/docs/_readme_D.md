## PSK31 receive internals

`char_to_code` looks up the Varicode codeword and bit count for an ASCII byte below 128 and gives nothing for
anything higher, which is how the encoder skips non-ASCII input.
[@ANCHOR: char_to_code]

`correlate_symbol_iq` correlates one symbol's worth of samples against a cosine and a sine at the carrier and
returns the two sums (I, Q). Each sample is weighted by the same raised-cosine envelope the modulator applied,
and the carrier phase is computed from the absolute sample index of the stream (`sample_idx_start` plus the
position in the chunk), so consecutive calls stay phase-continuous. Both the whole-buffer demodulator and the
streaming decoder call it, so the constellation points shown to the operator are the numbers the bits were
decided from.
[@ANCHOR: correlate_symbol_iq]

`phase_to_bit` makes the BPSK31 bit decision: the phase change from the previous symbol, wrapped to
[-pi, pi], decides 1 if its magnitude is under pi/2 and 0 otherwise. With no previous symbol it returns 1,
which is correct because every codeword starts with a 1 and the modulator starts at zero phase.
[@ANCHOR: phase_to_bit]

`VaricodeAccumulator::push_bit` is the streaming form of the boundary logic in `psk31_decode_bits`. It takes one
bit, and on the second consecutive zero it drops the first gap zero from the accumulator, looks the codeword
up, resets, and returns the character if it matched. Otherwise it returns nothing. As in the batch decoder, a
run beyond 11 accumulated bits resets the accumulator.
[@ANCHOR: VaricodeAccumulator::push_bit]

## RTTY framing and synthesis internals

`push_framed_char` appends one character's seven bit periods to a bit vector: a SPACE start bit, the five
data bits least significant first (true is MARK), and a MARK stop bit. The stop bit is one entry here; the
audio stage stretches it to 1.5 bit periods.
[@ANCHOR: push_framed_char]

`framed_bits_to_audio` is the FSK synthesis behind `rtty_modulate`, kept separate so tests can feed it bit
sequences that the text path would never produce. MARK is `mark_hz` and SPACE is `mark_hz + 170`. The phase
accumulator runs across all bits so the waveform is continuous, and each bit lasts `sample_rate / 45.45`
samples, rounded per bit. Every seventh bit (index modulo 7 equal to 6) is taken to be a stop bit and is
lengthened to 1.5 times, so it assumes the input is built from 7-bit frames.
[@ANCHOR: framed_bits_to_audio]

## RTTY correlators

`window_mark_space_energy` is the slow reference correlator. For one window it computes the quadrature
(I and Q) correlation against the MARK and SPACE tones, recomputing sine and cosine for every sample, and
returns whether MARK is stronger together with both energies (I squared plus Q squared). It is not used on the
decode path; the tests compare it with the table-driven version.
[@ANCHOR: window_mark_space_energy]

`TrigTables::new` precomputes cosine and sine values for the MARK and SPACE tones at window-relative sample
offsets 0 to `capacity`. The phase always counts from the window's first sample, so the tables serve a window
starting anywhere in the stream.
[@ANCHOR: TrigTables::new]

`TrigTables::window_evidence` correlates `samples[start..start+len]` against the tables and returns the
signed MARK-to-SPACE ratio in dB (positive means MARK is stronger; energies are floored at 1e-15) together
with the linear total MARK plus SPACE energy. It returns `None` if `len` is zero, the start is past the end,
or more than 10 percent of the window is missing. A window short by up to 10 percent, and one longer than the
tables, is shortened to what is available; this lets the last character of a whole-buffer decode through.
[@ANCHOR: TrigTables::window_evidence]

`TrigTables::signed_ratio_db` is `window_evidence` with the energy dropped, for the many callers that need only
the ratio.
[@ANCHOR: TrigTables::signed_ratio_db]

`TrigBank::new_with_offsets` builds one `TrigTables` per carrier offset in the given list, each with MARK at
the configured frequency plus that offset and SPACE 170 Hz above it. It keeps the offset list so a decoder
can report the real offset of the table that won.
[@ANCHOR: TrigBank::new_with_offsets]

`TrigBank::new` is that bank over the standard nine offsets, -40 to +40 Hz in 10 Hz steps.
[@ANCHOR: TrigBank::new]

## RTTY edge detection and scan

`pre_is_persistently_mark` checks that the bit period just before a candidate start edge is steady MARK: it
splits that period into two sub-windows and requires each to read more than 4 dB toward MARK. This rejects
candidates whose context is not an idle or previous stop bit. An edge within one bit period of the start of the
buffer passes without a test.
[@ANCHOR: pre_is_persistently_mark]

`frame_score` rates a candidate start edge using the whole 7-bit frame, after the persistence check. It
correlates the start bit window, five data bit windows and the 1.5-bit stop window, each from the table passed
in. It returns three numbers: an accept score, the sum of the start (SPACE-ward), stop (MARK-ward) and
five data-bit ratios, each clipped to 18 dB, which decides whether a character is there; a raw unclipped
start-plus-stop score, used to find the exact edge position; and the total linear energy, used to choose
between frequency candidates. It returns `None` if the persistence check fails or any window runs past the
buffered audio.
[@ANCHOR: frame_score]

`best_frame_evidence` evaluates `frame_score` at one position for every table in the bank and keeps only those
whose accept score is above 80 dB. Of those it returns the one with the greatest linear energy, with its index.
The energy chooses the winner because a clipped score saturates and cannot tell neighbouring offsets apart.
[@ANCHOR: best_frame_evidence]

`ScanState::new` creates the scan state for a decoder: position zero, letters shift, no non-digit figures run,
and the given initial locked-offset index (the default state uses the middle, 0 Hz, entry).
[@ANCHOR: ScanState::new]

`rtty_scan` is the loop shared by the whole-buffer and streaming decoders. It steps through the buffer in
steps of one eighth of a bit period. At each position `best_frame_evidence` is tried against the full
offset bank. When it finds a candidate, the table is fixed for that character, and the position is refined
over plus or minus half a bit by taking the highest raw score. `attempt_character` then makes the hard decision,
and on success the decoder emits the text, records the winning offset and jumps past the character. A failed
attempt just moves on one step. In streaming mode it stops, leaving the position unchanged, when the
character plus the refinement margin is not fully buffered yet, so the next call retries the same position. A
zero-length bit window returns nothing.
[@ANCHOR: rtty_scan]

## RTTY presence signature

`characterize_rtty_signature` is a cheap, decode-free probe that is not wired into any decoder. It cuts the
audio into whole one-bit-period hops and takes the sign of the MARK/SPACE ratio of each, and reports the
fraction of adjacent hops that differ and a histogram of run lengths (runs of 8 hops or more share the last
bucket). It returns `None` with fewer than two hops or when the sample rate is too low for one sample per bit.
Measurement showed that keyed RTTY and noise both flip about half the time, so the one case it separates cleanly
is a steady unkeyed carrier, which never flips.
[@ANCHOR: characterize_rtty_signature]

## FT8 waveform internals

`gfsk_pulse` is the Gaussian-smoothed frequency pulse for one FT8 symbol, with a bandwidth-time product of
2.0: half the difference of two error functions, `(erf(k*BT*(t+0.5)) - erf(k*BT*(t-0.5))) / (2T)`, where t is
in symbol periods, T is the symbol period in seconds and k is pi times the square root of 2 over ln 2. It
integrates to 1 over time, so a symbol's frequency offset adds up to the tone number.
[@ANCHOR: gfsk_pulse]
