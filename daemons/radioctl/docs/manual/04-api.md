# 4. The API

All of this chapter is **implemented** unless marked **planned**, and all of it is **unverified**
against real radios. The reference is the rustdoc of `radioctl-core`; this chapter explains how the
parts fit. The signatures below are copied from the source and checked by the manual test.

## Getting a handle

```rust signatures
pub fn spawn<E: ProtocolEngine, T: Transport>(
    runtime: &Handle,
    engine: E,
    transport: T,
    options: RigOptions,
) -> Result<RigHandle, RigError>

pub fn spawn_current<E: ProtocolEngine, T: Transport>(
    engine: E,
    transport: T,
    options: RigOptions,
) -> Result<RigHandle, RigError>
```

`engine` is one of the engines of `radioctl-protocols` built from a rig description. `transport` is
any tokio `AsyncRead + AsyncWrite + Send + Unpin + 'static`: a serial link's stream, a `TcpStream`, a
virtual rig's in-memory stream. `radioctl_protocols::conformance::engine_for` returns the right
engine for a description as a `Box<dyn ProtocolEngine>`; `Box<dyn ProtocolEngine>` itself does not
implement the trait, so `radioctld` wraps it in a small adapter (`rigs::BoxedEngine`). A program that
knows its radio simply constructs `CivEngine::new`, `KenwoodEngine::new`, `ElecraftEngine::new` or
`YaesuEngine::new` with the description.

Options:

```rust signatures
pub struct RigOptions {
    pub queue_capacity: usize,
    pub default_timeout: Duration,
    pub poll: PollConfig,
    pub dispatcher_capacity: usize,
    pub stream_capacity: usize,
    pub scope_capacity: usize,
    pub callback: Option<Callback>,
    pub link_watch: Option<watch::Receiver<LinkStatus>>,
    pub max_transmit: Duration,
    pub heartbeat: Option<Duration>,
}

pub struct PollConfig {
    pub enabled: bool,
    pub fast_min: Duration,
    pub fast_max: Duration,
    pub slow_min: Duration,
    pub slow_max: Duration,
}
```

Defaults: queue of 64, 2 second deadline, polling on (fast fields 250 ms to 2 s, meters 1 s to 5 s),
a callback queue and an event-stream buffer of 64 each, a scope-frame buffer of 16, a 180 second transmit limit (`DEFAULT_MAX_TRANSMIT`), no heartbeat, no callback, no link watch (`link_watch`: a serial link's `watch()`, chapter [5](05-transports.md), so the actor follows the port across a reconnect and unkeys when it is back).

## Rig handles

`RigHandle` is `Clone + Send + Sync`. Methods that do not talk to the radio:

```rust signatures
pub fn id(&self) -> &str

pub fn capabilities(&self) -> &CapabilitySet

pub fn transmit_permit(&self) -> Option<TransmitPermit>

pub fn stats(&self) -> RigStats

pub fn link_up(&self) -> bool

pub fn events(&self) -> EventStream

pub fn cached(&self, field: Field) -> Option<Cached<Observation>>

pub fn heartbeat(&self)
```

`stats()` returns counters (commands ok and failed, preempted, polls, unsolicited observations,
unrecognised byte runs, events, stream lag, watchdog unkeys, failed unkeys, coalesced unkeys, scope frames
and scope lag, and the dispatcher's delivered, queue-full, coalesced and panic counts). `heartbeat()` is the
caller's "I am alive" for the transmit watchdog (chapter [10](10-safety.md)). `link_up()` is false once the transport has failed for good.

## Operations

There are two ways to run a command. The general one takes an `Op`:

```rust signatures
pub async fn execute(&self, op: Op) -> Reply

pub async fn execute_within(&self, op: Op, timeout: Duration) -> Reply

pub async fn execute_from(&self, op: Op, timeout: Duration, source: Source) -> Reply
```

`Reply` is `Result<Option<Observation>, RigError>`: a read returns `Some(observation)`, a set returns
`None`. `execute_from` records a `Source` on the events the command causes; the splitter uses it to
mark its traffic as `Source::Splitter`. The typed convenience methods cover the common reads and
sets:

```rust signatures
pub async fn frequency(&self, vfo: Vfo, freshness: Freshness) -> Result<Cached<Hz>, RigError>

pub async fn set_frequency(&self, vfo: Vfo, hz: Hz) -> Result<(), RigError>

pub async fn mode(&self, freshness: Freshness) -> Result<Cached<Mode>, RigError>

pub async fn set_mode(&self, mode: Mode) -> Result<(), RigError>

pub async fn ptt(&self, freshness: Freshness) -> Result<Cached<bool>, RigError>

pub async fn set_ptt(&self, on: bool) -> Result<(), RigError>

pub async fn signal(&self, freshness: Freshness) -> Result<Cached<u16>, RigError>
```

Everything else is an `Op`:

| Group | Ops |
|---|---|
| Frequency, mode | `GetFrequency(Vfo)`, `SetFrequency(Vfo, Hz)`, `GetMode`, `SetMode(Mode)` |
| Transmit | `GetPtt`, `SetPtt(bool)`, `SendCw(String)` (both can transmit), `StopCw` (stops Morse in progress; an emergency command like `SetPtt(false)`, chapter [10](10-safety.md)), `GetPower`, `GetSwr` |
| Receive | `GetSignal`, `GetFilterWidth`, `SetFilterWidth(Hz)` |
| Split, RIT, XIT | `GetSplit`, `SetSplit(bool)`, `GetTxVfo`, `GetRit`, `SetRit(bool)`, `GetXit`, `SetXit(bool)`, `GetRitOffset`, `SetRitOffset(i16)` |
| Levels | `GetLevel(Level)`, `SetLevel(Level, u16)` for AF gain, RF gain, mic gain, monitor level, keyer speed, RF power setting, and the receive attenuator (`Level::Attenuator`, decibels, 0 is off) and preamplifier (`Level::Preamp`, a step number, 0 is off). Which attenuator values and preamplifier steps a model has is its description's `[levels]` block (chapter [6](06-rig-descriptions.md)); an engine refuses any other value with `Unsupported`. |
| Reporting | `SetTransceive(bool)` |
| Identity | `GetModelId`, `GetFirmware` |
| Scope | `SetScopeOutput`, `GetScopeOutput`, `SetScopeAxis`, `GetScopeAxis`, `SetScopeSpan`, `GetScopeSpan`, `SetScopeEdges`, `SetScopeReference`, `GetScopeReference` (see the end of this chapter) |

Each `Op` names the `Capability` it needs (`Op::required_capability`). The set of capabilities comes
from the rig description (appendix [A](A-capability-matrix.md)): `read-frequency`, `set-frequency`,
`second-vfo`, `read-mode`, `set-mode`, `read-ptt`, `set-ptt`, `read-signal`, `read-power`, `send-cw`,
`transceive`, `split`, `rit-xit`, `filter`, `levels`, `read-swr`, `identify`, `scope`. Asking for something the rig lacks fails
at once with `Unsupported` and uses no queue slot. A capability in the description does not
guarantee that the engine implements the operation for that family; the engine then returns
`Unsupported` too. For example, the CI-V engine refuses `SetTransceive` (it is a menu item on those
radios, chapter [7](07-protocol-engines.md)).

**Planned:** the proposal's per-capability traits, a typed method for each operation, a raw-command
escape hatch behind a separate capability (radioctld never serves one, chapter [9](09-radioctld.md)),
memories, functions such as noise blanker, tuner and amplifier control, and power on and off.

## Typed values and units

- Frequencies are integer hertz in `Hz(pub u64)`. `Hz::from_khz` refuses overflow; `as_mhz` is for
  display only. There is no floating point on any wire path.
- `Mode` has eleven values: `Lsb`, `Usb`, `Am`, `Cw`, `CwR`, `Rtty`, `RttyR`, `Fm`, `Wfm`, `Data`,
  `DataR`. Wire codes live in the rig description, not in the enum.
- `Vfo` is `A` or `B`. Only the VFOs a description lists are usable (`second-vfo` for B).
- Other quantities are plain integers whose unit is in the `Op`/`Observation` documentation, not in
  a type: the S-meter and power meter are raw units (`u16`); SWR is tenths of a unit (10 is 1.0:1);
  RIT/XIT offset is hertz (`i16`); firmware is `(major, minor)`; levels are in the rig's own units
  (`radioctld` rescales them, chapter [9](09-radioctld.md)). Calibrated dB and watts are **planned**:
  calibration tables are per-model measurements that the descriptions do not yet hold.

## Freshness and the state cache

Each field the actor sees is cached with the instant it was seen. A typed read takes a `Freshness`:

```rust signatures
pub enum Freshness {
    Cached,
    MaxAge(Duration),
    Fresh,
}

pub struct Cached<T> {
    pub value: T,
    pub age: Duration,
}
```

`Cached` returns any cached value, however old, and asks the radio only if nothing is cached.
`MaxAge(d)` returns the cache if it is no older than `d`. `Fresh` always asks. The `age` tells the
caller how stale the answer is; a fresh read has age zero. The cache is updated by every successful
read, by every successful set (before the radio is asked again), by polling and by unsolicited frames.
`RigHandle::cached` reads it without touching the radio.

## Errors

```rust signatures
pub struct RigError {
    pub kind: ErrorKind,
    pub retry: RetryHint,
    pub detail: String,
    pub transcript_tail: Vec<String>,
}

pub enum RetryHint {
    Never,
    Now,
    After(Duration),
}
```

| `ErrorKind` | Meaning | Default retry hint |
|---|---|---|
| `Timeout` | no answer before the deadline. For a transmit command the actor has already sent an unkey (chapter [3](03-architecture.md)). | `Now` |
| `Rejected` | the radio refused (CI-V NG, Kenwood `?;`) | `Never` |
| `Unsupported` | the rig, its description or its engine lacks the operation | `Never` |
| `LinkDown` | the transport is down or the actor stopped | `After(1 s)` |
| `Cancelled` | abandoned for an emergency unkey, or shut down | `Now` |
| `TransmitGateClosed` | a transmit-capable command with the gate closed | `Never` |
| `Malformed` | the radio sent something the engine could not use | `Never` |
| `QueueFull` | the command queue is full; nothing was waited for | `Now` |

`transcript_tail` holds the last 16 wire lines before the failure, oldest first, as direction-prefixed
hex: `TX 46 41 3b`, `RX ...`, `?? ...` for bytes that formed no frame, `LINK DOWN <reason>`, and `FAIL CLOSED unkey: <reason>` when the actor unkeyed after an unanswered transmit command. Errors
raised before a command reaches the actor (unsupported, gate closed in the handle, queue full) have an
empty tail. `RigError` implements `std::error::Error`, so `?` works in any `Box<dyn Error>` context.

## Status changes: callback and stream

Every change of a cached value produces a `RigEvent` (a read that returns the same value as before is
not a change). The event also carries the change of the derived `Field::ActiveTxFrequency` when the
value changed:

```rust signatures
pub struct RigEvent {
    pub rig: String,
    pub source: Source,
    pub changes: Vec<Change>,
}

pub struct Change {
    pub field: Field,
    pub old: Option<Observation>,
    pub new: Observation,
}
```

`source` says where it came from: `Api` (a command through the handle), `Poll` (found by polling),
`Transceive` (an unsolicited frame, or a late reply to an abandoned command), `Splitter` (through
the splitter) or `Watchdog` (the actor's own transmit watchdog). `old` is `None` the first time a field is seen. The event of a front-panel change may
arrive as `Poll` or `Transceive` depending on which saw it first.

Two views of the same events, shown together by `crates/radioctld/examples/events.rs`:

- **Callback.** `RigOptions::default().with_callback(|event| ...)`. Synchronous, on a dedicated thread,
  bounded and coalescing, panic-isolated (chapter [3](03-architecture.md)).
- **Stream.** `rig.events()` returns an `EventStream`; `stream.next().await` yields `Option<RigEvent>`
  and returns `None` when the rig stops. A subscriber that falls behind skips events, counted in
  `stream_lagged`.

```rust example=events
//! Status changes: the callback (its own thread) and the async event stream, fed by a knob
//! turned on a virtual radio.
use std::error::Error;
use std::sync::mpsc;
use std::time::Duration;

use radioctl_core::op::{Field, Observation, Op};
use radioctl_core::rig::{spawn_current, RigOptions};
use radioctl_core::rigfile;
use radioctl_core::units::{Hz, Vfo};
use radioctl_protocols::elecraft::ElecraftEngine;
use radioctl_protocols::io::spawn_virtual_rig;
use radioctl_protocols::virtual_rig::{PanelChange, VirtualRig};
use tokio::runtime::Handle;
use tokio::time::timeout;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let description = rigfile::parse(include_str!("../../../rigs/elecraft-kx3.toml"))?.description;
    let radio = VirtualRig::from_description(&description)?;
    let (transport, front_panel) = spawn_virtual_rig(&Handle::current(), radio, Duration::ZERO);

    // The callback is plain synchronous code. It runs on a dedicated thread, never on the
    // actor, so a slow callback cannot delay an unkey.
    let (seen, callback_events) = mpsc::channel();
    let options = RigOptions::default().with_callback(move |event| {
        let _ = seen.send(event);
    });
    let rig = spawn_current(ElecraftEngine::new(description)?, transport, options)?;
    let mut stream = rig.events();

    // Ask the radio to report changes by itself, then turn a knob on it.
    rig.execute(Op::SetTransceive(true)).await?;
    front_panel.front_panel(PanelChange::Frequency(Hz(7_100_000)));

    let wanted = Field::Frequency(Vfo::A);
    loop {
        let event = timeout(Duration::from_secs(5), stream.next())
            .await?
            .ok_or("the rig stopped")?;
        if let Some(change) = event.changes.iter().find(|c| c.field == wanted) {
            if change.new == Observation::Frequency(Vfo::A, Hz(7_100_000)) {
                println!(
                    "stream: VFO A changed to 7100000 Hz (source {:?})",
                    event.source
                );
                break;
            }
        }
    }
    // The callback hears the same changes, in order, on its own thread.
    loop {
        let delivered = callback_events.recv_timeout(Duration::from_secs(5))?;
        if delivered.changes.iter().any(|c| c.field == wanted) {
            println!(
                "callback: rig {} reported the change of VFO A",
                delivered.rig
            );
            break;
        }
    }
    Ok(())
}
```

### Alarms

The actor reports safety conditions of its own as events whose change is `Field::Alarm` with an
`Observation::Alarm(Alarm)`: `TransmitTimeout` (the radio was keyed longer than `max_transmit`; the actor
unkeyed it), `HeartbeatLapsed` (the caller's heartbeat stopped while the radio transmitted; the actor
unkeyed it) and `UnkeyFailed` (an unkey failed after every attempt: **the radio may still be transmitting**).
An alarm is not a value read from the radio, is never cached and is never deduplicated: every occurrence is
an event. The transcript also records `ALARM <kind>`. An application should treat `UnkeyFailed` as an
emergency of its own.

## Polling

The actor polls so that a knob turned at the radio shows up as an event. Polling is the lowest
priority work: it runs only when no command is waiting.

- **What is polled:** VFO A's frequency, VFO B's frequency (rigs with `second-vfo`), the split state
  (rigs with `split`), the mode and the PTT state (the fast group) and the S-meter and power meter (the
  slow group), each only if the description has the capability. RIT, XIT, the RIT offset, filter width
  and levels are **not** polled: a front-panel change of those is seen only when the radio reports it by
  itself or a caller reads it. VFO B and split are polled so that `active_tx_frequency` follows a change
  made at the radio.
- **Adaptive intervals:** a change resets a field to its fastest interval; no change doubles it up to
  the maximum; a failed poll jumps to the maximum.
- **While the rig transmits** only the PTT state and the power meter are polled.
- **Transceive:** once the radio has reported something unsolicited, every field polls at its
  maximum interval. Turning transceive on is the caller's job (`Op::SetTransceive(true)`), and for
  CI-V radios it must be switched on in the radio's menu.
- `PollConfig { enabled: false, .. }` turns polling off, as most tests do.

## Transmit frequency and split (Hamlib issue 138)

Hamlib's issue 138 (TS-590, "split frequency reporting via CAT not working",
<https://github.com/Hamlib/Hamlib/issues/138>) is a lesson in what "the frequency" means. An
amplifier controller needed the frequency the radio would transmit on; Hamlib answered with the
frequency of whichever VFO was asked for, as its maintainers intended, and other programs assumed the
transmit VFO. The proposal (section 14) therefore asks radioctl for a first-class transmit frequency:
one value that is correct in split mode and while keyed, that amplifier and antenna followers use, and
whose changes fire the callback, including a change made on the front panel.

**Implemented** (unverified on real radios):

- The API is explicit about VFOs (`Op::GetFrequency(Vfo)`), so nothing is guessed from PTT state, and it
  offers the questions a follower asks:

```rust signatures
pub async fn split(&self, freshness: Freshness) -> Result<Cached<bool>, RigError>

pub async fn tx_vfo(&self, freshness: Freshness) -> Result<Vfo, RigError>

pub async fn rx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError>

pub async fn tx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError>

pub async fn active_tx_frequency(&self, freshness: Freshness) -> Result<Hz, RigError>
```

- **The derivation** is a pure function in `radioctl_core::txstate` (`TxState`). The receive VFO is the
  one the rig reports, else VFO A. The transmit VFO is the receive VFO when split is off; in split it is
  the one the rig reports, else the other VFO. The transmit frequency is that VFO's frequency.
  `active_tx_frequency` is the transmit frequency: "now, or when keyed". A RIT or XIT offset is
  deliberately **not** folded in (a few kilohertz at most; a band decoder wants the VFO frequency).
- **The actor publishes it.** After every observation the actor recomputes the value from the cache and,
  if it changed, adds a `Change` for `Field::ActiveTxFrequency` (an `Observation::ActiveTxFrequency`) to
  the event, so the callback and the stream report it whichever field moved it. It is derived: no engine
  reports it and no command reads it. `Observation::RxVfo` and `TxVfo` carry the rig's own statement
  where the protocol gives one.
- **Each engine says how it knows.** Kenwood reads the receive VFO (`FR`/`IF` field) and the transmit
  VFO (`FT`; split is `FR0;FT1;`, and from `IF` the other VFO when the split flag is set); Elecraft
  `FT` (`FT1` transmits on B); Yaesu newcat `FT` (the FT-991A, B in split); CI-V uses the
  unselected VFO (`Vfo::B`) as the transmit VFO in split.
- **The follower test exists on virtual rigs.** `radioctl-protocols/tests/tx_frequency.rs` drives the
  Kenwood, Elecraft, Yaesu newcat and CI-V virtual radios into split, keys them on the virtual front panel
  (the gate stays closed; nothing reaches hardware), and checks that `active_tx_frequency` equals the
  transmit VFO's frequency and that the callback reports it. It has not been run on a real radio.
- **The splitter** answers a plain read of the current frequency (CI-V `03`, TS-2000 `FA;`) with the
  transmit frequency while the rig is keyed in split, as OmniRig-style clients assume. Reads that name
  the other VFO (`25 01`, `FB;`) are unchanged. `SplitterConfig::report_tx_frequency_while_keyed`
  (default true) turns it off for a client that wants the receive frequency while keyed.
- **`radioctld`** keeps Hamlib's meaning: `f` returns the frequency of the VFO asked for (`VFOA`, `VFOB`,
  `currVFO`, `RX`, `TX`), and the new `\get_active_tx_freq` returns the transmit frequency whichever VFO
  is current. The tokens `RX` and `TX` resolve through the same derivation.

**Not done**

- Yaesu legacy does not report a transmit VFO yet (a `TODO` note in `yaesu_legacy.rs`), so
  `active_tx_frequency` on an FT-817, FT-857D or FT-897D follows the generic rule only.
- The FTDX10 and FTDX101D split by band (MAIN/SUB), not by VFO, so their descriptions have no `split`
  capability and radioctl cannot express their split yet.
- FlexRadio has no second VFO or split in the model.
- RIT and XIT offsets are excluded from the value by design.
- **Open, bench check needed:** after enabling split, Hamlib 4.6.2 reads the other frequency of a
  TS-590 with `FA;` where radioctl reads `FB;`. The oracle test records the disagreement
  (`GetFrequency(B)` in `known_kenwood_divergences`); a real TS-590 decides which is right. The same
  goes for every derived value: all of it follows the manuals and the virtual radios.

## Scope and spectrum

A rig with a spectrum scope (band scope) has the capability `scope`. At the time of writing the IC-7300
and IC-7610 descriptions list it; the IC-705 and IC-9700 descriptions carry a `[scope]` block but not
yet the capability. **Implemented, unverified** (CI-V command `0x27`, facts from Hamlib's source; no scope
has been used):

- **Settings** are ordinary operations whose reads land in the cache: `SetScopeOutput(bool)` and
  `GetScopeOutput` (whether the radio sends waveform sweeps on its own), `SetScopeAxis(receiver, axis)`
  and `GetScopeAxis` (`ScopeAxis::Center`, `Fixed`, `ScrollCenter`, `ScrollFixed`),
  `SetScopeSpan(receiver, Hz)` and `GetScopeSpan` (a span the model's menu does not offer is
  `Unsupported`), `SetScopeEdges(range, edge, low, high)` for fixed mode, and `SetScopeReference` and
  `GetScopeReference` (hundredths of a dB). Receiver 0 is Main, 1 is Sub.
- **Sweeps** are not cache fields and not status events. The engine returns each complete sweep as a
  `ScopeFrame` and the actor publishes it on a stream of its own:

```rust signatures
pub fn scope_frames(&self) -> ScopeStream

pub async fn next(&mut self) -> Option<ScopeFrame>
```

  A `ScopeFrame` carries a sequence number, the receiver, the axis mode, the lowest and highest
  frequency on the axis, an out-of-range flag, the largest sample value, and one amplitude byte per
  pixel, lowest frequency first, in the rig's own units. The stream is bounded (`RigOptions::scope_capacity`,
  16 by default); a subscriber that lags skips frames and the loss is counted (`scope_lagged` in the
  stats, with `scope_frames` published). Sweeps flow once `SetScopeOutput(true)` has run. No dB
  calibration is guessed.
- The virtual IC-7300 and IC-7610 honour fixed edges and send sweeps; a test
  (`radioctl-protocols/tests/scope.rs`) checks the round trip and the stream.
- **Planned:** scope for non-Icom radios, scope speed and hold settings through `Op`, the descriptions of
  the IC-705 and IC-9700 listing the capability, and the relay's own scope-source frame built from a
  `ScopeFrame` (the relay does that, not this crate).
