---
title: Hamlib rigs (receivers and dummy-load transmitters)
summary: Radios driven through Hamlib's rigctld: status and control of receivers, receiver tests, and transmit tests into an attested dummy load only.
see_also: [instruments/hamlib/overview, instruments/sound/overview, concepts/safety-tiers]
instrument: hamlib
verified: n/a (navigation)
---

# Hamlib rigs (receivers and dummy-load transmitters)

The `bench_hamlib_*` tools. A rig is any radio Hamlib supports, reached through the `rigctld` daemon. Start with the overview, then setup (the config entry, which tier to give the rig) and limits (what the safety model can and cannot know).

<!-- children -->
