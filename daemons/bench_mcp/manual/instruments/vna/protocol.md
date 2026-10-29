---
title: VNWA protocol summary
summary: The USB and audio protocol in short, with pointers to the crate documentation and the clean-room provenance.
see_also: [instruments/vna/overview, instruments/vna/limits]
instrument: vna
verified: observed on the unit by USB capture and replay, 2026-10-28
---

# VNWA protocol summary

The full evidence is in `daemons/vnwa_open/docs/PROTOCOL.md` and the crate README; this is the outline.

* **Control** is vendor control transfers on endpoint 0 only (no interface claim, no reset). The measured signals arrive as audio from the two codecs (48 kHz, 16 bit, stereo; the only format they offer).
* **Set-up block** of nine requests (0x40, 0x41, 0x42, 0x43, 0x44, 0x47, 0x4b, 0x49, 0x64) per sweep point, reproduced byte for byte against captured blocks, and the time-per-point bytes against 84 settings. 0x49 carries the sweep word, per-chip tuning words and the offset tail. The IF is set by 0x42 and 0x41; RF level by 0x58 (14 bits).
* **Generator**: 0x4c frequency; on, off and level from captured sequences (0x62, 0x61, 0x98, 0x9a, 0x0e, 0x04, 0x58, 0x91, 0x66). AM is a replay of one captured sequence; fields for frequency, depth and FM are not decoded.
* **Audio to a complex number**: the tone at the IF is summed over whole IF periods in the measured and the reference channel of each codec and divided (the left channel by the right); a steadiness check rejects unsteady points. Overruns of the audio buffers make a window unusable and the point is measured again.
* **Firmware version and temperature**: requests 0xff and 0x85 (temperature in uncalibrated counts).
* **Reliability**: transient errors (EPROTO shows as an I/O error, stalls as pipe errors) are resent up to 3 times 5 ms apart; the read of 0x0d after a block stalls in about 7 % of tries.
* **Not decoded**: the step counter byte of 0x49, the start-up reads, the identity table read through 0x15 (not needed), FM and pulse modulation.

Provenance: clean room. The protocol was derived from usbmon captures and audio recordings of the owner's own unit running the vendor program, and from the vendor's Help file as documentation of behaviour. No vendor code, firmware or images are used or reproduced.
