# radioctl design note 0001: foundations

Status: accepted for Phase 0/1 (Bruce, 2026-10-30 decisions; runtime rule added the same day).

## Context

The relay wraps every Hamlib call in one global lock, so one slow radio delays every other radio,
PTT release included. radioctl replaces that with one actor per rig and no shared lock. The proposal
(`RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md`, sections 3.5 and 4) holds the full reasoning; this note
records the decisions that shape the code.

## Decisions

1. **Async only, on tokio, and the library never creates its own runtime by default.**
   Constructors take a `tokio::runtime::Handle` (or use `Handle::current()`) and spawn the rig actor
   and the callback dispatcher on it. A convenience constructor that builds and owns a small runtime
   exists only behind a cargo feature (for `radioctld` and the C ABI), never by default.
   There is no `block_on` inside the library. Blocking work (serial on platforms that need it, DSP)
   runs on dedicated threads or `spawn_blocking`, never on runtime workers.
2. **`RigHandle` is `Clone + Send + Sync`** and usable from any thread or runtime: callers only
   await mpsc and oneshot channels. Tests cover two rigs on two runtimes and a handle used from a
   thread with no runtime.
3. **Protocol engines are sans-IO.** An engine is a pure state machine: bytes in, bytes and events
   out, the clock passed in as an argument. A thin tokio layer drives it. Engines are therefore
   testable from transcripts and fuzzable without a runtime. They live in `radioctl-protocols`.
4. **One actor per rig, no global lock.** Commands carry a priority and a deadline. PTT-off and
   emergency commands jump the queue; polling is lowest priority.
5. **Fail closed.** Transmit-capable commands are refused unless an explicit capability gate has been
   opened for that rig. Until real transports exist nothing can reach hardware at all.
6. **Typed values.** Frequencies are integer hertz (`Hz`); units appear in type and field names.
7. **No `unsafe`, no `unwrap`/`expect`/`panic` in library code** (workspace lints). Tests may use
   `expect`. The one exception is `radioctl-capi`, the C ABI, which cannot avoid raw pointers: it
   opts out of the workspace table, denies `unsafe_code` crate-wide, allows it in `ffi.rs` only, and
   keeps the unwrap/expect/panic denials.
8. **Knowledge sources.** Vendor manuals, our own captures and existing repo code, and (Tier 1)
   program output of `rigctl --dump-caps`. Hamlib source may be read later for facts only, with a
   provenance note per entry and no copied code or tables.
9. **Licence.** AGPL-3.0-or-later (decision 1 of proposal section 11).

10. **Callback dispatcher is a dedicated OS thread, not a runtime task.** The proposal said "dispatcher
    task"; application callbacks are synchronous and may block, and the runtime-worker rule forbids
    blocking a worker. The actor pushes into a bounded queue (one short mutex hold) and never waits;
    a full queue merges the new event into the newest queued one (latest value per field wins) and
    counts the intermediate values lost. Panics are caught and counted. The async view is a tokio
    broadcast channel (`EventStream::next`); a lagging subscriber skips events and the loss is counted.
11. **Deadlines live in the actor.** `RigHandle` methods only await a oneshot, so they work from any
    thread or runtime and from a thread with none. The actor answers every command by its deadline.
12. **Preemption.** Unkeying uses a separate channel and a wake-up; it abandons a command that waits
    for a reply (`Cancelled`) but never interrupts a write. The abandoned command's late answer
    reaches the cache as an unsolicited observation. Transmit commands are never retried.
13. **Cache semantics.** A successful set updates the cache at once (including Kenwood sets, which
    have no reply); polling reconciles with the rig.
14. **Transmit gate.** Closed by default, per rig; opened only by `open_transmit_gate(TransmitPermit)`.
    Checked in the handle (no queue slot used) and again in the actor when the command runs.

15. **Serial runs on dedicated threads, USB ports are found by id, and lines are gated.** The
    `serialport` crate (no libudev: Linux ports are listed from sysfs) is driven by a supervisor
    thread and a reader thread per connection, bridged to the actor by an in-memory stream on the
    caller's runtime. Control lines are an immediate call on a second handle of the port, never
    behind a queued read. A vanished device moves the link to reconnecting with exponential backoff
    and the port is looked up again by USB id; bytes written meanwhile are dropped and counted, and
    the command ends in its own typed timeout. The actor follows the status through
    `RigOptions::link_watch`: down (commands refused, `Alarm::LinkDown`) while reconnecting, and
    on the reopen an unkey pass before any other command, then `Alarm::LinkRestored`. Keying through RTS or DTR (`LineKeyer`) is refused
    unless the rig's transmit gate is open; releasing is always allowed. Opening asserts DTR/RTS in
    the kernel for an instant before the library makes them inactive; ports without modem lines say
    so (`LineControl::Unmanaged`) instead of the library pretending.

16. **The protocol splitter is its own crate and goes through the one actor.** `radioctl-splitter`
    presents IC-7300 CI-V or TS-2000 CAT on virtual serial ports (a pty per outside program; TCP
    for others), translating every request into an operation on the rig actor's single queue and
    answering reads from the actor's cache when fresh enough. Personalities are sans-IO. Each client
    has its own echo and auto-information state and is sent unsolicited frames when the rig changes
    (a first reading by a request is not a change). One transmitter, one owner: the first client to
    key owns it, others are refused until it unkeys or goes away (a pty closed by its program counts
    as going away and unkeys the rig); unkeying is open to all; the rig's closed transmit gate still
    applies to every keying request. The TS-2000 personality does not answer `IF` yet (the Kenwood
    engine decodes it from the published guides; the personality has no composite-status encoder) and
    both personalities map data modes to the sideband they sit on.

17. **radioctld is a library plus a thin binary, and the binary owns the one runtime.** The `rigctld`
    text protocol is sans-IO (`command` parses, `reply` renders); `server`, `rigs` and `daemon` take the
    caller's `Handle`. It does not use `owned-runtime`: one multi-thread runtime for all rigs, not one per
    rig. Each rig has its own listener. A client's PTT ownership is the splitter's `PttOwner` and ends
    with its connection (the rig is unkeyed); transmit commands are refused up front while the transmit
    gate is closed or the link is down, and the gate is opened only by `transmit = true` in the
    configuration. `\dump_state` is generated from the rig description and its layout and encodings were
    checked by reading it back through Hamlib's own `rigctl -m 2` (`hamlib-oracle` feature). A bare
    loopback bind and a loopback allow list are the defaults; any wider bind needs an explicit option and
    an explicit allow list.

18. **Transmit watchdog in the actor, independent of the caller** (roadmap baseline 2a.2 to 2a.4).
    From the moment a rig is keyed (by command, or seen keyed by polling or transceive) or sent
    Morse, its actor runs a dead-man timer: past `RigOptions::max_transmit` (default three minutes;
    an owner may raise it or disable it with `Duration::MAX`) or past a lapse of the caller's
    heartbeat (`RigOptions::heartbeat`, off by default, kept alive with `RigHandle::heartbeat()`,
    enforced only while transmitting) the actor unkeys through the fail-closed path, preempting a
    command that is waiting for a reply, and emits an `Alarm` event. Fail-closed unkeys are retried
    three times with a short backoff; a failure is counted and reported as `Alarm::UnkeyFailed`,
    never swallowed; so is an unkey a caller asked for that the rig did not take, whatever the
    caller does with its result. An emergency unkey skips inter-command spacing and rig files
    with a timing block that would delay or defeat it are refused at load. Link loss is reported
    once as `Alarm::LinkDown`. An unkey never gets `QueueFull`: when the emergency queue is full it rides on
    the unkey already queued and receives that unkey's result. This is the library's layer; the
    relay's own time-out and the FX2 hardware time-out (`RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md`
    4.2) remain the outer layers.

19. **The station interlock and the keyed state** (Bruce, 2026-10-31; emergency operations are
    never blocked). `radioctl-station` classifies every switching command by what it does to the RF
    path. Power-off of any device (the amplifier, the rig, the standby step an amplifier power-off
    begins with) and power-on of a radio pass in every keyed state, known keyed and unknown alike,
    with no confirmation and no password: cutting power can only stop a transmission, and an off
    radio cannot be keyed. The one exception is opt-in and off by default: `shared_supply = true`
    on a radio's power channel (it also feeds an amplifier, antenna switch or other RF-path
    device) makes that radio's power-on an RF-path change, refused while any rig is keyed or its
    keyed state is unknown, and its reboot is refused up front. Everything that could hot-switch an
    RF path (antenna selection, amplifier power-on and operate, band switching, any other output)
    is refused while any rig is known keyed or its keyed state is unknown. The emergency all-off
    never looks at the keyed state. None of it touches the transmit gate (decision 14). History:
    until 2026-10-30 an unknown keyed state counted as keyed and blocked everything but the
    all-off; 2026-10-30 let power-off and the power-on of a down radio pass an unknown state
    (confirmed 2026-10-31); 2026-10-31 settled the rest as above. Nothing else relaxes.

## Consequences

- A caller on any runtime can use any rig, at the price that the actor's runtime must be alive.
- Sans-IO engines duplicate a little glue per transport; accepted for testability.

## Open

First rigs, rotators and amplifiers in or out of the first release, and the exact production split
of `radioctld` versus in-process use remain Bruce's calls (proposal section 11).
