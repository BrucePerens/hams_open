---
title: Glasgow logic analyzer bench session
summary: The wiring, step by step, to verify the logic analyzer and the pattern generator on real hardware, and why the Glasgow cannot loop its own pattern into its own analyzer.
see_also: [instruments/glasgow/bench-session, instruments/glasgow/logic-analyzer, tools/bench_gla_capture, tools/bench_gla_pattern]
instrument: glasgow
verified: fixtures only; not run on the unit as of 2026-11-08 (waiting for a 480 Mb/s link and for the wiring)
---

# Glasgow logic analyzer bench session

Prerequisites: [bench-session](instruments/glasgow/bench-session) steps 1 and 2 done (loopback gpio works), `bench_glasgow_list` shows `high_speed: true`, ports at 0 V. After each step run `bench_glasgow_safe_state`. Nothing here is connected to a radio or to a target that matters. Port A at 3.3 V is used throughout.

**Why no self loopback.** The Glasgow runs one applet at a time, so while the analyzer records, nothing can drive the pins, and a jumper from a driven pin to a captured pin cannot be observed. The signal sources below are outside the Glasgow. Pattern checks use the scope as the independent instrument.

1. **Idle capture, nothing connected.** `bench_glasgow_port_set {"port":"A","volts_v":3.3,"confirm":true}`, then `bench_gla_capture {"pins":"A0:3","duration_s":2,"pull":"up","confirm":true}` (a pull needs `confirm`). Expect all four pins high with zero edges (pull-ups) and then with `"pull":"down"` low with zero edges; with `"pull":"none"` a floating input may show a few edges of mains pickup. No overrun. This is the only step that needs no wire.
2. **Known 1 kHz signal.** Scope probe-compensation output to A0, the probe ground clip to a Glasgow GND pin (about 3 V, 1 kHz, 50 %). `bench_gla_capture {"pins":"A0","duration_s":2}`. Expect `frequency_hz` about 1000 (within the scope's own tolerance), `duty_cycle` about 0.5, about 2000 rising edges, glitches 0. Add `"trigger":{"type":"edge","pin":"A0","edge":"rise"},"pre_trigger_ms":0.5,"post_trigger_ms":5`: the window must start 0.5 ms before a rising edge and hold about 5 rising edges. Open the written `.sr` in PulseView (optional) and check the 1 kHz.
3. **Pattern checked by the scope.** Remove the signal. Scope probe on A1 and ground. `bench_gla_pattern {"pins":"A1","pattern":{"type":"pwm","frequency_hz":100,"duty":0.25,"duration_s":3},"confirm":true}`. Expect the scope to measure about 100 Hz and 25 % (the answer's `planned.achieved` is the target; the real edges jitter by about 1 ms), 3.3 V, then A1 high-impedance. Repeat with a `bits` pattern `10110010` at `bit_ms` 5 and compare the sequence on the scope.
4. **UART decode (optional, needs a 3.3 V UART source).** A 3.3 V USB-serial adapter or a microcontroller board (never the radio's serial adapter) sending at a known baud, its TX to A0, ground to ground: `bench_gla_capture {"pins":"A0","duration_s":3,"decode":{"type":"uart","pin":"A0","baud":9600}}`. Expect the text it sends.
5. **Edge-rate limit (optional, needs a 3.3 V CMOS oscillator can of 1, 4, 8, 12 or 25 MHz).** Power the can from the port's Vio pin and GND (it draws a few mA; the port trips at 0.1 A), its output to A0. Capture 2 s from the slowest can upward and note the first rate at which `overrun` becomes true; with a can of frequency f the edge rate is 2f. The model says 4 pins at 480 Mb/s carry about 13 million edges per second, so the 8 MHz can (16 million) should overrun and the 4 MHz can (8 million) should not. Write the measured values on [logic-analyzer](instruments/glasgow/logic-analyzer).

Record what happened and set the `verified` lines with the date. Do not change a limit to make a step pass.
