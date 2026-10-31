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
    pub callback: Option<Callback>,
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
a callback queue and an event-stream buffer of 64 each, no callback.

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
```

`stats()` returns counters (commands ok and failed, preempted, polls, unsolicited observations,
unrecognised byte runs, events, stream lag, and the dispatcher's delivered, queue-full, coalesced and
panic counts). `link_up()` is false once the transport has failed for good.

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
| Transmit | `GetPtt`, `SetPtt(bool)`, `SendCw(String)` (both can transmit), `GetPower`, `GetSwr` |
| Receive | `GetSignal`, `GetFilterWidth`, `SetFilterWidth(Hz)` |
| Split, RIT, XIT | `GetSplit`, `SetSplit(bool)`, `GetRit`, `SetRit(bool)`, `GetXit`, `SetXit(bool)`, `GetRitOffset`, `SetRitOffset(i16)` |
| Levels | `GetLevel(Level)`, `SetLevel(Level, u16)` for AF gain, RF gain, mic gain, monitor level, keyer speed and RF power setting |
| Reporting | `SetTransceive(bool)` |
| Identity | `GetModelId`, `GetFirmware` |

Each `Op` names the `Capability` it needs (`Op::required_capability`). The set of capabilities comes
from the rig description (appendix [A](A-capability-matrix.md)): `read-frequency`, `set-frequency`,
`second-vfo`, `read-mode`, `set-mode`, `read-ptt`, `set-ptt`, `read-signal`, `read-power`, `send-cw`,
`transceive`, `split`, `rit-xit`, `filter`, `levels`, `read-swr`, `identify`. Asking for something the rig lacks fails
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
not a change):

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
`Transceive` (an unsolicited frame, or a late reply to an abandoned command) or `Splitter` (through
the splitter). `old` is `None` the first time a field is seen. The event of a front-panel change may
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
    let delivered = callback_events.recv_timeout(Duration::from_secs(5))?;
    println!(
        "callback: rig {} reported {} change(s)",
        delivered.rig,
        delivered.changes.len()
    );
    Ok(())
}
```

## Polling

The actor polls so that a knob turned at the radio shows up as an event. Polling is the lowest
priority work: it runs only when no command is waiting.

- **What is polled:** VFO A's frequency, the mode and the PTT state (the fast group) and the S-meter
  and power meter (the slow group), each only if the description has the capability. VFO B, split,
  RIT, XIT, filter width and levels are **not** polled: a front-panel change of those is seen only
  when the radio reports it by itself or a caller reads it.
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
transmit VFO. The proposal (section 14) draws these requirements for radioctl: a first-class
transmit frequency, one value, `active_tx_frequency`, that is correct in split mode and while keyed,
that amplifier and antenna followers use, and whose changes fire the callback; each engine states how
it knows the transmit VFO; and a test that the follower's value equals the radio's actual transmit
frequency in every split configuration.

What exists today:

- **Implemented:** the API is explicit about VFOs (`Op::GetFrequency(Vfo)`), so nothing is guessed
  from PTT state. Split is a boolean `Split` observation, and the model is the one every engine
  implements: VFO A receives, VFO B transmits. `radioctld` follows Hamlib's "ask for the VFO you
  want" rule: `f` with `VFOA` reads A; with `TX` it reads B when the cached split state is on, else A;
  the split-frequency commands (`i`, `\get_split_freq_mode`) read VFO B. Chapter [9](09-radioctld.md) has the details.
- **Not implemented (planned):** `active_tx_frequency` and a typed `tx_frequency`/transmit-VFO read
  and their events; transmit-VFO reads per engine (Kenwood `IF;`, `FR;`/`FT;`; CI-V unselected-VFO
  read; KX3's own reads); the splitter personalities reporting the transmit frequency while keyed in
  split; and the amplifier-follower test. Today a program that needs the transmit frequency must
  read VFO B itself when it knows split is on. The FTDX10 and FTDX101D descriptions have no `split`
  capability because those radios split by band, not VFO, and radioctl's model cannot express it yet.
- **Open, bench check needed:** after enabling split, Hamlib 4.6.2 reads the other frequency of a
  TS-590 with `FA;` where radioctl reads `FB;`. The oracle test records the disagreement
  (`GetFrequency(B)` in `known_kenwood_divergences`); a real TS-590 decides which is right.

## Scope and spectrum

- **Implemented, not reachable through the API:** the Icom CI-V scope (command `0x27`) is decoded in
  `radioctl_protocols::civ_scope` (frame parsing, a sweep reassembler, a scope simulator), the
  description format has an optional `[scope]` block (four Icom files carry one), and `CivEngine`
  collects complete sweeps for `take_scope_lines` and builds scope requests with `scope_request`.
- **Planned:** a scope `Op`, scope events on `RigHandle`, scope settings (span, mode, speed), and
  scopes for non-Icom radios. Nothing in `radioctl-core` knows about a spectrum yet. All scope facts
  are from Hamlib's source and unverified.
