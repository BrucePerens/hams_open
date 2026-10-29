---
title: Sound device tools
summary: What the bench_audio_* tools do, the tiers, the reserved VNA codecs, and why they live in bench_mcp.
see_also: [setup]
tools: [bench_audio_devices, bench_audio_status]
instrument: sound
tier: read
verified: device list and status run against the dev box PipeWire graph, 2026-10-29
---

# Sound device tools

The `bench_audio_*` tools list the host's sound outputs and inputs, record and play, verify that a device under test made a
sound, report sound events over time, speak, alert the operator, set levels, and discover by listening which outputs and
inputs really work. They are in bench_mcp (`src/audio/`) so that the bench has one server.

* Tiers: `read_only` lists and reports; `read_config` records and verifies (the microphone hears the room, recordings can contain speech); `full` makes noise (play, speak, alert, volume, loopback, discovery).
* The two USB AUDIO CODEC cards (`08bb:29c0`) inside the VNWA belong to the VNA driver. They are listed as reserved and no audio tool opens them or reads their volume.
* Audio goes through PipeWire; the tools use `pw-dump`, `pw-play`, `pw-record` and `wpctl`.

See `docs/AUDIO_TOOLS.md` in the bench_mcp directory for the configuration entry.
