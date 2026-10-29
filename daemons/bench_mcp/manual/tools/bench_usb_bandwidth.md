---
title: bench_usb_bandwidth
summary: Which devices share a translator, root hub or controller, and the periodic bandwidth they use and could reserve.
see_also: [instruments/usb-hubs/operation, tools/bench_usb_diagnose]
tools: [bench_usb_bandwidth]
instrument: usb-hubs
tier: read_only
verified: run on the dev box tree: the VNWA chain, 2026-10-29
---

# bench_usb_bandwidth

Per domain: a transaction translator (full- and low-speed devices behind a high-speed hub; one TT per port on a multi-TT hub, one for the whole hub on a single-TT hub), a high-speed bus,
a SuperSpeed bus. Gives the periodic budget in bytes per ms (1350 for a full-speed frame, 90 % of 1500), what the endpoints of the current alternate settings reserve now (`active`) and
what the busiest alternate setting of each interface could reserve (`worst_case`, from the descriptors), the devices in the domain, and findings. Also lists each controller's root hubs
(the USB 2 and USB 3 root hubs of one xHCI controller are companions: a SuperSpeed device uses the SS root hub, anything slower on that connector the USB 2 one). Argument: `bus`.

Worked example (the dev box, 2026-10-29): the VNWA's two audio codecs and its control chip hang from a full-speed hub (3-2.2) on port 2 of the high-speed hub 3-2. All four share
that one port's translator. The idle codecs reserve nothing; each could reserve about 407 bytes/ms when streaming (two interfaces' busiest alternate settings), so the pair can reserve about
60 % of the translator's periodic budget. Two codecs streaming at once fit; adding another full-speed isochronous device behind the same hub would not leave much room.
The figures are the host's reservation arithmetic, an estimate and not a measurement of what the devices send.

## Arguments

<!-- arguments -->
