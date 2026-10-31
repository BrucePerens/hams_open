# hams-relay-broker (prototype)

The relay's privileged broker from `hams_com/docs/proposals/RELAY_ZERO_PRIVILEGE_SPLIT.md`
(sections 3.1, 3.5, 3.6, 5, 6), phase 3 of that plan, built as a standalone crate. **Nothing here
is wired into the relay or installed by any package**; the integration path is at the end.

A small separate process (about 2,000 lines of source without tests; no web, no async runtime,
no FFI, no shell, two commented `unsafe` blocks that adopt kernel-provided descriptors) that
answers a fixed set of typed requests from authenticated local peers over a Unix domain socket:

| Action | What it does | Privilege it needs |
|---|---|---|
| `list_devices` | serial ttys and ALSA nodes from sysfs (read only), USB ids and product strings; protected devices are left out | none |
| `open_device` | validates one path against the policy, opens it `O_NOFOLLOW|O_CLOEXEC|O_NOCTTY`, re-checks the open descriptor, passes it with SCM_RIGHTS; the worker never needs `/dev` access and never learns another device's path | group membership (`dialout`, `audio`) or, failing that, one capability (see the unit) |
| `grant_group` / `provision_groups` | reports which of `dialout`/`audio` the **caller's own** account lacks; changes nothing unless `apply_groups = true`, then runs `/usr/sbin/gpasswd -a USER GROUP` with a fixed argv | root or a gpasswd setuid, only with apply |

Every request is rate-limited per UID and written to an append-only `events.jsonl` audit log.

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
denial of service by a legitimate peer against itself (rate limit is per UID); TOCTOU on sysfs
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
- [@ANCHOR: relay_broker:relay_broker_client_connect_and_call] (client.rs) Connects to the broker socket at `path` and performs one call.
- [@ANCHOR: relay_broker:relay_broker_parse_usb_pair] (config.rs) Parses `vvvv:pppp` (hex) into a vendor/product pair.
- [@ANCHOR: relay_broker:relay_broker_config_parse] (config.rs) Parses the file text. Comments start with `#`; blank lines are skipped.
- [@ANCHOR: relay_broker:relay_broker_config_load] (config.rs) Reads and parses `path`, first requiring it to be owned by `owner_uid` (0 in production)
- [@ANCHOR: relay_broker:relay_broker_main_parse_args] (main.rs) Parses argv; unknown options are an error.
- [@ANCHOR: relay_broker:relay_broker_main] (main.rs) Entry point.
- [@ANCHOR: relay_broker:relay_broker_kind_of_mode] (os.rs) Maps `st_mode`/`st_rdev` to a `NodeKind`.
- [@ANCHOR: relay_broker:relay_broker_read_hex_u16] (os.rs) Reads a sysfs attribute holding a hexadecimal id (`idVendor`, `idProduct`).
- [@ANCHOR: relay_broker:relay_broker_usb_identity_from_sysfs] (os.rs) Walks up from a sysfs device directory (at most eight levels) to the USB device that holds
- [@ANCHOR: relay_broker:relay_broker_linux_sysfs_dir_for] (os.rs) The sysfs class directory of a device node: `/dev/ttyUSB0` is `class/tty/ttyUSB0`,
- [@ANCHOR: relay_broker:relay_broker_linux_scan_class] (os.rs) Lists one sysfs class directory. `keep` decides by entry name; `dev_dir` is where the
- [@ANCHOR: relay_broker:relay_broker_linux_peer_credentials] (os.rs) fn peer_credentials(&self, socket: BorrowedFd<'_>) -> io::Result<Peer> {
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
