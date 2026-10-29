---
title: The VNA as a spectrum analyzer
summary: How the VNWA's rudimentary spectrum analyzer works, what was measured, and what is not verified (amplitude, ghost lines, subharmonics).
see_also: [tools/bench_vna_spectrum, instruments/vna/limits, instruments/vna/modulation]
tools: [bench_vna_spectrum]
instrument: vna
verified: 2026-10-29 on the unit with its own tracking generator in loopback; no external calibrated tone was available
---

# The VNA as a spectrum analyzer

The unit's receive mixer is driven by its LO chip; the TX chip sits 12 kHz below it. For an analysis at frequency f the LO is set to f and the receiver converts straight to baseband: whatever the RX port carries within the codec's band of f appears in the audio. A point is the rms of that audio in a band, so the picture is the RX input's power against LO frequency with a resolution of `band_hz`, two-sided (a signal at f+d and one at f-d land on the same audio frequency).

**Measured** (10 MHz, TX cabled to RX): with the tracking generator off the audio is 2.05 counts rms in a 5 kHz band, the same at every one of 21 points over 100 kHz (flat to 0.1 count). With it on the 12 kHz line reads 8771 counts rms, constant to 0.03 % over five points and equal to the line seen in the audio spectrum (12,407 counts peak); in a band that stops below 12 kHz it is invisible (1.9 counts). The line lies at exactly the programmed IF for IFs of 4, 8, 12, 16 and 20 kHz, so the LO is the TX word plus the IF; the scope's FFT puts the TX at exactly 9,988,000 Hz for the word of an analysis at 10 MHz (12 kHz below the LO). The dynamic range in a 15 kHz band is about 72 dB.

**Not verified:** the amplitude in dBm (the result is in counts), the ghost lines the Help file describes (birdies from the unfiltered LO, worse above 100 MHz) and the responses to subharmonics of the input (direct conversion), the behaviour with strong signals, and anything above 100 MHz. These need a calibrated external source, which was not on the bench. Use the tool to find where a signal is, not how strong it is.
