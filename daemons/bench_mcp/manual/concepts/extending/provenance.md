---
title: Protocol work, provenance and capture hygiene
summary: How to learn an instrument's protocol without copying anyone's code, how to record where each fact came from, and how to scrub captures.
see_also: [concepts/extending, instruments/vna/protocol, instruments/example/protocol]
verified: code only, 2026-10-29
---

# Protocol work, provenance and capture hygiene

bench_mcp is AGPL-3.0-or-later. A contribution is AGPL-3.0-or-later too and must be yours to license that way.

## Allowed sources

* The vendor's public documentation, including the SCPI or command manual published for programming use.
* Observation of the device's own responses to commands you are permitted to send (read-only first).
* Captures (USB, serial, network) of YOUR OWN sessions with the vendor's program on the owner's instrument, with the owner's consent. The clean-room worked example is `daemons/vnwa_open/docs/PROTOCOL.md`: usbmon captures of the vendor program on the owner's unit, nothing from the vendor's code or firmware.
* Measurements with other instruments.

## Not allowed

* Vendor source code, firmware images, drivers, DLL or executable disassembly, decompiled output, or help-file text.
* Pasting copyrighted manual text. Describe commands and behaviour in your own words; a command name and a number range are facts, a paragraph is not.
* Code from a project whose licence is not compatible with AGPL-3.0-or-later (no "all rights reserved", no non-commercial-only, no "personal use"). Ask first when unsure.
* Sources whose terms forbid reuse or reverse engineering of that kind: leave them out; do not ask the maintainer to waive.
* A vendor SDK as a dependency or vendored copy. If it is the only way, say so in the pull request, describe the interface as an optional run-time library load (as the SDR tools load SoapySDR at run time), and do not vendor it.

## Record each source

The `protocol` page of the instrument has a provenance table: one row per fact or group of facts, with the source kind (vendor programming manual; observation of replies; capture of the owner's own session; measurement; other public documentation), and what was observed on which unit and firmware. End it with one sentence: nothing in this module comes from vendor source, firmware, drivers or help text. (`instruments/example/protocol` is the template; `instruments/vna/protocol` and `daemons/vnwa_open/docs/PROTOCOL.md` are real ones, with a "confirmed on the wire" table and corrections kept visible.)

## Capture hygiene

* Capture only what you need, and only the owner's instrument.
* Scrub before the file leaves the owner's machine: serial numbers (`SN-0000`), MAC addresses, IP addresses (use 192.0.2.x), hostnames, licence keys, account names, speech or ambient audio. Replace, do not black out, so the format stays parseable.
* Keep the raw capture on the owner's machine; commit only the scrubbed transcript, in the form of `tests/example_fixtures/status.txt`.
* Say in the pull request what you scrubbed. `cargo test` rejects public IPv4 addresses, MAC addresses and credential words in fixtures, pages and the README; it cannot catch a serial number, so look.
