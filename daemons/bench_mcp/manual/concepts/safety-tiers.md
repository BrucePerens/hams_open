---
title: Safety tiers
summary: The three permission tiers of an instrument, what each allows and how the server enforces them.
see_also: [concepts/configuration, concepts/etiquette, concepts/errors]
verified: code only, 2026-10-29
---

# Safety tiers

Every instrument in the configuration has a tier: `read_only` (the default when the entry gives none), `read_config` or `full`. Every tool has a minimum tier. The server enforces it in two ways: a tool that no configured instrument may use is not listed at all, and a call above the instrument's tier is refused with `<tool> needs tier <Tier>; "<alias>" is configured at <Tier>`. The refusal is made before any hardware is touched. The tiers are ordered: `full` includes `read_config` includes `read_only`.

| Tier | Meaning | Examples |
|---|---|---|
| `read_only` | looks, changes nothing the equipment keeps | `bench_list`, `bench_identify`, `bench_status`, `bench_measure`, `bench_capture`, `bench_screenshot`, the manual tools, topology and diagnosis of USB, listing sound devices |
| `read_config` | changes settings or runs a measurement that stimulates something, but cannot hurt the equipment or the bench | `bench_set` (scope settings, supply setpoints within the caps), `bench_scope_spectrum` (may set timebase and memory depth), every `bench_vna_*` tool, `bench_sdr_*` (tunes a receiver), `bench_psu_safe_state`, recording sound |
| `full` | can cause damage, make noise or stop a running device | `bench_psu_output`, `bench_scope_self_cal`, USB port power off and cycle, playing sound |

Rules that go beyond the tier:

* Switching the supply output needs `full`, `confirm_load_connected: true` from the person at the bench (or `dry_run`), and the server switches it off again after `for_s` seconds (at most `max_on_s`, default 600). `bench_psu_safe_state` (`read_config`) switches it off and reads it back. The tool still refuses when the setpoints stored in the supply exceed the configured `max_volts` and `max_amps`.
* `bench_scope_self_cal` needs `confirm_inputs_disconnected: true`, and that must come from the person at the bench who unplugged every probe. Do not pass it on your own judgement.
* Supply setpoints are capped by the instrument's `max_volts` and `max_amps` in the configuration (defaults 5.5 V and 1.0 A) whatever the tier.
* Nothing in bench_mcp is meant to transmit on the air. The SDRs only receive; the Hamlib transmit tools key a rig only if the owner has attested in the config that its antenna port is on a dummy load, and the software cannot know what is connected ([limits](instruments/hamlib/limits)); the VNA and its generator drive a cable port at about -17 dBm at the highest setting. See [etiquette](concepts/etiquette).
* USB and sound tools have their own protections (a protected-device list, volume caps, quiet hours); see the [USB hub](instruments/usb-hubs/overview) and [sound](instruments/sound/overview) pages.

To get a tool that is not listed, the person who owns the configuration raises the tier of the instrument; the model cannot.
