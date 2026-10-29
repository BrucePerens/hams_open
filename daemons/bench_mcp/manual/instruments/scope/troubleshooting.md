---
title: Scope troubleshooting
summary: Symptom, cause and action for the scope.
see_also: [instruments/scope/setup, concepts/errors]
instrument: scope
verified: probed on the unit (SDS1202X-E, firmware 1.3.27), 2026-10-28
---

# Scope troubleshooting

| Symptom | Cause | Action |
|---|---|---|
| connection error or timeout on any call | scope off, wrong `host`, network | check power and the address in the configuration; ping it; ask the person |
| `bench_capture` refused: scope is stopped | trigger mode `STOP` | `bench_set {"trigger_mode": "AUTO"}` |
| measurements read `****` | no edges, signal off screen, scale too coarse | adjust `vdiv_v` and `timebase_s` |
| capture returns a handful of samples | memory settings not reset (old firmware behaviour) | the driver sets full-point reads; retry; report if it persists |
| voltages are 10 times off | probe ratio mismatch | `bench_set` `attenuation` |
| display is dark | display sleep after 30 minutes | harmless; tools still work |
| settings changed after self-cal | calibration resets the vertical scale | read `bench_status`, restore |
