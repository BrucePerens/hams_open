---
title: Etiquette
summary: Exclusive hardware, nothing radiates, transmit levels, noise and leaving the bench as found.
see_also: [concepts/safety-tiers, concepts/several-instruments]
verified: code only, 2026-10-29
---

# Etiquette

* **Exclusive hardware.** One job at a time uses an instrument. If a tool reports the VNA, a codec or a USB device as busy or reserved, wait or ask; do not work around it.
* **Nothing radiates.** There is no transmit tool for the SDRs. The Hamlib transmit tools key a rig only into the dummy load that the owner attested in the config; never ask for an antenna to be connected to such a rig, and never raise a cap yourself. The VNA generator and sweep put a low level on a cable port (the TX port; about -17 dBm at 0 dB), not on an antenna. Do not connect an antenna to TX or RX, or ask the person to, for a measurement here.
* **Transmit level.** Use the lowest level that gives a clean result: the VNA `level_db` or `rf_level` low, the supply voltage and current limit just above what the load needs. Switch sources and the supply output off when you are done.
* **Supply output.** Switch the output on only with a known load connected and the setpoints checked with `bench_status`. Switch it off at the end of the job.
* **Noise.** Sound tools make real noise in a room where a person may be. Quiet hours and volume caps are enforced; do not play at night, and use `bench_audio_alert` with a severity rather than raw playback to get attention.
* **Cables and probes.** Do not ask for a scope self-calibration with probes connected. Tell the person which cable or probe you need moved.
* **Leave the bench as found.** Restore scope settings you changed (timebase, trigger mode `AUTO`), leave generators off, do not leave a device deauthorized or a USB port off unless that was the job.
* **Report what you did.** In your final answer list the settings changed, the files written and anything you could not verify.
