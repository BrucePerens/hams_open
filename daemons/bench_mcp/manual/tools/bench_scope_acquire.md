---
title: bench_scope_acquire
summary: Acquisition mode, average count and memory depth, read back with the sample rate.
see_also: [tools/bench_set, tools/bench_status, instruments/scope]
tools: [bench_scope_acquire]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_acquire

Sets the acquisition mode (`sampling`, `peak_detect`, `average` with `average_count`, `high_resolution`) and the memory depth, then reads back the mode, the depth and the sample rate. With no argument it only reads. The commands (`ACQW`, `AVGA`, `MSIZ`) come from the SDS1000X-E programming guide and are not yet confirmed on firmware 1.3.27: the read-back shows what the scope accepted, and a setting it did not accept shows its old value or no answer.

## Returns

`sent`, `readback` (each query and its reply), `mode`, `memory_depth`, `sample_rate_sa_s`.

## Arguments

<!-- arguments -->

* `mode`: `sampling`, `peak_detect`, `average` or `high_resolution`.
* `average_count`: with `average`, a power of two from 4 to 1024; default 16.
* `memory_depth`: `7K`, `70K`, `700K` or `7M` (the larger sheet sizes `14K`, `140K`, `1.4M`, `14M` are also accepted).
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).
