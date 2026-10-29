---
title: bench_screenshot
summary: Save the scope screen as an image file and return its path.
see_also: [instruments/scope]
tools: [bench_screenshot]
instrument: scope
tier: read_only
verified: code only, 2026-10-29
---

# bench_screenshot

Asks the scope for its display (`SCDP`) and writes the image into the output directory. The format is detected from the data: `bmp` or `png`.

## Arguments

<!-- arguments -->

* `instrument`: the scope alias; optional when only one scope is configured.

## Returns

`file` (path of the image) and `bytes`.

## Notes and failure modes

A screenshot works while the scope's display is asleep (it blanks after 30 minutes), but the picture of a sleeping display is only the stored frame. Fewer than 100 bytes received is reported as an error. Look at the image with a viewer; the measurement tools are better than reading numbers off a picture.

## Example

`bench_screenshot {}` returns `{"file": "<output dir>/screen_<time stamp>.bmp", "bytes": <size>}`.
