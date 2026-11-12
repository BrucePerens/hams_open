# Data structures

### `Stream` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_bandwidth_stream]

**Purpose and location.** One periodic stream (an isochronous or interrupt endpoint of an active alternate setting) and the bandwidth it asks of its bus, used to find bandwidth conflicts. Defined in `daemons/bench_mcp/src/usb_hub/bandwidth.rs:63` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `interface` | `u32` | The interface number it belongs to. |
| `alt` | `u32` | The alternate setting number. |
| `address` | `u8` | The endpoint address. |
| `kind` | `&'static str` | `isochronous` or `interrupt`. |
| `bytes_per_ms` | `f64` | The periodic bandwidth in bytes per millisecond. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `f_close_stream` (src/sdr.rs:1210), `f_activate` (src/sdr.rs:1213), `f_deactivate` (src/sdr.rs:1217), `potential_streams` (src/usb_hub/bandwidth.rs:72), `potential_load` (src/usb_hub/bandwidth.rs:131).

**See also:** test `descriptors_give_every_alternate_settings_periodic_endpoints (src/usb_hub/bandwidth.rs)`.

### `Domain` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_bandwidth_domain]

**Purpose and location.** The domain a device's periodic traffic is counted in: its translator for a full- or low-speed device below a high-speed hub, otherwise its bus at its own speed. Defined in `daemons/bench_mcp/src/usb_hub/bandwidth.rs:150` as a pub `struct`. It derives Clone, Debug, PartialEq, Eq, PartialOrd, Ord.

| Field | Type | Meaning |
|---|---|---|
| `bus` | `u32` | The bus number. |
| `kind` | `&'static str` | The domain kind as text, such as `SuperSpeed bus` or `high-speed bus` or a transaction translator. |
| `hub` | `String` | The hub that hosts the translator, empty for a bus domain. |
| `port` | `u32` | The port of that hub, 0 for a bus domain. |

**Invariants, ownership and concurrency.** Ordered and comparable so it can key a map of loads per domain.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `domain_of` (src/usb_hub/bandwidth.rs:157).

**See also:** test `the_domain_of_a_device_is_its_translator_or_its_bus (src/usb_hub/bandwidth.rs)`.

### `Finding` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_diagnose_finding]

**Purpose and location.** One finding of the USB diagnosis: how serious, what was found, the evidence for it and what to do next. Defined in `daemons/bench_mcp/src/usb_hub/diagnose.rs:43` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `level` | `&'static str` | Severity word: `problem`, `warning` or `note`. |
| `what` | `String` | The finding in a sentence. |
| `evidence` | `String` | The numbers or log lines that support it. |
| `next` | `String` | The suggested next step. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `f` (src/usb_hub/diagnose.rs:56).

**See also:** test `a_powered_off_port_and_a_deauthorized_device_are_found (src/usb_hub/diagnose.rs)`.

### `Event` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_history_event]

**Purpose and location.** One USB-related line of the kernel log, parsed. Defined in `daemons/bench_mcp/src/usb_hub/history.rs:17` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `ts` | `f64` | Timestamp in seconds from the log. |
| `kind` | `&'static str` | The kind of event as a fixed word chosen by the log classifier. |
| `path` | `Option<String>` | The device path the line is about (`3-2.2.1`); for a port message the device that would be on the port. |
| `errno` | `Option<i32>` | The error number the line carried, if any. |
| `devnum` | `Option<u32>` | The USB device number the line named, if any. |
| `text` | `String` | The original line text. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `decode_morse` (src/audio/events.rs:248), `active_samples` (src/audio/verify.rs:261), `ev` (src/audio/verify.rs:536), `evidence` (src/audio/verify.rs:546), `parse_line` (src/usb_hub/history.rs:63), `parse` (src/usb_hub/history.rs:152), `analyze` (src/usb_hub/history.rs:190).

**See also:** test `lines_are_classified_with_device_and_error_numbers (src/usb_hub/history.rs)`.

### `Query` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_history_query]

**Purpose and location.** Options for the history analysis: which device, how far back, how many events and the flap detection window. Defined in `daemons/bench_mcp/src/usb_hub/history.rs:160` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `device` | `Option<String>` | Restrict to one device path, if set. |
| `since` | `Option<f64>` | Only events after this time, if set. |
| `limit` | `usize` | The most events to return. |
| `flap_window_s` | `f64` | The window in seconds in which repeated connects count as flapping. |
| `flap_connects` | `usize` | How many connects within the window count as flapping. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `analyze` (src/usb_hub/history.rs:190), `q` (src/usb_hub/history.rs:495).

**See also:** test `flapping_needs_enough_connects_inside_the_window (src/usb_hub/history.rs)`.

### `Snap` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_history_snap]

**Purpose and location.** What the tree shows for one device, for the watch diff. Defined in `daemons/bench_mcp/src/usb_hub/history.rs:352` as a private `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `devnum` | `u32` | Device number. |
| `vidpid` | `String` | The vendor and product id as `vid:pid`. |
| `speed` | `String` | The link speed text. |
| `authorized` | `bool` | Whether the kernel authorized it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_watch_reports_a_device_leaving_and_returning_with_a_new_number (src/usb_hub/history.rs)`.

### `Snaps` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_history_snaps]

**Purpose and location.** The pair of maps the watch compares: device snapshots by path, and a second map of (text, number) by path. Defined in `daemons/bench_mcp/src/usb_hub/history.rs:359` as a private `type`. It derives no traits.

**Definition.** `type Snaps = (BTreeMap<String, Snap>, BTreeMap<String, (String, u64)>);` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `snapshot` (src/usb_hub/history.rs:361), `diff` (src/usb_hub/history.rs:394).

**See also:** test `a_watch_reports_a_device_leaving_and_returning_with_a_new_number (src/usb_hub/history.rs)`.

### `Plan` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_hubs_plan_type]

**Purpose and location.** Everything about a port action that can be decided before touching it: who is affected, the protection verdict and whether the hub can switch power. Defined in `daemons/bench_mcp/src/usb_hub/hubs.rs:257` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `affected` | `Vec<String>` | The device paths below the port that the action would disconnect. |
| `verdict` | `protect::Verdict` | The protection verdict (reasons the action may be refused). |
| `switching` | `Result<(), String>` | `Ok` when the hub switches per port, else the reason it cannot. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `plan` (src/gla/pattern.rs:56), `backend_args` (src/gla/pattern.rs:169), `plan` (src/glasgow/ops.rs:142), `done` (src/glasgow/ops.rs:736), `plan` (src/radioctl/control.rs:172).

**See also:** test `dry_run_says_what_would_happen_and_changes_nothing (src/usb_hub/hubs.rs)`.

### `Target` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_ladder_target_type]

**Purpose and location.** What every rung of the recovery ladder needs: the tree, the device named by the arguments and the protection verdict for acting on it. Defined in `daemons/bench_mcp/src/usb_hub/ladder.rs:42` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `tree` | `Tree` | The USB tree read at the start. |
| `device` | `String` | The device path named by the caller. |
| `verdict` | `protect::Verdict` | The protection verdict for acting on it. |
| `dry` | `bool` | The `dry_run` argument: report and change nothing. |
| `over` | `bool` | The `override_protection` argument: accept the overridable reasons. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `dry` (src/usb_hub/ladder.rs:80).

**See also:** test `protected_devices_are_refused_before_the_helper_runs_and_dry_run_changes_nothing (src/usb_hub/ladder.rs)`.

### `PortRef` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_power_port_ref]

**Purpose and location.** A hub port as uhubctl names it. Defined in `daemons/bench_mcp/src/usb_hub/power.rs:28` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `hub` | `String` | The hub location as `-l` takes it. |
| `port` | `u32` | The port number. |
| `exact` | `bool` | Pass `-e` to uhubctl, needed for a SuperSpeed hub whose location uhubctl 2.6.0 does not find otherwise. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `resolve_port` (src/usb_hub/hubs.rs:176), `port_ref` (src/usb_hub/hubs.rs:211), `uhubctl` (src/usb_hub/power.rs:108), `port` (src/usb_hub/power.rs:420).

**See also:** test `the_exact_flag_reaches_the_uhubctl_command (src/usb_hub/power.rs)`.

### `Seen` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_power_seen]

**Purpose and location.** A device as the tree shows it right now: its device number and ids, read straight from its sysfs directory. Defined in `daemons/bench_mcp/src/usb_hub/power.rs:54` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `devnum` | `u32` | The device number, which changes when the device re-enumerates. |
| `vidpid` | `String` | The vendor and product id as `vid:pid`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `probe` (src/usb_hub/power.rs:59).

**See also:** test `a_cycle_is_off_then_on_through_sudo_uhubctl_and_proves_re_enumeration (src/usb_hub/power.rs)`.

### `Evidence` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_power_evidence]

**Purpose and location.** What a session has proved about a port by cycling it: that the device went away and came back with a new device number. Defined in `daemons/bench_mcp/src/usb_hub/power.rs:76` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `unix_time` | `u64` | When the cycle happened. |
| `gone_after_s` | `f64` | Seconds until the device left the tree. |
| `back_after_s` | `f64` | Seconds until it returned. |
| `old_devnum` | `u32` | Device number before. |
| `new_devnum` | `u32` | Device number after. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `judge` (src/audio/verify.rs:110), `evidence` (src/audio/verify.rs:546), `proven` (src/usb_hub/power.rs:86), `remember` (src/usb_hub/power.rs:100).

**See also:** test `a_cycle_is_off_then_on_through_sudo_uhubctl_and_proves_re_enumeration (src/usb_hub/power.rs)`.

### `Why` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_protect_why]

**Purpose and location.** One reason a USB port action is refused. Defined in `daemons/bench_mcp/src/usb_hub/protect.rs:18` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `text` | `String` | The reason in words, naming what would be affected. |
| `overridable` | `bool` | Whether the caller may accept it with the override argument. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_hub_uplink_is_refused_unless_the_config_says_it_is_the_target (src/usb_hub/protect.rs)`.

### `Critical` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_protect_critical]

**Purpose and location.** Device path to the reasons that path is critical (the bench's own instruments and the things they depend on). Defined in `daemons/bench_mcp/src/usb_hub/protect.rs:24` as a pub `type`. It derives no traits.

**Definition.** `pub type Critical = BTreeMap<String, Vec<Why>>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `add` (src/usb_hub/protect.rs:35), `check_device` (src/usb_hub/protect.rs:241), `check_port` (src/usb_hub/protect.rs:271), `report` (src/usb_hub/protect.rs:291).

**See also:** test `keyboards_and_bench_serial_adapters_are_critical (src/usb_hub/protect.rs)`.

### `BenchRef` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_protect_bench_ref]

**Purpose and location.** What a bench instrument uses on the USB tree: a serial device path (a power supply), the hub port its unit hangs from and its vid:pid (the VNA). Defined in `daemons/bench_mcp/src/usb_hub/protect.rs:28` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `instrument` | `String` | The instrument's name from the configuration. |
| `path` | `Option<String>` | Serial device path, if it has one. |
| `hub_port` | `Option<(String, u32)>` | The hub and port it hangs from, if known. |
| `vidpid` | `Option<String>` | Its USB ids, if known. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `bench_refs` (src/usb_hub/hubs.rs:157).

**See also:** test `keyboards_and_bench_serial_adapters_are_critical (src/usb_hub/protect.rs)`.

### `Verdict` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_protect_verdict]

**Purpose and location.** The verdict on acting on a port: the list of reasons it should be refused; empty means permitted. `permit` turns it into a refusal unless the reasons are all overridable and the override was given. Defined in `daemons/bench_mcp/src/usb_hub/protect.rs:191` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `reasons` | `Vec<Why>` | The reasons in order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `check_device` (src/usb_hub/protect.rs:241), `check_port` (src/usb_hub/protect.rs:271).

**See also:** test `the_vna_hub_port_and_everything_below_it_is_protected_but_overridable (src/usb_hub/protect.rs)`.

### `Endpoint` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_sysfs_endpoint]

**Purpose and location.** One endpoint of the active alternate setting of an interface. Defined in `daemons/bench_mcp/src/usb_hub/sysfs.rs:11` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `address` | `u8` | Endpoint address. |
| `kind` | `String` | Transfer type text. |
| `max_packet` | `u32` | Maximum packet size in bytes. |
| `interval_us` | `u32` | Polling interval in microseconds. |
| `direction` | `String` | `in` or `out`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `endpoints_of` (src/usb_hub/sysfs.rs:216).

**See also:** test `endpoints_are_read_from_their_directories_and_sorted_by_address (src/usb_hub/sysfs.rs)`.

### `Node` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_sysfs_node]

**Purpose and location.** A device node, network interface or sound card an interface exposes. Defined in `daemons/bench_mcp/src/usb_hub/sysfs.rs:21` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `class` | `String` | Kind of node (for example tty, net, sound). |
| `name` | `String` | The node name. |
| `dev` | `Option<String>` | Major and minor numbers as text, if any. |
| `path` | `Option<String>` | The device path in `/dev`, if any. |
| `id` | `Option<String>` | An id such as a sound card id, if any. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `collect_nodes` (src/usb_hub/sysfs.rs:268), `class_nodes` (src/usb_hub/sysfs.rs:291), `node_json` (src/usb_hub/sysfs.rs:649).

**See also:** test `the_root_filesystem_device_and_mounts_on_usb_storage_are_found_through_dm_and_partitions (src/usb_hub/protect.rs)`.

### `Interface` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_sysfs_interface]

**Purpose and location.** One interface of a device, from sysfs. Defined in `daemons/bench_mcp/src/usb_hub/sysfs.rs:30` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The sysfs name of the interface. |
| `number` | `u32` | The interface number. |
| `alt` | `u32` | The active alternate setting. |
| `class` | `u8` | Interface class code. |
| `subclass` | `u8` | Interface subclass code. |
| `protocol` | `u8` | Interface protocol code. |
| `driver` | `Option<String>` | The bound kernel driver, if any. |
| `nodes` | `Vec<Node>` | Nodes the interface exposes. |
| `endpoints` | `Vec<Endpoint>` | Endpoints of the active alternate setting. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `interface_of` (src/usb_hub/sysfs.rs:338), `iface_json` (src/usb_hub/sysfs.rs:653).

**See also:** test `a_codec_exposes_its_sound_card_hid_node_and_drivers (src/usb_hub/sysfs.rs)`.

### `Port` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_sysfs_port]

**Purpose and location.** The state of one downstream port, from the hub's `<hub>-portN` directory. Defined in `daemons/bench_mcp/src/usb_hub/sysfs.rs:44` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `hub` | `String` | The hub's path. |
| `number` | `u32` | The port number. |
| `state` | `String` | The port state text. |
| `connect_type` | `String` | The connect type text (hotplug, hard-wired and so on). |
| `over_current_count` | `u64` | Over-current events counted by the kernel. |
| `disabled` | `bool` | Whether the port is disabled. |
| `peer` | `Option<String>` | The companion port of a USB 3 hub (the USB 2 side), if any. |
| `child` | `Option<String>` | The path of the device on the port, if any. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/link.rs:224), `identify` (src/nanovna.rs:285), `measure` (src/nanovna.rs:414), `sweep_and_read` (src/nanovna.rs:475), `ports` (src/audio/mod.rs:202), `ports_of` (src/usb_hub/sysfs.rs:356), `port_json` (src/usb_hub/sysfs.rs:661).

**See also:** test `a_powered_off_port_and_a_deauthorized_device_are_found (src/usb_hub/diagnose.rs)`.

### `Controller` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_sysfs_controller]

**Purpose and location.** The host controller a root hub belongs to. Defined in `daemons/bench_mcp/src/usb_hub/sysfs.rs:56` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `pci` | `String` | The PCI address. |
| `driver` | `String` | The kernel driver. |
| `vendor` | `String` | PCI vendor id. |
| `device` | `String` | PCI device id. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `controller_of` (src/usb_hub/sysfs.rs:404).

**See also:** test `the_recorded_tree_has_the_vnwa_chain_and_its_topology (src/usb_hub/sysfs.rs)`.

### `Device` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_sysfs_device]

**Purpose and location.** One USB device read from sysfs: identity, speed, power, interfaces and, for a hub, its ports. Defined in `daemons/bench_mcp/src/usb_hub/sysfs.rs:64` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `path` | `String` | The sysfs name: `usb3` for a root hub, `3-2.2` for the device on port 2 of the hub `3-2`. |
| `parent` | `Option<String>` | The sysfs path of the parent hub, `None` for a root hub. |
| `port` | `Option<u32>` | The port number on the parent (None for a root hub). |
| `bus` | `u32` | Bus number. |
| `devnum` | `u32` | Device number on the bus. |
| `vid` | `String` | Vendor id (hex text). |
| `pid` | `String` | Product id (hex text). |
| `manufacturer` | `Option<String>` | Manufacturer string, if present. |
| `product` | `Option<String>` | Product string, if present. |
| `serial` | `Option<String>` | Serial string, if present. |
| `bcd_usb` | `String` | The USB version text. |
| `class` | `u8` | Device class code. |
| `protocol` | `u8` | Device protocol code. |
| `speed` | `String` | Link speed text. |
| `max_power_ma` | `u32` | The configured maximum current in milliamperes. |
| `self_powered` | `bool` | Whether it is self-powered. |
| `configuration` | `Option<String>` | The configuration string, if any. |
| `authorized` | `bool` | Whether the kernel authorized it. |
| `removable` | `String` | The `removable` attribute text. |
| `power_control` | `Option<String>` | The runtime power control setting (`on` or `auto`), if present. |
| `autosuspend_delay_ms` | `Option<i64>` | The autosuspend delay in milliseconds, if present. |
| `runtime_status` | `Option<String>` | The runtime power status text, if present. |
| `maxchild` | `u32` | The number of downstream ports (0 for a non-hub). |
| `interfaces` | `Vec<Interface>` | The interfaces. |
| `ports` | `Vec<Port>` | The downstream ports, for a hub. |
| `hub_interface_protocol` | `Option<u8>` | The hub interface protocol, if a hub. |
| `controller` | `Option<Controller>` | The host controller, for a root hub. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `reserved_reason` (src/audio/devices.rs:132), `reserved_reason_in` (src/audio/devices.rs:139), `parse` (src/audio/devices.rs:166), `list` (src/audio/devices.rs:316), `resolve` (src/audio/devices.rs:330), `is_hdmi` (src/audio/discover.rs:282), `fingerprint` (src/audio/discover.rs:305), `changes` (src/audio/discover.rs:365).

**See also:** test `hdmi_ports_without_a_monitor_report_unavailable_as_evidence (src/audio/devices.rs)`.

### `Tree` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_sysfs_tree]

**Purpose and location.** The whole USB tree: every device by sysfs name, read in one pass. A fresh value is read for each action. Defined in `daemons/bench_mcp/src/usb_hub/sysfs.rs:481` as a pub `struct`. It derives Clone, Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `devices` | `BTreeMap<String, Device>` | Devices keyed by sysfs name. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `domain_of` (src/usb_hub/bandwidth.rs:157), `bandwidth` (src/usb_hub/bandwidth.rs:228), `power_budget` (src/usb_hub/diagnose.rs:15), `missing_target` (src/usb_hub/diagnose.rs:79), `snapshot` (src/usb_hub/history.rs:361), `resolve_port` (src/usb_hub/hubs.rs:176), `port_ref` (src/usb_hub/hubs.rs:211), `resolve_device` (src/usb_hub/mod.rs:278).

**See also:** test `the_recorded_tree_has_the_vnwa_chain_and_its_topology (src/usb_hub/sysfs.rs)`.

### `Dir` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_testkit_dir]

**Purpose and location.** A temporary directory for the hub tests, removed when dropped (it wraps `TestDir`). Defined in `daemons/bench_mcp/src/usb_hub/testkit.rs:9` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `0` | `crate::testdir::TestDir` | The wrapped `TestDir`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `resolve` (src/audio/devices.rs:330), `pick` (src/audio/tools.rs:49), `new` (src/radioctl/tests.rs:17), `virtual_bench` (src/radioctl/tests.rs:45), `sysfs` (src/rx888/mod.rs:865), `new` (src/rx888/mod.rs:881), `none` (src/usb_hub/protect.rs:329), `new` (src/usb_hub/testkit.rs:12).

**See also:** test `the_recorded_devbox_lists_its_sinks_and_sources_with_defaults (src/audio/devices.rs)`.

### `Behaviour` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_testkit_behaviour]

**Purpose and location.** What the scripted hub does when asked to switch a port off. Defined in `daemons/bench_mcp/src/usb_hub/testkit.rs:53` as a pub `enum`. It derives Clone, Copy, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Switches` | `Switches` | The device disappears and comes back with a new device number. |
| `Ignores` | `Ignores` | The command succeeds but the device stays (a hub that does not really switch). |
| `Fails` | `Fails` | The command fails. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/usb_hub/testkit.rs:72).

**See also:** test `an_empty_powered_port_is_a_powered_port_with_nothing_connected (src/usb_hub/diagnose.rs)`.

### `FakeHub` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_testkit_fake_hub]

**Purpose and location.** A stand-in for `sudo -n uhubctl` that edits a copy of sysfs and answers a plain status request from a recording. Defined in `daemons/bench_mcp/src/usb_hub/testkit.rs:63` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `sys` | `PathBuf` | The copy of sysfs it edits. |
| `behaviour` | `Behaviour` | What it does when told to switch a port off. |
| `calls` | `RefCell<Vec<String>>` | The commands it received (behind a RefCell). |
| `parked` | `RefCell<Option<(PathBuf, PathBuf, String)>>` | While a device is switched off, the (original path, parked path, name) of what was moved aside (behind a RefCell). |
| `next_devnum` | `RefCell<u32>` | The device number the next re-enumeration gets (behind a RefCell). |

**Invariants, ownership and concurrency.** Plain data: its fields hold RefCell (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/usb_hub/testkit.rs:72).

**See also:** test `an_empty_powered_port_is_a_powered_port_with_nothing_connected (src/usb_hub/diagnose.rs)`.

### `ProcessRunner` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_uhubctl_process_runner]

**Purpose and location.** The production runner: executes the commands (`sudo -n uhubctl`, `lsusb`) as real processes. It is what can really switch hub port power. Defined in `daemons/bench_mcp/src/usb_hub/uhubctl.rs:14` as a pub `struct`. It derives no traits.

**Fields.** none (unit or opaque definition).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `sudo_failures_say_what_to_fix (src/usb_hub/uhubctl.rs)`.

### `Switching` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_uhubctl_switching]

**Purpose and location.** How uhubctl describes a hub's power switching (its `lpsm` field). Defined in `daemons/bench_mcp/src/usb_hub/uhubctl.rs:68` as a pub `enum`. It derives Clone, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `PerPort` | `PerPort` | Each port can be switched alone. |
| `Ganged` | `Ganged` | All ports switch together. |
| `None` | `None` | The hub cannot switch power. |
| `Unknown` | `Unknown(String)` | Any other value, with its text. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `switching_of` (src/usb_hub/uhubctl.rs:195), `bracket` (src/usb_hub/uhubctl.rs:206).

**See also:** test `the_recorded_uhubctl_output_parses_with_and_without_descriptions (src/usb_hub/uhubctl.rs)`.

### `PortBits` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_uhubctl_port_bits]

**Purpose and location.** A port status word decoded (USB 2 and USB 3 hubs lay the word out differently). Defined in `daemons/bench_mcp/src/usb_hub/uhubctl.rs:109` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `word` | `u16` | The raw 16-bit status word. |
| `powered` | `bool` | Port power is on. |
| `connected` | `bool` | A device is connected. |
| `enabled` | `bool` | The port is enabled. |
| `suspended` | `Option<bool>` | Whether suspended (USB 2 only, so optional). |
| `over_current` | `bool` | An over-current condition is reported. |
| `reset` | `bool` | The port is in reset. |
| `speed` | `&'static str` | Link speed name. |
| `link_state` | `Option<&'static str>` | The USB 3 link state name, if any. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `decode_port` (src/usb_hub/uhubctl.rs:121).

**See also:** test `port_words_decode_for_usb2_and_usb3 (src/usb_hub/uhubctl.rs)`.

### `UPort` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_uhubctl_uport]

**Purpose and location.** One port line of uhubctl output. Defined in `daemons/bench_mcp/src/usb_hub/uhubctl.rs:175` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `number` | `u32` | The port number. |
| `bits` | `PortBits` | The decoded status word. |
| `flags` | `Vec<String>` | The flag words uhubctl printed. |
| `device_vidpid` | `Option<String>` | The connected device's ids, if shown. |
| `device_text` | `Option<String>` | The device description, if shown. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_recorded_uhubctl_output_parses_with_and_without_descriptions (src/usb_hub/uhubctl.rs)`.

### `UHub` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_uhubctl_uhub]

**Purpose and location.** One hub block of `uhubctl` output. Defined in `daemons/bench_mcp/src/usb_hub/uhubctl.rs:185` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `location` | `String` | The hub location. |
| `vidpid` | `String` | The hub's ids. |
| `description` | `Option<String>` | The hub's description, if shown. |
| `usb_version` | `String` | The USB version text. |
| `ports` | `u32` | The number of ports. |
| `switching` | `Switching` | The power switching capability. |
| `port_list` | `Vec<UPort>` | The ports, in order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_status` (src/usb_hub/uhubctl.rs:233), `status` (src/usb_hub/uhubctl.rs:292).

**See also:** test `the_recorded_uhubctl_output_parses_with_and_without_descriptions (src/usb_hub/uhubctl.rs)`.

### `HubDescriptor` [@ANCHOR: bench_mcp:bench_mcp_usb_hub_uhubctl_hub_descriptor_type]

**Purpose and location.** The hub class descriptor fields only `lsusb -v` as root shows. Defined in `daemons/bench_mcp/src/usb_hub/uhubctl.rs:299` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `characteristics` | `u16` | The hub characteristics word. |
| `ports` | `u32` | The number of ports. |
| `power_on_to_good_ms` | `u32` | Power-on to power-good time in milliseconds. |
| `controller_current_ma` | `u32` | The controller current in milliamperes. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_hub_descriptor` (src/usb_hub/uhubctl.rs:337), `hub_descriptor` (src/usb_hub/uhubctl.rs:378).

**See also:** test `the_hub_descriptor_from_lsusb_gives_switching_overcurrent_and_think_time (src/usb_hub/uhubctl.rs)`.
