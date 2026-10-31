# radioctl-station

Station control for radioctl: external switches, power sequencing, interlocks, powered-off radio
records and relay-board drivers. Placement decision (Bruce, 2026-10-30): the station-control layer
is a radioctl module, built as a new crate in this workspace so it can be built and tested apart
from the rig crates. Proposal: `docs/proposals/RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md`.

Phase 1 is virtual: no hardware, nothing is ever switched at start-up, and every transmit-related
path stays behind radioctl's closed gate. Interlock rule: nothing switches while any attached rig is
keyed or reads PTT on; an unknown keyed state counts as keyed; the emergency all-off is never
blocked and needs no confirmation.

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

