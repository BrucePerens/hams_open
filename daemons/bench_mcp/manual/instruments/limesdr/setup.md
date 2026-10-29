---
title: LimeSDR setup
summary: Software, firmware, connectors and the configuration entry.
see_also: [instruments/limesdr/overview, instruments/limesdr/troubleshooting, concepts/configuration]
instrument: limesdr
verified: code only, 2026-10-29
---

# LimeSDR setup

Configuration: `"lime": {"driver": "limesdr", "tier": "read_config"}`; optional `serial` (hex digits). Software: `soapysdr-module-lms7` and LimeSuite from apt. The radio needs firmware 4 and gateware 2.23 (`LimeUtil --update` writes them; the owner did that on 2026-10-26).

Connectors: the signal goes to the connector the tools select. With the default `antenna` of `LNAL` use the L connector of the receiver; a signal on H or W is not heard unless `antenna` `LNAH` or `LNAW` is given. For coherence, split one signal to the same-type connector of both channels. Do not connect anything to the transmit connectors from this server's point of view: nothing transmits.
