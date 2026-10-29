---
title: Example instrument troubleshooting
summary: Symptoms, causes and next actions in the escalation order. The model for a troubleshooting page.
see_also: [concepts/errors, procedures/power-cycle-a-stuck-device]
instrument: example
verified: fixtures only
---

# Example instrument troubleshooting

| Symptom | Likely cause | Do |
|---|---|---|
| `no usable reply after 3 tries` | cable, wrong host or port, instrument off | check them; `bench_example_status`; if still silent, power-cycle (a switchable hub port: `bench_usb_port_power`, else by hand) |
| `not an EXAMPLE-1000` | the address belongs to something else | nothing was sent to it; fix the config |
| `is not a finite number` | the meter input is open or the instrument is faulted | reconnect the input; if it persists, `bench_example_safe_state` and power-cycle |
| `SAFE STATE NOT CONFIRMED` | the output may be on | switch it off at the front panel or pull the power now; tell the owner |

The escalation ladder for every driver: retry the read (bounded), reset, power-cycle through the hub tools, then a clear error that says what a person must do. The driver never returns an earlier value in place of a new one.
