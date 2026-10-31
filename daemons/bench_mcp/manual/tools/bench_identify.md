---
title: bench_identify
summary: Ask an instrument who it is (scope and supply *IDN?, VNA and SDR driver facts).
see_also: [tools/bench_list, tools/bench_status, concepts/errors]
tools: [bench_identify]
instrument: [scope, psu, vna, bladerf, limesdr]
tier: read_only
verified: code only, 2026-10-29
---

# bench_identify

Confirms that the instrument behind an alias is reachable and is what the configuration says. For the scope and the supply it sends `*IDN?`; for the VNA it reports the driver and the configured codecs and hub; for an SDR it opens the radio through SoapySDR and reports the driver, hardware key and board information.

## Arguments

<!-- arguments -->

* `instrument`: the alias from [bench_list](tools/bench_list). Optional when exactly one scope, supply, VNA or SDR is configured; otherwise the call fails with `name the instrument: ...`.
* `device`: the name of the instrument instance, the same as `instrument`; required when several instances fit, otherwise optional. See [multiple devices](concepts/several-instruments).

## Returns

* Scope, supply: `{"idn": "<manufacturer,model,serial,firmware>"}`.
* VNA: `idn` (`DG8SAQ VNWA (vid:pid) driven by vnwa_open`), `codec_s11`, `codec_s21` (ALSA ids, null when found automatically), `hub` (`"3-2 port 2"` or null).
* SDR: `driver`, `soapy_args`, `soapy_driver`, `hardware`, and the board information (firmware, gateware, serial) under their own keys.

## Failure modes

The scope or supply does not answer: a timeout or connection error naming the address or serial port; check power and cabling and see [errors](concepts/errors). An SDR whose SoapySDR library or module is missing says so. The tool never changes a setting.

## Example

`bench_identify {"instrument": "scope"}` returns a single `idn` string (the Siglent identification string, for the unit used here an SDS1202X-E on firmware 1.3.27).
