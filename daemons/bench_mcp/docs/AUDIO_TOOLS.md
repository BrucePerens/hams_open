# Audio tools (bench_mcp, `audio` driver)

The `bench_audio_*` tools work with the host's sound devices: list them, record and play, check that a device under test made
a sound, report sound events, speak, alert the operator, set levels, and find out which outputs and inputs actually work by
listening. They live in `src/audio/` and are part of the bench_mcp server, so there is one server for the bench.

All audio goes through PipeWire (`pw-dump`, `pw-play`, `pw-record`, `wpctl`). Recordings stay in the managed capture directory of
the machine and are never uploaded.

## Registering

The tools belong to one instrument entry of the bench config (`BENCH_MCP_CONFIG`) with `"driver": "audio"`. The entry's tier decides
what is listed and allowed:

* `read_only`: `bench_audio_devices`, `bench_audio_status`, levels, the stored discovery result.
* `read_config`: record, verify, detect events. They listen to the room, and a recording can contain speech.
* `full`: play, speak, alert, set volume and mute, loopback test, discovery. They make noise in the room, inside the volume cap and quiet hours.

```json
{"instruments": {
  "snd": {"driver": "audio", "tier": "full",
          "outputs": {"desk": "Speaker", "tv": {"match": "HDMI / DisplayPort 1", "volume_cap": 0.2}},
          "inputs":  {"mic": "Digital Microphone"},
          "alert_output": "auto", "verify_input": "auto",
          "volume_cap": 0.5, "quiet_volume": 0.15,
          "quiet_hours": {"start": "22:00", "end": "07:00"},
          "capture_dir": "/var/lib/bench/audio", "retention_files": 50, "retention_days": 14, "max_record_s": 30,
          "reserved": ["08bb:29c0"]}
}}
```

* `outputs`, `inputs`: aliases. The value is a name fragment (matched against the PipeWire node name and description), or an object with `match` and a `volume_cap` of its own. Persistent names are PipeWire node names; the numeric ids change when a device is re-plugged and are never stored.
* `alert_output`, `verify_input`: `auto` or an alias. The automatic choice comes from the discovery result (heard by listening, not by name).
* `volume_cap`: the highest volume any tool sets or plays at. `quiet_volume`: the volume for audible output during `quiet_hours` (local time; `"quiet_hours": false` turns them off). Both are in `wpctl` units, where the signal amplitude factor is the cube of the number (0.5 is about -18 dB).
* `capture_dir` (default `<BENCH_MCP_OUT>/audio`), `retention_files`, `retention_days`, `max_record_s`.
* `reserved`: more `vid:pid` values or node-name fragments to leave alone. `08bb:29c0` (the two PCM2900C codecs inside the VNWA 3E) is always reserved: the VNA driver opens them exclusively, and a second opener breaks its sweeps. Reserved devices are listed and flagged, are refused by name everywhere, and are not even asked for their volume. The check uses the kernel's `/proc/asound/cardN/usbid`, never a card number, because a VNA power cycle renumbers the cards.

## Tools

| Tool | Tier | What it does |
| --- | --- | --- |
| `bench_audio_devices` | read_only | Outputs and inputs with names, aliases, ALSA card/device, USB path and vid:pid, jack or port availability, defaults, rates, formats, channels, volume, mute, reserved flag. |
| `bench_audio_status` | read_only | Helper programs found, safety limits, quiet hours now, capture directory, aliases. |
| `bench_audio_levels` | read_only | Volume, gain in dB, mute and cap of one or all devices. |
| `bench_audio_record` | read_config | Records an input for N seconds (or from a level threshold) to a WAV in the capture directory; peak, RMS, clipping, silence fraction, all-zero flag. Can record what an output plays (`monitor_of_output`). |
| `bench_audio_meter` | read_config | Level readings (peak, RMS) every interval for N seconds; nothing saved. |
| `bench_audio_play` | full | Plays a generated tone (sine, sweep, two_tone, noise, level in dBFS) or a WAV from the capture directory, inside the volume cap and quiet hours. |
| `bench_audio_set_volume` | full | Volume and mute of one device, an output never above its cap. |
| `bench_audio_loopback` | full | Plays a tone on one output while recording one input; heard or not, level, SNR, noise floor, indicative latency. |

## Safety

* The volume cap (`volume_cap`, and a `volume_cap` per output alias) bounds every audible operation. Playing lowers a louder output to the limit for the operation and puts the volume and mute back afterwards (also when the operation fails). The tone level is limited to -1 dBFS.
* In quiet hours (default 22:00 to 07:00, local time) audible operations use the quiet volume. `bench_audio_loopback` accepts `urgent: true` to use the cap.
* Generated sound is limited to `max_play_s` (default 10 s); recordings to `max_record_s` (default 30 s).
* `bench_audio_play` plays files only from the managed capture directory (no paths outside it, no symbolic links out of it).
* Recordings are written to the capture directory with owner-only permissions, are pruned by `retention_files` and `retention_days`, and are never uploaded. A recording from a microphone can contain speech.
* Reserved devices (the VNWA codecs) are refused by name in every tool before any stream is opened.

## Tests

Tests read a recording of this development machine's PipeWire graph in `tests/audio_fixtures/devbox` (made by `tests/audio_fixtures/capture.py`).
`tests/hil_audio.rs` (ignored by default) runs the tools through the MCP protocol against the real PipeWire with a virtual sink and source made by `pw-loopback`, so nothing is audible:
`cargo test --test hil_audio -- --ignored --test-threads=1`.

* The `audio` config entry parses, defaults, and refuses bad entries; the VNWA codec is always reserved. [@ANCHOR: audio_config]
* `pw-dump` output becomes the device list: defaults from the active metadata, USB path, route availability, formats; the VNWA codecs are reserved by usb id. [@ANCHOR: audio_devices_parse]
* `wpctl get-volume` output parses. [@ANCHOR: audio_levels_parse]
* WAV files read (16, 24, 32-bit, float) and write. [@ANCHOR: audio_wav]
* Generated tones have the requested level, edges and arguments. [@ANCHOR: audio_tones]
* Level statistics: peak, RMS, clipping, silence fraction, all-zero input, one tone's amplitude. [@ANCHOR: audio_analysis]
* Was a tone heard: over the noise before it, over neighbouring frequencies, with onset and offset; dead input and noise bursts. [@ANCHOR: audio_heard_tone]
* The volume limits: cap, per-output cap, quiet hours, urgent. [@ANCHOR: audio_safety_limits]
* The volume guard lowers, unmutes and restores, and refuses reserved devices before asking anything. [@ANCHOR: audio_safety_guard]
* Managed files only, and retention. [@ANCHOR: audio_store_managed] [@ANCHOR: audio_store_retention]
* Record, play, loopback and volume tools end to end on a virtual sink and source. [@ANCHOR: audio_record_tool] [@ANCHOR: audio_play_tool] [@ANCHOR: audio_loopback_tool] [@ANCHOR: audio_set_volume_tool]
