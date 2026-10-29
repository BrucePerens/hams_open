---
title: bench_list
summary: List the configured instruments with their driver and permission tier.
see_also: [concepts/configuration, concepts/safety-tiers, tools/bench_identify]
tools: [bench_list]
instrument: [scope, psu, vna, bladerf, limesdr, usb-hubs, sound]
tier: read_only
verified: code only, 2026-10-29
---

# bench_list

The first call of a session: which instruments exist on this bench, under which alias, with which driver and at which tier. The tier decides which tools were listed to you and which calls are refused.

## Arguments

<!-- arguments -->

This tool takes no arguments.

## Returns

A JSON array with one object per configured instrument: `name` (the alias to pass as `instrument`), `driver` (`siglent_sds`, `atm6301`, `vnwa_open`, `bladerf`, `limesdr`, `usb_hub`, `audio`) and `tier` (`ReadOnly`, `ReadConfig` or `Full`).

## Side effects and failure modes

None; it touches no hardware and cannot fail on a bench that started.

## Example

Request: `bench_list {}`. Answer on the bench this manual was written for (shape; the aliases are the owner's):

```json
[{"name": "scope", "driver": "siglent_sds", "tier": "ReadConfig"},
 {"name": "psu",   "driver": "atm6301",     "tier": "ReadConfig"},
 {"name": "vna",   "driver": "vnwa_open",   "tier": "ReadConfig"},
 {"name": "blade", "driver": "bladerf",     "tier": "ReadConfig"},
 {"name": "lime",  "driver": "limesdr",     "tier": "ReadConfig"}]
```

Related: [bench_identify](tools/bench_identify) asks one instrument who it is; [bench_status](tools/bench_status) what it is doing.
