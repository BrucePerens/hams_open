---
title: bench_scope_passfail
summary: Pass/fail mask test: enable, mask tolerance, create, run, counts.
see_also: [tools/bench_screenshot, instruments/scope]
tools: [bench_scope_passfail]
instrument: scope
tier: read_config
verified: fixtures only
---

# bench_scope_passfail

Runs the scope's mask (pass/fail) test: `enable` (`PFEN`) and `source` (`PFSC`), the mask tolerance in divisions (`x_mask_div` with `y_mask_div`, `PFST`, 0.04 to 4 in steps of 0.04), `create_mask` (`PFCM`: builds the mask round the source waveform; the scope accepts it only while the test is on and stopped), the display of the counts (`PFDS`), stop on fail (`PFFS`), the buzzer output (`PFBF`), `clear` (`PACL`, zero the counts) and `run` (`PFOP`). The failed, passed and total counts (`PFDD?`) are always returned. Saving and loading mask files (`:MTESt:MASK:SAVE`, `LOAD`) is not offered because the guide says a two-channel scope does not support them. From the SDS1000X-E programming guide; not yet checked on firmware 1.3.27.

## Returns

`sent`, `readback`, `settings` (each setting as the scope reports it), `mask_tolerance` (`PFST?`) and `result`: `{failed, passed, total}` parsed from `PFDD?`, or `{unparsed: <reply>}` when the reply is not three numbers.

## Arguments

<!-- arguments -->

* `enable`: true or false: the mask test on or off.
* `source`: `C1` or `C2`.
* `x_mask_div, y_mask_div`: tolerance in divisions, 0.04 to 4.0 in steps of 0.04; both together.
* `create_mask`: true: build the mask (test on and stopped).
* `display`: true or false: show the counts on the screen.
* `stop_on_fail`: true or false: stop the test at the first failure.
* `output`: true or false: buzzer on failure.
* `clear`: true: zero the counts.
* `run`: true to run the test, false to stop it.
* `instrument`: the scope alias.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).
