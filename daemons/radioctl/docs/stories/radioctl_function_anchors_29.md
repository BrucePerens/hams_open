# radioctl function anchors, batch 29

Anchors for the `radioctl-protocols` crate (the virtual Kenwood level command) and the `radioctld` daemon crate (ADR 0113 standard): the client allow list (`access.rs`), the Hamlib-compatible capability text for `\dump_state` and `\dump_caps` (`caps.rs`), the rigctld text-protocol line buffer and parser (`command.rs`) and the `[server]` configuration checks (`config.rs`). Tests cite each anchor with `Tests [@ANCHOR: ...]`.
Transmit gate: parsing a command (`set_ptt`, `send_morse`) only builds a typed `Command`; it does not key anything. The daemon's transmit gate, band check and lock are applied when the command is executed, not in these functions. The allow list decides which clients may connect at all.


## `virtual_rig.rs`

### `fn kenwood_level(&mut self, name: &str, arg: &str) -> Option<String>` [@ANCHOR: radioctl:radioctl_virtual_kenwood_level]

Handles a Kenwood level command on the virtual radio. `&mut self`; `name` is the two-letter command (`AG` AF gain 0 to 255, `RG` RF gain 0 to 255, `MG` mic 0 to 100, `ML` monitor 0 to 9, `KS` keyer speed 10 to 60, anything else is RF power `PC` 5 to 100) and `arg` is the text after it.
A read is a bare command, except AF gain, which needs its main-receiver digit: a lone `0` reads and `0nnn` sets. A read returns `Some("<name>[0]<3 digits>;")` from the stored value (0 if never set). A set needs exactly 3 digits inside the range; a bad digit string or a value outside the range returns `Some("?;")`, the Kenwood error, and changes nothing. A successful set stores the value and returns `None` (no answer line). Simulated state only.


## `access.rs`

### `fn normalise(ip: IpAddr) -> IpAddr` [@ANCHOR: radioctl:radioctld_access_normalise]

Converts an IPv4-mapped IPv6 address (`::ffff:a.b.c.d`) into the IPv4 address it stands for; every other address is returned unchanged. `ip` is by value.
A dual-stack listening socket reports an IPv4 client in the mapped form, so without this a rule written as `192.168.1.0/24` would never match that client. It is applied to both the allow-list entries and the peer address. Pure, no errors.

### `fn prefix_matches(a: &[u8], b: &[u8], prefix: u8) -> bool` [@ANCHOR: radioctl:radioctld_access_prefix_matches]

Compares the first `prefix` bits of two byte strings. `a` and `b` are the address bytes (4 for IPv4, 16 for IPv6), `prefix` is the network prefix length in bits. Whole bytes before the prefix boundary must be equal; then the remaining bits are compared under a mask (for example prefix 20 compares 2 whole bytes and the top 4 bits of the third).
Returns false when either slice is too short for the prefix, so malformed input never matches by accident. A prefix of 0 matches everything. Pure, no allocation, no errors.

### `fn octets(ip: IpAddr) -> Vec<u8>` [@ANCHOR: radioctl:radioctld_access_octets]

Returns the address bytes of an IP address as a vector: 4 for IPv4 and 16 for IPv6, in network order. `ip` is by value.
It lets `allows` compare addresses with `prefix_matches` without caring about the family. Allocates a small vector; pure, no errors.

### `fn loopback_only() -> AllowList` [@ANCHOR: radioctl:radioctld_access_loopback_only]

Builds the default allow list: this machine only, `127.0.0.0/8` for IPv4 and `::1/128` for IPv6. It takes no arguments and cannot fail.
It is used when the `[server]` section has no `allow` key, so radioctld accepts only local clients unless the owner lists networks. Binding to a non-loopback address additionally requires an explicit allow list (see `server_config`), so a rig is not reachable from the network by default.

### `fn parse(entries: &[String]) -> Result<AllowList, String>` [@ANCHOR: radioctl:radioctld_access_parse]

Builds an allow list from text entries. `entries` is borrowed; each entry is an IP address with an optional prefix length, such as `192.168.1.0/24`, `10.0.0.5` (a single host, prefix 32) or `fd00::/8` (IPv6). Whitespace around the address and the prefix is trimmed, and IPv4-mapped addresses are normalised.
Errors are `Err(String)` sentences naming the entry: not an IP address, or a prefix that is not a number or is longer than 32 (IPv4) or 128 (IPv6). An empty list is valid and allows nobody. Pure; the result is checked per connection with `allows`.

### `fn allows(&self, peer: IpAddr) -> bool` [@ANCHOR: radioctl:radioctld_access_allows]

Says whether a client address is allowed to connect. `&self`; `peer` is the client's IP address by value. It is normalised (mapped IPv4 becomes IPv4), then compared against every network of the list; the address family must match and the address must fall inside the prefix.
Returns true if any network contains the peer, false otherwise, including for an empty list. Pure, no errors. The server calls it for every accepted connection and closes a connection that is not allowed, before any command is read.


## `caps.rs`

### `fn modes(description: &RigDescription) -> Vec<Mode>` [@ANCHOR: radioctl:radioctld_caps_modes]

Lists the operating modes of a description in a stable order. `description` is borrowed. It collects the modes of the description's mode-code table, sorts them and removes duplicates, so the order is the same on every call and a mode listed twice (a sideband and its data variant sharing a code) appears once.
The result is used to build the mode masks and the per-mode lines of `\dump_state` and the mode list of `\dump_caps`. Pure, no errors.

### `fn range_line(low: u64, high: u64, modes: u64, power: (i64, i64), vfos: u32) -> String` [@ANCHOR: radioctl:radioctld_caps_range_line]

Formats one frequency range line of the Hamlib `\dump_state` text. `low` and `high` are the range limits in hertz, `modes` is the bit mask of the modes allowed in the range, `power` is the (minimum, maximum) transmit power pair in milliwatts (the receive range uses -1, -1), and `vfos` is the VFO mask (1 for A only, 3 for A and B).
The line has the form `<low>.000000 <high>.000000 <modes hex> <min power> <max power> <vfos hex> 0x1\n`, where the trailing `0x1` is the antenna mask. Pure, no errors.

### `fn level_gran(levels: &[LevelInfo], description: &RigDescription) -> String` [@ANCHOR: radioctl:radioctld_caps_gran]

Builds the `level_gran` text of `\dump_state`: one `index=min,max,step;` entry for each of the 64 level bits (`GRAN_ENTRIES`). `levels` is the list of levels the daemon serves, borrowed, each with its bit; `description` is the rig description used to find ranges.
A bit that is not served gets `0,0,0`. A served bit gets its minimum, maximum and step from `granularity`. The text is read by Hamlib's own client, so the numeric encodings matter (see `wire_float_bits` and `wire_int_as_float`). Pure, no errors.

### `fn wire_float_bits(value: f64) -> String` [@ANCHOR: radioctl:radioctld_caps_wire_float]

Formats a value the way Hamlib's `rigctld` writes the first level entries (AF, RF, and the bits up to 4) in `level_gran`: the bit pattern of the value as a 32-bit float, printed as a decimal integer. `value` is a `f64`, converted to `f32` first, so precision beyond 32-bit is lost.
Observed behaviour: Hamlib's client reads `4=0,1065353216,998277249` as the range 0 to 1 in steps of 1/255. Pure, returns a `String`, no errors.

### `fn wire_int_as_float(value: u32) -> String` [@ANCHOR: radioctl:radioctld_caps_wire_int]

Formats an integer level value the way Hamlib's client expects for an integer level at a later `level_gran` index: the integer's bit pattern read as a 32-bit float, in scientific notation, so `8` becomes `1.1210388e-44`. `value` is a `u32` by value.
It is an odd encoding but it is what the client parses. Pure, returns a `String`, no errors.

### `fn granularity(info: &LevelInfo, description: &RigDescription) -> (String, String, String)` [@ANCHOR: radioctl:radioctld_caps_granularity]

Gives the minimum, maximum and step text of one served level in the form the client parses for that level. `info` is the level's entry (its bit, its source radio level or computed value, whether it is a fraction); `description` supplies integer ranges.
Fraction levels are written as 32-bit float bit patterns for bits up to 4 and as six-decimal floats for later bits. Integer radio levels take their range from the description (`names::level_range`, 0 to 0 if unknown); at bits up to 4 they are plain decimal with step 1, and at later bits they use `wire_int_as_float`. Returns three strings. Pure. The remaining level sources are in the source beyond this entry.

### `fn preamp_list(description: &RigDescription) -> String` [@ANCHOR: radioctl:radioctld_caps_preamp_list]

Builds the preamplifier line of `\dump_state`: the step numbers the model accepts, each followed by a space, ended by `0`. `description` is borrowed; the steps are 1 up to `levels.preamp_steps`, so a model with two steps gives `1 2 0` and a model with none gives `0`.
Pure, no errors.

### `fn attenuator_list(description: &RigDescription) -> String` [@ANCHOR: radioctl:radioctld_caps_attenuator_list]

Builds the attenuator line of `\dump_state`: the decibel values the model accepts, each followed by a space, ended by `0`. `description` is borrowed; the values are the ascending `levels.attenuator_db` of the description, so a model with 6, 12 and 18 dB steps gives `6 12 18 0` and a model with none gives `0`.
Pure, no errors. A model that takes any whole-decibel value up to a maximum (`attenuator_any_up_to_db`) lists nothing here.

### `fn dump_state(view: &CapsView<'_>) -> String` [@ANCHOR: radioctl:radioctld_caps_dump_state]

Builds the text of rigctld's `\dump_state` answer, which Hamlib's client reads to learn what a rig can do. `view` is a read-only `CapsView` holding the description, the Hamlib model number, the receive range limits, whether the transmit gate is enabled and the transmit power in milliwatts.
It writes the protocol version and model, the receive range, a transmit range only if the transmit gate is enabled and the rig can set PTT (so a gate-closed daemon advertises no transmit), the tuning-step and filter lines, the maximum RIT/XIT, the preamplifier and attenuator lists, function and level masks, and `key=value` lines such as `ptt_type`, `has_set_freq`, `has_get_freq` and `rig_model`. Pure, no I/O. Exact masks come from helpers; see the source.

### `fn dump_caps(view: &CapsView<'_>) -> String` [@ANCHOR: radioctl:radioctld_caps_dump_caps]

Builds the text of rigctld's `\dump_caps` answer, a readable summary ended by a blank line. `view` is the same read-only `CapsView` as for `dump_state`.
It prints the model, `Mfg name: radioctl`, a version line naming radioctld, the rig id, the transmit gate state (`open` or `closed`), the mode list in Hamlib names, and Y/N lines for what the description can do. Pure, no I/O, no errors. The remaining lines are in the source.


## `command.rs`

### `fn new(max: usize) -> LineBuffer` [@ANCHOR: radioctl:radioctld_command_linebuffer_new]

Creates a buffer that collects incoming bytes into request lines. `max` is the longest line allowed, in bytes, not counting the line ending; a `max` of 0 is raised to 1.
The buffer starts empty. It bounds memory: nothing beyond `max` is ever held for a line, so a client that sends no newline cannot make the daemon buffer without limit. Cannot fail.

### `fn has_partial(&self) -> bool` [@ANCHOR: radioctl:radioctld_command_linebuffer_has_partial]

Says whether the start of a line has arrived but its newline has not. `&self`, returns true when the pending bytes are not empty.
The server uses it to apply the line timeout: a client that sends half a line and stops is cut off, while an idle client between lines is governed by the idle timeout instead. Pure, no errors.

### `fn push(&mut self, bytes: &[u8]) -> Result<Vec<Line>, LineTooLong>` [@ANCHOR: radioctl:radioctld_command_linebuffer_push]

Adds received bytes and returns the lines they completed. `&mut self`; `bytes` is the new data from the socket, which may hold part of a line, one line or several.
Each `\n` ends a line; a `\r` just before it is removed, and each line is turned into `Line::Text` or `Line::Invalid` by `into_line`. If a line grows past the limit the function returns `Err(LineTooLong)` and the caller drops the connection; the bytes past the limit are never stored. Partial data stays in the buffer for the next call. No I/O.

### `fn into_line(bytes: Vec<u8>) -> Line` [@ANCHOR: radioctl:radioctld_command_into_line]

Classifies one received line. `bytes` is the line without its newline, by value. It is `Line::Text` when the bytes are valid UTF-8 and contain no control characters (a tab is allowed), and `Line::Invalid` otherwise.
Rejecting control characters keeps escape sequences and binary data out of the parser, the logs and the response header. Pure, no errors.

### `fn resolve(word: &str, long_form: bool) -> Option<(&'static str, bool)>` [@ANCHOR: radioctl:radioctld_command_resolve]

Finds what a command word means. `word` is the text after the leading `\` (long form) or a single character (short form); `long_form` says which. Returns `Some((canonical long name, implemented))`, or `None` when the word is not a command at all.
Long names are looked up in the served list, then in the known-but-unimplemented list (returned with false). A short form must be exactly one character: it is looked up in the table of short commands, `1` is `dump_caps`, `q` or `Q` is `quit`, and the known unimplemented short letters give `("unimplemented", false)`. A word of several characters in short form gives `None`. Pure.

### `fn takes_vfo(name: &str) -> bool` [@ANCHOR: radioctl:radioctld_command_takes_vfo]

Says whether a command accepts an optional leading VFO token (such as `VFOA` or `currVFO`). `name` is the canonical long command name. The commands that do not take one are `dump_caps`, `get_active_tx_freq`, `dump_state`, `chk_vfo`, `get_info`, `get_rig_info`, `get_vfo_list`, `quit`, `set_vfo`, `set_powerstat`, `get_lock_mode` and `set_lock_mode`; every other name returns true.
Pure, no errors.

### `fn parse_hz(text: &str) -> Option<u64>` [@ANCHOR: radioctl:radioctld_command_parse_hz]

Parses a frequency in hertz from decimal text. `text` may be `14074000` or `14074000.000000`; a fraction of a hertz is dropped. The whole part must be 1 to 12 digits, the fraction (if any) digits only and at most 12, with no sign and no exponent.
Returns `None` for anything else, and for a value above `MAX_HZ`, 99,999,999,999 Hz. The limits keep a hostile or broken client from sending a number that overflows or is nonsense. Pure.

### `fn parse_int(text: &str) -> Option<i64>` [@ANCHOR: radioctl:radioctld_command_parse_int]

Parses a signed integer from text. `text` is borrowed and may be at most 12 characters, so it cannot be a huge number string; a leading minus or plus sign follows Rust's integer parsing.
Returns `None` for text longer than 12 characters, empty text or anything that is not an integer, and a value that does not fit in an `i64` (not possible within 12 characters). Pure.

### `fn parse_float(text: &str) -> Option<f64>` [@ANCHOR: radioctl:radioctld_command_parse_float]

Parses a floating-point number from text. `text` is borrowed and may be at most 32 characters. Returns `Some(value)` only for a finite number: `NaN` and infinity are refused, as is text that does not parse.
Used by `build` for floating-point command arguments. Pure, no errors beyond `None`.

### `fn sanitise(text: &str) -> String` [@ANCHOR: radioctl:radioctld_command_sanitise]

Makes client-supplied text safe to echo back in a response header: every control character becomes `?`. `text` is borrowed; the result is a new `String`.
The daemon repeats the arguments of a command in the extended response format, so without this a client could inject line breaks or terminal escape sequences into the reply or the logs. Pure, no errors.

### `fn morse_text(text: &str) -> Result<String, i32>` [@ANCHOR: radioctl:radioctld_command_morse_text]

Validates the text of a `send_morse` command. `text` is borrowed and must be 1 to `MAX_MORSE_CHARS` (64) printable ASCII characters (0x20 to 0x7E).
Returns `Ok(text)` or `Err(EINVAL)`, the Hamlib invalid-argument code -1, for empty text, too long text or any other byte. It only checks the text; sending the message keys the transmitter, and the transmit gate and band checks are applied when the command is executed.

### `fn mode_and_passband(args: &[&str]) -> Result<(Mode, i64), i32>` [@ANCHOR: radioctl:radioctld_command_mode_and_passband]

Parses the arguments of `set_mode`: exactly two, a Hamlib mode name and a passband width. `args` is the already split tokens. The mode name is converted with `mode_from_hamlib` and the passband with `parse_int`.
Returns `Ok((Mode, passband))` or `Err(EINVAL)` for a wrong number of arguments, an unknown mode name or a passband that is not an integer. The passband is in hertz, with 0 meaning the radio's default in Hamlib's convention (`unknown`: check the command handler for the meaning of 0 and negative values). Pure.

### `fn flag(text: &str) -> Result<bool, i32>` [@ANCHOR: radioctl:radioctld_command_flag]

Parses a 0/1 flag argument. `text` is borrowed; any nonzero integer means on and 0 means off. Returns `Ok(bool)` or `Err(EINVAL)` when the text is not an integer (at most 12 characters, as for `parse_int`).
Pure.

### `fn build(name: &str, args: &[&str], rest: &str) -> Result<Command, i32>` [@ANCHOR: radioctl:radioctld_command_build]

Builds the typed `Command` for a command name from its arguments. `name` is the canonical long name; `args` is the argument tokens, with the VFO token already removed; `rest` is the argument text with its internal spacing, for commands that carry free text (`send_morse`).
Commands with no arguments (`get_freq`, `get_mode`, `get_ptt` and others) must receive none. `set_freq` takes a frequency, `set_mode` a mode and passband, `set_vfo` a VFO name, `set_ptt` a value 0 to 3, `set_split_vfo` an on flag and a transmit VFO, and so on. A wrong count or an invalid value returns `Err(EINVAL)`. Pure: it only builds the command; `set_ptt` and `send_morse` key the transmitter only when executed, under the transmit gate. The rest of the table is in the source.

### `fn take_vfo<'a>(name: &str, rest: &'a str) -> (Option<(String, VfoSel)>, &'a str)` [@ANCHOR: radioctl:radioctld_command_take_vfo]

Splits an optional leading VFO token from the argument text. `name` is the command name; `rest` is the text after the command word, borrowed. Returns the optional (original token, selected VFO) and the remaining text with leading space removed.
For `send_morse` only `VFOA`, `VFOB` and `currVFO` are accepted, and only when text follows, so Morse text that happens to look like a VFO name, such as `TX test`, is not taken as one. Pure, no errors.

### `fn parse_line(line: &str) -> Option<Parsed>` [@ANCHOR: radioctl:radioctld_command_parse_line]

Parses one rigctld request line. `line` is the text of the line; surrounding whitespace is trimmed and a blank line returns `None` (nothing to answer). A leading `+` asks for the extended response format.
A `\` starts a long command name, otherwise the first character is a short command. An unknown word gives `Parsed` with `command = Err(EINVAL)`; a known but unimplemented one gives `Err(ENIMPL)`. For implemented commands an optional VFO token is taken, the arguments are split on whitespace (at most `MAX_ARGS`, more is `EINVAL`) and `build` makes the command. The echoed arguments are sanitised. Pure; it executes nothing.


## `config.rs`

### `fn in_range<T: PartialOrd + std::fmt::Display + Copy>(key: &str, value: Option<T>, default: T, low: T, high: T) -> Result<T, String>` [@ANCHOR: radioctl:radioctld_config_in_range]

Checks a configuration value against an inclusive range. `key` is the config key used in the message; `value` is the optional value read from the file; `default` is used when it is absent; `low` and `high` are the limits. The type is any copyable, ordered, displayable type.
Returns the value (or default) when within the range, otherwise `Err("<key> = <value> is outside <low>..=<high>")`. The default is checked too. Pure.

### `fn server_config(section: ServerSection, base_dir: &Path) -> Result<(ServerConfig, bool), String>` [@ANCHOR: radioctl:radioctld_config_server]

Validates the `[server]` section of the daemon's configuration file and builds `ServerConfig`. `section` is taken by value; `base_dir` is the directory a relative `secrets_dir` is resolved against. Returns the config and the `allow_non_loopback_bind` flag, or an `Err(String)` sentence.
Defaults and limits: `allow` defaults to loopback only; `max_clients` 16 (1 to 256); `idle_timeout_secs` 600 (1 to 86400); `line_timeout_secs` 10 (1 to 300); `max_line_bytes` 1024 (64 to 65536); `request_timeout_ms` 2000 (100 to 30000); `cache_max_age_ms` 1000 (0 to 60000). An empty `secrets_dir` is an error. `allow_non_loopback_bind` without an explicit `allow` list is refused, so the daemon cannot be exposed to the network without naming who may connect. No I/O.
