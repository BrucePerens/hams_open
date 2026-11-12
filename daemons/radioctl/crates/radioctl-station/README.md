# radioctl-station

Station control for radioctl: external switches, power sequencing, interlocks, powered-off radio
records and relay-board drivers. Placement decision (Bruce, 2026-10-30): the station-control layer
is a radioctl module, built as a new crate in this workspace so it can be built and tested apart
from the rig crates. Proposal: `docs/proposals/RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md`.

Phase 1 is virtual: no hardware, nothing is ever switched at start-up, and every transmit-related
path stays behind radioctl's closed gate.

Which relay and switch products suit it, and which do not: [docs/RECOMMENDED_HARDWARE.md](../../docs/RECOMMENDED_HARDWARE.md).

**Interlock rule (decided by Bruce, 2026-10-31; emergency operations are never blocked).** The
guard classifies every command by what it does to the RF path (`station::Action`):

- **Power-off of any device** (the amplifier, the rig; the standby step an amplifier power-off
  begins with) passes in every keyed state: known keyed, unknown (link down or never read) and
  unkeyed. Cutting power can only stop a transmission. No confirmation, no password.
- **Power-on of a radio** passes in every keyed state too: an off radio cannot be keyed, and a
  radio whose link dropped must be power-cyclable. No confirmation, no password. The power button
  (on, off, reboot) follows.
- **Shared-supply protection, opt-in, off by default:** `shared_supply = true` on a radio's power
  channel (set it when that channel also feeds an amplifier, antenna switch or other RF-path
  device) makes that radio's power-on an RF-path change: refused while any attached rig is keyed
  or its keyed state is unknown, and a reboot is refused up front rather than after cutting power.
  The flag is accepted only on the power channel of a radio (`StationDef::validate`).
- **Everything that could hot-switch an RF path** (antenna selection, amplifier power-on and
  operate, band switching, any other output) is refused while any rig is known keyed (`Refused`)
  or its keyed state is unknown (`KeyedUnknown`).
- The emergency `all_off` is never blocked and does not look at the keyed state at all.

History: the first rule treated an unknown keyed state as keyed and blocked everything but
`all_off`, which made a radio whose link dropped impossible to power-cycle; 2026-10-30 let
power-off and the power-on of a down radio pass an unknown state (confirmed by Bruce 2026-10-31),
and the same day Bruce decided the rest: power-off and radio power-on pass every keyed state, the
shared-supply protection is the opt-in exception. Nothing else relaxes. None of this touches the
transmit gate, which is `radioctl-core`'s and closed by default: no keyed state and no power
command opens it (`tests/station_rig.rs` pins both against a real rig actor).

## Radio lifecycle and records

`lifecycle` is the sans-IO state machine (Off, PoweringOn, Booting, Ready, Degraded, Unreachable)
fed by the switch channel state, the rig actor's link state and hotplug or ping evidence.
`record` is the durable radio record: one JSON file, mode 0600, atomic replace; a corrupt file is
set aside and never trusted, a bad record is dropped. Neither switches anything, and nothing is
restored to a radio by itself after it reconnects.

`manager` joins them: the station-level event stream (`RadioEvent`, a broadcast receiver plus a
callback helper), the power button (on, off, reboot; interlocks and dependency graph apply),
reconnect with backoff after power-on, and the start-up reconcile (reads each power channel back,
writes nothing, reports where the record and the hardware disagree). `VirtualRadioLink` is the
virtual rig for tests (boot delay, never boots, dropped link). Record saves use blocking `std::fs`
for a small file; a caller on a busy runtime may wrap `persist` in `spawn_blocking`.

## Real rigs behind the manager

`actor_link::ActorLinks` is the `RadioLink` of real rig actors. The application supplies a
`RigSpawner` (open the transport, build the engine, spawn the actor, keep the transmit gate closed).
`connect` then does what `RigLinks` left to the caller: it spawns the actor, and respawns it when it
is dead (a transport that ended, such as a plain TCP stream at end of file), the manager's backoff
(250 ms doubling to 8 s) spacing the attempts. Three rules:

- **The unkey pass.** A freshly spawned actor does not unkey on its first connection, but the radio
  may have been keyed when the link died. So the new actor is sent an unkey (the emergency path:
  no gate, never refused) before the link is reported up, and an unkey that fails is a failed
  connection: the actor is dropped, nothing is adopted, the manager backs off and tries again.
  Only reads follow (frequency and mode, reported, never restored).
- **No second actor.** A transport that reconnects by itself (`radioctl_core::net::open_link`, the
  serial link) keeps its actor and reports `Reconnecting`; `ActorLinks` then refuses to respawn
  and the actor unkeys on its own when the link returns (`Alarm::LinkRestored`).
- **The interlock follows the actor.** `ActorLinks::key_source` is the keyed-state source to give
  the `Station`: it looks at the radio's current actor on every call (unknown with none), so a
  respawn does not leave the interlock watching a dead handle.

The off time of a power channel (`min_off_ms`) is counted from a commanded or observed turn-off,
never from the first sight of the channel: a radio found off at start-up can be switched on at once.

`spawner::DescriptionSpawner` is the ready-made `RigSpawner`: per radio a `RigSpec` (the rig
description, an endpoint: a TCP address with an optional framing factory such as the TS-890S LAN
login, or a serial port, and the actor options). It builds the engine for the description, opens a
reconnecting transport and spawns the actor with the transport's link status; the transmit gate stays
closed and credentials never pass through it (a framing factory closes over what the application read
from its secret store).

## Amplifier and tuner channels

`type = "amplifier"` channels belong to an amplifier or tuner device. `function = "operate"` is a
binary output (on = operate, off = standby) and must declare `safe = "off"`; `function = "tune"` is
a momentary pulse (`pulse_ms`) that starts the device's own tune cycle (the station never keys a
transmitter). Rules: operate is an RF-path change, so it is refused while anything is keyed or the
keyed state is unknown; standby is the safe direction and passes every keyed state (like a
power-off, of which it is the first step). Operate is refused unless the device is known powered on (confirmed, or recorded on a board that cannot be read back) and, when the channel names
a `fault_input` (an input channel, high = tripped), unless that line is confirmed clear; a tripped
or unknown line refuses. Powering an amplifier off puts it in standby first, and the emergency
`all_off` writes standby before anything else.

## Rotators

`rotator` holds the descriptions (`data/station/rotators.toml`), the `rotctld`, GS-232 and Easycomm engines and
the `Rotator` command handle; `virtual_rotator` is the virtual rotator and a loopback server for it.
See the manual, chapter 14. Positions outside the travel are refused, never clamped; every wait is
bounded; nothing is written to a down link; nothing is restored after a reconnect.

## Home Assistant

`homeassistant::HomeAssistant` is a channel driver whose lines are Home Assistant entities
(`switch`, `input_boolean`, `light`, `fan` to switch; `binary_sensor` to read). It follows the design
in `RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md` 4.3a:

- State and confirmation come from the WebSocket API (`subscribe_events` for `state_changed`, plus
  a `get_states` snapshot); commands are REST service calls. While the session is down, or an
  entity is `unavailable`, `unknown` or missing, its state is **unknown**: `read_line` fails, the
  switchboard keeps no confirmed value, and the dependency checks that need it confirmed refuse.
  A command is still sent while unknown, because an off must always be tried.
- **Assurance level.** Drivers report an `Assurance`; this one is `Mediated`, the lowest. The
  switchboard refuses a `ptt` channel on it, and `Station::new` refuses an amplifier powered through
  it unless the device declares `external_deadman = true` (a hardware dead-man or plug-level
  auto-off the owner has set up). `Station::power_assurance` tells a front end what to show.
- **Relay boards that cannot be read back** (SainSmart 16-channel; Bruce, 2026-11-01, DESIGN
  decision 23). `status_readable = false` and `safe_state` are board data. The Modbus driver never
  sends function 1 for such a board, reports `Assurance::Recorded` (acknowledged write, state is our
  own record, never confirmed), writes the safe state in one function 15 frame on first open and
  after every `ModbusRelay::reopen` before any other command (failure: coils unknown, nothing else
  sent, retry with backoff), and keeps a shadow of the 16 coils (`ModbusRelay::record`,
  `Switchboard::recorded`). The emergency all-off writes the safe state and is never blocked.
- **The token** comes from a `SecretStore` by name (`FileSecrets` reads mode-0600 files;
  `MemorySecrets` is for tests), lives in a `Secret` whose `Debug` is redacted, and is sent only as
  the bearer header and the WebSocket auth message. Not in config, errors, the audit log or logs.
- **Private IP literals only** (default port 8123). The link is plain HTTP and WebSocket unless
  `HaConfig::tls` is set: then HTTPS and `wss`, trusting only the owner's CA (PEM; the address must
  be an IP entry of the certificate) or one pinned SHA-256 certificate fingerprint (`tls.rs`).
  Verification is never off and the system's public roots are not used. Tested against a fake
  Home Assistant with a CA-signed certificate; not run against a real Home Assistant.
- **Audit.** Every service call, session start, authentication result, loss and discovery goes to
  an `AuditLog`. Wiring it to the relay's audit log is the integrator's step.
- `discover()` lists what Home Assistant offers, to choose from; only configured entities are ever
  written. The `ws` module is a small RFC 6455 codec (no dependency); tested against the RFC's
  accept-key example and a fake Home Assistant.

## Anchors

Function anchors whose full entries (purpose, signature, arguments, returns, errors, special concerns; ADR 0113) are one `###` heading each in the story files [`docs/stories/radioctl_function_anchors_44.md`](../../docs/stories/) onwards are not repeated here; the rest are listed below.


