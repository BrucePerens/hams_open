# Data structures of the vnwa_open examples

As a developer changing `examples/mod_probe.rs` or `examples/soak.rs`, I want their data structures explained in full (ADR 0113): what each is for, its fields, who builds it, and how it is used. Each entry sits under the anchor on the definition in code; the tests in each example's `tests` module cite the same anchors. Both examples are bench tools for a VNWA unit and an oscilloscope on the lab network; they never key a transmitter and hold no secrets.

## `Scope` in `mod_probe.rs` [@ANCHOR: vnwa_open:mod_probe_scope_struct]

**Purpose.** A line-oriented SCPI connection to the bench oscilloscope for the modulation probe: `open(addr)` connects, `send(cmd)` writes one command and `q(cmd)` writes it and reads one reply line; `capture()` reads CH1 as volts plus the sample interval. It exists only in this example.

**Definition.** `struct Scope { r: BufReader<TcpStream>, w: TcpStream }`.

| Field | Type | Meaning |
|---|---|---|
| `r` | `BufReader<TcpStream>` | Reading half: a clone of the socket, buffered so a reply is read by line. |
| `w` | `TcpStream` | Writing half: commands go out as `<command>\n`. |

**Lifetime and errors.** `open` has a 20 s read timeout, does not check the identity of the instrument (the soak example's `Scope` does), and returns `Err(String)` for a connect or clone failure; `send` and `q` return `Err(String)` with the command text on a write or read failure (a timeout is a read failure). One thread at a time; the scope's SCPI server accepts a single client. The socket closes when the value drops. Test: the example's `tests` module runs it against a loopback server.

## `Scope` in `soak.rs` [@ANCHOR: vnwa_open:soak_scope_struct]

**Purpose.** The scope connection of the soak example: the same two socket halves, but `open(addr)` goes through `vnwa_open::soak_scope::open_checked`, which sends `*IDN?` and, on a connect error, reset or empty reply, waits 2, 5, 10 and 20 seconds and tries again before giving a `NO-GO:` error with a recommended action (power-cycle the scope). It also provides `send`, `q` and `rms()`, the rms of the AC part of one CH1 acquisition in volts, read as a waveform block (`C1:WF? DAT2`).

**Definition.** `struct Scope { r: BufReader<TcpStream>, w: TcpStream }`.

| Field | Type | Meaning |
|---|---|---|
| `r` | `BufReader<TcpStream>` | Reading half from `open_checked`, already past the `*IDN?` reply. |
| `w` | `TcpStream` | Writing half; commands go out as `<command>\n`. |

**Lifetime and errors.** Built once per soak step that needs the scope; every method returns `Err(String)`, never panics on I/O; blocking calls with the socket timeouts of `soak_scope`. Single client. Test: the example's `tests` module (loopback fake scope).

## `Shared` [@ANCHOR: vnwa_open:soak_shared_struct]

**Purpose.** The run state of the soak example that the main loop, the journal and host-monitor threads and the watchdog all read and write: the run directory, the open `events.jsonl`, event counts per class, the kernel journal lines, the epochs of USB-disconnect lines and the intervals of our own power cycles. One instance per process, stored in the `SHARED` `OnceLock` and read through `shared()`, which panics (`run directory not set up`) before setup.

**Definition.** `struct Shared { dir: PathBuf, events: Mutex<File>, counts: Mutex<BTreeMap<String, u32>>, journal: Mutex<VecDeque<(f64, String)>>, disconnects: Mutex<Vec<f64>>, windows: Mutex<Vec<(f64, f64)>> }`.

| Field | Type | Meaning |
|---|---|---|
| `dir` | `PathBuf` | The run directory: `events.jsonl`, `events.md`, `STOP`, `PAUSE`, `lsusb_<tag>.txt` live here. |
| `events` | `Mutex<File>` | `events.jsonl` opened for append; one JSON line per event, written by `event()`. |
| `counts` | `Mutex<BTreeMap<String, u32>>` | Events seen per class, printed in the final summary. |
| `journal` | `Mutex<VecDeque<(f64, String)>>` | Kernel journal lines with their epoch seconds, newest last; `journal_slice` takes the lines near an event (at most 60). |
| `disconnects` | `Mutex<Vec<f64>>` | Epochs of kernel "USB disconnect" lines of the unit. |
| `windows` | `Mutex<Vec<(f64, f64)>>` | (start, end) epochs in which a power cycle of ours was under way, so a disconnect inside one is expected. |

**Concurrency.** Each field has its own `Mutex`; no lock order is documented in the code (unknown whether any code path holds two at once; to-do if one is added). It is set once and then only read through `&'static Arc<Shared>`; it is never serialized except through the event lines. Test: the example's `tests` module sets it up in a temporary directory and records two events.
