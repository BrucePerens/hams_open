---
title: bench_scope_display
summary: Screen saver, grid, intensity, XY mode, persistence, dot join and menu of the scope display, read back.
see_also: [tools/bench_screenshot, instruments/scope]
tools: [bench_scope_display]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_display

Sets the screen saver (`SCSV`: the display sleeps after 30 minutes by default; SCPI and screenshots still work asleep, set `OFF` only when the screen has to be viewed), the grid (`GRDS`) the trace and grid intensity (`INTS`), XY mode (`XYDS`), persistence (`PESU`), dots instead of vectors (`DTJN`) and the menu (`MENU`), and reads them back. Not yet confirmed on firmware 1.3.27.

## Returns

`sent`, `readback`, `screen_saver`, `grid`, `intensity` and `more` (`xy`, `persistence`, `dot_join`, `menu` as the scope reports them).

## Arguments

<!-- arguments -->

* `screen_saver`: `OFF`, `1MIN`, `5MIN`, `10MIN`, `15MIN`, `30MIN` or `60MIN`.
* `grid`: `FULL`, `HALF` or `OFF`.
* `trace_intensity`, `grid_intensity`: 0 to 100; both together.
* `xy`, `dot_join`, `menu`: true or false.
* `persistence`: `OFF`, `INFINITE`, `1`, `5`, `10` or `30` (seconds).
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).
