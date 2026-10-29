---
title: bench_hamlib_stations
summary: The reference station table: WWV, WWVH, AM band samples, weather radio and the owner's entries, with notes, last heard dates and retirements.
see_also: [instruments/hamlib/overview, instruments/hamlib/limits, instruments/hamlib/measurement-recipes]
tools: [bench_hamlib_stations]
instrument: hamlib
tier: read_only
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_stations

Lists the stations that the load check listens for. Each has a frequency, a mode, what to look for in its audio (`wwv`: 1 kHz ticks and 440/500/600 Hz tones; `wwvh`: 1.2 kHz ticks; `carrier`; `voice`; `none`), a note saying where its status was checked and when, `last_heard` (the date this bench's own detections last found it) and `heard_recently` (within 30 days). A station that is silent is not heard, which is never a fault: stations go off the air (the Canadian CHU stopped its shortwave service on 2026-06-22 and is not listed) and propagation fails. The shipped table is a starting point: review it, retire what is gone, add what you know is strong at your site with [bench_hamlib_station_set](tools/bench_hamlib_station_set).

## Arguments

<!-- arguments -->

## Returns

`stations` (each with the fields above, `retired` and `added_by_owner`), `saved` (whether the owner has saved a table yet; until then the defaults apply), `table_file`, and with `frequency_hz` also `references_now`: the stations the check would use for that transmit frequency at this hour (HF: time stations first, lower frequencies at night and higher by day, then AM band samples; VHF and UHF: weather radio and the owner's entries; limited to what the rig can receive).

## Failure modes

* `stations.json ... Next: repair or remove the file`: the saved table is damaged; the defaults are not substituted silently.

## Example

`bench_hamlib_stations {"frequency_hz": 7100000}`.
