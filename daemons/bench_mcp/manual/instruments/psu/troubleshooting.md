---
title: Supply troubleshooting
summary: Symptom, cause and action for the supply.
see_also: [instruments/psu/setup, concepts/errors]
instrument: psu
verified: code only, 2026-10-29
---

# Supply troubleshooting

| Symptom | Cause | Action |
|---|---|---|
| timeout, no answer | wrong baud rate (only 115200 works), wrong port, supply off | check `serial` and `baud`, the adapter in `bench_usb_topology` |
| the call waits a long time | another process holds the port lock | wait; only one server may use the port |
| `refused: the supply is set to ...` when switching on | stored setpoints above the caps (front panel) | `bench_set` lower values, then switch on |
| `needs tier Full` | switching on needs `full` | ask the bench owner; do not work around it |
| voltage below the setpoint | current limit reached | check the load and `set_current_a` |
| `error` field not `0` | an earlier command was rejected | read it, resend the corrected setting |
