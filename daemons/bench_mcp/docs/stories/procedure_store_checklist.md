# The Run Directory Keeps Everything a Procedure Run Produces, and the Contributor Checklist Keeps New Instruments Honest

As the person (or AI agent) who starts a bench procedure and polls it later, I want everything the run produced to sit in one directory on disk, readable after the run ended and after a restart of the server, with files that are never half written and logs that cannot fill the disk; and as a reviewer of a pull request that adds an instrument to `bench_mcp`, I want a set of tests that fail until the instrument ships its manual pages, its dry run, its safe-state tool and clean fixtures, so I can trust the structure without owning the instrument. This story documents two files in `daemons/bench_mcp`: `src/procedure/store.rs` (the managed directory of the procedure runner) and `src/checklist.rs` (the contributor checklist, compiled only for `cargo test`).

Nothing in either file keys a transmitter, opens a serial port or talks to an instrument. `store.rs` only reads and writes files under the managed directory; the tier cap, call budget, instrument reservation, band check and transmit gate all apply in the tool path that `run.rs` drives, not here. `checklist.rs` contains tests and helpers only; it is not part of the shipped binary (`mod checklist` is `#[cfg(test)]` in `main.rs`).

The layout of a run directory, `<root>/runs/<run id>/`, as `store.rs` and its callers make it: `state.json` (rewritten atomically at every change by `Run::persist` in `run.rs`), `events.jsonl` (the event log, appended), `stdout.txt` and `stderr.txt` (size-capped), `emitted.json`, `result.json`, `touched.json` (the safe-state actions the run made necessary), `source.py`, `args.json`, `tools.json`, `config.json`, `checkpoints/` and `scratch/` (the program's working directory). `store.rs` itself creates only the run directory, `scratch/` and `events.jsonl`; the rest is written by `run.rs`, `safe.rs` and the Python runtime through `write_json` and `append_stream`. Beside `runs/`, the same root holds `saved/` (saved procedures, `saved.rs`) and `check/` (a scratch directory for syntax checks).

Two constants belong to the entries below and have no anchor of their own. `EVENTS_CAP` is `8 << 20` (8 MiB, 8,388,608 bytes): the size of `events.jsonl` past which events are counted and dropped. `STREAM_CAP` is `1 << 20` (1 MiB, 1,048,576 bytes): the size of `stdout.txt` or `stderr.txt` past which output is dropped. Both are `pub const` of type `u64` and are not configurable.

## The managed directory: time, root, files

### `proc_store_now` [@ANCHOR: proc_store_now]

Returns the current wall-clock time as seconds since the Unix epoch, with fractions. It is the one clock the procedure store uses: run start and end times (`started_unix`, `ended_unix`), the time of an event relative to the start of its run, the run id (milliseconds), the retention cutoff in `purge`, and the time a saved procedure version was written. Called from `store.rs` itself and from `run.rs`, `saved.rs` and `procedure/mod.rs`.

| Argument | Type | Meaning |
|---|---|---|
| (none) | | |

Returns `f64`, seconds since 1970-01-01 UTC (about 1.8e9 today; sub-millisecond resolution is available but callers use milliseconds). Special value: `0.0` when the system clock reads before the epoch (the `duration_since` error is swallowed). It cannot panic, blocks nothing, takes no lock and has no side effects. It is not monotonic: a clock step changes it, so differences across a step are meaningless (the run's wall-clock budget uses monotonic time in the Python runtime, not this).

See also: `proc_store_purge` (uses it for the cutoff), `proc_store_new_run_dir` (uses it for the id), `proc_store_event_append`. Test: `src/procedure/tests.rs` cites it above the test that runs a procedure to the end and reads everything back.

### `proc_store_root` [@ANCHOR: proc_store_root]

Returns the managed directory under which all procedure files live: the `dir` entry of the procedure section of the config when one is set, otherwise `procedures` under the bench output directory (`crate::out_dir()`, which is `BENCH_MCP_OUT` or `bench_mcp` under the system temp directory, and is created when missing). Every other function here, and `saved.rs`, and `run.rs` (the per-root active-run count and the instrument lock release) start from this path.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` (borrowed) | The procedure configuration (`procedure::Config`). Only `cfg.dir: Option<PathBuf>` is read. |

Returns an owned `PathBuf`. It does not create the `procedures` directory itself and does not check that `dir` exists, is writable or is absolute; a relative `dir` is relative to the server's working directory. It cannot fail or panic. Calling it creates the bench output directory as a side effect when `dir` is unset (through `out_dir`). Pure otherwise and safe to call from any thread.

See also: `Config` in `src/procedure/mod.rs`, `proc_store_new_run_dir`, `proc_store_list_runs`, `proc_store_purge`.

### `proc_store_write_json` [@ANCHOR: proc_store_write_json]

Writes a JSON value to a file so that a concurrent reader sees either the whole old file or the whole new file, never half of one. This is how `state.json`, `emitted.json`, `result.json`, `touched.json`, the checkpoints and the saved-procedure `meta.json` are written, so a poller (`bench_proc_status`, `bench_proc_result`) can read at any moment, including while the run is changing state.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&Path` (borrowed) | The final file. Its directory must exist. |
| `v` | `&Value` (borrowed) | Any `serde_json::Value`; it is written pretty-printed. |

Returns `Result<(), String>`. `Ok(())` after the rename. `Err` carries the path and the operating system error when the temporary file cannot be written (missing directory, full disk, no permission) or the rename fails; a serialization error is returned as its own message. On an error the final file is unchanged; a temporary file may be left behind.

Method and caveats: the text is written to `path` with its extension replaced by `tmp` (so `state.json` goes through `state.tmp`, and `meta.json` through `meta.tmp`) and then renamed over `path`. The rename is atomic on one filesystem. There is no fsync, so a power loss can still leave an empty or old file. The temporary name depends only on the file stem, so two threads writing the same path at the same moment share one temporary file; callers (`persist` in `run.rs`) are written so that one run writes its own `state.json`, and the function takes no lock itself. Blocking file I/O; do not call it with a lock held that a poller needs, though the callers in `run.rs` hold only a short record lock while building the value and release it before writing.

See also: `proc_store_read_json`; callers in `run.rs` (`proc_run_persist`), `safe.rs`, `saved.rs`. Test: the run tests in `src/procedure/tests.rs`.

### `proc_store_read_json` [@ANCHOR: proc_store_read_json]

Reads a JSON file into a `serde_json::Value`. The matching reader of `write_json`; used for `state.json` (status, list, purge), `emitted.json`, `result.json`, `touched.json` and the saved-procedure `meta.json`.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&Path` (borrowed) | The file to read. |

Returns `Result<Value, String>`. `Err` is `"<path>: <os or parse error>"` when the file does not exist, cannot be read, is not valid UTF-8 or is not valid JSON. There is no distinction between "missing" and "corrupt" in the type; callers that treat a missing file as normal (for example `bench_proc_output` reading `emitted.json`) map the error to an empty object themselves. Blocking file I/O, no locks, no side effects, safe from any thread. Because `write_json` renames into place, a reader never sees a half-written file from this module's writers.

See also: `proc_store_write_json`, `proc_store_purge`, `proc_store_list_runs`.

## Creating and finding runs

### `proc_store_new_run_dir` [@ANCHOR: proc_store_new_run_dir]

Creates the directory of a new run and gives it an id. The id is unique and sorts by start time, so a plain sort of directory names is a time order. Called once per run by `run::start` right after `purge`.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` (borrowed) | Gives the root through `proc_store_root`. |

Returns `Result<(String, PathBuf), String>`: the run id and the directory `<root>/runs/<id>`, with an empty `scratch/` directory inside it. The id has the exact shape `r` + 11 lowercase hex digits of the Unix time in milliseconds + 3 hex digits of a process-wide counter modulo 4096, 15 characters in all (for example `r0193a7c1b2d001`). `proc_store_run_dir` accepts only this shape.

Errors (all `Err(String)`): the `runs` directory cannot be created, the run directory cannot be created for a reason other than "already exists", or `scratch/` cannot be created (the run directory is then left behind without a `scratch/`). An "already exists" clash is not an error: the loop draws the next counter value and tries again, so it terminates unless the filesystem keeps reporting the clash. Side effects: creates `<root>/runs/` (and the root) when missing, then the run directory and `scratch/`. Thread-safe: the counter is an atomic and directory creation is the arbiter. The ids of two server processes sharing one root can clash only inside one millisecond with the same counter value, and the loop then picks another counter value.

See also: `proc_store_run_dir`, `proc_store_event_open`, `run::start` (`proc_run_start`).

### `proc_store_run_dir` [@ANCHOR: proc_store_run_dir]

Turns a run id received from a caller (an AI agent or user through `view.rs`: status, output and result; whether other run tools use it is unknown from this file) into the run's directory, refusing anything that is not exactly a run id. It is the guard that keeps an id such as `../../etc` or `r/../x` from leaving the `runs` directory.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` (borrowed) | Gives the root. |
| `id` | `&str` (borrowed) | The id as typed by the caller. |

Returns `Result<PathBuf, String>`: `<root>/runs/<id>` when the directory exists. Errors: `"<id> is not a run id (see bench_proc_list)"` unless the id is exactly 15 bytes, starts with `r` and the remaining 14 are ASCII hex digits (upper case hex is accepted; `new_run_dir` makes lower case, so an upper-case id passes the check and then fails the directory test on a case-sensitive filesystem); `"no run <id> (see bench_proc_list; finished runs are deleted after the retention time)"` when the shape is right but there is no such directory. The error text is written for an AI agent and names the tool to call next. Read-only (one `is_dir` check), no locks, no side effects.

See also: `proc_store_new_run_dir`, `proc_store_purge`, `proc_view_status`, `proc_view_output`, `proc_view_result`.

## The event log

### `proc_store_event_open` [@ANCHOR: proc_store_event_open]

Opens (creating it if needed) the `events.jsonl` file of a run and returns the `EventLog` that appends to it. Called once by `run::start`; the log is then shared by the whole run behind a mutex (`log: Mutex<EventLog>` in the run context).

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` (borrowed) | The run directory (from `new_run_dir`). |
| `started` | `f64`, Unix seconds | The run's start time; every event's `t` is its time minus this. |

Returns `Result<EventLog, String>`; `Err` is `"<path>: <os error>"` when the file cannot be opened for appending. The file is opened in append mode and is not truncated, so a reopen of an existing file adds to it, but the in-memory counters start at zero: `seq` would then restart at 1 and `bytes` at 0 (this is not done in practice; each run opens its log once).

#### The `EventLog` structure

`EventLog` is a private-field struct in `store.rs`; it is created only here and held by the run. Fields: `file` (`File`, the open append handle), `seq` (`u64`, number of the last event stored, starting at 0; numbers start at 1 and are the cursor of `bench_proc_output`), `bytes` (`u64`, bytes written so far, compared with `EVENTS_CAP`), `started` (`f64`, Unix seconds, the origin of event times), `dropped` (`u64`, number of events discarded because the file was full). Invariants: `seq` only grows; once `bytes >= EVENTS_CAP` nothing more is written and `dropped` grows instead. Serialized form: one JSON object per line, `{"seq": n, "t": seconds-since-start rounded to milliseconds, "kind": "...", ...the event's fields}`; an event whose data is not a JSON object is stored under the key `data`. Concurrency: methods take `&mut self`, so the run wraps it in a `Mutex`; poisoned locks are recovered by the callers. Related: `proc_store_event_append`, `proc_store_event_count`, `proc_store_event_dropped`, `proc_store_read_events`.

### `proc_store_event_append` [@ANCHOR: proc_store_event_append]

Appends one event to the run's event log. Events are how a run reports what happened (state changes, tool calls, steps, log lines, emitted values, operator questions); a poller reads them by cursor with `bench_proc_output`.

| Argument | Type | Meaning |
|---|---|---|
| `&mut self` | `EventLog` | The log; the call changes `seq`, `bytes` and `dropped`. |
| `kind` | `&str` (borrowed) | The event type, for example `state`, `call`, `log`, `step`; free text, stored as `kind`. |
| `data` | `serde_json::Value` (owned) | The event's fields. An object is merged into the line (a key named `seq`, `t` or `kind` in it overrides the standard one); any other value is stored as `data`. |

Returns `u64`: the event's number (1, 2, 3, ...), or `0` when the file is already at `EVENTS_CAP` and the event was dropped (`dropped` is incremented). Special case: when this line would carry the file past the cap, the line is replaced by one `{"seq": n, "kind": "truncated", "msg": "the event list is full; later events are counted, not stored"}` (no `t`), and that call still returns `n`, a non-zero number, although the original event was not stored; every later call returns 0.

Errors: none returned. A failed write to the file is ignored (`let _ =`), but `seq` and `bytes` still advance, so a disk-full condition silently loses events and leaves gaps in the stored sequence that a reader cannot tell from a dropped event. No panic path. Blocking `write_all` of one line (a single `write` of the whole line, newline included, so lines from one handle do not interleave); no fsync. No locks of its own: the caller holds the log mutex. Time stamps use `proc_store_now`.

See also: `proc_run_event` in `run.rs` (the only writer), `proc_store_event_open`, `proc_store_read_events`. Test: `src/procedure/tests.rs` cites `proc_store_event_dropped` above a tool-level test; whether any test floods the event file to 8 MiB is unknown.

### `proc_store_event_count` [@ANCHOR: proc_store_event_count]

Returns the number of the last stored event, which is also the number of events stored (they are numbered from 1 without gaps, except for the ignored-write case described under `proc_store_event_append`). `Run::persist` writes it into `state.json` as `events`, so a status poll shows how far the log has grown and a poller can compare it with its cursor.

Arguments: `&self` only. Returns `u64`; `0` for a log nothing was appended to. Cannot fail, takes no lock (the caller holds the log mutex), no side effects. The truncation marker counts as an event; dropped events do not.

See also: `proc_store_event_dropped`, `proc_run_persist`, `proc_view_status`.

### `proc_store_event_dropped` [@ANCHOR: proc_store_event_dropped]

Returns how many events were discarded because the event file had reached `EVENTS_CAP`. `Run::persist` writes it into `state.json` as `events_dropped` so that a reader knows the log is incomplete (a non-zero value means: the stored events end early; the run itself continued normally).

Arguments: `&self` only. Returns `u64`, zero while the file has room. Cannot fail, no lock of its own, no side effects. The count does not include the event replaced by the truncation marker.

See also: `proc_store_event_append`, `proc_store_event_count`. Test: cited in `src/procedure/tests.rs` above a tool-level test; whether any test fills the event file is unknown.

### `proc_store_read_events` [@ANCHOR: proc_store_read_events]

Reads the events after a cursor from a run's `events.jsonl`, as many as fit in a size budget. It backs `bench_proc_output`: the caller passes back the cursor it got last time and receives only what is new, so repeated polling is cheap for the caller and complete.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` (borrowed) | The run directory (from `run_dir`). |
| `cursor` | `u64` | The `seq` of the last event the caller has; 0 for "from the start". Events with `seq <= cursor` are skipped. |
| `max_bytes` | `usize` | The budget for the JSON text of the events returned, summed over their stored line lengths (newlines not counted). At least one event is always returned when any is new, even if it alone is larger. |

Returns `Result<(Vec<Value>, u64, bool), String>`: the events in order, the `seq` of the last one returned (the new cursor; equal to the old cursor when nothing is new), and `more`, true when events were left out because of the budget. Special values: a missing `events.jsonl` is not an error (it returns no events, cursor unchanged, `more` false); lines that do not parse as JSON (for example a last line being written at that instant) are skipped; a line without a `seq` counts as `seq` 0 and is skipped. The only `Err` is another read error, as the text of the OS error without the path. Side effects: none, it only reads. It reads and parses the whole file on every call (up to 8 MiB), so its cost grows with the log, not with the number of new events; it takes no lock and runs while the writer appends.

See also: `proc_store_event_append`, `proc_view_output`. Example: a first call `read_events(dir, 0, 20000)` returns events 1 to 40 and `more = true`; the next call with cursor 40 returns from event 41.

## Output streams

### `proc_store_append_stream` [@ANCHOR: proc_store_append_stream]

Appends text from the program's standard output or standard error to `stdout.txt` or `stderr.txt`, keeping the file under a size cap. A runaway program that prints in a loop therefore cannot fill the disk. Called from `run.rs` for each `stdout` and `stderr` message the Python runtime sends.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&Path` (borrowed) | The stream file; created if missing. |
| `text` | `&str` (borrowed) | The text to add. |

Returns `u64`: the number of bytes of `text` that were dropped (0 when everything was stored). `run.rs` sums it into the run's dropped-output record. Behavior: when the file is already `STREAM_CAP` bytes or more, all of `text` is dropped and nothing is written. Otherwise as much as fits in the remaining room is written, cut back to a UTF-8 character boundary so a multi-byte character is never split, and when part of the text did not fit the line `[output limit reached; the rest was dropped]` (with a newline before and after) is appended. Hence a stream file can end up to about 45 bytes over `STREAM_CAP`, which the test asserts with its slack of 200 bytes. The marker is written once: later calls see the file at or over the cap and write nothing.

Errors: none returned; an open or write failure is ignored and the function still reports the bytes it meant to drop (zero when the text would have fitted), so a full disk loses output silently. Blocking file I/O; it reads the file size with `metadata` on every call. No lock: one writer per stream file is assumed (the run's pump).

See also: `proc_store_read_stream`, `proc_run_handle`.

### `proc_store_read_stream` [@ANCHOR: proc_store_read_stream]

Reads a slice of a stream file by byte offset, so a poller can fetch output in pieces and continue where it stopped. Used by `bench_proc_output` for `stdout.txt` and `stderr.txt`, and by status and failure reports for the last lines of `stderr.txt`.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&Path` (borrowed) | The stream file. |
| `from` | `u64`, bytes | The byte offset to start at: 0 the first time, then the `next` value returned previously. |
| `max` | `usize`, bytes | The most bytes to read in this call. |

Returns `(String, u64, bool)`: the text, the next offset, and `more` (true when bytes remain after what was read). Special cases: a missing or unopenable file gives `("", from, false)`; `from` at or past the end of the file (or a seek failure) gives `("", min(from, length), false)`, so a caller holding an offset beyond the end is brought back to the end. The slice is converted with lossy UTF-8: if `from` or the end of the slice falls inside a multi-byte character the broken bytes become the replacement character U+FFFD, and the next offset is still counted in bytes, so the two halves of such a character are not joined. A single `read` is made, so a short read gives `more` true with a smaller text; a read error returns an empty text with `next` equal to `from` (`n` is 0). Blocking file read, no lock, never fails the caller.

See also: `proc_store_append_stream`, `proc_view_output`. Example: `read_stream(p, 0, 4000)` returns the first 4000 bytes and `next = 4000, more = true`; the caller passes 4000 next time.

## Retention and listing

### `proc_store_purge` [@ANCHOR: proc_store_purge]

Deletes old finished runs. It runs at the start of every new run (`run::start`, before `new_run_dir`), so the managed directory is kept in bounds without a background task and without a separate tool.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` (borrowed) | `dir` (root), `retention_days` (`f64`, days, default 14; finished runs older than this are deleted) and `keep_runs` (`usize`, default 200; at most this many finished runs are kept, oldest deleted first). |

Returns `usize`, the number of run directories removed (0 when the `runs` directory does not exist or nothing qualified). A directory whose removal fails is neither counted nor reported.

Rules, in order: every directory in `runs/` whose `state.json` reads and parses is a candidate; one whose `state` is `starting`, `running`, `waiting_operator` or `cancelling` is skipped (it is active and never deleted); a directory with no readable `state.json` (for example a run still being created) is skipped. The rest are sorted by `ended_unix`, newest first. A run is deleted when its `ended_unix` is older than `now - retention_days * 86400`, or when its position is `keep_runs` or beyond. Deletion is `remove_dir_all`, so the run's whole directory including `scratch/` and checkpoints goes. Side effects: file deletion only. It is synchronous and its cost grows with the number of runs (one small JSON read each). It takes no lock; two server processes sharing a root could both try to delete the same directory, which is harmless.

A state file with no `state` field is left alone (it is not known to be finished). A finished run whose `ended_unix` is missing or null counts as ended at time 0 and is therefore deleted first (intended: a finished run always has the field). `retention_days` is validated by the config (0 to 3650), so 0 or a very small value deletes every finished run at the next purge (the test sets it to 0.00001).

See also: `proc_store_list_runs`, `proc_store_run_dir`. Test: `finished_runs_older_than_the_retention_time_are_deleted_when_the_next_run_starts` in `src/procedure/tests.rs`.

### `proc_store_list_runs` [@ANCHOR: proc_store_list_runs]

Lists the runs that are on disk, newest first, for the tool `bench_proc_list`.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` (borrowed) | Gives the root. |
| `limit` | `usize` | The most runs to return; `view::list` passes it through from `bench_proc_list`. |

Returns `Vec<Value>`; each item is `{"run", "state", "label", "started_unix", "ended_unix", "calls"}`, taken from the run's `state.json` (a field missing there is `null`; `ended_unix` is `null` while the run is going). The list is empty when `runs/` is missing or unreadable. Ordering: the directory names are sorted and reversed, which is newest first because ids begin with a fixed-width hex time. The `limit` is applied to directory names before `state.json` is read, so an entry without a readable `state.json` (a run still starting, or a foreign directory) uses up one place of the limit and is left out, and fewer than `limit` items can come back. It does not check that a name has the run id shape. Read-only; no locks; one JSON read per returned directory.

See also: `proc_store_purge`, `proc_view_list`, `proc_run_persist` (what `state.json` holds).

## The contributor checklist

### `bench_mcp_checklist` [@ANCHOR: bench_mcp_checklist]

The module-level anchor of `src/checklist.rs`: a file of tests (compiled only for `cargo test`) that enforces what the manual page `concepts/extending` says a new instrument module must ship. Every test here cites this anchor with a `// Tests [@ANCHOR: bench_mcp_checklist]` line. It is written so a reviewer who does not own the instrument can rely on the structure being right and spend time on the content. Where an older module predates a rule it is named in a LEGACY table, and a table only shrinks: a new module is never added to one, and a test fails when a table entry becomes unnecessary (so the exception disappears with the cause). It is run by `cargo test -p bench_mcp` on any machine; it needs no hardware, network or instrument.

There are no arguments or return values; the tables below are the data the tests read. All are module-private `const` slices.

| Table | Type | Meaning |
|---|---|---|
| `SAFE_STATE_COMPANION` | `&[(&str, &str)]` | Pairs of (tier `full` tool, the tool that puts that instrument in its safe state and reads it back). Currently 22 pairs: the supply, audio, example, Glasgow logic tools and radioctl tools. A new `full` tool is added here with its companion. |
| `FULL_WITHOUT_SAFE_STATE` | `&[(&str, &str)]` | `full` tools with no companion and the reason (more than 40 characters). Currently only `bench_scope_self_cal`: the calibration ends inside the call and leaves nothing energised. The code says not to add to it without the owner agreeing. |
| `LEGACY_ORPHAN_TEST_ANCHORS` | `&[&str]` | Anchors that tests cite although no source declares them. Currently empty; the code says not to add to it. |
| `REQUIRED_INSTRUMENT_PAGES` | `&[&str]` | The six manual pages every instrument directory under `manual/instruments/` must have: `overview`, `setup`, `operation`, `limits`, `troubleshooting`, `protocol`. |
| `LEGACY_MISSING_PAGES` | `&[(&str, &[&str])]` | Instruments excused from some of those pages. Currently empty. |
| `SERIAL_PLACEHOLDERS` | `&[&str]` | The scrubbed forms of a serial number: `SN-0000`, `SERIAL-REDACTED`. |
| `HARDWARE_CLAIMS` | `&[&str]` | Words by which a `verified` line claims a run on real equipment (`measured`, `probed`, `observed`, `confirmed`, `loopback`, `real `, and so on). |

What the tests check (each one fails the build with a message that names the offender and the manual page to read):

- `every_tool_has_the_house_shape`: every registered tool is named `bench_<instrument>_<verb>` in lower case, digits and underscores; its schema has a `properties` object; every tool except `bench_manual_*` and `bench_list` has an optional `instrument` property that is not in `required`.
- `tools_that_actuate_have_a_dry_run`: every tier `full` tool (the ones that can radiate, drive or actuate) has a `dry_run` property, which returns the plan and sends nothing.
- `every_full_tool_names_its_safe_state_companion_or_says_why_it_has_none`: every `full` tool is in exactly one of the two tables; each listed pair names registered tools; the companion is tier `read_config` or `read_only`, belongs to the same instrument family (the first two name parts) and is named `..._safe_state`; a listed tool that is no longer a registered `full` tool must be removed.
- `every_instrument_has_the_standard_manual_pages`: at least seven instrument directories exist; each has the six required pages (or is excused by the LEGACY table, which must not name a page that exists); and `whats-new.md` mentions `instruments/<name>`.
- `template_wording_does_not_survive_the_copy`: outside the example instrument and the `concepts/extending` pages no manual page contains the template phrases "The model for" or "The model of a tool", and no source file (other than `src/example/` and this file) still has the header "THE SKELETON FOR A NEW INSTRUMENT".
- `every_driver_has_a_config_example_in_the_readme`: for each driver name from `config::driver_names()` the README contains `"driver": "<name>"`; `usb_hub` and `audio` are excused by a list inside the test (see the oddities below).
- `every_test_anchor_has_a_source_anchor_in_the_crate`: over `src/` (`.rs` and `.py`) and `tests/`, every anchor cited by a `// Tests [@ANCHOR: ...]` line is declared by some other line, and every declared anchor is cited by at least one test. This is the crate's own anchor pairing check, in addition to `verify_anchors.py`.
- `the_serial_number_detector_sees_what_it_should` and `the_leak_detector_sees_what_it_should`: unit tests of the two detectors below, with lists of lines that must and must not be flagged.
- `no_fixture_page_or_doc_holds_a_serial_number`: scans the text files of `manual`, `docs`, `tests` and `src` (symlinks skipped, `src/checklist.rs` itself excluded) and `README.md` for a serial number, and for a sysfs file named `serial` holding anything but a placeholder. The line detector (`serial_leak`) looks for the word `serial` (also `serial_short`, `serial number`, lsusb `iSerial 3 VALUE`) followed by a token of six or more characters with a digit that is not a path or a placeholder, and for four-field instrument identification replies `maker,model,serial,firmware`.
- `fixtures_docs_and_manual_hold_no_addresses_or_credentials`: scans `manual` and `docs` (`.md`), `tests` (`.txt .json .csv .log .md`) and the README with `leak`, which flags a MAC address (except `00:11:22:33:44:55` and all zeros), a public IPv4 address (private, loopback, documentation, link-local and multicast ranges and dotted versions with a first number under 20 pass), and a credential word followed by a value (`password=`, `passwd=`, `api_key=`, `secret=`, `token=`, private key headers). Documentation examples use `192.0.2.x` and `SN-0000`.
- `the_guide_the_pr_template_and_the_plan_issue_are_in_place`: every `concepts/extending/*` manual page is listed in `docs/EXTENDING.md`, the page `procedures/add-an-instrument` exists and is listed, the pull request template `.github/PULL_REQUEST_TEMPLATE/bench_instrument.md` keeps its six headings and the issue template `bench_instrument_plan.md` keeps "Planned tools", and the standing-rules page keeps its incident rule keywords (including `dry_run` and the repository URL).
- `verified_lines_carry_a_real_date_that_is_not_in_the_future`: every manual page's `verified` line holds an ISO date, unless it starts with `n/a` or `see `, or is exactly `fixtures only` or `in progress`; every date must exist, must not be in the future and must not be before 2026-01-01 (the project start); a line beginning `code only` must not use a word of `HARDWARE_CLAIMS`. Uses `days_from_civil` and `iso_dates`.

Concurrency and side effects: tests only read files in the crate directory (found through `CARGO_MANIFEST_DIR`) and the embedded manual; nothing is written, no tool is called, no instrument is opened. The scans read every text file under four directories, so the whole checklist takes a short moment compared with the build.

Oddities (reported, not changed): the README sentence for this anchor (the paragraph on `src/example/`) is the only prose entry; the legacy driver list `usb_hub`, `audio` inside `every_driver_has_a_config_example_in_the_readme` is a third legacy table that lives in a function rather than at the top with the others; and several tests cite other anchors (`bench_mcp_manual_*`, such as `bench_mcp_manual_tool_registry` and `bench_mcp_manual_truncate_chars`) that belong to `src/manual.rs`, because they are the only tests that call into those functions.

See also: `docs/EXTENDING.md` and the `concepts/extending` manual pages (the rules these tests enforce), `bench_mcp_checklist_days_from_civil`, `bench_mcp_checklist_iso_dates`, `tools/new_instrument.py`.

### `bench_mcp_checklist_days_from_civil` [@ANCHOR: bench_mcp_checklist_days_from_civil]

Converts a calendar date to a day count, and in doing so tells whether the date exists. The checklist uses it to compare the dates in manual `verified` lines with today, with the project start and with each other, without a date library.

| Argument | Type | Meaning |
|---|---|---|
| `y` | `i64` | The year, proleptic Gregorian (the Gregorian rules are applied to all years). |
| `m` | `i64` | The month, 1 to 12. |
| `d` | `i64` | The day of the month, 1 up to the length of that month. |

Returns `Option<i64>`: `Some(days)` since 1970-01-01 (0 for 1970-01-01, 19,782 for 2024-02-29, negative before 1970), or `None` when the date does not exist: a month outside 1 to 12, a day of 0 or past the month's length, or 29 February in a year that is not a leap year (a leap year is divisible by 4 and not by 100, or divisible by 400). It cannot panic for any input and has no side effects, no I/O and no locks; it is pure and safe from any thread. The count is a day number, with no time zone or time of day.

Method: it uses the standard civil-from-days style arithmetic with 400-year eras (146,097 days each) and a March-based year, then subtracts 719,468 to place day 0 at 1970-01-01.

Example: `days_from_civil(2026, 2, 29)` is `None`; `days_from_civil(1970, 1, 1)` is `Some(0)`.

See also: `bench_mcp_checklist_iso_dates`; test `verified_lines_carry_a_real_date_that_is_not_in_the_future` in `src/checklist.rs` (it asserts both examples above).

### `bench_mcp_checklist_iso_dates` [@ANCHOR: bench_mcp_checklist_iso_dates]

Finds the ISO dates (`YYYY-MM-DD`) in a piece of text and gives each one's day count. The checklist applies it to the `verified` line of each manual page to check that a verification claim says when it was made.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` (borrowed) | Any text, for example `confirmed on the unit 2026-10-29`. |

Returns `Vec<(String, Option<i64>)>`: for each match in order of appearance, the ten characters as found and the result of `days_from_civil` on them, which is `None` when the shape is right but the date does not exist (`2026-13-01`). A match is any ten bytes of the form four digits, hyphen, two digits, hyphen, two digits; the scan goes left to right and continues after the ten characters of a match, so overlapping matches are not reported. Nothing about the characters before or after is checked: a longer digit run such as `12026-10-29` still yields `2026-10-29`, and `2026-10-299` yields `2026-10-29`. An empty text or one without a match gives an empty vector. It cannot panic (a match consists only of ASCII bytes, so the string slices fall on character boundaries), does no I/O, takes no lock and has no side effects.

Example: `iso_dates("a 2026-10-29 b 2026-13-01")` returns two entries, the second with `None`.

See also: `bench_mcp_checklist_days_from_civil`; test `verified_lines_carry_a_real_date_that_is_not_in_the_future`.
