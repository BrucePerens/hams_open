# 14. The station: power, switches, rotators, amplifiers

`radioctl-station` is the station-control layer of radioctl: external switches and relays, power
sequencing, interlocks, radios that are switched off, rotators and amplifier controls. It uses
`radioctl-core` for rig handles and the reconnect backoff and follows the same rules: no global
state, no runtime of its own (the caller passes a `Handle`), protocol code sans-IO, nothing switches
at start-up, and every path that could transmit stays behind the closed gate. Its own README
(`crates/radioctl-station/README.md`) lists its anchors. Status of everything here: **Implemented**
and tested against virtual devices and loopback fakes, **unverified** on hardware.

Products that suit this layer, and the SainSmart module that does not: [Recommended hardware](../RECOMMENDED_HARDWARE.md).

## Real rig actors behind the radio manager

`RadioManager` (power button, lifecycle, reconnect with backoff) learns about a radio through the
`RadioLink` trait. `ActorLinks` is the implementation for real actors: the application provides a
`RigSpawner` (open the transport, build the engine, spawn the actor, keep the gate closed) and

```rust signatures
pub fn new(spawner: Arc<dyn RigSpawner>) -> ActorLinks
```

does the rest. A connection attempt spawns the actor; a dead actor (a plain TCP stream at end of
file, a closed serial stream) is respawned on the manager's backoff; a transport that reconnects by
itself (`net::open_link`, the serial link) keeps its actor, and no second actor is spawned beside it.
**Every new actor is sent an unkey before the link is reported up**, because a radio keyed when its
link died may still be keyed and a fresh actor has no reason to look; a failed unkey is a failed
connection. Only reads follow (frequency, mode), reported and never restored.
`ActorLinks::key_source` gives the station's interlock a keyed-state source that follows the radio's
current actor across respawns.

`DescriptionSpawner` is the ready-made `RigSpawner`: for each radio a rig description, an endpoint (a
TCP address with an optional framing, such as the TS-890S LAN login, or a serial port) and the actor
options. It builds the engine, opens a reconnecting transport (`open_link` or `open_serial`) and spawns
the actor with the transport's status. The transmit gate stays closed; credentials never pass through
it.

The minimum off time of a power channel counts from a commanded or observed turn-off only, so a radio
found off at start-up can be switched on at once.

## Assurance levels and the Home Assistant driver

Each channel driver reports how far the station can rely on it (`Assurance`): `HardwareDeadMan`
(the device stops itself if the host goes silent), `HostSupervised` (the default), `Recorded` (the
board acknowledged the write but cannot report its state; the state is the driver's own record),
`Mediated` (another system sits between the station and the device). The Home Assistant driver is
`Mediated`:

- a `ptt` channel on a `Mediated` driver is refused;
- an amplifier powered through one is refused by `Station::new` unless its device declares
  `external_deadman = true`, the owner's statement that a hardware dead-man or a plug-level auto-off
  exists independently;
- state comes from `state_changed` events (a `get_states` snapshot first); while the session is down
  or an entity is `unavailable`, `unknown` or missing, the state is **unknown**, the switchboard keeps
  no confirmed value and every step that needs the channel confirmed refuses. A command is still sent
  while the state is unknown, because an off must always be tried;
- the long-lived access token comes from a `SecretStore` by name, never from configuration; its
  `Secret` wrapper has a redacted `Debug` and the token is never in an error, a log or the audit log;
- the address is a private IP literal (no names, no internet) and the link is plain HTTP and
  WebSocket: no TLS is implemented;
- every service call and session event goes to an `AuditLog`.

### Relay boards whose status is not readable (SainSmart 16-channel)

Decided by Bruce on 2026-11-01: assume the SainSmart module cannot report its state, so set it to a
known safe state when it powers up and keep our own record. Its board description has
`status_readable = false` and a `safe_state` (one level per coil, default all off), both in
`data/station/boards.toml`. For such a board:

- function 1 (read coils) is never sent, and nothing is read back; the channel's assurance is
  `Recorded`, so its confirmed state is always unknown;
- the safe state is written in one function 15 frame when the link first opens and after every
  reconnect (`ModbusRelay::reopen`), before any other command. A failed or unacknowledged write
  leaves every coil unknown, nothing else is sent, and it is retried with a backoff (250 ms to 8 s);
- the driver records each coil only when the board acknowledges a write; a timeout, garbled reply or
  exception makes the coil unknown. `Switchboard::recorded` gives the level and its age, as
  "recorded, not confirmed";
- a step that needs a confirmed power state (an amplifier needing its radio on) refuses on such a
  channel; the keyed-state rules below apply as before, and the emergency all-off writes the
  board's safe state and is never blocked.

## The interlock: what the keyed state blocks

Every switching command passes one guard, which classifies the command by what it does to the RF
path (`station::Action`). The rule was decided by Bruce on 2026-10-31: emergency operations are
never blocked.

| Command | Rig known keyed | Keyed state unknown (link down, never read) | Unkeyed |
|---|---|---|---|
| Power-off of any device (amplifier, rig; the amplifier's standby step) | allowed | allowed | allowed |
| Power-on of a radio (default: its power channel is not a shared supply) | allowed | allowed | allowed |
| Power button on, off, reboot (default) | allowed | allowed | allowed |
| Power-on or reboot of a radio whose power channel has `shared_supply = true` | refused (`Refused`) | refused (`KeyedUnknown`) | allowed |
| Antenna selection, amplifier power-on and operate, band switching, any other output | refused (`Refused`) | refused (`KeyedUnknown`) | allowed |
| Emergency `all_off` | allowed | allowed | allowed |

Power-off and power-on need no confirmation and no password: cutting power can only stop a
transmission, and an off radio cannot be keyed. The **shared-supply protection** is opt-in and off
by default: set it on a radio's power channel when that channel also feeds an amplifier, antenna
switch or other RF-path device, so that switching the radio on cannot hot-switch RF. A reboot of
such a radio is refused up front rather than after cutting power. The flag is accepted only on the
power channel of a radio; anywhere else the definition is refused at load.

```toml fragment
[[channels]]
id = "radio_pwr"
type = "power"
driver = "bank"
line = 0
safe = "off"
shared_supply = true   # this outlet also feeds the amplifier: radio power-on waits for unkeyed
```

The station never touches the transmit gate, which belongs to `radioctl-core`, is closed by default
and is opened only by `open_transmit_gate(TransmitPermit)`: no keyed state and no power command
opens it. `crates/radioctl-station/tests/station_rig.rs` pins the table against a real rig actor (a
keyed virtual IC-7300, and a dead link), `src/station.rs`, `src/manager.rs` and `tests/amplifier.rs`
against a fake keyed source, default and opt-in alike.

## Amplifier and tuner channels

```toml fragment
[[channels]]
id = "amp_operate"
type = "amplifier"
device = "amp"
function = "operate"      # on = operate, off = standby
fault_input = "amp_fault" # an input channel, high = tripped
driver = "bank"
line = 2
safe = "off"
```

Operate is an RF-path change: refused while anything is keyed or the keyed state is unknown, and
refused unless the amplifier is confirmed powered on and its fault line is confirmed clear (a tripped
or unknown line refuses). Standby is the safe direction and passes every keyed state, like a
power-off (it is the first step of one). Powering an amplifier off puts it in standby first, and the emergency all-off writes
standby before anything else. `function = "tune"` is a momentary pulse (`pulse_ms`) that starts the
device's own tune cycle; the station never keys a transmitter for it.

## Rotators

A rotator has a TOML description (`data/station/rotators.toml`: protocol, azimuth and elevation
travel, slew rate, park position, reply timeout, a `verified` flag), a sans-IO protocol engine, and a
`Rotator` handle over any transport.

```rust signatures
pub async fn set_position(&self, az: f32, el: f32) -> Result<(), StationError>

pub async fn position(&self) -> Result<(f32, f32), StationError>

pub async fn stop(&self) -> Result<(), StationError>
```

- Two protocols: `rotctld` (Hamlib's text protocol, which fronts every Hamlib rotator backend) and
  the Yaesu GS-232A/B ASCII family.
- A position outside the description's travel is **refused**, never clamped, and nothing is written.
- Every wait is bounded (the description's reply timeout); commands are serialised; stale bytes are
  drained before and after a failure so a late reply is never taken for the next one.
- With a link status (`NetLink::watch`), a command on a down link fails at once without writing.
  Nothing is restored after a reconnect: the rotator keeps its own target. `stop` is subject to no
  interlock.
- `wait_for` polls until the rotator is within a tolerance of a position, or says where it was last
  seen. `VirtualRotator` (both protocols, a slewing model, stuck and silent faults) and
  `serve_virtual_rotator` give tests a rotator on a loopback socket.

`radioctld` serves a rotator in Hamlib's `rotctld` text protocol from a `[[rotator]]` section
(chapter [9](09-radioctld.md#rotators)), over a reconnecting TCP link or against the virtual rotator;
tested against the virtual rotator and Hamlib's `rotctl -m 2` only, unverified on a real rotator.
Planned: rotator control from the relay (`rotor_steer`), serial transports and further engines
(Easycomm, Rot2Prog, DCU), and rotators in the C ABI.

## The C ABI

`crates/radioctl-capi` builds `libradioctl_capi` (a cdylib and a static library) and the header
`crates/radioctl-capi/include/radioctl.h`, generated by cbindgen (a test fails when the checked-in
header differs from the generated one). It is a sketch of version 1: runtime and rig handles,
frequency, mode, PTT, the link state. Rules:

- **Explicit runtime ownership.** No hidden global runtime. The C caller creates a `rctl_runtime`
  (`rctl_runtime_new`, which exists with the `owned-runtime` feature, on by default), opens rigs on
  it, closes them, then frees it; freeing a runtime that still has rigs is refused (`RCTL_ERR_BUSY`).
  A Rust host that already has a tokio runtime hands it in with `runtime_from_handle` instead. Calls
  block the calling thread and must not be made from inside the runtime's own tasks (that is caught
  and returned as `RCTL_ERR_INTERNAL`).
- **The transmit gate is closed by default.** `rctl_rig_set_ptt(rig, 1)` returns
  `RCTL_ERR_TX_GATE_CLOSED` until the explicit permit call `rctl_rig_permit_transmit(rig, who)`,
  which needs a plain-text name of who decided, and `rctl_rig_revoke_transmit` closes it again.
  Unkeying is always allowed. There is no raw command and no Morse in the C API, so nothing else
  can transmit. `rctl_rig_close` unkeys before it releases the rig.
- **Reconnect is on.** A TCP rig (`rctl_rig_open_tcp`) uses `net::open_link` and a serial rig the
  serial link, so a lost link is reported by `rctl_rig_link_up` and the radio is unkeyed on every
  reconnect without any call from C.
- **No panic crosses the boundary**: each entry point catches panics and returns
  `RCTL_ERR_INTERNAL`. Errors are negative `RCTL_ERR_*` codes plus a per-thread message from
  `rctl_last_error`. Handles carry a magic number, so most wrong or stale handles give
  `RCTL_ERR_BAD_HANDLE`; using a freed handle is still undefined, as in any C API.
- The crate is the one place in the workspace with `unsafe` (see `docs/DESIGN.md` item 7).
  `crates/radioctl-capi/tests/c/capi_test.c` is compiled by the `cc` command in a test with
  `-std=c99 -Wall -Wextra -Werror -pedantic` and run against the shared library.
