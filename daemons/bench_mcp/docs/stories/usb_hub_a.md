# The USB Hub Tools Find a Device, Say What Is Safe to Touch, and Switch a Port Only When It Is Allowed

As the person (or the AI agent) running the bench, I want `bench_mcp` to show me the USB tree, tell me which device is `/dev/ttyUSB0`, report
whether a hub can really switch one port, and then reset, deauthorize, unbind or power-cycle a stuck device, while refusing to touch the root disk,
the keyboard, the bench instruments' own adapters or a hub whose uplink would take down everything below it. This story documents the part of the
`usb_hub` driver that does that: the module root and its config and tool table (`daemons/bench_mcp/src/usb_hub/mod.rs`), the hub report, port
naming and the `bench_usb_port_power` tool (`hubs.rs`), the escalation ladder below a power cycle (`ladder.rs`), the port power cycle itself
(`power.rs`) and the protection list (`protect.rs`). The tool pages for the agent are in `docs/USB_HUB_TOOLS.md`; the sysfs reader, the `uhubctl`
parser, the root helper, history, bandwidth and diagnose are other stories.

None of these functions keys a transmitter: they read sysfs and `/proc/mounts`, and run `sudo -n uhubctl` and this binary's `--usb-priv` helper
through `sudo -n`. The safety that applies is the tier of the `usb_hub` instrument (`read_only`, `read_config`, `full`), the protection list and the
`dry_run` / `override_protection` arguments; every entry says which of them it touches. Nothing here has a band check or a TX gate.

## The module root: config, instrument choice, topology, find and dispatch

### `port_key` [@ANCHOR: bench_mcp_usb_hub_port_key]

Checks that a key of the `ports` map in the config is spelled like a hub port, so that a typo is caught when the config is loaded and not when a
tool silently finds no port. Called once per key by `parse_config`.

| Argument | Type | Meaning |
|---|---|---|
| `k` | `&str`, borrowed | The map key, for example `"3-2/2"` (hub path, slash, port number) or `"usb3/1"` (a root hub and its port). |

Return value: `Ok(())` when the text before the first `/` is not empty and starts with `usb` or contains a `-`, and the text after it is not
empty and is all ASCII digits. A second slash makes the port part fail the digit test. The port number is not range checked here (0 and 99 pass),
and nothing checks that the hub exists in the tree; that happens when a tool uses the key. `Err(String)` is `port key "<k>" must look like "3-2/2"
(hub path, slash, port number)`, which `parse_config` returns to the config loader (the daemon refuses to start with it, prefixed with the
instrument name). Pure: no I/O, no locking. Example: `"3-2.2/1"` passes, `"3/1"`, `"3-2/"` and `"3-2/a"` fail. See also: `usb_hub_config`; the
unit test `parse_config` cases cite it.

### `parse_config` [@ANCHOR: usb_hub_config]

Reads the settings of one `usb_hub` instrument entry of the bench config (`"usb": {"driver": "usb_hub", "tier": "...", ...}`) into a `Config`. It
exists so the labels an operator gives ports ("VNWA 3E"), the ports and device ids that must never be switched, and the manual verification
evidence are validated once, at load, and then used by every tool. It is called by `config.rs` for every instrument of kind `usb_hub`, with the
error prefixed by the instrument name.

| Argument | Type | Meaning |
|---|---|---|
| `c` | `&serde_json::Value`, borrowed | The instrument's JSON object. Only the keys `ports` and `critical_devices` are read; others are ignored here. |

The data it builds:

| Struct field | Type | Meaning, default, who reads it |
|---|---|---|
| `Config.ports` | `BTreeMap<String, PortCfg>` | Key `"hub/port"` (see `port_key`) to that port's settings. Empty when `ports` is absent. Read by every tool for labels, `resolve_device`, `resolve_port`, `protect` and `verification`. |
| `Config.critical_devices` | `Vec<String>` | Lower-case `vid:pid` strings, nine characters, for example `0451:2036`. Devices with these ids are protected wherever they are plugged in. Read by `protect::critical_set`. |
| `PortCfg.label` | `Option<String>` | The operator's name for the port, unique case-insensitively; usable as the `label` or `device` argument. |
| `PortCfg.critical` | `bool`, default `false` | The port, and whatever is on it, is protected and the protection cannot be overridden by an argument. |
| `PortCfg.hub_uplink_ok` | `bool`, default `false` | A hub plugged into this port may be switched or reset although everything below it goes too. |
| `PortCfg.confirmed_by_hand` | `Option<String>` | Free text: the operator saw the device go dark and lose its settings. Raises verification to level 2. |
| `PortCfg.state_cleared` | `Option<String>` | A device specific check that the device loses its settings. Level 3. |
| `PortCfg.vbus_measured` | `Option<String>` | A manual VBUS measurement. Level 4. |

Return value: `Ok(Config)`; an absent `ports` and `critical_devices` give an empty `Config`. A text field of the wrong type is read as absent and a
flag of the wrong type as `false` (no error); a port value that is not an object gives a port with every default. Errors, all `Err(String)`:
`ports must be an object`; any message of `port_key`; `port <k>: label is empty` (an empty or all-blank label); `label "<l>" is on both <other>
and <k>` (labels compare without regard to case); `critical_devices must be a list of "vid:pid"`; and `critical_devices entry <x> is not a "vid:pid"
like "0451:2036"` (not nine characters, no colon in position five, or a non-hex digit). Entries are stored lower-cased. Pure: no files, no locks.
See also: the `usb_hub_config` test, `bench_mcp_usb_hub_port_key`, `bench_mcp_usb_hub_hubs_hub_json` (shows the labels).

### `instrument` [@ANCHOR: bench_mcp_usb_hub_instrument]

Chooses which configured `usb_hub` instrument a tool call is about. Most benches have one, and then the `instrument` argument may be left out; with
several, the caller must name one. `call_with` calls it for every tool call before looking at the tier. It does not build an instrument from a
config entry (the README line says so; the config was built earlier by `parse_config`).

| Argument | Type | Meaning |
|---|---|---|
| `insts` | `&[Instrument]`, borrowed | Every instrument of the bench config; only those of kind `UsbHub` count. |
| `args` | `&Value`, borrowed | The tool arguments; `instrument` (string) names one. |

Return value: `Ok(&Instrument)` borrowed from `insts`. With a name given, the one with that exact (case-sensitive) name. Without one, the only
`usb_hub` instrument. Errors: `no usb_hub instrument named "<n>"` (also when the name belongs to an instrument of another kind), `no usb_hub
instrument is configured`, and `name the instrument: a, b` when there are several and none was named. A non-string `instrument` is ignored as if
absent. Pure: no I/O, no locking. See also: `bench_mcp_usb_hub_call_with`.

### `resolve_device` [@ANCHOR: bench_mcp_usb_hub_resolve_device]

Turns the text of a `device` argument into the path of a device in the USB tree. The tools accept either a kernel path (`3-2.2.1`, or `usb3` for a
root hub) or the operator's port label, so an agent can say "the VNWA" without knowing the topology. Called by `topology`, `resolve_port`, the
ladder's `target`, and the other tools that take `device`.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&sysfs::Tree`, borrowed | The USB tree read from sysfs for this call. |
| `cfg` | `&Config`, borrowed | The instrument's config, for the labels. |
| `s` | `&str`, borrowed | A device path or a label. Labels match without regard to case; paths must match exactly. |

Return value: `Ok(path)`, owned. A path found in the tree wins over a label of the same text. A label is turned into the child path of its
`hub/port` key (`sysfs::child_path`). Errors: `port <key> is labelled "<s>" but nothing is on it now` (the label exists, the port is empty), and
`"<s>" is neither a device path in the tree nor a port label`. Pure over the tree it is given: no I/O, no locking, no instrument contact. See also:
the `resolve_device` test, `bench_mcp_usb_hub_hubs_resolve_port`.

### `topology` [@ANCHOR: bench_mcp_usb_hub_topology]

The implementation of `bench_usb_topology`: the USB tree as nested JSON (or a flat list), optionally one device and everything below it, or one
bus. It answers "what is plugged in where, at what speed, with which drivers and device nodes". Read only (tier `read_only`); no sudo.

| Argument (JSON) | Type | Meaning |
|---|---|---|
| `root` (Rust) | `&Path` | The sysfs root, `/sys` in production, a copy in tests. |
| `cfg` | `&Config` | For port labels in the output. |
| `device` | string, optional | A device path or a label (via `resolve_device`): returns that device and what is below it. |
| `bus` | integer, optional | Only the root hub of this bus number. Ignored when `device` is given. |
| `flat` | boolean, default `false` | A JSON list of device objects instead of nested trees. Any other type reads as `false`. |

Return value: `Ok(Value)`: a JSON array of tree objects, one per root hub (nested under `children`), or, with `device` and not `flat`, one tree object,
or with `flat`, an array of device objects (`sysfs::device_json`). Errors: whatever `sysfs::Tree::read` returns when sysfs cannot be read, any error
of `resolve_device`, and `no USB bus <n>` when `bus` names none (`no USB bus 0` when the filter yields nothing and no bus was given). It reads
files under `/sys/bus/usb` only; it takes no lock and runs no program. See also: the `topology` tests, `bench_usb_find`.

### `find` [@ANCHOR: bench_mcp_usb_hub_find]

The implementation of `bench_usb_find`: answers "which USB device is `/dev/ttyUSB0`", "where is the CODEC sound card", "which port is the device
labelled spare". Read only; no sudo; reads sysfs.

| Argument (JSON) | Type | Meaning |
|---|---|---|
| `node` | string | A device node name or id: `/dev/ttyUSB0`, `ttyACM1`, `hidraw3`, `eth1`, a sound card id, `sdb`. The `/dev/` and `input/` prefixes are stripped; compared case-insensitively against the node name and node id of every interface. |
| `vidpid` | string | Exact `vid:pid`, case-insensitive. |
| `product` | string | Case-insensitive substring of the product string. |
| `label` | string | Case-insensitive substring of the label of the port the device is on. |

At least one of the four is required; every one given must match (they are ANDed). Return value: `Ok` array, one object per match, in the order of
`Tree::sorted`, with `device` (path), `label`, `vidpid`, `product`, `bus`, `devnum`, `chain` (the path from the root hub down to the device) and
`nodes` (for a `node` search, the interface name, driver, node name, class and sysfs path that matched). Errors: `give node, vidpid, product or
label` (none given), `nothing in the USB tree matches` (no hit), and a sysfs read error. Pure reads, no locking. See also: the `find` test.

### `call_with` [@ANCHOR: bench_mcp_usb_hub_call_with]

The dispatcher behind every `bench_usb_*` tool: it finds the tool in `SPECS`, picks the instrument, checks the tier, and calls the function that
implements the tool. `call` wraps it with the production program runner (`uhubctl::ProcessRunner`), `/sys` and `/proc`; `main.rs` registers `call`
under the prefix `bench_usb_`. Tests call `call_with` with a recorded sysfs tree, a fake runner and a fake `/proc`.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn uhubctl::Runner`, borrowed | Runs `sudo`, `uhubctl`, `lsusb`, `journalctl` and the root helper. Fake in tests. |
| `root` | `&Path` | The sysfs root. |
| `proc_root` | `&Path` | The procfs root, read for `mounts` by the protection list. |
| `insts` | `&[Instrument]` | All instruments of the config; the bench entries become the protection list's `BenchRef`s. |
| `name` | `&str` | The tool name, for example `bench_usb_reset`. |
| `args` | `&Value` | The tool arguments as sent by the client. |

Return value: `Ok(Value)` from the tool. Errors: `unknown tool <name>`, any error of `instrument`, `<name> needs tier <T>; "<inst>" is configured at <T>`
when the instrument's tier is below the tool's (read_only: topology, find, hubs, protection, history, watch, bandwidth, diagnose; read_config:
reset, authorize, driver, autosuspend, and port_power, which itself requires `full` for `off` and `cycle`), and `<name> does not apply to "<inst>"`
for a name in `SPECS` without a branch. It adds no locking itself; `port_power` serialises switching. Whether a call changes hardware depends on
the tool: only reset, authorize, driver, autosuspend and port_power do, through `sudo -n`. See also: `bench_mcp_usb_hub_instrument`, the dispatch
tests.

## Hubs, port naming and `bench_usb_port_power`

### `verification` [@ANCHOR: usb_hub_verification_levels]

Works out how well it is known that switching one port really removes the device's power. Software cannot see VBUS, so the output gives a level and
the evidence for each statement, and says plainly what each level does not show. It is called by `hub_json` for every port of `bench_usb_hubs` and by
`port_power` after a cycle.

| Argument | Type | Meaning |
|---|---|---|
| `cfg` | `&Config` | Source of `confirmed_by_hand`, `state_cleared` and `vbus_measured` for the port. |
| `key` | `&str` | The port key `hub/port`. |
| `claimed` | `bool` | Whether `uhubctl` lists the hub as switching ports individually. |

Levels (the maximum of what applies): 0 `not switchable by uhubctl`; 1 `claimed` (`claimed` is true); 2 `re-enumerated` (this server process cycled the
port and saw the device leave and return with a new device number, from `power::proven`, or `confirmed_by_hand` is set in the config); 3 `state
cleared` (`state_cleared` set); 4 `VBUS measured` (`vbus_measured` set). Level 3 or 4 from the config is accepted without level 2 or 1: the config is
taken at its word. Return value: a JSON object `{level, name, evidence[], note}`; `evidence` has one sentence per applicable level, with the measured
times and device numbers for level 2. The `note` says that level 2 shows the port dropped the device and not that VBUS fell, and only level 4 is a
VBUS measurement. No errors. It reads the process-wide proof store of `power.rs` (a mutex held for a moment); no files, no hardware. See also: the
`verification_levels` test, `bench_mcp_usb_hub_power_report_level`.

### `hub_json` [@ANCHOR: bench_mcp_usb_hub_hubs_hub_json]

Builds the JSON object for one hub in `bench_usb_hubs`: its sysfs facts, what `uhubctl` says about its switching, the hub class descriptor and one
entry per port. Called by `hubs` for each hub.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree` | The tree, for the count of devices below the hub. |
| `cfg` | `&Config` | Labels and verification evidence. |
| `d` | `&Device` | The hub. |
| `u` | `Option<&UHub>` | The hub as `uhubctl` listed it (location, switching mode, per-port status), if it did. |
| `desc` | `Option<&Result<HubDescriptor, String>>` | The `lsusb -v` class descriptor, `None` when `privileged: false`. |
| `ur` | `&Result<Vec<UHub>, String>` | The `uhubctl` result, to tell "not listed" from "could not be run". |

Return value: an object with `hub`, `label`, `uhubctl_location`, `vidpid`, `product`, `usb_version`, `speed`, `bus`, `devnum`, `ports`, `root_hub`,
`bus_powered`, `tt`, `uplink` (`{hub, port}` or null), `controller` (`{pci, driver}`), `uhubctl_power_switching` (the switching mode name, or `not
listed by uhubctl` or `unknown (uhubctl could not be run)`), `hub_descriptor`, `port_status` and `devices_below`. Each port entry gives the port
number, label, child device, sysfs state, connect type, over-current count, software-disabled flag, peer port (the USB 2 or USB 3 twin), the decoded
status bits, the downstream hub's switching text, `power_switching` (`per-port` only when uhubctl says per-port) and `power_control_verification`
(see `verification`). The descriptor block is `{unavailable: <error>}` when `lsusb` failed, null when not asked for, and carries the note that these are
the hub's own claims; `tt_think_time_fs_bits` is null for a USB 3 hub (5000 Mbit/s or more). No errors, no I/O, no locks. See also: the
`hub_json` test.

### `hubs` [@ANCHOR: bench_mcp_usb_hub_hubs_hubs]

The implementation of `bench_usb_hubs` (tier `read_only`): every hub with its capabilities and per-port status. It reads sysfs, and by default runs
`sudo -n uhubctl` and `sudo -n lsusb -v` for the parts only root can read.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Runner` | Runs the sudo commands. |
| `sysfs_root` | `&Path` | The sysfs root. |
| `cfg` | `&Config` | Labels, evidence. |
| `args.hub` | string, optional | Only the hub with exactly this path (for example `3-2`). |
| `args.privileged` | boolean, default `true` | `false` runs no sudo; the `uhubctl` part then says `available: false` with `not asked for`. A non-boolean reads as the default. |

Return value: `{hubs: [...], uhubctl: {available, hubs_listed | error}, needs_sudo: <explanation>}`. A sudo failure never fails the call: the
sysfs facts are returned and the failure text is in `uhubctl.error` and in each hub's `hub_descriptor.unavailable`. Error: `no hub <only>` (or `no
hub found`) when no hub matches, and any sysfs read error. Side effects: reads files; with `privileged` it starts `sudo -n uhubctl` and one `sudo
-n lsusb -v` per hub (read only commands: nothing is switched). No lock held. See also: the `hubs` test, `bench_mcp_usb_hub_hubs_hub_json`.

### `resolve_port` [@ANCHOR: bench_mcp_usb_hub_hubs_resolve_port]

Turns the arguments of a port tool into a `PortRef`: a hub path, a port number, and whether `uhubctl` needs `-e` for it. A port can be named three
ways, and this is the one place that decides which wins. Called by `port_power` and `protection`.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree` | The tree, to find the parent port of a device and the speed of a hub. |
| `cfg` | `&Config` | Labels. |
| `args.label` | string | A port label (case-insensitive). Wins over the others. |
| `args.device` | string | A device path or label; the port is the one the device is plugged into. |
| `args.hub` + `args.port` | string + integer | The hub path and the port number; both needed. |

Return value: `PortRef {hub, port, exact}`; `exact` is true when the hub runs at 5000 Mbit/s or more (a SuperSpeed hub, for which `uhubctl` 2.6.0 does
not find the location without `-e`). Errors: `no port is labelled "<l>"`, any error of `resolve_device`, `no such device`, `<path> is a root hub and
has no port`, `port is too large` (above `u32`), and `name the port: hub and port (...), or label, or device`. Note that a label is looked up in the
config alone, so a label whose port is empty still resolves to the port. The port number is not range checked here; `plan` does that. Pure over the tree.

### `secs` [@ANCHOR: bench_mcp_usb_hub_hubs_secs]

Reads an optional number-of-seconds argument with a default and a range, for `off_time_s` and `back_timeout_s`. It rejects instead of clamping, so
a request for a 300 s power-off does not quietly become 60 s.

| Argument | Type | Meaning |
|---|---|---|
| `args`, `key` | `&Value`, `&str` | The arguments and the name of the field. |
| `default` | `Duration` | Returned when the key is absent or null. |
| `min`, `max` | `Duration` | Inclusive limits; for `off_time_s` 1 s to 60 s, for `back_timeout_s` 5 s to 300 s. |

Return value: `Ok(Duration)`. Fractions are kept (`2.5` is 2.5 s); negative numbers are raised to zero and then fail the minimum. Errors: `<key> must be a
number of seconds` (a string, or a non-finite number), and `<key> must be between <min> and <max> seconds`. Pure. See also: the `secs` test.

### `flag` [@ANCHOR: bench_mcp_usb_hub_hubs_flag]

Reads an optional boolean argument that defaults to `false`: `dry_run` and `override_protection`. It exists so a string `"true"` is an error and
does not silently mean "no", which for `override_protection` would be a refusal anyway but for `dry_run` would make a real change from a request the
caller thought was a rehearsal.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The tool arguments. |
| `key` | `&str` | The field name. |

Return value: `Ok(false)` when absent or null, `Ok(b)` for a boolean. Error: `<key> must be true or false` for any other type. Pure; also used by the
ladder (`target`, `driver`). See also: the `flag` test.

### `plan` [@ANCHOR: bench_mcp_usb_hub_hubs_plan]

Works out, before anything is touched, everything a port action needs to know: who is affected, whether protection allows it, and whether the hub can
switch one port alone. `port_power` uses it for both the dry run and the real action, so the rehearsal and the action cannot disagree. Result type
`Plan {affected: Vec<String>, verdict: protect::Verdict, switching: Result<(), String>}`; `affected` is the device on the port and everything below it
(device paths), empty if the port is empty; `switching` is `Ok` only when `uhubctl` lists the hub as per-port switching.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree` | The tree. |
| `cfg` | `&Config` | Protection flags. |
| `crit` | `&protect::Critical` | The protection set built for this call. |
| `run` | `&dyn Runner` | Runs `sudo -n uhubctl` (read only status listing). |
| `port` | `&PortRef` | The port. |

Errors (`Err`, the action cannot even be described): `no hub <h> in the USB tree`, and `<h> has <n> ports; <p> is not one of them` (not a hub, port 0 or
above its port count). A `uhubctl` problem does not fail `plan`: it is placed in `switching` as `Err`: the `uhubctl` error text, `uhubctl does not list
hub <h> as a switchable hub, so its ports cannot be switched`, or `hub <h> switches power <mode> according to uhubctl, not per port: ...` (ganged
hubs refused so one port is never switched with its neighbours). It runs `sudo -n uhubctl` once. No lock; changes nothing. See also: the `plan`
tests, `bench_mcp_usb_hub_protect_check_port`.

### `port_power` [@ANCHOR: usb_hub_port_power]

The implementation of `bench_usb_port_power`: switch one hub port off, on, or cycle it (off, wait, on) through `sudo -n uhubctl`. This is the only
tool that removes power from a device; it is why the protection list and the tier exist. Callers: `call_with`.

| Argument | Type | Meaning |
|---|---|---|
| `run`, `sysfs_root`, `proc_root` | as in `call_with` | Program runner, sysfs and procfs roots. |
| `cfg` | `&Config` | Labels and flags. |
| `bench` | `&[BenchRef]` | Bench instruments, for protection (`bench_refs`). |
| `tier` | `Tier` | The instrument's tier. |
| `args.action` | string, required | `off`, `on` or `cycle`. |
| `args.hub`/`port`/`label`/`device` | | The port, see `resolve_port`. |
| `args.off_time_s` | number, default 5, 1 to 60 | A cycle's minimum time off; the report gives the actual time. |
| `args.back_timeout_s` | number, default 45, 5 to 300 | How long to wait for the device to enumerate again after power-on. |
| `args.expect_vidpid` | string, optional | The `vid:pid` that must come back (lower-cased); the device's own by default. |
| `args.dry_run` | boolean | Describe and change nothing. |
| `args.override_protection` | boolean | Overrides only overridable reasons of the protection verdict. |

Tier rules: `off` and `cycle` need `full`; `on` needs only `read_config` (so a port left off can always be switched back on). A tier too low is `<action>
needs tier Full; ...`. Order of checks: action, tier, `dry_run`/`override_protection`, the two durations, the port, then `plan`. A dry run returns
`{dry_run, action, port, label, devices_affected, steps, protection, hub_switching, would_be_refused, nothing_was_changed}` and touches nothing (except
read only `uhubctl` status). A real `off` or `cycle` is refused by the verdict unless overridable and `override_protection` is true (`on` is never checked
against protection), and then by a hub that is not per-port. Switching runs under one process-wide mutex, so two calls never switch at once. `on`
returns the `power::set` report; `off` the same plus a warning that the port is now OFF; `cycle` returns `power::cycle`'s report with
`devices_affected` and the verification (level 2 when the leave-and-return was seen, from the cycle). Errors: the above, every error of `plan` and
`power::cycle`/`set` (a failed power-on after a power-off ends `THE PORT IS STILL OFF`). It is slow: up to 10 s waiting for the device to leave, the off
time, and the back timeout. Does not transmit; it can cut the power of a radio's USB interface, hence the protection list. See also: the
`port_power` tests, `usb_hub_power_cycle`, `usb_hub_verification_levels`.

### `protection` [@ANCHOR: bench_mcp_usb_hub_hubs_protection]

The implementation of `bench_usb_protection` (tier `read_only`): the whole protection list, and the verdict for one target when one is named, so an
agent can ask "may I power off this port" before trying.

| Argument | Type | Meaning |
|---|---|---|
| `t`, `cfg`, `sysfs_root`, `proc_root` | | Tree, config, sysfs and procfs roots. |
| `bench` | `&[BenchRef]` | The bench instruments. |
| `args.hub`+`port`, `label` or `device` | | Optional port to judge. The check starts when any of `hub`, `label` or `device` is present. |

Return value: the object of `protect::report` and, for a named port, `port_verdict: {port, verdict}` where the verdict is `protect::check_port(...).json()`.
Errors: those of `resolve_port` when a named port cannot be resolved (for example `hub` without `port`). It reads `/proc/mounts` and sysfs; no sudo,
no lock, changes nothing. See also: `bench_mcp_usb_hub_protect_report`, `bench_mcp_usb_hub_protect_check_port`.

## The escalation ladder: reset, authorize, driver, autosuspend

### `helper_command` [@ANCHOR: bench_mcp_usb_hub_ladder_helper_command]

Builds the argument list that runs this program's root helper through sudo: `-n <exe> --usb-priv <args...>`. The ladder needs root to write to
sysfs and to issue `USBDEVFS_RESET`; rather than giving the daemon blanket sudo, one binary path is allowed and it accepts four operations.

| Argument | Type | Meaning |
|---|---|---|
| `exe` | `&str` | Absolute path of this binary (`std::env::current_exe`), or `bench_mcp` when that fails. |
| `args` | `&[&str]` | The helper operation and its operands: `reset <dev>`, `authorize <dev> 0|1`, `unbind|bind <iface> <driver>`, `autosuspend <dev> on|auto <delay>`. |

Return value: `Vec<String>`, `["-n", exe, "--usb-priv", args...]`, to be given to `sudo`. Pure; no checks of the operands, which the helper makes itself.
The same list, joined, is the `commands` text of a dry run. See also: `bench_mcp_usb_hub_ladder_run_helper`, `usb_hub_privileged_ops`.

### `run_helper` [@ANCHOR: bench_mcp_usb_hub_ladder_run_helper]

Runs the root helper once through the runner and returns what it printed. The privileged step of every rung.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Runner` | Runs `sudo` with the arguments of `helper_command`. |
| `exe` | `&str` | This binary's path. |
| `args` | `&[&str]` | The helper operation. |

Return value: `Ok(text)` with the helper's output, trimmed, when its exit status is zero. Errors: the runner's own error, with `sudo is not installed:`
put in front when it starts with `sudo`; and for a non-zero exit `explain_failure(exe, output)`, which names the fix for a missing NOPASSWD sudoers
rule, a missing program or a user not allowed to run it, or else `sudo -n <exe> failed: <output>`. Side effects: starts `sudo -n bench_mcp --usb-priv`
as a subprocess and waits with no timeout. This is where hardware changes. The caller has already applied the tier and the protection verdict. See
also: `bench_mcp_usb_hub_uhubctl_explain_failure`.

### `target` [@ANCHOR: bench_mcp_usb_hub_ladder_target]

Collects what every rung needs before it acts: the tree, the device the arguments name, the protection verdict and the `dry_run` and
`override_protection` flags. Used by `reset`, `authorize` and `autosuspend` (`driver` does the same steps itself for an interface).

| Argument | Type | Meaning |
|---|---|---|
| `sysfs`, `proc_root` | `&Path` | Sysfs and procfs roots. |
| `cfg` | `&Config` | Labels, protection flags. |
| `bench` | `&[BenchRef]` | Bench instruments for protection. |
| `args.device` | string, required | A device path or a port label. |
| `args.dry_run`, `args.override_protection` | boolean | Via `flag`. |

Return value: a private `Target {tree, device, verdict, dry, over}`. Errors: a sysfs read error, `name the device: its path (for example 3-2.2.1) or its
port label`, any error of `resolve_device`, `<device> is a root hub: these tools act on devices behind a port` (a root hub is never a target, since it
has no port) and the `flag` errors. It reads sysfs and `/proc/mounts`; it changes nothing. See also: `usb_hub_ladder`.

### `reset` [@ANCHOR: usb_hub_ladder]

The implementation of `bench_usb_reset` (tier `read_config`) and the first rung of the ladder (reset; then deauthorize and authorize; then driver
unbind and bind; then the port power cycle of `bench_usb_port_power`): a `USBDEVFS_RESET` of one device. The host re-initialises the device, which
keeps its power and its device number, and its drivers are rebound. For a hub, everything below it re-enumerates. The anchor is also cited by the
tests of the whole ladder.

| Argument | Type | Meaning |
|---|---|---|
| `run`, `sysfs`, `proc_root`, `cfg`, `bench` | | As for `target`. |
| `args.device` | string, required | A device path or label. |
| `args.dry_run` | boolean | Return what would be done: `commands` (the sudo line), `devices_affected`, `protection`, `would_be_refused`, `nothing_was_changed`. |
| `args.override_protection` | boolean | Overrides only overridable reasons. |

Return value: `{reset, back_in_the_tree, after_s, device_number_before, device_number_after, note}`; `back_in_the_tree` is whether the device is in
sysfs again within 10 s (polled every 100 ms). A normal reset keeps the device number. Errors: those of `target`; the refusal of
`Verdict::permit("resetting <device>:")` (not overridable for config or bench reasons; overridable with `override_protection: true` for the rest, the
hub uplink case among them); and the helper's error from `run_helper`. Side effects: one `sudo -n <exe> --usb-priv reset <device>`. No lock. No
transmitter involved, but a reset of a radio's USB device interrupts its control link, which is why the protection applies. See also: the `ladder`
tests, `bench_mcp_usb_hub_ladder_authorize`.

### `authorize` [@ANCHOR: bench_mcp_usb_hub_ladder_authorize]

The implementation of `bench_usb_authorize` (tier `read_config`): deauthorize a device (a software disconnect: the kernel unbinds its drivers and the
device stays powered but unused) or authorize it again.

| Argument | Type | Meaning |
|---|---|---|
| `run`, `sysfs`, `proc_root`, `cfg`, `bench` | | As for `target`. |
| `args.device` | string, required | Path or label. |
| `args.authorized` | boolean, required | `false` deauthorizes, `true` authorizes. |
| `args.dry_run`, `args.override_protection` | boolean | As for `reset`. |

Return value: `{device, authorized, device_number_before, device_number_after, now: {authorized, interfaces_with_a_driver}, note}`; the `now` object is
read back from sysfs after the helper ran. Errors: those of `target`; `authorized must be true or false`; the protection refusal for
deauthorizing only (authorizing is never refused, so a deauthorized device can always be brought back, even when protected); the helper's error.
Side effects: one `sudo -n <exe> --usb-priv authorize <device> 0|1`. Deauthorizing a hub drops everything below it. Note the dry run comes before the
protection check and reports `would_be_refused`. See also: the `ladder` test.

### `driver` [@ANCHOR: bench_mcp_usb_hub_ladder_driver]

The implementation of `bench_usb_driver` (tier `read_config`): unbind a USB interface from its driver, or bind it to a named one. Unbinding removes
the nodes the driver made (tty, sound card, input), which is the narrowest way to free a misbehaving function of a composite device.

| Argument | Type | Meaning |
|---|---|---|
| `args.interface` | string, required | An interface name such as `3-2.2.2:1.3`, as `bench_usb_topology` lists it. The part before the colon is the device. |
| `args.action` | string, required | `unbind` or `bind`. |
| `args.driver` | string | Required for `bind`; for `unbind` it defaults to the driver bound now. |
| `args.dry_run`, `args.override_protection` | boolean | As for `reset`. |

Return value: `{interface, action, driver, driver_bound_now}` where the last is read back from sysfs; a dry run gives `would`, `commands`,
`currently_bound` and `nodes_that_go_away`. Errors: `name the interface, for example 3-2.2.2:1.3`, `an interface name looks like 3-2.2.2:1.3`, `no
device <d> in the tree`, `<d> has no interface <i>`, `action must be unbind or bind`, `<i> has no driver bound` (unbind of an unbound interface
without `driver`), `bind needs a driver name`, `unknown action`, flag errors, the protection refusal for `unbind` of the interface's device (`bind` is
never refused), and the helper's error. Side effect: one `sudo -n <exe> --usb-priv unbind|bind <interface> <driver>`. See also: the `ladder` tests.

### `autosuspend` [@ANCHOR: bench_mcp_usb_hub_ladder_autosuspend]

The implementation of `bench_usb_autosuspend` (tier `read_config`): set the runtime power management of one device. `on` means never autosuspend (the
fix for a device that misbehaves when suspended), `auto` lets the kernel suspend it after an idle delay.

| Argument | Type | Meaning |
|---|---|---|
| `args.device` | string, required | Path or label. |
| `args.control` | string, required | `on` or `auto`. |
| `args.delay_ms` | integer, optional | -1 to 3600000; absent or null passes `-` (leave the delay as it is). |
| `args.dry_run`, `args.override_protection` | boolean | As for `reset`; a dry run shows the current control, delay and runtime status. |

Return value: `{device, now: {control, delay_ms, runtime_status}}` read back from sysfs after the change. Errors: those of `target`; `control must be
on (never suspend) or auto (may suspend)`, `unknown control`, `delay_ms must be a whole number of milliseconds`, `delay_ms must be -1 to 3600000`;
the protection refusal when `control` is `auto` (allowing a protected device to suspend); the helper's error. Side effect: one `sudo -n <exe> --usb-priv
autosuspend <device> <control> <delay>`. See also: the `autosuspend` test.

## Port power: probe, switch, wait, cycle, report

### `probe` [@ANCHOR: bench_mcp_usb_hub_power_probe]

Reads what a device is right now straight from its sysfs directory, so a power cycle can tell "the same device is still there" from "a device came
back with a new device number". Result type `Seen {devnum: u32, vidpid: String}`. Used by `cycle`, `set` and the ladder's `reset`.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path` | The sysfs root. |
| `path` | `&str` | Device path such as `3-2.2`. |

Return value: `Some(Seen)` when `bus/usb/devices/<path>` has readable `idVendor`, `idProduct` and a numeric `devnum`; `vidpid` is `vvvv:pppp` as written
in sysfs (trimmed, not lower-cased). `None` when the directory or any of the three files is absent or `devnum` is not a number, which is also what a
device in the middle of leaving looks like. It reads three small files; no locking, no errors.

### `uhubctl` [@ANCHOR: bench_mcp_usb_hub_power_uhubctl]

Runs one `uhubctl` switch for one port: `sudo -n uhubctl -N -l <location> -p <port> -a off|on` (with `-e` for a SuperSpeed hub). This is the only
place the power commands are built, for `cycle` and `set`. `-N` is uhubctl's no-descriptions switch (it skips reading device descriptions, which is faster); the code only passes it.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Runner` | Runs sudo. |
| `port` | `&PortRef` | Hub path (location by `PortRef::location`), port number, `exact` flag. |
| `action` | `&str` | `off` or `on`. |

Return value: `Ok(())`, or `Err("uhubctl <action> on hub <loc> port <p> failed: <reason>")`, where the reason comes from `uhubctl::sudo` and
names a missing NOPASSWD rule or a missing program. Side effect: a subprocess that changes USB port power; no lock here (the callers are serialised by
`port_power`'s mutex). It does no protection check: the caller must have done it. See also: `bench_mcp_usb_hub_uhubctl_sudo`.

### `wait_until` [@ANCHOR: bench_mcp_usb_hub_power_wait_until]

Polls a condition every 100 ms until it is true or a time runs out. All waits for "the device left the tree" and "the device is back" use it, so a
test can use a short timeout.

| Argument | Type | Meaning |
|---|---|---|
| `timeout` | `Duration` | The longest to wait. The condition is checked at once, so a zero timeout checks once. |
| `done` | `impl FnMut() -> bool` | The condition; called repeatedly, may change state (the cycle stores the probed device in it). |

Return value: `true` when the condition became true, `false` at the timeout. It blocks the thread with `std::thread::sleep`; no error, no lock. Used also
by the ladder.

### `Report::level` [@ANCHOR: bench_mcp_usb_hub_power_report_level]

Gives the verification level one power cycle reached, for its report: 2 when the cycle showed the device leave the tree and return with a new device
number (`re_enumerated`), 1 otherwise. A cycle that did not see this still completed, but it did not confirm that the port really removes power, and
the report's `verification` text says so. Argument: `&self`, a `Report` (fields: the port, device before and after, requested and actual off time,
times to leave, return and be ready, `re_enumerated`, notes). Return value `u8`, 1 or 2; never 0, 3 or 4, which come from the config via
`verification`. Pure; no error.

### `cycle` [@ANCHOR: usb_hub_power_cycle]

One off-wait-on cycle of a port, with checks that the device really left the tree and came back. It is the single implementation behind
`bench_usb_port_power cycle` and the VNA's cold reset.

| Argument | Type | Meaning |
|---|---|---|
| `run`, `sysfs` | | Runner and sysfs root. |
| `port` | `&PortRef` | The port. |
| `opts` | `&Opts` | `off_time` (minimum off time), `back` (wait for return, from power-on), `gone` (wait for leaving, default 10 s) and `expect_vidpid` (the device's own when `None`). `Opts::new(off, back)` fills in `gone` and `None`. |
| `ready` | `Option<&mut dyn FnMut() -> bool>` | A check that the device is usable (for example answers its protocol), waited for in what is left of `back`. |

Steps: read the device on the port; `uhubctl off`; wait up to `gone` for it to leave (or change device number); hold the port off until `off_time` has
passed since the off command finished (the report's `off_time_actual_s` is the real figure); `uhubctl on`, with one retry after 500 ms; wait up to
`back` for a device with a different device number and the expected `vid:pid`; wait for `ready`. When the port was empty before, no leaving is
expected. Return value: `Ok(Report)`. `re_enumerated` is true when it left and came back with a new number and a before and after exist; then the
proof is remembered in a process-wide store (`Evidence`: time, seconds to leave, seconds to return, old and new device number) that `verification`
reads, until the daemon restarts. A device that stays in the tree gives a `notes` entry and level 1. Errors (`Err(String)`): the `uhubctl` error of the
off step; both power-on attempts failed (`... THE PORT IS STILL OFF: switch it on again with the port power tool`); the device was back but not ready
(`... was not ready within <n> s of power-on ...`); the device did not come back (`the device did not come back within <n> s ...: ` with the reason:
the same number is still there, a different `vid:pid` is there, or nothing enumerated). On any error after the off step the port may be left off.
Takes no lock itself; the caller serialises. Blocks for up to `gone` + `off_time` + `back`. See also: the `power_cycle` tests,
`bench_mcp_usb_hub_power_report_level`.

### `cycle_escalating` [@ANCHOR: usb_hub_power_escalation]

Runs `cycle` for a list of (off time, wait) settings in turn and stops at the first that works, for a unit that fails to enumerate after a short
power-off but comes back after a longer one.

| Argument | Type | Meaning |
|---|---|---|
| `run`, `sysfs`, `port` | | As for `cycle`. |
| `schedule` | `&[Opts]` | The cycles to try, in order. |
| `ready` | `Option<&mut dyn FnMut() -> bool>` | The readiness check, offered to every cycle. |

Return value: `Ok(Report)` of the first cycle that succeeded; when it was not the first, a note says which cycle it was and lists the earlier
failures. When every cycle fails: `Err` with the failures joined by `; `, each as `cycle <n> (off <s> s): <error>`. An empty schedule gives an empty
error string. The port may be off after a failure. Same side effects as `cycle`, repeated. See also: the `power_escalation` test.

### `set` [@ANCHOR: bench_mcp_usb_hub_power_set]

Switches a port off or on once and reports what the tree did, for `bench_usb_port_power` with `action` `off` or `on`.

| Argument | Type | Meaning |
|---|---|---|
| `run`, `sysfs`, `port` | | As for `cycle`. |
| `on` | `bool` | `true` powers the port, `false` switches it off. |
| `back` | `Duration` | For `on`: how long to wait for any device to enumerate. |

Return value: `{port, powered, device_before: {devnum, vidpid}|null, observed}`. For off: waits up to 10 s for the device to leave; `observed` says
how long it took, that the device is still in the tree (the port did not drop it), or that no device was on the port. For on: `observed` says when a
device enumerated, or `no device enumerated yet (the port may be empty)` after `back`; that is not an error. Error: the `uhubctl` error when the
command fails. Side effect: one `sudo -n uhubctl` and up to `GONE_TIMEOUT` or `back` of polling. It records no proof (only `cycle` does). See also:
`bench_mcp_usb_hub_power_uhubctl`.

## The protection list

### `add` [@ANCHOR: bench_mcp_usb_hub_protect_add]

Adds one reason to the list of why a device is critical, without duplicates. The list (`Critical`) is a map from device path to `Why {text,
overridable}`. Arguments: `c` (`&mut Critical`, mutated), `dev` (the device path), `text` (the sentence, which starts with a verb, for example `has
/dev/sda1 mounted at /mnt`, because it is joined to the device name in a refusal), `overridable` (`true` when `override_protection` may override it;
`false` for reasons from the config and from bench instruments, and for the root filesystem). A reason with the same text on the same device is not
added twice, whatever its flag. No return value, no error. Pure.

### `block_names` [@ANCHOR: bench_mcp_usb_hub_protect_block_names]

Collects every name a block device is known by on the way down to the USB device that carries it: itself, its whole disk (`sda1` gives `sda`,
`nvme0n1p2` gives `nvme0n1`) and, for a device-mapper or md device, the devices under it (listed in `class/block/<name>/slaves`), recursively. A
mounted `/dev/mapper/...` volume on a USB disk is thereby found. Used by `critical_set` for each mount.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path` | The sysfs root. |
| `name` | `&str` | The kernel block name, for example `dm-0`. |
| `out` | `&mut BTreeSet<String>` | The names found, appended to. |
| `depth` | `u32` | The recursion depth; start at 0. At more than 6 it stops. |

No return value and no error: a missing `slaves` directory is no slaves, and a name already in `out` stops the recursion (so a loop cannot run for
ever). Reads directories under `class/block`; no locking.

### `critical_set` [@ANCHOR: usb_hub_protection]

Builds the protection list for one call: every USB device that a tool must not power off, reset, unbind or deauthorize by default, with the reasons.
It is built fresh at each call, from the tree, `/proc/mounts`, sysfs and the config, so it follows the machine as it is now. Callers: `port_power`,
`protection` and each ladder rung.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree` | The USB tree. |
| `cfg` | `&Config` | `critical_devices` and the `critical` flag and label of ports. |
| `bench` | `&[BenchRef]` | What the bench instruments use: `path` (a serial device path), `hub_port` (hub path and port number), `vidpid`. |
| `sysfs`, `proc_root` | `&Path` | Roots for `class/block` and `mounts`. |

Reasons and whether an argument can override them: the device carries the root filesystem (not overridable); has another filesystem mounted
(overridable), found through `/proc/mounts` entries starting `/dev/`, canonicalised, with `block_names`; is a boot-protocol keyboard or mouse
(HID interface class 3, subclass 1, protocol 1 or 2; overridable); its `vid:pid` is in `critical_devices` or it is on a port marked `critical` (not
overridable); is the serial adapter of a bench instrument (not overridable); is on the hub port of a bench instrument, or is the unit of a bench
instrument by `vid:pid`, as the VNA (overridable, since the VNA's own cold reset does not go through this list). Return value: `Critical`
(device path to reasons), which is empty when nothing is critical. Reads `/proc/mounts` and canonicalises device paths; unreadable files give no
reasons. No error, no lock, no hardware. See also: the `protection` tests, `bench_mcp_usb_hub_protect_check_device`.

### `Verdict::permit` [@ANCHOR: bench_mcp_usb_hub_protect_verdict_permit]

Decides whether an action may go ahead, given a verdict and the caller's `override_protection`. It is where "refused by default" is enforced, and
it carries the message the agent sees.

| Argument | Type | Meaning |
|---|---|---|
| `&self` | `&Verdict` | `reasons: Vec<Why>`; `clear()` is true when there are none; `overridable()` is true when all are. |
| `what` | `&str` | The beginning of the message, for example `resetting 3-2.2:`; the reasons follow it. |
| `override_protection` | `bool` | The caller's argument. |

Return value: `Ok(())` when there are no reasons, or when all reasons are overridable and the flag is true. Errors: `refused: <what> <reasons joined by
"; ">. This protection comes from the config or a bench instrument and no argument overrides it` when any reason is not overridable, whatever the
flag; `refused: <what> <reasons>. Add override_protection: true if this is really intended` otherwise. Pure. Used by `port_power`, `reset`,
`authorize`, `driver` and `autosuspend`, each with its own `what`. See also: the `verdict` tests.

### `check_device` [@ANCHOR: bench_mcp_usb_hub_protect_check_device]

The verdict for acting on a device and everything below it: a reset, a deauthorize, or the power-off of its port. A hub is judged with its
whole subtree, because switching a hub takes down all it carries.

| Argument | Type | Meaning |
|---|---|---|
| `t`, `cfg`, `crit` | | The tree, config and `critical_set` result. |
| `device` | `&str` | Device path. |

Return value: a `Verdict`; each reason of the device and of any device below it, worded `the device <path> <reason>` or `<path> (below <device>)
<reason>`, keeping its overridable flag. If the device is a hub whose port is not marked `hub_uplink_ok`, one more overridable reason is added:
`<device> is a hub and this is its uplink: acting on it takes down the <n> device(s) below it`. An unknown device path gives only what the
subtree finds (nothing). No error, pure. See also: `bench_mcp_usb_hub_protect_check_port`.

### `check_port` [@ANCHOR: bench_mcp_usb_hub_protect_check_port]

The verdict for a port: what is plugged into it (`check_device`), preceded when the config marks the port `critical` by a reason
`port <label or key> is marked critical in the config` that cannot be overridden (so a critical port stays protected when empty). Arguments: `t`,
`cfg`, `crit` as above, `hub` (the hub path), `port` (number). Return value: a `Verdict`. No error, no I/O. Used by `plan` and `protection`.

### `report` [@ANCHOR: bench_mcp_usb_hub_protect_report]

The JSON of `bench_usb_protection` without a target: the list of protected devices and the rules, for an agent or a person to read before acting.

| Argument | Type | Meaning |
|---|---|---|
| `t`, `cfg`, `crit` | | The tree, the config and the `critical_set` result. |

Return value: `{critical_devices: [{device, vidpid, reasons: [{why, overridable}], hubs_on_the_path}], critical_ports: [keys], rules: [3 sentences]}`.
`hubs_on_the_path` lists the non-root hubs above the device, since powering one off would also remove it; a device in `crit` but missing from the
tree has a null `vidpid`. No error, no I/O. See also: `bench_mcp_usb_hub_hubs_protection`.
