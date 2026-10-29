---
title: Scope protocol notes
summary: SCPI summary used by the driver, with the quirks of this firmware.
see_also: [instruments/scope/limits]
instrument: scope
verified: probed on the unit (SDS1202X-E, firmware 1.3.27), 2026-10-28
---

# Scope protocol notes

Text commands over TCP 5025, newline terminated. No SCPI manual for this scope was available; the commands were found by probing. Used: `*IDN?`, `TRIG_MODE?` and `TRIG_MODE <AUTO|NORM|SINGLE|STOP>`, `TDIV`, `SARA?`, `Cn:VDIV`, `Cn:OFST`, `Cn:CPL`, `Cn:ATTN`, `Cn:PAVA? <item>`, `WFSU SP,1,NP,0,FP,0`, `Cn:WF? DAT2` (8-bit samples, scaled as value times vdiv over 25 minus offset), `MSIZ`, `TRSE <type>,SR,Cn`, `SCDP` (screen image), `*CAL?`. The scope's FFT (`FFTW`, `FFTC`, `FFTS`, `FFTU`, `DEFINE EQN,"FFT(C1)"`, `MATH:WF? DAT2`) answers but its frequency axis was not decoded. Clean-room: derived by probing the instrument, not from vendor software.
