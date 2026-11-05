---
title: SSA3000X troubleshooting
summary: Symptoms, causes and next actions for the SSA3000X driver, in escalation order.
see_also: [concepts/errors, instruments/sa/protocol]
instrument: sa
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# SSA3000X troubleshooting

| Symptom | Likely cause | Do |
|---|---|---|
| `no usable reply after 3 tries` | cable, wrong host or port, analyzer off or busy | check them; `bench_sa_status`; if still silent power-cycle the analyzer |
| `not an SSA3000X spectrum analyzer` | the address belongs to something else | nothing else was sent; fix the config |
| `the analyzer holds ... not the requested` | the unit clamped the range, or reference level and attenuation are coupled | choose a range inside the model's limits, or other values |
| `would take ... s, more than the 120 s limit` | narrow RBW over a wide span, or many averages | widen the RBW, narrow the span, average fewer sweeps |
| `the sweep did not finish` | the unit is busy, in a menu, or hung | `bench_sa_status`; press Sweep > Continuous on the front panel; power-cycle |
| `the trace has N points` | a USB or cut-short reply | use the LAN, not USB |
| `SAFE STATE NOT CONFIRMED` | the generator may still be on | press TG then TG off on the front panel, or unplug what is on TG OUTPUT |
| `reference was taken with ...` | different range, RBW, attenuation, detector, averaging or level | repeat `bench_sa_tg_normalize`, or leave those arguments out |
| `does not list the TG ... option` | no tracking generator on this unit | use the spectrum tools only |

The sweep tools put the analyzer back in continuous sweep on every path. The driver never returns an earlier trace in place of a new one.
