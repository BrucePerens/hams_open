---
title: Anatomy of an instrument module
summary: Files, registration, config, tool naming, arguments, errors, timeouts, locking and the escalation ladder for a flaky link.
see_also: [concepts/extending, concepts/errors, concepts/several-instruments, concepts/units-and-conventions]
verified: code only, 2026-10-29
---

# Anatomy of an instrument module

The working model is `src/example/` (read `src/example/mod.rs` top to bottom; about 450 lines with tests). Other instruments: `src/usb_hub/` and `src/audio/` (multi-file modules with their own tool lists), `src/scope.rs` and `src/psu.rs` (SCPI over `src/link.rs`).

## Files a new instrument adds

| Path | Content |
|---|---|
| `src/<name>/mod.rs` | `Config` and `parse_config`, `SPECS` (tool name, tier, description, JSON-schema properties), `tool_defs`, `call_with`, the tool functions, unit tests |
| `src/<name>/testkit.rs` | `#[cfg(test)]` fake device, replay of a recorded session |
| `tests/<name>_fixtures/*.txt` | scrubbed recordings (`> ` host to device, `< ` device to host) |
| `tests/hil_<name>.rs` | `#[ignore]` hardware check the owner runs |
| `manual/instruments/<name>/` | index, overview, setup, operation, limits, troubleshooting, protocol |
| `manual/tools/bench_<name>_<verb>.md` | one page per tool |
| `README.md`, `manual/whats-new.md` | a config example and a driver row; a line under the current build version |

## Registration: the shared edits

`python3 tools/new_instrument.py <name>` copies the skeleton and prints these. In `src/config.rs`: a row in `DRIVER_KINDS` (driver name and `Kind`; the table is alphabetical), the `Kind` variant, a field of `Instrument` holding your `Config`, that field built with `module_config!(<name>, driver_name, c, name)` (it calls your `parse_config` only for your driver, so a bad entry fails at load, not at first use), and a host or serial presence check if your link needs one. In `src/main.rs`: `mod <name>;` and one row in `MODULES` (alphabetical): the tool prefix `bench_<name>_`, your `call` and `tool_defs`, and a sample instrument entry so that the manual tests see your tools. `connect()` needs no edit, because only the SCPI drivers use it. Keep each edit to a single line; do not reformat or reorder neighbouring code. Rows are alphabetical so that two drivers added at once edit different lines.

A tool is listed only when an instrument of your kind is configured at or above the tool's tier, and is refused again at call time. A tool that applies to several kinds of instrument (such as `bench_identify`) is a shared tool: prefer your own `bench_<name>_*` tool unless the meaning is identical.

## Config

An entry under `instruments` in the file named by `BENCH_MCP_CONFIG`: `name`, `driver`, `tier` (default `read_only`), the connection (`host` and `port`, or `serial` and `baud`, or a device path or serial number for USB), the limits (`max_*` in SI units with the unit in the key), and the reserved-device list when the instrument shares hardware with others. Validate every value in `parse_config`; refuse unknown ranges, NaN and infinities, and return a message naming the key. Defaults are the safest values. Give a full example entry in `setup` and in the README using documentation addresses (192.0.2.x), never the owner's.

## Tool names, arguments and returns

* `bench_<instrument>_<verb>` in lower case (the checklist test enforces the shape). One verb per tool; no "mode" arguments that change a read into a write.
* Every instrument tool takes an optional `instrument` argument (the alias); optional when exactly one instrument fits.
* SI units, with the unit in the name: `freq_hz`, `level_v`, `pulse_s`, `power_dbm`. Give the range and default in the schema `description`. Reject out-of-range values before sending a byte; the error names the limit and where it comes from (the rating, the owner's config, the driver).
* Return numbers with units in the key, not strings with units. Do not return a bare `ok: true`; return what the instrument reports after the change.
* A measurement is never summarised as a judgement ("good", `passed: true`). Return the values and let the caller compare.
* Errors are structured text: what failed, the cause as far as known, and a recommended next action. The model reading it should know what to do.

## Timeouts, retries, cancellation

* Every I/O call has a bounded timeout (`Link::set_timeout_secs`; never leave the default); nothing blocks forever. Count the worst case: tries times timeout must be within a minute for a read tool.
* Bounded retries on reads only (`ask()` in the example). A command that changes the instrument is not repeated blind; read back and compare instead.
* A tool that waits (a pulse, a sweep, a soak) has a hard maximum and checks it in a loop; the tool call ends when the maximum is reached whatever the instrument does.
* An abandoned call must leave the instrument safe: use the instrument's own time limit where it has one.

## Concurrency

The server serializes calls to one instrument by name (an `instrument_lock`), so the single conversation of a serial port or socket is safe; calls to different instruments run in parallel and must not depend on each other. If your hardware is a USB device that another module also opens (sound card, SDR), say which is exclusive and which is shared, and register it as reserved. A tool that uses another instrument (a scope to check a generator) goes in a procedure page, not in the driver.

## Reserved devices

List the identifiers (USB vid:pid, ALSA card id, node name) of hardware that your driver owns, in the config reserved list, and make other modules refuse them: see [reserved devices](concepts/reserved-devices) and the [standing rules](concepts/extending/standing-rules).

## The flaky link: the escalation ladder

Order for every failure of the link, with the instrument never returning a silently wrong number: (1) retry the read, bounded; (2) software reset of the link or device if the tools allow (`bench_usb_reset`); (3) power-cycle through the hub tools (`bench_usb_port_power`, only on a port the owner has marked) ; (4) a clear error saying what a person must do. Each rung is a separate decision in your code and in the troubleshooting page. A value from before a failure is never reused. A reply that does not parse, is not finite, or is out of the instrument's range is an error.
