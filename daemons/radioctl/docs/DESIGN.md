# radioctl design note 0001: foundations

Status: accepted for Phase 0/1 (Bruce, 2026-10-30 decisions; runtime rule added the same day).

## Context

The relay wraps every Hamlib call in one global lock, so one slow radio delays every other radio,
PTT release included. radioctl replaces that with one actor per rig and no shared lock. The proposal
(`RADIOCTL_RUST_RIG_CONTROL_LIBRARY.md`, sections 3.5 and 4) holds the full reasoning; this note
records the decisions that shape the code.

## Decisions

1. **Async only, on tokio, and the library never creates its own runtime by default.**
   Constructors take a `tokio::runtime::Handle` (or use `Handle::current()`) and spawn the rig actor on it. A convenience constructor that builds and owns a small runtime
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

10. **Superseded by 20.** The callback dispatcher thread (a bounded, coalescing queue with
    panic isolation) was removed on 2026-10-31.
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

20. **The event stream is the only status-change API; no callbacks** (Bruce, 2026-10-31).
    `RigOptions::with_callback`, the `Callback` type, `RigOptions::dispatcher_capacity`, the
    `radioctl-callbacks-<rig>` dispatcher thread and `RigStats::dispatcher` are removed, with no
    deprecation period. Why: a dedicated OS thread per rig cost a thread, a mutex-guarded queue and
    a coalescing scheme, and delivered events asynchronously with respect to the application's own
    tasks, so the application had to bridge a thread back into its runtime anyway. A tokio
    application wants to `tokio::spawn` its own consumer. `RigHandle::events()` returns an
    `EventStream` that implements `futures_core::Stream<Item = RigEvent>`, backed by
    `tokio::sync::broadcast` (a hand-written `Stream` over a boxed `recv` future, so no `tokio-stream`
    dependency; `futures-core` was already in the dependency tree). The actor does one non-blocking
    send per event and never waits, so a slow or panicking subscriber cannot delay an unkey or the
    watchdog. A subscriber that falls behind skips events; the count goes to `stream_lagged` and to
    `EventStream::lagged()`. A stream holds the rig only weakly, so it ends (`None`) when the rig
    stops. Per-event semantics (old and new, `Source`, `ActiveTxFrequency`) are unchanged. The
    transmit gate and watchdog are untouched. No per-field filter or `watch` view was added.
21. **A convenience layer over the same API: preludes, by-name constructors, one-call helpers**
    (Bruce, 2026-10-31: "the 30-line example is very verbose"). Before: the quick start was 30 lines
    and a virtual rig took four calls (`rigfile::parse`, `VirtualRig::from_description`,
    `spawn_virtual_rig`, `spawn_current(Engine::new(..))`), repeated in five examples. The lower
    API is unchanged and stays the reference; the layer only composes it. Rules: no new crate;
    no unsafe; no helper opens a transmit gate (the one that touches the gate takes the
    `TransmitPermit` as an argument, so the decision is visible at the call site); every helper
    has a cited test (ADR 0090).
    *Added in `radioctl-core`:*
    - `radioctl_core::prelude`: the usual vocabulary in one `use`.
    - `rigfile::rigs_dir()` and `rigfile::load(id_or_path)`: the three-step rig directory rule
      (`$RADIOCTL_RIGS`, `./rigs`, the checkout) that three examples and a test each copied.
    - `RigHandle::level`, `set_level`, `meter`: typed like `frequency` and `signal`.
    - `RigHandle::start_scope` / `stop_scope`: subscribe first, then `SetScopeOutput(true)`; the
      order is easy to get wrong by hand and loses the first sweep.
    - `EventStream::next_change(field)`: the "loop until this field changes" every events example
      wrote. A pull helper on the stream, not a filtered stream and not a callback; decision 20
      still holds.
    - `RigHandle::transmit_lease(permit, max)` returning a `TransmitLease` with `key`, `unkey`,
      `transmit_for`, `remaining`, `release`. The permit is a required argument; the lease refuses a
      gate that is already open (someone else's decision), a zero length and a missing runtime.
      A timer task on the caller's runtime ends it at `max`; ending (timer, `release`, `Drop`)
      closes the gate and sends an unkey on the emergency path, which needs no await, so `Drop`
      works. It is a bound on the permission, not a second transmit watchdog: `max_transmit` and
      the heartbeat still apply inside it.
    *Added in `radioctl-protocols` (the crate that sees both the core and the engines, so the
    dependency direction is unchanged):*
    - `radioctl_protocols::prelude`: the core prelude plus `engine_for`, `VirtualRig`,
      `PanelChange` and the constructors below.
    - `VirtualRig::spawn("icom-ic-7300")` and `spawn_virtual(runtime, description, options,
      reply_delay)`: the four calls in one, returning `SpawnedVirtualRig { rig, control }` (the
      control is needed for front-panel changes, state and fault injection, so a bare handle
      would not replace the examples). The id is looked up with `rigfile::load`; no rig
      descriptions are compiled into the library.
    - `open_serial_rig(id_or_file, port, baud)` and `open_serial_rig_with(runtime, description,
      SerialConfig, RigOptions)`: description, engine, port, link watch, actor, and the start-up
      unkey radioctld already sends (unkeying needs no gate; a radio that cannot be unkeyed is
      not handed over). It returns `SerialRig { rig, link }`: the port monitor is returned, not
      swallowed, because the link watch and a line keyer depend on it living.
    Follow-up, not done here: `radioctld::rigs::BoxedEngine` and the self-test's boxed engine
    duplicate `impl ProtocolEngine for Box<dyn ProtocolEngine>`, which `radioctl-core` already has.
    *Rejected:*
    - A `RigOptions` builder or `without_polling()`: struct-update syntax (`RigOptions { max_transmit: .., ..Default::default() }`)
      is already one expression and "polling off" is written in three places in the code base.
    - `spawn_subscriber(|event| ..)`: it is the callback decision 20 removed.
    - A helper that opens the gate "for this one call", a boolean `transmit = true` option, or
      keying inside `set_frequency`: the gate is opened only by an explicit `TransmitPermit`.
    - A second transmit watchdog inside the lease, and heartbeats sent for the caller: with a
      `RigOptions::heartbeat` set, the caller still calls `heartbeat()`.
    - A TCP constructor and a `usb:VID:PID` port string: `net::open_link` is two lines and USB
      matching is the volunteer self-test's job; neither repeats in the manual.
    - Compiling the rig descriptions into the library (`include_str!` of `rigs/`): every binary
      would carry 28 files; `rigfile::rigs_dir` finds them and the error names the file.
    - Typed wrappers for `Op` groups that have one caller (RIT, split offsets, filter width): `execute(Op::..)`
      is already one call.
22. **USB devices are a transport: a frame over a byte stream, behind a device-access trait**
    (Bruce, 2026-10-31). The SDR boards and dongles (Si570 kits, FiFi-SDR, FUNcube, Digital World
    Traveller) speak vendor control transfers or HID reports, not a byte stream, but the actor, the
    link-watch/unkey rules and every engine are written against a byte stream. So an engine encodes
    each operation as a *request frame* (`usb_wire::UsbTransfer`: control in/out, bulk, interrupt, and
    interrupt write-then-read for a HID command and reply) and decodes a *reply frame*; `usb_link`
    runs a supervisor thread that owns the open device, performs one transfer at a time and writes
    the reply, with the serial link's rules: loss (`NoDevice`) is `Reconnecting`, writes meanwhile
    are dropped and counted, reopen is by id and strings with backoff, and the actor unkeys the
    device before any other command after a reopen. A timeout or stall is reported to the engine and
    does not take the link down; a lost or timed-out transfer gives the engine no inbound at all, so
    the command ends in the actor's own typed `Timeout`.

    Device access is two traits, `UsbBackend::open(target)` (by vendor id, product id, manufacturer
    and product strings, serial number, interface to claim; called again after every loss, which is the
    hot-plug reconnect) and `UsbDevice` (control in/out, bulk and interrupt read/write). The relay's
    zero-privilege broker implements them over a file descriptor or a proxy it hands over, and no
    engine or link code changes. Implementations: `FakeUsbBackend` (a virtual twin that can be
    unplugged and replugged, with a transfer log; every test uses it and no test touches a USB
    device) and `LibusbBackend`, the direct backend over the `rusb` crate (MIT, with `libusb1-sys`,
    MIT, linking the system libusb-1.0 dynamically) behind the cargo feature `usb`, **off by
    default**, so the default build, its dependency tree and the binary's run-time libraries are
    unchanged. `cargo deny check licenses advisories` against the broker's policy passes for the new
    crates (the only licence failures are older dependencies that policy never covered). `radioctld`
    has the same feature and `kind = "usb"` in `[rig.transport]` (optional `vid`, `pid`, `serial`,
    `transfer_timeout_ms`); a build without the feature refuses it with a clear message, and a
    `virtual` transport on a USB description runs the twin over the fake bus.

    The FUNcube needs no HID library: Hamlib talks to it with libusb interrupt transfers on its
    HID interface (interface 2, endpoints 0x02 and 0x82, 64-byte reports), so `hidapi` is not a
    dependency. Engines: `si570_usb.rs` (frequency, and PTT where the kit lists it, request 0x50,
    unverified, gate closed), `funcube.rs` (frequency, with Hamlib's hertz-then-kilohertz fallback
    spread over two calls), `dwt.rs` (set frequency only). *Not done:* the Elektor SDR-USB (an FTDI
    bit-bang I2C programmer for a Cypress PLL), the pre-0x0F00 DG8SAQ firmware that needs the Si570
    registers solved on the host, the FiFi-SDR's own 0xAB/0xAC requests, and the FUNcube's LNA,
    mixer and IF gain and RSSI. *Rejected:* a second engine interface that returns transfers instead
    of bytes (the actor, the link watch and the transcript machinery would all need a twin), and
    `nusb` or `hidapi` as the direct backend (libusb is what Hamlib uses, so its facts carry over).

23. **A relay board whose status is not readable is run from the driver's own record, with a known
    safe state** (Bruce, 2026-11-01; the SainSmart 16-channel USB module). Assume the module cannot
    report its state and has no watchdog. The board description carries `status_readable` (default
    true; false for the SainSmart in `data/station/boards.toml` and `ModbusBoard::sainsmart_16ch`)
    and `safe_state` (one level per coil, default all off). With `status_readable = false`: function
    1 (read coils) is never sent (refused below every other layer), there is no read-back step, and
    the driver reports the new `Assurance::Recorded` ("we sent the write and the board acknowledged
    it; the state is our own record"), never a confirmation, so the switchboard's confirmed state
    stays unknown and a step that needs a confirmed power state refuses. The safe state goes out as
    ONE function 15 frame over every coil when the link first opens and after every
    `ModbusRelay::reopen`, before any other command; until it is acknowledged every coil is unknown,
    no other frame is sent, and attempts are spaced by a backoff (250 ms doubling to 8 s). The driver
    keeps a shadow of all coils (value, age, or unknown), updated only by an acknowledged function 5
    or 15, made unknown by a timeout, garbled reply or exception; `ModbusRelay::record`,
    `Switchboard::recorded` and `read_line` expose it as recorded, not confirmed. The emergency
    all-off writes the board's safe state (not a hard-coded all off), ignores the backoff, and is
    never blocked. The keyed-state guard is unchanged: an unknown or recorded relay state never
    admits an action the keyed rules block. *Rejected:* trusting the module's function 1 reply, and
    treating a recorded state as confirmed to let an amplifier go to operate.

24. **The engine owns its session set-up; the actor sends it before traffic** (2026-11-01). Two
    default-no-op `ProtocolEngine` hooks return requests, never a transmit-capable one:
    `open_requests` (once, when the actor starts with the link up: Icom PCR `H101` and `G300`) and
    `link_restored` (after every reconnect of a serial, TCP, UDP or USB link: Kenwood, TS-890S,
    Elecraft `AI2;` and Flex `sub` lines only if the controller had turned them on; the PCR family
    repeats its open step; Barrett 4100 remote mode). On a restored link the actor first unkeys
    (fail closed), then writes the hook's requests once each, then raises the link-restored event;
    no queued command is served before them, and a failed write takes the link down again. The
    engine also drops half frames and owed answers of the old link. The gate and the unkey are not
    the hook's business. *Rejected:* each engine sending set-up from `encode` lazily (it would race the
    queue), and the actor knowing protocol details.

## Consequences

- A caller on any runtime can use any rig, at the price that the actor's runtime must be alive.
- Sans-IO engines duplicate a little glue per transport; accepted for testability.

## Open

First rigs, rotators and amplifiers in or out of the first release, and the exact production split
of `radioctld` versus in-process use remain Bruce's calls (proposal section 11).
