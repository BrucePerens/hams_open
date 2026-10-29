---
title: bench_usb_history
summary: Connect, disconnect and enumeration timeline from the kernel journal with USB error counters and flapping detection.
see_also: [instruments/usb-hubs/operation, instruments/usb-hubs/troubleshooting]
tools: [bench_usb_history, bench_usb_watch]
instrument: usb-hubs
tier: read_only
verified: run on the dev box journal with use_sudo (299 connects, 7 enumeration failures, -71 and -32 counts for 3-2.2.1), 2026-10-29
---

# bench_usb_history

Reads `journalctl -k -o short-unix` (current boot unless `all_boots`) and returns the timeline and, per device, counters: connects, disconnects, resets, enumeration failures
("unable to enumerate USB device"), device descriptor read failures, address errors, kernel "attempt power cycle" lines, over-current, `-19` "cannot submit urb ... no device" lines,
xhci errors and errno counts (`-71` EPROTO, `-32`). Flapping: `flap_connects` (default 3) or more connects of one device within `flap_window_s` (default 120 s).

Arguments: `device` (path or label; includes everything below it), `since_s`, `max_events` (default 100, at most 1000), `flap_window_s`, `flap_connects`, `all_boots`, `use_sudo`.

The kernel journal is readable only by root and the groups `adm` and `systemd-journal`. Without that the call fails naming the group; `use_sudo: true` runs `sudo -n journalctl -k` for that call only.
Counted `-19` lines mean software kept submitting to a device that had already gone; they are not a link fault by themselves.

## Arguments

<!-- arguments -->
