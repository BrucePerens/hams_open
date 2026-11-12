# The Spectrum Analyzer Driver's Core: Config, Tool List, Shared Helpers, Status and Dispatch

As an operator with a Siglent SSA3000X spectrum analyzer (with the tracking generator option) on the bench LAN, I want the MCP server to list only the
spectrum analyzer tools my configured tier allows, to refuse an address that is not an SSA3000X before it sends anything else, and to give a model clear,
step-by-step errors when the instrument is silent. This story covers `daemons/bench_mcp/src/sa/mod.rs`: the config entry, the fixed limits, the tool
table, the argument and reply helpers shared with `sweep.rs` and `tg.rs`, the identity check, `bench_sa_status`, and the dispatcher. The sweep, marker and
screenshot tools (`sweep.rs`) and the tracking generator tools (`tg.rs`) have their own stories. Nothing here transmits on the air: the tracking generator
is a bench signal source (0 dBm at most) and is handled like any other setting at tier `read_config`; there is no TX gate or band check.

## Configuration and limits

### `bench_mcp_sa_limits` [@ANCHOR: bench_mcp_sa_limits]

The fixed limits that bound what the driver does, declared at the top of `src/sa/mod.rs`. They are compile-time constants, never read from the config file,
and shared read-only by every call. (`POINTS` and `MAX_SWEEP_S` live in `sweep.rs` and are documented with the sweep tools.)

| Constant | Type, value | Meaning |
|---|---|---|
| `MODEL_PREFIX` | `&str`, `"SSA3"` | The model field of `*IDN?` must start with this (the SSA3000X family). Anything else is another device at that address, and the driver stops before sending it any other command. |
| `REPLY_TIMEOUT_S` | `u64`, 5 seconds | Read timeout of an ordinary exchange, passed to `ask`. |
| `READ_TRIES` | `u32`, 3 | Total number of times a query is tried. Commands that change the instrument are never repeated blind. |
| `OFF_TRIES` | `u32`, 3 | Attempts to switch the tracking generator off and read it back as off (used by `tg.rs`; the SAFE STATE NOT CONFIRMED path follows when all fail). |
| `TG_RATED_MIN_DBM` | `pub(crate) f64`, -20.0 dBm | Lowest tracking generator output level of the unit (the unit's guide, section 3.9.2). |
| `TG_RATED_MAX_DBM` | `pub(crate) f64`, 0.0 dBm | Highest rated level; also the default of `Config.max_tg_dbm`. |

Invariants: `TG_RATED_MIN_DBM < TG_RATED_MAX_DBM`; the rated range is what `parse_config` accepts for `max_tg_dbm` and what the tools document for
`tg_level_dbm`. No locking, no I/O. Tests citing it: `sa::tests::config_takes_the_tg_cap_with_a_default_and_refuses_bad_ones`. See also `bench_mcp_sa_config`, `bench_mcp_sa_ask`.

### `bench_mcp_sa_config` [@ANCHOR: bench_mcp_sa_config]

The `siglent_ssa` settings of an instrument entry (`struct Config`, `#[derive(Clone, Debug, PartialEq)]`), in `src/sa/mod.rs`. It is held in
`Instrument.sa` (`src/config.rs`) for the life of the process.

| Field | Type, unit | Meaning |
|---|---|---|
| `max_tg_dbm` | `f64`, dBm | Optional owner cap on the tracking generator level. Valid -20.0 to 0.0 (the unit's own rating). Default 0.0 (`TG_RATED_MAX_DBM`), meaning no extra cap. Set by `parse_config` from the config key `max_tg_dbm`; read by the tracking generator tools in `tg.rs` (`level_arg`, `switch_on`) and by `status`, which reports it. |

Invariants: finite and inside the rating whenever it came from `parse_config`; `Default` is the rating's top. Created when the config file is read, immutable
afterwards, shared read-only by all tool calls, so there is nothing to lock. Serialized form: the optional key `max_tg_dbm` in the instrument's JSON entry,
for example `{"driver":"siglent_ssa","tier":"read_config","host":"192.168.10.42","port":5025,"max_tg_dbm":-10}`. There is no version field; an absent key
means the default. Instruments of other drivers carry `Config::default()` unused. Tests: `sa::tests::config_takes_the_tg_cap_with_a_default_and_refuses_bad_ones`.

### `bench_mcp_sa_parse_config` [@ANCHOR: bench_mcp_sa_parse_config]

Reads the `siglent_ssa` extras from one instrument's JSON entry. Called by `config::parse` (`src/config.rs`) for an entry whose driver is `siglent_ssa`;
the caller prefixes any error with the instrument's name.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&Value`, borrowed | The instrument's whole JSON object from `BENCH_MCP_CONFIG`; only `max_tg_dbm` is read, other keys are ignored here. |

Returns `Ok(Config)`: the default when `max_tg_dbm` is absent, else the given cap. Errors (strings, which make the whole config file fail to load, so the server
does not start with a bad limit): `max_tg_dbm must be a number` when the value is not numeric (a string, or `null`);
`max_tg_dbm <x> must be -20 to 0 dBm (the tracking generator's rating)` when it is non-finite or outside the rated range (inclusive at both ends). No panics,
no I/O, no locks. Example: `{"max_tg_dbm": -6}` gives `Config{max_tg_dbm:-6.0}`; `{"max_tg_dbm": 3}` is refused. Tests: `sa::tests::config_takes_the_tg_cap_with_a_default_and_refuses_bad_ones`.

## The tool table

### `bench_mcp_sa_spec` [@ANCHOR: bench_mcp_sa_spec]

One tool's static description (`struct Spec`), the row type of `SPECS`. It carries what the MCP `tools/list` answer is built from and what the dispatcher checks.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | Tool name, unique, starting `bench_sa_`; matched exactly by `call_with`. |
| `tier` | `Tier` (`ReadOnly` < `ReadConfig` < `Full`) | The least tier an instrument must be configured at for the tool to be listed and callable. |
| `desc` | `&'static str` | The description shown to the model; `tool_defs` prefixes it with the tier in brackets. |
| `props` | `&'static str` | The JSON-schema `properties` object, as JSON text built with the `props!` and `sweep_props!` macros; parsed on every `tool_defs` call, so it must be valid JSON. |
| `required` | `&'static [&'static str]` | Names of required arguments (only `bench_sa_tg_level` has any: `tg_level_dbm`). |

Invariants: all fields are `'static`, built at compile time; no mutation, so no concurrency concern. Not serialized itself; its content is emitted as the
tool's JSON schema. Tests: `sa::tests::tools_are_listed_by_tier_and_refused_below_it_before_any_connection`.

### `bench_mcp_sa_specs` [@ANCHOR: bench_mcp_sa_specs]

`SPECS`, the table of every spectrum analyzer tool (`pub const SPECS: &[Spec]`). It is the single list that `tool_defs` (what is advertised) and `call_with`
(what is accepted) both start from.

| Tool | Tier | What it does (see the `sweep.rs` and `tg.rs` stories) |
|---|---|---|
| `bench_sa_status` | `read_only` | Identity, options, tracking generator and sweep settings; changes nothing. |
| `bench_sa_sweep` | `read_config` | Set range and options, one sweep, CSV of the 751-point trace, peaks. |
| `bench_sa_marker` | `read_config` | Marker 1 to 4 search or place, read frequency and level. |
| `bench_sa_screenshot` | `read_only` | Screen as a BMP in `BENCH_MCP_OUT`. |
| `bench_sa_tg_on` | `read_config` | Set level then switch the tracking generator on. |
| `bench_sa_tg_off` | `read_config` | Switch it off and prove it (up to `OFF_TRIES`). |
| `bench_sa_tg_level` | `read_config` | Set the level without changing on or off. |
| `bench_sa_tg_normalize` | `read_config` | Through reference sweep stored by name. |
| `bench_sa_tg_s21` | `read_config` | Scalar S21 sweep, optionally against a reference. |

Adding a tool takes four things together: a new entry here, a match arm in `call_with`, a page under `manual/tools/<name>.md`, and a test; forgetting the match
arm yields the `does not apply` error at call time. Order is the listing order. Immutable constant; no locking. Tests: `sa::tests::tools_are_listed_by_tier_and_refused_below_it_before_any_connection`.

### `bench_mcp_sa_tool_defs` [@ANCHOR: bench_mcp_sa_tool_defs]

Builds the MCP tool list for the spectrum analyzer driver. Registered in `main.rs` as the driver's `tool_defs` and called when a client asks `tools/list`.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | All configured instruments of every kind; only those with `kind == Kind::Sa` matter. |

Returns `Vec<Tool>`: one per `SPECS` entry for which some spectrum analyzer is configured at a tier greater than or equal to the entry's tier (a `read_only`
analyzer therefore lists only `bench_sa_status` and `bench_sa_screenshot`). Each tool's description is `[<Tier>] <desc>` and its input schema is
`{"type":"object","properties":<props>,"required":<required>}`. With no spectrum analyzer configured, the list is empty. Panics: `serde_json::from_str(...).unwrap()` on
a `props` string that is not valid JSON (a programming error, not reachable from config). Pure apart from allocation: no I/O, no locks.
Tests: `sa::tests::tools_are_listed_by_tier_and_refused_below_it_before_any_connection`. See also `bench_mcp_sa_specs`, `bench_mcp_sa_call_with` (which re-checks the tier at call time).

## Argument and reply helpers

### `bench_mcp_sa_instrument` [@ANCHOR: bench_mcp_sa_instrument]

Chooses which configured spectrum analyzer a call is for. Private; called first by `call_with`.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | All configured instruments; filtered to `Kind::Sa`. |
| `args` | `&Value`, borrowed | The tool arguments; the optional string `instrument` names the analyzer. |

Returns `Ok(&Instrument)` borrowed from `insts`. If `instrument` is a string it must match an analyzer's name exactly. A non-string `instrument` is ignored as
if absent. Without a name, exactly one analyzer is chosen automatically. Errors: `no spectrum analyzer named "<n>"`; `no spectrum analyzer is configured`;
`name the instrument: a, b` when several exist and none is named. No I/O, no locks. Example: with analyzers `sa1` and `sa2`, `{}` fails listing both and
`{"instrument":"sa2"}` selects `sa2`. Tests: the tier test above and `status_reads_the_recorded_session_and_a_stranger_is_sent_nothing`.

### `bench_mcp_sa_flag` [@ANCHOR: bench_mcp_sa_flag]

Reads an optional boolean tool argument strictly. `pub(crate)`, used by `sweep.rs` and `tg.rs` for flags such as `tg_off_after`, `leave_tg_on` and `continuous_after`.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The tool arguments object. |
| `key` | `&str` | The argument name. |
| `default` | `bool` | The value when the key is absent or `null`. |

Returns `Ok(bool)`: the default for absent or JSON `null`, else the value if it is a JSON boolean. Any other type (a string `"true"`, a number) is an error
`<key> must be true or false`, so the model's typo is not silently read as the default. Pure, no I/O. Tests: `sa::tests::argument_helpers_refuse_wrong_types_and_ranges_and_models_give_their_top_frequency`.

### `bench_mcp_sa_number` [@ANCHOR: bench_mcp_sa_number]

Reads an optional numeric tool argument with a range check. `pub(crate)`, used for every numeric argument of the sweep, marker and tracking generator tools.

| Argument | Type, unit | Meaning |
|---|---|---|
| `args` | `&Value` | The tool arguments. |
| `key` | `&str` | The argument name. |
| `lo`, `hi` | `f64`, the argument's own unit | Inclusive limits. |
| `why` | `&str` | Where the limit comes from (for example the unit's rating), quoted in the error. |

Returns `Ok(None)` when the key is absent or `null`, `Ok(Some(x))` for a finite number inside `lo..=hi`. Errors: `<key> must be a number` for a non-numeric value;
`<key> <x> is outside <lo>..=<hi> (<why>)` for out of range, and also for NaN or infinity (not representable in JSON, but a check against a later computed value
costs nothing). Integers are accepted and converted; no rounding is done here. Pure, no I/O. Tests: `sa::tests::argument_helpers_refuse_wrong_types_and_ranges_and_models_give_their_top_frequency`.

### `bench_mcp_sa_next_step` [@ANCHOR: bench_mcp_sa_next_step]

Builds the text that ends every I/O error of this driver, so the model is told what to do next. Private; used by `ask`, `ask_f64`, `on_off`.

| Argument | Type | Meaning |
|---|---|---|
| `what` | `&str` | What went wrong, without a trailing period. |

Returns `String`: `<what>. Next: check the LAN cable and that the analyzer is on and at the configured host and port (the raw SCPI socket is port 5025), then call
bench_sa_status. If it stays silent, power-cycle the analyzer. Do not use the USB connection: its replies are cut short.` Cannot fail, no I/O. Used as an error, not a
success value. Related: the `SAFE STATE NOT CONFIRMED` text in `tg.rs`, which has its own instructions.

### `bench_mcp_sa_ask` [@ANCHOR: bench_mcp_sa_ask]

Asks the analyzer a query, retrying a silent or empty reply. `pub(crate)`; the basis of every read in `mod.rs`, `sweep.rs` and `tg.rs`.

| Argument | Type, unit | Meaning |
|---|---|---|
| `l` | `&mut Link` | The open SCPI link (one client at a time; the caller owns it for the whole tool call). |
| `cmd` | `&str` | The query, for example `*IDN?`; must be a read. |
| `tries` | `u32` | Total attempts (usually `READ_TRIES`, 3). With 0 nothing is sent and the error is returned. |
| `secs` | `u64`, seconds | Reply timeout set on the link before each attempt (usually `REPLY_TIMEOUT_S`, 5). |

Returns the first non-empty reply, trimmed by `Link::query`. An empty reply or an I/O error (timeout, reset) counts as a failed try. Error after the last try:
`<cmd>: no usable reply after <tries> tries of <secs> s (<last cause>)` followed by the `next_step` advice. Blocking: it can take up to `tries x secs` seconds, and
it changes the link's timeout (left set at `secs`). It is only for reads: repeating a query changes nothing on the instrument, so commands that change settings
go through `Link::send` and are not repeated. No locks beyond the caller's exclusive hold of the link. Tests: `sa::tests::a_silent_analyzer_is_retried_a_bounded_number_of_times_and_a_garbage_number_is_an_error`.

### `bench_mcp_sa_ask_f64` [@ANCHOR: bench_mcp_sa_ask_f64]

A query whose reply must be one finite number. `pub(crate)`, used for nearly every setting read.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | The open link. |
| `cmd` | `&str` | The query. |

Calls `ask` with `READ_TRIES` and `REPLY_TIMEOUT_S`, then parses the trimmed reply as `f64`. Returns the value. Errors: everything `ask` returns, and
`<cmd>: the reply "<r>" is not a finite number` (plus the `next_step` advice) for text, NaN or infinity. Units are not stripped: the unit replies a bare number
(Hz, dBm, seconds) to these queries. Blocks up to 15 seconds at worst. Tests: `sa::tests::a_silent_analyzer_is_retried_a_bounded_number_of_times_and_a_garbage_number_is_an_error`.

### `bench_mcp_sa_on_off` [@ANCHOR: bench_mcp_sa_on_off]

Decodes a boolean reply strictly. `pub(crate)`, used for the continuous sweep flag and the tracking generator state.

| Argument | Type | Meaning |
|---|---|---|
| `cmd` | `&str` | The query that was asked (only for the error text). |
| `reply` | `&str` | The reply text. |

Returns `Ok(true)` for `ON`, `1` or `1.000000`, `Ok(false)` for `OFF`, `0` or `0.000000` (case-insensitive, surrounding whitespace ignored); the decimal forms
are what the unit sometimes sends. Any other word is `Err(<cmd>: the reply "<reply>" is neither on nor off. Next: ...)`, never a guess: the safety checks of the
tracking generator rely on a reply that cannot be read being a fault, not "off". Pure, no I/O. Tests: `sa::tests::argument_helpers_refuse_wrong_types_and_ranges_and_models_give_their_top_frequency`.

### `bench_mcp_sa_model_max_hz` [@ANCHOR: bench_mcp_sa_model_max_hz]

Derives the analyzer's top frequency from its model name, so range checks are made against the actual unit. `pub(crate)`; called by `open`; the result travels in `Ident.max_hz` to the sweep argument checks.

| Argument | Type | Meaning |
|---|---|---|
| `model` | `&str` | The model field of `*IDN?`, for example `SSA3021X`. |

Returns `Some(hz)`: the two digits after `SSA30` times 100 MHz, so `SSA3021X` gives 2.1e9, `SSA3032X` 3.2e9, `SSA3015X` 1.5e9. Returns `None` when the name lacks the
`SSA30` prefix, has fewer than two characters after it, they are not digits, or they are `00`. The suffix letters are not examined. Pure, no I/O. Tests:
`sa::tests::argument_helpers_refuse_wrong_types_and_ranges_and_models_give_their_top_frequency`.

### `bench_mcp_sa_ident` [@ANCHOR: bench_mcp_sa_ident]

What `open` learned from `*IDN?` (`pub struct Ident`), returned alongside the link and used by `status` and the sweep tools.

| Field | Type | Meaning |
|---|---|---|
| `make` | `String` | First `*IDN?` field, normally `Siglent Technologies`. Not validated. |
| `model` | `String` | Second field, for example `SSA3021X`; guaranteed to start with `SSA3` and to have a known top frequency. |
| `serial` | `String` | Third field, the serial number as sent. |
| `firmware` | `String` | Fourth field, the firmware version as sent. |
| `max_hz` | `f64`, Hz | `model_max_hz(model)`: the highest frequency the model can tune. Always greater than 0 here. |

Invariants: built only by `open` after the prefix and model checks, with each field trimmed of whitespace; fields beyond the fourth in the reply are dropped.
Not serialized itself (status copies the fields into its JSON as `make`, `model`, `serial`, `firmware`, `max_frequency_hz`). Owned by the caller of `open` for
one tool call; never shared.

## Opening the instrument and status

### `bench_mcp_sa_open` [@ANCHOR: bench_mcp_sa_open]

Opens the link to the analyzer and proves it is an SSA3000X before anything else is sent. Private; the first step of every tool call that talks to the unit.

| Argument | Type | Meaning |
|---|---|---|
| `connect` | `&Connector` | How to reach the instrument (the TCP socket in production, a fake in tests). |
| `inst` | `&Instrument` | The configured analyzer; `host` and `port` identify it. |

Returns `Ok((Link, Ident))`. It connects, asks `*IDN?` (`READ_TRIES` of `REPLY_TIMEOUT_S`), splits the reply on commas, and requires at least four fields, a
model starting with `SSA3`, and a known top frequency. Errors: any connector error (DNS, refused, 5 s connect timeout); the `ask` errors; and
`<host>:<port> answered *IDN? with "<reply>", not an SSA3000X spectrum analyzer. Nothing else was sent to it. Next: check the host and port in the bench config.`
The wrong-device error closes the link by dropping it. The link is exclusive: the analyzer serves one SCPI client at a time, so a second concurrent call can
fail to connect. Side effects: a TCP connection and one query; no setting is changed. Tests: `sa::tests::status_reads_the_recorded_session_and_a_stranger_is_sent_nothing`.

### `bench_mcp_sa_options` [@ANCHOR: bench_mcp_sa_options]

Lists the options installed in the analyzer. `pub(crate)`; used by `status` and by `tg.rs` to find out whether the tracking generator (`TG`) is fitted.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&mut Link` | The open link. |

Asks `:SYSTem:OPTions?` and splits the reply on spaces and commas, dropping empty pieces. Returns `Ok(Vec<String>)`, for example `["Meas","EMI","TG","CAT"]`; an
empty list is possible. Errors: those of `ask`. Names are returned as sent; callers compare case-insensitively. Read-only, blocks up to 15 seconds.
Tests: `sa::tests::status_reads_the_recorded_session_and_a_stranger_is_sent_nothing`.

### `bench_mcp_sa_status` [@ANCHOR: bench_mcp_sa_status]

The tool `bench_sa_status` (tier `read_only`): identity, options, the tracking generator state and the sweep settings, all read back from the unit. It changes nothing.

| Argument | Type | Meaning |
|---|---|---|
| `connect` | `&Connector` | How to reach the instrument. |
| `inst` | `&Instrument` | The analyzer to read. |

Returns `Ok(Value)`, a JSON object: `make`, `model`, `serial`, `firmware`, `max_frequency_hz`, `options` (the list), `tracking_generator` and `sweep`. The
generator entry is `{"state":"on"|"off","level_dbm","max_tg_dbm","level_offset_db"}` when option `TG` is installed (a case-insensitive match), or
`{"state":"not installed","max_tg_dbm"}` otherwise. `sweep` holds `start_hz`, `stop_hz`, `rbw_hz`, `vbw_hz`, `ref_level_dbm`, `attenuation_db`, `detector` (text as
the unit gives it), `sweep_time_s`, `continuous` (boolean) and `points` (751). Errors: every error of `open`, `options`, `ask`, `ask_f64` and `on_off`; the first
failing read ends the call, with the `next_step` advice. About 15 queries over the LAN, each at most 3 tries of 5 seconds; no command that changes state is sent.
Safe at any time, and the first thing to call when anything went wrong. Tests: `sa::tests::status_reads_the_recorded_session_and_a_stranger_is_sent_nothing`.

## Reaching the instrument and dispatch

### `bench_mcp_sa_connector` [@ANCHOR: bench_mcp_sa_connector]

The type of the "how to reach the instrument" function: `pub type Connector = dyn Fn(&Instrument) -> Result<Link, String>`. Production passes `tcp`; tests pass a closure
that returns a `Link` onto the fake instrument in `testkit.rs`, so the whole path (tier check, argument checks, the SCPI dialect) is exercised with no hardware.
It is used as `&Connector` (borrowed, not stored), is called once per tool call by `open`, and must return an exclusive, freshly opened link or an error string. It
needs no locking of its own. Tests: every `sa::` test that runs a tool through a fake.

### `bench_mcp_sa_tcp` [@ANCHOR: bench_mcp_sa_tcp]

The production `Connector`: opens the analyzer's raw SCPI socket.

| Argument | Type | Meaning |
|---|---|---|
| `inst` | `&Instrument` | The analyzer; `host` (empty string when the config gave none, which then fails to resolve) and `port` (5025 for the raw socket). |

Returns `Link::tcp(host, port)`: a TCP connection with a 5 second connect timeout and Nagle off, line terminator `\n`. Errors (strings): name resolution failure,
no address, connect failure or timeout. Side effect: one TCP connection, closed when the `Link` is dropped at the end of the tool call. Not used for the USB
connection, whose replies are cut short. Tests: `sa::tests` (the string-based tests use fake connectors; unknown whether `tcp` itself has a direct test here).

### `bench_mcp_sa_call` [@ANCHOR: bench_mcp_sa_call]

The driver's entry point for the MCP server: runs one tool against real hardware. Registered in `main.rs` as the driver's `call`.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]` | All configured instruments. |
| `name` | `&str` | The tool name, one of `SPECS`. |
| `args` | `&Value` | The tool arguments (a JSON object). |

It is `call_with(&tcp, insts, name, args)` and returns the same `Result<Value, String>`: the tool's JSON result or an error text for the model. Blocking and
synchronous; the caller decides which thread runs it. See `bench_mcp_sa_call_with` for the checks and the tier gate. Tests: `sa::tests::tools_are_listed_by_tier_and_refused_below_it_before_any_connection`.

### `bench_mcp_sa_call_with` [@ANCHOR: bench_mcp_sa_call_with]

Runs one spectrum analyzer tool, with the way to reach the instrument given so tests can use a fake. The tier gate lives here.

| Argument | Type | Meaning |
|---|---|---|
| `connect` | `&Connector` | `tcp` in production, a fake in tests. |
| `insts` | `&[Instrument]` | All configured instruments. |
| `name` | `&str` | The tool name. |
| `args` | `&Value` | The tool arguments. |

Order of work: find the `Spec` by name; choose the analyzer with `instrument`; compare the analyzer's configured tier with the tool's tier; then dispatch to
`status`, `sweep::sweep`, `sweep::marker`, `sweep::screenshot`, `tg::tg_on`, `tg::tg_off`, `tg::tg_level`, `tg::normalize` or `tg::s21`. Returns that function's
`Ok(Value)`. Errors: `unknown tool <name>`; the `instrument` errors; `<name> needs tier <T>; "<inst>" is configured at <T2>` when the configured tier is lower
than the tool's (so a `read_only` analyzer cannot be reconfigured even if the model guesses a tool name that was not listed); and `<name> does not apply to "<inst>"` for a
`SPECS` entry without a match arm. No tool is run and no connection is opened when any of the first three checks fails. Side effects and blocking come from
the tool called (TCP, the instrument's settings, files in `BENCH_MCP_OUT`); the tracking generator is a low-level bench source, so only the tier and the config cap
`max_tg_dbm` apply, not a transmit lock. Example: `call_with(&fake, &insts, "bench_sa_status", &json!({}))`. Tests: `sa::tests::tools_are_listed_by_tier_and_refused_below_it_before_any_connection` and the status test.
