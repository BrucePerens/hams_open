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
   `expect`.
8. **Knowledge sources.** Vendor manuals, our own captures and existing repo code, and (Tier 1)
   program output of `rigctl --dump-caps`. Hamlib source may be read later for facts only, with a
   provenance note per entry and no copied code or tables.
9. **Licence.** AGPL-3.0-or-later (decision 1 of proposal section 11).

## Consequences

- A caller on any runtime can use any rig, at the price that the actor's runtime must be alive.
- Sans-IO engines duplicate a little glue per transport; accepted for testability.

## Open

First rigs, rotators and amplifiers in or out of the first release, and the exact production split
of `radioctld` versus in-process use remain Bruce's calls (proposal section 11).
