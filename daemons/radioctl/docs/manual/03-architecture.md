# 3. Architecture

## Layers

```text
  outside programs        WSJT-X, fldigi, loggers          your Rust application
        |                       |           |                       |
   pty / TCP (CI-V, CAT)   rigctld text protocol            RigHandle (direct)
        |                       |                                   |
 radioctl-splitter          radioctld                               |
        \_______________________|___________________________________/
                                |
                       RigHandle  (Clone + Send + Sync; channels only)
                                |
                  rig actor  (one per radio, one command at a time)
                    |  queue + priorities + deadlines + cache + polling + events
                    |
              ProtocolEngine  (sans-IO: bytes in, bytes and events out)
                    |
              Transport  (any tokio AsyncRead + AsyncWrite)
                    |
        serial / USB-serial / TCP / in-memory virtual rig
```

The proposal lists six layers: transport, protocol engine, rig profile, capability model, public API,
and services. In the code they are:

| Proposal layer | In the code |
|---|---|
| Transport | `radioctl_core::transport::Transport`, `serial`, `usb`, `tcp`, `keyer` (chapter [5](05-transports.md)) |
| Protocol engine | `radioctl_core::engine::ProtocolEngine`; engines in `radioctl-protocols` (chapter [7](07-protocol-engines.md)) |
| Rig profile | `RigDescription` and the TOML files in `rigs/` (chapter [6](06-rig-descriptions.md)) |
| Capability model | `Capability` and `CapabilitySet` |
| Public API | `RigHandle` (chapter [4](04-api.md)) |
| Services | `radioctl-splitter`, `radioctld` (chapters [8](08-protocol-splitter.md), [9](09-radioctld.md)) |

The proposal calls the public API "typed capability traits". The code does not have traits per
capability. It has one closed command enum, `Op`, a `Capability` enum, and a `CapabilitySet` that
`RigHandle::submit` checks every command against: a command whose capability is not in the rig's
description is refused with `Unsupported` before it is queued. A few typed convenience methods
(frequency, mode, PTT, signal) exist on the handle; everything else goes through `execute(Op)`.

## The rig actor

`rig::spawn` builds one actor per radio. The actor owns the transport and the engine, so a rig is
single-threaded internally while any number of tasks and threads hold clones of its handle.

Its loop (`Actor::run`) waits on four sources in a fixed priority order:

1. the **emergency channel**: unkeying the transmitter (`Op::SetPtt(false)`), capacity 16;
2. the **command queue**: ordinary commands in arrival order, capacity `queue_capacity` (default 64);
3. the **transport**: bytes the radio sent on its own (transceive frames, late replies);
4. the **poll timer**: the next due read, only when nothing above is ready.

Each command carries a deadline (the call's timeout, by default 2 seconds) and gets exactly one
answer: a value, or a typed `RigError`. The actor never hangs a caller. Behaviour worth knowing:

- **Spacing and retries.** The description's `inter_command_ms` is kept between writes. A command
  that times out is repeated up to `retries` more times, except a command that transmits, which is
  never repeated. (`SetPtt(false)` does not transmit, so it is repeated.)
- **Preemption.** Unkeying jumps the queue and abandons a command that is waiting for a reply (that
  command ends with `Cancelled`). It never interrupts a write in progress. The abandoned command's
  late answer, if it arrives, reaches the cache as an unsolicited observation.
- **Fail closed on the unknown.** If a command that transmits (`SetPtt(true)`, `SendCw`) was written to
  the radio and never answered, or its write timed out part way, the actor sends an unkey itself before
  it reports the timeout, because the radio may be keyed. That unkey is not preemptible, waits at most
  the engine's reply timeout, and leaves `FAIL CLOSED unkey: <why>` in the transcript. When the last
  `RigHandle` is dropped and the actor stops, it does the same for a radio whose cache says keyed or whose
  transmit gate was open. It is best effort and does nothing if the link is down or the radio cannot be
  unkeyed by command.
- **Saturating deadlines.** A timeout too large for the clock (`Duration::MAX`) becomes the farthest
  representable instant instead of a panic.
- **Derived transmit frequency.** After every observation the actor recomputes `active_tx_frequency` from
  the cached split, VFO and frequency values and adds it to the event when it changed (chapter
  [4](04-api.md)).
- **Cache.** A successful set updates the cache at once, including Kenwood-style sets that have no
  reply; polling reconciles it with the radio later.
- **Link loss.** If the transport read or write fails or ends, the actor marks the link down and
  every later command fails with `LinkDown`. This is final for that actor: there is no reconnect
  inside it. Serial reconnection happens below the actor, in the transport (chapter [5](05-transports.md)),
  and the actor does not see it.

## The runtime model

The application owns the tokio runtime. This is design decision 1 and the proposal's section 13.

- Constructors take a `tokio::runtime::Handle` (`rig::spawn`) or use the current one
  (`rig::spawn_current`) and spawn the actor on it. The runtime needs the time driver.
- The library never builds a runtime of its own by default and contains no `block_on`.
- One exception exists behind the cargo feature `owned-runtime` (off by default):
  `rig::spawn_with_owned_runtime` builds a one-worker runtime and returns an `OwnedRig`, for a
  program that is not a tokio program. It is meant for a future C ABI. `radioctld` does not use it:
  the `radioctld` binary builds one multi-thread runtime for all rigs and passes its `Handle` in. The
  binary calls `block_on` on it; that is the application's right, not the library's.
- A `RigHandle` only awaits `mpsc` and `oneshot` channels, which work across runtimes. So a handle
  can be used from a different runtime than the actor's, or from a thread with no runtime (the
  tests do both; see the end of this chapter).
- Blocking work runs on dedicated threads, never on runtime workers: serial port I/O, pseudo-terminal
  I/O and the application callback.

## Sans-IO engines

A protocol engine is a pure state machine. It is given an `Op` and returns the bytes to write and
what reply to expect; it is given received bytes and the current time as a `Duration` and returns
decoded events. The trait, from `radioctl-core/src/engine.rs`:

```rust signatures
pub trait ProtocolEngine: Send + 'static {
    fn description(&self) -> &RigDescription;

    fn encode(&mut self, op: &Op) -> Result<Request, RigError>;

    fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<Inbound>;
}

pub struct Request {
    pub bytes: Vec<u8>,
    pub reply: ReplyKind,
}

pub enum ReplyKind {
    None,
    Ack,
    Observation(Field),
}

pub enum Inbound {
    Observation(Observation),
    Ack,
    Nak,
    Unrecognised(Vec<u8>),
}
```

Because an engine touches no socket, port, runtime or clock, it can be replayed from a transcript,
driven by property tests with arbitrary bytes, and run against a virtual rig in a plain unit test. The
price is a little glue per transport, which `radioctl-core` provides once.

## The callback dispatcher thread

An application callback is ordinary synchronous code and may block. Running it on the actor, or on a
runtime worker, would let one slow callback delay a PTT release. So `RigOptions::with_callback`
starts a dedicated OS thread per rig (named `radioctl-callbacks-<rig id>`), and the actor hands events
over through a bounded queue under a mutex held only for a push. The actor never waits for the
callback. When the queue is full the new event is merged into the newest queued one (latest value
per field wins) and the lost intermediate values are counted. A panic in the callback is caught and
counted, and the thread keeps running. The async view of the same events is a `tokio` broadcast
channel (`RigHandle::events`); a subscriber that lags skips events and the skip is counted.
Chapter [4](04-api.md) shows both.

## Threading and cancellation

| Thing | Runs on | Stops when |
|---|---|---|
| Rig actor | the caller's runtime | every `RigHandle` clone is dropped (after its fail-closed unkey) |
| Callback dispatcher | its own OS thread | the rig is dropped (it finishes what is queued) |
| Serial supervisor and reader | OS threads, one supervisor per port and one reader per connection | the byte stream handed to the actor is dropped |
| Pty reader and writer (splitter) | two OS threads per endpoint | the `PtyEndpoint` is dropped |
| Splitter clients, `radioctld` listeners and clients | tasks on the caller's runtime | the client disconnects, or the server is shut down |

Cancellation rules, from the code:

- Dropping the future returned by `execute` or a typed read does **not** cancel the command. It has
  already been queued when the future was first polled; the actor still runs it and discards the
  reply. A caller that needs to stop a transmission uses `set_ptt(false)`, not a drop.
- Every command has a deadline enforced inside the actor, not by the caller's future. A command whose
  deadline has passed before it runs ends with `Timeout`.
- The emergency unkey is the only preemption.
- Shutting a rig down means dropping its handles; the actor then unkeys a radio that may be keyed, as
  above. For `radioctld`, `Daemon::stop` closes the listeners and clients and always sends each rig an
  unkey.

## What the tests prove about this chapter

`crates/radioctl-core/tests/actor.rs` contains, among others: a slow rig does not delay another rig's
PTT-off; PTT-off abandons a slow read on the same rig; a blocked callback never delays PTT-off and its
events coalesce; a panicking callback leaves the rig alone; a lagging stream subscriber skips and
counts; two rigs on two runtimes work and a handle works with no runtime at all; the owned-runtime
constructor works without any caller runtime; the description's inter-command gap is kept.
