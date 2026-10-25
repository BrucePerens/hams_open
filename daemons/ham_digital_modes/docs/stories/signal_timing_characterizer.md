# Story: finding a symbol clock before choosing a decoder (ham_digital_modes)

The timing characterizer answers "does this audio have a periodic symbol clock, and at what baud rate?" without
decoding as any particular mode, so an auto-tune bank can skip candidate frequencies that carry no keyed signal.
It works on a scalar feature series derived from the audio.

`two_tone_ratio_feature` makes that series for two-tone FSK such as RTTY. For each hop it correlates a window
against both tones and reports the ratio of their energies in dB (positive when tone A is stronger). The window
must be long enough to tell the tones apart; the hop should be much shorter than a symbol, around a
sixteenth to an eighth of the window. Zero window, hop or sample rate returns an empty series.
*(Reference: `src/timing_characterizer.rs` -> `[@ANCHOR: two_tone_ratio_feature]`)*

`envelope_feature` makes the series for on-off keyed signals (CW, Hellschreiber): the RMS level of each
consecutive frame of `frame_len` samples, dropping a trailing partial frame. The frame should be much shorter
than the shortest keyed element.
*(Reference: `src/timing_characterizer.rs` -> `[@ANCHOR: envelope_feature]`)*

`dominant_symbol_period` finds the clock. It autocorrelates the first difference of the series at lags that
correspond to baud rates between the given minimum and maximum, and returns the rate with the strongest
correlation magnitude together with a confidence: that magnitude divided by the mean over all candidates.
Differencing means only transitions count, so a constant offset in the feature does not create a false clock.
It returns `None` for a series shorter than four values, a non-positive rate, an empty or reversed baud range, a
search range longer than the series, or a series with no variation at all.
*(Reference: `src/timing_characterizer.rs` -> `[@ANCHOR: dominant_symbol_period]`)*
