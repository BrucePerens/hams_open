---
title: Hamlib limits and safety model
summary: What the read and control tools enforce, where each limit comes from, and what is not verified.
see_also: [instruments/hamlib/setup, concepts/safety-tiers, concepts/extending/standing-rules]
instrument: hamlib
verified: dummy rig only, 2026-10-29
---

# Hamlib limits and safety model

* Tiers: information tools are `read_only`; changing the rig is `read_config` or higher. The server refuses a tool above the instrument's tier and does not list it.
* Every rigctld command has a timeout (`reply_timeout_s`, default 3 s). Reads are tried twice, over a new connection the second time; commands that change the rig are never repeated blind. A rig that does not answer is an error with a next step; no value from before a failure is reused.
* Arguments sent to rigctld are limited to letters, digits and `_ . + - / :` and 64 characters, so no argument can carry a second command. There is no raw-command tool.
* rigctld listens on 127.0.0.1 only. The config refuses a remote rigctld unless the owner sets `allow_remote_rigctld`; it then has no authentication, and the owner accepts that.
* The managed rigctld is stopped when the server stops. If the server is killed (SIGKILL), the daemon is left running until the next start of the server stops it; the pages on transmitting say what that means for a keyed rig.
* A serial device that is missing, or that this user may not open, is reported before a daemon is started, because rigctld keeps running after it fails to open a rig.
* Receive frequencies must lie inside the rig's reported receive range and the owner's `rx_ranges_hz`; a scan is at most 400 steps and 120 s. The receiver-control tool never sets a transmit-side level or function and refuses to change anything while the rig reports PTT on.
* Starting the daemon opens the serial port, and opening a port sets the DTR and RTS lines. Some interfaces key the transmitter from one of them. If the rig has such an interface, set `dtr_state` and `rts_state` to OFF (and `ptt_type`) in `set_conf` before the first start. `bench_hamlib_diagnose` reports what the config says and never opens the port itself.
* The load plausibility check is evidence, not proof. It listens, with the rig in receive, for what a real antenna hears and compares it with a baseline recorded when the owner attested the load. An open or unplugged connection and an antenna on a dead band at low gain also hear nothing; the baseline is valid only for the cabling and settings it was recorded with; hearing no reference station proves nothing, because propagation can fail on all of them; and a station's ticks in the audio, a reading above the baseline, an unsteady reading, an unreadable S-meter or a dead audio input refuse. The owner's attestation in the config gates transmitting; the software cannot know what is connected. The station table is a starting point and is reviewed by the owner: stations go off the air, and a silent one is "not heard", never a failure.
* What the driver reports is what Hamlib's backend reports. A backend can declare a function the firmware lacks (the rig then answers "not available"), and a rig's S-meter scale is its maker's choice.
* Not verified: any real rig. All tests ran against Hamlib's dummy rig (model 1) and a scripted fake of rigctld. `verified: dummy rig only` on these pages means that. A first test on a real rig is the owner's to run; read tools first.
