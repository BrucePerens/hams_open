---
title: ELAD FDM receivers: troubleshooting
summary: Symptoms, causes, actions.
see_also: [instruments/elad/setup, instruments/elad/limits]
instrument: elad
verified: fixtures only
---

# ELAD FDM receivers: troubleshooting

| Symptom | Cause | Action |
|---|---|---|
| `no ELAD receiver is on the USB bus` | unplugged, a switched hub port, or another computer owns the port | check `lsusb` for `1721:`, replug, check the hub |
| `could not open the receiver (Access denied)` | no permission on the device node | install the udev rule or set an ACL (setup page) |
| `needs its FPGA image loaded` | an FDM-S1/S2 with no declared image | load the image yourself (setup page), then set `"fpga_loaded": true` |
| `delivered no samples` / `FPGA image is probably not loaded` | the image is not loaded, was lost at power-up or at a rate change | reload it for this rate; replug resets it |
| `measured_rate_hz` well below `rate_hz`, `bulk_timeouts` above zero | USB host too slow or shared | use a lower rate, another port, a USB 2.0 port without other traffic |
| `rate_hz ... is not one of the` | rate not offered by the model | use a listed rate |
