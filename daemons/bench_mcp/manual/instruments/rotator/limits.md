---
title: Rotator limits
summary: Which limits are enforced and where each comes from; what is not verified.
see_also: [instruments/rotator/setup, concepts/safety-tiers]
instrument: rotator
verified: dummy rotator only, 2026-10-29
---

# Rotator limits

* A move is sent only inside the limits: the rotator's own (from `dump_state`) narrowed by `min_az`, `max_az`, `min_el` and `max_el` in the config. If neither side gives limits, nothing is moved. A target outside them is refused with the limits and their source named, before any byte is sent.
* `wait_s` is at most `max_wait_s` (600 s at most whatever the file says). A wait that ends before arrival is reported as not arrived, never as success.
* `bench_rotator_stop` is safe at any time, does not take the instrument lock, and checks with two position reads that the motion ended; if it did not, the answer says to remove power from the controller or use its own stop.
* rotctld has no authentication and listens where its owner started it; the config refuses a remote host unless the owner accepts that.
* The software does not know what the antenna is near: cables, a mast, a person. The owner's limits are the control. Nothing here moves a rotator without a call from the model, and the tools are at `read_config`, which the owner grants.
* Not verified: any real rotator. The tests ran against Hamlib's dummy rotator and a scripted fake.
