# hams-relay-broker (prototype)

The relay's privileged broker from `hams_com/docs/proposals/RELAY_ZERO_PRIVILEGE_SPLIT.md`
(sections 3.1, 3.5, 3.6, 5, 6), phase 3 of that plan, built as a standalone crate. **Nothing here
is wired into the relay or installed by any package**; the integration path is at the end.

A small separate process (about 2,000 lines of source without tests; no web, no async runtime,
no FFI, no shell, two commented `unsafe` blocks that adopt kernel-provided descriptors) that
answers a fixed set of typed requests from authenticated local peers over a Unix domain socket:

| Action | What it does | Privilege it needs |
|---|---|---|
| `list_devices` | serial ttys and ALSA (the Linux sound system) nodes from sysfs (read only), USB ids, product and manufacturer strings; protected devices are left out | none |
| `open_device` | validates one path against the policy, opens it `O_NOFOLLOW|O_CLOEXEC|O_NOCTTY`, re-checks the open descriptor, passes it with SCM_RIGHTS; the worker never needs `/dev` access and never learns another device's path | group membership (`dialout`, `audio`) or, failing that, one capability (see the unit) |
| `grant_group` / `provision_groups` | reports which of `dialout`/`audio` the **caller's own** account lacks; changes nothing unless `apply_groups = true`, then runs `/usr/sbin/gpasswd -a USER GROUP` with a fixed argv | root or a gpasswd setuid, only with apply |
| `bind_udp` | binds a UDP socket on a port the config lists (default only 123, the relay's time service) on the wildcard address and passes the descriptor; any other port is `Denied` | root (privileged port) |
| `update_package` | asks for the relay package upgrade: creates `/var/lib/hams-relay-broker/update-requested`, which the package's root-owned `hams-relay-update.path` unit turns into the one fixed `apt-get install --only-upgrade hams-local-relay` (or `dnf upgrade`), because the broker itself holds no right to run a package manager; nothing from the caller; only with `update_package = true` in the config (the relay package's config turns it on) |
| `set_system_time` / `time_status` | `set_system_time` steps the system clock to a UTC the relay chose (`CLOCK_REALTIME`; needs `CAP_SYS_TIME`), only with `set_time = true` (the relay package's config turns it on). The broker applies its own ceilings and does not trust the relay: the target must not be before the build date (2026-11-01); a step beyond `max_unproven_step_secs` (default 30) needs a GPS source or two agreeing sources; at most `max_time_steps_per_hour` (default 4) steps an hour, then one more attempt trips a circuit breaker that refuses everything for an hour. Every attempt is audited with the clock before, the target, the source and the evidence. `time_status` is read-only: the clock, steps in the last hour, breaker state, the earliest accepted target | root or `CAP_SYS_TIME` |
| `hub_start` / `hub_stop` / `hub_status` | the stand-alone emergency hub: only with `hub = true` in the config. Validates the typed settings, writes `hostapd.conf` and `dnsmasq.conf` from fixed templates into `/run/hams-relay-broker/hub` (mode 0600), gives the wifi interface the hub address with `ip` and starts `hostapd -B` and `dnsmasq` with fixed argument lists; stop signals only a pid whose `/proc/PID/comm` still names the program, then removes the files | root |

Every request is rate-limited per UID (user ID) and written to an append-only `events.jsonl` audit log (a peer turned away is logged once per UID per minute; the file moves to `events.jsonl.1` at 4 MiB).

## Wire protocol (`src/wire.rs`, `src/transport.rs`)

Length-prefixed frames (`u32` big-endian, 1..=4096 bytes) over a Unix stream socket; the body is
a version byte (1), a tag byte and typed fields; strings are `u16` length + UTF-8, at most 255
bytes, no NUL. The schema is closed: four request variants, four response payloads, six status
codes. Unknown tags, short buffers, trailing bytes and over-long strings are errors, never
panics (`tests/fuzz_parser.rs`, proptest, 2,000 cases per property; the same three functions are
what a cargo-fuzz harness would call). A passed descriptor rides in SCM_RIGHTS alongside the
`Opened` response; descriptors a *client* sends are closed unread.

## Peer authentication (`src/auth.rs`)

`SO_PEERCRED` on accept, before any byte is read. The UID or primary GID must be on the
allow-list in the root-owned config (`/etc/hams-relay-broker.conf`, `key = value`, unknown keys
are errors; it must be owned by uid 0 and not group/world-writable). An empty allow-list answers
nobody. Unlisted peers are logged and closed with no response.

The other direction: a client reads the server's `SO_PEERCRED` after `connect` and sends nothing
unless the uid is one it expects (`client::verify_server`; `connect_and_call` expects root, which
is what a socket-activated broker shows; `connect_and_call_as` names other accounts). A process that
took over the socket path therefore cannot answer the relay, and the relay does not depend on the
installer making the socket's directory writable by root alone.

## Policy (`src/policy.rs`)

Three checks, each closing a different hole: lexical (absolute, under `/dev`, no `.`/`..`/empty
segment, byte-for-byte what will be opened, not on `deny_path`: `/dev/mem`, `/dev/kmem`,
`/dev/port`, `/dev/console`, `/dev/tty`), node walk (no symlink anywhere in the chain, so
`/dev/stdin`, `/dev/fd/0`, `/dev/serial/by-id/...` and a planted link are all refused; the final
node is a character device whose major belongs to the requested class: 4/166/188/204/207
serial, 116 ALSA; disks are block devices and fail here), USB identity (the VNWA codec
`08bb:29c0` and anything on `deny_usb`), and a post-open `fstat` re-check so a swap between the
walk and the open fails closed.

## Threat model

Protects against: a compromised relay worker (web parser, serial reply parser, decoder) using the
broker to open a device it was not meant to have (a disk, `/dev/mem`, another rig's port, the
VNWA), to reach a device by a symlink or a traversal, to flood the broker, or to change group
membership of any account but its own; a stranger on the machine talking to the socket (socket
mode 0660 plus the allow-list); a malformed or oversize request crashing or stalling the broker
(bounded frames, strict parser, I/O timeouts, one request per connection); provisioning changing
the machine without an explicit configuration decision (dry run by default). No action prompts,
blocks on an operator, or asks for a password: the emergency path has none of those (3.5, 3.6).

NOT protected: a compromised **core** (it holds the keys; the broker never sees them and cannot
help); a worker that already holds a descriptor (the broker is off the data path by design, 3.5);
the config file itself (root-owned by assumption; the broker refuses other owners but cannot
defend against root); `pid` in the audit log (reusable, informational); the gpasswd binary and
`/etc/group` on apply; a kernel driver bug reachable through a *legitimately* opened device;
denial of service by a legitimate peer against itself (rate limit is per UID); TOCTOU (time-of-check to time-of-use race) on sysfs
USB identity (the id is read before the open; a device re-enumerating between the two is a
hardware event, not an attacker); macOS and Windows (the `Os` trait is the seam, nothing is
implemented there yet).

## Root-only paths and how they are tested

Tests run unprivileged (`socketpair`, temporary directories, the real `/dev/null`, a sysfs tree
built in a tempdir, `/etc/group`-shaped files). What cannot run without root and is covered only
by the `Os` trait fake plus review: opening a real `/dev/ttyUSB*` as a non-member of `dialout`,
a successful `gpasswd` apply, the root-ownership check on the config (the same code path is
exercised with the test's own UID), and the systemd units (examples under `packaging/systemd`,
never installed; `CapabilityBoundingSet=` empty by default, one capability to be added per
action only when measured necessary).

The emergency hub's generated configuration has one more test, `tests/hub_netns.rs`, skipped unless `HAMS_HUB_NETNS=1`. It runs the real `dnsmasq`
from `hub::dnsmasq_conf` in a network namespace joined to a client namespace by a veth pair (no Wi-Fi), takes a DHCP (automatic network address assignment) lease (router, DNS server and NTP
server options, pool, lease time) and resolves the relay name and the captive-portal probe names against it; `hostapd` cannot run without a radio, so
`hub::hostapd_conf` is only parsed by the real `hostapd` (a deliberately broken file must be rejected). It needs passwordless `sudo ip`, and runs as a
bounded job: `agent_procs.py run --tag NAME hubns -- timeout 120 env HAMS_HUB_NETNS=1 cargo test --test hub_netns`.

## Build and test

```sh
cd daemons/relay_broker
CARGO_TARGET_DIR=... nice -n 10 cargo test
cargo clippy --all-targets --all-features -- -D warnings && cargo fmt --check && cargo deny check
# development run (no systemd):
hams-relay-broker --config ./broker.conf --config-owner $(id -u) --socket /tmp/broker.sock
cargo run --example broker_client -- /tmp/broker.sock list
cargo run --example broker_client -- /tmp/broker.sock open serial /dev/ttyUSB0
```

## Path to integration (phases 3 to 6 of the proposal; nothing done yet)

Relay call sites this broker replaces, from `RELAY_PRIVILEGE_INVENTORY.md` and
`privilege_inventory.json` (broker-class modules):

- `audio_open_policy::check`/`plan_open` (the single "may this device be opened" decision) moves
  into `policy.rs` as the audio class rules; the relay keeps the user-facing choice logic.
- `serial_permissions::fix_serial_permissions` and `privileged_action::run_pkexec` become
  `provision_groups` (headless, no polkit); `missing_groups` becomes a dry-run `provision_groups`.
- `discovery` (serial/USB enumeration over `/dev/tty*`, `/sys/bus/usb`), `hotplug` (serial
  scans; the uevent socket stays in the radio worker), `audio_probe` (`/proc/asound`,
  `/dev/snd`, the `sh` subprocess) and `audio_select` (`/dev/serial`, `pactl`) use
  `list_devices`; `firmware_guard` becomes a `deny_usb` table entry per printer id.
- Every serial and ALSA open in the radio and media workers becomes `open_device` and receives
  a descriptor. Hamlib, direwolf and the tty bridges take a path, not a descriptor
  (proposal section 5): they need `/proc/self/fd/N` or a pty shim, or stay in a worker that is
  allowed the device directly.
- `tool_install` and `updater` are not in this prototype: by 3.7 they become package-manager
  calls (`update-package` as a fixed action) once the package repository exists.

Remaining steps: (3) wire the relay's call sites above behind a feature flag with the
`client` module, and run the fuzz target in CI; (4) split the web front; (5) split radio, media
and decoders with the latency measurement gate; (6) per-worker systemd restrictions. Tracked in
`hams_com/night_shift_todo/high/`.

## Conventions

- Anchors (ADR 0090): a function carries `// [@ANCHOR: id]` in source, its test carries
  `// Tests [@ANCHOR: id]`, and this README cites it as `[@ANCHOR: relay_broker:id]` below.
  `tests/conventions.rs` fails when source, tests and README disagree or a file lacks its SPDX
  header.
- Tests use the production code paths; the only test seam is the `Os` trait.

## Function reference

- [@ANCHOR: relay_broker:relay_broker_time_guard_new] (time_guard.rs) A guard with the configured ceilings.
- [@ANCHOR: relay_broker:relay_broker_time_guard_check] (time_guard.rs) Decides whether the clock may be stepped: breaker, build date, large step without proof, steps per hour; a refusal on the step count trips the breaker.
- [@ANCHOR: relay_broker:relay_broker_time_guard_record_step] (time_guard.rs) Counts one completed step.
- [@ANCHOR: relay_broker:relay_broker_time_guard_status] (time_guard.rs) Steps in the last hour and whether the breaker is open.
- [@ANCHOR: relay_broker:relay_broker_broker_set_system_time] (broker.rs) Steps the clock when enabled and inside the ceilings; audits every outcome with before, target, source and evidence.
- [@ANCHOR: relay_broker:relay_broker_broker_time_status] (broker.rs) The clock and the step limits; read-only, always served.
- [@ANCHOR: relay_broker:relay_broker_os_system_time] (os.rs) The wall clock now in Unix seconds (the Linux layer also steps it with `clock_settime`).
- [@ANCHOR: relay_broker:relay_broker_json_escape] (audit.rs) Escapes a string for a JSON literal (quotes, backslash, control characters).
- [@ANCHOR: relay_broker:relay_broker_audit_event_to_json_line] (audit.rs) The record as one JSON line (with the trailing newline). `ts` is Unix seconds.
- [@ANCHOR: relay_broker:relay_broker_audit_log_open] (audit.rs) Opens (creating, mode 0600) the log at `path`; `None` disables logging to a file, which
- [@ANCHOR: relay_broker:relay_broker_audit_log_record] (audit.rs) Appends one event. A write failure is reported but never stops the broker.
- [@ANCHOR: relay_broker:relay_broker_allow_list_permits] (auth.rs) True when the peer's UID or primary GID is listed. An empty list permits no one: a
- [@ANCHOR: relay_broker:relay_broker_broker_new] (broker.rs) Assembles a broker from a config, an audit log and an OS layer.
- [@ANCHOR: relay_broker:relay_broker_broker_os] (broker.rs) The OS layer (the server uses it for peer credentials).
- [@ANCHOR: relay_broker:relay_broker_broker_log] (broker.rs) Writes one audit record; a failed write is ignored here (the log is best effort once
- [@ANCHOR: relay_broker:relay_broker_broker_handle] (broker.rs) Decides and performs one request for an already-authenticated peer. Returns the response
- [@ANCHOR: relay_broker:relay_broker_broker_open_device] (broker.rs) The open action: lexical check, node walk, open, post-open check. Any failure closes the
- [@ANCHOR: relay_broker:relay_broker_broker_grant] (broker.rs) The provisioning action: which of `groups` the caller lacks, applied only when the
- [@ANCHOR: relay_broker:relay_broker_client_call] (client.rs) Sends `req` on `stream` and waits for the answer. The descriptor, if any, is the opened
- [@ANCHOR: relay_broker:relay_broker_client_default_server_uids] (client.rs) `DEFAULT_SERVER_UIDS: &[u32] = &[0]`, the accounts a broker socket may be served
  by when the caller names none: root. A socket-activated broker has its listener made by systemd, so a client reads root's credentials even
  though the service runs as `hams-relay-broker`; a broker started directly by a service account must be named to `connect_and_call_as`.
- [@ANCHOR: relay_broker:relay_broker_client_verify_server] (client.rs) `verify_server(stream: &UnixStream, expected_uids: &[u32]) -> io::Result<()>`.
  Reads `SO_PEERCRED` of the connected socket and requires its uid to be in `expected_uids` (an empty list permits no one). Nothing has been
  sent when it runs. `Err(PermissionDenied)` names the uid found; any other error is the failure to read the credentials. This is the
  client authenticating the broker; the broker authenticating the client is `auth.rs`. Blocking, no side effect.
- [@ANCHOR: relay_broker:relay_broker_client_connect_and_call] (client.rs) `connect_and_call(path, req)`: `connect_and_call_as` with `DEFAULT_SERVER_UIDS` (root).
  Errors: connect failure, `PermissionDenied` from the verification, then those of `call`.
- [@ANCHOR: relay_broker:relay_broker_client_connect_and_call_as] (client.rs) `connect_and_call_as(path: &Path, req: &Request, expected_uids: &[u32])
  -> io::Result<(Response, Option<OwnedFd>)>`. Connects, runs `verify_server`, and only then sends `req` and returns the response and the passed
  descriptor, if any (`call`). A wrong server is refused with `PermissionDenied` and receives no byte, so an impostor on the socket path cannot
  answer `ListDevices`, `ProvisionGroups` or `OpenDevice` with a descriptor of its choice. Blocking; one connection per call.
- [@ANCHOR: relay_broker:relay_broker_parse_usb_pair] (config.rs) Parses `vvvv:pppp` (hex) into a vendor/product pair.
- [@ANCHOR: relay_broker:relay_broker_config_parse] (config.rs) Parses the file text. Comments start with `#`; blank lines are skipped.
- [@ANCHOR: relay_broker:relay_broker_config_load] (config.rs) Reads and parses `path`, first requiring it to be owned by `owner_uid` (0 in production)
- [@ANCHOR: relay_broker:relay_broker_main_parse_args] (cli.rs) Parses argv; unknown options are an error.
- [@ANCHOR: relay_broker:relay_broker_remove_stale_socket] (cli.rs) Removes a leftover socket at the `--socket` path from an earlier run; any other kind of file there is an error and is left alone. The listener is then bound and set to mode 0660.
- [@ANCHOR: relay_broker:relay_broker_main] (cli.rs) Entry point.
- [@ANCHOR: relay_broker:relay_broker_kind_of_mode] (os.rs) Maps `st_mode`/`st_rdev` to a `NodeKind`.
- [@ANCHOR: relay_broker:relay_broker_read_hex_u16] (os.rs) Reads a sysfs attribute holding a hexadecimal id (`idVendor`, `idProduct`).
- [@ANCHOR: relay_broker:relay_broker_usb_identity_from_sysfs] (os.rs) Walks up from a sysfs device directory (at most eight levels) to the USB device that holds
- [@ANCHOR: relay_broker:relay_broker_linux_sysfs_dir_for] (os.rs) The sysfs class directory of a device node: `/dev/ttyUSB0` is `class/tty/ttyUSB0`,
- [@ANCHOR: relay_broker:relay_broker_linux_scan_class] (os.rs) Lists one sysfs class directory. `keep` decides by entry name; `dev_dir` is where the
- [@ANCHOR: relay_broker:relay_broker_linux_peer_credentials] (os.rs) fn peer_credentials(&self, socket: BorrowedFd<'_>) -> io::Result<Peer> {
- [@ANCHOR: relay_broker:relay_broker_linux_update_package] (os.rs) fn update_package(&self) -> io::Result<()> { // creates the request file the package's root-owned path unit acts on; runs no program
- [@ANCHOR: relay_broker:relay_broker_linux_node_kind] (os.rs) fn node_kind(&self, path: &Path) -> NodeKind {
- [@ANCHOR: relay_broker:relay_broker_linux_open_device] (os.rs) fn open_device(&self, path: &Path, read_only: bool) -> io::Result<OwnedFd> {
- [@ANCHOR: relay_broker:relay_broker_linux_fd_kind] (os.rs) fn fd_kind(&self, fd: BorrowedFd<'_>) -> NodeKind {
- [@ANCHOR: relay_broker:relay_broker_linux_usb_identity] (os.rs) fn usb_identity(&self, path: &Path) -> Option<(u16, u16, String)> {
- [@ANCHOR: relay_broker:relay_broker_linux_list_devices] (os.rs) fn list_devices(&self) -> Vec<DeviceDescriptor> {
- [@ANCHOR: relay_broker:relay_broker_linux_username] (os.rs) fn username(&self, uid: u32) -> Option<String> {
- [@ANCHOR: relay_broker:relay_broker_linux_groups_of] (os.rs) fn groups_of(&self, user: &str) -> Vec<String> {
- [@ANCHOR: relay_broker:relay_broker_linux_add_to_group] (os.rs) fn add_to_group(&self, user: &str, group: &str) -> io::Result<()> {
- [@ANCHOR: relay_broker:relay_broker_denial_reason] (policy.rs) A stable short reason for the audit log and the response.
- [@ANCHOR: relay_broker:relay_broker_policy_check_path] (policy.rs) The lexical check. Returns the normalised path on success. No filesystem access.
- [@ANCHOR: relay_broker:relay_broker_policy_class_of_major] (policy.rs) The class a character-device major belongs to under this policy.
- [@ANCHOR: relay_broker:relay_broker_policy_check_kind] (policy.rs) Accepts a node kind only when it is a character device of `class`.
- [@ANCHOR: relay_broker:relay_broker_policy_check_node] (policy.rs) The filesystem check: every directory on the way from the device root is a real
- [@ANCHOR: relay_broker:relay_broker_policy_check_fd] (policy.rs) The post-open check on the descriptor itself.
- [@ANCHOR: relay_broker:relay_broker_rate_limiter_new] (ratelimit.rs) `per_minute` requests sustained, with a burst of the same size. Zero means one a minute.
- [@ANCHOR: relay_broker:relay_broker_rate_limiter_allow] (ratelimit.rs) Takes one token for `uid` at time `now`; false when the bucket is empty. At most 256
- [@ANCHOR: relay_broker:relay_broker_serve_connection] (server.rs) Serves one connection to completion. Never panics; every failure is an `Outcome`.
- [@ANCHOR: relay_broker:relay_broker_run] (server.rs) Accepts connections until `idle` passes with none, or until `max_connections` have been
- [@ANCHOR: relay_broker:relay_broker_activated_listener] (server.rs) The listener systemd passed (`LISTEN_PID` is this process and `LISTEN_FDS` is 1), if any.
- [@ANCHOR: relay_broker:relay_broker_set_timeouts] (transport.rs) Applies `IO_TIMEOUT` as the receive and send timeout of a socket.
- [@ANCHOR: relay_broker:relay_broker_send_frame] (transport.rs) Writes `body` as one frame, passing `fd` (if any) with the first bytes.
- [@ANCHOR: relay_broker:relay_broker_recv_exact] (transport.rs) Fills `buf` with `recvmsg`, appending any passed descriptors to `fds`. Descriptors beyond
- [@ANCHOR: relay_broker:relay_broker_adopt_received_fd] (transport.rs) Wraps a descriptor that `recvmsg` just received as SCM_RIGHTS. This is the crate's only
- [@ANCHOR: relay_broker:relay_broker_recv_frame] (transport.rs) Reads one frame: the body bytes and any descriptors passed with it. The announced length is
- [@ANCHOR: relay_broker:relay_broker_group_name] (wire.rs) The system group name.
- [@ANCHOR: relay_broker:relay_broker_reader_take] (wire.rs) The next `n` bytes, or `Truncated`.
- [@ANCHOR: relay_broker:relay_broker_reader_u8] (wire.rs) One byte.
- [@ANCHOR: relay_broker:relay_broker_reader_u16] (wire.rs) A big-endian u16.
- [@ANCHOR: relay_broker:relay_broker_reader_string] (wire.rs) A length-prefixed string of at most `MAX_STRING` bytes, UTF-8, no NUL.
- [@ANCHOR: relay_broker:relay_broker_reader_finish] (wire.rs) Succeeds only when every byte was consumed.
- [@ANCHOR: relay_broker:relay_broker_put_string] (wire.rs) Appends a length-prefixed string, truncating at a character boundary to `MAX_STRING` bytes
- [@ANCHOR: relay_broker:relay_broker_device_class_from_tag] (wire.rs) The class for a wire tag.
- [@ANCHOR: relay_broker:relay_broker_request_encode] (wire.rs) The body bytes (no frame header) for this request.
- [@ANCHOR: relay_broker:relay_broker_request_decode] (wire.rs) Parses a body. Every failure is a `WireError`; no input panics.
- [@ANCHOR: relay_broker:relay_broker_response_error] (wire.rs) Shorthand for an error response.
- [@ANCHOR: relay_broker:relay_broker_response_encode] (wire.rs) The body bytes (no frame header). Device lists are cut at `MAX_DEVICES`.
- [@ANCHOR: relay_broker:relay_broker_response_decode] (wire.rs) Parses a response body. Used by the client and by the fuzz/property tests.
- [@ANCHOR: relay_broker:relay_broker_frame_encode] (wire.rs) Prefixes a body with its length. Bodies above `MAX_FRAME` are an encoder bug and are cut to
- [@ANCHOR: relay_broker:relay_broker_frame_body_len] (wire.rs) The body length announced by a 4-byte header, if it is within bounds.
