---
title: SSA3000X limits
summary: Ranges the driver enforces and where each comes from, the unit's fixed trace length, accuracy notes and what is not verified.
see_also: [instruments/sa/setup, instruments/sa/protocol]
instrument: sa
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# SSA3000X limits

| Limit | Value | From |
|---|---|---|
| Frequency | 0 to the model's top (2.1 GHz for an SSA3021X, 3.2 GHz for an SSA3032X) and at least 100 Hz wide | the model name, checked before any command; the unit's clamping is also detected by reading the range back |
| RBW | 10 Hz to 1 MHz; the unit rounds up to 10, 30, 100, 300 ... 1 MHz and the tool reports its value | programming guide |
| VBW | 1 Hz to 1 MHz | programming guide |
| Reference level | -100 to +30 dBm | programming guide; the value is read back and a mismatch is an error |
| Attenuation | 0 to 50 dB, whole numbers | programming guide; read back |
| Averaging | 1 to 999 sweeps | programming guide |
| Sweep length | sweep time times sweeps at most 120 s | the driver; a longer one is refused before it starts |
| Trace | 751 points, fixed | the unit; no command sets it |
| Generator level | -20 to 0 dBm, and `max_tg_dbm` if configured | the unit's rating, the owner's config |

Reads are tried 3 times of 5 s. A sweep waits for `*OPC?` with twice the unit's sweep time plus 10 s. A reply that is not a finite number is an error.

Accuracy: levels are the unit's own, in dBm at the RF INPUT, with no correction for cables or attenuators outside the unit. Peaks closer than 2 points of a stronger one are merged; the 0 Hz point shows the unit's local-oscillator leakage. A software S21 is only as good as the through: a reading within about 10 dB of the noise floor of the sweep is not reliable. The analyzer cannot measure phase.

Verified on the unit (2026-11-05): identity, options, status, sweeps with each setting and with averaging, markers, the screenshot, and generator off. **Not verified on the unit: the generator switched on, the through reference and the S21 sweep**; they are tested against a simulator only.
