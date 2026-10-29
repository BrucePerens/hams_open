---
title: bench_hamlib_station_set
summary: Edit the reference station table: add, retire, restore, remove an owner entry, or change a note.
see_also: [instruments/hamlib/overview, instruments/hamlib/limits, instruments/hamlib/measurement-recipes]
tools: [bench_hamlib_station_set]
instrument: hamlib
tier: read_config
verified: dummy rig only, 2026-10-29
---

# bench_hamlib_station_set

`action` is `add` (needs `id`, `frequency_hz`, and a `note` saying where the station's status was checked and when; `mode` defaults to AM, `audio` to none), `retire` or `restore` (the shipped entries are retired, never deleted, so the record that they were checked stays), `remove` (only an entry the owner added) or `note`. The table is saved as `stations.json` in the managed directory. Expect to retire entries over time; a retired station is not used as a reference and is hidden unless `include_retired` is asked for.

## Arguments

<!-- arguments -->

## Returns

`{"done": "retired wwv-2_5", "stations_in_table": 36, "table_file": ...}`.

## Failure modes

* `station ... exists`, `a note is needed`, `frequency_hz must be 10 kHz to 6 GHz`, `mode ... is not a Hamlib mode name`: the entry is refused and nothing is saved.
* `part of the shipped table; retire it instead`.

## Example

`bench_hamlib_station_set {"action": "add", "id": "rpt-146940", "frequency_hz": 146940000, "mode": "FM", "audio": "carrier", "note": "local repeater, heard here 2026-10-29"}`.
