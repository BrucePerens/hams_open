---
title: bench_elad_list
summary: ELAD receivers on the USB bus: ids, model, whether it streams under this config. Opens nothing.
see_also: [instruments/elad/overview, instruments/elad/setup]
tools: [bench_elad_list]
instrument: elad
tier: read_only
verified: fixtures only
---

# bench_elad_list

Lists the ELAD receivers (USB vendor 1721, a known product id) on the bus. Opens and sends nothing, so it is safe at any time and works without device permission.

## Arguments

<!-- arguments -->

## Returns

`{"receivers": [{"bus", "address", "usb_id", "model", "needs_fpga_image", "streams_under_this_config"}], "count"}`.

## Failure modes

An empty list means nothing is on the bus: see [troubleshooting](instruments/elad/troubleshooting).

Example: `bench_elad_list {}` returned `{"count": 1, "receivers": [{"model": "FDM-S2", "usb_id": "1721:061c", "needs_fpga_image": true, "streams_under_this_config": false, ...}]}`.
