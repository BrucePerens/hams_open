---
title: bench_rx888_capture
summary: RX-888 through radiod: seconds of I/Q or demodulated audio from a channel, saved as a 16-bit WAV.
see_also: [instruments/rx888/overview, instruments/rx888/operation, instruments/rx888/limits]
tools: [bench_rx888_capture]
instrument: rx888
tier: read_config
verified: measured on the unit (RX-888 MkII, no antenna), 2026-11-05
---

# bench_rx888_capture

Opens a channel (SSRC `ssrc_base + 2`) at `freq_hz`. For `kind` iq: two channels (I, Q), plus or minus 5 kHz, at a fixed gain (`gain_db`, or chosen to put the noise near -30 dBFS). For `kind` audio: `mode` with AGC. Receives the RTP stream on the data group for `seconds`, renewing the channel's lifetime each second, writes `rx888_<instrument>_<kind>_<ms>.wav` in `BENCH_MCP_OUT`, and removes the channel.

## Arguments

<!-- arguments -->

## Returns

`wav`, `kind`, `mode`, `frequency_hz`, `sample_rate_hz`, `channels`, `frames`, `seconds`, `rtp_packets`, `rtp_lost`, `rtp_out_of_order`, `rtp_restarts` (the channel's sequence started again: `radiod` re-created it), `rtp_skipped`, `gain_db`, `rms_dbfs`, `peak_counts`.

## Failure modes

* `no audio arrived`: check bench_rx888_status and loopback multicast.

Example: `bench_rx888_capture {"freq_hz": 10000000, "seconds": 2}` returned `{"channels": 2, "frames": 24000, "rtp_packets": 100, "rtp_lost": 0, "gain_db": 91.2, "rms_dbfs": -34.2, ...}`.
