# The Server Reads Its Instrument List, Opens the Right Link and Serves One Tool Call at a Time

As the person (or the AI agent) running the bench, I want `bench_mcp` to start from one JSON file, refuse a bad file before it touches any
hardware, hand every tool call to the right instrument, never run two conversations on one serial port or scope socket, and give me readable
errors, so that a wrong config or a wrong tool name is found at once and a dead instrument does not hide the others. This story documents the
core of that: the server entry point and tool dispatch (`daemons/bench_mcp/src/main.rs`), the config file parser (`config.rs`) and the SCPI link
over TCP or a serial port (`link.rs`). The per-instrument tool pages are in the other stories and the `docs/*_TOOLS.md` files; the user-level
description of the config is the README. Line numbers are not cited because they move; every entry names the function and its anchor.

Safety that applies across this slice. None of these functions keys a radio transmitter. Three of them decide what may drive hardware: the tier of
an instrument (`read_only`, `read_config`, `full`, parsed by `tier_of`, checked in `call`), the one-call-at-a-time lock (`instrument_lock`) and the
supply's timed switch-off (`psu_output`, `psu_timer`). `call` checks the tier only for the tools in its own `TOOLS` table (scope, supply, VNA,
SDR); the tools of the module rows in `MODULES` (audio, ELAD, example, Glasgow, procedures, radioctl, rotator, RX-888, spectrum analyzer, USB hub)
and the manual tools are handed over before that check, so each of those modules does its own tier check (not verified here: see that module's
story). The band check and the transmit gate of the radio tools live in the `radioctl` module, not in this file.

## The output directory

### `out_dir` [@ANCHOR: bench_mcp_main_out_dir]

The directory where captures, screenshots, CSV files and calibration files are written. Every tool that saves a file calls it (for example
`scope::capture`, `vna::sweep`, `sdr::capture`) and passes the result on. It reads the process environment variable `BENCH_MCP_OUT` and gives it to
`out_dir_for`.

No arguments. Returns a `PathBuf` that exists (or that `create_dir_all` tried to create: see `out_dir_for`). It cannot fail or panic. It reads
the environment on each call, so changing `BENCH_MCP_OUT` while the server runs changes where the next file goes. It is `pub(crate)` because the
modules call it. Tests do not set the variable: they call `out_dir_for` with their own value. See also: the test for `out_dir_for`, and
`default_out_dir`.

### `out_dir_for` [@ANCHOR: bench_mcp_main_out_dir_for]

Decides the output directory from a given value of `BENCH_MCP_OUT`, creates it and returns it. It exists so a test can pass its own value instead of
setting the process-wide variable that every other test reads.

| Argument | Type | Meaning |
|---|---|---|
| `configured` | `Option<OsString>` (owned) | The value of `BENCH_MCP_OUT`. `None`: not set, so the default directory is used. `Some(path)` is used as given, relative paths stay relative to the server's working directory. An empty string is taken as a path (an empty `PathBuf`): unknown what the file system then does with it; the code does not check. |

Returns the `PathBuf`. Side effect: `create_dir_all` on it, and the error of that call is ignored (`let _ =`), so a path that cannot be created is
returned anyway and the first tool that writes a file there fails with the operating system's error. No locking, no panic.

### `product_default_out_dir` [@ANCHOR: bench_mcp_main_product_default_out_dir]

The product's default output directory when `BENCH_MCP_OUT` is not set: `bench_mcp` under the system temporary directory (`std::env::temp_dir()`,
which honours `TMPDIR`; `/tmp/bench_mcp` on a normal Linux box). It is its own function so a test can check the production value even though tests
are redirected by `default_out_dir`. No arguments; returns a `PathBuf`; creates nothing; cannot fail. Called only by `default_out_dir` (non-test
build) and by the test that checks the value.

### `default_out_dir` [@ANCHOR: bench_mcp_main_default_out_dir]

What `out_dir_for(None)` uses. In the shipped build it is `product_default_out_dir()`. In a test build (`#[cfg(test)]`) it is instead the single
per-process directory of `testdir::process_dir()`, which every test of the run shares and which is removed when the test process exits, so a test
run leaves nothing in the temporary directory. The choice is made at compile time, not by a flag at run time, so the shipped binary contains no
test path. No arguments; returns a `PathBuf`; it creates nothing itself (`out_dir_for` does).

## Choosing the instrument and linking to it

### `pick` [@ANCHOR: bench_mcp_pick]

Chooses which configured instrument a call is for. `call` uses it for every tool in the `TOOLS` table after the manual, module and survey cases.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` (borrowed) | All configured instances, in the file's order. |
| `tool` | `&ToolSpec` (borrowed) | The tool being called; its `kinds` list says which instrument kinds it applies to. |
| `args` | `&Value` (borrowed) | The tool arguments. Only the string `instrument` is read (by then `with_device` has copied `device` into it). |

Returns `Ok(&Instrument)` borrowed from `insts`. With `instrument` given: the instance of that name, if the tool applies to its kind. Without it:
the only configured instance the tool applies to. Errors (all `Err(String)`): no instance of that name (the text lists the configured names and says
the argument is also called `device`), the tool does not apply to that instance's kind (`<tool> does not apply to "<name>"`), the tool applies to
no configured instance, or several fit and none was named (the text lists their names). A non-string `instrument` is treated as absent. No I/O, no
locking, no panic. Tests: the pick test in `main.rs`.

### `connect` [@ANCHOR: bench_mcp_main_connect]

Opens the SCPI link of an instrument that is spoken to through `Link`: a scope or a supply. It is called by `call` once the tier and lock are
settled, so a refused call never opens a port.

| Argument | Type | Meaning |
|---|---|---|
| `i` | `&Instrument` (borrowed) | The instrument. Its `kind`, `host`, `port`, `serial` and `baud` are read. |

For `Kind::Scope` it calls `Link::tcp_checked` with the host, the port and waits of 2, 5, 10 and 20 seconds (37 s of waiting at most, five tries),
the diagnosis being `Link::scope_state(host, [port, 80])`; a scope that kept a dead session after a power loss is therefore waited out before the
call reports NO-GO. For `Kind::Psu` it calls `Link::serial(serial, baud)`. A missing host or serial becomes an empty string, which makes the link
fail with its own message (the config parser already refuses a scope without `host` and a supply without `serial`). Any other kind returns
`Err("<Kind> instruments have no SCPI link")`: those kinds have their own module and open their own link. Blocking: up to about 37 s plus the connect
time for a scope, up to 10 s for a serial port held by another process. It opens a socket or a device file; it holds no lock itself (the caller
holds the instrument lock). Test: the connect test in `main.rs`.

### `instrument_lock` [@ANCHOR: bench_mcp_main_instrument_lock]

Gives the lock that makes calls to one instrument run one at a time. A serial port or a scope socket has a single conversation, and the server runs
every tool call on its own thread, so without the lock two calls would interleave their commands.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` (borrowed) | The key. For an instrument it is the instance name. `call` also uses `hub:<hub id>` as a key to serialize the switching of one power-switching USB hub. |

Returns an `Arc<Mutex<()>>` that is the same object for the same name for the life of the process (a process-wide `static` map, created on first
use, entries never removed). The caller locks it and holds the guard for the whole call. A poisoned lock (a call that panicked while holding it) is
taken over (`into_inner`) rather than failing every later call. Lock order used by `call`: the instrument's own lock first, then the hub lock; no
other order exists in this file, so they cannot deadlock. Test: the lock test in `main.rs`.

### `with_device` [@ANCHOR: bench_mcp_main_device_arg]

Lets a tool call name its instrument with `device` as well as `instrument`: the instrument-instance name is called `device` in the multi-instance
config, and `instrument` in the older schemas. `call` runs it first on every call.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` (borrowed) | The tool arguments (a JSON object). |

Returns an owned copy of the arguments. Without `device` (or with `device: null`) the copy is unchanged. With `device`, the copy has `instrument`
set to it. Errors: `device` is not a string (`device must be the name of an instrument`); both are given and differ
(`device "a" and instrument "b" differ; give one`). Both given and equal is accepted. No I/O, no locking. Tests: the device-argument tests in
`main.rs`.

### `survey` [@ANCHOR: bench_mcp_main_survey]

Answers `bench_identify` and `bench_status` when no instrument is named and several could answer: it asks every one of them, so a single absent
instrument does not hide the rest.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` | All configured instances. |
| `tool` | `&ToolSpec` | The tool; only instances whose kind it applies to are asked. |
| `name` | `&str` | The tool name, passed on to `call`. |
| `args` | `&Value` | The arguments; copied once per instrument with `instrument` set to that instance. |

Returns `{"instruments": [row, ...]}`; each row has `name`, `kind` and `tier` (the Rust debug names, for example `Scope`, `ReadConfig`), `driver`,
`state` (`"ok"` or `"error"`), `result` (the tool's answer when ok, else null) and `error` (the message when not ok, else null). It never returns an
error itself. Side effects: it calls `call` for each instrument one after another, so each instrument's own lock is taken and its link opened in
turn; a dead scope costs up to the 37 s of waiting in `connect` before its row says `error`. Tests: the survey test in `main.rs`.

## Dispatching one tool call

### `call` [@ANCHOR: bench_mcp_main_call]

The one place where a tool call becomes work on an instrument. `BenchServer::call_tool` runs it on a blocking thread; `survey` and the tests call it
directly.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` (borrowed) | The configured instances. |
| `name` | `&str` | The tool name (`bench_...`). |
| `args` | `&Value` | The arguments, a JSON object. |

Returns `Ok(Value)` (the tool's JSON result, which the server prints indented) or `Err(String)` (shown to the caller as a tool error, never a crash).

Order of decisions: (1) `with_device`; (2) a manual tool (`bench_manual_*`) goes to `manual::call`, no instrument; (3) a tool whose name starts with
the prefix of a `MODULES` row goes to that module's `call`; (4) otherwise the name must be in `TOOLS` (`unknown tool <name>` if not); (5)
`bench_list` returns every instance with `name`, `driver` and `tier` and nothing else, no link opened; (6) `bench_identify` or `bench_status` with no
`instrument` and more than one fitting instance goes to `survey`; (7) `pick` chooses the instance; (8) the tier is checked, `inst.tier < tool.tier`
returning `<tool> needs tier <T>; "<name>" is configured at <T>`, so a tool above the instrument's tier is refused before anything is touched; (9)
the instrument's lock is taken (and, for a VNA with a hub, `bench_vna_power_cycle` and `bench_vna_generator_off` also take the lock `hub:<hub>`,
after it); (10) the call goes on by kind: `bench_vna_time_domain` reads a stored CSV and needs no unit; a NanoVNA goes to `nanovna::call`; a
VNWA checks its arguments and that the unit is present, installs the power-cycle hook and runs the named `vna` or `vna_tools` function; an SDR goes
to the `sdr` functions; a scope or supply opens the link with `connect` and runs the `scope`, `scope_more` or `psu` function. A name that the kind does
not support returns `<tool> does not apply to "<name>"`.

Blocking and side effects: it blocks while it waits for the lock and for the instrument, and the lock is held until the function returns. RF: the VNA
generator, sweep and meter tools put RF on the unit's TX port; this function gates them only by the `read_config` tier of their `TOOLS` rows (the
band check and the TX gate of the radio tools are not applied here: unknown whether the VNA code applies any, see the VNA story). The supply's
`bench_psu_output` is the `full`-tier row and has its own confirm flag (see `psu_output`). Poisoned locks are taken over. It does not panic on bad
arguments. Tests: several in `main.rs` cite it, among them the tier test, the device-argument tests and the psu tests.

### `psu_timer` [@ANCHOR: bench_mcp_main_psu_timer]

Gives the switch-off timer state of a supply. The supply has no output timer of its own, so after `bench_psu_output` switches the output on, this
server starts a thread that switches it off again; the state returned here is how that thread and later calls know whether it is still wanted.

| Argument | Type | Meaning |
|---|---|---|
| `name` | `&str` | The instrument name. |
| `lock` | `Arc<Mutex<()>>` (owned clone) | The instrument's lock from `instrument_lock`; the timer thread takes it before it touches the supply. |

Returns `psu::Timer { lock, generation }`. `generation` is an `Arc<AtomicU64>` that is the same counter for the same name for the life of the process
(a process-wide map; entries are never removed). `psu::output_on` and `psu::safe_state` add one to it, so an earlier pending switch-off sees that its
number is stale and does nothing: an explicit off, or a second on with a new duration, cancels the old timer. No I/O. A poisoned map lock is taken
over. Tests: the psu timer tests in `main.rs`, and `psu.rs`.

### `psu_output` [@ANCHOR: bench_mcp_main_psu_output]

The tool `bench_psu_output`: switch the supply output on or off. Turning on drives whatever is connected to the supply, so every argument is checked
before the supply is asked anything. Tier `full` (set in the `TOOLS` table).

| Argument | Type | Meaning |
|---|---|---|
| `inst` | `&Instrument` | The supply (name, serial path and baud are read). |
| `l` | `&mut Link` | The open link to the supply (borrowed mutably; one conversation). |
| `args` | `&Value` | `on` (bool, required), `for_s` (number, 1 to `max_on_s`, default `max_on_s`), `confirm_load_connected` (bool), `dry_run` (bool). `null` counts as absent for the two flags. |
| `lim` | `&psu::Limits` | `max_volts`, `max_amps`, `max_on_s` from the config. |
| `timer` | `&psu::Timer` | From `psu_timer`. |

Returns: `on: false` runs `psu::safe_state` (switch off and read back, up to three tries; error text says what to do by hand if it will not confirm
off), or with `dry_run` returns the plan `{"dry_run": true, "sent": [], "would": [...]}` without sending anything. `on: true`: `for_s` is checked
(`psu::on_seconds`); with `dry_run` it returns the plan from `psu::plan_on` (stored setpoints read, nothing switched); without
`confirm_load_connected: true` it is refused with a message that says the person at the bench must say the load is connected; otherwise
`psu::output_on` runs: the stored setpoints must be within `max_volts` and `max_amps`, `OUTP ON` is sent, a thread is armed to switch the output off
after `for_s` seconds on its own new link (it reopens the serial port with `Link::serial`), and the state with `off_after_s` is returned. Errors: `on`
missing or not a bool, a flag that is not a bool, `for_s` out of range, stored setpoints above the caps. Timing and locks: `call` holds the
instrument lock for the call; the timer thread takes the same lock when it fires. The timer does not survive the server: if the server dies with the
output on, `procedure::recover` and the operator's safe state are the means (see the procedure story). Not a transmitter, but it can drive a load.
Tests: the psu tests in `main.rs`.

## Listing the tools

### `all_tool_defs` [@ANCHOR: bench_mcp_main_all_tool_defs]

Every tool the server can offer, as listed for a bench in which each driver is configured at the top tier. No hardware is touched and no
`BENCH_MCP_CONFIG` is read. The manual's argument tables and the checks that every tool is documented read the schemas from here.

No arguments. Returns `Vec<Tool>`. It builds a config text from the instruments this file owns (scope on 127.0.0.1, supply on `/dev/null`, a VNWA, a
bladeRF, a LimeSDR) plus the `sample` entry of each `MODULES` row, parses it with `config::parse`, and returns `tool_defs` of the result. It panics
(`expect`) if that sample config does not parse: a new `MODULES` row with a bad `sample` fails every test that calls it, at once. `pub(crate)`.
Tests: the all-tool-defs tests in `main.rs`.

### `tool_defs` [@ANCHOR: bench_mcp_main_tool_defs]

The tool list the server returns to an MCP client for the instruments actually configured. This is how the server "does not list tools no
instrument may use".

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` | The configured instances. |

Returns `Vec<Tool>`: every `TOOLS` row for which some configured instrument has a kind the tool applies to and a tier at or above the tool's tier
(`bench_list` is always listed), each as a JSON-schema object (`properties` from the row's text, `required`) whose description is prefixed with the
tier in brackets (for example `[ReadConfig] ...`); then each module's own tools (`tool_defs` of each `MODULES` row), then the manual tools; each tool
gets the `device` property (`with_device_property`); finally `manual::with_pointers` is applied. It panics (`unwrap`) if a row's `props` text is not
JSON: a test checks them all. No I/O. Listing a tool does not allow it: `call` still checks the tier. Tests: the tool-defs tests in `main.rs`.

### `with_device_property` [@ANCHOR: bench_mcp_main_device_property]

Adds the `device` argument next to `instrument` in the schema of a tool, so a client can use either name.

| Argument | Type | Meaning |
|---|---|---|
| `tool` | `Tool` (owned) | A tool definition. |

Returns the tool, with a string property `device` ("the name of the instrument instance to use (same as instrument); required when several fit")
added to its `properties` when it has `instrument`, does not already have `device`, and its name does not start with `bench_manual_`. Otherwise it
is returned with an equivalent schema. No errors, no I/O. `pub(crate)`. Tests: the device-property test in `main.rs`.

## The server

### `BenchServer::call_tool` [@ANCHOR: bench_mcp_main_call_tool]

The MCP `tools/call` handler: it takes the request from the client, runs `call` and turns the answer into an MCP result.

| Argument | Type | Meaning |
|---|---|---|
| `request` | `CallToolRequestParams` (owned) | `name` and optional `arguments` (an object; missing means empty). |
| `_context` | `RequestContext<RoleServer>` | Unused. |

Returns `Ok(CallToolResponse)` always, unless the worker task panics or is cancelled, when it returns an MCP internal error `tool task: ...`. A tool
that fails is an `Ok` response whose result has the error flag set and the message as text; a tool that succeeds has the JSON result pretty-printed
as text. Asynchronous and threads: the instruments are blocking peers, so `call` runs inside `tokio::task::spawn_blocking`, which keeps the async
runtime threads free; several calls may be in flight at once, and per-instrument ordering is the lock inside `call`. The `insts` list is shared by
`Arc` and never changes after start.

### `main` [@ANCHOR: bench_mcp_main_serve]

The program entry point: starts the MCP server on standard input and output. It is a `tokio` async main.

Command line: two special first arguments run helper modes and exit, without a config and without serving MCP. `--usb-priv ...` (the flag of
`usb_hub::privileged`) is the root helper of the USB hub tools (`sudo -n bench_mcp --usb-priv <operation> ...`); `radioctl::guard::FLAG` is the
transmit watchdog of the radioctl tools, this program run again with the rig's `radioctld` address and a deadline. Both end the process with the
helper's exit status. Otherwise: `config::load()` (an error ends the program with it, before any hardware is touched), `procedure::recover` (runs of a
server that died while active are marked lost and their instruments put in a safe state), a task that waits for SIGTERM or SIGINT, then the
`BenchServer` serves on stdio until the client disconnects. On a signal the task runs `procedure::shutdown`, `radioctl::shutdown` and
`glasgow::shutdown` on a blocking thread (a keyed rig is released and the daemons this server started are stopped) and exits with status 143. When
the connection ends any other way, the same three shutdowns run before the program returns the connection's result. If the signal handlers cannot be
installed the task returns quietly and signals are then not handled (no message: odd, reported). Environment: `BENCH_MCP_CONFIG` (required),
`BENCH_MCP_OUT` (optional). Return: `Ok(())`, or the error of the config or of the transport.

## The config file (`config.rs`)

### `driver_names` [@ANCHOR: bench_mcp_config_driver_names]

The list of valid `driver` names, for error messages and tests. No arguments; returns a `Vec<&'static str>` of the first column of `DRIVER_KINDS`, in
that table's order (alphabetical: `atm6301`, `audio`, `bladerf`, `elad`, `example`, `glasgow`, `limesdr`, `nanovna`, `procedure`, `radioctl`,
`rotator`, `rtlsdr`, `rx888`, `siglent_sds`, `siglent_ssa`, `usb_hub`, `vnwa_open`). Used in the `unknown driver` error of `parse`; the checklist tests
require every one of them to appear in the README with an example entry. It cannot fail.

`DRIVER_KINDS` (the table itself, a `const` slice of `(&str, Kind)`) maps a driver name to the `Kind` it makes: `atm6301` Psu, `audio` Audio, `bladerf`,
`limesdr` and `rtlsdr` Sdr, `elad` Elad, `example` Example, `glasgow` Glasgow, `nanovna` and `vnwa_open` Vna, `procedure` Procedure, `radioctl`
Radioctl, `rotator` Rotator, `rx888` Rx888, `siglent_sds` Scope, `siglent_ssa` Sa, `usb_hub` UsbHub. It is kept alphabetical so two drivers added at
once touch different lines. The removed driver name `hamlib` is not in it (see `parse`).

### `tier_of` [@ANCHOR: bench_mcp_config_tier_of]

Turns the `tier` text of an instrument entry into the `Tier` enum, whose order is the permission order: `ReadOnly` (0) below `ReadConfig` (1) below
`Full` (2); `call` compares tiers with `<`.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `Option<&str>` | The `tier` value. |

Returns `Tier::ReadOnly` for `None` and for `"read_only"`, `ReadConfig` for `"read_config"`, `Full` for `"full"`. Any other text is
`Err("unknown tier \"x\" (read_only, read_config, full)")`; `parse` adds the instrument name in front. A `tier` that is present but not a JSON string
is read as absent by the caller and so becomes `read_only` (the least permission). The default when omitted is the least permission, so an entry without a
tier can only read. No I/O. Tests: the tier tests in `config.rs`.

### `connect_waits` [@ANCHOR: bench_mcp_config_connect_waits]

Reads the `connect_waits_s` key of a scope entry: the pauses, in seconds, that `connect` makes after each failed connection attempt before it reports a NO-GO (see `bench_mcp_link_tcp_checked`). `parse` calls it for every entry; only scopes use the value.

| Entry key | Type, range | Meaning |
|---|---|---|
| `connect_waits_s` | array of 0 to 8 numbers, each 0 to 3600 | One pause per retry; the scope is tried once more than there are pauses. Omitted: 2, 5, 10, 20 (37 s in all). `[]` means one attempt and no pause. |

Errors: a value that is not such an array is refused with `connect_waits_s must be up to eight numbers from 0 to 3600 (seconds)`, with the instrument name in front. It exists for the stale-session experiment: setting `[300, 600, 1800]` makes one tool call wait the scope out for 5, 10 and 30 minutes and the NO-GO text records each attempt, so the idle timeout of the scope's SCPI server can be measured without a code change. Pure: no I/O. Tests: `connect_waits_default_to_37_seconds_and_are_bounded` in `config.rs` and `a_scope_holding_a_dead_session_is_waited_out_for_as_long_as_connect_waits_s_says` in `main.rs` (a fake scope that resets new connections for 0.7 s).

### `vna_config` [@ANCHOR: bench_mcp_config_vna]

Reads the VNA settings of an instrument entry into `vnwa_open::sweep::Config`. `parse` calls it for every entry (not only VNAs; for the other
kinds the values are read and then unused).

| Entry key | Type, range | Meaning |
|---|---|---|
| `codec_s11`, `codec_s21` | string | Parts of the ALSA ids of the two codecs; found from the USB tree when omitted. |
| `if_hz` | number | Intermediate frequency, a divisor of 48 kHz (12000 etc.); default from `vnwa_open`. |
| `point_ms` | number | Time per point in ms; default from `vnwa_open`. |
| `hub`, `hub_port` | string, integer 0 to 255 | The power-switching USB hub and port for cold resets (`uhubctl -l <hub> -p <port>`); both or neither. |
| `retries` | integer | Retries of a failing point; larger than `u32` is refused. |
| `settle_ms`, `window_ms` | number | Optional; omitted means "unset" (`None`), not the default. |
| `rf_level` | integer 0 to 16383 | The RF level; larger than `u16` is refused. |

Anything omitted keeps the `vnwa_open::sweep::Config::default()`. A value of the wrong JSON type is silently ignored (treated as omitted). The result
is checked with `vnwa_open::sweep::check`. Errors: `hub` without `hub_port` or the reverse (`hub and hub_port must be given together`), `hub_port`
above 255 (`hub_port must be 1 to 255`), `retries is too large`, `rf_level is above the 14-bit maximum 16383` (this message is shown for any value that
does not fit in 16 bits; a value from 16384 to 65535 is given to `check`, whose behaviour is in the `vnwa_open` crate: unknown here), and whatever
`check` returns. Pure: no I/O. Tests: the VNA config tests in `config.rs`; the README line on hub and retries.

### `sdr_config` [@ANCHOR: bench_mcp_config_sdr]

Reads the SDR settings of an instrument entry into `sdr::Config`.

| Argument | Type | Meaning |
|---|---|---|
| `driver` | `&str` | The driver name of the entry. |
| `c` | `&Value` | The entry's JSON object. |

The result has `driver`, `serial` (read only for `bladerf`, `limesdr` and `rtlsdr`, else `None`), `xb200` and `direct_samp` (strings). For the three SDR drivers
it runs `sdr::check_config` and returns its error. For any other driver, `xb200` is refused (`xb200 applies to the bladeRF only`) and so is
`direct_samp` (`direct_samp applies to the RTL-SDR only`). The meaning of `xb200` and `direct_samp` values and the rule for which driver may use them are
in `sdr::check_config` (the SDR story). Pure. Tests: the SDR config test in `config.rs`.

### `flatten_entries` [@ANCHOR: bench_mcp_config_flatten]

Turns the `instruments` object into a flat list of named instances, so one config can hold several instruments of one kind.

| Argument | Type | Meaning |
|---|---|---|
| `map` | `&serde_json::Map` | The `instruments` object. |

Returns a `Vec<(&String, &Value)>` of (instance name, entry), in file order. An entry with a `driver` key is one instance named by its key (the old
form). An entry without `driver` that is a non-empty object all of whose members have a `driver` is a group, such as `"vnas": {"vnwa": {...},
"nano": {...}}`: each member is an instance named by its own key and the group's key is only a label. Anything else is passed through as one entry
(and `parse` then reports `no driver`). Error: two instances with the same name anywhere in the config (`config: two instances are named "x"; every
instance name must be unique`). Pure; names are compared exactly. Tests: the multi-instance config test in `main.rs` cites it.

### `reject_shared_claims` [@ANCHOR: bench_mcp_config_shared_claims]

Refuses a config in which two instances claim one physical thing, since two conversations on one device would corrupt each other.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` | The parsed instances. |

Returns `Ok(())` or the first clash as `<a> and <b> both claim <what>; each device belongs to one instance`. A clash is: the same serial port (a supply's
`serial` or a NanoVNA's `serial`, compared after resolving symlinks with `canonicalize`; a path that cannot be resolved is compared as written), the same scope
host and port, two `vnwa_open` instances (the driver opens the one USB unit), or two VNAs with the same hub and hub port. Not checked here: other kinds
(SDRs, ELAD and so on) do not take part, and a `serial` shared between a supply and another driver's own port setting is not compared. Reads the file
system (symlink resolution) only. Tests: the shared-claims test in `main.rs`.

### `parse` [@ANCHOR: bench_mcp_config]

Parses the config text into the list of instances. Every check that can be made without touching hardware is made here, so a bad file stops the server
before it starts.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | The JSON text. |

Returns `Vec<Instrument>` (see the `Instrument` structure below). Errors, as text that names the instance where it can: invalid JSON, no
`instruments` object, a duplicate name, an entry with no `driver`, the removed `hamlib` driver (the message tells how to use `radioctl` instead), an unknown
driver (the message lists the valid ones), a bad `tier`, a bad module setting (each module's `parse_config` is called only for its own driver, so a bad
entry for one driver cannot stop another from loading), a scope, spectrum analyzer or example instrument without `host`, a supply without `serial`,
`max_volts` or `max_amps` not a finite number above 0, `max_on_s` not from 1 to 86400, a shared device (`reject_shared_claims`), and an empty list
(`config: no instruments`).

Defaults, applied when a key is absent or has the wrong JSON type: `port` 5025 (a larger value wraps: the code casts, not checks), `baud` 115200,
`max_volts` 5.5, `max_amps` 1.0, `max_on_s` 600. `tier` is `read_only` when absent. The USB hub settings are parsed for every entry
(`usb_hub::parse_config`); the others only for their own driver. Pure apart from the symlink resolution in `reject_shared_claims`. Tests: the config tests
in `config.rs` and the tests in `main.rs` that build instances with it.

### `load` [@ANCHOR: bench_mcp_config_load]

Reads the config from the file named by the environment variable `BENCH_MCP_CONFIG` and returns the instances. `main` calls it once at start.

No arguments. Steps: read the variable (error `BENCH_MCP_CONFIG is not set`), read the file (error `<path>: <os error>`), parse the text a first time as
plain JSON, give the top-level `reserved_serial` list to `serial_guard::set_reserved` (via `parse_reserved`, which errors on a bad list), then
`parse`. The reserved list is stored for the whole process before any device is opened, and it is set only once (a second call changes nothing), so
`load` is for the server's start, not for reloading. The file is read once; a change to it needs a restart. Returns the instances or the error text, which
`main` prints and exits on. Tests: the load test in `config.rs`.

### `Instrument`, `Tier` and `Kind` (data structures of `config.rs`; no anchor of their own)

`Instrument` is the parsed form of one config entry and what every function above receives. Fields: `name` (instance name, unique), `kind` (`Kind`, from
the driver), `driver` (the text), `tier` (`Tier`), `host` (optional, scope and spectrum analyzer and example), `port` (default 5025), `serial` (optional,
supply), `baud` (default 115200; the link accepts 9600, 19200, 38400, 57600 and 115200), `max_volts`, `max_amps` (supply caps, defaults 5.5 V and 1 A),
`max_on_s` (the longest the output stays on, 1 to 86400 s, default 600), then one `Config` per module (`vna`, `nanovna`, `sdr`, `usb`, `audio`, `radioctl`,
`rotator`, `elad`, `example`, `sa`, `procedure`, `rx888`, `glasgow`), each its default unless the entry's driver is that module. It is built by `parse`
at start, never changed afterwards and shared by `Arc` across threads. It is not serialized back anywhere.

## The SCPI link (`link.rs`)

### `Serial::open` [@ANCHOR: bench_mcp_link_serial_open]

Opens a serial device for the supply and a NanoVNA: raw mode, 8 data bits, no parity, 1 stop bit, no flow control.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | The device, normally a `/dev/serial/by-id/...` path. |
| `baud` | `u32` | One of 9600, 19200, 38400, 57600, 115200. |

Returns `Serial` with a 5 s read timeout. Order: first the reserved check, `serial_guard::check_reserved_open(path)` (a device on the config's
`reserved_serial` list is refused and nothing is opened); then the baud (`unsupported baud rate N`); then `open` read-write with `O_NOCTTY` and
`O_NONBLOCK` (`open <path>: <os error>`); then an exclusive `flock` tried every 50 ms for up to 10 s (`<path> is in use by another process`), so another
bench_mcp process or a test cannot talk on the port at the same time (the lock goes with the file descriptor and ends when the `Serial` is dropped);
then `tcgetattr` (`<path>: not a serial port`), raw mode with `CLOCAL | CREAD`, the speed, `tcsetattr`, and a flush of both queues. Blocking: up to 10 s.
It contains `unsafe` calls to libc; they only use the descriptor of the open file. `Link::serial` adds the supply-name check before this call. Tests:
the serial-open tests in `link.rs` and the guard tests in `serial_guard.rs`.

### `Serial::read` [@ANCHOR: bench_mcp_link_serial_read]

The `Read` implementation of `Serial`: a read that gives up after the port's timeout.

| Argument | Type | Meaning |
|---|---|---|
| `buf` | `&mut [u8]` | Where to put the bytes. |

It waits with `poll` for data for the timeout (set by `Port::set_timeout`; 5 s after `open`, 10 s once a `Link` owns it, the longest value is capped at
`i32::MAX` ms) and then reads what is there. Returns the number of bytes (after `poll` reported data, so normally at least one). Errors: `poll` failed (the OS error), no data within the timeout (`ErrorKind::TimedOut`, which `Link` turns into `read: ...` text, and which
`raw_until_idle` treats as the end of a stream). It reads no more than one `read` call's worth; callers that need an exact count use `read_exact`.
Tests: the serial tests in `link.rs`.

### `Link::tcp` [@ANCHOR: bench_mcp_link_tcp]

Opens a SCPI link to an instrument over TCP (normally port 5025).

| Argument | Type | Meaning |
|---|---|---|
| `host` | `&str` | Host name or address. |
| `port` | `u16` | TCP port. |

Returns a `Link` with terminator `\n` and a 10 s read timeout. It resolves the name (the first address is used; an IPv6 address given first is not
skipped), connects with a 5 s timeout and sets `TCP_NODELAY`. Errors: `<host>:<port>: <resolve error>`, `no address`, `connect <host>:<port>: <error>`.
It blocks up to 5 s and sends nothing to the instrument. `tcp_checked` is the version that also asks `*IDN?`. Tests: the loopback instrument tests in
`link.rs`.

### `Link::tcp_checked` [@ANCHOR: bench_mcp_link_tcp_checked]

A scope link that is proven with `*IDN?` and that waits out a stale session. The Siglent scope serves one SCPI client at a time: after a host power
loss it kept the dead session, accepted TCP on 5025 and reset every connection on its first query (soak of 2026-10-29; the cause is not confirmed).

| Argument | Type | Meaning |
|---|---|---|
| `host`, `port` | `&str`, `u16` | As `tcp`. |
| `waits` | `&[Duration]` | The pause after each failed attempt; the number of attempts is `waits.len() + 1`. `connect` uses 2, 5, 10, 20 s. |
| `diagnose` | `&dyn Fn(&str, u16) -> String` | Called once, after the last failure, for the state text of the error. `connect` passes `scope_state`. |

Each attempt is `tcp` followed by `query("*IDN?")`; it succeeds when the reply is not empty. A failed attempt (connect error, reset, empty or no reply)
drops the socket, sleeps the next wait and tries again. Returns the first working `Link`. After the last attempt: `Err("NO-GO: <host>:<port> did not
answer *IDN? in N attempts over S s (attempt 1: ...; ...). State: <diagnose>. Recommended action: power-cycle the scope ...")`. Blocking: the sum of
the waits plus each attempt's time (a query that gets no reply waits out the 10 s read timeout). The `*IDN?` answer is thrown away. Tests: the
checked-link tests in `link.rs`.

### `Link::scope_state` [@ANCHOR: bench_mcp_link_scope_state]

The scope's condition for a failure report: whether it answers a ping and whether TCP connects to each of some ports. It reads the network only and sends
nothing to the instrument's SCPI service.

| Argument | Type | Meaning |
|---|---|---|
| `host` | `&str` | Host name or address. |
| `ports` | `&[u16]` | The TCP ports to try (`connect` passes the SCPI port and 80). |

Runs the `ping` program once (`ping -c 1 -W 1 <host>`, output discarded) and reports `ping answers`, `ping does not answer` or `ping unavailable`
(the program could not be run); then, for each port, a connect with a 2 s timeout: `port N accepts`, `port N: <error>` or `port N: no address`. Returns the
parts joined with `, `. It cannot fail. A subprocess is started and a connection is made and closed at once; whether that short connection disturbs a scope that serves one client at a time is unknown (the code does not say). Worst case about 1 s plus 2 s per port. Tests: the state tests in `link.rs`.

### `Link::new` [@ANCHOR: bench_mcp_link_new]

Builds a `Link` around an open byte stream. It is private; `tcp` and `serial` are the entries.

| Argument | Type | Meaning |
|---|---|---|
| `p` | `Box<dyn Port>` (owned) | The stream: a `TcpStream` or a `Serial`. |
| `eol` | `&'static str` | The command terminator: `"\n"` for the scope, `"\r\n"` for the serial supply. |

Sets the stream's timeout to 10 s, which is also remembered as the default that `reset_timeout` restores after a short wait. Returns the `Link`.
The `Link` is not `Sync`; one thread uses it at a time, which `call` ensures with the instrument lock. It owns the stream: dropping the `Link` closes the
socket or the port and releases the port's file lock.

### `Link::byte` [@ANCHOR: bench_mcp_link_byte]

Reads exactly one byte from the stream; the base of every reply reader (`line`, `block`).

No arguments besides `self`. Returns the byte. Error: `read: <error>`, which is a timeout (`timed out`, after the current timeout), a closed
connection (`failed to fill whole buffer`) or an I/O error. Blocks until a byte arrives or the timeout ends. Private.

### `Link::line` [@ANCHOR: bench_mcp_link_line]

Reads one reply line.

No arguments besides `self`. It reads bytes until `\n`, so `\r\n` and bare `\n` terminators both work, converts them as UTF-8 (invalid bytes are replaced,
not an error), trims white space from both ends and returns the text (an empty line returns an empty string). Errors: those of `byte`. A partial line
is lost on error. There is no length limit: an instrument that sends no newline holds the call until its timeout, and one that sends a lot is read into
memory. `query` is `send` then `line`. Tests: the loopback tests in `link.rs`.

### `Link::number` [@ANCHOR: bench_mcp_link_number]

Sends a query and returns its reply as a number, for both instrument header settings.

| Argument | Type | Meaning |
|---|---|---|
| `cmd` | `&str` | The query, for example `VOLT?`. |

Takes the value part of the reply (after the last space or comma, `Link::value`, so `C1:VDIV 1.00E+00V`, `1.00E+00` and `PKPK,4.04E+00` all work),
drops anything after the last digit or decimal point (units such as `V`, `Hz`, `Sa/s`, `MHz`) and parses an `f64`. Returns the number. Errors: those of
`send` and `line`, and `<cmd>: not a number: "<reply>"` (for example `****` from a scope that could not measure). The multiplier in `MHz` or `kHz` is NOT
applied: `10MHz` returns 10, so the caller must know the unit. Tests: the number tests in `link.rs`.

### `Link::block` [@ANCHOR: bench_mcp_link_block]

Sends a command and reads its binary reply in the IEEE 488.2 definite-length format `#<n><length digits><data>`.

| Argument | Type | Meaning |
|---|---|---|
| `cmd` | `&str` | The command; the terminator is added. |

Skips any bytes before the `#` (a text echo or header), reads the digit count `n` (one digit), then `n` digits of length, then exactly that many data bytes
(read with the stream's timeout). Afterwards it waits 200 ms for the trailing terminator bytes (up to two) and discards what comes, then restores the
default timeout. Returns the payload. Errors: `bad block header` (the byte after `#` is not a digit), `bad block length` (also for `#0`, the
indefinite-length form, which this function does not support), `block: <error>` when the data is short, and the errors of `send` and `byte`. A very
large length asks for that much memory before reading. Tests: the block tests in `link.rs`.

### `Link::send_block` [@ANCHOR: bench_mcp_link_send_block]

Sends a command whose argument is binary data in the IEEE 488.2 definite-length format, the counterpart of `Link::block`. The scope's `PNSU` setup restore is the user: its data may hold any byte, including newlines, so it cannot go through `send`.

| Argument | Type | Meaning |
|---|---|---|
| `head` | `&str` | The command text before the block, for example `PNSU ` (the trailing space is the caller's). |
| `data` | `&[u8]` | The payload, at most 999 999 999 bytes. |

Writes `head`, then `#9` and the length as nine decimal digits, then the data unchanged, then the link's terminator (`\n` for the scope), in one write. Returns `Ok(())` or `send: <error>`. It reads nothing: a command that answers is read by the caller afterwards. Tests: `a_binary_block_goes_out_with_its_length_and_every_byte_intact` in `link.rs`, which checks the bytes a loopback server receives, newline and 0xFF included.

### `Link::raw_until_idle` [@ANCHOR: bench_mcp_link_raw_until_idle]

Sends a command and reads a raw stream that has no header, such as the scope screenshot command `SCDP`, until the instrument goes quiet.

| Argument | Type | Meaning |
|---|---|---|
| `cmd` | `&str` | The command. |
| `idle_ms` | `u64` | Milliseconds of silence that end the read, once some data has arrived. |
| `overall_s` | `u64` | Whole seconds after which it stops regardless. |

Sets the read timeout to `idle_ms`, reads in chunks of up to 64 KiB and appends them. Stops when a read returns zero bytes, when a timeout or would-block
happens after at least one byte arrived, or when `overall_s` seconds have passed (checked between reads, so it can end up to `idle_ms` late). A timeout
before the first byte is not an end: it keeps waiting until `overall_s`. Returns all the bytes (possibly empty). Error: `raw read: <error>` for any
other error. Always restores the default timeout before returning when it returns normally (an error return does not restore it; the `Link` is then
normally dropped). Tests: the raw-stream test in `link.rs`.
