# Story: finding a symbol clock before choosing a decoder (ham_digital_modes)

The timing characterizer answers "does this audio have a periodic symbol clock, and at what baud rate?" without
decoding as any particular mode, so an auto-tune bank can skip candidate frequencies that carry no keyed signal.
It works on a scalar feature series derived from the audio.

## `two_tone_ratio_feature` [@ANCHOR: two_tone_ratio_feature]

`two_tone_ratio_feature` makes that series for two-tone FSK such as RTTY. For each hop it correlates a window
against both tones and reports the ratio of their energies in dB (positive when tone A is stronger). The window
must be long enough to tell the tones apart; the hop should be much shorter than a symbol, around a
sixteenth to an eighth of the window. Zero window, hop or sample rate returns an empty series.

| Argument | Type | Meaning |
|---|---|---|
| `samples` | `&[i16]`, borrowed | Mono PCM, normalised internally by `i16::MAX`. |
| `tone_a_hz`, `tone_b_hz` | `f64` | The two keyed tones in hertz. |
| `sample_rate` | `u32` | Samples per second; zero gives an empty result. |
| `window_len` | `usize` | Samples correlated per output value; long enough to separate the tones. |
| `hop_len` | `usize` | Stride between windows; keep it well under one symbol. |

**Returns:** `Vec<f64>`, one value per hop, `10 log10(energy A / energy B)` in decibels with both energies floored at 1e-15 so silence gives 0 rather than infinity. **Errors:** none. **Special concerns:** pure; cost is `4 * window_len` multiply-adds per output. **See also:** `dominant_symbol_period`, tests tagged `two_tone_ratio_feature`.

*(Reference: `src/timing_characterizer.rs`)*

## `envelope_feature` [@ANCHOR: envelope_feature]

`envelope_feature` makes the series for on-off keyed signals (CW, Hellschreiber): the RMS level of each
consecutive frame of `frame_len` samples, dropping a trailing partial frame. The frame should be much shorter
than the shortest keyed element.

**Call interface:** `pub fn envelope_feature(samples: &[i16], frame_len: usize) -> Vec<f64>`; `samples` is borrowed mono PCM and `frame_len` the samples per frame (zero returns an empty vector). **Returns:** one RMS level per whole frame, on the 0 to 1 scale of full-scale PCM, so the series rate is `sample_rate / frame_len`. **Errors:** none. **Special concerns:** pure and linear-time. The result feeds `dominant_symbol_period`. **See also:** tests tagged `envelope_feature`.

*(Reference: `src/timing_characterizer.rs`)*

## `dominant_symbol_period` [@ANCHOR: dominant_symbol_period]

`dominant_symbol_period` finds the clock. It autocorrelates the first difference of the series at lags that
correspond to baud rates between the given minimum and maximum, and returns the rate with the strongest
correlation magnitude together with a confidence: that magnitude divided by the mean over all candidates.
Differencing means only transitions count, so a constant offset in the feature does not create a false clock.
It returns `None` for a series shorter than four values, a non-positive rate, an empty or reversed baud range, a
search range longer than the series, or a series with no variation at all.

| Argument | Type | Meaning |
|---|---|---|
| `feature` | `&[f64]`, borrowed | Scalar series from `two_tone_ratio_feature` or `envelope_feature`. |
| `feature_rate_hz` | `f64` | Samples of the series per second (audio rate divided by the hop or frame length). |
| `min_baud`, `max_baud` | `f64` | Range of symbol rates to test; must satisfy `0 < min_baud < max_baud`. |

**Returns:** `Option<SymbolClock>`, where `SymbolClock { baud, confidence }` holds the best rate and the ratio of the winning lag's autocorrelation magnitude to the mean over all lags (near 1 for noise, clearly larger for a real clock; only comparable with a noise measurement made the same way). **Errors:** none; the `None` cases are listed above. **Special concerns:** pure; cost is the series length times the number of candidate lags, so keep the baud range narrow. **See also:** `autocorrelation_profile`, which it is built on, and tests tagged `dominant_symbol_period`.

*(Reference: `src/timing_characterizer.rs`)*
