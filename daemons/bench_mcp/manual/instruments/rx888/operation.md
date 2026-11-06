---
title: RX-888 operation
summary: The usual sequence, what each measurement means, and how to check the receiver with no antenna.
see_also: [instruments/rx888/limits, instruments/rx888/troubleshooting]
instrument: rx888
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# RX-888 operation

Sequence: `bench_rx888_status`, then `bench_rx888_boot` (only when it says unbooted), then `bench_rx888_radiod_start`. Measure with `bench_rx888_frontend`, `_tune`, `_spectrum` and `_capture`, and finish with `bench_rx888_radiod_stop`. A `radiod` this server started keeps running after the server exits. A later server finds it through its pid file and refuses to start a second one. `bench_rx888_radiod_stop` stops it after checking the process.

What the numbers mean:

- **Levels** (`baseband_power_db`, spectrum bins) are `radiod`'s, in dB re full scale. They are referred to the input: `radiod` removes the front end's RF gain. They are not dBm (`rf_level_cal_db` is reported but not applied).
- **Noise density** is dB/Hz in the channel. On noise, baseband power tracks 10 log10(passband): measured within 0.6 dB from 500 Hz to 10 kHz. A passband wider than the 12 kHz output rate is clipped by it.
- **SNR** is `10 log10(P/N - 1)` from the baseband power and the noise in the passband. -30 means no signal above the noise.
- **Input rate** is measured by `bench_rx888_tune` from `radiod`'s sample counter. The counter advances in 20 ms blocks, so the result is good to `rate_resolution_fraction`. More `readings` give a longer interval.
- **Overrange** (`ad_overrange_count`) counts A/D clips since `radiod` started. It should stay 0. With an antenna, strong broadcast stations can raise it; then set `rf_atten_db` or a lower `rf_gain_db` at `radiod_start`.
- **IF power** is the A/D input power in dBFS. It reads null (`-inf`) while no channel exists, because `radiod` stops the front end then.

Checking the receiver without an antenna (what was done on 2026-11-05):

- the firmware loads and the link is 5000 Mb/s;
- the measured rate is 64.8 Msps;
- overrange stays 0;
- the RF AGC settles at 34 dB with IF power near -52.5 dBFS;
- the floor is flat within about 1.5 dB from 0 to 32.4 MHz;
- tuning reads back exactly;
- level follows the passband width;
- captures arrive with 0 lost.

A real-signal check (a broadcast station or WWV with an antenna) is still to be done.
