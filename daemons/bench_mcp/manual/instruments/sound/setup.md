---
title: Registering and configuring the sound tools
summary: The audio entry of the bench config: aliases, alert output, verify input, volume cap, quiet hours, capture directory.
see_also: [instruments/sound/overview, instruments/sound/safety]
tools: [bench_audio_devices, bench_audio_status, bench_audio_levels, bench_audio_record, bench_audio_meter, bench_audio_play, bench_audio_set_volume, bench_audio_loopback]
instrument: sound
tier: read_only
verified: config parser tested; status run on the dev box, 2026-10-29
---

# Registering and configuring the sound tools

Add an instrument with `"driver": "audio"` to the bench config. Every setting is optional.

* `outputs` and `inputs` give aliases to devices by a fragment of the PipeWire node name or description.
* `alert_output` and `verify_input` are `auto` or an alias.
* `volume_cap` and `quiet_volume` are in wpctl units (the signal amplitude factor is the cube).
* `quiet_hours` are local time, `{"start":"22:00","end":"07:00"}`, or `false`.
* `reserved` adds devices to leave alone; `08bb:29c0` is always reserved.

Run `bench_audio_status` to see the programs found and the limits in effect, and `bench_audio_devices` for the devices.
