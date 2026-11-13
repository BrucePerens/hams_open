# The USB Hub Tools Estimate Bus Bandwidth, Diagnose a Port, Read the Kernel's USB History and Do Root Work Through One Narrow Helper

As the person (or the AI agent) running the bench, I want `bench_mcp` to tell me when several USB devices cannot all stream at once because they share
one transaction translator, to explain why a device is slow, flapping or missing, to show the kernel's own record of what the bus did and to watch the
tree for a few seconds while I touch a cable, and, when a stuck device has to be reset, deauthorized or unbound, to do that as root through a helper that
accepts only those four operations. This story documents four files of the `usb_hub` driver in `daemons/bench_mcp/src/usb_hub/`: `bandwidth.rs`
(`bench_usb_bandwidth`), `diagnose.rs` (`bench_usb_diagnose` and the power budget), `history.rs` (`bench_usb_history` and `bench_usb_watch`) and
`privileged.rs` (the `--usb-priv` helper that the reset ladder runs through `sudo -n`). The module root, the sysfs reader, the `uhubctl` parser, the port
power tool, the reset ladder and the protection list are the story `usb_hub_a.md`; the tool pages for the agent are in `docs/USB_HUB_TOOLS.md`.

None of these functions keys a transmitter. The first three files are read only: they read `/sys/bus/usb/devices` and its `descriptors` files, and the
diagnose and history tools may run `journalctl -k` and `sudo -n uhubctl -N`. They belong to the `usb_hub` tools of tier `read_only`, so a `read_only`
instrument may call all of them, and none takes a lock of its own. The helper in `privileged.rs` is the only code here that writes (sysfs files and one
`ioctl`) and only as root; it has no notion of the tier, the protection list or `dry_run`, which the server enforces before it ever starts the helper.
There is no band check and no TX gate anywhere in these four files. Several functions pass the device speed around as a number of Mb/s taken from the
sysfs `speed` file (1.5, 12, 480, 5000, 10000, 20000); where a function compares it, the entry says against what.

## Bus bandwidth: `bandwidth.rs`

Full-speed and low-speed devices behind a high-speed hub share that hub's transaction translator (TT) and its one millisecond frame. Several
full-speed audio codecs on one busy bus run out of room there. The functions below count the periodic (isochronous and interrupt) bandwidth the way the
host reserves it (packet size plus protocol overhead, per polling interval). The numbers are an estimate of reservations, never a measurement of what the
devices send. Two constants set the budgets: `FS_PERIODIC_BYTES_PER_MS` (1350.0: a 12 Mb/s frame carries 1500 bytes per millisecond of which periodic
transfers may take 90 %) and `HS_PERIODIC_BYTES_PER_MS` (48000.0: 7500 bytes per 125 us microframe, 80 % periodic, eight microframes a millisecond).

### `overhead` [@ANCHOR: bench_mcp_usb_hub_bandwidth_overhead]

Gives the protocol overhead, in bytes on the wire, that one periodic packet costs on top of its payload. Private helper of `endpoint_load`, which is
the only caller, so the overhead of every endpoint is chosen in one place.

| Argument | Type | Meaning |
|---|---|---|
| `speed` | `f64`, Mb/s, by value | The negotiated speed from sysfs. Only the comparison `speed >= 480.0` is made: 480 and above is counted as high speed, below it as full or low speed. |
| `iso` | `bool`, by value | `true` for an isochronous endpoint, `false` for an interrupt endpoint. |

Return value, `f64` bytes: high speed isochronous 38, high speed interrupt 55, full or low speed isochronous 9, full or low speed interrupt 13. Low
speed uses the full-speed figures; the code does not distinguish them (the 1.5 Mb/s case is not treated separately). No errors, no panic, pure
arithmetic, no I/O and no lock. See also: `endpoint_load`; test `endpoint_loads_follow_the_usb_arithmetic`.

### `endpoint_load` [@ANCHOR: bench_mcp_usb_hub_bandwidth_endpoint_load]

Computes how many bytes per millisecond one periodic endpoint reserves on its bus. It is the one formula behind both the "active" figure (the endpoints
of the interface settings now in use, through the private `active_load`) and the "worst case" figure (every alternate setting read from the
descriptors, through `potential_streams`). Public so other tools can use the same arithmetic.

| Argument | Type | Meaning |
|---|---|---|
| `speed` | `f64`, Mb/s | Negotiated speed of the device. 5000 and above is not counted; 480 and above is high speed. |
| `iso` | `bool` | Isochronous (`true`) or interrupt (`false`); picks the overhead from `overhead`. |
| `mps` | `u32` | The raw `wMaxPacketSize` of the endpoint descriptor. Bits 0 to 10 are the payload bytes; at high speed bits 11 and 12 add extra transactions per microframe. Higher bits are ignored. |
| `interval_us` | `u32`, microseconds | The polling interval of the endpoint, from `interval_us` (the private function) or from sysfs. |

Return value: `Some(bytes_per_ms)` equal to `(payload + overhead) * transactions * (1000 / interval_us)`, where `payload` is `mps & 0x7ff` and
`transactions` is `1 + ((mps >> 11) & 3)` at high speed and 1 below it (the value 3 in those bits is reserved by the USB specification but is still
computed as 4 here). `None` when `speed >= 5000.0` (SuperSpeed links have their own bandwidth and are not counted, which is why such endpoints never
appear in the output) or when `interval_us` is 0 (an unknown interval, as sysfs reports it for some endpoints). Example: a full-speed isochronous
endpoint of 192 bytes polled every 1000 us reserves `(192 + 9) * 1 * 1 = 201` bytes per ms. Pure, no errors, no lock. See also: `overhead`,
`interval_us`, `potential_streams`; test `endpoint_loads_follow_the_usb_arithmetic`.

### `interval_us` [@ANCHOR: bench_mcp_usb_hub_bandwidth_interval_us]

Turns the `bInterval` byte of an endpoint descriptor into a polling interval in microseconds. Private; called by `potential_streams` for each endpoint
it finds in the descriptor bytes. (The live figure for the active setting comes from sysfs already in microseconds and does not pass here.)

| Argument | Type | Meaning |
|---|---|---|
| `speed` | `f64`, Mb/s | Negotiated speed; `>= 480.0` selects the high-speed rule. |
| `iso` | `bool` | Isochronous or interrupt. |
| `b` | `u8` | The raw `bInterval`. 0 is treated as 1 (the interrupt case) or clamped to 1 (the exponent cases); values above 16 are clamped to 16 in the exponent cases. |

Return value, `u32` microseconds. At high speed (both types) and for full-speed isochronous endpoints `bInterval` is an exponent: the interval is
`2^(b-1)` units, the unit being 125 us at high speed and 1000 us for full-speed isochronous. For full- and low-speed interrupt endpoints it is a plain count
of milliseconds, `1000 * max(b, 1)`. The result is never 0, so `endpoint_load` never sees "unknown" from this path. Pure, no errors, no lock. See also:
`endpoint_load`; test `endpoint_loads_follow_the_usb_arithmetic`.

### `potential_streams` [@ANCHOR: bench_mcp_usb_hub_bandwidth_potential_streams]

Lists every periodic endpoint of every alternate setting that a device offers, read from its raw descriptors, not only the ones in use. This is what
lets the tool say "these codecs could reserve most of the translator if both started streaming", which the current settings do not show. Called by
`bandwidth` once per device and by `potential_load`'s callers.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path`, borrowed | Root of the sysfs tree (`/sys` in production, a copy of a recorded tree in tests). The file read is `<sysfs>/bus/usb/devices/<d.path>/descriptors`. |
| `d` | `&Device`, borrowed | The device, from `Tree`; its `path` locates the file and its `speed_mbps()` decides the arithmetic. |

Return value, `Vec<Stream>`. Each `Stream` has `interface` (`u32`, interface number), `alt` (`u32`, alternate setting number), `address` (`u8`, the
endpoint address with its direction bit), `kind` (`"isochronous"` or `"interrupt"`) and `bytes_per_ms` (`f64`, from `endpoint_load`). The walk goes
through the descriptor bytes (length, type, body): an interface descriptor (type 4, at least 9 bytes) sets the current interface and alternate number;
an endpoint descriptor (type 5, at least 7 bytes) is kept when its transfer type (`bmAttributes & 3`) is isochronous (1) or interrupt (3). Bulk and
control endpoints are skipped, and so is every endpoint for which `endpoint_load` gives `None` (SuperSpeed devices therefore give an empty list). An
unreadable or missing `descriptors` file gives an empty list, not an error; a descriptor with length under 2 or one that runs past the end of the data
stops the walk, and what was found before it is returned. Blocking file read of a few hundred bytes, no lock, no write. See also: `potential_load`,
`endpoint_load`; test `descriptors_give_every_alternate_settings_periodic_endpoints`.

### `potential_load` [@ANCHOR: bench_mcp_usb_hub_bandwidth_potential_load]

Reduces a device's list of streams to its worst-case reservation: for each interface only one alternate setting can be selected at a time, so the
worst case takes, per interface, the alternate setting with the largest total, and adds the interfaces together. Called by `bandwidth` for the
`worst_case_bytes_per_ms` figure.

| Argument | Type | Meaning |
|---|---|---|
| `streams` | `&[Stream]`, borrowed | The output of `potential_streams` for one device. Order does not matter. |

Return value, `f64` bytes per millisecond; 0.0 for an empty slice. The streams are grouped by interface and alternate setting, the loads of one alternate
setting are summed, and the largest sum per interface is taken. No errors, pure, no lock. See also: `potential_streams`; test
`descriptors_give_every_alternate_settings_periodic_endpoints`.

### `domain_of` [@ANCHOR: bench_mcp_usb_hub_bandwidth_domain_of]

Decides where a device's periodic traffic is counted: its own bus at its own speed, or the transaction translator (TT) it hangs from. Called by
`bandwidth` for every non-root device, and by `bench_usb_diagnose` (`present_checks`) to find the domain row for one device. The `Domain` value it
returns is a map key, so it derives `Ord`.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree`, borrowed | The USB tree read from sysfs. Used to walk from the device up through its parents. |
| `d` | `&Device`, borrowed | The device whose domain is wanted. |

Return value, a `Domain` with `bus` (`u32`, the bus number), `kind` (a fixed text), `hub` (the path of the hub or root hub that owns the budget, for
example `3-2` or `usb3`) and `port` (`u32`; 0 unless the TT is per port). The `kind` values are: `"SuperSpeed bus"` (device speed 5000 or more) and
`"high-speed bus"` (480 or more), both with `hub` = `usb<bus>` and port 0. For a full- or low-speed device the walk goes up its parents: the first parent
that is high speed and not a root hub gives a TT domain, `"TT (one per port)"` when that hub is multi-TT (its `tt_mode()` starts with `multi-TT` and
its interface protocol is not 1) with `port` = the port of the child on that hub, otherwise `"TT (single, shared by all ports)"` with port 0; a full-speed
hub is walked through, and when the walk reaches a root hub the kind is `"full-speed root hub"` with that root hub as `hub`. A device whose parent is
missing from the tree falls back to `"full-speed root hub"` of `usb<bus>`. No errors; pure; no lock. See also: `capacity`; test
`the_domain_of_a_device_is_its_translator_or_its_bus`; `bench_mcp_usb_hub_sysfs_device_tt_mode` (story `usb_hub_a.md`).

### `capacity` [@ANCHOR: bench_mcp_usb_hub_bandwidth_capacity]

Gives the periodic budget of a domain, in bytes per millisecond, from the domain's `kind` text. Private; called by `bandwidth` for each domain.

| Argument | Type | Meaning |
|---|---|---|
| `kind` | `&str`, borrowed | The `kind` field of a `Domain`. Matched by prefix. |

Return value: `Some(1350.0)` for a kind starting with `TT` or `full-speed` (`FS_PERIODIC_BYTES_PER_MS`), `Some(48000.0)` for one starting with `high-speed`
(`HS_PERIODIC_BYTES_PER_MS`), and `None` for anything else, in practice `"SuperSpeed bus"`. A `None` makes `bandwidth` print a null budget and a null
percentage and skip the findings for that domain. The prefixes are the contract with `domain_of`: a new `kind` text must be added to both. Pure, no
errors, no lock. See also: `domain_of`; test
`the_three_full_speed_devices_of_the_vnwa_share_one_translator_and_the_codecs_can_reserve_most_of_it`.

### `bandwidth` [@ANCHOR: usb_hub_bandwidth]

The tool `bench_usb_bandwidth` (tier `read_only`). It groups the devices of the tree by the domain that counts their periodic traffic, gives each domain's
budget, its active and worst-case load and the devices in it, and adds findings in words. It also lists the root hubs of each bus with the controller they
share, so a USB 3 port and its USB 2 companion are visible. Called by the tool dispatch in `usb_hub/mod.rs` with a freshly read tree, and by
`bench_usb_diagnose` for the bus of one device.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree`, borrowed | The tree read from `root`. |
| `cfg` | `&Config`, borrowed | The instrument's `usb_hub` config; only the port labels are used (`cfg.label(hub, port)`) to name devices. |
| `sysfs` | `&Path`, borrowed | The sysfs root, for the `descriptors` files read by `potential_streams`. |
| `args` | `&Value`, JSON object | Optional `bus` (integer): limit the report to that bus number. A value that is not an unsigned integer is ignored, which means all buses. |

Return value: `Ok(json)` with `buses` (per root hub: `bus`, `root_hub`, `speed`, `controller` as `{pci, driver}` or null, `ports`, `devices` below it and
`companion_root_hubs_on_the_same_controller`), `domains` (per domain: `domain` name, `bus`, `kind`, `hub`, `port` or null, `periodic_budget_bytes_per_ms`
or null, `active_bytes_per_ms`, `active_percent`, `worst_case_bytes_per_ms`, `worst_case_percent`, and `devices` with label, vid:pid, product, speed,
active load, worst-case load and the count of periodic endpoints found in the descriptors; percentages are rounded to 0.1, loads to whole bytes),
`findings` (strings) and a `method` note. Three findings exist: worst case above the budget ("cannot all stream at once"); two or more devices with
isochronous endpoints and a worst case above half the budget; and active load above 90 % of the budget; the second applies only when the first does
not. Root hubs themselves are not counted. SuperSpeed domains have a null budget, so no findings are made for them.

Errors: `Err("no devices on that bus")` when no non-root device falls in the selection (an empty bus, or a `bus` number that does not exist). The
caller (`mod.rs`) also returns the `Tree::read` error when sysfs cannot be read. Blocking: it reads one `descriptors` file per device in addition to the
tree, with no subprocess, no `sudo`, no lock and no write, so it is cheap and safe to call repeatedly. The figures are reservation arithmetic, as the
`method` text in the answer says. See also: `bench_mcp_usb_hub_bandwidth_domain_of`, `bench_mcp_usb_hub_bandwidth_potential_load`; test
`the_three_full_speed_devices_of_the_vnwa_share_one_translator_and_the_codecs_can_reserve_most_of_it`.

## Diagnosis: `diagnose.rs`

`bench_usb_diagnose` collects what the other tools read (the sysfs tree, the port's state and the `uhubctl` status word, the power budget, translator
sharing and the last day of the kernel journal) and turns it into findings ranked `problem`, `warning` or `note`, each with its evidence and a next step. It
reports what it can see and says in `could_not_read` what it could not. The private `Finding` struct is `level` (`&'static str`), `what`, `evidence` and
`next` (strings); its JSON form has the keys `level`, `what`, `evidence` and `next_step`. `driverless_class_matters` (private, no anchor) says that an
interface of USB class 0x01 (audio), 0x02 (communications), 0x03 (HID), 0x08 (mass storage), 0x0a (CDC data) or 0x0e (video) without a driver is worth a
warning and any other class only a note.

### `power_budget` [@ANCHOR: usb_hub_power_budget]

Checks whether the current a device asked for fits what its port and its hub can supply. It exists because a bus-powered hub shares one 500 mA budget
among everything behind it, and a device that starts only on a powered hub is a classic cause of "it works there but not here". Public; called by
`present_checks` and by the topology report of `sysfs.rs` (the `power_budget` field of a device).

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree`, borrowed | The tree, to find the parent and the siblings. |
| `d` | `&Device`, borrowed | The device to check; its `max_power_ma` (the `bMaxPower` of its configuration, in mA) is what it asked for. |

Return value: `Value::Null` when the device has no parent in the tree (a root hub). Otherwise a JSON object: `device_ma`; `port_budget_ma` (900 when the
parent runs at SuperSpeed or faster, else 500); `parent_hub_bus_powered` (the parent is not a root hub and not self powered); `parent_hub_left_for_its_ports_ma`
(the port budget minus the parent hub's own `max_power_ma` when bus powered, saturating at 0; null otherwise); `all_devices_on_that_hub_ma` (the sum of
`max_power_ma` over all children of the parent, the device included); `over_budget`; and a `note`. `over_budget` is true when the device alone asks for
more than the port budget, or when the parent is bus powered and the sum of all its children is more than what is left. `bMaxPower` is what a device
requests, not what it draws, which the note states; a finding built from it is a warning, not proof. Pure, no error, no lock. See also: test
`the_power_budget_flags_a_bus_powered_hub_asked_for_more_than_it_has`.

### `rank` [@ANCHOR: bench_mcp_usb_hub_diagnose_rank]

Maps a finding level to a sort key so that the worst findings come first. Private; used only by `finish`.

| Argument | Type | Meaning |
|---|---|---|
| `l` | `&str`, borrowed | The level: `"problem"`, `"warning"` or anything else (`"note"` in practice). |

Return value, `u8`: 0 for `problem`, 1 for `warning`, 2 for every other text, so a misspelled level sorts as a note rather than failing. Pure; no errors, no
lock. See also: `finish`; test `an_empty_powered_port_is_a_powered_port_with_nothing_connected`.

### `missing_target` [@ANCHOR: bench_mcp_usb_hub_diagnose_missing_target]

Works out which hub port a device that is not in the tree would be on, from the tool's arguments, so that a missing device can still be diagnosed. Private;
called by `diagnose` only when the `device` argument did not resolve to a device that is present, or when no `device` was given.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree`, borrowed | The tree; only used to check that a named `hub` exists. |
| `cfg` | `&Config`, borrowed | The config; its port labels (`ports["3-2/4"].label`) are matched case-insensitively. |
| `args` | `&Value`, JSON object | `device` (string: a device path such as `3-2.2.1`, or a port label), or `hub` (string) with `port` (unsigned integer). |

Return value: `Ok((hub_path, port_number))`. With `device`: a text that `parent_of` can split (a device path) gives its parent hub and port without checking
that anything exists there; otherwise the first config port whose label equals the text ignoring case gives that port's key split at `/` (a port number that
does not parse becomes 0). A label that happens to look like a device path (for example `vna-2`) is taken as a path first. With `hub` and `port`: the
hub must be in the tree and the port is cut to 32 bits. Errors, all `String`: `"<text>" is neither a device path nor a port label`; `hub <h> is not in the
USB tree`; `name the device (path or port label), or hub and port` when neither form was given. Pure, no locks. See also: `usb_hub_diagnose`,
`bench_mcp_usb_hub_resolve_device` and `bench_mcp_usb_hub_sysfs_parent_of` (story `usb_hub_a.md`); test
`an_empty_powered_port_is_a_powered_port_with_nothing_connected`.

### `diagnose` [@ANCHOR: usb_hub_diagnose]

The tool `bench_usb_diagnose` (tier `read_only`): why a USB device is slow, misbehaving or missing. Called by the dispatch in `usb_hub/mod.rs`.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Runner`, borrowed | Runs programs: the production `ProcessRunner` starts real processes; tests pass a fake. Used for `sudo -n uhubctl -N` and `journalctl`. |
| `sysfs` | `&Path`, borrowed | Root of the sysfs tree. |
| `cfg` | `&Config`, borrowed | The instrument's config, for port labels. |
| `now` | `f64`, Unix seconds | The current time; the journal is read from `now - 86400` (24 hours). |
| `args` | `&Value`, JSON object | `device` (path or port label) or `hub` plus `port` (see `missing_target`); `privileged` (bool, default `true`): `false` skips the `uhubctl` call. |

Flow: read the tree; if `device` resolves to a device in the tree its parent hub and port are used (a root hub has no port, so it is an error),
otherwise `missing_target` names the port. Then, when `privileged`, run `sudo -n uhubctl -N` for the port's status word; read the kernel journal of the
last 24 hours with `history::journal` (no `sudo`; the `use_sudo` option is not offered here) and run `history::analyze` for the device with a 120 s flap
window and 3 connects. A failure of either read is not an error: it is added as a string to `could_not_read` and the checks that need it are skipped. If
the port's hub is not in the tree the answer is a single `problem` finding. Otherwise a hub that is deauthorized gives a `problem`, then `missing` runs
when nothing is on the port and `present_checks` when something is.

Return value: `Ok(json)` from `finish`. Errors, `String`: the tree cannot be read; `a root hub has no port to diagnose`; any error of `missing_target`;
`<hub> has N ports; M is not one of them` when the port number exceeds the hub's port count. Side effects: up to two subprocesses (`sudo -n uhubctl -N`,
`journalctl -k -o short-unix --no-pager -q -b --since=@...`), run one after the other on the calling thread with no timeout of our own; unknown whether the
runner adds one. Nothing is written, no lock is taken and nothing keys a transmitter. `sudo -n` never prompts; without a NOPASSWD rule the `uhubctl` part
lands in `could_not_read`. See also: `bench_mcp_usb_hub_diagnose_finish`, `bench_mcp_usb_hub_diagnose_missing`,
`bench_mcp_usb_hub_diagnose_present_checks`, `usb_hub_history_parse`, `bench_mcp_usb_hub_history_analyze`; test
`an_empty_powered_port_is_a_powered_port_with_nothing_connected`.

### `finish` [@ANCHOR: bench_mcp_usb_hub_diagnose_finish]

Builds the final JSON answer of `bench_usb_diagnose` from the findings: sorts them worst first and writes the one-sentence verdict. Private; called at the
two places where `diagnose` returns.

| Argument | Type | Meaning |
|---|---|---|
| `dev` | `&str` | The device path that was diagnosed (`3-2.2.1`). |
| `label` | `Option<String>`, owned | The configured port label, or `None`. |
| `present` | `bool` | Whether the device is in the tree. |
| `findings` | `Vec<Finding>`, owned and sorted here | The findings, in the order they were found; the sort is stable, so equal levels keep that order. |
| `unread` | `Vec<String>`, owned | What could not be read, copied to `could_not_read`. |
| `hist` | `Option<Value>`, owned | The `analyze` summary, or `None` when the journal was not readable. |

Return value: an object with `device`, `port_label`, `present_in_the_tree`, `verdict`, `findings` (each `{level, what, evidence, next_step}`),
`could_not_read` and `history_24h` (`{totals, per_device, flapping}` taken from the summary, or null). The verdict is `a problem was found` when the worst
finding is a problem; `no outright fault, but something looks wrong` for a warning; and with only notes or nothing, `nothing wrong was found in what can be
read from software` when the device is present or `the device is not on the bus and no cause was found in what can be read from software` when it is not.
No errors, pure, no lock. See also: `rank`, `usb_hub_diagnose`; test `an_empty_powered_port_is_a_powered_port_with_nothing_connected`.

### `missing` [@ANCHOR: bench_mcp_usb_hub_diagnose_missing]

Adds the findings for a port with nothing in the tree: a port switched off, an over-current, a connection that never enumerated, and a history of failed
enumerations. Private; called by `diagnose` when the port's child device is absent.

| Argument | Type | Meaning |
|---|---|---|
| `out` | `&mut Vec<Finding>` | The findings list; appended to. |
| `hub` | `&Device`, borrowed | The hub the port belongs to; its `self_powered` and `path` are used. |
| `sport` | `Option<&Port>` | The port as sysfs shows it (`state`, `over_current_count`, `disabled`), or `None`. |
| `up` | `Option<&UPort>` | The port as `uhubctl` shows it, with the decoded status word `bits`, or `None` when `uhubctl` was not read. |
| `hist` | `Option<&Value>` | The `analyze` summary for the port's device path, or `None`. |

The checks, with level. From the status word: power bit clear gives `problem` (the port is powered off); over-current bit gives `problem`; powered but not
connected gives `warning`; connected but not enabled gives `problem` (enumeration failed). From sysfs: `over_current_count > 0` gives `warning`; the port
`disable` file being 1 gives `problem`; and when `uhubctl` was not read a `note` reports the sysfs port state. From history: the first `per_device` row
with any enumeration failures, descriptor read errors or address errors in 24 hours gives `problem`. The first row is the smallest path in the summary,
which for a missing device is a device below the port if one was seen. Finally, if the hub is bus powered, a `note` says so; this one is skipped for a self-powered hub. No return value, no errors, pure apart from the
list. See also: `usb_hub_diagnose`; test `an_empty_powered_port_is_a_powered_port_with_nothing_connected`.

### `present_checks` [@ANCHOR: bench_mcp_usb_hub_diagnose_present_checks]

Adds the findings for a device that is in the tree: speed, authorization, drivers, runtime suspend, power budget, translator sharing and the kernel's
history for it. Private (with `#[allow(clippy::too_many_arguments)]`); called by `diagnose` when the port's child device is present.

| Argument | Type | Meaning |
|---|---|---|
| `out` | `&mut Vec<Finding>` | The findings list; appended to. |
| `t` | `&Tree` | The tree, for the parent and the power budget. |
| `cfg` | `&Config` | Port labels, passed on to `bandwidth`. |
| `sysfs` | `&Path` | The sysfs root, passed on to `bandwidth` for the descriptors. |
| `d` | `&Device` | The device. |
| `sport` | `Option<&Port>` | The port in sysfs, or `None`. |
| `up` | `Option<&UPort>` | The `uhubctl` port, or `None`. |
| `hist` | `Option<&Value>` | The history summary, or `None`. |

The checks, with level: a speed finding from `speed_finding` (`warning` when it is a warning, else `note`); deauthorized gives `problem`; for a device that
is not a hub, each interface without a driver gives `warning` or `note` by class (see above); `power/control` auto with `runtime_status` suspended gives a
`note`; `power_budget` over budget gives `warning`; over-current events on the port give `warning`; a suspended port per the status word gives a `note`
(normal for an idle autosuspended device). Then the bandwidth of the device's bus is computed with `bandwidth` and the row of this device's domain is
looked up: worst case above 100 % gives `warning`; above 50 % with more than two devices gives `note`. Finally from history for exactly this device path: any
errno -71 or enumeration failure gives `warning`; more than 2 resets gives `warning`; being in the flapping list gives `warning`. A failure of
`bandwidth` is silently skipped. It reads one `descriptors` file for every device of the bus (through `bandwidth`) and takes no lock. No return value, no
errors. See also: `usb_hub_diagnose`, `usb_hub_power_budget`, `usb_hub_bandwidth`, `bench_mcp_usb_hub_sysfs_speed_finding` (story `usb_hub_a.md`); test
`an_empty_powered_port_is_a_powered_port_with_nothing_connected`.

## History and watch: `history.rs`

The kernel writes a line to its log for every USB connect, disconnect, reset and enumeration error. `bench_usb_history` reads that log (`journalctl -k -o
short-unix`) and turns it into a timeline, per-device counters and flapping detection; `bench_usb_watch` is the live alternative that polls the sysfs tree.
The log is exact; the watch can miss a change shorter than its poll interval. The journal is readable only by root and by the groups `adm` and
`systemd-journal`; otherwise the tool fails with a message that names the fix, and `use_sudo: true` runs `sudo -n journalctl` for one call.
The private data structures of the file are `Event` (one parsed line: `ts`, `f64` Unix seconds as the journal wrote it; `kind`, one of the fixed words
listed under `parse_line`; `path`, the device the line is about, if any; `errno`, a negative error number if the line has one; `devnum`, the USB device
number the kernel assigned, if the line has one; `text`, the message after the `usb <device>:` prefix) and `Query` (the options of `analyze`, shown under it).

### `port_target` [@ANCHOR: bench_mcp_usb_hub_history_port_target]

Reads a kernel port token and gives the device path that would be on that port. The kernel writes `3-2.2-port1` for port 1 of hub `3-2.2`, and
`usb3-port2` for port 2 of root hub 3. Private; called by `parse_line` for the first word after `usb` or `hub`.

| Argument | Type | Meaning |
|---|---|---|
| `tok` | `&str`, borrowed | The token, possibly with a trailing `:`. |

Return value: `Some("3-2.2.1")` for `3-2.2-port1` and `Some("3-2")` for `usb3-port2` (through `child_path`). `None` when the token has no `-port` or the text
after the last `-port` is not an unsigned number once a trailing colon is removed. Pure, no errors, no lock. See also: `bench_mcp_usb_hub_sysfs_child_path`
(story `usb_hub_a.md`), `usb_hub_history_parse`; test `lines_are_classified_with_device_and_error_numbers`.

### `errno_of` [@ANCHOR: bench_mcp_usb_hub_history_errno_of]

Finds a negative kernel error number in the text of a log line, for example the -71 (a protocol error on the link) or -19 (no such device) that USB
messages carry. Private; called by `parse_line`, whose classification and the counters of `analyze` depend on it.

| Argument | Type | Meaning |
|---|---|---|
| `msg` | `&str`, borrowed | The message part of the line. |

Return value, `Option<i32>`: the first negative number that follows one of the keys `error `, `err = `, `err `, `error=`, `status `, tried in that order,
returned with its minus sign (`-71`). For each key all occurrences are scanned before the next key is tried. A positive number, a key with no number or
a number that is not a decimal integer is skipped. `None` when none matches. Pure, no errors, no lock. See also: `usb_hub_history_parse`; test
`lines_are_classified_with_device_and_error_numbers`.

### `parse_line` [@ANCHOR: usb_hub_history_parse]

Classifies one line of `journalctl -k -o short-unix` output as a USB event, or says it is not one. This is the heart of the history tool: its kinds are what
the counters of `analyze` and the findings of `bench_usb_diagnose` count. (The README bullet for this id is about the history parsing as a whole.)

| Argument | Type | Meaning |
|---|---|---|
| `line` | `&str`, borrowed | One journal line, `<unix time> <host> kernel: <message>`. |

Return value: `Some(Event)` or `None`. `None` means: no ` kernel: ` separator, a first field that is not a number, a message not about USB (the first word
must be `usb`, `hub`, or begin with `xhci_hcd`, `ehci`, `ohci` or `uhci`), or a USB message of no interest. The device `path` comes from the word after `usb`
or `hub`: a port token goes through `port_target`, otherwise the text before the first `:` (an interface `3-2.2:1.0` gives `3-2.2`); it is kept only when
it starts with a digit or `usb`, and controller messages have none. The `kind` is the first rule that matches the lowercase message: `connect` ("new ...
USB device number"), `found` (starts "new USB device found"), `disconnect`, `reset` (starts "reset ..." with "USB device number"), `enumeration_failed`
("unable to enumerate"), `descriptor_read_error`, `address_error` ("not accepting address" or "not responding to setup address"), `power_cycle_attempt`,
`over_current`, `urb_no_device` ("cannot submit urb" with errno -19), `hub_port_error` ("disabled by hub" or "babble"), `xhci_error` (an xhci line with
error, died, warn or not responding), `error` (any other line with an errno), `info` (hub found, ports detected, manufacturer, product, serial number
or device strings). `devnum` is the number after `number `. The line is borrowed and a new `Event` is built. Pure, no errors, no lock. See also:
`bench_mcp_usb_hub_history_parse`, `bench_mcp_usb_hub_history_errno_of`, `bench_mcp_usb_hub_history_port_target`; test
`lines_are_classified_with_device_and_error_numbers`.

### `parse` [@ANCHOR: bench_mcp_usb_hub_history_parse]

Parses a whole journal text into events in time order. Public; called by `history` and by `bench_usb_diagnose` after `journal`.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str`, borrowed | The output of `journalctl -k -o short-unix`, any number of lines. |

Return value: a `Vec<Event>` of the lines that `parse_line` accepted, sorted by `ts` (a stable sort, so equal times keep their order). Empty text gives an empty
vector; there is no error. A journal that spans boots is sorted by its own time stamps, which is why `journal` uses `-b` unless asked otherwise. Pure, no
lock. See also: `usb_hub_history_parse`, `bench_mcp_usb_hub_history_analyze`; test `the_recorded_journal_shows_the_vnwa_chain_flapping_and_its_errors`.

### `flaps` [@ANCHOR: bench_mcp_usb_hub_history_flaps]

Finds the densest burst of connects in a list of times: used to say that a device connected again and again, which a bad cable or a power problem does.
Private; called by `analyze` once per device.

| Argument | Type | Meaning |
|---|---|---|
| `times` | `&[f64]`, seconds, ascending | The connect times of one device. They must be in ascending order (`parse` guarantees it). |
| `window_s` | `f64`, seconds | The window length; two times are in one window when they differ by at most `window_s`. |
| `min` | `usize` | The least number of connects that counts as flapping. |

Return value: `Some((count, start))` where `count` is the largest number of connects in any window and `start` is the time of the first connect of the
first window that reached it; `None` when no window has at least `min` connects (or the slice is empty). A sliding window, linear in the number of
connects. No errors, pure, no lock. See also: `bench_mcp_usb_hub_history_analyze`; test `flapping_needs_enough_connects_inside_the_window`.

### `analyze` [@ANCHOR: bench_mcp_usb_hub_history_analyze]

Summarizes a list of events: totals by kind, counters per device, error number counts, flapping devices and the last events as a timeline. Public; called
by `history` for the tool and by `bench_usb_diagnose` for one device.

| Argument | Type | Meaning |
|---|---|---|
| `events` | `&[Event]`, borrowed | The events, in time order (`parse`). |
| `q` | `&Query`, borrowed | The selection and the flapping rule, below. |

`Query` fields: `device` (`Option<String>`: only the device and everything below it, where "below" means the same path or one that continues with `.` or `:`, so
`3-2` selects `3-2.2` and `3-2:1.0` but not `3-20`); `since` (`Option<f64>`, Unix seconds: drop earlier events); `limit` (`usize`: the number of events kept in
the timeline, 0 for none); `flap_window_s` (`f64`, seconds) and `flap_connects` (`usize`): a device flaps when it connected at least that many times
within that window. `info` events are dropped unless a device is selected.

Return value, a JSON object: `events_considered`, `first_t`, `last_t` (null when empty), `totals` (count per kind), `per_device` (one row per device path, or
`(controller)` for lines without one: `device`, `connects`, `disconnects`, `resets`, `enumeration_failures`, `descriptor_read_errors`, `address_errors`,
`power_cycle_attempts_by_the_kernel`, `over_current`, `urb_no_device_minus_19`, `xhci_errors`, `other_errors`, `hub_port_errors` and `errno_counts`, a map from the
error number as text to its count, or null), `flapping` (`device`, `connects_in_window`, `window_s`, `first_at`, `total_connects`), `flapping_rule` (words),
`timeline` (the last `limit` events: `t`, `kind`, `device`, `errno`, `devnum`, `text`) and `timeline_truncated_to_last` (the limit when events were
dropped, else null). No errors, pure, no lock. See also: `bench_mcp_usb_hub_history_flaps`, `usb_hub_diagnose`; test
`the_recorded_journal_shows_the_vnwa_chain_flapping_and_its_errors`.

### `journal` [@ANCHOR: bench_mcp_usb_hub_history_journal]

Reads the kernel journal text, or fails with the fix when this user may not read it. Public; called by `history` and by `bench_usb_diagnose`.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Runner`, borrowed | Starts the program (production: a real process; tests: a script). |
| `since_unix` | `Option<f64>`, Unix seconds | Passed as `--since=@N` (whole seconds, negative treated as 0); `None` means no limit. |
| `all_boots` | `bool` | `false` (the normal case) adds `-b`, the current boot only; `true` reads every boot in the journal, which can be large. |
| `use_sudo` | `bool` | `true` runs the command as `sudo -n journalctl ...` through `uhubctl::sudo`. |

The command is `journalctl -k -o short-unix --no-pager -q [-b] [--since=@N]`. Return value: `Ok(text)`. Errors, `String`: with `use_sudo` false, a program that
could not be run (for example `journalctl is not installed`) or a command that failed, or output with the "insufficient permissions" message of
`journalctl`, or the group hint with fewer than 6 lines, gives the constant `NO_ACCESS` message: the kernel journal is not readable by this user, add the
user to `systemd-journal` (or `adm`) or pass `use_sudo: true`. With `use_sudo`, the errors of `uhubctl::sudo` (a missing `sudo` or a refused `sudo -n`, which
never asks for a password). Blocking subprocess run on the calling thread, no timeout of our own; read only. See also: `bench_mcp_usb_hub_history_history`,
`bench_mcp_usb_hub_uhubctl_sudo` (story `usb_hub_a.md`); test `an_unreadable_journal_fails_with_the_fix_and_sudo_is_opt_in`.

### `num` [@ANCHOR: bench_mcp_usb_hub_history_num]

Reads an optional non-negative number from the arguments object. Private; used by `history` and `watch_with` for every numeric option.

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&Value` | The arguments object. |
| `k` | `&str` | The key. |

Return value: `Ok(None)` when the key is absent or null; `Ok(Some(n))` for a finite number of 0 or more (integers are accepted); `Err("<k> must be a non-negative
number")` for a negative number, a string, a boolean or anything else. Pure, no lock. See also: `bench_mcp_usb_hub_history_history`, `usb_hub_watch_with`;
test `an_unreadable_journal_fails_with_the_fix_and_sudo_is_opt_in`.

### `history` [@ANCHOR: bench_mcp_usb_hub_history_history]

The tool `bench_usb_history` (tier `read_only`): the USB timeline, counters and flapping from the kernel journal. Called by the dispatch in `mod.rs`.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Runner` | Starts `journalctl`. |
| `sysfs` | `&Path` | Sysfs root, read only when `device` is given. |
| `cfg` | `&Config` | For port labels. |
| `now` | `f64`, Unix seconds | The current time, the base of `since_s`. |
| `args` | `&Value` | `device` (path or label), `since_s` (seconds back, 0 or more), `max_events` (default 100, clamped 1 to 1000), `flap_window_s` (default 120, at least 1), `flap_connects` (default 3, at least 2), `all_boots` (default false), `use_sudo` (default false). |

`device` is resolved with `resolve_device` against the tree; if that fails but the text starts with a digit it is used as given, so a device that has
already left the tree can still be looked up. Return value: `Ok(json)`, the `analyze` object plus `source` (`journalctl -k` or `sudo -n journalctl -k`) and a
`note` that explains -19 and -71. Errors, `String`: a numeric argument that is not a non-negative number; a device that is neither resolvable nor
digit-led; a tree that cannot be read; the errors of `journal`. Blocking subprocess; read only; no lock. See also: `bench_mcp_usb_hub_history_journal`,
`bench_mcp_usb_hub_history_analyze`; test `an_unreadable_journal_fails_with_the_fix_and_sudo_is_opt_in`.

### `snapshot` [@ANCHOR: bench_mcp_usb_hub_history_snapshot]

Reduces a tree to the facts a watch compares between two polls. Private; called by `watch_with` for the first poll and each later one.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree`, borrowed | A tree read from sysfs. |

Return value, `Snaps`, a pair of maps. First, device path to `Snap` (`devnum` `u32`, the number the kernel gave the device; `vidpid` text; `speed` the sysfs
speed text in Mb/s; `authorized` bool). Second, port key `<hub path>/<port number>` to `(state text, over-current count)`. Root hubs are included. No errors,
pure, no lock. See also: `bench_mcp_usb_hub_history_diff`; test `a_watch_reports_a_device_leaving_and_returning_with_a_new_number`.

### `diff` [@ANCHOR: bench_mcp_usb_hub_history_diff]

Lists what changed between two snapshots. Private; called by `watch_with` after each poll.

| Argument | Type | Meaning |
|---|---|---|
| `at` | `f64`, seconds | Time since the watch began, put in each event as `t`. |
| `a` | `&Snaps`, borrowed | The older snapshot. |
| `b` | `&Snaps`, borrowed | The newer snapshot. |

Return value, `Vec<Value>`, in this order: for each device in `b` by path, one of `appeared` (not in `a`; with `vidpid`, `devnum`, `speed_mbps`),
`re-enumerated` (the device number changed; `devnum_before`, `devnum_after`), `speed changed` or `authorized changed` (only the first applicable one per
device per poll); then `disappeared` for each device only in `a`; then for each port in both, `port state changed` and `over-current count rose` (the code
reports the event whenever the count differs, in either direction). A port present in only one snapshot gives no event. No errors, pure, no lock. See also:
`bench_mcp_usb_hub_history_snapshot`; test `a_watch_reports_a_device_leaving_and_returning_with_a_new_number`.

### `watch` [@ANCHOR: usb_hub_watch]

The tool `bench_usb_watch` (tier `read_only`): watches the tree for a few seconds and reports each change with its time, and the number of enumerations per
device, so a device that connects again and again shows as flapping. Called by the dispatch in `mod.rs`.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path` | Sysfs root polled. |
| `args` | `&Value` | `seconds` (1 to 120, default 10) and `interval_ms` (50 to 5000, default 200, clamped). |

It runs `watch_with` with a callback that never stops it, so the return value and errors are those of `watch_with`. It blocks the calling thread for about
`seconds`; whether other tool calls of the server run meanwhile is unknown here. Read only, no lock. See also: `usb_hub_watch_with`; test
`the_watch_tool_polls_for_its_duration_and_checks_its_arguments`.

### `watch_with` [@ANCHOR: usb_hub_watch_with]

The loop behind `watch`, with a callback after every snapshot so a test can use the number of polls as a clock that does not depend on how busy the machine
is. Private. (`docs/USB_HUB_TOOLS.md` has an earlier entry for this id.)

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path`, borrowed | Root of the tree polled. |
| `args` | `&Value` | `seconds`, `interval_ms` as for `watch`. |
| `after_poll` | `&mut dyn FnMut(u64) -> bool` | Called on the watching thread once after the first snapshot with 0 and after every later poll with 1, 2, ...; returning true ends the watch early (the report is still built). |

The first snapshot is the baseline: devices already present are not reported. The loop then sleeps one interval, reads the tree, adds `diff` events and
repeats until the elapsed time reaches `seconds` or the callback asks to stop. Return value: `Ok(json)` with `watched_s`, `started_unix`, `poll_ms`, `events`
(each with `t`, seconds since the start, and `event`), `enumerations_per_device` (count of `appeared` plus `re-enumerated` per path), `flapping` (a list of
the paths with three or more) and a `method` note that polling can miss a change shorter than the interval. Errors, `String`: `seconds` or `interval_ms` is
not a non-negative number; `seconds must be 1 to 120`; the tree cannot be read (at the start, or at any later poll, which ends the watch with that error and
no partial report). Blocking; it reads only; no lock. The callback runs inside the loop, so one that changes the tree changes what the next poll sees. See
also: `bench_mcp_usb_hub_history_snapshot`, `bench_mcp_usb_hub_history_diff`; test `a_watch_reports_a_device_leaving_and_returning_with_a_new_number`.

## The root helper: `privileged.rs`

Resetting a device, deauthorizing it, unbinding a driver and setting autosuspend need root. The server runs `sudo -n <this binary> --usb-priv <operation>
<arguments>`, and the first argument makes the binary run `privileged::main` instead of serving MCP (`main.rs`). The helper accepts only these operations and
checks every argument against the real sysfs before it writes: `reset <device>`, `authorize <device> <0|1>`, `unbind <interface> <driver>` and `bind
<interface> <driver>`, `autosuspend <device> <on|auto> <delay_ms|->`. A sudoers rule for the binary lets the user run exactly these as root; it works as a
limit only when the binary is owned by root and not writable by the user, or the rule is a way to run anything. The tier (`read_config`), the protection list
and `dry_run` are checked by the server code in `ladder.rs` before the helper is started; the helper does not repeat them. The constant `FLAG` is `--usb-priv`.

### `device_dir` [@ANCHOR: bench_mcp_usb_hub_privileged_device_dir]

Turns a device name from the command line into its real sysfs directory, refusing anything that is not a USB device behind a hub port. Private; called by
`reset`, and by `run_at` for `authorize` and `autosuspend`.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path`, borrowed | The sysfs root (`/sys` in production, a writable copy in tests). |
| `dev` | `&str`, borrowed | The device name, `3-2.2.1` style: a bus number, `-`, then dot-separated port numbers. |

Return value: `Ok(PathBuf)`, the canonical path of `<sysfs>/bus/usb/devices/<dev>` (the symlink resolved). Errors, `String`: `<dev> is not the path of a USB
device behind a hub port (root hubs are not allowed)` when the text is not a device name (`is_device_name`: anything with a slash or `..` fails here) or
starts with `usb`; `<dev>: <OS error>` when the path does not exist; `<dev> is not a USB device` when the directory has no `idVendor` file. Read only (one
`canonicalize` and one `is_file`), no lock, no write. Safe to call repeatedly. See also: `usb_hub_privileged_ops`, `bench_mcp_usb_hub_sysfs_is_device_name`
(story `usb_hub_a.md`); test `anything_outside_the_allowlist_is_refused_before_a_write`.

### `interface_name` [@ANCHOR: bench_mcp_usb_hub_privileged_interface_name]

Checks that the interface argument of `bind` and `unbind` is spelled like a USB interface, `3-2.2.2:1.3`: a device name, `:`, then digits and dots
(configuration and interface number). Private; called by `run_at`.

| Argument | Type | Meaning |
|---|---|---|
| `i` | `&str`, borrowed | The text to check. |

Return value: `Ok(())`, or `Err("<text>" is not a USB interface name like 3-2.2.2:1.3)`. The device part is tested with `is_device_name`, which also accepts a
root hub name such as `usb3`; unlike `device_dir`, this check does not exclude root hubs. It does not check that the interface exists (the kernel
rejects a name it does not know when written). Pure, no lock. See also: `bench_mcp_usb_hub_privileged_driver_dir`, `usb_hub_privileged_ops`; test
`anything_outside_the_allowlist_is_refused_before_a_write`.

### `driver_dir` [@ANCHOR: bench_mcp_usb_hub_privileged_driver_dir]

Turns a driver name into the sysfs directory of that installed USB driver, refusing anything else. Private; called by `run_at` for `bind` and `unbind`.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path` | The sysfs root. |
| `driver` | `&str` | The driver name, for example `snd-usb-audio`: not empty and made only of ASCII letters, digits, `_`, `-` and `.` (no slash, so no path escape). |

Return value: `Ok(<sysfs>/bus/usb/drivers/<driver>)` when that is a directory. Errors, `String`: `<name> is not a driver name`; `no USB driver named <name>`. A
name made only of dots passes the character test; the resulting path is not a driver directory under a real sysfs, and the later write fails with an
operating system error. Read only, no lock. See also: `usb_hub_privileged_ops`; test `anything_outside_the_allowlist_is_refused_before_a_write` and
`bind_and_unbind_write_the_interface_to_the_drivers_file`.

### `reset` [@ANCHOR: bench_mcp_usb_hub_privileged_reset]

Resets one device with the `USBDEVFS_RESET` ioctl (0x5514) on its device node: a port reset, after which the device is re-enumerated by the kernel; per
the module's own description the device keeps its number and its power. It is the gentlest rung of the reset ladder. Private; called by `run_at`.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path` | The sysfs root, used only to find the device; the node is always under `/dev/bus/usb`. |
| `dev` | `&str` | The device name, checked by `device_dir`. |

Steps: `device_dir`; read `busnum` and `devnum` from the device directory; open `/dev/bus/usb/<bus>/<dev>` (three digits each) for writing; call the ioctl with
no argument. Return value: `Ok("reset <dev> (<node>)")`. Errors, `String`: those of `device_dir`; `cannot read busnum of <dir>` (or `devnum`); `open <node>: <error>`
(for example when the device vanished or the caller is not root); `USBDEVFS_RESET on <node>: <error>`. Needs root to open the node. Blocking: the ioctl returns
when the kernel finishes the reset; how long that takes is unknown. It contains one `unsafe` call, which takes no data pointer. Any driver bound to the device
sees a disconnect-like event; not idempotent in effect, but safe to repeat. Unit tests exercise the refusals only (`reset` with no device, a path with `..`);
the ioctl itself is not run in them. See also: `bench_mcp_usb_hub_privileged_device_dir`; test
`authorize_and_autosuspend_write_only_their_files_after_checking_the_device` and `anything_outside_the_allowlist_is_refused_before_a_write`.

### `run_at` [@ANCHOR: usb_hub_privileged_ops]

Runs one allowed operation against the sysfs at a given root; the testable core of the helper. Public; called by `main` with `/sys`, and by the tests with a
writable copy.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path`, borrowed | Sysfs root. |
| `args` | `&[String]`, borrowed | The words after `--usb-priv`. |

The accepted forms and what they write (all text, no newline):

| Words | Effect |
|---|---|
| `reset <dev>` | `reset` above. |
| `authorize <dev> <0\|1>` | Writes `0` or `1` to the device's `authorized` file; 0 is a software disconnect that keeps power. |
| `unbind <iface> <driver>`, `bind <iface> <driver>` | Writes the interface name to the driver's `unbind` or `bind` file. |
| `autosuspend <dev> <on\|auto> <delay>` | If `delay` is not `-`, an integer from -1 to 3600000 (milliseconds; the meaning of -1 is the kernel's, this code passes it on) is written to `power/autosuspend_delay_ms`; then `on` or `auto` to `power/control`. |

Return value: `Ok(text)` such as `bind 3-2.2.2:1.0 snd-usb-audio` or `3-2.2.1 authorized=0`. Errors, `String`: `not an allowed operation: <words>` for any other
shape (wrong count, an unknown verb, a value other than 0 or 1, `on` or `auto`); the errors of `device_dir`, `interface_name` and `driver_dir`; `<delay> is not a delay
in ms`; `delay_ms must be -1 to 3600000`; `write <path>: <error>` when a write fails. Every argument is checked before the write that uses it, but the
autosuspend case writes the delay first and the control second, so a failed second write leaves the delay changed. Side effects: files under sysfs, as root;
no lock; repeating an operation is harmless. Nothing here keys a transmitter. See also: `bench_mcp_usb_hub_privileged_main`, `bench_mcp_usb_hub_privileged_reset`,
`bench_mcp_usb_hub_ladder_helper_command` (story `usb_hub_a.md`); tests
`authorize_and_autosuspend_write_only_their_files_after_checking_the_device`, `bind_and_unbind_write_the_interface_to_the_drivers_file`,
`anything_outside_the_allowlist_is_refused_before_a_write`.

### `main` [@ANCHOR: bench_mcp_usb_hub_privileged_main]

The entry point of `--usb-priv`: refuses to run unless root, runs the operation on the real `/sys` and prints the outcome. Called from `main.rs` when the first
command-line argument is `--usb-priv`, whose result becomes the process exit code; it takes no config and serves no MCP (Model Context Protocol).

| Argument | Type | Meaning |
|---|---|---|
| `args` | `&[String]`, borrowed | The words after `--usb-priv`, passed on to `run_at`. |

Return value, `i32` exit code: 0 on success, with the result text on standard output; 1 when `run_at` fails, with the error on standard error; 2 when the
effective user id is not 0, with `--usb-priv must run as root (through sudo -n)` on standard error, before any argument is looked at. Blocking, single threaded,
and the success path acts on the real `/sys`, so the unit test only checks the refusals (exit 2 for a non-root caller, otherwise exit 1 for an operation outside
the list). Side effects are those of `run_at`. See also: `usb_hub_privileged_ops`; test
`the_entry_point_refuses_a_non_root_caller_and_an_operation_outside_the_allowlist`.
