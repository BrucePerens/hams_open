# Data structures

### `Net` [@ANCHOR: radioctl:radioctl_d_access_net]

**Purpose and location.** One network: an address and a prefix length. Defined in `daemons/radioctl/crates/radioctld/src/access.rs:9` as a private `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `addr` | `IpAddr` | The network address (IPv4 or IPv6). |
| `prefix` | `u8` | The prefix length in bits; a client address matches when its first `prefix` bits equal those of `addr`. |

**Invariants, ownership and concurrency.** A plain `Copy` value inside an `AllowList`; private to the access module.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `networks_match_by_prefix_and_an_empty_list_allows_nobody (src/access.rs)`.

### `AllowList` [@ANCHOR: radioctl:radioctl_d_access_allow_list]

**Purpose and location.** The set of networks allowed to connect. Defined in `daemons/radioctl/crates/radioctld/src/access.rs:16` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `nets` | `Vec<Net>` | The allowed networks; an empty list allows no one. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `loopback_only` (src/access.rs:59), `parse` (src/access.rs:76), `list` (src/access.rs:123).

**See also:** test `the_default_allows_only_this_machine (src/access.rs)`.

### `RigSource` [@ANCHOR: radioctl:radioctl_d_builtin_rig_source]

**Purpose and location.** Where descriptions are read from. Defined in `daemons/radioctl/crates/radioctld/src/builtin.rs:16` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Embedded` | `Embedded` | The set built into this program. |
| `Dir` | `Dir(PathBuf)` | `*.toml` files in a directory. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `choose_source` (src/builtin.rs:35), `load_texts` (src/builtin.rs:45).

**See also:** test `the_flag_beats_the_environment_beats_the_embedded_set (src/builtin.rs)`.

### `RigSummary` [@ANCHOR: radioctl:radioctl_d_candidates_rig_summary]

**Purpose and location.** What a listing needs to know about one rig description file. Defined in `daemons/radioctl/crates/radioctld/src/candidates.rs:18` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | The file stem and description id, such as `icom-ic-7300`. |
| `name` | `String` | The display name, such as `Icom IC-7300`. |
| `family` | `String` | The protocol family name. |
| `highest` | `&'static str` | The highest verified state among the facts. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `load_summaries` (src/candidates.rs:55), `summaries_from` (src/candidates.rs:74), `candidates_for_product` (src/candidates.rs:127), `render_listing` (src/candidates.rs:144), `rigs` (src/candidates.rs:201), `net_from_packet` (src/discovery.rs:219), `listen_flex` (src/discovery.rs:237), `rigs` (src/discovery.rs:324).

**See also:** test `summaries_come_from_text_and_a_bad_one_names_its_label (src/candidates.rs)`.

### `CapsView` [@ANCHOR: radioctl:radioctl_d_caps_caps_view]

**Purpose and location.** Everything the text needs besides the description. Defined in `daemons/radioctl/crates/radioctld/src/caps.rs:22` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `description` | `&'a RigDescription` | The rig. |
| `hamlib_model` | `u32` | The model number to report. |
| `rx_low_hz` | `u64` | Lowest frequency covered, in hertz. |
| `rx_high_hz` | `u64` | Highest frequency covered, in hertz. |
| `tx_power_mw` | `u32` | Maximum transmit power, in milliwatts. |
| `transmit_enabled` | `bool` | True when the rig's transmit gate is open. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `dump_state` (src/caps.rs:178), `dump_caps` (src/caps.rs:271), `view` (src/caps.rs:333), `caps_view` (src/service.rs:145).

**See also:** test `dump_caps_summarises_the_rig_and_the_gate (src/caps.rs)`.

### `Command` [@ANCHOR: radioctl:radioctl_d_command_command]

**Purpose and location.** A parsed command, independent of the VFO token and of the response format. Defined in `daemons/radioctl/crates/radioctld/src/command.rs:27` as a pub `enum`. It derives Debug, Clone, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `GetFreq` | `GetFreq` | `f` |
| `GetActiveTxFreq` | `GetActiveTxFreq` | `\get_active_tx_freq`: the frequency the rig transmits on now (or will when keyed), whatever VFO is current. `f` keeps Hamlib's meaning, the frequency of the VFO asked for. |
| `SetFreq` | `SetFreq(u64)` | `F freq` |
| `GetMode` | `GetMode` | `m` |
| `SetMode` | `SetMode { mode: Mode, passband: i64, }` | `M mode passband` The mode. Passband in hertz; 0 and -1 leave the rig's own width. |
| `GetVfo` | `GetVfo` | `v` |
| `SetVfo` | `SetVfo(VfoSel)` | `V vfo` |
| `GetPtt` | `GetPtt` | `t` |
| `SetPtt` | `SetPtt(u8)` | `T 0|1|2|3` |
| `GetSplitVfo` | `GetSplitVfo` | `s` |
| `SetSplitVfo` | `SetSplitVfo { on: bool, tx: VfoSel, }` | `S split txvfo` Split on or off. The transmit VFO. |
| `GetSplitFreq` | `GetSplitFreq` | `i` |
| `SetSplitFreq` | `SetSplitFreq(u64)` | `I freq` |
| `GetSplitMode` | `GetSplitMode` | `x` |
| `SetSplitMode` | `SetSplitMode { mode: Mode, passband: i64, }` | `X mode passband` The mode. Passband in hertz. |
| `GetSplitFreqMode` | `GetSplitFreqMode` | `\get_split_freq_mode` |
| `SetSplitFreqMode` | `SetSplitFreqMode { freq: u64, mode: Mode, passband: i64, }` | `\set_split_freq_mode freq mode passband` Transmit frequency in hertz. The mode. Passband in hertz. |
| `GetRit` | `GetRit` | `j` |
| `SetRit` | `SetRit(i64)` | `J offset` |
| `GetXit` | `GetXit` | `z` |
| `SetXit` | `SetXit(i64)` | `Z offset` |
| `GetLevel` | `GetLevel(String)` | `l level` |
| `SetLevel` | `SetLevel(String, f64)` | `L level value` |
| `GetFunc` | `GetFunc(String)` | `u func` |
| `SetFunc` | `SetFunc(String, bool)` | `U func 0|1` |
| `DumpCaps` | `DumpCaps` | `1` or `\dump_caps` |
| `DumpState` | `DumpState` | `\dump_state` |
| `ChkVfo` | `ChkVfo` | `\chk_vfo` |
| `GetPowerStat` | `GetPowerStat` | `\get_powerstat` |
| `SetPowerStat` | `SetPowerStat(i64)` | `\set_powerstat n` |
| `GetLockMode` | `GetLockMode` | `\get_lock_mode` |
| `SetLockMode` | `SetLockMode(bool)` | `\set_lock_mode 0|1` |
| `GetInfo` | `GetInfo` | `\get_info` |
| `SendMorse` | `SendMorse(String)` | `b text` / `\send_morse text` |
| `StopMorse` | `StopMorse` | `\stop_morse` |
| `GetVfoInfo` | `GetVfoInfo` | `\get_vfo_info` |
| `GetVfoList` | `GetVfoList` | `\get_vfo_list` |
| `SendCmd` | `SendCmd(String)` | `w text` / `\send_cmd text`: the raw escape hatch, never served. |
| `Quit` | `Quit` | `q` / `Q` |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build` (src/command.rs:450).

**See also:** test `the_common_commands_parse_with_and_without_a_vfo_token (src/command.rs)`.

### `Parsed` [@ANCHOR: radioctl:radioctl_d_command_parsed]

**Purpose and location.** What a line turned into. Defined in `daemons/radioctl/crates/radioctld/src/command.rs:133` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `extended` | `bool` | The client asked for the extended (`+`) response format. |
| `name` | `&'static str` | The long name of the command, for the response header; empty when it was not recognised. |
| `vfo` | `Option<(String, VfoSel)>` | The VFO token the client sent, as sent, with what it names. |
| `args` | `String` | The remaining arguments, blank-normalised, for the response header. |
| `command` | `Result<Command, i32>` | The command, or the `RPRT` code to answer with. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_line` (src/command.rs:545).

**See also:** test `the_extended_prefix_the_echo_and_blank_lines (src/command.rs)`.

### `Line` [@ANCHOR: radioctl:radioctl_d_command_line]

**Purpose and location.** A line from a client. Defined in `daemons/radioctl/crates/radioctld/src/command.rs:148` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Text` | `Text(String)` | A text line without its newline (and without a trailing carriage return). |
| `Invalid` | `Invalid` | A line that is not text: invalid UTF-8 or a control character. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `push` (src/command.rs:185), `into_line` (src/command.rs:207), `text` (src/command.rs:612), `rot_answer` (src/rotctl.rs:272), `answer` (src/rotctl.rs:297), `answer` (src/server.rs:44), `answer` (src/server.rs:52), `answer` (src/server.rs:229).

**See also:** test `lines_are_assembled_across_reads_and_crlf_is_trimmed (src/command.rs)`.

### `LineTooLong` [@ANCHOR: radioctl:radioctl_d_command_line_too_long]

**Purpose and location.** A line grew past the limit before its newline arrived. Defined in `daemons/radioctl/crates/radioctld/src/command.rs:157` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

**Fields.** none (unit or opaque definition).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `push` (src/command.rs:185).

**See also:** test `a_line_over_the_limit_is_an_error_and_is_never_buffered (src/command.rs)`.

### `LineBuffer` [@ANCHOR: radioctl:radioctl_d_command_line_buffer]

**Purpose and location.** Assembles lines from a byte stream with a hard bound on the length of a line. Defined in `daemons/radioctl/crates/radioctld/src/command.rs:161` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `pending` | `Vec<u8>` | The bytes of the line being assembled, no newline yet. |
| `max` | `usize` | The longest line allowed in bytes; past it the buffer reports `LineTooLong` and the client is dropped. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/command.rs:169).

**See also:** test `lines_are_assembled_across_reads_and_crlf_is_trimmed (src/command.rs)`.

### `FileDoc` [@ANCHOR: radioctl:radioctl_d_config_file_doc]

**Purpose and location.** The whole configuration file as the TOML parser reads it: the optional `[server]` table, and the `[[rig]]` and `[[rotator]]` arrays. Unknown keys are an error (`deny_unknown_fields`), so a mistyped setting is not silently ignored. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:67` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `server` | `Option<ServerSection>` | The `[server]` table, if present. |
| `rig` | `Vec<RigSection>` | The `[[rig]]` entries (empty when none). |
| `rotator` | `Vec<RotatorSection>` | The `[[rotator]]` entries (empty when none). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `unknown_keys_bad_values_and_duplicates_are_errors (src/config.rs)`.

### `RotatorSection` [@ANCHOR: radioctl:radioctl_d_config_rotator_section]

**Purpose and location.** One `[[rotator]]` table of the configuration file, before validation. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:77` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The rotator's name for logs. |
| `model` | `String` | The shipped rotator description to use. |
| `listen` | `Option<String>` | The `host:port` clients connect to; default loopback, port 4533 plus the rotator's index. |
| `transport` | `RotatorTransportSection` | The `[rotator.transport]` table. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `rotators_are_validated_and_share_the_address_space_with_rigs (src/config.rs)`.

### `RotatorTransportSection` [@ANCHOR: radioctl:radioctl_d_config_rotator_transport_section]

**Purpose and location.** The `[rotator.transport]` table, selected by its `kind` key (`virtual`, `tcp` or `serial`). Defined in `daemons/radioctl/crates/radioctld/src/config.rs:86` as a private `enum`. It derives Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Virtual` | `Virtual` | A virtual rotator served inside the daemon. |
| `Tcp` | `Tcp { address: String, }` | A TCP connection; `address` is `host:port`. |
| `Serial` | `Serial { path: Option<String>, usb: Option<UsbSection>, baud: u32, lines: Option<String>, }` | A serial port, given by `path` or by `usb` ids, at `baud`, with optional `lines`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `rotators_are_validated_and_share_the_address_space_with_rigs (src/config.rs)`.

### `ServerSection` [@ANCHOR: radioctl:radioctl_d_config_server_section]

**Purpose and location.** The `[server]` table of the configuration file, before validation; every key is optional and has a default in `ServerConfig`. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:101` as a private `struct`. It derives Deserialize, Default.

| Field | Type | Meaning |
|---|---|---|
| `allow` | `Option<Vec<String>>` | Allowed networks as `address/prefix` strings. |
| `allow_non_loopback_bind` | `bool` | Permit a `listen` address that is not loopback (default false). |
| `max_clients` | `Option<usize>` | Most simultaneous clients per rig. |
| `idle_timeout_secs` | `Option<u64>` | Idle time in seconds after which a connection is closed. |
| `line_timeout_secs` | `Option<u64>` | Time in seconds a begun line may take to end. |
| `max_line_bytes` | `Option<usize>` | Longest request line in bytes. |
| `request_timeout_ms` | `Option<u64>` | Deadline in milliseconds for a request that goes to the rig. |
| `cache_max_age_ms` | `Option<u64>` | Age in milliseconds up to which reads are answered from the cache. |
| `secrets_dir` | `Option<String>` | Directory of secret files, relative to the configuration file. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `server_config` (src/config.rs:407).

**See also:** test `a_minimal_file_gets_safe_defaults (src/config.rs)`.

### `RigSection` [@ANCHOR: radioctl:radioctl_d_config_rig_section]

**Purpose and location.** One `[[rig]]` table of the configuration file, before validation. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:116` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The rig's name for logs. |
| `description` | `String` | The rig description file (TOML) to use. |
| `listen` | `Option<String>` | The `host:port` clients connect to; default loopback, port 4532 plus the rig's index. A non-loopback address needs `allow_non_loopback_bind` (and an allow list). |
| `vfo_mode` | `bool` | Answer `\chk_vfo` with 1 and echo the VFO in responses (default false). |
| `transmit` | `bool` | Open the rig's transmit gate at start (default false). |
| `hamlib_model` | `Option<u32>` | The model number reported in `\dump_state`. |
| `rx_low_hz` | `Option<u64>` | Lowest frequency the rig is said to cover. |
| `rx_high_hz` | `Option<u64>` | Highest frequency the rig is said to cover. |
| `tx_power_mw` | `Option<u32>` | Maximum transmit power reported, in milliwatts. |
| `max_transmit_s` | `Option<u64>` | The transmit time limit in seconds (1 to 3600). |
| `transport` | `TransportSection` | The `[rig.transport]` table. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_minimal_file_gets_safe_defaults (src/config.rs)`.

### `TransportSection` [@ANCHOR: radioctl:radioctl_d_config_transport_section]

**Purpose and location.** The `[rig.transport]` table, selected by its `kind` key (`virtual`, `serial`, `tcp`, `usb` or `radiod`). Each variant lists the keys that kind accepts; `transport_config` validates them into a `TransportConfig`. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:134` as a private `enum`. It derives Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Virtual` | `Virtual { reply_delay_ms: Option<u64>, }` | An in-memory virtual rig; optional `reply_delay_ms`. |
| `Serial` | `Serial { path: Option<String>, usb: Option<UsbSection>, baud: Option<BaudSection>, baud_probe: Option<Vec<u32>>, lines: Option<String>, }` | A serial port: `path` or `usb` ids, `baud` (a rate or `"auto"`), optional `baud_probe` list and `lines`. |
| `Tcp` | `Tcp { address: String, framing: Option<String>, lan_account: Option<String>, lan_administrator: Option<bool>, lan_password_secret: Option<String>, idle_timeout_ms: Option<u64>, }` | A TCP link: `address`, optional `framing`, `lan_account`, `lan_administrator`, `lan_password_secret` (the name of a secret, never the password) and `idle_timeout_ms`. |
| `Usb` | `Usb { vid: Option<u16>, pid: Option<u16>, serial: Option<String>, transfer_timeout_ms: Option<u64>, firmware_dir: Option<String>, }` | A USB board: optional `vid`, `pid`, `serial`, `transfer_timeout_ms` and `firmware_dir`. |
| `Radiod` | `Radiod { status: String, status_port: Option<u16>, data: String, data_port: Option<u16>, iface: Option<String>, ssrc: u32, lifetime_frames: Option<u32>, refresh_ms: Option<u64>, min_hz: Option<u64>, max_hz: Option<u64>, }` | A ka9q-radio channel: `status` and `data` multicast groups, `ssrc`, and optional ports, `iface`, `lifetime_frames`, `refresh_ms`, `min_hz` and `max_hz`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `transports_are_validated (src/config.rs)`.

### `BaudSection` [@ANCHOR: radioctl:radioctl_d_config_baud_section]

**Purpose and location.** The `baud` key of a serial transport: a number, or the word `auto`. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:177` as a private `enum`. It derives Deserialize.

| Variant | Definition | Meaning |
|---|---|---|
| `Rate` | `Rate(u32)` | A fixed baud rate such as `19200`. |
| `Word` | `Word(String)` | A word; only `auto` is accepted by validation. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `transports_are_validated (src/config.rs)`.

### `UsbSection` [@ANCHOR: radioctl:radioctl_d_config_usb_section]

**Purpose and location.** A USB identification of a serial port: `usb = { vid, pid, serial }`. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:184` as a private `struct`. It derives Deserialize.

| Field | Type | Meaning |
|---|---|---|
| `vid` | `u16` | The USB vendor id. |
| `pid` | `u16` | The USB product id. |
| `serial` | `Option<String>` | The serial number string, to tell two identical devices apart. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** It derives a serde trait, so it has a serialized form defined by its fields.

**Functions that touch it.** `serial_target` (src/config.rs:766).

**See also:** test `a_usb_transport_takes_optional_ids_a_serial_and_a_bounded_timeout (src/config.rs)`.

### `ServerConfig` [@ANCHOR: radioctl:radioctl_d_config_server_config]

**Purpose and location.** Server-wide settings. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:192` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `allow` | `AllowList` | Who may connect. |
| `max_clients` | `usize` | Most simultaneous clients per rig. |
| `idle_timeout` | `Duration` | A connection with nothing to say for this long is closed. |
| `line_timeout` | `Duration` | A line that has begun must end within this. |
| `max_line_bytes` | `usize` | Longest request line, in bytes. |
| `request_timeout` | `Duration` | Deadline for a request that has to go to the rig. |
| `cache_max_age` | `Duration` | Reads at most this old are answered from the rig's cache. |
| `secrets_dir` | `Option<PathBuf>` | The directory of secret files (one per secret, readable by the owner only), when a transport needs one. Relative to the configuration file. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `server_config` (src/config.rs:407), `service_config` (src/rigs.rs:156), `from_config` (src/server.rs:79).

**See also:** test `a_minimal_file_gets_safe_defaults (src/config.rs)`.

### `TcpFraming` [@ANCHOR: radioctl:radioctl_d_config_tcp_framing]

**Purpose and location.** What a TCP transport wraps the CAT stream in. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:214` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Raw` | `Raw` | Nothing: the CAT stream is the wire stream. |
| `Ts890Lan` | `Ts890Lan { account: String, administrator: bool, password_secret: String, }` | The Kenwood TS-890S LAN session: `##CN;` connection request, `##ID` login, `ID;` keep-alive. The password is read from the secret store at start, never from this file. The LAN account name (not a secret). Whether the account is the administrator account. The name of the secret (a file in `server.secrets_dir`) holding the password. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_ts890_lan_framing_needs_an_account_a_secret_name_and_a_secret_store (src/config.rs)`.

### `BaudSetting` [@ANCHOR: radioctl:radioctl_d_config_baud_setting]

**Purpose and location.** A serial rig's baud rate: the default is a fixed rate and nothing probes unless the file says `baud = "auto"` or gives a `baud_probe` list. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:232` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Fixed` | `Fixed(u32)` | Open the port at this rate. |
| `Probe` | `Probe(Vec<u32>)` | Find the rate with the read-only baud probe (`radioctl_core::baud_probe`), trying these rates in this order; the port is then opened at the first one that answered. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `transports_are_validated (src/config.rs)`.

### `TransportConfig` [@ANCHOR: radioctl:radioctl_d_config_transport_config]

**Purpose and location.** How a rig is reached. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:242` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Virtual` | `Virtual { reply_delay: Duration, }` | An in-memory virtual rig generated from the description. How long the virtual rig takes to answer. |
| `Serial` | `Serial { target: PortTarget, baud: BaudSetting, lines: LineControl, }` | A serial or USB-serial port. The port, by path or by USB ids. The baud rate, fixed or probed. Whether the port has modem control lines this daemon may use. |
| `Tcp` | `Tcp { address: String, framing: TcpFraming, idle_timeout: Option<Duration>, }` | A TCP connection (a network serial server or a radio with its own CAT port), reconnected with backoff after a loss. `host:port`. The session around the CAT stream. Silence on the wire after which the link is declared lost and reconnected. |
| `Usb` | `Usb { vid: Option<u16>, pid: Option<u16>, serial: Option<String>, transfer_timeout: Duration, firmware_dir: Option<PathBuf>, }` | A USB SDR board or dongle driven by vendor control transfers or HID reports (`radioctl_core::usb_link`), reopened with backoff after an unplug. The description names the vendor and product ids and strings; these keys narrow or override them. Vendor id override (default: the description's). Product id override (default: the description's). The serial number string, to tell two identical boards apart. Longest wait for one USB transfer. Where a board that needs the user's own firmware (the Microtelecom Perseus) finds it; default `RADIOCTL_PERSEUS_FIRMWARE_DIR`, else `~/.config/radioctl/firmware/perseus`. |
| `Radiod` | `Radiod(RadiodTransport)` | One receive channel of a ka9q-radio `radiod`, reached on its multicast groups (`radioctl_protocols::radiod_io`); only for a `radiod-channel` description. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_usb_transport_takes_optional_ids_a_serial_and_a_bounded_timeout (src/config.rs)`.

### `RigConfig` [@ANCHOR: radioctl:radioctl_d_config_rig_config]

**Purpose and location.** One rig to serve. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:317` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The name used in logs. |
| `description` | `PathBuf` | The rig description file. |
| `listen` | `SocketAddr` | Where clients connect. |
| `vfo_mode` | `bool` | `\chk_vfo` answers 1 and responses echo the VFO. |
| `transmit` | `bool` | Open the rig's transmit gate at start. Off unless the file says so. |
| `hamlib_model` | `u32` | The model number reported in `\dump_state`. |
| `rx_low_hz` | `u64` | Lowest frequency the rig is said to cover, in hertz. |
| `rx_high_hz` | `u64` | Highest frequency the rig is said to cover, in hertz. |
| `tx_power_mw` | `u32` | Maximum transmit power reported, in milliwatts. |
| `max_transmit` | `Option<Duration>` | The actor's transmit time limit, when the file sets one (1 to 3600 seconds). |
| `transport` | `TransportConfig` | How to reach the rig. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `service_config` (src/rigs.rs:156).

**See also:** test `a_minimal_file_gets_safe_defaults (src/config.rs)`.

### `RotatorTransportConfig` [@ANCHOR: radioctl:radioctl_d_config_rotator_transport_config]

**Purpose and location.** How a rotator is reached. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:344` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Virtual` | `Virtual` | A virtual rotator of the model, served inside the daemon (for tests and demonstrations). |
| `Tcp` | `Tcp { address: String, }` | A TCP connection (a Hamlib `rotctld`, or a serial server in front of a GS-232 controller), reconnected when it is lost. `host:port`. |
| `Serial` | `Serial { target: PortTarget, baud: u32, lines: LineControl, }` | A serial or USB-serial port (a GS-232 controller or a rotator interface), 8N1 with no flow control, reopened by the link when it comes back after an unplug. The port, by path or by USB ids. The baud rate (a fixed one: rotator controllers have no probe). Whether the port has modem control lines this daemon may use. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `rotators_are_validated_and_share_the_address_space_with_rigs (src/config.rs)`.

### `RotatorConfig` [@ANCHOR: radioctl:radioctl_d_config_rotator_config]

**Purpose and location.** One rotator to serve. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:367` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The name used in logs. |
| `description` | `RotatorDescription` | The description, from the shipped rotator descriptions. |
| `listen` | `SocketAddr` | Where clients connect. |
| `transport` | `RotatorTransportConfig` | How to reach the rotator. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `rotators_are_validated_and_share_the_address_space_with_rigs (src/config.rs)`.

### `Config` [@ANCHOR: radioctl:radioctl_d_config_config]

**Purpose and location.** A validated configuration. Defined in `daemons/radioctl/crates/radioctld/src/config.rs:380` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `server` | `ServerConfig` | Server-wide settings. |
| `rigs` | `Vec<RigConfig>` | The rigs, in file order. |
| `rotators` | `Vec<RotatorConfig>` | The rotators, in file order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse` (src/config.rs:919), `parse` (src/config.rs:985), `load` (src/main.rs:63), `config_for` (tests/serial_pty.rs:64).

**See also:** test `a_usb_transport_takes_optional_ids_a_serial_and_a_bounded_timeout (src/config.rs)`.

### `ServedRotator` [@ANCHOR: radioctl:radioctl_d_daemon_served_rotator]

**Purpose and location.** One rotator being served. Defined in `daemons/radioctl/crates/radioctld/src/daemon.rs:23` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The rotator's name from the configuration. |
| `service` | `Arc<RotatorService>` | The rotator's service. |
| `server` | `ServerHandle` | The listener. |
| `virtual_control` | `Option<VirtualRotatorControl>` | Control of the virtual rotator behind the service, for virtual rotators. |
| `net_link` | `Option<NetMonitor>` | The network link's status and counters, for TCP (and virtual) rotators. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_rig_and_a_rotator_are_served_from_one_file (tests/rotator.rs)`.

### `Served` [@ANCHOR: radioctl:radioctl_d_daemon_served]

**Purpose and location.** One rig being served. Defined in `daemons/radioctl/crates/radioctld/src/daemon.rs:37` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The rig's name from the configuration. |
| `service` | `Arc<RigService>` | The rig's service. |
| `server` | `ServerHandle` | The listener. |
| `virtual_control` | `Option<VirtualRigControl>` | Control of the virtual rig behind the service, for virtual rigs. |
| `serial_link` | `Option<SerialMonitor>` | The serial port's control lines and status, for serial rigs; held so the port's supervisor and the actor's link watch live as long as the rig is served. |
| `net_link` | `Option<NetMonitor>` | The network link's status and counters, for TCP rigs. |
| `usb_link` | `Option<radioctl_core::usb_link::UsbMonitor>` | The USB link's status and counters, for USB rigs. |
| `baud_probe` | `Option<ProbeResult>` | What the baud probe found, for a serial rig configured with `baud = "auto"` or `baud_probe`; `None` otherwise. |
| `radiod` | `Option<radioctl_protocols::radiod_io::RadiodControl>` | The `radiod` channel's audio queue, counters and front-end status, for a rig on a `radiod` transport (`RadiodControl::take_audio`): a program that embeds the daemon takes the channel's audio from here. `None` for every other transport. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_rig_and_a_rotator_are_served_from_one_file (tests/rotator.rs)`.

### `Daemon` [@ANCHOR: radioctl:radioctl_d_daemon_daemon]

**Purpose and location.** The running daemon. Defined in `daemons/radioctl/crates/radioctld/src/daemon.rs:63` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rigs` | `Vec<Served>` | Every rig being served, in configuration order. |
| `rotators` | `Vec<ServedRotator>` | Every rotator being served, in configuration order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `bytes` (tests/arbitration.rs:16), `ptt` (tests/arbitration.rs:24), `rigctl` (tests/rigctl_oracle.rs:37), `control` (tests/rigctl_oracle.rs:85), `rotator_client` (tests/rotator.rs:28), `start` (tests/ts890_lan.rs:200), `start` (tests/common/mod.rs:41), `start_with` (tests/common/mod.rs:67).

**See also:** test `stopping_the_daemon_stops_the_virtual_rotators_listener (tests/rotator.rs)`.

### `Outcome` [@ANCHOR: radioctl:radioctl_d_discovery_outcome]

**Purpose and location.** What discovery concluded about one port. Defined in `daemons/radioctl/crates/radioctld/src/discovery.rs:50` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `NotRadio` | `NotRadio(String)` | Clearly not a radio; never opened. Holds the reason. |
| `Unidentified` | `Unidentified` | No description is named by the device; never opened. |
| `Identified` | `Identified { id: String, how: String, }` | One description, and how it was decided. The description id. `product string` or `answered ID n`. |
| `Ambiguous` | `Ambiguous(Vec<String>)` | Several descriptions fit and the radio's answers did not settle it; skipped. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `ports_are_skipped_named_or_left_ambiguous_without_writing_to_non_radios (src/discovery.rs)`.

### `Finding` [@ANCHOR: radioctl:radioctl_d_discovery_finding]

**Purpose and location.** One serial port and what was concluded. Defined in `daemons/radioctl/crates/radioctld/src/discovery.rs:68` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `port` | `PortInfo` | The port. |
| `outcome` | `Outcome` | The conclusion. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `render_table` (src/discovery.rs:259), `identified` (src/discovery.rs:295).

**See also:** test `ports_are_skipped_named_or_left_ambiguous_without_writing_to_non_radios (src/discovery.rs)`.

### `NetFinding` [@ANCHOR: radioctl:radioctl_d_discovery_net_finding]

**Purpose and location.** A radio found on the network. Defined in `daemons/radioctl/crates/radioctld/src/discovery.rs:77` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `address` | `String` | Address and TCP port. |
| `model` | `String` | The model the radio announced. |
| `description` | `Option<String>` | The description id it matches, if exactly one. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `net_from_packet` (src/discovery.rs:219), `listen_flex` (src/discovery.rs:237), `render_table` (src/discovery.rs:259).

**See also:** test `the_table_lists_network_radios_and_says_when_nothing_was_found (src/discovery.rs)`.

### `KnobOutcome` [@ANCHOR: radioctl:radioctl_d_knob_knob_outcome]

**Purpose and location.** What watching the knob found. Defined in `daemons/radioctl/crates/radioctld/src/knob.rs:29` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Seen` | `Seen(i64)` | The radio reported a frequency change by itself; the change in hertz. |
| `NotSeen` | `NotSeen` | Nothing arrived within the wait. |
| `Skipped` | `Skipped` | The volunteer pressed Enter. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `knob_result` (src/knob.rs:84).

**See also:** test `a_knob_turn_on_a_transceive_rig_is_seen_with_its_frequency_delta (src/knob.rs)`.

### `Cli` [@ANCHOR: radioctl:radioctl_d_main_cli]

**Purpose and location.** What the command line asked for. Defined in `daemons/radioctl/crates/radioctld/src/main.rs:18` as a private `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `config` | `PathBuf` | The configuration file path (default as the loader chooses). |
| `check` | `bool` | Check the configuration and exit without serving. |
| `help` | `bool` | `--help` or `-h` was given: print the usage and exit with status 0. |

**Invariants, ownership and concurrency.** Parsed from the command line by `parse_args`; `help` and `check` end the program before any port is opened.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_args` (src/main.rs:29), `run` (src/main.rs:78).

**See also:** test `loading_a_missing_file_fails_and_check_mode_does_not_listen (src/main.rs)`.

### `MeterCal` [@ANCHOR: radioctl:radioctl_d_meter_cal_meter_cal]

**Purpose and location.** The scale points of one model: ascending `(raw deflection, value)` pairs. Defined in `daemons/radioctl/crates/radioctld/src/meter_cal.rs:23` as a pub `struct`. It derives Debug, Clone, Copy.

| Field | Type | Meaning |
|---|---|---|
| `swr` | `&'static [(f64, f64)]` | SWR scale points as ascending (raw deflection, ratio) pairs. |
| `alc_full` | `f64` | The deflection at which the ALC meter reads its maximum. |
| `comp` | `&'static [(f64, f64)]` | Compression meter points as (raw, decibels) pairs. |
| `vd` | `&'static [(f64, f64)]` | Supply voltage meter points as (raw, volts) pairs. |
| `id` | `&'static [(f64, f64)]` | Drain current meter points as (raw, amperes) pairs. |
| `power` | `&'static [(f64, f64)]` | Deflection to the fraction (0 to 1) of full output power. |

**Invariants, ownership and concurrency.** A static table of scale points, UNVERIFIED against any radio (`CALIBRATION_VERIFIED` is false); the other fields `alc_full` and `power` are documented in the field list above.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `for_rig` (src/meter_cal.rs:86), `swr_ratio` (src/meter_cal.rs:101), `alc_fraction` (src/meter_cal.rs:115), `comp_db` (src/meter_cal.rs:138), `vd_volts` (src/meter_cal.rs:144), `id_amps` (src/meter_cal.rs:150), `power_fraction` (src/meter_cal.rs:159), `level` (src/meter_cal.rs:177).

**See also:** test `swr_passes_through_each_models_printed_points (src/meter_cal.rs)`.

### `ModeEntry` [@ANCHOR: radioctl:radioctl_d_names_mode_entry]

**Purpose and location.** One mode in Hamlib's vocabulary: the library mode, its `rigctl` name, its bit and a nominal passband. Defined in `daemons/radioctl/crates/radioctld/src/names.rs:13` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `mode` | `Mode` | The library's mode. |
| `name` | `&'static str` | The name used in the `rigctld` text protocol. |
| `bit` | `u32` | The bit position in the mode mask of `\dump_state`. |
| `nominal_width_hz` | `u32` | A nominal passband width in hertz, reported for the mode. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `every_mode_round_trips_through_its_hamlib_name (src/names.rs)`.

### `VfoSel` [@ANCHOR: radioctl:radioctl_d_names_vfo_sel]

**Purpose and location.** A VFO as a client names it. Defined in `daemons/radioctl/crates/radioctld/src/names.rs:157` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Current` | `Current` | `currVFO`: the VFO selected by `V`. |
| `A` | `A` | `VFOA`, `Main` or `MainA`. |
| `B` | `B` | `VFOB` or `MainB`. |
| `Rx` | `Rx` | `RX`: the receive VFO. |
| `Tx` | `Tx` | `TX`: the transmit VFO (VFO B while split is on). |
| `Absent` | `Absent` | A VFO Hamlib knows that radioctl rigs do not have (`VFOC`, `Sub`, `MEM`, ...). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `take_vfo` (src/command.rs:528), `parse_vfo` (src/names.rs:176), `vfo_echo` (src/reply.rs:80), `resolve_vfo` (src/service.rs:209), `set_split_vfo` (src/service.rs:418).

**See also:** test `vfo_tokens_are_recognised_and_other_words_are_not (src/names.rs)`.

### `LevelSource` [@ANCHOR: radioctl:radioctl_d_names_level_source]

**Purpose and location.** Where a level's value comes from. Defined in `daemons/radioctl/crates/radioctld/src/names.rs:206` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Radio` | `Radio(Level)` | A numeric level of the rig, scaled by the rig family's range. |
| `Swr` | `Swr` | The SWR meter: tenths of a unit from `read-swr`, else the raw meter via `meter_cal`. |
| `RawStr` | `RawStr` | The raw S-meter reading. |
| `Strength` | `Strength` | The signal strength in decibels relative to S9 (Hamlib's `STRENGTH`), from the engine's signal reading through the family's table; served only where the family has one. |
| `PowerWatts` | `PowerWatts` | The output power in watts from the engine's power reading and the description's calibration. |
| `PowerFraction` | `PowerFraction` | The output power as a fraction of the meter's scale (calibrated when the description has points). |
| `Meter` | `Meter(radioctl_core::units::Meter)` | A raw transmitter meter converted by the unverified [`crate::meter_cal`] points. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `level_ranges_match_what_each_engine_accepts (tests/levels.rs)`.

### `LevelInfo` [@ANCHOR: radioctl:radioctl_d_names_level_info]

**Purpose and location.** A level radioctld can serve. Defined in `daemons/radioctl/crates/radioctld/src/names.rs:226` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | Hamlib's name. |
| `bit` | `u32` | Bit in the `has_get_level` / `has_set_level` masks and index in `level_gran`. |
| `source` | `LevelSource` | Where the value comes from. |
| `fraction` | `bool` | True when the client sees a 0..1 fraction; false when it sees the rig's own integer. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `level_gran` (src/caps.rs:62), `granularity` (src/caps.rs:92), `served_levels` (src/names.rs:501), `level_mask` (src/names.rs:589), `served_level` (src/service.rs:474).

**See also:** test `level_names_are_known_unserved_or_unknown (src/names.rs)`.

### `LevelLookup` [@ANCHOR: radioctl:radioctl_d_names_level_lookup]

**Purpose and location.** A level name lookup. Defined in `daemons/radioctl/crates/radioctld/src/names.rs:376` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Known` | `Known(LevelInfo)` | A level radioctld can serve (if the rig has it). |
| `Unserved` | `Unserved` | A level Hamlib has that radioctld does not serve. |
| `Unknown` | `Unknown` | Not a level name at all. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `level_lookup` (src/names.rs:387).

**See also:** test `level_names_are_known_unserved_or_unknown (src/names.rs)`.

### `FuncKind` [@ANCHOR: radioctl:radioctl_d_names_func_kind]

**Purpose and location.** A function (on/off switch) in Hamlib's vocabulary that radioctld serves. Defined in `daemons/radioctl/crates/radioctld/src/names.rs:536` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Rit` | `Rit` | RIT on or off. |
| `Xit` | `Xit` | XIT on or off. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `functions_are_known_unserved_or_unknown (src/names.rs)`.

### `FuncLookup` [@ANCHOR: radioctl:radioctl_d_names_func_lookup]

**Purpose and location.** A function name lookup. Defined in `daemons/radioctl/crates/radioctld/src/names.rs:555` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Known` | `Known(FuncKind)` | A function radioctld serves (if the rig has it). |
| `Unserved` | `Unserved` | A function Hamlib has that radioctld does not serve. |
| `Unknown` | `Unknown` | Not a function name at all. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `func_lookup` (src/names.rs:566).

**See also:** test `functions_are_known_unserved_or_unknown (src/names.rs)`.

### `Body` [@ANCHOR: radioctl:radioctl_d_reply_body]

**Purpose and location.** What a successful command answers with. Defined in `daemons/radioctl/crates/radioctld/src/reply.rs:34` as a pub `enum`. It derives Debug, Clone, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Done` | `Done` | A set, or any command with no value to return. |
| `Fields` | `Fields(Vec<(&'static str, String)>)` | Values: a label for the extended format (empty for none) and the value text. |
| `Text` | `Text { text: String, rprt: bool, }` | Free text that is its own answer (`\dump_state`, `\dump_caps`); `rprt` appends `RPRT 0`. The text, ending in a newline. Whether `RPRT 0` follows. |
| `Close` | `Close` | Answer `RPRT 0` and close the connection. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `render` (src/reply.rs:105), `execute` (src/rotctl.rs:235), `set` (src/service.rs:197), `set_mode` (src/service.rs:268), `refuse` (src/service.rs:319), `key` (src/service.rs:348), `unkey` (src/service.rs:364), `send_morse` (src/service.rs:376).

**See also:** test `plain_format_matches_hamlib (src/reply.rs)`.

### `Header` [@ANCHOR: radioctl:radioctl_d_reply_header]

**Purpose and location.** The parts of a request that shape its response header. Defined in `daemons/radioctl/crates/radioctld/src/reply.rs:52` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `extended` | `bool` | The extended (`+`) format was asked for. |
| `name` | `&'static str` | The command's long name. |
| `vfo` | `Option<String>` | The VFO to echo: the token the client sent, or `currVFO` in VFO mode. |
| `args` | `String` | The arguments to echo. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `header_line` (src/reply.rs:90), `render` (src/reply.rs:105), `header` (src/reply.rs:145).

**See also:** test `rendered_replies_hold_no_control_characters_whatever_the_client_sent (tests/parser_props.rs)`.

### `BoxedEngine` [@ANCHOR: radioctl:radioctl_d_rigs_boxed_engine]

**Purpose and location.** A protocol engine behind a box, so one `spawn` serves every family. Defined in `daemons/radioctl/crates/radioctld/src/rigs.rs:45` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `0` | `Box<dyn ProtocolEngine>` | The boxed engine; every `ProtocolEngine` call is forwarded to it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_daemons_boxed_engine_forwards_every_engine_hook (src/rigs.rs)`.

### `BuiltRig` [@ANCHOR: radioctl:radioctl_d_rigs_built_rig]

**Purpose and location.** A rig that is up: its service, and the control of its virtual stand-in if it is one. Defined in `daemons/radioctl/crates/radioctld/src/rigs.rs:82` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `service` | `Arc<RigService>` | The service clients talk to. |
| `virtual_control` | `Option<VirtualRigControl>` | Control of the virtual rig behind the service (state, front panel), for virtual rigs. |
| `serial_link` | `Option<SerialMonitor>` | The serial port's control lines, status and counters, for serial rigs. Kept for the life of the rig: the actor's link watch and the keyer's lines depend on it. |
| `net_link` | `Option<NetMonitor>` | The network link's status and counters, for TCP rigs. |
| `usb_link` | `Option<UsbMonitor>` | The USB link's status and counters, for USB rigs (virtual USB rigs included). |
| `baud_probe` | `Option<ProbeResult>` | What the baud probe found, when the configuration asked for one (`baud = "auto"` or `baud_probe`); `None` for a fixed rate, which is the default. |
| `radiod` | `Option<RadiodControl>` | The `radiod` channel's audio queue, counters and front-end status, for radiod rigs. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_radiod_transport_becomes_the_bridge_settings_with_the_descriptions_start (src/rigs.rs)`.

### `BuiltRotator` [@ANCHOR: radioctl:radioctl_d_rotators_built_rotator]

**Purpose and location.** A rotator that is up: its service, and the control of its virtual stand-in if it is one. Defined in `daemons/radioctl/crates/radioctld/src/rotators.rs:23` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `service` | `Arc<RotatorService>` | The service clients talk to. |
| `virtual_control` | `Option<VirtualRotatorControl>` | Control of the virtual rotator behind the service (position, commands seen, faults), for virtual rotators. |
| `net_link` | `Option<NetMonitor>` | The network link's status and counters. Kept for the life of the rotator. |
| `serial_link` | `Option<SerialMonitor>` | The serial port's status and counters. Kept for the life of the rotator. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_rig_and_a_rotator_are_served_from_one_file (tests/rotator.rs)`.

### `RotCommand` [@ANCHOR: radioctl:radioctl_d_rotctl_rot_command]

**Purpose and location.** A rotator command the server executes. Defined in `daemons/radioctl/crates/radioctld/src/rotctl.rs:26` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `SetPosition` | `SetPosition { az: f32, el: f32, }` | `P az el`: move. Azimuth, degrees. Elevation, degrees. |
| `GetPosition` | `GetPosition` | `p`: read the position. |
| `Stop` | `Stop` | `S`: stop. |
| `Park` | `Park` | `K`: go to the park position. |
| `GetInfo` | `GetInfo` | `_`: the rotator's name. |
| `DumpState` | `DumpState` | `\dump_state`: what `rotctl -m 2` reads on connect. |
| `Quit` | `Quit` | `q`: close the connection. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `rot_error_code` (src/rotctl.rs:189), `execute` (src/rotctl.rs:235).

**See also:** test `requests_parse_strictly (src/rotctl.rs)`.

### `RotParsed` [@ANCHOR: radioctl:radioctl_d_rotctl_rot_parsed]

**Purpose and location.** What a request line turned into. Defined in `daemons/radioctl/crates/radioctld/src/rotctl.rs:50` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `extended` | `bool` | The extended (`+`) response format was asked for. |
| `name` | `&'static str` | The long name of the command, for the response header; empty when it was not recognised. |
| `args` | `String` | The arguments, blank-normalised, for the response header. |
| `command` | `Result<RotCommand, i32>` | The command, or the `RPRT` code to answer with. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_rot_line` (src/rotctl.rs:126), `parsed` (src/rotctl.rs:315).

**See also:** test `requests_parse_strictly (src/rotctl.rs)`.

### `RotatorService` [@ANCHOR: radioctl:radioctl_d_rotctl_rotator_service]

**Purpose and location.** The service behind a rotator's listener: one [`Rotator`] handle, shared by every client. Defined in `daemons/radioctl/crates/radioctld/src/rotctl.rs:202` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rotator` | `Rotator` | The rotator handle shared by every client. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/rotctl.rs:209), `rot_answer` (src/rotctl.rs:272).

**See also:** test `a_rotctld_rotator_moves_stops_and_refuses_what_is_out_of_travel (tests/rotator.rs)`.

### `SharedRotatorService` [@ANCHOR: radioctl:radioctl_d_rotctl_shared_rotator_service]

**Purpose and location.** A shared rotator service. Defined in `daemons/radioctl/crates/radioctld/src/rotctl.rs:309` as a pub `type`. It derives no traits.

**Definition.** `pub type SharedRotatorService = Arc<RotatorService>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_rotctld_rotator_moves_stops_and_refuses_what_is_out_of_travel (tests/rotator.rs)`.

### `Verdict` [@ANCHOR: radioctl:radioctl_d_selftest_verdict]

**Purpose and location.** How one check ended. Defined in `daemons/radioctl/crates/radioctld/src/selftest.rs:39` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Pass` | `Pass(String)` | The radio answered and the answer decoded. |
| `NotApplicable` | `NotApplicable(String)` | The description does not support the operation (not a radio fault). |
| `Fail` | `Fail(String)` | The radio did not answer, refused, or sent bytes the engine could not decode. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `res` (src/selftest_cli.rs:1106), `result` (src/upload.rs:510).

**See also:** test `a_check_that_did_not_pass_is_never_blamed_on_the_user (src/selftest.rs)`.

### `CheckResult` [@ANCHOR: radioctl:radioctl_d_selftest_check_result]

**Purpose and location.** One executed check. Defined in `daemons/radioctl/crates/radioctld/src/selftest.rs:50` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The check's name, such as `read frequency A`. |
| `op` | `String` | The operation, as printed by `Debug`. |
| `verdict` | `Verdict` | What happened. |
| `millis` | `u128` | Milliseconds from sending to the verdict. |
| `wire` | `String` | The wire log lines (hexadecimal, both directions) written during this check; empty when no wire file was given. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `knob_result` (src/knob.rs:84), `render_report` (src/selftest.rs:228), `run_checks` (src/selftest.rs:421), `firmware_text` (src/selftest_cli.rs:281), `thanks` (src/selftest_cli.rs:303), `res` (src/selftest_cli.rs:1106), `result` (src/upload.rs:510), `input` (src/upload.rs:520).

**See also:** test `a_check_that_did_not_pass_is_never_blamed_on_the_user (src/selftest.rs)`.

### `ReportHeader` [@ANCHOR: radioctl:radioctl_d_selftest_report_header]

**Purpose and location.** What the report header states about the run. Defined in `daemons/radioctl/crates/radioctld/src/selftest.rs:66` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `rig_id` | `String` | The rig description id. |
| `rig_name` | `String` | The description's display name. |
| `family` | `String` | The protocol family name. |
| `target` | `String` | The target with any USB serial number removed. |
| `baud` | `u32` | The baud rate used. |
| `started` | `String` | The time of the run, as printed by [`utc_stamp`]. |
| `verified` | `BTreeMap<&'static str, usize>` | Facts of the description per verified state. |
| `port_identity` | `String` | The USB identity of the port (ids and product string, never the serial number), or empty. |
| `baud_probe` | `String` | What the baud probe tried, winner last, or empty when it did not run. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `render_report` (src/selftest.rs:228).

**See also:** test `a_check_that_did_not_pass_is_never_blamed_on_the_user (src/selftest.rs)`.

### `AnyEngine` [@ANCHOR: radioctl:radioctl_d_selftest_any_engine]

**Purpose and location.** `spawn_current` wants a sized engine type; this lets the engine of any family go through it. Defined in `daemons/radioctl/crates/radioctld/src/selftest.rs:344` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `0` | `crate) struct AnyEngine(pub(crate) Box<dyn ProtocolEngine>` | The boxed engine; calls are forwarded to it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `virtual_ic7300_passes_and_the_wire_log_has_both_directions (src/selftest.rs)`.

### `BaudArg` [@ANCHOR: radioctl:radioctl_d_selftest_cli_baud_arg]

**Purpose and location.** The baud rate to use. Defined in `daemons/radioctl/crates/radioctld/src/selftest_cli.rs:52` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Fixed` | `Fixed(u32)` | This rate. |
| `Auto` | `Auto` | Try the usual rates with the description's own frequency read (`baud = "auto"`). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_baud` (src/selftest_cli.rs:166).

**See also:** test `flags_and_the_old_positional_spelling_parse (src/selftest_cli.rs)`.

### `Options` [@ANCHOR: radioctl:radioctl_d_selftest_cli_options]

**Purpose and location.** What the command line asked for. Defined in `daemons/radioctl/crates/radioctld/src/selftest_cli.rs:61` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `list` | `bool` | Print the ports and descriptions, test nothing. |
| `version` | `bool` | Print the version. |
| `no_discover` | `bool` | Do not discover; a description and a port are required. |
| `rigs_dir` | `Option<PathBuf>` | A directory of descriptions instead of the embedded set. |
| `description` | `Option<String>` | Test this description id. |
| `port` | `Option<String>` | Test the rig on this port. |
| `baud` | `BaudArg` | The baud setting. |
| `unmanaged` | `bool` | Do not manage DTR and RTS (pseudo-terminals have none). |
| `out` | `Option<PathBuf>` | The report file when one rig is tested. |
| `out_dir` | `Option<PathBuf>` | The directory for reports and the summary. |
| `name` | `Option<String>` | Contact name given on the command line. |
| `email` | `Option<String>` | Contact email given on the command line. |
| `no_upload` | `bool` | Write the local report only. |
| `update` | `bool` | Install a newer version of this program if there is one, without asking. |
| `no_update_check` | `bool` | Do not check for a newer version. |
| `server` | `Option<String>` | Upload here instead of hams.com (https, or http to this machine). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_args` (src/selftest_cli.rs:99), `report_path` (src/selftest_cli.rs:264), `run` (src/selftest_cli.rs:571).

**See also:** test `flags_and_the_old_positional_spelling_parse (src/selftest_cli.rs)`.

### `Host` [@ANCHOR: radioctl:radioctl_d_selftest_cli_host]

**Purpose and location.** What the program needs from the machine it runs on (injected so tests need no hardware). Defined in `daemons/radioctl/crates/radioctld/src/selftest_cli.rs:177` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `ports` | `Vec<PortInfo>` | The serial ports present. |
| `interactive` | `bool` | True when a person is at a terminal and can be asked. |
| `contact_file` | `Option<PathBuf>` | The file the contact answers are kept in, if there is a configuration directory. |
| `env_contact` | `Contact` | Contact details from environment variables. |
| `lan_window` | `Duration` | How long to listen for network radios (zero: do not). |
| `update` | `Option<UpdateConfig>` | The release channel to check for a newer version; `None` when this build has none. |
| `restart_after_update` | `bool` | Start the new program after an update (tests turn this off). |
| `enter_pressed` | `Option<EnterPoll>` | Polled during the knob check: true once the volunteer pressed Enter. `None`: never. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `run` (src/selftest_cli.rs:571), `host` (tests/selftest_cli.rs:100), `host` (tests/selftest_update.rs:95), `flow` (tests/selftest_update.rs:124).

**See also:** test `version_list_and_usage_errors_need_no_radio (src/selftest_cli.rs)`.

### `Target` [@ANCHOR: radioctl:radioctl_d_selftest_cli_target]

**Purpose and location.** One rig to test. Defined in `daemons/radioctl/crates/radioctld/src/selftest_cli.rs:198` as a private `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `target` | `String` | The text that names the port or address under test. |
| `port` | `Option<PortInfo>` | The port information when the target is a local port, if known. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `an_unopenable_port_is_an_error_not_a_hang (src/selftest_cli.rs)`.

### `Tested` [@ANCHOR: radioctl:radioctl_d_selftest_cli_tested]

**Purpose and location.** What a tested rig leaves for the upload. Defined in `daemons/radioctl/crates/radioctld/src/selftest_cli.rs:204` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `results` | `Vec<CheckResult>` | The check results in order. |
| `transport` | `&'static str` | The transport word (`serial`, `tcp` or similar) written to the report. |
| `baud` | `u32` | The baud rate that was used. |
| `usb_vid_pid` | `Option<String>` | The USB ids of the port, if it is a USB device. |
| `probe_tried` | `Option<Vec<u32>>` | The baud rates the probe tried, if a probe ran. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_summary_has_the_table_and_a_line_per_rig (src/selftest_cli.rs)`.

### `EnterPoll` [@ANCHOR: radioctl:radioctl_d_selftest_cli_enter_poll]

**Purpose and location.** A poll that is true once the volunteer pressed Enter. Defined in `daemons/radioctl/crates/radioctld/src/selftest_cli.rs:782` as a pub `type`. It derives no traits.

**Definition.** `pub type EnterPoll = Box<dyn FnMut() -> bool + Send>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_line_can_be_read_by_a_prompt_or_taken_by_the_poll_but_not_both (src/selftest_cli.rs)`.

### `StdinLines` [@ANCHOR: radioctl:radioctl_d_selftest_cli_stdin_lines]

**Purpose and location.** Standard input read by one thread, one line at a time, so the program can both wait for a line (a prompt) and ask whether one has arrived (the knob check's Enter to skip). Defined in `daemons/radioctl/crates/radioctld/src/selftest_cli.rs:786` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rx` | `std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<String>>>` | The receiving end of the channel the stdin reader thread fills, shared with the poll closure under a mutex. |
| `buf` | `Vec<u8>` | The bytes of the line being read. |
| `pos` | `usize` | The read position in `buf`. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex, mpsc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `spawn` (src/selftest_cli.rs:814).

**See also:** test `a_line_can_be_read_by_a_prompt_or_taken_by_the_poll_but_not_both (src/selftest_cli.rs)`.

### `BoxFuture` [@ANCHOR: radioctl:radioctl_d_server_box_future]

**Purpose and location.** A boxed future, for the object-safe [`LineService`]. Defined in `daemons/radioctl/crates/radioctld/src/server.rs:38` as a pub `type`. It derives no traits.

**Definition.** `pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `answer` (src/rotctl.rs:297), `disconnect` (src/rotctl.rs:303), `answer` (src/server.rs:44), `disconnect` (src/server.rs:46), `answer` (src/server.rs:52), `disconnect` (src/server.rs:58).

**See also:** test `a_peer_outside_the_allow_list_is_dropped_before_any_byte_is_read (tests/limits.rs)`.

### `Limits` [@ANCHOR: radioctl:radioctl_d_server_limits]

**Purpose and location.** The bounds a server enforces. Defined in `daemons/radioctl/crates/radioctld/src/server.rs:65` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `max_clients` | `usize` | Most simultaneous clients. |
| `idle_timeout` | `Duration` | A connection silent for this long is closed. |
| `line_timeout` | `Duration` | A started line must end within this. |
| `max_line_bytes` | `usize` | Longest request line, in bytes. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_config` (src/server.rs:79).

**See also:** test `limits_follow_the_configuration (src/server.rs)`.

### `ServerStats` [@ANCHOR: radioctl:radioctl_d_server_server_stats]

**Purpose and location.** Counters of a server. Defined in `daemons/radioctl/crates/radioctld/src/server.rs:91` as a pub `struct`. It derives Debug, Clone, Copy, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `accepted` | `u64` | Connections served. |
| `denied` | `u64` | Connections refused by the allow-list. |
| `busy` | `u64` | Connections refused because the client limit was reached. |
| `active` | `u64` | Clients connected now. |
| `line_too_long` | `u64` | Clients dropped for an over-long line. |
| `timeouts` | `u64` | Clients dropped for silence, a stalled line, or a stalled write. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `stats` (src/server.rs:170).

**See also:** test `the_client_limit_is_enforced_and_slots_are_freed (tests/limits.rs)`.

### `Counters` [@ANCHOR: radioctl:radioctl_d_server_counters]

**Purpose and location.** The atomic counters behind `ServerStats`, plus the next client id. Defined in `daemons/radioctl/crates/radioctld/src/server.rs:107` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `accepted` | `AtomicU64` | Connections accepted. |
| `denied` | `AtomicU64` | Connections refused by the allow list. |
| `busy` | `AtomicU64` | Connections refused because `max_clients` were connected. |
| `active` | `AtomicU64` | Clients connected now. |
| `line_too_long` | `AtomicU64` | Clients dropped for an over-long line. |
| `timeouts` | `AtomicU64` | Clients dropped for silence, a stalled line, or a stalled write. |
| `next_client` | `AtomicU64` | The next client number to hand out. |

**Invariants, ownership and concurrency.** Relaxed atomics updated from many tasks without a lock.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_client_limit_is_enforced_and_slots_are_freed (tests/limits.rs)`.

### `Shared` [@ANCHOR: radioctl:radioctl_d_server_shared]

**Purpose and location.** The state shared by the accept task and every client task of one server. Defined in `daemons/radioctl/crates/radioctld/src/server.rs:117` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `service` | `Arc<dyn LineService>` | The line service that answers requests. |
| `access` | `Arc<AllowList>` | The allow list. |
| `limits` | `Limits` | The connection limits. |
| `counters` | `Counters` | The counters. |
| `shutdown` | `watch::Receiver<bool>` | The shutdown signal, set by the handle. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `accept_loop` (src/server.rs:198).

**See also:** test `silent_and_stalled_clients_are_dropped (tests/limits.rs)`.

### `ServerHandle` [@ANCHOR: radioctl:radioctl_d_server_server_handle]

**Purpose and location.** A running server. Defined in `daemons/radioctl/crates/radioctld/src/server.rs:126` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `local_addr` | `SocketAddr` | The address the listener is bound to. |
| `shared` | `Arc<Shared>` | The shared state. |
| `stop` | `watch::Sender<bool>` | The sender of the shutdown signal. |
| `accept` | `JoinHandle<()>` | The accept task. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, JoinHandle (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_default_listener_is_on_loopback (tests/limits.rs)`.

### `ServiceConfig` [@ANCHOR: radioctl:radioctl_d_service_service_config]

**Purpose and location.** Per-rig service settings. Defined in `daemons/radioctl/crates/radioctld/src/service.rs:38` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `vfo_mode` | `bool` | `\chk_vfo` answers 1 and responses echo the VFO. |
| `cache_max_age` | `Duration` | Reads at most this old are answered from the rig's cache. |
| `request_timeout` | `Duration` | Deadline for a request that has to go to the rig. |
| `hamlib_model` | `u32` | The model number reported in `\dump_state`. |
| `rx_low_hz` | `u64` | Lowest frequency the rig is said to cover, in hertz. |
| `rx_high_hz` | `u64` | Highest frequency the rig is said to cover, in hertz. |
| `tx_power_mw` | `u32` | Maximum transmit power reported, in milliwatts. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `service_config` (src/rigs.rs:156), `new` (src/service.rs:102), `config` (src/service.rs:121).

**See also:** test `the_virtual_ic7300_transcript (tests/protocol.rs)`.

### `ServiceStats` [@ANCHOR: radioctl:radioctl_d_service_service_stats]

**Purpose and location.** Counters of a service. Defined in `daemons/radioctl/crates/radioctld/src/service.rs:57` as a pub `struct`. It derives Debug, Clone, Copy, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `commands` | `u64` | Commands executed. |
| `cache_hits` | `u64` | Reads answered from the cache. |
| `transmit_refused` | `u64` | Transmit commands refused (gate closed, link down, other client owns the transmitter). |
| `ptt_conflicts` | `u64` | Keying attempts refused because another client owns the transmitter. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `stats` (src/service.rs:127).

**See also:** test `a_second_client_reads_what_the_first_set_from_the_cache (tests/arbitration.rs)`.

### `Counters` [@ANCHOR: radioctl:radioctl_d_service_counters]

**Purpose and location.** The atomic counters behind `ServiceStats`. Defined in `daemons/radioctl/crates/radioctld/src/service.rs:69` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `commands` | `AtomicU64` | Commands answered. |
| `cache_hits` | `AtomicU64` | Reads answered from the cache. |
| `transmit_refused` | `AtomicU64` | Keying commands refused (transmit gate closed or not allowed). |
| `ptt_conflicts` | `AtomicU64` | Keying attempts refused because another client owns the transmitter. |

**Invariants, ownership and concurrency.** Relaxed atomics updated from many tasks without a lock.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_second_client_reads_what_the_first_set_from_the_cache (tests/arbitration.rs)`.

### `RigService` [@ANCHOR: radioctl:radioctl_d_service_rig_service]

**Purpose and location.** The service behind a rig's listener: the rig handle, its description, settings, the transmitter owner and the current VFO, shared by every client. It enforces that one client at a time may key and that transmit is refused unless the gate is open. Defined in `daemons/radioctl/crates/radioctld/src/service.rs:83` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `rig` | `RigHandle` | The rig handle. |
| `description` | `RigDescription` | The rig description. |
| `config` | `ServiceConfig` | The service settings. |
| `owner` | `Mutex<PttOwner>` | Which client owns the transmitter, behind a mutex held only to check or change the owner. |
| `current` | `Mutex<Vfo>` | The VFO that the current-VFO token means, behind a mutex. |
| `counters` | `Counters` | The counters. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `answer` (src/server.rs:229), `new` (src/service.rs:102).

**See also:** test `one_transmitter_one_owner_and_a_departing_owner_unkeys (tests/arbitration.rs)`.

### `UpdateConfig` [@ANCHOR: radioctl:radioctl_d_update_update_config]

**Purpose and location.** Everything the update needs to know; the relay's wrapper binary supplies it. Defined in `daemons/radioctl/crates/radioctld/src/update.rs:32` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `base_url` | `String` | The release channel, such as [`DEFAULT_BASE`] (https, or http to this computer). |
| `current_version` | `String` | The version of the bundle this program shipped in (the relay's version). |
| `keys` | `Vec<[u8; 32]>` | The release keys a bundle may be signed with (32-byte ed25519 public keys). |
| `exe` | `PathBuf` | The running executable, which is replaced. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `replacing_keeps_the_old_program_as_prev_and_failure_leaves_it_alone (src/update.rs)`.

### `Latest` [@ANCHOR: radioctl:radioctl_d_update_latest]

**Purpose and location.** What the release channel says is newest. Defined in `daemons/radioctl/crates/radioctld/src/update.rs:45` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `version` | `String` | The version, `X.Y.Z`. |
| `sha256` | `String` | The sha256 of the bundle, in hexadecimal. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `fetch_latest` (src/update.rs:95).

**See also:** test `the_channel_answers_the_version_and_the_bundle_and_offline_is_an_error (src/update.rs)`.

### `Contact` [@ANCHOR: radioctl:radioctl_d_upload_contact]

**Purpose and location.** What the volunteer chose to tell hams.com so it can contact them about the rig. Defined in `daemons/radioctl/crates/radioctld/src/upload.rs:35` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `name` | `Option<String>` | Name, if given. |
| `email` | `Option<String>` | Email address, if given. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `notice` (src/upload.rs:492), `input` (src/upload.rs:520).

**See also:** test `the_payload_has_the_contract_fields_and_nothing_private (src/upload.rs)`.

### `PayloadInput` [@ANCHOR: radioctl:radioctl_d_upload_payload_input]

**Purpose and location.** Everything the payload is built from. It has no field for a path, serial number or host name. Defined in `daemons/radioctl/crates/radioctld/src/upload.rs:44` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `report_id` | `String` | A random UUID v4 made for this run. |
| `sent_at` | `u64` | Unix time of the upload. |
| `description_id` | `String` | The rig description id. |
| `rig_name` | `String` | The description's display name, such as `Icom IC-7300`. |
| `transport` | `&'static str` | `serial`, `usb` or `virtual`. |
| `baud` | `u32` | The baud rate used. |
| `usb_vid_pid` | `Option<String>` | USB vendor and product id as `vvvv:pppp`, when known. |
| `probe_tried` | `Option<Vec<u32>>` | The rates the baud probe opened (winner last), when it ran. |
| `results` | `&'a [CheckResult]` | The checks that ran. |
| `contact` | `Contact` | The contact details given, if any. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_payload` (src/upload.rs:110), `input` (src/upload.rs:520).

**See also:** test `the_payload_has_the_contract_fields_and_nothing_private (src/upload.rs)`.

### `SavedContact` [@ANCHOR: radioctl:radioctl_d_upload_saved_contact]

**Purpose and location.** What is kept between runs: whether the volunteer was asked, and the answers. Defined in `daemons/radioctl/crates/radioctld/src/upload.rs:190` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `contact` | `Contact` | The saved contact details (either may be absent: Enter skips). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `load_saved` (src/upload.rs:215), `save` (src/upload.rs:228), `prompt_contact` (src/upload.rs:264).

**See also:** test `saved_answers_round_trip_and_are_private (src/upload.rs)`.

### `ServerUrl` [@ANCHOR: radioctl:radioctl_d_upload_server_url]

**Purpose and location.** Where to POST: parsed `--server` or default URL. Defined in `daemons/radioctl/crates/radioctld/src/upload.rs:312` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `tls` | `bool` | True for `https`. |
| `host` | `String` | Host name or address (no brackets). |
| `port` | `u16` | TCP port. |
| `path` | `String` | Path and query, starting with `/`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_server` (src/upload.rs:326).

**See also:** test `plain_http_is_loopback_only (src/upload.rs)`.
