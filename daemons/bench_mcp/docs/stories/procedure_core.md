# The Procedure Runner Takes My Long Bench Job Off the Agent's Context and Still Obeys Every Tier, Lock and Safe State

As the person (or the AI agent) running the bench, I want to hand `bench_mcp` a Python program that sweeps, soaks or calibrates for an hour
and have it run next to the server, so the agent does not carry every step in its context, and still have every tool the program calls go
through the same tier check, confirmation and device rules as a call I made myself, have two runs never drive the same instrument at once,
and have the instruments put back in a safe state when a run does not end well. This story documents the core of that in
`daemons/bench_mcp/src/procedure/`: the module root with the instrument's `procedure` settings and the ten `bench_proc_*` tools (`mod.rs`),
instrument reservations between runs (`locks.rs`), the safe-state routine (`safe.rs`), saved procedures (`saved.rs`), the tool index and tier
names (`tiers.rs`), the read side that answers status, output, result, cancel and list (`view.rs`) and the shipped examples (`examples.rs`).
The run supervisor (`run.rs`), the on-disk store (`store.rs`), the CPython backend and the program side (`bench_runtime.py`) are in other
story files. None of these functions keys a transmitter by itself. A procedure reaches a transmitter only by calling a tool such as
`bench_radioctl_tx_key_test` through `crate::call`, the server's normal dispatcher; the tier of the instrument, its own transmit gate and band
check, and the per-instrument lock then apply exactly as for an agent's call, and the tier cap of the run is checked first. Each entry below
says which of those it touches.

## The module root: settings, tools and dispatch (`mod.rs`)

### `proc_config` [@ANCHOR: proc_config]

Reads the `procedure` settings of one instrument entry in the bench config into a `Config`, filling every key that is absent with its default
and refusing a bad value, so the server fails at config load rather than at the first run. `parse_config` is `pub fn parse_config(c: &Value) -> Result<Config, String>`; `config.rs` calls it for an entry whose `driver` is `procedure` and prefixes the error with the instrument name. The entry's own `tier` key (read in `config.rs`, not here) is the highest tier cap any run may have.

| Key | Type and range | Default | Meaning |
|---|---|---|---|
| `dir` | absolute path string | none, which means `procedures` under the bench output directory (`store::root`) | The managed directory: `runs/`, `saved/`, `locks/`. A relative path is refused. |
| `max_runs` | number 1 to 16, cut to an integer | 2 | Runs active at once; `bench_proc_run` refuses the next one. |
| `retention_days` | number 0 to 3650 | 14 | Finished runs older than this are purged. |
| `keep_runs` | number 1 to 100000, cut to an integer | 200 | At most this many finished runs are kept. |
| `python` | absolute path string | `/usr/bin/python3` | The interpreter of the CPython backend. |
| `grace_s` | number 0.5 to 120 seconds | 5 | Time a cancelled program has for its cleanup handlers before it is killed. |
| `max_wall_s` | number 1 to 31536000 seconds | 86400 | Largest `wall_s` a run may ask for. |
| `max_memory_mb` | number 256 to 1048576 | 16384 | Largest `memory_mb` a run may ask for. |
| `wrapper` | `"none"` or `"systemd-run"` | `"none"` | `systemd-run` puts each program in a transient user scope with memory and task limits. |
| `safe_state_psu_off` | boolean | true | Whether a run that switched the supply output on has it switched off when it ends other than by success (see `proc_safe_action_for`). |
| `lock_wait_s` | number 0 to 3600 seconds | 2 | How long a tool call waits for an instrument another run holds before it is refused. |
| `operator_timeout_s` | number 1 to 604800 seconds | 3600 | How long a run may wait in `bench.ask_operator`; this wait is not counted in the wall-clock budget. |

Returns `Ok(Config)`. Errors are strings: `<key> must be a number from <lo> to <hi>` (not a number, not finite, or out of range; JSON `null` is also refused here), `dir must be an absolute path`, `python must be an absolute path`, `safe_state_psu_off must be true or false`, and `wrapper must be "none" or "systemd-run"`. Keys it does not know are ignored without a message, so a misspelled key silently keeps its default. A fractional `max_runs`, `keep_runs` or `max_memory_mb` is truncated.

Pure: no files, no locks, no instrument. `Config` itself is cloned into each run, so a config change applies to runs started afterwards.

See also: test `procedure/tests.rs` (cites `proc_config`); `proc_run_params`, which applies `max_wall_s` and `max_memory_mb`; `proc_run_tool`, which applies `max_runs`; the `procedure` section of `README.md`.

### `proc_instrument` [@ANCHOR: proc_instrument]

Picks which procedure runner entry a `bench_proc_*` call is for. A bench may have several `procedure` instruments, each with its own directory, tier and limits; the agent names one in the `instrument` argument, or leaves it out when there is exactly one. Called first by `call` for every runner tool. Signature: `fn instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String>`.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | Every configured instrument; only those of kind `Procedure` are considered. |
| `args` | `&Value`, borrowed | The tool arguments; only the string `instrument` is read. |

Returns a reference into `insts`. Errors: `no procedure runner named "<n>"` when a name was given and none matches (a name of a non-procedure instrument also gives this); `no procedure runner is configured` when there is none and no name; `name the instrument: a, b` listing the names when there are several and no name was given. A non-string `instrument` is treated as absent. Pure: no files, no locks, no instrument I/O.

See also: test `procedure/tests.rs` (cites `proc_instrument`); `proc_call`.

### `proc_tool_defs` [@ANCHOR: proc_tool_defs]

Builds the MCP (Model Context Protocol) tool list entries for the runner's ten tools from the static table `SPECS` (name, tier, description, JSON properties text, required names). The server's `tool_defs` merges the result into the list clients see, and `ToolIndex::build` (`proc_tool_index`) reads the same list to decide what a procedure may call. Signature: `pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool>`.

`insts` (borrowed) is every configured instrument. A tool appears only when some `Procedure` instrument has a tier at or above the tool's own tier, so a runner configured `read_only` lists only `bench_proc_status`, `_output`, `_result`, `_cancel`, `_list` and `_load`; the tools that start runs, answer prompts and save or delete (`bench_proc_run`, `_input`, `_save`, `_delete`) need `read_config`. Each tool's description is prefixed with the tier in Debug form, for example `[ReadConfig] PROCEDURE RUNNER: ...`, and `ToolIndex::build` parses that prefix, so the format must not change without changing it. Returns an empty list when no runner is configured.

Panics: the schema text is parsed with `unwrap`, so a malformed `props` constant in `SPECS` panics at the first listing; the tests list every tool, which would catch it. Pure and cheap, it is called on every `tools/list`.

See also: test `procedure/tests.rs` (cites `proc_tool_defs`); `proc_tool_index`, `proc_call`.

### `proc_number_arg` [@ANCHOR: proc_number_arg]

Reads one optional numeric tool argument, checks it and supplies the default. Used for `wall_s`, `memory_mb`, `max_calls`, `cpu_s`, `max_bytes`, `cursor`, `stdout_from`, `stderr_from`, `wait_s` and `limit`. Signature: `fn number(args: &Value, key: &str, default: f64, lo: f64, hi: f64) -> Result<f64, String>`.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value`, borrowed | The tool arguments object. |
| `key` | `&str` | The member to read. |
| `default` | `f64` | Returned when the member is absent or JSON `null`. |
| `lo`, `hi` | `f64` | The inclusive range a present value must be in. |

Returns the value as `f64`; callers cast to an integer, which truncates a fraction. Error: `<key> must be a number from <lo> to <hi>` when the member is present but not a number, not finite, or outside `lo..=hi`. The default is not range-checked. Note the difference from `proc_config`: there `null` is refused, here it means absent. Pure.

See also: test `procedure/tests.rs` (cites `proc_number_arg`); `proc_flag_arg`, `proc_run_params`.

### `proc_flag_arg` [@ANCHOR: proc_flag_arg]

Reads one optional true/false tool argument: `confirm_full_tier`, `trace`, `dry_run` and `confirm`. Signature: `fn flag(args: &Value, key: &str) -> Result<bool, String>`; `args` is borrowed, `key` the member name. Returns false when the member is absent or JSON `null`, the boolean when it is one, and otherwise the error `<key> must be true or false` (so `"true"` as a string and `1` are refused rather than guessed). Pure.

The flags that matter for safety are `confirm_full_tier` (a run with tier `full` needs it, and it must come from the person at the bench) and `confirm` on `bench_proc_delete`; neither can be inferred from anything else.

See also: test `procedure/tests.rs` (cites `proc_flag_arg`); `proc_number_arg`, `proc_run_params`, `proc_call`.

### `proc_run_params` [@ANCHOR: proc_run_params]

Turns the arguments of `bench_proc_run` into the `run::Params` the supervisor starts from, checking every value against the runner's limits so nothing reaches the supervisor unchecked. Called by `run_tool`. Signature: `fn run_params(inst: &Instrument, args: &Value) -> Result<run::Params, String>`; `inst` is the runner entry (borrowed) whose `procedure` config and `tier` give the limits, `args` the tool arguments (borrowed; the source and arguments are copied).

| Argument read | Type | Rule |
|---|---|---|
| `name`, `version` | string, integer | Run a saved procedure (`saved::load`; a shipped `example-` name works too). Its source is used. |
| `source` | string | Python text, 1 byte (after trimming) to 262144 bytes. Giving both `name` and a non-null `source` is an error. One of them is required. |
| `language` | string | Absent or `"python"`; anything else is refused. |
| `args` | JSON object | Becomes `bench.args`; at most 65536 bytes of JSON text; absent or `null` gives `{}`. |
| `tier` | `read_only`, `read_config`, `full` | The tier cap of the run. Default: the declared tier of a named procedure; otherwise `read_config`, or the runner's own tier when that is lower. Never above the runner's tier. |
| `confirm_full_tier` | boolean | Required true when the cap is `full`. |
| `wall_s` | number 1 to `max_wall_s` | Wall-clock budget, default 600 (or `max_wall_s` if smaller). |
| `memory_mb` | number 256 to `max_memory_mb` | Address-space limit, default 2048 (or the maximum if smaller). |
| `max_calls` | number 1 to 1000000 | Tool-call budget, default 1000. |
| `cpu_s` | number 1 to `max_wall_s + 10` | CPU-time limit, default `wall_s` rounded up plus 10. |
| `label` | string | Shown in lists; cut to 100 characters; default `<name>@<version>` of a saved procedure, else empty. |
| `trace` | boolean | Record the lines reached. |

The fixed limits `files: 256` and `file_mb: 64` are not settable. Returns `run::Params` (source, args, label, cap, wall budget, call budget, limits, trace). Errors, all strings: those of `saved::load` for a bad name or version; `give source or name, not both`; `source (or the name of a saved procedure) is required`; `source must be 1 byte to 256 KiB of Python`; `language "<x>" is not available (python)`; `args must be a JSON object of at most 64 KiB`; the `proc_tier_parse` error for a bad tier; `tier <t> is above the <Tier> tier of the procedure runner "<name>"`; `a run with tier full needs confirm_full_tier true, and that must come from the person at the bench: the run may call tools that drive outputs`; and the range errors of `proc_number_arg`. A `tier` that is present but not a string is ignored and the default applies.

Safety: this is where the tier cap is fixed. The cap travels with the run and `ToolIndex::allow` enforces it for every tool call the program makes, so a procedure that may drive outputs exists only if the instrument was configured `full` and the person at the bench confirmed. Reads the saved-procedure files when `name` is given; otherwise pure.

See also: test `procedure/tests.rs` (cites `proc_run_params`); `proc_config`, `proc_saved_load`, `proc_tier_parse`, `proc_run_tool`.

### `proc_backend_for` [@ANCHOR: proc_backend_for]

Chooses the language backend for a run. Today it always returns the CPython backend built from the runner's `python` and `wrapper` settings; the `backend.rs` `Backend` trait (`check`, `start`) is the seam for another language, which is why `language` in `bench_proc_run` is an enum with one value. Signature: `fn backend_for(cfg: &Config) -> Arc<dyn backend::Backend>`; `cfg` is borrowed and its two strings are cloned. Returns a shared backend; cannot fail and does no I/O (the interpreter is not started or looked up here, so a wrong `python` path shows up at `check` or `start`). Called by `run_tool` and by `bench_proc_save`.

See also: test `procedure/tests.rs` (cites `proc_backend_for`); `proc_run_tool`, `proc_saved_save`, `proc_config`.

### `proc_run_tool` [@ANCHOR: proc_run_tool]

Implements `bench_proc_run`: validates the request (`run_params`) and then either answers a `dry_run` or starts a run and returns its id at once, without waiting for it. The agent polls `bench_proc_status`, `bench_proc_output` and `bench_proc_result` afterwards. Signature: `fn run_tool(insts: &[Instrument], inst: &Instrument, args: &Value) -> Result<Value, String>`; `insts` is all instruments (the run's tool index is built from it), `inst` the chosen runner, `args` the arguments; all borrowed, the run keeps its own clone of the instrument list.

Dry run (`dry_run: true`): the backend `check`s the source in a throwaway `check/` directory under the managed directory, which is removed afterwards, and nothing starts. The reply is `{dry_run: true, started: false, source: <backend check result>, tool_calls: [...], tier_cap, wall_s, max_calls, memory_mb}`, where each tool the source names literally gets `{tool, tier, allowed: true}` or `{tool, allowed: false, why}` from `ToolIndex::allow` for the run's cap. The program is not executed, no instrument is touched, no lock is taken.

Start: if `run::active_in` is already at `max_runs` the call fails with `<n> runs are already active (the limit of "<name>" is <max>); wait for one or cancel it with bench_proc_cancel`. Otherwise `run::start` creates the run directory, writes `source.py`, `args.json`, `tools.json`, `config.json`, and spawns the supervisor; the reply is `{run: <id>, state: "running", tier_cap, wall_s, max_calls, next: ...}`. Errors also include every error of `run_params`, the backend `check` error for a dry run, and file errors from creating the directories.

Concurrency: the active count and the start are two steps, so two simultaneous starts could both pass the check. Whether the registry prevents that is unknown here; see `run.rs`. Side effects: files in the managed directory, a subprocess (the Python interpreter, in its own process group), later reservations and tool calls by the program. It does not itself key a transmitter; any transmit call the program makes is bound by the cap fixed here and dispatched under the rules in the opening paragraph.

See also: test `procedure/tests.rs` (cites `proc_run_tool`); `proc_run_params`, `proc_backend_for`, `proc_tool_allow`, `proc_run_start`, `proc_view_status`.

### `proc_call` [@ANCHOR: proc_call]

The dispatcher for the ten runner tools, called from the server's `call` for every name that starts with the procedure prefix. It finds the tool's `Spec`, picks the runner (`proc_instrument`), checks the runner's tier against the tool's, then runs the tool. Signature: `pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String>`; all borrowed.

| Tool | Tier | What `call` does |
|---|---|---|
| `bench_proc_run` | read_config | `run_tool`. |
| `bench_proc_status` | read_only | `view::status` for `run`. |
| `bench_proc_output` | read_only | `view::output`; `max_bytes` 100 to 200000 (default 20000), `cursor`, `stdout_from`, `stderr_from` from 0. |
| `bench_proc_result` | read_only | `view::result`. |
| `bench_proc_cancel` | read_only | `view::cancel`; `wait_s` 0 to 60 (default 0). Only stops work, so the lowest tier suffices. |
| `bench_proc_list` | read_only | `view::list` with `limit` 1 to 1000 (default 20), plus a `saved` array from `saved::list`. |
| `bench_proc_input` | read_config | `run::answer` with `answer` text. |
| `bench_proc_save` | read_config | `saved::save` with `name`, `source`, `tier` required and `description`, `note` optional. |
| `bench_proc_load` | read_only | `saved::load` with `name` required and `version`. |
| `bench_proc_delete` | read_config | `saved::delete`; `confirm` must be true. |

Errors (strings): `unknown tool <name>`; those of `proc_instrument`; `<name> needs tier <T>; "<inst>" is configured at <T>`; `run is required (the id from bench_proc_run)`; `answer is required and must be text`; `name is required`; `source is required`; `tier is required (read_only, read_config or full)`; `delete needs confirm true`; the range errors of `proc_number_arg`; and whatever the tool returns. The match has a last arm `<name> is not implemented` for a spec without a handler.

Blocking: `bench_proc_cancel` with `wait_s` and `bench_proc_input` (up to 5 s) wait on the calling thread; `bench_proc_run` returns at once. Side effects are those of the tools called; this function takes no lock of its own. A procedure's own tool calls reach `crate::call`, not this function, and `ToolIndex::allow` refuses any `bench_proc_` name, so a program cannot start or control runs.

See also: test `procedure/tests.rs` (cites `proc_call`); `proc_tool_defs`, `proc_instrument`, `proc_view_status`, `proc_saved_save`.

## Instrument reservations between runs (`locks.rs`)

### `proc_locks_path` [@ANCHOR: proc_locks_path]

Gives the file whose `flock` is one instrument's reservation. Signature: `fn lock_path(root: &Path, instrument: &str) -> PathBuf`. `root` (borrowed) is the managed directory, `instrument` the configured name. Every character that is not ASCII alphanumeric or `-` becomes `_`, and the result is `<root>/locks/<safe>.lock`, so no name can reach outside `locks/`. The owner note sits next to it with extension `.owner`.

Two names that differ only in such characters (for example `a b` and `a_b`) map to the same file and so exclude each other; instrument names in real configs rarely differ this way. The function does not create anything and cannot fail.

See also: test `procedure/tests.rs` (cites `proc_locks_path`); `proc_locks_reserve`, `proc_locks_owner_text`.

### `proc_locks_instrument_of` [@ANCHOR: proc_locks_instrument_of]

Works out which instrument a procedure's tool call is for, so the run can reserve it before a call that changes something. Called by `run::exec_call` for every call above the read-only tier. Signature: `pub fn instrument_of(insts: &[Instrument], tool: &str, args: &Value) -> Option<String>`; all borrowed.

Order of the rules: (1) a string `instrument` argument, returned as given and not checked against the list; (2) for a tool in the main `TOOLS` table, the instrument `crate::pick` selects for those arguments, none if `pick` fails; (3) for a tool of a module prefix (`bench_audio_`, `bench_example_`, `bench_radioctl_`, `bench_rotator_`, `bench_usb_`), the only configured instrument of that kind, none when there are zero or several. Returns `Some(name)` or `None`; `None` means the call is made without a reservation, and the dispatcher's per-instrument mutex (`instrument_lock` in `main.rs`) still serialises the call itself. A tool whose module is not in the table above (for example the scope or the supply, if they are not in `TOOLS`) is only found by rule 2: this is unknown from this file alone.

Pure: no files, no I/O. Cost: linear scans over the tool and instrument lists.

See also: test `procedure/tests.rs` (cites `proc_locks_instrument_of`); `proc_locks_reserve`, `bench_mcp_pick`.

### `proc_locks_owner_text` [@ANCHOR: proc_locks_owner_text]

Describes the holder of a reservation for the refusal message, so the agent can see which run to wait for or cancel. Signature: `fn owner_text(root: &Path, instrument: &str) -> String`. Reads `<root>/locks/<name>.owner`, a JSON object `{run, label, pid, since_unix}` written by `reserve`, and returns `run <id> ("<label>", holding it for <n> s)`. If the file is missing or unreadable it returns `another procedure run`; a file that is not JSON gives `run ? ("")`, and a missing `since_unix` omits the holding time. Only advice: the lock file, not this note, is the truth about who holds it, so the note can be stale or briefly absent while a run takes or gives up a reservation. Read-only file access, no lock.

See also: test `procedure/tests.rs` (cites `proc_locks_owner_text`); `proc_locks_reserve`, `proc_locks_release`.

### `proc_locks_reserve` [@ANCHOR: proc_locks_reserve]

Takes the reservations of a set of instruments for one run, all or none, waiting up to a timeout. Called by `RunCtx::hold` (`run.rs`), which serves both the automatic reservation before a changing tool call and the program's `bench.reserve`. Signature:

```rust
pub fn reserve(root: &Path, run: &str, label: &str, names: &[String], wait: Duration,
               cancelled: &dyn Fn() -> bool) -> Result<Vec<(String, File)>, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `root` | `&Path` | The managed directory; `locks/` is created under it. |
| `run` | `&str` | Run id written to the owner note. |
| `label` | `&str` | Run label written to the owner note. |
| `names` | `&[String]` | Instruments to take; sorted and de-duplicated here, so two runs asking for the same set take it in the same order and cannot deadlock. |
| `wait` | `Duration` | Total time to keep trying across all names (the same clock for the whole call). `Duration::ZERO` tries once. |
| `cancelled` | `&dyn Fn() -> bool` | Polled between attempts (every 25 ms); true gives up at once. |

For each name it opens or creates the lock file (never truncated) and tries `flock(LOCK_EX | LOCK_NB)` in a loop with 25 ms sleeps, then writes the owner note `{run, label, pid, since_unix}` with a best-effort write. Returns the held `(name, File)` pairs; the caller must keep them, because the kernel releases a lock when the file is dropped or the process ends, including when the server is killed. On failure it drops what it already took (so none is held) and returns `instrument "<n>" is reserved by <owner_text>. Wait for it to end (bench_proc_status), or call bench.reserve with a timeout to wait for it`; other errors are `locks: <io error>` and `<path>: <io error>`. When `wait` has passed or `cancelled()` is true while a name is still taken, that is the failure.

Concurrency and side effects: the lock is between processes sharing the directory, not between threads of one process that open the file twice with separate descriptors; `flock` on a second descriptor of the same file in the same process does conflict, which is how two runs of one server exclude each other. It blocks the calling thread while waiting, so it must not be called with a lock another run needs (`hold` takes only the run's own `held` list). The owner notes of names already taken in a failed call are not removed, so a later refusal can name a run that no longer holds the instrument. Only procedure runs reserve; the transmit gate and band check of any radio tool are applied later by the dispatcher, not here.

See also: test `procedure/tests.rs` (cites `proc_locks_reserve`); `proc_locks_release`, `proc_locks_path`, `proc_locks_owner_text`, `proc_run_hold`.

### `proc_locks_release` [@ANCHOR: proc_locks_release]

Releases one reservation a run holds, on the program's `bench.release` or when a run ends. Signature: `pub fn release(root: &Path, held: &mut Vec<(String, File)>, name: &str)`. `root` is the managed directory, `held` the run's list (mutated: the entry is removed), `name` the instrument. If `name` is in `held` it deletes the owner note, removes the entry, flushes and drops the file, which releases the `flock`; the lock file itself stays on disk for the next run. A name not held is ignored, and file errors are ignored. Returns nothing. The caller holds the run's `held` mutex; no other lock is taken. Idempotent.

See also: test `procedure/tests.rs` (cites `proc_locks_release`); `proc_locks_reserve`, `proc_run_reserve_op`.

## The safe-state routine (`safe.rs`)

### `proc_safe_action_for` [@ANCHOR: proc_safe_action_for]

Decides whether a tool call a procedure is about to make means that an instrument must be put in a safe state later, and if so what call does it. Called by `run::exec_call` for every call before it is dispatched. Signature: `pub fn action_for(tool: &str, args: &Value, cfg: &Config) -> Option<Value>`.

| Argument | Type | Meaning |
|---|---|---|
| `tool` | `&str` | The tool the procedure is calling. |
| `args` | `&Value` | Its arguments. A boolean `dry_run` of true means the call changes nothing, so no action. |
| `cfg` | `&Config` | Only `safe_state_psu_off` is read. |

The table `RULES` is the registry; a tool gets its safe state by having a row:

| Trigger tool(s) | Counts only when | Undone by |
|---|---|---|
| `bench_psu_output` | `on` is true, and `safe_state_psu_off` is true | `bench_psu_safe_state` |
| `bench_vna_generator_on`, `_am`, `_fm` | always | `bench_vna_generator_off` |
| `bench_example_pulse` | always | `bench_example_safe_state` |
| `bench_radioctl_tx_key_test`, `bench_radioctl_tx_power_sweep` | always | `bench_radioctl_safe_state` |
| `bench_rotator_point`, `bench_rotator_park` | always | `bench_rotator_stop` |
| `bench_set` | `trigger_mode` is `STOP` or `SINGLE` | `bench_set` with `trigger_mode` `AUTO` |

Returns `None` when no row matches, the condition fails, the PSU rule is switched off, or it is a dry run; otherwise `{"tool": <undo tool>, "args": {instrument?, <fixed arg>?}, "trigger": <tool>}`. The `instrument` argument of the trigger call is copied when present, so the undo goes to the same device; without it the undo call carries no instrument and relies on the dispatcher's own pick, which fails if several instruments fit (the failure is reported in the outcome of `proc_safe_run`). Pure, no I/O. For a transmitter the row for the two radio tools means that when a run ends badly `bench_radioctl_safe_state` is called through the dispatcher (the module's header names this as PTT released at minimum power); the transmit gate and band check of the radio instrument apply to that call as to any other.

See also: test `procedure/tests.rs` (cites `proc_safe_action_for`); `proc_safe_record`, `proc_safe_run`, `proc_config`.

### `proc_safe_record` [@ANCHOR: proc_safe_record]

Adds a safe-state action to the run's list of things to undo and saves the list to disk, before the triggering call is dispatched, so the routine knows what to undo even if the server crashes mid-call. Signature: `pub fn record(dir: &Path, touched: &mut Vec<Value>, action: Value)`. `dir` is the run's directory (borrowed), `touched` the run's in-memory list (mutated, owned by the run context behind a mutex the caller holds), `action` the value from `action_for` (moved).

An action with the same `tool` and `args` as one already recorded is dropped, so the list holds one entry per safe-state call and instrument. A new one is appended and the whole list written to `touched.json` through `store::write_json` (a write that cannot be seen half-done). A write error is ignored: the in-memory list is then right but a crash would lose it. Returns nothing. After a restart, `run::recover` reads `touched.json` to put the instruments of a lost run in their safe state.

See also: test `procedure/tests.rs` (cites `proc_safe_record`); `proc_safe_action_for`, `proc_safe_run`.

### `proc_safe_run` [@ANCHOR: proc_safe_run]

Runs the recorded safe-state calls through the normal dispatcher when a run ends other than by success (cancel, time-out, error, crash, server shutdown, or found lost at the next start). Called by the supervisor (`run.rs`) and by `recover` and `shutdown`. Signature: `pub fn run(insts: &[Instrument], touched: &[Value]) -> Vec<Value>`; both borrowed.

The actions are taken in reverse order of recording (the last change is undone first). Each is called with `crate::call(insts, tool, args)`, up to three attempts with a one second sleep between failures, and the outcome object `{tool, args, trigger, attempts, ok, result | error}` is collected, with a successful result cut by `run::clip`. Returns one outcome per action; a failure is not an error of the function but `ok: false` with the error text, and the supervisor puts the list in the result so an undone safe state is visible, never silently dropped.

Safety and concurrency: these calls use the dispatcher directly, so they are not subject to the run's tier cap, call budget or reservation check (the run still holds its reservations while this runs); the instrument's own tier and the per-instrument lock apply. The function blocks its thread for up to about two seconds per failing action. It does not itself decide whether the run needs it; that is the caller's rule. Idempotent for tools like `generator_off` and `stop`; repeating it after success is harmless.

See also: test `procedure/tests.rs` (cites `proc_safe_run`); `proc_safe_action_for`, `proc_safe_record`, `proc_run_recover`.

## Saved procedures (`saved.rs`)

Saved procedures live under `<managed dir>/saved/<name>/`: `meta.json` (`{name, description, tier, versions: [{version, saved_unix, bytes, tier, note}]}`) and one source file `v<N>.py` per version. `KEEP_VERSIONS` (20) versions are kept; `MAX_SOURCE` is 262144 bytes. The three shipped examples are also listed and loadable under the reserved `example-` names, but are never on disk.

### `proc_saved_valid_name` [@ANCHOR: proc_saved_valid_name]

Says whether a string can be the name of a saved procedure. Signature: `pub fn valid_name(name: &str) -> bool`. True when the name is 1 to 60 bytes of `a-z`, `0-9`, `_` and `-` and starts with a lowercase letter or digit (the length test is in bytes, which equals characters because the allowed ones are ASCII, and the empty string is false). No path can be made of a valid name, which is the whole point: it becomes a directory name. Pure.

See also: test `procedure/tests.rs` (cites `proc_saved_valid_name`); `proc_saved_dir_of`.

### `proc_saved_dir_of` [@ANCHOR: proc_saved_dir_of]

The directory of a saved procedure, after the two checks every save, load and delete needs. Signature: `fn dir_of(cfg: &Config, name: &str) -> Result<PathBuf, String>`; `cfg` is borrowed (for the managed directory), `name` the procedure. Returns `<managed dir>/saved/<name>`. Errors: a name starting with `example-` gives `"<n>" is a shipped example: it cannot be saved over or deleted (save a copy under another name)`; a name failing `valid_name` gives `"<n>" is not a procedure name (1 to 60 characters of a-z, 0-9, _ and -, starting with a letter or digit)`. Does not check that the directory exists and does no I/O.

See also: test `procedure/tests.rs` (cites `proc_saved_dir_of`); `proc_saved_valid_name`, `proc_examples_find`.

### `proc_saved_save` [@ANCHOR: proc_saved_save]

Saves a procedure as the next version of its name, after checking that it compiles and that its declared tier covers every tool it names. Called by `bench_proc_save` (tier read_config). Signature:

```rust
pub fn save(cfg: &Config, insts: &[Instrument], backend: &dyn Backend,
            what: (&str, &str, &str, &str), note: &str) -> Result<Value, String>
```

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | Gives the managed directory. |
| `insts` | `&[Instrument]` | Builds the full tool index to look up the tiers of the tools the source names. |
| `backend` | `&dyn Backend` | `check(source, scratch)` compiles the source without running it. |
| `what` | `(name, source, description, declared tier)`, all `&str` | Name as `valid_name`; source 1 byte to 256 KiB; description may be empty; tier one of `read_only`, `read_config`, `full`. |
| `note` | `&str` | What changed in this version, stored in `meta.json`. |

Steps: `dir_of`; `parse_tier`; source size; the backend check in a temporary `check/` directory (removed afterwards); then each tool the source names as a string literal in `bench.call` is looked up. A tool whose tier is above the declared tier is an error: `the source calls <t>, which needs tier <a>, but the declared tier is <b>: declare <a> or change the source`. A tool that is unknown, a procedure tool, or not available is not an error; it is listed with an `unavailable` reason. Then the next version number (highest existing plus one) is written to `v<N>.py`, entries beyond the newest 20 and their files are deleted, the description is replaced only when non-empty (or when none exists yet), the tier of `meta.json` is set, and `meta.json` is written atomically.

Returns `{name, version, tier, tool_calls: [{tool, tier} | {tool, unavailable}]}`. Other errors: `source must be 1 byte to 256 KiB of program text`, `the source does not compile: <error> (line <n>)`, the backend's own error (for example the interpreter cannot start), and file errors with the path. Only literal tool names are seen, so a program that builds the name at run time can call a higher tier than declared; the run's tier cap, not this check, is what stops it. Nothing is run and no instrument is touched. There is no lock: two saves of one name at the same moment could take the same version number. A `meta.json` that cannot be read or parsed is treated as a new procedure, so its old versions are no longer listed.

See also: tests `procedure/tests.rs` (cite `proc_saved_save`); `proc_saved_load`, `proc_tool_allow`, `proc_backend_for`.

### `proc_saved_load` [@ANCHOR: proc_saved_load]

Returns a saved procedure with its source, for `bench_proc_load` and for `bench_proc_run` with `name`. Signature: `pub fn load(cfg: &Config, name: &str, version: Option<u64>) -> Result<Value, String>`; `version` `None` means the latest.

A name that matches a shipped example (`examples::find`) is answered from the program text built into the server: version 1 only, `shipped: true`; any other version is `<name> is a shipped example with one version`. Otherwise the name goes through `dir_of`, `meta.json` is read, and the version is checked against the versions listed there. The source is read from `v<N>.py`. Returns `{name, version, tier, description, versions: [...], source}` where `tier` is that version's tier (falling back to the procedure's). Errors: those of `dir_of`; `no saved procedure "<n>" (see bench_proc_list)`; `<n> has no version <v> (it has [...])`; `the procedure has no versions`; and a file read error. Read-only; takes no lock, so it can see a save half way (a version listed in `meta.json` is only written after its source file).

See also: test `procedure/tests.rs` (cites `proc_saved_load`); `proc_saved_save`, `proc_run_params`, `proc_examples_find`.

### `proc_saved_delete` [@ANCHOR: proc_saved_delete]

Deletes one version of a saved procedure, or all of it. Called by `bench_proc_delete` after it has checked `confirm` is true. Signature: `pub fn delete(cfg: &Config, name: &str, version: Option<u64>) -> Result<Value, String>`.

With `version` `None` the whole directory `saved/<name>` is removed and the reply is `{name, deleted: "all versions"}`. With a version, that entry is removed from `meta.json` and `v<N>.py` deleted; deleting the only remaining version removes the directory (`deleted: "the last version, so the procedure is gone"`), otherwise `meta.json` is rewritten atomically (`deleted: "version <v>"`). The top-level `tier` and `description` in `meta.json` are not recomputed. Errors: those of `dir_of` (shipped `example-` names cannot be deleted); `no saved procedure "<n>"`; `<n> has no version <v>`; file errors. Permanent: there is no trash. Runs that already started keep their own copy of the source (`source.py` in the run directory) and are not affected. No lock.

See also: test `procedure/tests.rs` (cites `proc_saved_delete`); `proc_saved_save`, `proc_call`.

### `proc_saved_list` [@ANCHOR: proc_saved_list]

Lists the saved procedures and the shipped examples, for the `saved` part of `bench_proc_list`. Signature: `pub fn list(cfg: &Config) -> Vec<Value>`. Reads every `saved/*/meta.json` (a directory without a readable one is skipped), sorts by name and appends the shipped examples. Each item is `{name, description, tier, latest, versions}` where `latest` is the highest version number and `versions` how many are kept, and a shipped one has `latest: 1`, `versions: 1`, `shipped: true`. If the `saved` directory does not exist only the shipped examples are returned. Cannot fail. Read-only, no lock; cost grows with the number of saved procedures.

See also: test `procedure/tests.rs` (cites `proc_saved_list`); `proc_saved_load`, `proc_examples_find`.

## Which tools a procedure may call (`tiers.rs`)

The tool index is a `ToolIndex` built from the same list clients see. It holds, per tool name, `ToolInfo {tier, description, schema}` (the JSON schema of the arguments), kept in a name-ordered map. The constant `NOT_FOR_PROCEDURES` is `bench_proc_`: no tool with that prefix is ever in an index, because a procedure that starts procedures would get around the run limit and the tier cap of its parent.

### `proc_tier_name` [@ANCHOR: proc_tier_name]

Spells a tier the way the `tier` argument, the manual and the reports spell it. Signature: `pub fn tier_name(t: Tier) -> &'static str`. Returns `read_only` for `Tier::ReadOnly`, `read_config` for `ReadConfig` and `full` for `Full`. Cannot fail; pure. It is the inverse of `parse_tier`. Note that the descriptions in the tool list use the Debug spelling (`ReadOnly`) instead, which `ToolIndex::build` parses.

See also: test `procedure/tests.rs` (cites `proc_tier_name`); `proc_tier_parse`.

### `proc_tier_parse` [@ANCHOR: proc_tier_parse]

Reads a `tier` argument. Signature: `pub fn parse_tier(s: &str) -> Result<Tier, String>`; exact lower-case match of `read_only`, `read_config` or `full`. Any other text, including different case or spaces, gives the error `tier "<s>" is not one of read_only, read_config, full`. Used by `run_params` and `save`. Pure.

See also: test `procedure/tests.rs` (cites `proc_tier_parse`); `proc_tier_name`, `proc_run_params`.

### `proc_tool_index` [@ANCHOR: proc_tool_index]

Builds the index of tools a run with a given tier cap may call. Called by `run::start` for each run (and its serialised form is the run's `tools.json`), and by `run_tool` for a dry run. Signature: `pub fn build(insts: &[Instrument], cap: Tier) -> ToolIndex` (an associated function of `ToolIndex`); `insts` is borrowed, `cap` the run's tier cap.

It takes `crate::tool_defs(insts)`, the list the MCP clients see, drops every `bench_proc_` tool, reads each tool's tier from the bracketed prefix of its description (`[ReadOnly]`, `[ReadConfig]`, `[Full]`) and keeps those whose tier is at most `cap`. A tool whose description does not begin with one of the three prefixes is skipped, so it cannot be called from a procedure; this ties the index to the format written by `proc_tool_defs` and the other modules. Returns the index; it cannot fail. Because the list depends on the configured instruments, a tool of an instrument that is not configured is not in it. It does no I/O and takes no lock, but it builds the whole tool list each time, which costs a little more than a lookup.

See also: test `procedure/tests.rs` (cites `proc_tool_index`); `proc_tool_allow`, `proc_tool_index_json`, `proc_tool_defs`.

### `proc_tool_allow` [@ANCHOR: proc_tool_allow]

The gate every tool call of a procedure passes before it is dispatched: may this run call this tool. Called by `run::exec_call` for each call and by dry runs and saves to report on literal names. Signature: `pub fn allow(&self, name: &str, cap: Tier, all: &[Instrument]) -> Result<Tier, (&'static str, String)>`; `name` is the tool, `cap` the run's cap, `all` every instrument (used only to build the explanation).

Returns `Ok(tier)`, the tool's own tier, when it is in this index (so its tier is within `cap`). Otherwise `Err((kind, message))` with the kind the program sees: `policy` for a `bench_proc_` name (`<name>: a procedure cannot start or control procedures`), `tier` when the tool exists at a higher tier (`<name> needs tier <t>; this run's tier cap is <cap>`), and `tool` when it is not a tool of this bench at all (`... unknown, or above every instrument's tier`). The `tier` and `tool` explanations build a second, full-tier index, which costs a full list build, and happens only on refusal. It is the tier cap of the run, not the instrument's tier, that is checked here; the dispatcher checks the instrument's tier afterwards, and a confirmation or attestation argument of the tool itself is still demanded by the tool. For tools that can transmit, passing this gate means only that the run is allowed to call them: the radio's own tier, transmit gate and band check decide whether it keys. Pure.

See also: test `procedure/tests.rs` (cites `proc_tool_allow`); `proc_tool_index`, `proc_run_params`, `proc_run_tool`, `proc_saved_save`.

### `proc_tool_index_json` [@ANCHOR: proc_tool_index_json]

Serialises the index as the program's `bench.tools()` returns it. Signature: `pub fn to_json(&self) -> Value`. Returns a JSON object from tool name to `{tier: "read_only" | "read_config" | "full", description: <text with the bracketed tier prefix>, schema: <JSON schema of the arguments>}`, names in sorted order. Written by `run::start` to `tools.json` in the run's directory, from which the Python side loads it. It describes only what this run may call, not every tool. Cannot fail; pure.

See also: test `procedure/tests.rs` (cites `proc_tool_index_json`); `proc_tool_index`, `proc_tier_name`.

## The read side: status, output, result, cancel, list (`view.rs`)

These functions read the managed directory, so they answer the same for a live run, a finished run and one left by a server process that restarted. All take `cfg: &Config` (borrowed) and a run id `id`, which `store::run_dir` checks (a run id is `r` plus 14 hex digits, and the directory must exist, so an id cannot name another path). They take no lock; the JSON files are replaced whole by `store::write_json`, so a reader sees an old or a new version, while the event list and the output streams are appended to and read up to their current end.

### `proc_view_terminal` [@ANCHOR: proc_view_terminal]

Says whether a run state means the run is over. Signature: `fn terminal(state: &str) -> bool`. False for `starting`, `running`, `waiting_operator` and `cancelling`; true for anything else: `succeeded`, `failed`, `cancelled`, `timed_out`, `lost`, and also any unknown or empty text, so a state this function does not know counts as finished. Used for the `finished` field of status, output and result, and by `cancel` to stop waiting. Pure.

See also: test `procedure/tests.rs` (cites `proc_view_terminal`); `proc_view_status`.

### `proc_view_status` [@ANCHOR: proc_view_status]

Implements `bench_proc_status`. Signature: `pub fn status(cfg: &Config, id: &str) -> Result<Value, String>`. Reads `state.json` of the run and returns it with two fields added: `elapsed_s` (seconds from `started_unix` to `ended_unix`, or to now while running, rounded to a millisecond) and `finished` (`terminal` of the state). The record carries the state, label, the current step or last call, the counters (calls, errors, hook violations), the limits and, for a run waiting for the operator, the prompt in `awaiting`. Errors: the run id and directory errors of `store::run_dir`, and a read or parse error for `state.json` with the path. Read-only. A missing `started_unix` counts as 0, which makes a huge `elapsed_s`.

See also: test `procedure/tests.rs` (cites `proc_view_status`); `proc_view_terminal`, `proc_view_output`, `proc_view_result`, `proc_call`.

### `proc_view_output` [@ANCHOR: proc_view_output]

Implements `bench_proc_output`: the progress of a run since the last time the agent looked. Signature: `pub fn output(cfg: &Config, id: &str, cursor: u64, out_from: u64, err_from: u64, max_bytes: usize) -> Result<Value, String>`.

| Argument | Type | Meaning |
|---|---|---|
| `id` | `&str` | The run id. |
| `cursor` | `u64` | The event number returned last time; events with a larger `seq` are returned. 0 starts from the beginning. |
| `out_from`, `err_from` | `u64` byte offsets | Where to continue reading `stdout.txt` and `stderr.txt`. |
| `max_bytes` | `usize` | Bound for each part (`proc_call` allows 100 to 200000, default 20000). |

Returns `{run, state, finished, events, cursor, more_events, stdout: {text, next, more}, stderr: {...}, emitted}`: the events (emitted values, log lines, tool calls, steps) up to the byte bound (the first is always returned even if larger), the new `cursor` to pass back (unchanged when nothing is new), the stream text with its next offset (the text is decoded as UTF-8 with invalid bytes replaced, so an offset in the middle of a character gives odd text), and the latest emitted values from `emitted.json` (an empty object if none). `more` flags say there is more to read now. Errors: those of `status`, and a read error of the event file other than not found. Read-only.

See also: test `procedure/tests.rs` (cites `proc_view_output`); `proc_store_read_events`, `proc_store_read_stream`, `proc_view_status`.

### `proc_view_result` [@ANCHOR: proc_view_result]

Implements `bench_proc_result`. Signature: `pub fn result(cfg: &Config, id: &str) -> Result<Value, String>`. When `result.json` exists it is returned (the final value or the structured failure report: error, traceback, local variables of the procedure's own frames, the last tool calls, the emitted values and the safe-state outcomes) with `finished: true` and `stderr_tail` added. Otherwise it returns a record with `ok: false`, `value: null`, the state, `finished`, a `failure` of `{kind: state, message: note}` when the run is finished (null while running), what has been emitted so far and the `note` that no result exists yet, which also covers a run that ended before a result could be written. Errors: those of `status`. Read-only.

`stderr_tail` is the last 2000 bytes of `stderr.txt` (`store::read_tail`; an earlier version read the first 2000 bytes, which for a long error output showed the beginning).

See also: test `procedure/tests.rs` (cites `proc_view_result`); `proc_view_status`, `proc_safe_run`.

### `proc_view_cancel` [@ANCHOR: proc_view_cancel]

Implements `bench_proc_cancel`: asks a run to stop and optionally waits for it. Signature: `pub fn cancel(cfg: &Config, id: &str, wait_s: f64) -> Result<Value, String>`; `wait_s` is 0 to 60 seconds as checked by `call`. It validates the id, and if this server process has the run (`run::lookup`) calls `request_cancel("cancel")`; the first cancel reason of a run wins. The program's cleanup handlers then run for `grace_s` and the process group is killed, and the safe-state routine runs (`proc_safe_run`). Then it polls the state every 25 ms until the run is finished or `wait_s` has passed and returns the status record (`proc_view_status`), so with `wait_s` 0 the returned state is usually still `running` or `cancelling`.

A run of an earlier server process is not in the table, so nothing is asked of it and its status comes back unchanged (such a run is normally marked `lost` at server start by `recover`). A finished run returns its final state. Errors: those of `store::run_dir` and `status`. Blocking: the calling thread sleeps while waiting. It needs only the lowest tier because it only stops work; it never starts anything or calls a tool itself.

See also: test `procedure/tests.rs` (cites `proc_view_cancel`); `proc_run_request_cancel`, `proc_run_lookup`, `proc_run_recover`.

### `proc_view_list` [@ANCHOR: proc_view_list]

Implements `bench_proc_list`, the run part. Signature: `pub fn list(cfg: &Config, limit: usize) -> Value`; `limit` is 1 to 1000 as checked by `call`. Returns `{runs: [{run, state, label, started_unix, ended_unix, calls}], active_here}`: up to `limit` runs found in `runs/`, newest first (ids sort by start time), skipping a directory with no readable `state.json`, and `active_here`, the number of runs active in this server process for this managed directory (`run::active_in`). `call` adds a `saved` array. Cannot fail; a missing directory gives an empty list. Read-only.

See also: test `procedure/tests.rs` (cites `proc_view_list`); `proc_store_list_runs`, `proc_run_active_in`, `proc_saved_list`.

## The shipped examples (`examples.rs`)

### `proc_examples_find` [@ANCHOR: proc_examples_find]

Looks up one of the example procedures shipped inside the server. The table `SHIPPED` holds three `Example {name, tier, description, source}` values, whose source text is embedded with `include_str!` from `src/procedure/examples/*.benchproc`: `example-sweep-series` (the same VNA sweep several times with retries and a power cycle after failures in a row), `example-soak-round` (rounds of generator steps read on the scope and the VNA, compared with a baseline, counting ERROR and WRONG) and `example-calibration-sequence` (the VNA calibration with operator prompts answered through `bench_proc_input`); all three are tier `read_config`. Each takes its real-bench tool names and settings from `bench.args` with defaults, so the same text runs on the mock bench in the tests.

Signature: `pub fn find(name: &str) -> Option<&'static Example>`. An exact match of the name gives the example, anything else `None`. The reserved prefix is `PREFIX`, `example-`: `proc_saved_dir_of` refuses to save or delete any name starting with it, `proc_saved_load` answers these names with version 1 and `shipped: true`, and `proc_saved_list` appends them. The manual's `procedures/` pages hold the same text and a test fails if the two differ. Pure; no files, no locks, no instrument.

See also: test `procedure/tests.rs` (cites `proc_examples_find`); `proc_saved_load`, `proc_saved_list`, `proc_saved_dir_of`, `proc_run_params`.
