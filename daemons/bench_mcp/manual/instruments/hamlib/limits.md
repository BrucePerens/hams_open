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
* What the driver reports is what Hamlib's backend reports. A backend can declare a function the firmware lacks (the rig then answers "not available"), and a rig's S-meter scale is its maker's choice.
* Not verified: any real rig. All tests ran against Hamlib's dummy rig (model 1) and a scripted fake of rigctld. `verified: dummy rig only` on these pages means that. A first test on a real rig is the owner's to run; read tools first.
