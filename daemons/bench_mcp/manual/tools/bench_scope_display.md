---
title: bench_scope_display
summary: Screen saver, grid and trace intensity of the scope display, read back.
see_also: [tools/bench_screenshot, instruments/scope]
tools: [bench_scope_display]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_display

Sets the screen saver (`SCSV`: the display sleeps after 30 minutes by default; SCPI and screenshots still work asleep, set `OFF` only when the screen has to be viewed), the grid (`GRDS`) and the trace and grid intensity (`INTS`), and reads them back. Not yet confirmed on firmware 1.3.27.

## Returns

`sent`, `readback`, `screen_saver`, `grid`, `intensity`.

## Arguments

<!-- arguments -->

* `screen_saver`: `OFF`, `1MIN`, `5MIN`, `10MIN`, `15MIN`, `30MIN` or `60MIN`.
* `grid`: `FULL`, `HALF` or `OFF`.
* `trace_intensity`, `grid_intensity`: 0 to 100; both together.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).
