---
title: SDR measurement recipes
summary: A noise survey of a band and a two-channel phase coherence check, as procedures with the expected shape of the result.
see_also: [tools/bench_sdr_survey, tools/bench_sdr_coherence, tools/bench_sdr_capture, instruments/bladerf/operation, instruments/bladerf/limits]
instrument: bladerf
verified: n/a (procedures; no measurement has been made on the bench yet)
---

# SDR measurement recipes

The SDRs only receive here; nothing is transmitted. Nothing below has been run on the bench: expectations are stated as shapes.

## Noise survey of a band

1. Terminate the receive input in 50 ohms (or connect the antenna you want to survey) and use a gain that keeps strong stations from clipping: start at 30 dB.
2. `bench_sdr_survey {"start_hz": 144e6, "stop_hz": 148e6, "gain_db": 30, "fft_len": 4096}`.
3. The answer has the median floor in dBFS and the ten strongest peaks; the CSV holds the whole stitched spectrum.
4. Repeat with a terminated input. The difference between the antenna floor and the terminated floor is the external noise; a terminated floor within a few dB of the radio's noise figure means the gain is sensible. Raise the gain until the terminated floor stops falling, then back off a step.

Read it as: lines at the radio's own spurs (the same frequency with the input terminated) are not signals. The survey stitches 70 percent of each capture, so a line that appears at every step edge is an artifact.

## Two-channel coherence (LimeSDR)

1. Split one signal source to both receive inputs with equal cables (a resistive splitter), at a level well under full scale.
2. `bench_sdr_coherence {"freq_hz": 100e6, "repeats": 6}`.
3. The answer is the phase and gain of channel 0 against channel 1 at the strongest tone, how stable the phase is across fresh tunes, and the time between captures.

Read it as: a stable phase across repeats (a spread of a few degrees or less) means the channels share their reference; a phase that jumps with every tune means they do not stay locked after a retune, and any direction-finding or phase-difference measurement has to recalibrate after each tune. The fixed phase offset itself comes from the cables and the splitter; swap the cables and repeat to separate them from the radio.

Verified status of both recipes: none.
