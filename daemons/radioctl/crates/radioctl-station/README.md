# radioctl-station

Station control for radioctl: external switches, power sequencing, interlocks, powered-off radio
records and relay-board drivers. Placement decision (Bruce, 2026-10-30): the station-control layer
is a radioctl module, built as a new crate in this workspace so it can be built and tested apart
from the rig crates. Proposal: `docs/proposals/RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md`.

Phase 1 is virtual: no hardware, nothing is ever switched at start-up, and every transmit-related
path stays behind radioctl's closed gate. Interlock rule: nothing switches while any attached rig is
keyed or reads PTT on; the emergency all-off is never blocked and needs no confirmation.

**DECISION FOR BRUCE TO CONFIRM (2026-10-30).** The first rule said an unknown keyed state (link
down, or never read) counted as keyed and blocked everything but `all_off`. That made a radio whose
link dropped impossible to power-cycle. Now, while the keyed state is UNKNOWN: powering OFF any
device is always allowed (it can only stop a transmission); powering ON a radio that is Off or
Unreachable is allowed; everything that could hot-switch an RF path (antenna selection, amplifier
power-on, band switching, any other output) stays blocked. A KNOWN keyed state still blocks
everything except `all_off`, power-off included. (Open question: should power-off also pass while
a rig is known to be keyed? It is not changed here.)

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

## Anchors

- [@ANCHOR: radioctl:radioctl_station_vbank_new] (radioctl-station) A bank whose lines all start off, with read-back.
- [@ANCHOR: radioctl:radioctl_station_vbank_no_readback] (radioctl-station) A bank that cannot be read back.
- [@ANCHOR: radioctl:radioctl_station_vbank_set_fault] (radioctl-station) Injects a fault.
- [@ANCHOR: radioctl:radioctl_station_vbank_line] (radioctl-station) The real (virtual) state of a line.
- [@ANCHOR: radioctl:radioctl_station_vbank_set_physical] (radioctl-station) Sets a line behind the switchboard's back (a sensor changing, a hand-flipped switch).
- [@ANCHOR: radioctl:radioctl_station_vbank_log] (radioctl-station) Every write the bank accepted or ignored, in order.
- [@ANCHOR: radioctl:radioctl_station_vbank_default] (radioctl-station) Same as [`VirtualBank::new`].
- [@ANCHOR: radioctl:radioctl_station_vbank_write_line] (radioctl-station) Applies a write unless a fault says otherwise.
- [@ANCHOR: radioctl:radioctl_station_vbank_read_line] (radioctl-station) Reads a line unless a fault or the lack of read-back says otherwise.
- [@ANCHOR: radioctl:radioctl_station_vbank_can_read_back] (radioctl-station) Whether this bank reads back.
- [@ANCHOR: radioctl:radioctl_station_debouncer_new] (radioctl-station) A debouncer with nothing believed yet.
- [@ANCHOR: radioctl:radioctl_station_debouncer_sample] (radioctl-station) Feeds one raw sample taken at `now` (time since any fixed start). Returns the new stable
- [@ANCHOR: radioctl:radioctl_station_debouncer_stable] (radioctl-station) The believed level, if any.
- [@ANCHOR: radioctl:radioctl_station_board_default] (radioctl-station) An empty switchboard.
- [@ANCHOR: radioctl:radioctl_station_board_new] (radioctl-station) An empty switchboard.
- [@ANCHOR: radioctl:radioctl_station_board_add_channel] (radioctl-station) Adds a validated channel on `driver`. Nothing is written.
- [@ANCHOR: radioctl:radioctl_station_board_subscribe] (radioctl-station) A receiver of every event from now on.
- [@ANCHOR: radioctl:radioctl_station_board_emit] (radioctl-station) Publishes an event (nobody listening is fine).
- [@ANCHOR: radioctl:radioctl_station_board_def] (radioctl-station) The definition of a channel.
- [@ANCHOR: radioctl:radioctl_station_board_ids] (radioctl-station) Every channel name, sorted.
- [@ANCHOR: radioctl:radioctl_station_board_status] (radioctl-station) Commanded and confirmed state of a channel.
- [@ANCHOR: radioctl:radioctl_station_board_store] (radioctl-station) Records commanded and/or confirmed state, noting when a channel went off.
- [@ANCHOR: radioctl:radioctl_station_board_read_value] (radioctl-station) Reads a binary channel, or a selector's lines, as one value. `Ok(None)`: ambiguous
- [@ANCHOR: radioctl:radioctl_station_board_read_back] (radioctl-station) Reads the real state of a channel from the driver and records it as confirmed, leaving the
- [@ANCHOR: radioctl:radioctl_station_board_verify] (radioctl-station) Compares the read-back with `want`, records and reports it.
- [@ANCHOR: radioctl:radioctl_station_board_set] (radioctl-station) Sets a power, output or selector channel and confirms it by read-back. A power-on inside
- [@ANCHOR: radioctl:radioctl_station_board_pulse] (radioctl-station) Pulses a momentary channel: the line goes on for the channel's pulse width, then off.
- [@ANCHOR: radioctl:radioctl_station_board_force_off] (radioctl-station) Drives a power, output or selector channel to off without waiting for any operation in
- [@ANCHOR: radioctl:radioctl_station_board_spawn_input] (radioctl-station) Starts a task on `runtime` that samples an input channel every `interval`, debounces it by
- [@ANCHOR: radioctl:radioctl_station_error_new] (radioctl-station) Builds an error.
- [@ANCHOR: radioctl:radioctl_station_error_display] (radioctl-station) `kind: detail`.
- [@ANCHOR: radioctl:radioctl_station_catalog_from_toml] (radioctl-station) Parses a catalog.
- [@ANCHOR: radioctl:radioctl_station_catalog_builtin] (radioctl-station) The built-in catalog.
- [@ANCHOR: radioctl:radioctl_station_catalog_get] (radioctl-station) A board by name.
- [@ANCHOR: radioctl:radioctl_station_map_from_toml] (radioctl-station) Parses a map and validates it against `catalog`.
- [@ANCHOR: radioctl:radioctl_station_map_validate] (radioctl-station) No pin has two functions or appears twice, no pin the board does not expose, no power,
- [@ANCHOR: radioctl:radioctl_station_map_safe_levels] (radioctl-station) The pins with a defined safe level, for the firmware or driver to apply on release.
- [@ANCHOR: radioctl:radioctl_station_http_is_private] (radioctl-station) Whether an address is loopback, private or link-local (IPv4 or IPv6, including IPv4-mapped).
- [@ANCHOR: radioctl:radioctl_station_http_parse_address] (radioctl-station) The address the owner typed: an IP literal, optionally `:port` (default 80), private only.
- [@ANCHOR: radioctl:radioctl_station_http_new] (radioctl-station) A driver for `relays` relays (1 to 32) at `address`. Nothing is sent.
- [@ANCHOR: radioctl:radioctl_station_http_get] (radioctl-station) One HTTP/1.0 GET; the body of a 200 reply.
- [@ANCHOR: radioctl:radioctl_station_http_check] (radioctl-station) Refuses a relay number past the bank.
- [@ANCHOR: radioctl:radioctl_station_http_write_line] (radioctl-station) Switches one relay.
- [@ANCHOR: radioctl:radioctl_station_http_read_line] (radioctl-station) Reads one relay's state from the device.
- [@ANCHOR: radioctl:radioctl_station_http_can_read_back] (radioctl-station) Both families report their relay state.
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
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_transact] (radioctl-station) Sends one request PDU and returns the validated reply.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_write_all] (radioctl-station) Function 15 over every coil: all on or all off in one frame.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_write_line] (radioctl-station) Function 5 on the line's coil.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_read_line] (radioctl-station) Function 1 on the line's coil.
- [@ANCHOR: radioctl:radioctl_station_modbus_relay_can_read_back] (radioctl-station) Whether the board description reads coils back.
- [@ANCHOR: radioctl:radioctl_station_channel_lines] (radioctl-station) The driver lines this channel uses.
- [@ANCHOR: radioctl:radioctl_station_channel_validate] (radioctl-station) Checks the channel on its own: a power or ptt channel needs a defined safe state, a safe
- [@ANCHOR: radioctl:radioctl_station_def_from_toml] (radioctl-station) Parses and validates a station definition.
- [@ANCHOR: radioctl:radioctl_station_def_channel] (radioctl-station) A channel by name.
- [@ANCHOR: radioctl:radioctl_station_def_device] (radioctl-station) A device by name.
- [@ANCHOR: radioctl:radioctl_station_def_validate] (radioctl-station) Checks names, references, channel kinds, one device per power channel, and that the
- [@ANCHOR: radioctl:radioctl_station_def_power_on_order] (radioctl-station) Every device in an order where each follows the devices it depends on (ties by name);
- [@ANCHOR: radioctl:radioctl_station_def_dependents] (radioctl-station) Every device that depends on `id`, directly or not, in power-off order (the most
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
- [@ANCHOR: radioctl:radioctl_station_station_check_power_on] (radioctl-station) The dependency refusals: dependencies confirmed on, required antennas selected.
- [@ANCHOR: radioctl:radioctl_station_station_power_on] (radioctl-station) Powers a device on: refused while anything is keyed, while a device it depends on is not
- [@ANCHOR: radioctl:radioctl_station_station_power_kind] (radioctl-station) The channel kind that powers a device (an output kind for none).
- [@ANCHOR: radioctl:radioctl_station_station_wait_ready] (radioctl-station) Waits for a device to answer, up to its ready timeout.
- [@ANCHOR: radioctl:radioctl_station_station_power_off] (radioctl-station) Orderly power-off: refused while anything is keyed. Devices that depend on this one go
- [@ANCHOR: radioctl:radioctl_station_station_run_profile] (radioctl-station) Runs a profile's steps in order within its timeout, with a `Step` event at each. A device
- [@ANCHOR: radioctl:radioctl_station_station_stop_profile] (radioctl-station) Powers off the devices a profile powers on, in reverse order, with the off delays and the
- [@ANCHOR: radioctl:radioctl_station_station_all_off] (radioctl-station) The emergency all-off. Never blocked: no keyed-state check, no minimum times, no delays,
- [@ANCHOR: radioctl:radioctl_station_times_default] (radioctl-station) 20 s boot, 10 s grace, 10 s switch confirmation, 60 s degraded.
- [@ANCHOR: radioctl:radioctl_station_lifecycle_new] (radioctl-station) A machine in `state` since `now`.
- [@ANCHOR: radioctl:radioctl_station_lifecycle_state] (radioctl-station) The current state.
- [@ANCHOR: radioctl:radioctl_station_lifecycle_since] (radioctl-station) When the current state began.
- [@ANCHOR: radioctl:radioctl_station_lifecycle_goto] (radioctl-station) Moves to `to` unless already there.
- [@ANCHOR: radioctl:radioctl_station_lifecycle_power_on_commanded] (radioctl-station) The power-on was commanded (the power button or a profile). A radio that is already
- [@ANCHOR: radioctl:radioctl_station_lifecycle_power_off_commanded] (radioctl-station) The power-off was commanded and the switch is going off: the radio is no longer expected
- [@ANCHOR: radioctl:radioctl_station_lifecycle_step] (radioctl-station) Feeds one snapshot at `now` and returns the transition it caused, if any. At most one
- [@ANCHOR: radioctl:radioctl_station_record_new] (radioctl-station) A bare record: unknown state, Unreachable, no history.
- [@ANCHOR: radioctl:radioctl_station_record_validate] (radioctl-station) Rejects a record that cannot be trusted: empty ids, absurd boot time, non-finite levels,
- [@ANCHOR: radioctl:radioctl_station_record_cached] (radioctl-station) The last-known state with its age at `now_unix` (a clock that went backwards gives age 0).
- [@ANCHOR: radioctl:radioctl_station_store_new] (radioctl-station) A store over `path`. Nothing is read or written.
- [@ANCHOR: radioctl:radioctl_station_store_path] (radioctl-station) The file.
- [@ANCHOR: radioctl:radioctl_station_store_load] (radioctl-station) Reads the file. A missing file is an empty store. A file that is too big, not JSON, of
- [@ANCHOR: radioctl:radioctl_station_store_tighten] (radioctl-station) Removes group and other permissions from an existing record file.
- [@ANCHOR: radioctl:radioctl_station_store_set_aside] (radioctl-station) Renames a bad file to `<name>.bad-<n>` (first free n) so it is kept but not read again.
- [@ANCHOR: radioctl:radioctl_station_store_save] (radioctl-station) Writes every record atomically with mode 0600: a temporary file in the same directory,
- [@ANCHOR: radioctl:radioctl_station_station_guard_action] (radioctl-station) The interlock with the action's class: a keyed source always refuses; an unknown keyed
- [@ANCHOR: radioctl:radioctl_station_station_set_hint] (radioctl-station) The lifecycle state of a radio, as the lifecycle manager sees it, so powering it on can
- [@ANCHOR: radioctl:radioctl_station_station_classify_on] (radioctl-station) The class of powering `d` on: a radio that is Off or Unreachable (by the lifecycle hint, or
- [@ANCHOR: radioctl:radioctl_station_station_classify_channel] (radioctl-station) The class of setting a channel by hand: off on a power channel is [`Action::PowerOff`]; on
- [@ANCHOR: radioctl:radioctl_station_station_classify_pulse] (radioctl-station) The class of pulsing a momentary line: a device's off line is [`Action::PowerOff`], its on
- [@ANCHOR: radioctl:radioctl_station_lifecycle_assume_powered] (radioctl-station) A momentary power-on line was pulsed, which has no read-back: the radio is taken to be
- [@ANCHOR: radioctl:radioctl_station_riglinks_new] (radioctl-station) No rigs yet.
- [@ANCHOR: radioctl:radioctl_station_riglinks_set] (radioctl-station) Attaches (or replaces, after a reconnect) the handle of a radio.
- [@ANCHOR: radioctl:radioctl_station_riglinks_link_up] (radioctl-station) The handle's `link_up`; a radio with no handle is down.
- [@ANCHOR: radioctl:radioctl_station_riglinks_present] (radioctl-station) No hotplug or ping evidence from a bare handle.
- [@ANCHOR: radioctl:radioctl_station_riglinks_connect] (radioctl-station) Succeeds only if the link is up; the caller respawns the actor.
- [@ANCHOR: radioctl:radioctl_station_vlink_new] (radioctl-station) A link over the bank whose lines power the radios.
- [@ANCHOR: radioctl:radioctl_station_vlink_add] (radioctl-station) A radio on bank line `line` that boots after `boot` (`None`: never).
- [@ANCHOR: radioctl:radioctl_station_vlink_set_boot] (radioctl-station) Changes how long the radio takes to boot (`None`: never).
- [@ANCHOR: radioctl:radioctl_station_vlink_set_report] (radioctl-station) The state the radio reports when it is connected.
- [@ANCHOR: radioctl:radioctl_station_vlink_drop_link] (radioctl-station) Fault: the link drops and cannot be re-established until the radio is power-cycled or
- [@ANCHOR: radioctl:radioctl_station_vlink_heal] (radioctl-station) Clears a dropped link.
- [@ANCHOR: radioctl:radioctl_station_vlink_connects] (radioctl-station) How many connect attempts were made.
- [@ANCHOR: radioctl:radioctl_station_vlink_refresh] (radioctl-station) Follows the power line: off resets everything, the first moment on starts the boot clock.
- [@ANCHOR: radioctl:radioctl_station_vlink_link_up] (radioctl-station) Up when booted, connected and not dropped.
- [@ANCHOR: radioctl:radioctl_station_vlink_present] (radioctl-station) Enumerated once booted (a dropped link still is).
- [@ANCHOR: radioctl:radioctl_station_vlink_connect] (radioctl-station) Connects if booted and not dropped.
- [@ANCHOR: radioctl:radioctl_station_manager_lock] (radioctl-station) Locks a mutex, ignoring poisoning (the data are plain values).
- [@ANCHOR: radioctl:radioctl_station_manager_new] (radioctl-station) Loads the record file (a bad one is set aside, see [`RecordStore::load`]) and builds an
- [@ANCHOR: radioctl:radioctl_station_manager_problems] (radioctl-station) What the record loader and later saves had to report.
- [@ANCHOR: radioctl:radioctl_station_manager_ids] (radioctl-station) The radios, sorted.
- [@ANCHOR: radioctl:radioctl_station_manager_subscribe] (radioctl-station) A receiver of every [`RadioEvent`] from now on (the station-level event stream).
- [@ANCHOR: radioctl:radioctl_station_manager_spawn_callback] (radioctl-station) Calls `callback` for every event, from a task on `runtime`, in the style of radioctl's
- [@ANCHOR: radioctl:radioctl_station_manager_state] (radioctl-station) The lifecycle state of a radio.
- [@ANCHOR: radioctl:radioctl_station_manager_record] (radioctl-station) A copy of the radio's record.
- [@ANCHOR: radioctl:radioctl_station_manager_cached] (radioctl-station) The last-known state with its age, served whether the radio is on or off.
- [@ANCHOR: radioctl:radioctl_station_manager_note_state] (radioctl-station) Records a state read from the live radio (and its capabilities, when given) and saves.
- [@ANCHOR: radioctl:radioctl_station_manager_persist] (radioctl-station) Saves every record. A failure is also kept in [`RadioManager::problems`].
- [@ANCHOR: radioctl:radioctl_station_manager_now] (radioctl-station) Time since the manager was built.
- [@ANCHOR: radioctl:radioctl_station_manager_power_evidence] (radioctl-station) The switch channel's confirmed state as evidence.
- [@ANCHOR: radioctl:radioctl_station_manager_gather] (radioctl-station) One snapshot of evidence.
- [@ANCHOR: radioctl:radioctl_station_manager_apply] (radioctl-station) Applies a transition: updates the hint, the record's health and lifecycle, saves, and
- [@ANCHOR: radioctl:radioctl_station_manager_poll] (radioctl-station) Gathers evidence and steps the radio's state machine once.
- [@ANCHOR: radioctl:radioctl_station_manager_connect_step] (radioctl-station) One reconnect attempt, if one is due: the radio is powered (or its power is unknown), not
- [@ANCHOR: radioctl:radioctl_station_manager_tick] (radioctl-station) A reconnect attempt if due, then one step of the state machine. A state the radio reports
- [@ANCHOR: radioctl:radioctl_station_manager_spawn_monitor] (radioctl-station) Ticks every radio every `interval` on `runtime`. Abort the handle to stop it.
- [@ANCHOR: radioctl:radioctl_station_manager_power_source] (radioctl-station) The device's power source, refusing a radio that has no power button.
- [@ANCHOR: radioctl:radioctl_station_manager_press_on] (radioctl-station) The on half of the button: power on through the station (interlocks and dependencies), then
- [@ANCHOR: radioctl:radioctl_station_manager_step_state] (radioctl-station) Runs a command-driven lifecycle transition and publishes it.
- [@ANCHOR: radioctl:radioctl_station_manager_press] (radioctl-station) The power button of a radio with a power source: on, off or reboot, behind the interlocks
- [@ANCHOR: radioctl:radioctl_station_manager_reconcile] (radioctl-station) The start-up reconcile. Reads back the real state of each radio's power channel (a read,
- [@ANCHOR: radioctl:radioctl_station_manager_life_times] (radioctl-station) The timing for one radio: the station's, with the record's boot time if it has one.
- [@ANCHOR: radioctl:radioctl_station_manager_unknown] (radioctl-station) The error for a radio the station does not have.

- [@ANCHOR: radioctl:radioctl_station_actorlink_lock] (radioctl-station) `lock` (see the source).
- [@ANCHOR: radioctl:radioctl_station_actorlink_transport_up] (radioctl-station) Whether the transport status, when there is one, says the link is usable.
- [@ANCHOR: radioctl:radioctl_station_actorlink_new] (radioctl-station) Links that spawn rigs through `spawner`; the unkey pass waits at most three seconds.
- [@ANCHOR: radioctl:radioctl_station_actorlink_with_unkey_timeout] (radioctl-station) Changes how long the unkey pass may take before the connection counts as failed.
- [@ANCHOR: radioctl:radioctl_station_actorlink_handle] (radioctl-station) The current actor of a radio, for the application's own use (queries, the transmit gate).
- [@ANCHOR: radioctl:radioctl_station_actorlink_key_source] (radioctl-station) A keyed-state source for the station's interlock that always follows the radio's current actor, so a respawn does not leave the interlock looking at a dead handle.
- [@ANCHOR: radioctl:radioctl_station_actorlink_spawns] (radioctl-station) How many actors were spawned for a radio.
- [@ANCHOR: radioctl:radioctl_station_actorlink_journal] (radioctl-station) What the links did, oldest first: spawns, unkey passes, refusals to double-spawn.
- [@ANCHOR: radioctl:radioctl_station_actorlink_note] (radioctl-station) `note` (see the source).
- [@ANCHOR: radioctl:radioctl_station_actorlink_state] (radioctl-station) What the current slot of a radio says: up, retrying on its own, dead, or absent.
- [@ANCHOR: radioctl:radioctl_station_actorlink_unkey_pass] (radioctl-station) The unkey pass on a new actor, then a read of what the radio is set to.
- [@ANCHOR: radioctl:radioctl_station_livekey_name] (radioctl-station) The radio's id.
- [@ANCHOR: radioctl:radioctl_station_livekey_key_state] (radioctl-station) The current actor's keyed state (see the `RigHandle` implementation); unknown with no actor, or while the transport is retrying.
- [@ANCHOR: radioctl:radioctl_station_actorlink_link_up] (radioctl-station) Up when the actor's link is up and its transport, if it has a status, is connected.
- [@ANCHOR: radioctl:radioctl_station_actorlink_present] (radioctl-station) No hotplug or ping evidence from a bare actor.
- [@ANCHOR: radioctl:radioctl_station_actorlink_connect] (radioctl-station) One connection attempt: nothing to do if the link is up; an error (and so the manager's backoff) while the transport is retrying on its own; otherwise a new actor, the unkey pass, and the state the radio reports.
