---
title: Keeping the sound server off the VNA's codecs
summary: Why the VNWA's two USB codecs must be hidden from the desktop sound server, how bench_status reports it, and how to fix it.
see_also: [instruments/vna/monitoring-and-recovery, instruments/vna/troubleshooting, concepts/reserved-devices]
tools: [bench_status]
instrument: vna
verified: fixtures only (parsing of pw-dump and pactl output, the installer against a staging root); the hidden state was seen on the dev box 2026-10-29
---

# Keeping the sound server off the VNA's codecs

The VNWA's two USB codecs (USB `08bb:29c0`) are its receivers. A sound server that manages them can send system sounds into the analyzer and can hold a card open during a sweep.
`bench_status` reports `codecs_protected_from_sound_server`: `true` when the server lists none of the unit's codecs, `false` when it lists one (with `codecs_protection_detail` giving the card
and its server id, and `codecs_protection_warning` giving the fix), and `null` when it cannot tell (no sound server, no `pw-dump` or `pactl`, or no VNWA plugged in). A sweep never fails because
of it.

The codecs are recognised by topology: a `08bb:29c0` device with the VNWA control chip `20a0:4118` beside it under the same hub. Another `08bb:29c0` dongle elsewhere is an ordinary sound card.

Fix: `vnwa_open install-system-config` (a dry run; add `--apply`). It writes a WirePlumber rule and a narrow udev rule under `/etc`, never restarts the audio service and never re-announces a device;
the rules apply at the next login or after replugging the unit. To hide an already-listed card now: `wpctl set-profile <sound_server_id> 0`.
