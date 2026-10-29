---
title: Working with several instruments at once
summary: Locks, shared hardware, and the order of operations when a job uses more than one instrument.
see_also: [concepts/reserved-devices, concepts/etiquette]
verified: code only, 2026-10-29
---

# Working with several instruments at once

* Calls are made one after another by the client, and the server serialises calls on the same instrument; a slow call on the VNA does not hold up the scope or the supply. Do not start a second call on an instrument while a long one (a sweep, a survey, a self-calibration) runs: it waits for the lock.
* A signal source and a receiver in one job: set up the receiver first (scope timebase, channel, coupling and probe attenuation), then start the source, measure, then switch the source off. A VNA generator or AM stays on until you stop it (AM only stops with a power cycle), so always finish by switching it off and say so.
* The VNA generator and a VNA sweep or the frequency meter must not overlap; switch the generator off before a sweep.
* Cables are the person's. Do not assume where a cable is connected: a procedure that needs TX connected to a scope input, a 50 ohm load or a splitter says so; ask the person to confirm the cabling or check it by measurement before relying on a level.
* The scope's input is 1 MOhm; a 50 ohm source into it reads twice its loaded level. The `dbm_50` figure assumes the source is terminated in 50 ohms elsewhere (for example a tee with a 50 ohm load).
* The VNA's power cycle goes through the USB hub the VNA hangs from. Do not cycle or reset other devices on that hub while the VNA is measuring.
* Files from different tools go into the same output directory; their names carry a time stamp. Do not delete files you did not create.

Cross-instrument jobs are written out step by step in [procedures](procedures).
