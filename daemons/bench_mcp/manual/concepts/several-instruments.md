---
title: Working with several instruments at once
summary: Locks, shared hardware, and the order of operations when a job uses more than one instrument.
see_also: [concepts/reserved-devices, concepts/etiquette]
verified: tested against fake instruments only, 2026-10-31
---

# Working with several instruments at once

## Naming instances: `device`

Every instance has a unique name. The old form of the config names it by its key (`"vna": {"driver": "vnwa_open", ...}` is the instance `vna`); the group form puts several under one label, and each is named by its own key:

    "vna": {"vnwa": {"driver": "vnwa_open", "tier": "read_config"},
            "nano": {"driver": "nanovna", "tier": "read_config", "serial": "/dev/serial/by-id/..."}}

Every tool that takes `instrument` also takes `device`, the same thing. With exactly one fitting instance either may be omitted. With several and neither given, the call fails with `name the instrument (argument `device`): a, b` and touches nothing; an unknown name fails with `no instrument named ...; configured: ...`. `bench_identify` and `bench_status` without a device return every fitting instance with its kind, driver, tier and state (`ok` with the answer, or `error` with the reason). Each instance has its own tier, settings and lock, so two instruments run at the same time, and a long call on one does not hold up another.

Sharing rules: the config is refused when two instances claim one serial port (compared after resolving symlinks), one scope host and port, the VNWA USB unit (two `vnwa_open` entries) or one hub port. Instances on one hub take turns switching it (a power cycle or a generator reset holds a per-hub lock after the instance's own lock). This dev box can run both VNAs: the VNWA 3E on its USB hub and the NanoVNA-H on `/dev/serial/by-id/usb-STMicroelectronics_ChibiOS_RT_Virtual_COM_Port_400-if00`. A VNWA that is not connected fails its own tools with `device not present` and does not delay the others.

* Calls are made one after another by the client, and the server serialises calls on the same instrument; a slow call on the VNA does not hold up the scope or the supply. Do not start a second call on an instrument while a long one (a sweep, a survey, a self-calibration) runs: it waits for the lock.
* A signal source and a receiver in one job: set up the receiver first (scope timebase, channel, coupling and probe attenuation), then start the source, measure, then switch the source off. A VNA generator or AM stays on until you stop it (AM only stops with a power cycle), so always finish by switching it off and say so.
* The VNA generator and a VNA sweep or the frequency meter must not overlap; switch the generator off before a sweep.
* Cables are the person's. Do not assume where a cable is connected: a procedure that needs TX connected to a scope input, a 50 ohm load or a splitter says so; ask the person to confirm the cabling or check it by measurement before relying on a level.
* The scope's input is 1 MOhm; a 50 ohm source into it reads twice its loaded level. The `dbm_50` figure assumes the source is terminated in 50 ohms elsewhere (for example a tee with a 50 ohm load).
* The VNA's power cycle goes through the USB hub the VNA hangs from. Do not cycle or reset other devices on that hub while the VNA is measuring.
* Files from different tools go into the same output directory; their names carry a time stamp. Do not delete files you did not create.

Cross-instrument jobs are written out step by step in [procedures](procedures).
