---
title: Sound tool protocol notes
summary: The programs the sound tools run and the arguments they use, and where each fact came from.
see_also: [instruments/sound/limits, instruments/sound/safety, concepts/extending/provenance]
instrument: sound
verified: code only, 2026-10-29
---

# Sound tool protocol notes

There is no instrument protocol: the tools run the PipeWire command-line programs and read their output. No command string comes from the caller.

| Program | Used for | Arguments that matter |
|---|---|---|
| `pw-dump` | the device list (nodes, ports, USB ids, defaults) | none; the JSON is parsed |
| `wpctl get-volume`, `set-volume`, `set-mute` | read and set volume and mute by node id | the id comes from `pw-dump` |
| `pw-play` | playing a generated or managed WAV | `-P "node.dont-fallback=true node.dont-reconnect=true" --target <node> --volume <v> <file>` |
| `pw-record` | recording one channel as raw 32-bit float | `-P "node.dont-fallback=true node.dont-reconnect=true" --raw --target <node> --rate <hz> --channels 1 --format f32 --latency 40ms -` (a monitor of an output adds `stream.capture.sink=true`) |
| `espeak-ng`, Piper, `spd-say` | speech | the text is passed on stdin; voice and language are checked against a safe character set first |
| `pkill -f "pw-play -P node.dont-fallback=true node.dont-reconnect=true"` | `bench_audio_safe_state`, stopping this server's own playback | the exact command line of a stream this server started |

`node.dont-fallback` and `node.dont-reconnect` make a stream whose target has vanished fail instead of moving to the default device. Reserved devices are found by the USB vendor and product ids of the card and its place in the tree, not by card number or name.

| Fact | Source kind | Source |
|---|---|---|
| the programs and their options | other public documentation | the PipeWire and WirePlumber manual pages on the dev box |
| the device graph used by the tests | observation of the device's replies | `tests/audio_fixtures/devbox/pw-dump.json`, host names and serial numbers replaced |
| volume scale (amplitude is the cube) | measurement | `wpctl` readings against recorded levels on the dev box |

Nothing was copied from vendor source, firmware, drivers or help text.
