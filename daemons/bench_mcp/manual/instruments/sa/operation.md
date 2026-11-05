---
title: SSA3000X operation
summary: The normal order of calls: status, sweep, markers, and the tracking generator sequence for an S21 measurement.
see_also: [instruments/sa/setup, instruments/sa/limits]
tools: [bench_sa_status, bench_sa_sweep, bench_sa_marker, bench_sa_screenshot, bench_sa_tg_on, bench_sa_tg_off, bench_sa_tg_level, bench_sa_tg_normalize, bench_sa_tg_s21]
instrument: sa
verified: measured on the unit (Siglent SSA3021X, firmware 1.3.9.10), 2026-11-05
---

# SSA3000X operation

Spectrum:

1. `bench_sa_status`: confirms the model, lists the options and shows the generator state and the current settings.
2. `bench_sa_sweep` with `start_hz` and `stop_hz` (or `center_hz` and `span_hz`), and an RBW. It returns the unit's own readback of the settings, a summary, the strongest peaks, and the path of a CSV with 751 rows of `freq_hz,level_dbm`. The frequency of row `i` is start plus `i` times (stop minus start) over 750.
3. `bench_sa_marker` reads a marker on the trace the unit holds: `search: "max"` for the highest peak, or `freq_hz` for a given frequency.
4. `bench_sa_screenshot` saves what the screen shows.

S21 of a filter, cable or amplifier:

1. Cable TG OUTPUT straight to RF INPUT. `bench_sa_tg_normalize` with the range, RBW and `tg_level_dbm`: the generator goes on, one sweep is stored as the 0 dB reference, the generator goes off.
2. Put the device under test in the path.
3. `bench_sa_tg_s21` with `reference` set to the same name: the sweep arguments default to the reference's, the generator goes on, one sweep is taken, the generator goes off, and the CSV holds `freq_hz,s21_db,measured_dbm`. Without a reference, `s21_db` is the measured level minus the generator level, which includes the cable and the generator's own flatness.

Sweeps leave the analyzer's settings as they set them (range, RBW and the rest), and put it back in continuous sweep. Calls to one analyzer are serialized by the server. The stored reference lives in the server process; the reference CSV carries its settings and can be passed as `reference_file` after a restart.
