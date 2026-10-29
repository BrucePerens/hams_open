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

## Tests

Tests read a recording of this development machine's PipeWire graph in `tests/audio_fixtures/devbox` (made by `tests/audio_fixtures/capture.py`).

* The `audio` config entry parses, defaults, and refuses bad entries; the VNWA codec is always reserved. [@ANCHOR: audio_config]
* `pw-dump` output becomes the device list: defaults from the active metadata, USB path, route availability, formats; the VNWA codecs are reserved by usb id. [@ANCHOR: audio_devices_parse]
* `wpctl get-volume` output parses. [@ANCHOR: audio_levels_parse]
