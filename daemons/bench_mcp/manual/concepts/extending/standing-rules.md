---
title: Standing safety rules for instrument modules
summary: Tiers, radiating and mains limits, privacy, and the five rules that come from real incidents. Every module obeys all of them.
see_also: [concepts/extending, concepts/safety-tiers, concepts/reserved-devices, instruments/sound/safety]
verified: code only, 2026-10-29
---

# Standing safety rules for instrument modules

## Safety tiers: how to choose

| Tier | Use it for | Examples in this repository |
|---|---|---|
| `read_only` | reads that change nothing the equipment keeps | `bench_status`, `bench_measure`, `bench_example_status` |
| `read_config` | changes settings or stimulates something that cannot hurt the equipment or the bench; anything that only moves toward a safe state | `bench_set` within caps, `bench_vna_sweep`, `bench_example_safe_state` |
| `full` | can damage, radiate, drive a load, make noise, or stop a running device | `bench_psu_output` on, `bench_scope_self_cal`, `bench_usb_port_power` off, `bench_example_pulse` |

Choose the lowest tier at which the tool cannot do harm if called wrongly by a confused model. When one tool has a safe and an unsafe form (supply output off or on), check the argument inside the tool as well as the tier (`bench_psu_output`). The config owner raises a tier; a tool never asks the model to.

## Transmit and RF

Levels stay low. Nothing radiates: a generator drives a cable port into a load, not an antenna; no tool connects or selects an antenna unless the owner has said so for that instrument. State the maximum level into the intended load in the `limits` page. Legal compliance of any transmission (licence, band, power, bandwidth, duty cycle) is the owner's responsibility; the instrument's `limits` page says so in those words.

## Mains, high voltage, high current

State the rated limit, enforce a lower owner cap from the config before any byte is sent, require `confirm` from the person at the bench for any function that connects a dangerous source, and give the tool a safe-state companion. A function you cannot test safely is left out.

## Privacy

Recordings, serial numbers, MAC and IP addresses and credentials are never committed. Recordings from microphones stay in the owner-only capture directory and are never uploaded. Config examples contain no secrets. Fixtures are scrubbed ([provenance](concepts/extending/provenance)); `cargo test` scans fixtures, pages and the README for public IP addresses, MAC addresses and credential words.

## The five incident rules

These come from incidents of this week. They apply to every module, existing and new.

1. **Never restart or reload a user's desktop audio service, and never trigger device re-enumeration from our tools.** No `systemctl restart pipewire`, `wireplumber`, `pulseaudio` or `alsa`, no `modprobe -r`, no USB unbind, reset, deauthorize or port power on a device that is not the instrument's own and marked by the owner. Re-enumeration moves card numbers, drops the user's calls and can disconnect their keyboard. If a problem seems to need it, report it and let the person do it. The USB tools refuse protected devices (`bench_usb_protection`) and take `dry_run`.
2. **Reserved devices are identified by topology and ids and refused by name; streams never fall back to the system default.** Identify hardware by USB vid:pid and its place in the USB tree, not by a card number or name that can change. Refuse a reserved device in every tool (`audio::Config.reserved`, VNA codecs `08bb:29c0`). A stream you open names its target node and sets `node.dont-fallback=true node.dont-reconnect=true` (`src/audio/engine.rs`, `NO_FALLBACK`), so that a vanished target is an error and not a silent move to whatever is the default now (which may be a reserved device or the user's headset). Do not add a new "default device" fallback.
3. **Every tool that can radiate or actuate needs `dry_run`, a hard timeout and a safe-state action.** `dry_run` returns the plan and sends nothing. The hard timeout is held by the instrument where it can (`bench_example_pulse` sends `OUTP:TMO` before `OUTP ON`) and by the tool in any case. The safe-state action is a tool that puts the instrument in its safe state and reads it back (`bench_example_safe_state`). A failure to confirm the safe state is a loud error that says what a person does by hand. `cargo test` fails a new `full` tool without `dry_run` (`src/checklist.rs`).
4. **Tests must not play audible sound outside the allowed hours or key a transmitter.** Unit tests use fixtures, a fake device or a virtual sink (`pw-loopback`, `tests/hil_audio.rs`); no test drives a real speaker, a real output or a real transmitter. A hardware test that needs sound or RF is `#[ignore]`, run only by the owner, only in the hours the owner allows (the sound module's quiet hours default to 22:00 to 07:00), and states what it will do before it does it. No test is skipped or altered to avoid this: it is written so it does not need to.
5. **Comments in any system file we install say what, why, scope, caveat and how to remove, and "added by bench_mcp" with the repository root URL.** A system file is a sudoers rule, a udev rule, a systemd unit, a config under `/etc`. The header comment of each, in this form:

```
# Added by bench_mcp (https://github.com/BrucePerens/hams_open).
# What: lets the user <name> run <exact command> as root without a password.
# Why:  <the tool that needs it> switches USB port power.
# Scope: only that command, only that user; it grants nothing else.
# Caveat: the binary must be owned by root and not writable by the user, or this rule runs anything.
# Remove: delete this file; the tool then reports that the rule is missing.
```

   Nothing is installed without the owner's yes in the conversation; the module documents the file in its `setup` page, and the agent shows the owner the exact text first. Today no module installs a file itself (`usb_hub/setup` documents the sudoers rule for the owner to add).

## Also

* No tool accepts a free-form command string for the instrument.
* No tool changes its behaviour because it is running in a test; the tests use the production path with a fake far end.
* Bounded everything: timeouts, tries, sizes of captures, durations.
