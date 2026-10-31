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

