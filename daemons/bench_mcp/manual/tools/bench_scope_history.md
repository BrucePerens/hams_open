---
title: bench_scope_history
summary: History (segmented) memory: mode, frame list, frame select, frame read-out.
see_also: [tools/bench_capture, instruments/scope]
tools: [bench_scope_history]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_history

Turns the scope's history mode on or off (`HSMD`), shows its frame list (`HSLST`), selects a frame (`FRAM`) and reads frames: for each number in `frames` the frame is selected, its time stamp (`FTIM?`) is read and its waveform on `channel` is read from the waveform memory and summarised (minimum, maximum, peak-to-peak), and saved as little-endian f32 volts with `save`. Recording frames is the scope's own function (history mode shows what was recorded while it ran); this tool reads what is there. The commands are from the SDS1000X-E programming guide and are not yet checked on firmware 1.3.27: the settings are read back, and a setting the scope did not accept shows its old value.

## Returns

`sent`, `readback`, `settings` (`history`, `list`, `frame` as the scope reports them) and `frames`, one object per requested frame: `frame`, `time` (the scope's `FTIM` text), `samples`, `sample_interval_s`, `min_v`, `max_v`, `pkpk_v`, and `file` when saved.

## Arguments

<!-- arguments -->

* `history`: true or false: history mode on or off.
* `list`: true or false: the frame list on or off (history must be on).
* `frame`: select this frame, 0 to 100000 (the scope limits it to the frames it recorded).
* `frames`: up to 20 frame numbers to read, with `channel`.
* `channel`: 1 or 2, for `frames`.
* `save`: with `frames`: also write each frame's waveform to a file in the output directory.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).
