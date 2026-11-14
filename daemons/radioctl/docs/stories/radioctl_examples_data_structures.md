# Data structures of the radioctld examples

As a developer changing `crates/radioctld/examples/bench_console.rs`, `splitter_pty.rs` or `virtual_radio_pty.rs`, I want their data structures explained in full (ADR 0113): what each is for, its fields, who builds it, how it is used and what it does with threads and files. Each entry sits under the anchor on the definition in code; the tests in each example's `tests` module (run by `cargo test -p radioctld --examples`) cite the same anchors. None of these examples keys a transmitter: the console has no key command, and the pty example answers from a virtual radio.

## `AnyEngine` in `bench_console.rs` [@ANCHOR: radioctl:radioctl_bench_console_any_engine]

**Purpose.** `spawn_current` wants a sized engine type, but `engine_for(&description)` returns a `Box<dyn ProtocolEngine>`; this one-field wrapper lets the engine of any protocol family go through it. It adds no behaviour.

**Definition.** `struct AnyEngine(Box<dyn ProtocolEngine>);` and `impl ProtocolEngine for AnyEngine`.

| Part | Type | Meaning |
|---|---|---|
| `.0` | `Box<dyn ProtocolEngine>` | The real engine, owned; every trait method forwards to it: `description()` returns its `&RigDescription`, `encode(&Op)` returns its `Result<Request, RigError>` and `feed(&[u8], Duration)` returns its `Vec<Inbound>` (elapsed time since the connection opened). |

**Lifetime and concurrency.** Built in `open_rig` and moved into the rig actor task; the actor is its only user, so no locking. Errors are the wrapped engine's. Test: the example's `tests` module checks that the wrapper gives the same description id, the same encoded bytes and the same `feed` result as a direct engine.

## `Out` [@ANCHOR: radioctl:radioctl_bench_console_out]

**Purpose.** The console's output sink: `line(text)` prints `<seconds since start, width 9, 3 decimals> <text>` on standard output and appends the same line to the optional log file, so a bench session can be read later with its timing.

**Definition.** `#[derive(Clone)] struct Out { started: Instant, log: Arc<Mutex<Option<File>>> }`.

| Field | Type | Meaning |
|---|---|---|
| `started` | `Instant` | The console's start time; the elapsed seconds in each line count from it. |
| `log` | `Arc<Mutex<Option<File>>>` | Optional log file, shared by every clone; `None` means no log. A poisoned lock or a write error is ignored, so logging never stops the console. |

**Concurrency.** Cloned for each place that prints (the clones share one file); the mutex serialises writes to the file, so lines from different tasks do not interleave inside a line. Test: the example's `tests` module.

## `AnyEngine` in `splitter_pty.rs` [@ANCHOR: radioctl:radioctl_splitter_pty_any_engine]

**Purpose.** The same forwarding wrapper as in `bench_console.rs`, here handing the engine of the chosen rig to `spawn_current` for the splitter example, which shares one virtual radio between several pseudo-terminals through `radioctl_splitter`.

**Definition.** `struct AnyEngine(Box<dyn ProtocolEngine>);` with `description`, `encode` and `feed` forwarded to the boxed engine.

**Lifetime and concurrency.** Built once in `main`, moved into the rig actor, used only there. Test: the example's `tests` module, as for the console's wrapper.

## `SharedRig` [@ANCHOR: radioctl:radioctl_virtual_radio_pty_shared_rig]

**Purpose.** The virtual radio shared between the pty thread (which feeds it the bytes read from the master side and writes its replies back) and the main thread (front-panel commands typed on standard input).

**Definition.** `type SharedRig = Arc<Mutex<VirtualRig>>;`.

**Concurrency.** Every user locks for one call (`feed`, `scope_sweep`, `front_panel`) and never holds the lock while writing to the pty; a poisoned lock ends the pty thread. Test: the `Cable` test builds one.

## `SharedLog` [@ANCHOR: radioctl:radioctl_virtual_radio_pty_shared_log]

**Purpose.** The optional wire log of the example: every byte in either direction as a hexadecimal line (`<- ` bytes the program under test sent, `-> ` bytes the virtual radio sent), written by `note`. `None` means no log.

**Definition.** `type SharedLog = Arc<Mutex<Option<File>>>;`.

**Notes.** Write errors are ignored. Shared by the pty thread and the main thread; the mutex keeps lines whole. Test: the `Cable` test reads the file back.

## `Cable` [@ANCHOR: radioctl:radioctl_virtual_radio_pty_cable]

**Purpose.** One pseudo-terminal with the thread that answers whoever opens its slave side: the stand-in for a radio's USB cable. `Cable::plug(rig, link, log)` makes the pty, symlinks `link` to the slave (replacing a file already there), and starts the thread; `unplug()` stops the thread and removes the link (the virtual radio keeps its state, a keyed radio stays keyed, as a real one does); `send(log, bytes)` writes bytes the radio sends by itself (transceive reports) and logs them; dropping a plugged cable unplugs it.

**Definition.** `struct Cable { stop: Arc<AtomicBool>, thread: Option<JoinHandle<()>>, link: PathBuf, writer: Arc<Mutex<TTYPort>> }`.

| Field | Type | Meaning |
|---|---|---|
| `stop` | `Arc<AtomicBool>` | Set (release) by `unplug` to end the thread's loop. |
| `thread` | `Option<JoinHandle<()>>` | The answering thread; `None` once unplugged, which is how `Drop` knows not to unplug twice. |
| `link` | `PathBuf` | The symlink to the slave pty, removed on unplug. |
| `writer` | `Arc<Mutex<TTYPort>>` | A clone of the master side shared with the thread, for the radio's own writes. |

**Thread behaviour.** The thread polls the master with a 20 ms timeout; about every 100 ms it asks the rig for a waveform sweep (a radio with its scope output on sends sweeps unasked) and writes it; bytes read are fed to the rig with the elapsed time and the reply is written back. It only touches the virtual radio and the pty, never a real serial device. `plug` returns an error if the pty or symlink cannot be made. Test: the example's `tests` module plugs, checks the link points into `/dev/pts`, sends, unplugs and reads the wire log.
