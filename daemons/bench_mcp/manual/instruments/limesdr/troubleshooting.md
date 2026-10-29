---
title: LimeSDR troubleshooting
summary: Symptom, cause and action for the LimeSDR.
see_also: [instruments/limesdr/setup, concepts/errors]
instrument: limesdr
verified: code only, 2026-10-29
---

# LimeSDR troubleshooting

| Symptom | Cause | Action |
|---|---|---|
| tool says the library or module is missing | `soapysdr-module-lms7` not installed | install it |
| radio not found | unplugged, USB power | `bench_usb_topology`; check `serial` |
| old firmware or gateware error | outdated images | the owner runs `LimeUtil --update` |
| signal not heard | it is on an H or W connector | pass `antenna` `LNAH` or `LNAW` |
| coherence far from 1 | no common signal on both inputs | split one signal to both |
| `the bladerf has 1 receive channel` style refusal | coherence on a one-channel radio | use the LimeSDR |
