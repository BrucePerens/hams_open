# The USB Hub Tools Read the Tree From sysfs Without Root and Read What Only Root Can See From uhubctl and lsusb

As the person (or the AI agent) running the bench, I want `bench_mcp` to know exactly which USB device hangs from which hub port, at what speed, with
which interfaces, endpoints, drivers and `/dev` nodes, and to know what each hub can really do (switch one port or all of them, report over-current,
how many transaction translators it has), so that the tools can say "this is `/dev/ttyUSB0`, it is on port 2 of hub `3-2`, and that hub is only
full-speed". This story documents the two readers behind that: `daemons/bench_mcp/src/usb_hub/sysfs.rs` (the tree, read from `/sys/bus/usb/devices` with
no privilege) and `daemons/bench_mcp/src/usb_hub/uhubctl.rs` (`sudo -n uhubctl` and `sudo -n lsusb -v`, the parsers for their text, and the decoding of
port status words). The module root, hub report, ladder, power cycle and protection list are in `usb_hub_a.md`; the tool pages for the agent are
`docs/USB_HUB_TOOLS.md`.

None of these functions keys a transmitter. The `sysfs.rs` functions only read files. The `uhubctl.rs` functions that run a program do so through
`sudo -n`, so a missing sudoers rule is an error and never a password prompt; the functions here only run `uhubctl -N` (status) and `lsusb -v` (read
only). Switching a port's power is done elsewhere (`power.rs`), under the `usb_hub` instrument tier and the protection list. Tests read a recording of
this development machine's `/sys`, `uhubctl` and `lsusb` output (`tests/usb_fixtures/devbox`); nothing touches real hardware.

Terms: a device is named as sysfs names it. `usb3` is the root hub of bus 3. `3-2` is the device on port 2 of `usb3`. `3-2.2.1` is port 1 of the hub
`3-2.2`. `3-2:1.0` (with a colon) is an interface of `3-2`, not a device.

## Speed and transaction translators

### `bench_mcp_usb_hub_sysfs_device_speed_name` [@ANCHOR: bench_mcp_usb_hub_sysfs_device_speed_name]

`Device::speed_name(&self) -> &'static str` turns the kernel's negotiated `speed` (a number of Mb/s kept as text in `Device::speed`) into the name shown in
reports. It is called by `device_json` (the `speed` key) and by the topology and diagnose reports.

| Argument | Type | Meaning |
|---|---|---|
| `self` | `&Device` | Borrowed. Only `speed` is read; it is parsed as a floating point number of Mb/s by `speed_mbps()`, and text that does not parse counts as 0. |

Returns, by the first rule that matches: 20000 or more `"SuperSpeed+ 20 Gb/s"`; 10000 or more `"SuperSpeed+ 10 Gb/s"`; 5000 or more `"SuperSpeed 5 Gb/s"`;
480 or more `"high-speed"`; 12 or more `"full-speed"`; above 0 `"low-speed"`; otherwise (empty, 0 or unreadable) `"unknown"`.
Pure, never fails, cannot panic. Example: `speed = "480"` gives `"high-speed"`; `speed = "5000"` gives `"SuperSpeed 5 Gb/s"`.

See also: `bench_mcp_usb_hub_sysfs_device_json`, `bench_mcp_usb_hub_sysfs_speed_finding`. Test: `the_json_tree_nests_children_and_labels_ports` in `sysfs.rs`.

### `bench_mcp_usb_hub_sysfs_device_tt_mode` [@ANCHOR: bench_mcp_usb_hub_sysfs_device_tt_mode]

`Device::tt_mode(&self) -> &'static str` says how a hub arranges its transaction translators. It is shown as the `tt` key of the `hub` object in `device_json`, so a person can see whether a hub has one translator for all its ports or one per port.

| Input read | Meaning |
|---|---|
| `is_hub()` | Class 9, or class 0 with an interface of class 9. |
| `speed_mbps()` | The negotiated speed. |
| `protocol` | The device protocol (`bDeviceProtocol`): 1 single TT, 2 multi-TT. |
| `hub_interface_protocol` | The protocol of the hub interface, when one was found; the multi-TT mode is that interface's alternate setting 1, which the kernel selects when it can. |

Returns, in order: not a hub `"not a hub"`; speed 5000 or more `"none (SuperSpeed hubs route without translators)"`; speed under 480
`"none (a full-speed hub)"`; protocol 2 with interface protocol 2 or unknown `"multi-TT (one per port)"`; protocol 2 with any other interface protocol
`"multi-TT hub running in single-TT mode"`; protocol 1 `"single TT"`; anything else `"none declared"`. Pure, never fails.

See also: `Device::is_hub`, `bench_mcp_usb_hub_sysfs_device_json`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

## Device names and port arithmetic

### `bench_mcp_usb_hub_sysfs_is_device_name` [@ANCHOR: bench_mcp_usb_hub_sysfs_is_device_name]

`is_device_name(n: &str) -> bool` decides whether an entry of `/sys/bus/usb/devices` is a USB device, as opposed to an interface (`3-2:1.0`) or anything
else. `Tree::read` uses it to filter the directory, and `privileged.rs` uses it to check a device name it was given before building a path from it.

| Argument | Type | Meaning |
|---|---|---|
| `n` | `&str` | The entry name, borrowed, no path separators expected. |

Returns true for `usb` followed by one or more digits (a root hub, `usb3`), and for `<bus>-<path>` where `<bus>` is one or more digits and `<path>` is
one or more dot-separated groups of one or more digits (`3-2`, `3-2.2.1`). False for everything else, including the empty string, `usb`, `usb3x`, `3-`,
`3-2.` and `3-2:1.0`. Digits are ASCII only. Pure, never fails.

See also: `Tree::read` (`usb_hub_sysfs_tree`). Test: `path_arithmetic_round_trips`.

### `bench_mcp_usb_hub_sysfs_parent_of` [@ANCHOR: bench_mcp_usb_hub_sysfs_parent_of]

`parent_of(path: &str) -> Option<(String, u32)>` computes the hub and port a device hangs from, from its name alone. `3-2.2.1` is port 1 of `3-2.2`;
`3-2` is port 2 of root hub `usb3`. `device_at` calls it to fill `Device::parent` and `Device::port`; `diagnose.rs` calls it as well.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | A device name as in `is_device_name`; not validated here. Borrowed. |

Returns `Some((hub_name, port))`, with the port a `u32` (1 and up in practice), or `None` when the name starts with `usb` (a root hub has no parent), has
no `-`, or the last group is not a number. It does not check that the parent exists; `Tree` does that by lookup. Pure.

See also: `bench_mcp_usb_hub_sysfs_child_path` (the inverse), `bench_mcp_usb_hub_sysfs_device_at`. Test: `path_arithmetic_round_trips`.

### `bench_mcp_usb_hub_sysfs_child_path` [@ANCHOR: bench_mcp_usb_hub_sysfs_child_path]

`child_path(hub: &str, port: u32) -> String` gives the sysfs name of the device on a given port of a hub: the inverse of `parent_of`. Called by `ports_of`,
`diagnose.rs`, `history.rs`, `power.rs` and `protect.rs` to go from "hub and port number" to a device name.

| Argument | Type | Meaning |
|---|---|---|
| `hub` | `&str` | The hub's name. `usb3` gives children named `3-<port>`; any other name gives `<hub>.<port>`. Not validated. |
| `port` | `u32` | The port number, 1 and up. 0 is not rejected. |

Returns the name as an owned `String`: `child_path("usb3", 2)` is `"3-2"` and `child_path("3-2", 1)` is `"3-2.1"`. The device need not exist. Pure, never fails.

See also: `bench_mcp_usb_hub_sysfs_parent_of`. Test: `path_arithmetic_round_trips`.

## Reading one device from its sysfs directory

### `bench_mcp_usb_hub_sysfs_parse_interval_us` [@ANCHOR: bench_mcp_usb_hub_sysfs_parse_interval_us]

`parse_interval_us(s: &str) -> u32` converts an endpoint's `interval` attribute (text such as `125us` or `1ms`) to microseconds. Private helper of
`endpoints_of`.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&str` | The attribute text; surrounding whitespace is ignored. A number followed by `us` or `ms`. |

Returns microseconds: `125us` gives 125, `8ms` gives 8000. Text with any other suffix (or none), or a number that does not parse, gives 0 (meaning
unknown). Note: the `ms` form multiplies in `u32`, so a value above about 4.29 million ms would overflow (a panic in a debug build, a wrap in a release
build); real endpoint intervals are far below that. Pure.

See also: `bench_mcp_usb_hub_sysfs_endpoints_of`. Test: `endpoint_intervals_are_read_in_microseconds`.

### `bench_mcp_usb_hub_sysfs_endpoints_of` [@ANCHOR: bench_mcp_usb_hub_sysfs_endpoints_of]

`endpoints_of(dir: &Path) -> Vec<Endpoint>` reads the endpoints of one interface directory: the subdirectories named `ep_*`. Sysfs shows the endpoints of the
interface's active alternate setting. Private; called by `interface_of`.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | The interface directory (for example `.../3-2:1.0`), borrowed. |

Each `Endpoint` has `address` (`bEndpointAddress`, parsed as hex; 0 when missing), `kind` (the `type` file text; empty when
missing), `max_packet` (`wMaxPacketSize` parsed as hex, 0 when missing), `interval_us` (the `interval` file through `parse_interval_us`, 0 when missing or
unreadable) and `direction` (the `direction` file; empty when missing). Returns the endpoints sorted by `address`. A directory that cannot be read gives an
empty list; there are no errors. Reads files only, blocking, no locks.

See also: `bench_mcp_usb_hub_sysfs_parse_interval_us`, `bench_mcp_usb_hub_sysfs_interface_of`. Test: `endpoints_are_read_from_their_directories_and_sorted_by_address`.

### `bench_mcp_usb_hub_sysfs_dev_path` [@ANCHOR: bench_mcp_usb_hub_sysfs_dev_path]

`dev_path(class: &str, name: &str) -> Option<String>` maps a device node found under an interface (its kernel class and name) to the path of its file in `/dev`.
Private; used by `class_nodes` to fill `Node::path`.

| Argument | Type | Meaning |
|---|---|---|
| `class` | `&str` | The sysfs class directory name, one of `NODE_CLASSES`. |
| `name` | `&str` | The node's name, for example `ttyUSB0` or `event5`. |

Returns `/dev/<name>` for the classes `tty`, `block`, `hidraw`, `scsi_generic`, `usbmisc`, `video4linux` and `misc`; `/dev/input/<name>` for class `input` when
the name starts with `event`; and `None` otherwise (`net` and `sound` have no node file, and `mouseN`/`jsN` input nodes are not listed). It does not check that
the file exists. Pure.

See also: `bench_mcp_usb_hub_sysfs_class_nodes`. Test: `a_codec_exposes_its_sound_card_hid_node_and_drivers`.

### `bench_mcp_usb_hub_sysfs_collect_nodes` [@ANCHOR: bench_mcp_usb_hub_sysfs_collect_nodes]

`collect_nodes(dir: &Path, depth: u32, out: &mut Vec<Node>)` walks an interface directory looking for the kernel device nodes the interface exposes (a serial
port, a sound card, a network interface, an input device). Private; called by `interface_of` with depth 0.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | The directory to search, borrowed. |
| `depth` | `u32` | Recursion depth; the walk stops when it exceeds 7. |
| `out` | `&mut Vec<Node>` | Nodes are appended here; existing contents are kept. |

A real subdirectory named like a class in `NODE_CLASSES` (`tty`, `block`, `net`, `sound`, `hidraw`, `input`, `usbmisc`, `video4linux`, `misc`, `scsi_generic`) is
handed to `class_nodes`; any other real subdirectory is searched one level deeper. The `power` directory and `ep_*` directories are skipped. Symbolic links
are not followed (the entry type is not resolved), which keeps `driver` and `subsystem` links out of the walk. Unreadable directories are ignored. Returns
nothing; no errors. Blocking file reads only.

See also: `bench_mcp_usb_hub_sysfs_class_nodes`, `bench_mcp_usb_hub_sysfs_interface_of`. Test: `a_codec_exposes_its_sound_card_hid_node_and_drivers`.

### `bench_mcp_usb_hub_sysfs_class_nodes` [@ANCHOR: bench_mcp_usb_hub_sysfs_class_nodes]

`class_nodes(class: &str, dir: &Path, out: &mut Vec<Node>)` lists the nodes inside one class directory, for example the `tty` directory of a USB serial
interface. Private; called by `collect_nodes`.

| Argument | Type | Meaning |
|---|---|---|
| `class` | `&str` | The class name (the directory's own name). |
| `dir` | `&Path` | The class directory, borrowed. |
| `out` | `&mut Vec<Node>` | Nodes appended, in name order within this call. |

Each real subdirectory becomes a `Node { class, name, path, dev, id }`: `name` is the directory name, `path` comes from `dev_path`, `dev` is the `major:minor`
text of the `dev` file when present, and `id` is the `id` file when present (a sound card's short id). Class `input` is special: each `inputN` directory
holds an `eventN` child (and `mouseN`, `jsN`, which are ignored); one `Node` is made per `event*` child, named `eventN`, with `dev` read from that child and
`id` set to the `name` file of the `inputN` directory (the human label of the device). Unreadable directories add nothing. No errors.

See also: `bench_mcp_usb_hub_sysfs_dev_path`, `bench_mcp_usb_hub_sysfs_collect_nodes`. Test: `a_codec_exposes_its_sound_card_hid_node_and_drivers`.

### `bench_mcp_usb_hub_sysfs_interface_of` [@ANCHOR: bench_mcp_usb_hub_sysfs_interface_of]

`interface_of(dir: &Path, name: &str) -> Interface` reads one interface directory of a device (`3-2:1.0`). Private; called by `device_at`.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | The interface's real directory, borrowed. |
| `name` | `&str` | The interface's name, copied into `Interface::name`. |

Returns an `Interface` with `number` (`bInterfaceNumber`, decimal) and `alt` (`bAlternateSetting`, decimal), `class`, `subclass` and `protocol` (the
`bInterface*` files, hex, 0 when missing), `driver` (the file name the `driver` symbolic link points to, `None` when the interface has no driver bound),
`nodes` (from `collect_nodes`) and `endpoints` (from `endpoints_of`). Missing or unparseable numeric files give 0; there are no errors. Blocking file reads.

See also: `bench_mcp_usb_hub_sysfs_device_at`, `bench_mcp_usb_hub_sysfs_collect_nodes`, `bench_mcp_usb_hub_sysfs_endpoints_of`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

### `bench_mcp_usb_hub_sysfs_ports_of` [@ANCHOR: bench_mcp_usb_hub_sysfs_ports_of]

`ports_of(hub: &str, dir: &Path, nports: u32) -> Vec<Port>` reads the per-port state of a hub from the `<hub>-portN` directories the kernel keeps under the hub's
interface directory. Private; called by `device_at` only for a device whose `maxchild` is above 0.

| Argument | Type | Meaning |
|---|---|---|
| `hub` | `&str` | The hub's sysfs name; used for `Port::hub` and to name each port's child. |
| `dir` | `&Path` | The hub's real device directory, borrowed. |
| `nports` | `u32` | The hub's `maxchild`; a port number above `max(nports, 255)` is ignored. |

The function looks inside each subdirectory whose name contains `:` (the interfaces) for entries named `...-portN`. Each becomes a `Port` with `number`,
`state` (the `state` file text; empty when missing), `connect_type` (the `connect_type` file text; empty when missing),
`over_current_count` (the `over_current_count` file, 0 when missing), `disabled` (true when the `disable` file is exactly `1`), `peer` (the name the `peer` link
points to, `None` when there is no such link) and `child` (always `Some(child_path(hub, number))` here: `Tree::read`
clears it when no such device is present). Returns the ports sorted by number; an unreadable directory gives an empty list. No errors. Blocking file reads.

See also: `bench_mcp_usb_hub_sysfs_child_path`, `usb_hub_sysfs_tree`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

### `bench_mcp_usb_hub_sysfs_controller_of` [@ANCHOR: bench_mcp_usb_hub_sysfs_controller_of]

`controller_of(real_root_hub: &Path) -> Option<Controller>` finds the host controller a root hub belongs to. Private; called by `Tree::read` for root hubs.

| Argument | Type | Meaning |
|---|---|---|
| `real_root_hub` | `&Path` | The root hub directory after resolving symbolic links (`canonicalize`), so its parent directory is the controller's. Borrowed. |

Returns `Some(Controller { pci, driver, vendor, device })` where `pci` is the parent directory's name (a PCI address such as `0000:00:14.0` for a PCI
controller), `driver` the name the parent's `driver` link points to, and `vendor` and `device` the text of the parent's `vendor` and `device` files (empty
when missing, as on a controller that is not on PCI). Returns `None` when the path has no parent or the parent has no bound driver. No errors.

See also: `usb_hub_sysfs_tree`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

### `bench_mcp_usb_hub_sysfs_device_at` [@ANCHOR: bench_mcp_usb_hub_sysfs_device_at]

`device_at(dir: &Path, path: &str) -> Option<Device>` builds the `Device` record for one sysfs device directory. Private; called by `Tree::read` for every
entry that passes `is_device_name`.

| Argument | Type | Meaning |
|---|---|---|
| `dir` | `&Path` | The device's real directory, borrowed. |
| `path` | `&str` | The device's sysfs name (`3-2.2`); it names the interfaces (`3-2.2:` prefix) and goes through `parent_of`. |

Returns `None` when `idVendor` or `idProduct` is missing. Otherwise a `Device` with: `vid`, `pid` (text);
`parent` and `port` (from `parent_of`); `bus`, `devnum`, `maxchild` (decimal files, 0 when missing); `manufacturer`, `product`, `serial` (`None` when the file
is missing); `bcd_usb` (the `version` file, empty when missing); `class` and `protocol` (`bDeviceClass`, `bDeviceProtocol`, hex); `speed` (the `speed`
file as text); `max_power_ma` (`bMaxPower` with a trailing `mA` removed, 0 when unreadable); `self_powered` (bit 0x40 of `bmAttributes`, hex);
`configuration`; `authorized` (true unless the file is exactly `0`); `removable` (the `removable` file text); `power_control`,
`autosuspend_delay_ms` and `runtime_status` (from the `power/` files; the delay as a signed number); `interfaces` (every entry beginning with `<path>:`,
sorted by interface number then alternate); `ports` (from `ports_of` when `maxchild` is above 0); `hub_interface_protocol` (the protocol of the first
interface of class 9); and `controller` always `None` here (`Tree::read` fills it afterwards). Reads many small files, blocking, no root needed.

See also: `bench_mcp_usb_hub_sysfs_interface_of`, `bench_mcp_usb_hub_sysfs_ports_of`, `bench_mcp_usb_hub_sysfs_parent_of`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

### `bench_mcp_usb_hub_sysfs_path_key` [@ANCHOR: bench_mcp_usb_hub_sysfs_path_key]

`path_key(p: &str) -> (u32, Vec<u32>, bool)` makes the sort key that puts devices in a natural order. Private; used by `Tree::sorted`.

| Argument | Type | Meaning |
|---|---|---|
| `p` | `&str` | A device name. |

Returns `(bus, ports, has_ports)`. A root hub `usb3` gives `(3, [], false)`; `3-2.10` gives `(3, [2, 10], true)`. Because the numbers are compared as numbers, `3-2.10`
sorts after `3-2.2` (a plain text sort would not), and because `false` sorts before `true` a bus's root hub comes before its devices. Parts that are not
numbers count as 0. Pure.

See also: `bench_mcp_usb_hub_sysfs_tree_sorted`. Test: `devices_sort_by_bus_then_numerically_by_port_with_the_root_hub_first`.

## The tree

### `usb_hub_sysfs_tree` [@ANCHOR: usb_hub_sysfs_tree]

`Tree::read(sysfs: &Path) -> Result<Tree, String>` reads the whole USB tree: every device, with its interfaces, ports and controller. It is the entry point of
the reader; the tools call it each time they need a fresh view, and the tests call it on a recording of `/sys`. The result type is `Tree { devices:
BTreeMap<String, Device> }`, keyed by sysfs name (`Default` is an empty tree); it is a plain value, owned by the caller, never shared or changed after reading.

| Argument | Type | Meaning |
|---|---|---|
| `sysfs` | `&Path` | The sysfs root: `/sys` in production, a copy in tests. The directory read is `<sysfs>/bus/usb/devices`. Borrowed. |

Steps: list the directory; keep the names accepted by `is_device_name`; resolve each link with `canonicalize` (so the real directory is read); build the device
with `device_at`; for root hubs record the controller (by bus number) with `controller_of`; then give every device its bus's controller, and clear the
`child` of every port whose child device is not present in the tree.

Errors (`Err(String)`): the directory cannot be read (the message is `<path>: <os error>`); or no device at all was found (`no USB devices found under <path>`).
An entry that cannot be resolved or read is skipped silently. Blocking file system reads, no root, no locks, no writes, safe to call repeatedly; the
tree is a snapshot and can be stale at once when a device is plugged or unplugged.

See also: all `bench_mcp_usb_hub_sysfs_tree_*` anchors, `bench_mcp_usb_hub_sysfs_is_device_name`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

### `bench_mcp_usb_hub_sysfs_tree_children` [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_children]

`Tree::children(&self, path: &str) -> Vec<&Device>` returns the devices directly below a hub, in port order.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | The hub's sysfs name (`usb3`, `3-2`). It need not exist. |

Returns borrowed references into the tree, sorted by `Device::port`. A name that is not in the tree, or a device with nothing below it, gives an empty vector.
Pure, scans every device (cost proportional to the tree size).

See also: `bench_mcp_usb_hub_sysfs_tree_subtree`, `bench_mcp_usb_hub_sysfs_tree_json`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

### `bench_mcp_usb_hub_sysfs_tree_roots` [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_roots]

`Tree::roots(&self) -> Vec<&Device>` returns the root hubs (devices with no parent), one per bus, sorted by bus number. No arguments; an empty tree gives an empty
vector. Borrowed references, pure.

See also: `bench_mcp_usb_hub_sysfs_tree_children`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

### `bench_mcp_usb_hub_sysfs_tree_ancestry` [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_ancestry]

`Tree::ancestry(&self, path: &str) -> Vec<&Device>` returns a device and every hub above it, the device first and the root hub last. It is used where a report needs the hubs a device sits behind.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | The device's sysfs name. |

Returns borrowed references. An unknown name gives an empty vector. The walk stops early, without an error, if a parent named by a device is not present in the
tree. It cannot loop because each name's parent is shorter than the name. Pure.

See also: `bench_mcp_usb_hub_sysfs_parent_of`. Test: `the_recorded_tree_has_the_vnwa_chain_and_its_topology`.

### `bench_mcp_usb_hub_sysfs_tree_subtree` [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_subtree]

`Tree::subtree(&self, path: &str) -> Vec<&Device>` returns the device itself and every device below it, breadth first (the device first, then its children in port
order, then theirs). `protect.rs` uses it to look at every device below a hub port.

| Argument | Type | Meaning |
|---|---|---|
| `path` | `&str` | The top device's sysfs name. |

Returns borrowed references. An unknown name gives an empty vector. Pure; cost is proportional to the tree size times the number of devices returned.

See also: `bench_mcp_usb_hub_sysfs_tree_children`. Test: `a_subtree_is_the_device_and_everything_below_it`.

### `bench_mcp_usb_hub_sysfs_tree_sorted` [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_sorted]

`Tree::sorted(&self) -> Vec<&Device>` returns every device in the natural order of `path_key`: by bus, the root hub first, then by port numbers compared as
numbers (`3-2.2` before `3-2.10`). Borrowed references. No arguments, no errors.

See also: `bench_mcp_usb_hub_sysfs_path_key`. Test: `devices_sort_by_bus_then_numerically_by_port_with_the_root_hub_first`.

## Findings and JSON

### `bench_mcp_usb_hub_sysfs_speed_finding` [@ANCHOR: bench_mcp_usb_hub_sysfs_speed_finding]

`speed_finding(d: &Device, parent: Option<&Device>) -> Option<Value>` says whether a device runs slower than it should, in a form the diagnose and topology
reports can show. A `bcdUSB` of 2.x does not prove high-speed capability (many full-speed devices report 2.0), so that case is only a note; only a USB 3
device on a slower link is a warning.

| Argument | Type | Meaning |
|---|---|---|
| `d` | `&Device` | The device; `bcd_usb` (parsed as a number, 0 if unparseable) and `speed` are used. Borrowed. |
| `parent` | `Option<&Device>` | The hub it hangs from, used only to word the reason; `None` when unknown. |

Returns `None` for a root hub, for a device whose speed is unknown or 0, and when there is nothing to report. Otherwise a JSON object `{"level": "warn" |
"note", "text": ...}`. `warn`: bcdUSB 3.0 or more and speed under 5000 Mb/s; the text blames the parent hub when it is itself under 5000 Mb/s, otherwise a USB 2.0
port, a USB 2.0 cable or a USB 3 link that failed to train. `note`: bcdUSB from 2.0 up to (not including) 3.0 and speed above 1.5 and up to 12 Mb/s;
the text says the device reports USB 2 but runs at full speed, because its parent is full-speed only or it may simply be full-speed only. Pure.

See also: `bench_mcp_usb_hub_sysfs_device_json`. Test: `speed_findings_separate_a_warning_from_a_note`.

### `bench_mcp_usb_hub_sysfs_device_json` [@ANCHOR: bench_mcp_usb_hub_sysfs_device_json]

`device_json(t: &Tree, d: &Device, labels: &dyn Fn(&str, u32) -> Option<String>) -> Value` renders one device as the JSON the topology and find tools
return. It is the one place that decides the JSON field names.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree` | The tree, used to find the parent (for the speed finding) and for the power budget. Borrowed. |
| `d` | `&Device` | The device to render, borrowed. |
| `labels` | `&dyn Fn(&str, u32) -> Option<String>` | Looks up the user's label for a port, given the hub's name and the port number; return `None` for no label. (The older doc comment calls the key `hub/port`; the code passes the two values separately.) |

Returns an object with `path`, `label` (the label of the port this device is on), `bus`, `devnum`, `vidpid`, `manufacturer`, `product`, `serial`, `parent`, `port`,
`usb_version`, `speed_mbps` (the text), `speed` (`speed_name`), `speed_finding` (null when none), `class` (two hex digits), `is_hub`, `max_power_ma`,
`self_powered`, `power_budget` (from `diagnose::power_budget`), `authorized`, `removable`, `power` (`control`, `autosuspend_delay_ms`, `runtime_status`),
`controller` (`pci`, `driver`, `vendor`, `device`, or null), `hub` (null unless the device is a hub; otherwise `ports`, `tt` from `tt_mode`, and
`port_status`, one entry per port with `port`, `label`, `state`, `connect_type`, `over_current_count`, `disabled_by_software`, `peer_port` and `device`) and
`interfaces` (each with `name`, `number`, `alt`, `class`, `subclass`, `protocol` as two hex digits, `driver` and `nodes` of `class`, `name`, `dev`, `path`, `id`).
Missing optional values are JSON null. Endpoints are not included. Pure; never fails.

See also: `bench_mcp_usb_hub_sysfs_tree_json`, `bench_mcp_usb_hub_sysfs_speed_finding`, `bench_mcp_usb_hub_sysfs_device_tt_mode`. Test: `the_json_tree_nests_children_and_labels_ports`.

### `bench_mcp_usb_hub_sysfs_tree_json` [@ANCHOR: bench_mcp_usb_hub_sysfs_tree_json]

`tree_json(t: &Tree, root: &str, labels: &dyn Fn(&str, u32) -> Option<String>) -> Value` renders a device and everything below it as nested JSON: each
`device_json` object gains a `children` array of the same shape, in port order.

| Argument | Type | Meaning |
|---|---|---|
| `t` | `&Tree` | The tree, borrowed. |
| `root` | `&str` | The sysfs name to start from (`usb3` for a whole bus, or any device). |
| `labels` | `&dyn Fn(&str, u32) -> Option<String>` | The port label lookup, passed unchanged to `device_json`. |

Returns the JSON object, or `null` when `root` is not in the tree. The recursion is as deep as the tree (at most a handful of hubs); names cannot form a loop. Pure.

See also: `bench_mcp_usb_hub_sysfs_device_json`, `bench_mcp_usb_hub_sysfs_tree_children`. Test: `the_json_tree_nests_children_and_labels_ports`.

## Running the privileged tools

### `bench_mcp_usb_hub_uhubctl_processrunner_run` [@ANCHOR: bench_mcp_usb_hub_uhubctl_processrunner_run]

`ProcessRunner::run(&self, program: &str, args: &[String]) -> Result<(bool, String), String>` is the production implementation of the `Runner` trait (the seam
tests replace with a scripted runner). It starts a program, waits for it to end, and returns whether it succeeded and what it printed.

| Argument | Type | Meaning |
|---|---|---|
| `program` | `&str` | The program name, looked up on `PATH`; no shell is involved, so nothing is interpreted. |
| `args` | `&[String]` | Its arguments, one per element. |

Returns `Ok((success, text))`: `success` is true when the exit status is 0, and `text` is the standard output followed by the standard error (lossy UTF-8).
`Err(String)` only when the program could not be started: `<program> is not installed (not found on PATH)` when it does not exist, otherwise
`<program>: <os error>`. A nonzero exit is not an error here; the caller decides. Blocking: it waits for the child with no timeout, so callers must not hold a
lock the rest of the server needs. The child gets no standard input. Side effect: whatever the program does (here `sudo` and then `uhubctl` or `lsusb`).

See also: `bench_mcp_usb_hub_uhubctl_sudo`. Test: `sudo_failures_say_what_to_fix`.

### `bench_mcp_usb_hub_uhubctl_explain_failure` [@ANCHOR: bench_mcp_usb_hub_uhubctl_explain_failure]

`explain_failure(tool: &str, text: &str) -> String` turns the text of a failed `sudo -n <tool>` into a message that says what to fix. Called by `sudo` and by
the ladder (`ladder.rs`).

| Argument | Type | Meaning |
|---|---|---|
| `tool` | `&str` | The tool that was run (`uhubctl`, `lsusb`). |
| `text` | `&str` | Its combined output; trimmed. |

Returns the first matching message: text with `a password is required` or `a terminal is required` gives `sudo -n <tool> needs a password: add a NOPASSWD
sudoers rule for <tool> for this user (<text>)`; text with `command not found`, or with `not found` and the tool name, gives `<tool> is not installed (sudo
could not find it): <text>`; text with `not in the sudoers` or `is not allowed to execute` gives `sudo does not allow this user to run <tool>: <text>`; anything
else gives `sudo -n <tool> failed: <text>`. The order matters: a password complaint wins. The matching is on English sudo messages, so a localized sudo falls
to the last, generic, message. Pure, never fails.

See also: `bench_mcp_usb_hub_uhubctl_sudo`. Test: `sudo_failures_say_what_to_fix`.

### `bench_mcp_usb_hub_uhubctl_sudo` [@ANCHOR: bench_mcp_usb_hub_uhubctl_sudo]

`sudo(run: &dyn Runner, tool: &str, args: &[&str]) -> Result<String, String>` runs `sudo -n <tool> <args...>` through a `Runner` and returns its text. It is
the only way this crate starts a privileged program for the USB tools (status, hub descriptor, history and power switching call it).

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Runner` | `ProcessRunner` in production, a script in tests. Borrowed. |
| `tool` | `&str` | The program for sudo to run; chosen by the calling code, never by the agent's text. |
| `args` | `&[&str]` | The tool's arguments. |

Returns `Ok(text)` (standard output followed by standard error) when the program exits 0. `Err(String)`: the runner failed to start `sudo` (the message begins
`sudo is not installed: ...`; because `ProcessRunner` already says `sudo is not installed ...` the sentence can read doubled); any other runner error as is;
or a nonzero exit, explained by `explain_failure`. `-n` makes sudo fail at once instead of asking for a password, so a missing NOPASSWD rule is an error,
never a hang. Blocking; no timeout of its own. Whether the call can change hardware depends on `tool` and `args`: `uhubctl -N` only reads, `lsusb -v` only
reads, and a power switch is made by `power.rs`, under the `usb_hub` tier and the protection list. Nothing here transmits.

See also: `bench_mcp_usb_hub_uhubctl_explain_failure`, `bench_mcp_usb_hub_uhubctl_processrunner_run`. Test: `sudo_failures_say_what_to_fix`.

## What uhubctl reports

### `bench_mcp_usb_hub_uhubctl_switching_name` [@ANCHOR: bench_mcp_usb_hub_uhubctl_switching_name]

`Switching::name(&self) -> String` gives the text shown in reports for a hub's power-switching ability. `Switching` has four variants: `PerPort` (each port can be
switched on its own), `Ganged` (all ports switch together), `None` (the hub cannot switch power) and `Unknown(String)` (a mode word uhubctl printed that this code
does not know, kept as it was). It is created by `switching_of` and held in `UHub::switching`.

Returns `"per-port"`, `"ganged"`, `"none"` or `"unknown (<word>)"`. Pure, never fails.

See also: `bench_mcp_usb_hub_uhubctl_switching_of`, `bench_mcp_usb_hub_uhubctl_hubdescriptor_switching`. Test: `switching_modes_have_their_report_names`.

### `bench_mcp_usb_hub_uhubctl_link_state` [@ANCHOR: bench_mcp_usb_hub_uhubctl_link_state]

`link_state(n: u16) -> &'static str` names the link state of a SuperSpeed port, the 4-bit value in bits 5 to 8 of its status word (`decode_port` passes it).

| Argument | Type | Meaning |
|---|---|---|
| `n` | `u16` | The link state number, 0 to 15. |

Returns `U0`, `U1`, `U2`, `U3 (suspended)`, `SS.Disabled`, `Rx.Detect`, `SS.Inactive`, `Polling`, `Recovery`, `Hot Reset`, `Compliance`, `Loopback` for 0 to 11 in that
order, and `reserved` for any other value. Pure.

See also: `bench_mcp_usb_hub_uhubctl_decode_port`. Test: `port_words_decode_for_usb2_and_usb3`.

### `bench_mcp_usb_hub_uhubctl_decode_port` [@ANCHOR: bench_mcp_usb_hub_uhubctl_decode_port]

`decode_port(word: u16, usb3: bool) -> PortBits` decodes a port status word, as uhubctl prints it in hexadecimal, into named flags. USB 2 and USB 3 hubs lay the
word out differently, so the caller says which one it has (`parse_status` passes true when the hub's USB version starts with 3).

| Argument | Type | Meaning |
|---|---|---|
| `word` | `u16` | The status word. |
| `usb3` | `bool` | True for a SuperSpeed hub's layout. |

`PortBits` (owned, plain data, `PartialEq`): `word` (the input), `powered`, `connected` (bit 0), `enabled` (bit 1), `over_current` (bit 3), `reset` (bit 4),
`suspended` (`Some(bit 2)` for USB 2, always `None` for USB 3), `speed` and `link_state` (`Some(name)` for USB 3, `None` for USB 2). Differences: USB 3 `powered`
is bit 9, USB 2 bit 8. USB 3 `speed` is bits 10 to 12: 0 `"SuperSpeed 5 Gb/s"`, 1 `"SuperSpeed+ 10 Gb/s"`, other `"unknown"`, and the link state is bits 5 to 8
through `link_state`. USB 2 `speed` is `"low-speed"` when bit 9 is set, else `"high-speed"` when bit 10, else `"full-speed"` when connected, else `"none"`.
`PortBits::json()` renders it with the word as four hex digits. Pure, never fails.

See also: `bench_mcp_usb_hub_uhubctl_link_state`, `usb_hub_uhubctl_parse`. Test: `port_words_decode_for_usb2_and_usb3`.

### `bench_mcp_usb_hub_uhubctl_switching_of` [@ANCHOR: bench_mcp_usb_hub_uhubctl_switching_of]

`switching_of(s: &str) -> Switching` converts the last word of a uhubctl hub line to a `Switching`. Private; called by `bracket`.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&str` | The word: `ppps`, `ganged` or `nops`. |

Returns `PerPort` for `ppps`, `Ganged` for `ganged`, `None` for `nops`, and `Unknown(s)` for any other word (so a new uhubctl version is shown, not an error). Pure.

See also: `bench_mcp_usb_hub_uhubctl_switching_name`, `bench_mcp_usb_hub_uhubctl_bracket`. Test: `the_recorded_uhubctl_output_parses_with_and_without_descriptions`.

### `bench_mcp_usb_hub_uhubctl_bracket` [@ANCHOR: bench_mcp_usb_hub_uhubctl_bracket]

`bracket(s: &str) -> Option<(String, Option<String>, String, u32, Switching)>` parses the bracketed part of a uhubctl hub line, for example `14b0:013a Generic 4-Port
USB 2.0 Hub, USB 2.10, 5 ports, ppps`. Private; called by `parse_status`.

| Argument | Type | Meaning |
|---|---|---|
| `s` | `&str` | The text between the square brackets, borrowed. |

The text is split at `, `. At least 4 parts are required. The last three are the USB version (`USB <v>`, the prefix removed), the port count (`<n> ports`) and the
switching word; the first part is the vendor and product id and, after a space, the description. Returns `Some((vidpid, description, usb_version, ports,
switching))` with the description `None` when uhubctl printed none (the `-N` form). Returns `None` when there are fewer than 4 parts, the version lacks `USB `,
the port count lacks ` ports` or is not a number, or the id is not nine characters with a colon in the fifth (`xxxx:xxxx`). Note: a description that itself contains
`, ` loses the text after its first comma, because only the first part is kept. Pure.

See also: `bench_mcp_usb_hub_uhubctl_switching_of`, `usb_hub_uhubctl_parse`. Test: `the_recorded_uhubctl_output_parses_with_and_without_descriptions`.

### `usb_hub_uhubctl_parse` [@ANCHOR: usb_hub_uhubctl_parse]

`parse_status(text: &str) -> Result<Vec<UHub>, String>` parses the output of `uhubctl` run without options (or with `-N`) into one `UHub` per hub block, each with its
ports. `status(run)` (not anchored) runs `sudo -n uhubctl -N` through `sudo` and calls it; the hub report and diagnose call `status`.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | The whole output, borrowed. |

A line starting `Current status for hub <location> [<bracket>]` starts a hub; a line (after trimming) starting `Port <n>: <hex word> <flags> [<device text>]` adds a
port to the latest hub; every other line is ignored. Data structures, owned plain values: `UHub { location (for example `3-2`), vidpid, description
(`None` with `-N`), usb_version (for example `2.10`), ports (the count the hub declares), switching, port_list }` and `UPort { number, bits (a `PortBits`
from `decode_port`, with the hub's USB 2 or 3 layout chosen by `usb_version` starting with `3`), flags (the words between the word and the bracket, such as
`power enable connect`), device_vidpid (the first nine characters of the bracket), device_text (the whole bracket text) }`.

Errors (`Err`): a hub line without ` [` or the closing `]` or with an unparseable bracket (`unreadable hub line: ...`); a port line before any hub
(`port line before any hub: ...`); a port line without `: ` (`unreadable port line`), with a status word that is not hex (`unreadable port status in`) or a port
number that is not a number (`unreadable port number in`); and no hub at all (`no hubs in uhubctl output: <text>`). Pure; the text of the output is the only input.

See also: `bench_mcp_usb_hub_uhubctl_bracket`, `bench_mcp_usb_hub_uhubctl_decode_port`. Test: `the_recorded_uhubctl_output_parses_with_and_without_descriptions`.

## The hub class descriptor (lsusb -v)

### `bench_mcp_usb_hub_uhubctl_hubdescriptor_switching` [@ANCHOR: bench_mcp_usb_hub_uhubctl_hubdescriptor_switching]

`HubDescriptor::switching(&self) -> &'static str` reads the power-switching mode from `wHubCharacteristics` of the hub class descriptor. `HubDescriptor` (owned
plain data) holds `characteristics` (the 16-bit field), `ports` (`nNbrPorts`), `power_on_to_good_ms` (`bPwrOn2PwrGood` doubled, in milliseconds) and
`controller_current_ma` (`bHubContrCurrent`, the number as printed, taken as mA).
Returns, from bits 0 and 1: 0 `"ganged"`, 1 `"per-port"`, 2 or 3 `"none"`. Pure.

See also: `bench_mcp_usb_hub_uhubctl_switching_name`, `bench_mcp_usb_hub_uhubctl_parse_hub_descriptor`. Test: `the_hub_descriptor_from_lsusb_gives_switching_overcurrent_and_think_time`.

### `bench_mcp_usb_hub_uhubctl_hubdescriptor_over_current` [@ANCHOR: bench_mcp_usb_hub_uhubctl_hubdescriptor_over_current]

`HubDescriptor::over_current(&self) -> &'static str` reads how the hub reports over-current from bits 3 and 4 of `characteristics`: 0 `"global"` (one signal for the
whole hub), 1 `"per-port"`, 2 or 3 `"none"`. Pure, never fails.
Related readers of the same field (not anchored): `compound()` (bit 2 set), `think_time_fs_bits()` (`8 * (1 + bits 5 and 6)` full-speed bit times, for USB 2 hubs)
and `port_indicators()` (bit 7 set).

See also: `bench_mcp_usb_hub_uhubctl_hubdescriptor_switching`. Test: `the_hub_descriptor_from_lsusb_gives_switching_overcurrent_and_think_time`.

### `bench_mcp_usb_hub_uhubctl_parse_hub_descriptor` [@ANCHOR: bench_mcp_usb_hub_uhubctl_parse_hub_descriptor]

`parse_hub_descriptor(text: &str) -> Option<HubDescriptor>` extracts the hub class descriptor from the output of `lsusb -v` for one device.

| Argument | Type | Meaning |
|---|---|---|
| `text` | `&str` | The full `lsusb -v` text, borrowed. |

It finds the line beginning `Hub Descriptor:` and reads the following lines while they begin with a space (it stops at the first line that does not, a blank
line included). Recognised lines: `wHubCharacteristic` (the first word after it is hex, with or without `0x`), `nNbrPorts`, `bPwrOn2PwrGood` (the number is doubled to
give milliseconds) and `bHubContrCurrent`. Missing numeric fields stay 0. Returns `None` when there is no `Hub Descriptor:` line or no `wHubCharacteristic`
line, or its value is not hex. Pure.

See also: `bench_mcp_usb_hub_uhubctl_hub_descriptor`. Test: `the_hub_descriptor_from_lsusb_gives_switching_overcurrent_and_think_time`.

### `bench_mcp_usb_hub_uhubctl_hub_descriptor` [@ANCHOR: bench_mcp_usb_hub_uhubctl_hub_descriptor]

`hub_descriptor(run: &dyn Runner, bus: u32, devnum: u32) -> Result<HubDescriptor, String>` runs `sudo -n lsusb -v -s <bus>:<devnum>` and parses its hub
descriptor. Only root can read this descriptor, so it goes through `sudo`. The hub report calls it (when the tool tier allows privileged reads) for every hub.

| Argument | Type | Meaning |
|---|---|---|
| `run` | `&dyn Runner` | The runner. |
| `bus` | `u32` | `Device::bus`. |
| `devnum` | `u32` | `Device::devnum`; the number changes each time a device re-enumerates, so read it fresh from a new `Tree`. |

Returns the `HubDescriptor`, or `Err`: whatever `sudo` returns for a failure (a missing NOPASSWD rule, `lsusb` not installed), or `lsusb printed no hub descriptor
for <bus>:<devnum>` when the device is not a hub or the text has none. Blocking, read only; it changes nothing on the bus.

See also: `bench_mcp_usb_hub_uhubctl_sudo`, `bench_mcp_usb_hub_uhubctl_parse_hub_descriptor`. Test: `the_hub_descriptor_is_read_through_sudo_lsusb_for_one_bus_and_device`.
