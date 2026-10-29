---
title: bench_audio_alert
summary: Alert the operator with a tone pattern and/or speech, and return the text to send elsewhere.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_alert]
instrument: sound
tier: full
verified: virtual sink and source with espeak-ng, 2026-10-29
---

# bench_audio_alert

Alert the operator with a tone pattern and/or speech, and return the text to send elsewhere.

Arguments: `message`, `severity` (info, warning, urgent), `tone`, `speak`, `repeat`, `output`. Quiet hours (default 22:00 to 07:00): lowest volume unless urgent. The result always has `text_to_send`; the operator may be away.

## Arguments

<!-- arguments -->

Speech options, as in [bench_audio_speak](tools/bench_audio_speak): `engine` (`auto`, `speech-dispatcher`, `espeak-ng` or `piper`), `language` and `voice` (codes; the configured defaults apply when omitted) and `rate_wpm` (80 to 450 words per minute).
