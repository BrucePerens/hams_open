# radioctl programmer's manual

radioctl is a thread-safe, async-only Rust library for controlling amateur radio transceivers, with a
server (`radioctld`) that speaks Hamlib's `rigctld` text protocol and a protocol splitter that lets
several programs share one radio. This manual is for a Rust programmer who wants to use the library,
run the daemon, add a radio, or understand how the design keeps a transmitter safe.

Manual status: written 2026-10-30 against `main` of `daemons/radioctl` (Phase 0/1 foundation). The
crate's own overview is [`../../README.md`](../../README.md) and the design decisions are in
[`../DESIGN.md`](../DESIGN.md); this manual explains them and does not replace them.

## How to read the status labels

Every statement about a feature carries one of these meanings, and the chapters say which they mean
where it is not obvious.

| Label | Meaning |
|---|---|
| **Implemented** | The code is on `main` and has tests. |
| **Unverified** | The code exists, but no real radio has ever been used to check it. At the time of writing this applies to every radio description: all 132 facts in the 14 files in `rigs/` are in the `unverified` state, and none is `hardware-verified`. |
| **Planned** | Described in a proposal (`hams_com/docs/proposals/`, not a public document), not in the code. Never rely on it. |

A feature is called implemented only if this manual's author found it in the code. Chapter
[13](13-roadmap.md) lists what is planned, and each chapter marks its planned parts.

## Contents

| Chapter | Read it to |
|---|---|
| [1. Introduction and concepts](01-introduction.md) | learn what radioctl is, why it exists and where it stands |
| [2. Quick start](02-quick-start.md) | build it, run `radioctld` with a virtual radio, drive it from Rust in 30 lines |
| [3. Architecture](03-architecture.md) | understand the layers, the rig actor, the runtime rules and the threads |
| [4. The API](04-api.md) | use `RigHandle`: operations, typed values, errors, events, polling and the cache |
| [5. Transports](05-transports.md) | connect to a radio over serial, USB serial or TCP, and survive unplugging |
| [6. Rig descriptions](06-rig-descriptions.md) | read or write a rig file, and add a radio step by step |
| [7. Protocol engines](07-protocol-engines.md) | see what each protocol family covers and what is not verified |
| [8. The protocol splitter](08-protocol-splitter.md) | present one radio as an IC-7300 or TS-2000 to several programs |
| [9. radioctld](09-radioctld.md) | run the `rigctld`-compatible server, configure it, know where it differs from Hamlib |
| [10. Safety](10-safety.md) | learn the transmit gate, PTT ownership and what radioctl does not guarantee |
| [11. Testing](11-testing.md) | run the test suites, the Hamlib oracle, and follow the anchor convention |
| [12. Provenance and licensing](12-provenance-and-licensing.md) | see where facts about radios came from and what is licensed how |
| [13. Roadmap and open items](13-roadmap.md) | find what is planned and what is known to be missing |
| [Appendix A. Capability matrix](A-capability-matrix.md) | summary of the generated Hamlib 4.6.2 matrix and the radioctl rigs |
| [Appendix B. Glossary](B-glossary.md) | terms and abbreviations |
| [Appendix C. radioctld command reference](C-radioctld-command-reference.md) | every command, response format and error code |

## Which chapters for which goal

- Use radioctl as a library in an application: 1, 2, 3, 4, 5, 10.
- Run `radioctld` and the splitter for existing ham software (WSJT-X, fldigi, loggers): 2, 8, 9, 10, appendix C.
- Add a radio and contribute it: 6, 7, 11, 12.
- Understand the design and the safety model: 1, 3, 10, 12, 13.

## Code in this manual is checked

Every Rust block in these pages is one of three things, and the test `crates/radioctld/tests/manual.rs`
fails when a page disagrees with the code:

- `rust example=NAME`: a verbatim copy of `crates/radioctld/examples/NAME.rs`. Cargo builds every
  example with `cargo test` and the test runs them (except `serial_rig`, which needs a radio).
- `rust signatures`: items copied from the source, with comments and attributes left out. Each must
  still exist.
- `rust pseudo`: pseudo-code, not meant to compile.

TOML blocks marked `rigfile`, `transcript` or `radioctld` are parsed by the production parsers.

The examples live in `radioctld/examples/` because that crate sees all the others; a program of your
own depends on `radioctl-core` (and `radioctl-protocols` for the engines) directly.

```sh
cd daemons/radioctl
cargo run -p radioctld --example quickstart
cargo test -p radioctld --test manual
```

## Editing this manual

Update the page when you change the thing it describes: the test names the page. A new chapter file
must be linked from this index. Never describe a feature as existing unless the code has it; mark
the rest planned. Link to vendor documents, do not copy their text (chapter
[12](12-provenance-and-licensing.md)).
