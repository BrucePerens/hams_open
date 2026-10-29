---
title: bench_audio_record
summary: Record an input to a WAV in the managed directory and report levels.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_record]
instrument: sound
tier: read_config
verified: run on the dev box (virtual sink and source, and the speaker with the digital microphone), 2026-10-29
---

# bench_audio_record

Record an input to a WAV in the managed directory and report levels.

Arguments: `input`, `seconds`, `sample_rate_hz`, `start_on_threshold_dbfs`, `max_wait_s`, `silence_threshold_dbfs`, `label`, `monitor_of_output`. It hears the room; the file can contain speech and stays on this machine. Returns the file, duration, peak and RMS dBFS, clipped samples, silence fraction and an all-zero flag (a muted or dead input records exact zeros).

## Arguments

<!-- arguments -->
