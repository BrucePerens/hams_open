---
title: RX-888 limits
summary: Ranges the tools enforce and where each comes from, measured behaviour, and what is not verified.
see_also: [instruments/rx888/operation, instruments/rx888/protocol]
instrument: rx888
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# RX-888 limits

| Limit | Value | From |
|---|---|---|
| Tuning | 10 kHz to 30 MHz | `radiod_client`'s default policy; the front end reports 15 kHz to 30.456 MHz usable at 64.8 Msps |
| Sample rate | 64.8 Msps (default) or 129.6 Msps | ka9q-radio's documented rates; 129.6 Msps needs a fast host and USB 3 |
| RF gain | -8 to 34 dB | the AD8370 VGA in high-gain mode (ka9q-radio's example config); omitted, the RF AGC is on |
| Attenuation | 0 to 31.5 dB in 0.5 dB steps | the PE4312 attenuator (ka9q-radio's example config) |
| Spectrum span | at most half the sample rate (32.4 MHz at 64.8 Msps) | measured: spans up to 32.41 MHz came back full, 32.44 MHz and wider with about half the bins zero |
| Spectrum bins | 64 to 16 000 | `radiod_client` refuses more than 16 000 float bins (17 000 crashed `radiod` in its trial) |
| Passband | 100 Hz to 24 kHz | the tool; `radiod_client` allows edges to plus or minus 12 kHz |
| Capture | 0.1 to 60 s | the tool; 12 kHz (24 kHz FM), 16-bit |
| I/Q gain | -20 to 160 dB | `radiod_client`; the default puts the noise near -30 dBFS |

Every channel the tools open has a 5 s lifetime that each read renews. It is removed when the tool ends. If the server dies, it expires on its own.

The bin at exactly 0 Hz reads zero; the next ones show the ADC's DC offset (about +70 dB over the floor). An internal spur sits at 24.3 MHz, about +13 dB over the floor with no antenna. It is unexplained; 777.6 MHz / 32 is one candidate.

Not verified: any real signal (no antenna was connected), levels in dBm, the 129.6 Msps rate, and a USB 2 path (the booted device came up at 5000 Mb/s).
