---
title: Hamlib measurement recipes
summary: Receiver tests that combine the rig tools with the sound tools and, by hand, the VNA generator; what is automated and what is left to the person at the bench.
see_also: [instruments/hamlib/overview, tools/bench_hamlib_rx_check, procedures/check-a-receiver, instruments/vna/operation]
tools: [bench_hamlib_rx_check, bench_hamlib_get, bench_hamlib_scan, bench_hamlib_load_check]
instrument: hamlib
verified: dummy rig only, 2026-10-29
---

# Hamlib measurement recipes

## What is automated, what is not

`bench_hamlib_rx_check` does the receiver test that needs the rig and its audio together: signal strength against audio level, noise floor, squelch, strongest tone and its offset, time station in the audio. The tests that need an external generator are recipes for a person or an agent working with the owner, built from the tools below, because a generator belongs to another instrument and its cabling is the owner's. They are not wrapped in a tool: the cabling must be declared by the owner, and a wrong assumption about it would be silent. The automatic sensitivity and AGC sweeps, the S-meter calibration table and the audio-chain delay measurement were left out of this module for that reason.

## Receiver check on the air

1. `bench_hamlib_stations {"frequency_hz": <band>}` to see which references fit the band and the hour.
2. `bench_hamlib_rx_check {"frequency_hz": 10000000, "seconds": 8}` for WWV on 10 MHz. Try several frequencies; propagation decides what is audible, so use the best reading rather than a single frequency. Silence is not a fault of the receiver.
3. `signal_vs_audio` and the detectors tell a station from noise: the ticks and tones are found in the audio well below the S-meter.

## S-meter against a known level (by hand)

Needs: the VNA generator (`bench_vna_generator_on`, fixed level, through the owner\'s attenuators) connected to the rig\'s antenna port, with the owner\'s statement of the attenuation. Set the generator to the receive frequency, `bench_hamlib_get {"item": "signal", "count": 10}` for the reading and its spread, then change the attenuation by the owner\'s steps and repeat. The table of generator level against reading is the calibration of that rig and those settings; the generator is not a laboratory standard, so say so in the result. A generator connected to the antenna port is not an antenna, but check [limits](instruments/hamlib/limits) first: do not run a generator into a rig that the config lists as transmit-capable without the owner\'s knowledge of what else is on the port.

## Frequency accuracy

A carrier of known frequency (the generator) in `USB`: `bench_hamlib_rx_check` with `expect_tone_hz` set to the offset you expect between the dial frequency and the carrier; `tone_offset_hz` is the receiver\'s tuning error plus the sound card clock\'s error (a fraction of a hertz per megahertz of audio frequency is negligible here, but say it).

## Audio-chain delay (by hand)

Use `bench_audio_play` on the rig\'s audio input path or the generator\'s AM keying and `bench_audio_events` on the rig\'s audio; the delay between the command and the event is the chain\'s delay. Not automated.
