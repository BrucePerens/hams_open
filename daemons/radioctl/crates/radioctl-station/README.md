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

`rotator` holds the descriptions (`data/station/rotators.toml`), the `rotctld` and GS-232 engines and
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

- [@ANCHOR: radioctl:radioctl_station_modbus_sainsmart] (radioctl-station) The SainSmart 16-channel 9-36 V USB relay module (sku 101-70-208): Modbus ASCII, 9600
- [@ANCHOR: radioctl:radioctl_station_modbus_from_toml] (radioctl-station) Parses a board description.
- [@ANCHOR: radioctl:radioctl_station_modbus_lrc] (radioctl-station) The Modbus ASCII checksum: the two's complement of the byte sum, low byte.
- [@ANCHOR: radioctl:radioctl_station_modbus_crc16] (radioctl-station) The Modbus RTU CRC-16 (polynomial 0xA001 reflected, start 0xFFFF).
- [@ANCHOR: radioctl:radioctl_station_modbus_hex_val] (radioctl-station) One hex digit's value.
- [@ANCHOR: radioctl:radioctl_station_modbus_encode_ascii] (radioctl-station) An ASCII frame: `:`, slave and PDU as upper-case hex, the LRC, CR LF.
- [@ANCHOR: radioctl:radioctl_station_modbus_decode_ascii] (radioctl-station) Checks an ASCII frame (colon, even hex digits, CR LF, LRC) and returns slave and PDU.
- [@ANCHOR: radioctl:radioctl_station_modbus_encode_rtu] (radioctl-station) An RTU frame: slave, PDU, CRC-16 low byte first.
- [@ANCHOR: radioctl:radioctl_station_modbus_decode_rtu] (radioctl-station) Checks an RTU frame's CRC and returns slave and PDU.
- [@ANCHOR: radioctl:radioctl_station_modbus_pdu_write_coil] (radioctl-station) Function 5: write one coil (`FF00` on, `0000` off).
- [@ANCHOR: radioctl:radioctl_station_modbus_pdu_write_coils] (radioctl-station) Function 15: write `bits.len()` coils from `start`, bit 0 of the first data byte first.
- [@ANCHOR: radioctl:radioctl_station_modbus_pdu_read_coils] (radioctl-station) Function 1: read `count` coils from `start`.
- [@ANCHOR: radioctl:radioctl_station_modbus_parse_reply] (radioctl-station) Checks a reply PDU against the request PDU it answers. Fails closed: an exception code, a
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_new] (radioctl-station) A driver for `board` over `io`. Nothing is sent.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_coil] (radioctl-station) The coil address of a line, bounds-checked.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_read_frame] (radioctl-station) Reads one reply frame: to CR LF (ASCII) or to an idle gap (RTU), bounded in size and time.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_write_all] (radioctl-station) Function 15 over every coil: all on or all off in one frame.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_write_line] (radioctl-station) Function 5 on the line's coil.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_read_line] (radioctl-station) Function 1 on the line's coil.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_can_read_back] (radioctl-station) Whether the board description reads coils back.
- [@ANCHOR: radioctl:radioctl_station_modbus_safe_bits] (radioctl-station) The safe level of every coil, in coil order: `safe_state`, or all off when it is empty.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_reopen] (radioctl-station) Replaces the byte stream after a reconnect or reopen: every coil becomes unknown and the safe state is required again before any other command.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_exchange] (radioctl-station) Sends one request PDU on the locked stream and returns the validated reply; read coils is refused for a board whose status is not readable.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_apply_safe] (radioctl-station) Writes the board's safe state in ONE function 15 frame and records it on an acknowledgement; a failure leaves every coil unknown and extends the backoff.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_ensure_safe_state] (radioctl-station) Applies the safe state if it has not been applied on this link; nothing else goes out before it succeeds.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_write_gated] (radioctl-station) The one path for a command write: safe state first, then the request; the affected coils are recorded on an acknowledgement and become unknown on any failure.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_record] (radioctl-station) The driver's record of every coil in order: the level the board last acknowledged and its age, or unknown. Recorded, not confirmed.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_assurance] (radioctl-station) `Assurance::Recorded` for a board whose status is not readable, otherwise `HostSupervised`.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_safe_line] (radioctl-station) The board's safe level for a line (`safe_state`, off when empty).
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_force_safe] (radioctl-station) The emergency path: the board's whole safe state in one frame, ignoring the retry delay. Never blocked.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_recorded_line] (radioctl-station) The record of one line: recorded, not confirmed.
- [@ANCHOR: radioctl:radioctl_station_rig_name] (radioctl-station) The rig id.
- [@ANCHOR: radioctl:radioctl_station_rig_key_state] (radioctl-station) Keyed state from the rig handle: unknown when the link is down; the rig's own PTT read
- [@ANCHOR: radioctl:radioctl_station_station_new] (radioctl-station) Builds a station over a switchboard that has every channel of `def`. Nothing is switched.
- [@ANCHOR: radioctl:radioctl_station_station_board] (radioctl-station) The switchboard, for status and events. Switching goes through the station.
- [@ANCHOR: radioctl:radioctl_station_station_def] (radioctl-station) The definition.
- [@ANCHOR: radioctl:radioctl_station_station_guard] (radioctl-station) The interlock: errors unless every keyed source reports unkeyed. A refusal is an event.
- [@ANCHOR: radioctl:radioctl_station_station_device_power] (radioctl-station) The confirmed power state of a device: always-on is `On`; a channel's confirmed value;
- [@ANCHOR: radioctl:radioctl_station_station_set_channel] (radioctl-station) Sets a power, output or selector channel by hand, behind the interlock. A power channel
- [@ANCHOR: radioctl:radioctl_station_station_select] (radioctl-station) Selects a way on a selector, behind the interlock.
- [@ANCHOR: radioctl:radioctl_station_station_pulse] (radioctl-station) Pulses a momentary channel, behind the interlock.
- [@ANCHOR: radioctl:radioctl_station_station_check_power_on] (radioctl-station) The dependency refusals: dependencies known on, required antennas selected.
- [@ANCHOR: radioctl:radioctl_station_station_power_on] (radioctl-station) Powers a device on, behind the interlock for its class (a radio passes every keyed state depends on is not
- [@ANCHOR: radioctl:radioctl_station_station_power_kind] (radioctl-station) The channel kind that powers a device (an output kind for none).
- [@ANCHOR: radioctl:radioctl_station_station_wait_ready] (radioctl-station) Waits for a device to answer, up to its ready timeout.
- [@ANCHOR: radioctl:radioctl_station_station_power_off] (radioctl-station) Orderly power-off: never refused for the keyed state (known keyed, unknown or unkeyed:
- [@ANCHOR: radioctl:radioctl_station_station_run_profile] (radioctl-station) Runs a profile's steps in order within its timeout, with a `Step` event at each. A device
- [@ANCHOR: radioctl:radioctl_station_station_stop_profile] (radioctl-station) Powers off the devices a profile powers on, in reverse order, with the off delays and the
- [@ANCHOR: radioctl:radioctl_station_station_all_off] (radioctl-station) The emergency all-off. Never blocked: no keyed-state check, no minimum times, no delays,
- [@ANCHOR: radioctl:radioctl_station_station_guard_action] (radioctl-station) The interlock with the action's class (Bruce, 2026-10-31): an [`Action::PowerOff`] and an
- [@ANCHOR: radioctl:radioctl_station_station_power_on_action] (radioctl-station) The class of powering device `id` on, for callers that must know before they start (a
- [@ANCHOR: radioctl:radioctl_station_station_classify_on] (radioctl-station) The class of powering `d` on: a radio is [`Action::PowerOnRadio`] unless its power channel
- [@ANCHOR: radioctl:radioctl_station_station_classify_channel] (radioctl-station) The class of setting a channel by hand: off on a power channel is [`Action::PowerOff`]; on
- [@ANCHOR: radioctl:radioctl_station_station_classify_pulse] (radioctl-station) The class of pulsing a momentary line: a device's off line is [`Action::PowerOff`], its on

- [@ANCHOR: radioctl:radioctl_station_station_power_assurance] (radioctl-station) The assurance level of the channel that powers a device.
- [@ANCHOR: radioctl:radioctl_station_station_check_operate] (radioctl-station) The refusals for putting an amplifier in operate: its device must be known powered on, and its fault input, when it has one, must be confirmed clear (a tripped or unknown fault line refuses: fail closed).
- [@ANCHOR: radioctl:radioctl_station_station_check_amp_powered] (radioctl-station) Refuses unless the amplifier or tuner is known powered on (confirmed, or recorded on a board that cannot be read back).
- [@ANCHOR: radioctl:radioctl_station_station_standby] (radioctl-station) Puts a device's amplifier channel in standby before its power goes, best effort: a failure is already an event on the switchboard and does not stop the power-off, because removing power is the safe direction.
- [@ANCHOR: radioctl:radioctl_station_vrot_step] (radioctl-station) Moves `from` toward `to` by at most `amount`.
- [@ANCHOR: radioctl:radioctl_station_vrot_new] (radioctl-station) A rotator at its minimum travel, at rest.
- [@ANCHOR: radioctl:radioctl_station_vrot_advance] (radioctl-station) Lets time pass: moves toward the target at the slew rate unless stuck.
- [@ANCHOR: radioctl:radioctl_station_vrot_state] (radioctl-station) The state now.
- [@ANCHOR: radioctl:radioctl_station_vrot_received] (radioctl-station) Every command line received, in order.
- [@ANCHOR: radioctl:radioctl_station_vrot_set_target] (radioctl-station) Accepts a target if it is inside the travel.
- [@ANCHOR: radioctl:radioctl_station_vrot_feed] (radioctl-station) Bytes from the client; returns the reply bytes (none when silent).
- [@ANCHOR: radioctl:radioctl_station_vrot_rotctld] (radioctl-station) One rotctld command line.
- [@ANCHOR: radioctl:radioctl_station_vrot_gs232] (radioctl-station) One GS-232 command (CR-terminated, already trimmed): `Waaa eee`, `C2`, `S`.
- [@ANCHOR: radioctl:radioctl_station_vrot_lock] (radioctl-station) `lock` (see the source).
- [@ANCHOR: radioctl:radioctl_station_vrot_control_state] (radioctl-station) Where the rotator is and is going.
- [@ANCHOR: radioctl:radioctl_station_vrot_control_received] (radioctl-station) Every command line received.
- [@ANCHOR: radioctl:radioctl_station_vrot_control_set_stuck] (radioctl-station) Makes the rotator stuck (or frees it).
- [@ANCHOR: radioctl:radioctl_station_vrot_control_address] (radioctl-station) The socket the virtual rotator is served on, if the listener could report it.
- [@ANCHOR: radioctl:radioctl_station_vrot_control_stop] (radioctl-station) Stops the listener task and drops the client connection it holds, closing the port; a second call does nothing.
- [@ANCHOR: radioctl:radioctl_station_vrot_control_set_silent] (radioctl-station) Makes the rotator silent (or lets it answer).
- [@ANCHOR: radioctl:radioctl_station_vrot_serve] (radioctl-station) Serves `rotator` on `listener` from a task on `runtime`; one client at a time, a new connection replaces the old one.
- [@ANCHOR: radioctl:radioctl_station_meter_driver_name] (radioctl-station) The configuration name.
- [@ANCHOR: radioctl:radioctl_station_meter_driver_from_name] (radioctl-station) The driver with a configuration name.
- [@ANCHOR: radioctl:radioctl_station_meter_driver_unit] (radioctl-station) The unit the driver reports in (both read watts).
- [@ANCHOR: radioctl:radioctl_station_meter_driver_default_baud] (radioctl-station) The serial speed to use when the configuration gives none (38400).
- [@ANCHOR: radioctl:radioctl_station_meter_driver_requests] (radioctl-station) The requests of one poll, in order. Each is sent after the previous one's reply.
- [@ANCHOR: radioctl:radioctl_station_meter_driver_has_reflected] (radioctl-station) Whether the meter reports reflected power.
- [@ANCHOR: radioctl:radioctl_station_meter_parse_kpa500] (radioctl-station) Parses a KPA500 `^WSppp sss;` frame into the output watts.
- [@ANCHOR: radioctl:radioctl_station_meter_parse_kpa1500] (radioctl-station) Parses one KPA1500 `^PWF` or `^PWR` frame: the tag and the watts.
- [@ANCHOR: radioctl:radioctl_station_meter_poller_new] (radioctl-station) A poller for `driver` over `io`. Nothing is sent until [`MeterPoller::poll`].
- [@ANCHOR: radioctl:radioctl_station_meter_poller_read_frame] (radioctl-station) Reads one `;`-terminated reply frame, bounded in size and time.
- [@ANCHOR: radioctl:radioctl_station_meter_poller_poll] (radioctl-station) Asks the meter once and returns what it said; a timeout or malformed reply is an error.
- [@ANCHOR: radioctl:radioctl_station_feed_new] (radioctl-station) A feed for a named meter: its unit, coupler offset in decibels and how long a reading stays good.
- [@ANCHOR: radioctl:radioctl_station_feed_with_clock] (radioctl-station) As [`PowerFeed::new`] with the clock the freshness rule reads.
- [@ANCHOR: radioctl:radioctl_station_feed_push] (radioctl-station) Records a sample taken now.
- [@ANCHOR: radioctl:radioctl_station_feed_name] (radioctl-station) The meter's name.
- [@ANCHOR: radioctl:radioctl_station_feed_read] (radioctl-station) See the source.
- [@ANCHOR: radioctl:radioctl_station_meter_run] (radioctl-station) Polls a meter at an interval into its feed; a failed poll leaves the feed to age out.
- [@ANCHOR: radioctl:radioctl_station_vmeter_set] (radioctl-station) Sets what the device reports.
- [@ANCHOR: radioctl:radioctl_station_vmeter_serve] (radioctl-station) Serves a virtual meter of a driver on a stream (duplex or pty) and returns its control.
