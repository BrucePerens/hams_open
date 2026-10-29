---
title: Example instrument limits
summary: Ranges, caps, what is refused and the legal and safety notes. The model for a limits page.
see_also: [instruments/example/setup]
instrument: example
verified: fixtures only
---

# Example instrument limits

A limits page lists every range the driver enforces, where the limit comes from (the instrument's rating, the owner's config, the driver), and what is not verified.

* Level: 0 to `max_level_v` (config, at most the 5 V rating). Checked before any byte is sent.
* Pulse: 0.1 s to `max_pulse_s` (config, at most 60 s). The instrument is also told its own time limit of the pulse plus one second, so it switches off even if the server stops.
* Reads: 3 tries of 2 s each; a reply that is not a finite number is an error.
* Nothing radiates. If your instrument can radiate, this page must say at what level, into what load, and that legal compliance for any antenna connection is the owner's responsibility.
* Not verified: everything. There is no hardware; the pages say `fixtures only`. A page for a real instrument says what was run on which unit and firmware, by whom, and when.
