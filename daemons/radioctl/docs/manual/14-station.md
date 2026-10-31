# 14. The station: power, switches, rotators, amplifiers

`radioctl-station` is the station-control layer of radioctl: external switches and relays, power
sequencing, interlocks, radios that are switched off, rotators and amplifier controls. It uses
`radioctl-core` for rig handles and the reconnect backoff and follows the same rules: no global
state, no runtime of its own (the caller passes a `Handle`), protocol code sans-IO, nothing switches
at start-up, and every path that could transmit stays behind the closed gate. Its own README
(`crates/radioctl-station/README.md`) lists its anchors. Status of everything here: **Implemented**
and tested against virtual devices and loopback fakes, **unverified** on hardware.

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
(the device stops itself if the host goes silent), `HostSupervised` (the default), `Mediated` (another
system sits between the station and the device). The Home Assistant driver is `Mediated`:

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
or unknown line refuses). Standby is the safe direction and passes an unknown keyed state, like a
power-off. Powering an amplifier off puts it in standby first, and the emergency all-off writes
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
