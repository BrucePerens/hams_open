# The Logic Analyzer and Pattern Generator Tools Trigger, Window, Resample, Export and Decode on the Host

As the person (or the AI agent) at the bench with a Glasgow, I want to record the pins of a circuit, find the moment I care about, keep a little of what happened before it,
choose a sample rate, get the result as VCD, CSV or sigrok files, read a summary with a glitch report, have UART, I2C and SPI decoded for me, and drive a bounded
square wave or bit pattern at a pin, all from `bench_gla_*` tools that refuse what the instrument and the config do not allow. This story covers every file of
`daemons/bench_mcp/src/gla/`: `mod.rs` (the tool list and the dispatcher), `tools.rs` (the five tools' working code), `wave.rs` (the waveform model, VCD and CSV),
`stats.rs` (the per-pin summary), `trigger.rs`, `export.rs` (the sigrok `.sr` file), `decode.rs` and `pattern.rs` (the planner). The tool pages for the agent are in
`docs/GLASGOW_LOGIC_TOOLS.md` and `manual/instruments/glasgow/`; the Glasgow driver these tools sit on is `src/glasgow/`.

The design in one paragraph: the Glasgow revision C `analyzer` applet streams every pin change with a 48 MHz timestamp and has no trigger and no rate setting.
`bench_gla_capture` asks the Glasgow module's `bench_glasgow_logic_capture` for a VCD file, and everything else (trigger, pre-trigger window, resampling, statistics, files,
decoding) happens here on the host. `bench_gla_pattern` plans a pattern here and has `bench_glasgow_pattern` play it. This module never reaches the device itself: it adds no way
to talk to USB, so every safety rule (port voltage set first, tier, time limit, `confirm`, parking the pins) is the Glasgow module's. All code except `tools::capture`,
`tools::pattern_tool` and the dispatcher's `safe_state` and `limits` arms is pure and runs without hardware.

Nothing here transmits on the air. The two tools that touch the target are passive recording (`bench_gla_capture`, at most weak pull resistors, see its entry) and pattern driving
(`bench_gla_pattern`), which drives the Glasgow's I/O pins at the voltage the owner set on the port. For those, each entry says how the tier, the `confirm` flag and the time limit
apply; there is no band check and no radio TX gate because no radio is involved.

Conventions used in the entries: times in the waveform model are nanoseconds (`u64`) from the start of the capture; a "pin" is a bit position in a `u32` pin-state word (bit n is
captured pin n, in the order of the VCD's wires, which is the order of the `pins` argument); a "pin argument" in a JSON spec is a captured pin name such as `"A0"` or its position as a
number. Tests live in `src/gla/tests.rs` and are cited as `gla::tests::<name>`. They use known waveforms and a fake backend; none touches a device.

## The tool list and the dispatcher (`mod.rs`)

### `gla_tool_defs` [@ANCHOR: gla_tool_defs]

`pub fn tool_defs(insts: &[Instrument]) -> Vec<Tool>`: the `bench_gla_*` tools the server lists. Called by the server's module table (`main.rs`, prefix `bench_gla_`) when a client asks
for the tool list, so a model sees only the tools its configuration allows.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | Every configured instrument (all kinds). Only entries of kind `Glasgow` count. |

Returns the entries of the private table `SPECS` for which at least one Glasgow instrument has a tier at or above the tool's tier (tiers are ordered `ReadOnly` < `ReadConfig` < `Full`). The
table holds five tools: `bench_gla_limits` and `bench_gla_analyze` (`ReadOnly`), `bench_gla_capture` and `bench_gla_safe_state` (`ReadConfig`), `bench_gla_pattern` (`Full`). With no Glasgow
configured the list is empty. Each tool's description is `[<Tier>] <text>`, where `<Tier>` is the Rust debug name of the tier. Each input schema is a JSON object whose `properties` are the
table's text plus an `instrument` string, with the table's `required` list (`file` for analyze, `pins` and `pattern` for pattern, none otherwise).

Errors: none. The property text is a compile-time constant; should it ever fail to parse as JSON the tool would be listed with a null `properties` (the code falls back to `Null` instead of
failing), and no test checks that every property block parses. Pure function, no locks, no I/O, safe on any thread. The result is rebuilt on each call.

See also: `gla_call` (the same tier rule is enforced again when a tool is called), `gla_instrument`. Test: `gla::tests::tools_are_listed_by_tier_the_instrument_is_found_and_the_tier_is_enforced`.

### `gla_real_backend` [@ANCHOR: gla_real_backend]

The production implementation of the `Backend` trait (`gla_backend_trait`): the private tuple struct `Real<'a>(&'a [Instrument])`, built by `call` for each tool call. Its one method,
`fn call(&self, tool: &str, args: &Value) -> Result<Value, String>`, forwards to `crate::glasgow::call(self.0, tool, args)` unchanged, with the same instrument list. The tools in `tools.rs` use it to ask for
`bench_glasgow_logic_capture`, `bench_glasgow_pattern` and `bench_glasgow_safe_state`; everything the backend does and refuses (tier of that tool, port voltage set first, `confirm`, the time and file-size
limits, the device lock file, parking the pins) is decided there, not here.

| Field | Type | Meaning |
|---|---|---|
| `0` | `&'a [Instrument]`, borrowed | The server's instrument list; lives for the duration of one call. |

Returns what `glasgow::call` returns: the tool's JSON answer, or its error text (passed through verbatim). Blocking: the Glasgow call can take as long as the capture or the pattern plus its operation
timeout. No state of its own, no lock of its own. Test: `gla::tests::the_production_backend_hands_the_call_to_the_glasgow_module` (with a fake in all other tests, `gla_backend_trait`).

### `gla_instrument` [@ANCHOR: gla_instrument]

`fn glasgow_instrument<'a>(insts: &'a [Instrument], args: &Value) -> Result<&'a Instrument, String>` (private): picks the Glasgow instrument a call is for. Called first by `call`.

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | All configured instruments; only kind `Glasgow` entries are considered. |
| `args` | `&Value`, borrowed | The tool arguments. Only the optional string `instrument` is read; a value that is not a string counts as absent. |

Returns a reference into `insts`. Rules: `instrument` given, then the Glasgow with that name; no `instrument`, then the only Glasgow if there is exactly one. Errors (strings): `no glasgow instrument named "<n>"`
(named one not found, including a name that belongs to a non-Glasgow instrument); `no glasgow instrument is configured`; `name the instrument: a, b` (several Glasgows and none named). Pure, no I/O, no locks.

See also: `gla_call`. Test: `gla::tests::tools_are_listed_by_tier_the_instrument_is_found_and_the_tier_is_enforced`.

### `gla_call` [@ANCHOR: gla_call]

`pub fn call(insts: &[Instrument], name: &str, args: &Value) -> Result<Value, String>`: runs one `bench_gla_*` tool. The server's module table calls it for every tool whose name starts with `bench_gla_`,
on a blocking worker thread (`spawn_blocking`).

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | All configured instruments. |
| `name` | `&str` | The tool name; must be one of the five in `SPECS`. |
| `args` | `&Value`, borrowed (cloned inside) | The tool arguments; the optional `instrument` is read here, the rest by the tool. |

Steps: find the tool in `SPECS` (`unknown tool <name>` otherwise); pick the instrument (`gla_instrument`); refuse a tier that is too low (`<name> needs tier <Full|...>; "<inst>" is configured at <...>`); build a
`tools::Ctx` (`gla_ctx_struct`) with the real backend (`gla_real_backend`), `crate::out_dir()` (the directory `BENCH_MCP_OUT` or `bench_mcp` under the system temp directory, created if missing) and the
instrument's `glasgow.max_capture_s` (config default 30 s, accepted range 1 to 120); add the chosen instrument name to the arguments; then dispatch:

| Tool | Action | Notes |
|---|---|---|
| `bench_gla_limits` | `tools::limits(pins, link)` | `pins` default 4 (an unsigned integer, else 4). `link` is the `link_mbps` argument; if absent, `link_speed_mbps` of the Glasgow found in `/sys/bus/usb/devices` (`glasgow::parse::usb_device`); if still unknown, 480. The reply gets `link_mbps_source`: `"argument"`, `"sysfs"` or `"assumed"`. |
| `bench_gla_safe_state` | backend `bench_glasgow_safe_state` with `{"instrument": name}` | Switches every port's regulator and pin driver off and reads back 0 V (that is the Glasgow module's job). |
| `bench_gla_analyze` | `tools::analyze` | Reads a VCD file; no device. |
| `bench_gla_capture` | `tools::capture` | Passive recording through the backend. |
| any other name | `tools::pattern_tool` | In practice only `bench_gla_pattern` reaches this arm, because of the `SPECS` check. |

Returns the tool's JSON answer or its error string. Locking: this function takes no per-instrument lock (the server's `instrument_lock` is not taken for module tools); the backend serialises device use with the
Glasgow install's lock file (`glasgow::device_lock`). Side effects only in the tools it calls. Transmit controls (for the pattern tool, which drives pins): the tier is checked here; `confirm`, the set port voltage, the time
limit and the release of the pins are the backend's.

See also: `gla_tool_defs`, `gla_instrument`. Test: `gla::tests::tools_are_listed_by_tier_the_instrument_is_found_and_the_tier_is_enforced`.

## The tools behind the names (`tools.rs`)

### `gla_backend_trait` [@ANCHOR: gla_backend_trait]

`pub trait Backend { fn call(&self, tool: &str, args: &Value) -> Result<Value, String>; }`: how the tools reach the Glasgow module, one call by tool name. It exists so the tests can replace the device with a fake
that writes known waveforms; the server uses `Real` (`gla_real_backend`).

| Method argument | Type | Meaning |
|---|---|---|
| `tool` | `&str` | A backend tool name: here `bench_glasgow_logic_capture`, `bench_glasgow_pattern` or `bench_glasgow_safe_state`. |
| `args` | `&Value`, borrowed | The backend tool's arguments, including `instrument`. |

Returns the backend tool's JSON answer, or its refusal text as `Err(String)`; the tools in this file pass an `Err` straight to the caller, so a refusal is shown as the Glasgow module wrote it. Implementations may block
for seconds (a real capture). No `Send`/`Sync` bound is declared; `Ctx` holds the backend by `&dyn Backend`, used from one thread per call. Test: `gla::tests::capture_asks_the_backend_with_only_its_own_arguments_and_analyses_the_file` and
`gla::tests::a_backend_refusal_ends_the_capture_and_the_pattern_with_its_own_message`.

### `gla_ctx_struct` [@ANCHOR: gla_ctx_struct]

`pub struct Ctx<'a>`: everything a tool call needs besides its arguments. Built by `call` for each call, passed by reference to `analyze`, `capture`, `pattern_tool` and `process`, and dropped when the call returns.

| Field | Type | Meaning |
|---|---|---|
| `backend` | `&'a dyn Backend` | The way to the Glasgow module (`gla_backend_trait`). Set by `call`, read by `capture` and `pattern_tool`. |
| `out_dir` | `PathBuf` | Where this module writes its exports and reads VCD files for `analyze`: the server's `BENCH_MCP_OUT` directory. Set by `call`; read by `inside`, `process`. |
| `max_capture_s` | `f64`, seconds | The instrument config's `max_capture_s` (default 30, config range 1 to 120): no capture or pattern runs longer. Read by `capture` (duration check) and `pattern_tool` (plan limit). |

Invariants: `max_capture_s` is positive and finite when it came from the config. There is no serialized form and no default. Concurrency: a plain value owned by one call; nothing mutates it, so no lock is needed.
Test: `gla::tests::capture_asks_the_backend_with_only_its_own_arguments_and_analyses_the_file`.

### `gla_pin_names` [@ANCHOR: gla_pin_names]

`pub fn pin_names(spec: &str) -> Result<Vec<String>, String>`: expands a pin spec such as `A0:3` or `A0:1,B0` into pin names (`A0 A1 A2 A3`; `A0 A1 B0`). Used by `capture` and `pattern_tool` to name the pins, and so to
know how many there are, before the backend is called.

| Argument | Type | Meaning |
|---|---|---|
| `spec` | `&str`, borrowed | Comma-separated parts. A part is a port letter `A` or `B` (upper case), a pin digit `0` to `7`, and optionally `:` and an end digit that is not lower than the first (`A2:5`). No spaces are trimmed. |

Returns the names in order. Errors: a bad part gives `pin spec part "<p>" is not like A0 or A0:3`; fewer than 1 or more than `wave::MAX_PINS` (16) pins gives `<n> pins; a capture holds 1 to 16`. Duplicates are not
detected (`A0,A0` gives two names). Pure, no side effects. Example: `pin_names("A0:1,B0")` is `["A0","A1","B0"]`.

See also: `gla_capture`, `gla_pattern_tool`. Test: `gla::tests::pin_specs_become_names_and_wrong_ones_are_refused`.

### `gla_auto_rate` [@ANCHOR: gla_auto_rate]

`pub fn auto_rate(w: &wave::Wave) -> f64`: the sample rate used when the caller gave no `sample_rate_hz`. Called by `process`.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave`, borrowed | The (windowed) waveform. |

Returns a rate in hertz. The shortest pulse is the smallest time between two consecutive changes of one pin, over all pins (the first and last spans, cut by the capture's ends, are not pulses). The wanted rate is
`4e9 / shortest_ns`, so the shortest pulse gets at least 4 samples; with no pulse at all, the wanted rate is 0. From the offered rates (1, 2, 5 and 10, 20, 50 and so on per decade from 1 kHz to 20 MHz, then 24 MHz and 48 MHz) it takes
the lowest one at or above the wanted rate that also keeps the sample count under `wave::MAX_SAMPLES` (8,000,000, tested as `end_ns / 1e9 * rate < MAX_SAMPLES`); if none qualifies, the highest that fits; if none fits at all, 1000. Never fails, pure.
Example: a shortest pulse of 500 ns wants 8 MHz and gets 10 MHz.

See also: `gla_process`, `gla_resample`. Test: `gla::tests::the_automatic_rate_shows_the_shortest_pulse_with_four_samples`.

### `gla_limits` [@ANCHOR: gla_limits]

`pub fn limits(npins: u32, link_mbps: f64) -> Value`: the model of what the USB link can carry, the whole of `bench_gla_limits`. A model, labelled as such (`"model": true`); the measured limits of a real unit are in the manual page.

| Argument | Type | Meaning |
|---|---|---|
| `npins` | `u32` | Number of captured pins; each record carries `ceil(npins/8)` data bytes (0 pins is accepted and gives 0 data bytes). |
| `link_mbps` | `f64`, megabits per second | The USB link speed. Below 400 counts as full speed (12), anything else, including NaN, as high speed (480). |

Returns a JSON object. The analyzer sends one record per pin change: 1 to 3 delay bytes (7 bits each, in 48 MHz ticks), one event byte and the data bytes. The usable bulk rate is `bulk_bytes_per_s`: 1.04 MiB/s (1,090,519.04 B/s)
at full speed, a figure the code comment says was measured on a revision C3 unit on 2026-11-10 with the full-speed fallback gateware; 40,000,000 B/s at high speed, which the code labels a model not yet measured. `rows` has three
entries (`spacing`, `bytes_per_edge`, `max_edges_per_s` = floor of the bulk rate over the bytes) for edges closer than 2.7 us, 2.7 us to 340 us and 340 us to 44 ms apart. Other keys: `capture_works` (true at high speed),
`capture_works_in_full_speed_fallback` (true at full speed), `bulk_bytes_per_s_source`, `note` (a long explanation, different for each speed), `timestamp_clock_hz` 48,000,000, `timestamp_resolution_ns` about 20.83, `trigger` and `sample_rate` (text
saying the trigger and the rate are applied on the host). The constants `CLOCK_HZ` (48e6) and `RATES` are in the same file. No errors, no device access, pure.

See also: `gla_call` (the `bench_gla_limits` arm adds `link_mbps_source`), `gla_capture` (which returns `rows` for 480 Mb/s whatever the real link). Test: `gla::tests::the_limits_say_that_full_speed_cannot_capture_and_high_speed_is_edge_limited` and
`gla::tests::analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`.

### `gla_read_wave` [@ANCHOR: gla_read_wave]

`fn read_wave(path: &Path) -> Result<wave::Wave, String>` (private): reads a VCD file and parses it with `wave::parse_vcd` (`gla_parse_vcd`). Used by `analyze` and `capture`.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&Path`, borrowed | The file to read. |

Returns the waveform. Errors: the read failure as `<path>: <os error>`, or any `parse_vcd` error text. Blocking file read; the whole file is held in memory (no size limit here; the backend bounds its own capture file by `max_capture_mb`).
Test: `gla::tests::analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`.

### `gla_inside` [@ANCHOR: gla_inside]

`fn inside(ctx: &Ctx, name: &str) -> Result<PathBuf, String>` (private): turns a file name argument into a path inside the output directory, refusing anything that could leave it. Used by `analyze` for its `file` argument.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx`, borrowed | Gives `out_dir`. |
| `name` | `&str` | A plain file name. Refused when empty, when it contains `/` or `\`, or when it starts with `.` (so `..` and hidden files are out). |

Returns `ctx.out_dir.join(name)`; it does not check that the file exists. Error: `file "<name>" must be a plain file name in the output directory <dir>`. Pure. Test: `gla::tests::analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`.

### `gla_process` [@ANCHOR: gla_process]

`pub fn process(ctx: &Ctx, w: &wave::Wave, args: &Value, stem: &str) -> Result<Value, String>`: the part common to `bench_gla_capture` and `bench_gla_analyze`: trigger and window, resample, summary, files, decode.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx` | Gives `out_dir` for the files. |
| `w` | `&Wave` | The full capture. |
| `args` | `&Value` | Reads `trigger` (`gla_trigger_parse`), `pre_trigger_ms`, `post_trigger_ms`, `samples`, `sample_rate_hz`, `glitch_ns`, `formats`, `decode` (`gla_decode_run`). |
| `stem` | `&str` | File name stem for the files written. |

Window length: `pre_trigger_ms` (default 0) is converted to ns (negative or non-finite values count as absent). `post_trigger_ms` if given; otherwise `samples` at `sample_rate_hz`, i.e. `samples / rate` seconds minus the pre-trigger part (an error `samples needs sample_rate_hz`
without a rate); otherwise the rest of the capture. The trigger window is found with `gla_trigger_apply`. The rate is `sample_rate_hz` or `auto_rate`; `wave::resample` checks its range. `glitch_ns` defaults to the larger of one sample period and two timestamp
ticks (about 41.7 ns), rounded. `formats` is a list of `vcd`, `csv`, `sr` (default all three; `null` means default; an empty list writes nothing and does not even create the directory); each is written to `<out_dir>/<stem>.<ext>`
and overwrites a file of that name. VCD and CSV hold the window at full resolution; `.sr` holds the resampled data.

Returns an object: `trigger` (`spec`, `at_ns_in_window`, `at_ns_in_capture`, `pre_trigger_ns` = the time before the trigger actually kept), `window` and `full_capture` (`gla_describe`), `sampling` (`rate_hz`, `samples`, `missed_pulses`, `chosen`),
`summary` (`gla_summarize`), `files` (per format `file` and `bytes`), `overrun` (true when the full capture had a FIFO overrun, even if the window excludes it) and, when `decode` is given, `decode`. Errors: those of the steps above, `formats must be a list of vcd, csv, sr`,
`format "<f>" must be vcd, csv or sr` (an unknown format is found while writing, so earlier formats in the list may already be on disk), and file errors as `<path>: <os error>`. Blocking file writes; memory is up to 8,000,000 samples (32 MB) plus the `.sr` copy. No device access, no lock.

See also: `gla_capture`, `gla_analyze`. Tests: `gla::tests::analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`, `gla::tests::a_capture_after_the_port_is_set_is_decoded_and_the_pins_are_parked`.

### `gla_analyze` [@ANCHOR: gla_analyze]

`pub fn analyze(ctx: &Ctx, args: &Value) -> Result<Value, String>`: the tool `bench_gla_analyze` (tier `read_only`): the same analysis on a VCD file already in the output directory (a capture of `bench_gla_capture` or `bench_glasgow_logic_capture`, or any VCD of 1-bit wires). It touches no device.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx` | Output directory. |
| `args` | `&Value` | `file` (required): a file name ending `.vcd`; plus every optional argument of `process`. |

Returns the `process` answer with `source` (the full path read) added; exported files are named `<name without .vcd>_analysis.<ext>` in the output directory. Errors: `file is required: a .vcd file name in the output directory`; `file "<n>" must be a .vcd file`; the `inside` refusal; read and parse errors; and those of `process`.
Note: although the tool needs no device and is listed at tier `read_only`, it writes export files into the output directory unless `formats` is `[]`. Blocking file I/O; no lock. Test: `gla::tests::analyze_reads_a_vcd_from_the_output_directory_and_writes_all_three_formats`.

### `gla_capture` [@ANCHOR: gla_capture]

`pub fn capture(ctx: &Ctx, args: &Value) -> Result<Value, String>`: the tool `bench_gla_capture` (tier `read_config`): records pins through the backend, then analyses the recorded file like `analyze`. The pins are inputs; nothing is transmitted.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx` | Backend, output directory, `max_capture_s`. |
| `args` | `&Value` | `pins` (string, default `A0:3`, 1 to 16 pins), `duration_s` (number, default 2.0, 0.1 up to `max_capture_s`), `pull` (`none`, `up`, `down`), `confirm` (required by the backend with `pull` up or down), `instrument`, plus every optional argument of `process`. |

Steps: expand the pins (`gla_pin_names`); check `duration_s` (error `duration_s <d> must be 0.1 to <max> (the config's max_capture_s)`); call the backend tool `bench_glasgow_logic_capture` with only `pins`, `duration_s`, `pull`, `confirm` and `instrument`; return the backend's answer at once if it
contains a `dry_run` key; take the `file` path from the answer (error `the backend answered without a capture file`); read it; replace the wire names by the pin names when the counts match; run `process` with the stem `<backend file stem>_gla`. The result is the `process` answer plus
`backend` (`file`, `overrun`, `pins_state`, `stopped_by` as the backend gave them) and `limits` (the `rows` of `limits(npins, 480.0)`, always for 480 Mb/s whatever the real link).

Safety: the backend refuses without a set port voltage (`bench_glasgow_port_set`), over the config's time and size limits, and without a high-speed link or the config's `full_speed_fallback`; its refusal text is returned as the error. Tier `read_config` is checked in `gla_call`. The backend alone decides `pull`: weak pulls put a level on the target and the backend requires `confirm: true` for them; this function forwards `confirm` (the tool's schema has it) so `pull` `up` or `down` works with `confirm: true` and is refused without it. Blocking for about `duration_s` plus the backend's operation timeout. The backend takes the device lock.

See also: `gla_process`, `gla_backend_trait`, `gla_limits`. Tests: `gla::tests::a_capture_after_the_port_is_set_is_decoded_and_the_pins_are_parked`, `gla::tests::capture_asks_the_backend_with_only_its_own_arguments_and_analyses_the_file`,
`gla::tests::the_capture_and_the_pattern_refuse_until_the_port_voltage_is_set_through_the_backend`, `gla::tests::a_full_speed_link_stops_the_capture_in_the_backend_before_anything_runs`, `gla::tests::a_backend_refusal_ends_the_capture_and_the_pattern_with_its_own_message`.

### `gla_pattern_tool` [@ANCHOR: gla_pattern_tool]

`pub fn pattern_tool(ctx: &Ctx, args: &Value) -> Result<Value, String>`: the tool `bench_gla_pattern` (tier `full`): plans a pattern (`gla_pattern_plan`) and has the backend play it. This drives the Glasgow's I/O pins at the voltage the owner set for the port; it does not transmit on the air.

| Argument | Type | Meaning |
|---|---|---|
| `ctx` | `&Ctx` | Backend and `max_capture_s`. |
| `args` | `&Value` | `pins` (required string, 1 to 8 pins, e.g. `A0:3`); `pattern` (an object as for `plan`; when absent the arguments themselves are read as the pattern); optional `confirm`, `dry_run`, `instrument`, passed on to the backend. |

Errors: `pins is required, e.g. A0:3`; the `pin_names` errors; `a pattern drives at most 8 pins`; every planner error (`gla_pattern_plan`); the backend's refusal. Returns `planned` (`steps` count, `repeat`, `total_s`, `achieved`, `pins`, `timing` text) and `backend` (the backend answer, or its dry-run plan).
The plan is made, and its limits are checked, even with `dry_run`.

Controls: the tier is checked in `gla_call`; the total run time is limited to `max_capture_s` by the planner and again by the backend; `confirm: true`, a port voltage set first, and releasing all pins at the end (also on failure) are the backend's rules, and without `confirm` the backend refuses unless `dry_run` is true.
Timing is host-timed: holds of 0.5 ms or more and about 1 ms of jitter. Blocking for the pattern's `total_s`. The backend takes the device lock.

See also: `gla_pattern_plan`, `gla_pattern_backend_args`. Tests: `gla::tests::a_pattern_is_planned_here_confirmed_and_bounded_by_the_backend_and_loops_back_into_a_capture`, `gla::tests::a_pattern_through_the_backend_needs_confirm_and_releases_the_pins`,
`gla::tests::the_capture_and_the_pattern_refuse_until_the_port_voltage_is_set_through_the_backend`, `gla::tests::a_backend_refusal_ends_the_capture_and_the_pattern_with_its_own_message`.

## The waveform model (`wave.rs`)

### `gla_wave_struct` [@ANCHOR: gla_wave_struct]

`pub struct Wave` (with `pub struct Edge { t_ns: u64, value: u32 }`, one change: at `t_ns` the pins take `value`): a captured waveform as a list of timestamped changes of up to `MAX_PINS` = 16 pins. Created by `parse_vcd` and `window`, read by every other function in the module.

| Field | Type | Meaning |
|---|---|---|
| `names` | `Vec<String>` | Pin names in order; bit n of every state word is `names[n]`. 1 to 16 entries when it came from `parse_vcd`. |
| `init` | `u32` | The pin state at time 0 (a bit per pin). |
| `edges` | `Vec<Edge>` | The changes, strictly increasing in `t_ns`; each differs from the state before it; changes with one timestamp are one entry. |
| `end_ns` | `u64`, ns | When the capture ended (not before the last edge). |
| `origin_ns` | `u64`, ns | Where this wave's time 0 lies in the original capture: 0 from `parse_vcd`, non-zero after `window`. Added back to times reported to the caller (`t_ns` in summaries and decodes). |
| `overrun_ns` | `Option<u64>` | Set to the time of the first `x` or `z` in the VCD (the analyzer's overrun marker: samples after it are lost); `None` if none. |

Derives `Clone, Debug, PartialEq`. Invariants as in the table, all established by `parse_vcd`; the functions that take a `Wave` assume them (a hand-built wave with unsorted edges gives wrong lookups). Lifetime: built per call, owned by the call, dropped at the end; the `window` result is a separate copy. No serialized form of its own: it is written with `write_vcd` and `write_csv` and summarised by `describe`.
Concurrency: plain data, no interior mutability, no lock. Test: `gla::tests::vcd_is_parsed_with_initial_state_coalesced_changes_and_the_overrun_marker`, `gla::tests::vcd_and_csv_written_from_a_wave_read_back_the_same`.

### `gla_sampled_struct` [@ANCHOR: gla_sampled_struct]

`pub struct Sampled`: a wave sampled at a fixed rate, made by `resample` and consumed by `write_sr`.

| Field | Type | Meaning |
|---|---|---|
| `rate_hz` | `f64`, Hz | The sample rate; sample i is the pin state at `i * 1e9 / rate_hz` ns. 1 to 48,000,000. |
| `data` | `Vec<u32>` | One pin-state word per sample (bit n = pin n); at most `MAX_SAMPLES` (8,000,000) entries. |
| `missed_pulses` | `u64` | Pulses that began and ended between two samples: present in the source wave, invisible at this rate. |

Derives `Clone, Debug, PartialEq`. Invariant: `data.len()` is `floor(end_ns / period) + 1` for the wave it came from. Owned by the call that made it; no serialized form of its own (`write_sr` packs it; `process` reports `rate_hz`, `data.len()` and `missed_pulses`). No concurrency concerns.
Test: `gla::tests::resampling_follows_the_wave_and_counts_the_pulses_it_cannot_show`, `gla::tests::describe_names_the_pins_the_length_and_an_overrun`.

### `gla_wave_value_at` [@ANCHOR: gla_wave_value_at]

`pub fn value_at(&self, t_ns: u64) -> u32`: the pin state at time `t_ns`, that is the value of the last change at or before it, or `init` if there is none. Binary search over the edges (O(log n)). `t_ns` beyond `end_ns` just returns the final state. No errors, pure.
Used by `write_csv`, `find_state`, `summarize` and the UART decoder. Test: `gla::tests::value_lookups_and_pin_names_resolve`.

### `gla_wave_value_before` [@ANCHOR: gla_wave_value_before]

`pub fn value_before(&self, i: usize) -> u32`: the pin state just before the change at index `i` of `edges`, which is what a clocked receiver sees at that edge: `init` for `i == 0`, else `edges[i - 1].value`. Argument `i` must be at most `edges.len()` (the value after the last edge); a larger `i` panics (index out of bounds).
Used by the trigger and the three decoders, which look at each edge together with the state before it. Pure. Test: `gla::tests::value_lookups_and_pin_names_resolve`.

### `gla_wave_pin_index` [@ANCHOR: gla_wave_pin_index]

`pub fn pin_index(&self, pin: &Value) -> Result<usize, String>`: resolves a pin argument of a JSON spec (trigger and decode) to its index.

| Argument | Type | Meaning |
|---|---|---|
| `pin` | `&Value` | An unsigned integer: the position, 0 to `names.len() - 1`. Or a string equal to a captured pin name (`"A0"`). |

Returns the index. Errors: a number out of range gives `pin <n> is outside 0..<count>`; a name not among the pins gives `no captured pin named "<s>"; the pins are A0, A1, ...`; anything else (a float, a negative number, null, an object) gives `a pin is a name such as "A0" or its position as a number`. The doc comment in the code says "or by number text"; the code does
not do that: the string `"0"` only matches a pin that is actually named `0`. Pure. Test: `gla::tests::value_lookups_and_pin_names_resolve`.

### `gla_parse_vcd` [@ANCHOR: gla_parse_vcd]

`pub fn parse_vcd(text: &str) -> Result<Wave, String>`: parses the VCD text the Glasgow `analyzer` applet writes (also any VCD of 1-bit wires) into a `Wave`. Used by `read_wave`.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str`, borrowed | The file contents. Read as whitespace-separated tokens. |

Header: `$timescale <number><unit>` with unit `s`, `ms`, `us`, `ns` or `ps` (default 1 ns when absent; the number must be an integer); each `$var` needs at least four fields, size exactly 1 (`$var wire 1 <id> <name> $end`), at most 16 wires; other header keywords are skipped; `$enddefinitions` ends it.
Body: `#<time>` sets the time (scaled to ns and rounded; equal times are allowed, going backwards is an error); `0<id>`/`1<id>` change a wire; `x`, `X`, `z`, `Z` record the overrun time (the first one only) and are otherwise ignored; `$dumpvars`, `$dumpall`, `$dumpon`, `$dumpoff` and bare `$end` are skipped, and any other `$` block up to its `$end` is skipped. Values given before
the first change at time 0 become the initial state. Changes at one timestamp merge into one edge; changes that do not alter the state, and edges that return to the previous state within a timestamp, are dropped. `end_ns` is the later of the last edge and the last `#time`; `origin_ns` is 0.

Errors (strings): unknown timescale unit; `VCD timescale has no number`; `a $var line is too short`; `wire <n> is <k> bits wide; only 1-bit wires are supported`; `more than 16 wires`; `bad VCD time`; `VCD time goes backwards at #<t>`; `VCD change "<tok>" does not start with a value` (also for a token starting with a multi-byte character); `names an unknown id`; `is not 0, 1 or x` (e.g. vector changes `b101 !`);
`the VCD declares no wires`. Pure, linear in the file size apart from a linear id lookup per change; the whole text is in memory. Test: `gla::tests::vcd_is_parsed_with_initial_state_coalesced_changes_and_the_overrun_marker`, `gla::tests::vcd_and_csv_written_from_a_wave_read_back_the_same`.

### `gla_write_vcd` [@ANCHOR: gla_write_vcd]

`pub fn write_vcd(w: &Wave) -> String`: the wave as VCD text, in the format GTKWave, PulseView and the Glasgow's own tools read: `$timescale 1 ns`, one scope `glasgow`, one `$var wire 1` per pin with the identifier character `!` plus the pin index and the pin name, `#0` with `$dumpvars` holding the initial state, then for each edge `#<t_ns>` and only the wires that changed, and a final `#<end_ns>` line when the capture ends after the last edge.
Pin names are written as they are (a name with a space would make the file unreadable). The overrun marker is not written: a wave with `overrun_ns` set loses it in the file (the answer's `overrun` field still reports it). Pure, no errors; `parse_vcd(write_vcd(w))` gives back the same wave apart from `origin_ns`
(0 after parsing) and `overrun_ns`. Used by `process` for the `vcd` format. Test: `gla::tests::vcd_and_csv_written_from_a_wave_read_back_the_same`.

### `gla_write_csv` [@ANCHOR: gla_write_csv]

`pub fn write_csv(w: &Wave) -> String`: the wave as CSV: the header `time_ns,<pin names joined by commas>`, a first row for time 0 with the initial state, one row per change (`<t_ns>,0,1,...`, one 0 or 1 per pin), and, when the capture ends after the last change, a last row at `end_ns` with the final state. A change at exactly time 0
(a trigger edge kept by `window`) gives two rows with time 0, the initial state first. Pure, no errors, no escaping of names. Used by `process` for the `csv` format. Test: `gla::tests::vcd_and_csv_written_from_a_wave_read_back_the_same`.

### `gla_window` [@ANCHOR: gla_window]

`pub fn window(w: &Wave, from_ns: u64, to_ns: u64) -> Wave`: the part of a wave from `from_ns` to `to_ns`, re-based so that time 0 is `from_ns`. Called by `trigger::apply`.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The source; not changed. |
| `from_ns`, `to_ns` | `u64`, ns | The span. `from_ns` is clamped to `end_ns`, `to_ns` to the range `from..=end_ns`, so it never fails and an inverted span becomes an empty one. |

The result's `init` is the state just before `from_ns`, so a change at exactly `from_ns` is kept as a change at time 0; edges within `from..=to` are copied with their times reduced by `from`; `end_ns` is `to - from`; `origin_ns` is the source origin plus `from`; `overrun_ns` is kept (re-based) only when it lies inside the span. Pure. Test: `gla::tests::a_window_is_rebased_and_keeps_where_it_came_from`.

### `gla_resample` [@ANCHOR: gla_resample]

`pub fn resample(w: &Wave, rate_hz: f64) -> Result<Sampled, String>`: samples the wave at a fixed rate and counts the pulses that fall between samples. Called by `process`.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The wave to sample. |
| `rate_hz` | `f64`, Hz | 1 to 48,000,000 (the analyzer's timestamp clock). |

Sample i is taken at `round(i * 1e9 / rate_hz)` ns and holds the state of the last change at or before that time (so a change exactly at a sample instant is visible in that sample); there are `floor(end_ns / period) + 1` samples. A pulse is the span between two consecutive changes of one pin; it counts in `missed_pulses` when no sample instant falls inside it (an instant at the later edge counts as outside).
Errors: `sample_rate_hz <r> must be 1 to 48000000 (...)` (NaN and infinity too); `<n> samples at <r> Hz exceeds 8000000: lower sample_rate_hz or shorten the window`. Pure; allocates up to 32 MB. Test: `gla::tests::resampling_follows_the_wave_and_counts_the_pulses_it_cannot_show`.

### `gla_describe` [@ANCHOR: gla_describe]

`pub fn describe(w: &Wave) -> Value`: the summary fields common to every file written: `pins` (the names), `duration_ns` (`end_ns`), `changes` (the number of edges), `origin_ns`, `overrun_at_ns` (the overrun time or null). Used by `process` for both the `window` and the `full_capture` blocks of the answer. Pure, no errors.
Test: `gla::tests::describe_names_the_pins_the_length_and_an_overrun`.

## The per-pin summary (`stats.rs`)

### `gla_summarize` [@ANCHOR: gla_summarize]

`pub fn summarize(w: &Wave, glitch_ns: u64) -> Value`: the statistics of a capture, per pin. Called by `process` on the window.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The wave. |
| `glitch_ns` | `u64`, ns | A pulse shorter than this is a glitch (0 reports none). |

Returns `{"duration_ns", "pins": {<name>: {...}}}`. Per pin: `first_level`, `last_level` (0 or 1), `edges`, `rising`, `falling`, `high_fraction` (time high over `end_ns`; null when the capture is 0 ns long), `frequency_hz` (from the median rising-edge period; null with fewer than two rising edges), `period_spread_ns` (longest minus shortest rising-edge period, null likewise), `duty_cycle`
(time high between the first and last rising edge over that span, null likewise), `high_pulse_ns` and `low_pulse_ns` (`min` and `max` of the complete high or low pulses, null when there is none), `glitches` (`threshold_ns`, `count`, and `first`: up to 16 entries of `t_ns` in capture time including `origin_ns`, `level` 1 or 0 and `width_ns`). A pulse is the span between two consecutive changes of one pin,
so the first and last spans, cut by the ends of the capture, are not pulses. The glitch count is always complete; only the list is capped. Pins are keyed by name, so two pins with the same name collapse into one entry. Pure, no errors. Tests: `gla::tests::a_square_wave_has_its_frequency_duty_edges_and_pulse_widths_reported`,
`gla::tests::glitches_below_the_threshold_are_counted_and_listed_and_a_flat_pin_has_no_frequency`.

## Triggers (`trigger.rs`)

### `gla_trigger_enum` [@ANCHOR: gla_trigger_enum]

`pub enum Trigger` (`Clone, Debug, PartialEq`): what starts the window. The Glasgow revision C has no hardware trigger, so this is a rule applied to the recorded wave. Made by `trigger::parse`, read by `find` and `apply`, and printed with `{:?}` in the "did not occur" error.

| Variant and fields | Meaning |
|---|---|
| `Immediate` | The start of the capture (time 0). |
| `Edge { pin: usize, rising: Option<bool> }` | A change of the pin at index `pin`: `Some(true)` rising, `Some(false)` falling, `None` either. |
| `Level { pin: usize, level: bool, min_ns: u64 }` | The pin at `level` for at least `min_ns` ns (0: the moment it gets there). |
| `Pattern { mask: u32, want: u32, min_ns: u64 }` | `state & mask == want` for at least `min_ns` ns. A set bit of `mask` is a pin that is compared (0 or 1); `want` holds the wanted values on those bits. `mask` is never 0 when made by `parse`. |

No serialized form of its own (the JSON spec is described at `gla_trigger_parse`). Plain data owned by one call; no lock. Test: `gla::tests::trigger_specs_are_parsed_and_bad_ones_refused`.

### `gla_trigger_parse` [@ANCHOR: gla_trigger_parse]

`pub fn parse(w: &Wave, v: Option<&Value>) -> Result<Trigger, String>`: reads the `trigger` argument. Needs the wave to resolve pin names and to know the number of pins.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The capture, for pin names and pin count. |
| `v` | `Option<&Value>` | The spec object, or `None`/null. |

Spec forms: none, null or `{"type":"none"}` gives `Immediate`; `{"type":"edge","pin":"A0","edge":"rise"|"rising"|"fall"|"falling"|"either"}` (default `either`); `{"type":"level","pin":"A0","level":1,"min_ns":100}` (`level` is 0, 1, `false` or `true`); `{"type":"pattern","pattern":"1x0x","min_ns":0}` with one character per captured pin, first pin first: `0`, `1`, `x` or `X` for do not care.
`min_ns` (unsigned integer, default 0) is read for every type but only level and pattern use it. `pin` may be a name or a position (`gla_wave_pin_index`).
Errors: `an edge trigger needs "pin"` and the same for level; `edge "<e>" must be rise, fall or either`; `level must be 0 or 1`; `a pattern trigger needs "pattern" such as "1x0x"`; a wrong character count (names the count and the pins); `pattern character "<c>" must be 0, 1 or x`; `a pattern of only x never triggers on anything: use type none`; `trigger type "<t>" must be none, edge, level or pattern` (also for a missing `type`). Pure.
Tests: `gla::tests::trigger_specs_are_parsed_and_bad_ones_refused`, `gla::tests::edge_level_and_pattern_triggers_find_the_right_moment`.

### `gla_trigger_find` [@ANCHOR: gla_trigger_find]

`pub fn find(w: &Wave, t: &Trigger, from_ns: u64) -> Option<u64>`: the time of the first trigger at or after `from_ns`, or `None`.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The capture. |
| `t` | `&Trigger` | The trigger. |
| `from_ns` | `u64`, ns | Where to start looking (`apply` passes 0). |

`Immediate` returns `from_ns` itself. `Edge` returns the time of the first change of the pin at or after `from_ns` in the wanted direction; the initial state at time 0 is not a change. `Level` and `Pattern` use `find_state`: they fire when the condition has held for `min_ns`, and the time returned is when it began, so the pre-trigger part ends where the condition started. No errors; linear in the number of edges. Pure.
Tests: `gla::tests::edge_level_and_pattern_triggers_find_the_right_moment`, `gla::tests::the_window_keeps_the_pre_trigger_part_and_the_trigger_position`.

### `gla_trigger_find_state` [@ANCHOR: gla_trigger_find_state]

`fn find_state(w: &Wave, from_ns: u64, min_ns: u64, ok: impl Fn(u32) -> bool) -> Option<u64>` (private): the start of the first span at or after `from_ns` over which the pin-state predicate `ok` holds for at least `min_ns`. Shared by the level and pattern triggers.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The capture. |
| `from_ns` | `u64` | The earliest start; a span that began earlier and is still going on starts at `from_ns`. |
| `min_ns` | `u64` | Required length of the span. With 0, the span must still have a positive length, or be the final state of the capture. |
| `ok` | `impl Fn(u32) -> bool` | Tests a whole pin-state word. |

Walks the spans between changes from `from_ns` on; for a span where `ok` holds and whose length (to the next change, or to `end_ns` for the last) is at least `min_ns`, returns its start. Returns `None` when the changes run out first. Linear time, pure, no errors. Tests: `gla::tests::edge_level_and_pattern_triggers_find_the_right_moment`, `gla::tests::the_window_keeps_the_pre_trigger_part_and_the_trigger_position`.

### `gla_trigger_apply` [@ANCHOR: gla_trigger_apply]

`pub fn apply(w: &Wave, t: &Trigger, pre_ns: u64, post_ns: u64) -> Result<(Wave, u64), String>`: the window around the first trigger. Called by `process`.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The full capture. |
| `t` | `&Trigger` | The trigger. |
| `pre_ns`, `post_ns` | `u64`, ns | How much to keep before and after the trigger time (clamped to the capture; additions saturate). |

Returns the window (`gla_window`: `at - pre_ns` to `at + post_ns`) and the trigger's position in the window's own time (`at - from`, which is less than `pre_ns` when the trigger is early in the capture). Error: `the trigger <debug text of the trigger> did not occur in the <end_ns> ns capture`. Pure. Test: `gla::tests::the_window_keeps_the_pre_trigger_part_and_the_trigger_position`.

## The sigrok session file (`export.rs`)

### `gla_crc32` [@ANCHOR: gla_crc32]

`pub fn crc32(data: &[u8]) -> u32`: the CRC-32 of IEEE 802.3 (polynomial `0xEDB88320`, initial value and final complement `0xFFFFFFFF`), as ZIP uses. Bit by bit, no table; `crc32(b"123456789") == 0xCBF43926`. Pure, no errors, linear in the input; used by `zip_store` for every entry.
Test: `gla::tests::crc32_matches_the_check_value_and_the_zip_lists_every_entry`.

### `gla_zip_store` [@ANCHOR: gla_zip_store]

`pub fn zip_store(entries: &[(&str, Vec<u8>)]) -> Vec<u8>`: a ZIP archive of the entries, each stored without compression, with local headers, a central directory and the end record. The date and time fields are fixed at 1980-01-01 00:00 so the bytes depend only on the names and the contents. Used by `write_sr`.

| Argument | Type | Meaning |
|---|---|---|
| `entries` | `&[(&str, Vec<u8>)]`, borrowed | File name and contents, in archive order. Names are written as UTF-8 with no flag set. |

Returns the archive bytes. Header fields: version needed and made by 20, flags 0, method 0 (stored), CRC-32, sizes, name length, no extra field and no comment, disk numbers 0. No ZIP64: sizes, offsets and the entry count are written as 32-bit and 16-bit values with plain casts, so an archive over 4 GiB, more than 65,535 entries or a name over 65,535 bytes would be written wrongly and silently (not reachable with `write_sr`,
which stays far below those). No errors, pure. Test: `gla::tests::crc32_matches_the_check_value_and_the_zip_lists_every_entry` and `gla::tests::a_sigrok_session_file_has_version_metadata_and_packed_samples` (which reads the archive back).

### `gla_rate_text` [@ANCHOR: gla_rate_text]

`fn rate_text(hz: f64) -> String` (private): the sample rate as sigrok writes it in the `samplerate=` line of `metadata`. The rate is rounded to an integer; a multiple of 1,000,000 gives `<n> MHz`, else a multiple of 1000 gives `<n> kHz`, else `<n> Hz`. Examples: 1,000,000 is `1 MHz`, 250,000 is `250 kHz`, 1234 is `1234 Hz`, 2,500,000 is `2500 kHz`. A negative or NaN input becomes 0 and gives `0 MHz`.
Pure, no errors. Test: `gla::tests::a_sigrok_session_file_has_version_metadata_and_packed_samples`.

### `gla_write_sr` [@ANCHOR: gla_write_sr]

`pub fn write_sr(s: &Sampled, names: &[String]) -> Vec<u8>`: the sigrok session file (`.sr`) of a resampled capture, which PulseView and sigrok-cli open. Used by `process` for the `sr` format with `win.names`.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&Sampled` | The samples and the rate. |
| `names` | `&[String]` | One probe name per pin. At most 8 names pack into one byte per sample, 9 to 16 into two (little-endian); there is no check that the samples have no higher bits. |

Returns a ZIP (`gla_zip_store`) of: `version` (the text `2`); `metadata` (an ini text: `[global]` with `sigrok version=0.5.2`, `[device 1]` with `capturefile=logic-1`, `total probes=<n>`, `samplerate=<text>`, `total analog=0`, `probe<i>=<name>` lines and `unitsize=<1|2>`); and the samples in chunks of 1,048,576 as `logic-1-1`, `logic-1-2`, and so on (none when
there are no samples). Pure, no errors; memory about twice the size of the samples. Test: `gla::tests::a_sigrok_session_file_has_version_metadata_and_packed_samples`.

## Protocol decoders (`decode.rs`)

The decoders work on the change list, not on samples, so they are exact to the capture's timestamps. Each returns at most `MAX_OUT` = 4096 entries.

### `gla_decode_uart` [@ANCHOR: gla_decode_uart]

`pub fn uart(w: &Wave, pin: usize, baud: f64, bits: u32, parity: &str, invert: bool) -> Result<Value, String>`: decodes asynchronous serial on one pin: idle high (low with `invert`), a start bit, `bits` data bits LSB first, an optional parity bit, then at least one stop bit (only the first is checked).

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The capture. |
| `pin` | `usize` | Index of the pin; the caller must pass one below `names.len()` (the dispatcher `run` guarantees it; the function does not check it). |
| `baud` | `f64` | 50 to 3,000,000 bits per second. |
| `bits` | `u32` | 5 to 8 data bits. |
| `parity` | `&str` | `none`, `even` or `odd`. |
| `invert` | `bool` | True when the line idles low. |

Method: each change from idle to active is a candidate start; the line is read again half a bit later, and if it is idle again the candidate is a false start (counted, not decoded). Data bits are read at 1.5, 2.5, ... bit times after the start edge, then parity, then the stop bit; the next frame is looked for only from the middle of the stop bit. A frame whose stop-bit sample point lies past the end of
the capture ends decoding (`last_frame_cut_by_capture_end`). A stop bit that is not idle is a framing error; parity is wrong when the total number of ones (data plus parity bit) is odd for `even`, or even for `odd`. Returns `protocol`, `baud`, `frames` (up to 4096, each `t_ns` of the start edge in capture time, `byte`, `hex`, `framing_error`, `parity_error`), `count` (`frames.len()`), `framing_errors` and
`parity_errors` (complete counts, even beyond the 4096 listed), `false_starts`, `text` (the listed bytes with 32 to 126 as characters and anything else as `.`) and `last_frame_cut_by_capture_end`. Errors: `baud <b> must be 50 to 3000000`, `bits <n> must be 5 to 8`, `parity "<p>" must be none, even or odd`. The list is cut at 4096 without a flag. Pure. Tests: `gla::tests::uart_frames_are_decoded_with_text_parity_and_framing_errors`,
`gla::tests::a_uart_glitch_is_not_a_frame_and_an_inverted_line_decodes`.

### `gla_decode_i2c` [@ANCHOR: gla_decode_i2c]

`pub fn i2c(w: &Wave, scl: usize, sda: usize) -> Result<Value, String>`: decodes I2C. START is SDA falling while SCL is high, STOP is SDA rising while SCL is high, data is read at each SCL rising edge (the SDA value after the change), nine clocks per byte, MSB first, the ninth is the acknowledge (low is ACK). The first byte after a START is the address byte.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The capture. |
| `scl`, `sda` | `usize` | Pin indexes (valid, below the pin count); must differ. |

Returns `{"protocol":"i2c","transactions":[...],"count"}`. Each transaction has `start_ns` (capture time), `repeated_start` (true when the previous transaction ended without a STOP), `address` (first byte shifted right by one) and `address_hex` (`0x..`), `read` (bit 0 of the first byte), `address_ack`, `ten_bit` (first byte of the form `11110xx`; the 10-bit address is not decoded further: the second byte is in `data`), `data` (the remaining bytes as `hex` and `ack`), `stop` (a STOP was seen) and
`partial_bits` (clocks of an unfinished byte). A transaction still open at the end of the capture is returned with `stop` false; a START with no bytes gives null address fields. The bus is taken as idle at the start of the capture: a transaction already running is first seen at the next START. Transactions beyond 4096 are dropped without a flag (`count` is the number kept). Error: `scl and sda must be different pins`. Pure.
Test: `gla::tests::i2c_transactions_are_decoded_with_addresses_acks_repeated_starts_and_stops`.

### `gla_decode_spi` [@ANCHOR: gla_decode_spi]

`pub fn spi(w: &Wave, cs: Option<usize>, sck: usize, copi: Option<usize>, cipo: Option<usize>, mode: u8, bits: u32, msb_first: bool, cs_active_low: bool) -> Result<Value, String>`: decodes SPI.

| Argument | Type | Meaning |
|---|---|---|
| `w` | `&Wave` | The capture. |
| `cs` | `Option<usize>` | Chip select pin; `None` means always selected, one transfer from time 0. |
| `sck` | `usize` | Clock pin. |
| `copi`, `cipo` | `Option<usize>` | Controller-out and peripheral-out data pins; at least one is required. |
| `mode` | `u8` | 0 to 3. Modes 0 and 3 take a word bit at each SCK rising edge, modes 1 and 2 at each falling edge; the clock idle level is not checked. |
| `bits` | `u32` | Word size, 4 to 32. |
| `msb_first` | `bool` | Bit order within a word. |
| `cs_active_low` | `bool` | True when CS is asserted low. |

A data pin is read as its state just before the sampling edge. A transfer runs from CS assertion to deassertion (or from time 0 when CS is already asserted); a clock edge at the instant of deassertion is not sampled. Returns `protocol`, `mode`, `bits`, `transfers` (at most 4096; each `start_ns` in capture time, `words` count, `copi` and `cipo` as lists of hex strings `0x..` padded to `ceil(bits/4)` digits or null when that pin was not given, and `extra_bits` left over) and `count` (all transfers found, which can exceed the number listed).
A CS assertion with no clocks gives a transfer with 0 words. Errors: `mode <m> must be 0 to 3`, `bits <n> must be 4 to 32`, `give at least one of copi and cipo`. Pure. Test: `gla::tests::spi_words_are_decoded_in_every_mode_and_bit_order`.

### `gla_decode_run` [@ANCHOR: gla_decode_run]

`pub fn run(w: &Wave, spec: &Value) -> Result<Value, String>`: runs the decoder that the `decode` argument asks for. Called by `process`.

| Spec key | Meaning and default |
|---|---|
| `type` | `uart`, `i2c` or `spi` (required). |
| uart: `pin` (required), `baud` (115200), `bits` (8), `parity` (`none`), `invert` (false) | See `gla_decode_uart`. |
| i2c: `scl`, `sda` (both required) | See `gla_decode_i2c`. |
| spi: `sck` (required); `cs`, `copi`, `cipo` (optional, null counts as absent), `mode` (0), `bits` (8), `msb_first` (true), `cs_active_low` (true) | See `gla_decode_spi`. |

Pins are names or positions (`gla_wave_pin_index`). Numbers are read as floats and cut to integers (`bits` 8.9 is 8; a negative number becomes 0 and is then refused by the decoder). A value of the wrong JSON type for `baud`, `bits`, `mode`, `parity` or a flag is refused (`decode <key> must be a number`, `... true or false`, `... text`); null counts as absent. Errors: `decode needs "<key>"` for a missing required pin; the pin errors; `decode type <t> must be uart, i2c or spi`; and those of the decoders.
Pure. Test: `gla::tests::the_decode_argument_picks_the_decoder_and_reads_pins_by_name`.

## The pattern planner (`pattern.rs`)

The planner turns a request into steps of (pin mask, hold in milliseconds) and says what will really come out. The backend plays holds timed from the host: 0.5 ms shortest, about 1 ms of jitter, so the planner refuses what that cannot do instead of approximating it. It drives nothing itself. Limits (public constants):
`MIN_HOLD_MS` 0.5, `MAX_HOLD_MS` 5000, `MAX_STEPS` 256 per round, `MAX_REPEAT` 100,000 rounds.

### `gla_pattern_plan_struct` [@ANCHOR: gla_pattern_plan_struct]

`pub struct Plan` (`Clone, Debug, PartialEq`): a planned pattern, made by `plan`, read by `backend_args` and by `pattern_tool`.

| Field | Type | Meaning |
|---|---|---|
| `steps` | `Vec<(u32, f64)>` | One round: each entry is a pin mask (bit n is the nth driven pin, 1 high, 0 low) and a hold in milliseconds (0.5 to 5000, already rounded to 0.1 ms). 1 to 256 entries. |
| `repeat` | `u64` | How many times the round is played, 1 to 100,000. |
| `total_s` | `f64`, seconds | Sum of the holds times `repeat`, over 1000; never above the limit given to `plan`. |
| `achieved` | `Value` (JSON object) | What will really come out: for square and pwm `frequency_hz`, `duty`, `high_ms`, `low_ms`, `cycles`; for pulses `width_ms`, `gap_ms`, `pulses`; for bits `bits`, `bit_ms`, `bit_rate_hz`, `repeat`; for steps `steps`, `repeat`. |

Owned by one call; no serialized form of its own (`pattern_tool` returns its parts, `backend_args` the steps and repeat). No concurrency. Test: `gla::tests::a_square_wave_is_planned_with_its_real_frequency_and_duty`.

### `gla_pattern_quantize` [@ANCHOR: gla_pattern_quantize]

`fn q(ms: f64) -> f64` (private): rounds a time in milliseconds to the nearest 0.1 ms (`(ms * 10).round() / 10`), the resolution at which the planner reports what it will play. Used by `hold`. Pure. Test: `gla::tests::a_square_wave_is_planned_with_its_real_frequency_and_duty`.

### `gla_pattern_hold` [@ANCHOR: gla_pattern_hold]

`fn hold(ms: f64, what: &str) -> Result<f64, String>` (private): checks one hold time and returns it quantized (`gla_pattern_quantize`). `what` names the quantity in the error. Accepts holds that are finite and, after rounding, from `MIN_HOLD_MS` to `MAX_HOLD_MS`. Error: `<what> is <ms> ms; the host-timed generator plays holds of 0.5 to 5000 ms (a square wave up to about 1 kHz at 50 %). Faster edges need the FPGA-timed generator, which this tool does not have`.
Pure. Test: `gla::tests::a_square_wave_is_planned_with_its_real_frequency_and_duty`, `gla::tests::pulse_trains_bit_patterns_and_raw_steps_are_planned`.

### `gla_pattern_num` [@ANCHOR: gla_pattern_num]

`fn num(spec: &Value, key: &str, lo: f64, hi: f64, def: Option<f64>) -> Result<f64, String>` (private): reads a number from the pattern spec.

| Argument | Type | Meaning |
|---|---|---|
| `spec` | `&Value` | The object to read. |
| `key` | `&str` | The member. |
| `lo`, `hi` | `f64` | Inclusive range. |
| `def` | `Option<f64>` | The default when absent; `None` makes the member required. |

Returns the value when finite and in range. Errors: `<key> <v> must be <lo> to <hi>`; `<key> is required`. A member that is present but not a number (a string, for example) counts as absent, so the default is used or `is required` is returned. Pure. Test: `gla::tests::a_square_wave_is_planned_with_its_real_frequency_and_duty`, `gla::tests::pulse_trains_bit_patterns_and_raw_steps_are_planned`.

### `gla_pattern_plan` [@ANCHOR: gla_pattern_plan]

`pub fn plan(spec: &Value, npins: usize, max_total_s: f64) -> Result<Plan, String>`: plans the pattern of `spec` for `npins` driven pins, at most `max_total_s` seconds in all. Called by `pattern_tool` with the config's `max_capture_s`. Pins other than the chosen `pin` stay low.

| Argument | Type | Meaning |
|---|---|---|
| `spec` | `&Value` | The pattern: `type` (required) is one of the forms below. |
| `npins` | `usize` | How many pins are driven (1 to 8); `pin` must be lower. |
| `max_total_s` | `f64`, s | Longest total run. |

| `type` | Members (range, default) |
|---|---|
| `square`, `pwm` | `pin` (0, default 0), `frequency_hz` (0.1 to 2000, required), `duty` (0 to 1, default 0.5), and `cycles` (1 to 100,000) or else `duration_s` (0.001 to `max_total_s`; required then). The high and low times (`1000/f` times duty and one minus duty) must each pass `hold`, so a duty of 0 or 1 is refused and the highest usable frequency is about 1 kHz at 50 %. With `duration_s` the cycle count is the number of whole cycles that fit, at least 1. `square` and `pwm` are the same code. |
| `pulses` | `pin`, `width_ms`, `gap_ms` (each through `hold`, required), `count` (1 to 100,000, required). |
| `bits` | `pin`, `bits` (text of 1 to 256 `0`/`1` characters, required), `bit_ms` (0.5 to 5000, required), `repeat` (1 to 100,000, default 1). Neighbouring equal bits are merged into one hold while it stays within 5000 ms. |
| `steps` | `steps`, a list of `{value, hold_ms}` (`value` 0 to 2^npins - 1, `hold_ms` through `hold`), `repeat` (default 1). |

Errors: `type is required: square, pwm, pulses, bits or steps`; `type "<t>" must be square, pwm, pulses, bits or steps`; `pin <p> is outside the <n> driven pins (0 is the first)`; member errors from `num` and `hold`; `bits is required: text such as "10110010"`; `bits must be 1 to 256 characters of 0 and 1`; `steps is required: a list of {value, hold_ms}`;
`<n> steps; a round holds 1 to 256`; `the pattern would run <t> s; the limit is <max> s (the config's max_capture_s)`. Pure. Example: `{"type":"square","frequency_hz":100,"duration_s":1}` plans 100 cycles of 5 ms high and 5 ms low. Tests: `gla::tests::a_square_wave_is_planned_with_its_real_frequency_and_duty`, `gla::tests::pulse_trains_bit_patterns_and_raw_steps_are_planned`.

### `gla_pattern_backend_args` [@ANCHOR: gla_pattern_backend_args]

`pub fn backend_args(p: &Plan, pins: &str) -> Value`: the arguments for the backend tool `bench_glasgow_pattern`: `{"pins": <pins>, "steps": [{"value": <mask>, "hold_ms": <ms>}, ...], "repeat": <n>}`. `confirm`, `dry_run` and `instrument` are not set here; `pattern_tool` copies them from the caller's arguments, so the backend sees the caller's own `confirm`.
`pins` is passed as given (it was already checked by `pin_names`). Pure, no errors. Test: `gla::tests::the_backend_arguments_carry_pins_steps_and_repeat`.
