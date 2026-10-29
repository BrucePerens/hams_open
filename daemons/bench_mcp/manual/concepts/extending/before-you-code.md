---
title: Before you write code
summary: Check for existing work, state the plan, and ask the owner for the facts and the permission that only they can give.
see_also: [concepts/extending, concepts/extending/standing-rules, concepts/etiquette]
verified: code only, 2026-10-29
---

# Before you write code

## 1. Is it already done or in progress

* `git fetch origin && git log origin/main --oneline -- daemons/bench_mcp | head -30`, and `ls daemons/bench_mcp/manual/instruments`.
* `gh pr list -R BrucePerens/hams_open --search "bench_mcp <model>" --state all` and `gh issue list -R BrucePerens/hams_open --search "<model>"`.
* If someone has the same instrument, continue their branch (ask in their PR thread) instead of writing a second copy.
* If not, open a draft PR early, or an issue using `.github/ISSUE_TEMPLATE/bench_instrument_plan.md`, that states the model, the planned tools and the tiers. Do this before you have written much; a maintainer can then say "split it", "use that crate" or "not on this tier" while that is cheap. The owner approves the text first ([submit](concepts/extending/submit)).

## 2. Ask the owner (the facts only they have)

Put each question to the owner and write the answers into the plan and later into the `setup` and `limits` pages:

* Make, model, firmware version, and the connection (LAN, USB, serial adapter, GPIB). The address or device path goes into their own config, never into the repository.
* The rated limits of the unit and of everything connected to it: maximum voltage, current, power, frequency, input levels. What is on the bench: what is connected to which port, any antenna, any device that could be damaged, anything live from the mains.
* Which limits they want enforced beyond the rating (the config caps).
* What the agent may do: read only, change settings, or drive an output. For each driving, radiating or mains function: whether it may be tried at all, at what level, into what load.
* Whether any program or person uses the instrument at the same time, and whether it has a reserved or shared role (see [reserved devices](concepts/reserved-devices)).
* Their local legal position if the instrument can transmit. That is the owner's responsibility, not yours or the maintainer's, and the instrument's `limits` page says so.

## 3. Permission to touch the hardware

Do not open the device, send any command or install anything on the owner's machine until the owner has said, in this conversation, that you may. State what you will do in the first step: identify and read status only. Then widen step by step, each time asking before the first control command and before any function in a higher tier.

* Start read-only: identity, status, a measurement. Control comes after the read path is tested and fixtures are recorded.
* Never exceed a rated limit, to test a limit or for any other reason.
* Document each destructive, radiating, high-voltage or mains-connected action, put the tool in the `full` tier, give it `confirm` and `dry_run` ([standing rules](concepts/extending/standing-rules)).
* If you cannot find a safe way to test a function, say so in the pull request and leave the function out. An untested actuator is worse than a missing one.

## 4. The plan you write down

Instrument and firmware; connection; tool list with name, tier and one line each; config keys; what the fake device will simulate; what the owner will run on the hardware; the sources you will learn the protocol from ([provenance](concepts/extending/provenance)).
