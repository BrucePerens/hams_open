# radioctl function anchors, batch 30

Anchors for the `radioctld` daemon crate (ADR 0113 standard): configuration validation (`config.rs`), daemon start and stop (`daemon.rs`), rotator construction (`rotators.rs`), the rotctld text protocol (`rotctl.rs`), the TCP server's rig service hooks (`server.rs`), the command line and signal handling (`main.rs`) and the Hamlib mode names (`names.rs`). Tests cite each anchor with `Tests [@ANCHOR: ...]`.
Transmit gate: the config functions only read and check values (a rig's `transmit` setting and `max_transmit_s` are carried through, not acted on). Daemon stop and a client disconnect unkey any rig whose transmitter a client held, which needs no gate. Rotators have no transmit path.


## `config.rs`

### `fn transport_config(section: TransportSection, has_secrets_dir: bool) -> Result<TransportConfig, String>` [@ANCHOR: radioctl:radioctld_config_transport]

Validates a rig's `[rig.transport]` section and builds the `TransportConfig`. `section` is the parsed section by value; `has_secrets_dir` says whether the server has a secret store, for a framing that needs one.
Kinds: `virtual` (a simulated radio; `reply_delay_ms` 0 to 10000, default 0), `serial` (a port by path or by USB identity, `baud` or `baud_probe` through `baud_setting`, and optional control `lines`), and the network kinds `tcp` (with its framing, see `tcp_framing`) and `radiod` (multicast groups checked by `multicast_group`), among others. Any invalid value returns `Err(String)` naming the key. Pure; it opens nothing.

### `fn baud_rate(rate: u32) -> Result<u32, String>` [@ANCHOR: radioctl:radioctld_config_baud_rate]

Checks one serial baud rate. `rate` is by value and must be 300 to 4,000,000 inclusive; the same value is returned on success.
A rate outside that range returns `Err("baud <rate> is outside 300..=4000000")`. It guards the file against typos (such as 96 for 9600) before a port is opened. Pure.

### `fn baud_setting(baud: Option<BaudSection>, baud_probe: Option<Vec<u32>>) -> Result<BaudSetting, String>` [@ANCHOR: radioctl:radioctld_config_baud_setting]

Turns a serial rig's `baud` and `baud_probe` keys into a `BaudSetting`. `baud` is optional and is a number (a fixed rate) or the word `auto`; `baud_probe` is an optional list of rates, tried in order.
A fixed rate alone gives `Fixed(rate)` and probes nothing. `baud = "auto"` gives `Probe(DEFAULT_RATES)`. A `baud_probe` list (with `baud` absent or `auto`) gives `Probe(list)`. Errors (`Err(String)`): a fixed rate together with a list, no baud setting at all, an empty or repeating list, a word other than `auto`, or a rate out of range (`baud_rate`). Pure; no port is touched here.

### `fn rig_config(section: RigSection, index: usize, base_dir: &Path, non_loopback_ok: bool, has_secrets_dir: bool) -> Result<RigConfig, String>` [@ANCHOR: radioctl:radioctld_config_rig]

Validates one `[[rig]]` entry into a `RigConfig`. `section` is by value; `index` is its position in the file and picks the default listen port; `base_dir` resolves a relative description path; `non_loopback_ok` is the server's explicit permission to listen beyond this machine; `has_secrets_dir` is passed to the transport check.
It checks the name (`check_name`), the listen address (`listen_address`; default loopback at the base port plus the index), the receive range (`rx_low_hz` default 30,000 and `rx_high_hz` default 60,000,000, low below high and high at most the maximum frequency) and the transport. It carries `vfo_mode`, `transmit` (whether the transmit gate may be opened), `hamlib_model` (default 1) and the optional `max_transmit_s`, 1 to 3600. Errors are `Err(String)` sentences naming the rig. Pure, no I/O.

### `fn parse(text: &str, base_dir: &Path) -> Result<Config, String>` [@ANCHOR: radioctl:radioctld_config_parse]

Parses and validates the text of a daemon configuration file (TOML) into a `Config`. `text` is the file contents; `base_dir` is the directory of the file, from which relative description paths are taken. It does not read the file; the caller does.
At least one `[[rig]]` or `[[rotator]]` is required. It checks the `[server]` section (`server_config`), each rig and each rotator, then enforces uniqueness: rig names are unique among rigs, rotator names among rotators, and every listen address (with a nonzero port) among rigs and rotators together. Errors are `Err(String)` beginning with `config:` or the name of the failing item. Pure, no I/O, no network.


## `daemon.rs`

### `async fn start(handle: &Handle, config: &Config, descriptions: Vec<RigDescription>, options: impl Fn() -> RigOptions) -> Result<Daemon, String>` [@ANCHOR: radioctl:radioctld_daemon_start]

Brings up the whole daemon from a validated `Config`. `handle` is the tokio runtime handle; `config` is borrowed; `descriptions` are the rig descriptions in the same order as `config.rigs`, taken by value; `options` is a factory that makes the `RigOptions` for each rig.
Order: for each rig `start_one` (builds the rig and binds its listener), then each rotator `start_rotator`, sharing one allow list and one set of connection limits from `config.server`. If anything fails, what was already started is stopped (`Daemon::stop`, which unkeys rigs) and the `Err(String)` is returned, so the daemon is never left half up. On success it returns the `Daemon` with the served rigs and rotators. Async; opens ports and serial links.

### `async fn start_one(handle: &Handle, config: &Config, rig: &crate::config::RigConfig, description: RigDescription, options: &impl Fn() -> RigOptions, access: &Arc<crate::access::AllowList>, limits: Limits) -> Result<Served, String>` [@ANCHOR: radioctl:radioctld_daemon_start_one]

Brings up one rig and its TCP listener. `handle` is the runtime; `config` the whole config; `rig` its entry; `description` the loaded description by value; `options` makes the `RigOptions`; `access` is the shared allow list; `limits` the connection limits. Async.
It builds the rig with `rigs::build_rig` (opening the link the transport names; whether it also sends a start-up unkey is decided there, `unknown`: check `rigs.rs`), binds a `TcpListener` on the rig's listen address and starts serving with the rigctld line service. Errors (`Err(String)`): the build error, or `rig <name>: cannot listen on <addr>`, or the server start error. The returned `Served` keeps the service, server and the links for later stop and status. The transmit gate starts closed.

### `async fn stop(self)` [@ANCHOR: radioctl:radioctld_daemon_stop]

Shuts the daemon down in an orderly way. `self` is consumed. For each rig it shuts down the listener and every client connection (`server.shutdown`), then the service (`service.shutdown`), which releases the transmitter and unkeys any rig whose transmitter a client held. For each rotator it shuts down the listener and, for a virtual rotator, stops its own socket task so the loopback port closes.
A rotator is left where it is, with its own target, as if a `rotctld` had stopped. Async; it reports no errors and always runs to the end. This is what runs on SIGINT, SIGTERM and SIGHUP.


## `config.rs`

### `fn check_name(kind: &str, name: &str) -> Result<(), String>` [@ANCHOR: radioctl:radioctld_config_check_name]

Checks the name of a rig or rotator. `kind` (`rig` or `rotator`) is used in the message; `name` must be 1 to 32 characters from letters, digits, `-`, `_` and `.`.
Anything else, including an empty name or a name with a space or a slash, returns `Err("<kind> name <name> must be 1-32 of [A-Za-z0-9_.-]")`. The name appears in logs and ports lists, so the restricted set keeps those safe. Pure.

### `fn listen_address(kind: &str, name: &str, given: Option<&String>, base: u16, index: usize, non_loopback_ok: bool) -> Result<SocketAddr, String>` [@ANCHOR: radioctl:radioctld_config_listen_address]

Works out the listen address of a rig or rotator. `kind` and `name` are for messages; `given` is the optional `listen` text, which must parse as `ip:port`; `base` is the base port for the kind and `index` the entry position; `non_loopback_ok` is the server's explicit permission to listen beyond this machine.
With no `listen` the address is `127.0.0.1` at `base + index` (an index that overflows the port range is an error, `too many ...s for the default ports`). A non-loopback address without permission returns an `Err` telling the owner to set `server.allow_non_loopback_bind = true` with an explicit `server.allow` list. Pure; nothing is bound here.

### `fn rotator_config(section: RotatorSection, index: usize, non_loopback_ok: bool) -> Result<RotatorConfig, String>` [@ANCHOR: radioctl:radioctld_config_rotator]

Validates one `[[rotator]]` entry into a `RotatorConfig`. `section` is by value; `index` is its position; `non_loopback_ok` as for rigs.
It checks the name, the listen address (default loopback at port 4533 plus the index), and the model, which must be one of the built-in rotator descriptions (the error lists the known ids), then the transport (virtual, tcp or serial; the rest of that match is in the source). Errors are `Err(String)` naming the rotator. Pure, no I/O. A rotator has no transmit path.

### `fn check_host_port(address: &str) -> Result<(), String>` [@ANCHOR: radioctl:radioctld_config_check_host_port]

Checks that a TCP address is `host:port`. `address` is borrowed. The text after the last `:` must be a number that fits `u16` and the host before it must not be empty.
No name lookup is done, so an unresolvable host name passes here and fails when the link is opened. A bad address returns `Err("tcp address ... is not host:port")`. Pure.


## `daemon.rs`

### `async fn start_rotator(handle: &Handle, rotator: &RotatorConfig, access: &Arc<crate::access::AllowList>, limits: Limits) -> Result<ServedRotator, String>` [@ANCHOR: radioctl:radioctld_daemon_start_rotator]

Brings up one rotator and its TCP listener. `handle` is the runtime; `rotator` its validated config; `access` the shared allow list; `limits` the connection limits. Async.
It builds the rotator (`rotators::build_rotator`: engine, link, handle, service), binds the listen address and serves the rotctld text protocol on it. Errors (`Err(String)`): the build error, `rotator <name>: cannot listen on <addr>` or the server start error. The result keeps the service, the server and, for a virtual rotator, its control handle and links. Rotators never key a transmitter.


## `rotators.rs`

### `async fn build_rotator(runtime: &Handle, rotator: &RotatorConfig) -> Result<BuiltRotator, String>` [@ANCHOR: radioctl:radioctld_rotators_build_rotator]

Builds one rotator: engine, link, handle and service. `runtime` is the tokio handle; `rotator` is the validated config. A virtual rotator is served on a loopback socket of its own, with a port chosen by the kernel, and reached over the same reconnecting link a real one uses, so the same code path runs. Tcp and serial rotators open their link as configured.
The first connection is one attempt: a rotator that cannot be reached stops the daemon from starting, as a rig does. Errors are `Err("rotator <name>: ...")` from the engine, the bind or the link open. Async; opens sockets or ports; does not move the rotator. The remainder of the serial case is in the source.


## `rotctl.rs`

### `fn resolve(word: &str, long_form: bool) -> Option<(&'static str, bool)>` [@ANCHOR: radioctl:radioctld_rotctl_resolve]

Finds the long name of a rotctld command word and whether the daemon serves it. `word` is the command word and `long_form` says whether it came after a `\`. Returns `Some((name, served))`, or `None` for a word Hamlib does not know.
`w` and `send_cmd` are recognised but never served (a raw string to the rotator is not offered), so they return `("send_cmd", false)`. Otherwise the word is looked up in the `NAMES` table, by long name or by its short letter. Pure.

### `fn degrees(token: &str) -> Result<f32, i32>` [@ANCHOR: radioctl:radioctld_rotctl_degrees]

Parses an angle argument. `token` is the text, for example `123.5`. Returns `Ok(f32)` only for a finite number; text that does not parse, `NaN` and infinity return `Err(EINVAL)`, which is the Hamlib code -1.
It does not check the travel limits; the rotator handle refuses a position outside them. Pure.

### `fn parse_rot_line(line: &str) -> Option<RotParsed>` [@ANCHOR: radioctl:radioctld_rotctl_parse_line]

Parses one rotctld request line. `line` is trimmed; a blank line returns `None` (nothing to answer). A leading `+` asks for the extended response format. A `\` starts a long command name, otherwise the first character is the short command; the rest is the argument text, with its spacing normalised.
An unknown word gives `RotParsed` with `command = Err(EINVAL)`; a known but not served word gives an error code (see the source for the code used); a served one is turned into a `RotCommand` with its arguments (angles via `degrees`). Pure, executes nothing.

### `fn rot_error_code(error: &StationError, command: RotCommand) -> i32` [@ANCHOR: radioctl:radioctld_rotctl_error_code]

Chooses the `RPRT` error code to send for a rotator failure. `error` is the station error; `command` is the command that failed. A refused position is `-1` (EINVAL), as Hamlib's `rot_set_position` answers one outside the limits; a refused park (the rotator has no park position) is `-11` (ENAVAIL).
Other kinds: timeout gives `-5`, a config error `-2`, a driver, store or keyed-unknown error `-6` (EIO), a not-confirmed move `-8` (EPROTO) and an unknown error `-11`. Total, pure, no panics.

### `fn new(rotator: Rotator) -> RotatorService` [@ANCHOR: radioctl:radioctld_rotctl_service_new]

Creates the rotctld text service for one rotator. `rotator` is the `Rotator` handle, taken by value and owned by the service. It cannot fail and starts no task by itself.
One service is shared by every client of the rotator's listener. It holds no per-client state.

### `fn rotator(&self) -> &Rotator` [@ANCHOR: radioctl:radioctld_rotctl_service_rotator]

Gives access to the service's rotator handle. `&self`; the result is a shared reference to the `Rotator`, valid as long as the service. No locking or cost.
It lets code that owns the service reach the handle (for example to read its description) without going through the text protocol. Cannot fail.

### `fn dump_state(&self) -> String` [@ANCHOR: radioctl:radioctld_rotctl_dump_state]

Builds the `\dump_state` text for the rotator, in the shape Hamlib 4.6.2's `rotctld` prints and that `rotctl -m 2` reads. `&self`; read-only.
Content: protocol version 1, the rotator model number 1, then `min_az`, `max_az`, `min_el`, `max_el` (degrees, six decimals) from the description, `south_zero=0`, `rot_type` (`AzEl` if the rotator has elevation, else `Az`) and `done`. Pure, no I/O.

### `async fn execute(&self, command: RotCommand) -> Result<Body, i32>` [@ANCHOR: radioctl:radioctld_rotctl_service_execute]

Executes one parsed rotctld command on the rotator. `&self`; `command` is the typed `RotCommand`. Async. Every wait is bounded by the description's reply timeout inside the rotator handle, so a stuck rotator cannot hang the client forever.
Set position moves the rotator to an azimuth and elevation, get position returns both with two decimals, stop halts the motion, park goes to the park position, get info returns the model name, dump state returns the text above and quit closes the connection. Returns `Ok(Body)` or `Err(code)` where the code comes from `rot_error_code`. Moves the antenna; no radio is keyed.

### `async fn rot_answer(service: &RotatorService, line: &Line) -> (String, bool)` [@ANCHOR: radioctl:radioctld_rotctl_answer]

Produces the text to send for one request line of the rotctld protocol, and whether the connection should close afterwards. `service` is the rotator service; `line` is the received line, valid text or invalid. Async.
An invalid line (bad UTF-8 or control characters) answers `RPRT -1`. A blank line gives an empty reply. Otherwise the line is parsed, executed (`execute`) and the outcome rendered in the plain or extended format. The connection closes only after a `quit`. Never panics on bad input.

### `fn answer<'a>(&'a self, _client: u64, line: &'a Line) -> BoxFuture<'a, (String, bool)>` [@ANCHOR: radioctl:radioctld_rotctl_service_answer]

The server's line-service hook for the rotator: it calls `rot_answer` for each received line. `&self`; `_client` is the client number, ignored because a rotator has no per-client state; `line` is the line.
Returns a boxed future of `(reply text, close the connection)`. No errors of its own.

### `fn disconnect(&self, _client: u64) -> BoxFuture<'_, ()>` [@ANCHOR: radioctl:radioctld_rotctl_service_disconnect]

The server's hook for a client going away. `&self`; `_client` is the client number. It does nothing, because a rotator service holds nothing per client: a rotator has no owner and no transmit path, so there is nothing to release when a client leaves.
The rotator keeps its current target, as it would if a `rotctld` client disconnected. Returns a ready future; cannot fail.


## `server.rs`

### `fn answer<'a>(&'a self, client: u64, line: &'a Line) -> BoxFuture<'a, (String, bool)>` [@ANCHOR: radioctl:radioctld_server_rig_answer]

The server's line-service hook for a rig: it calls the rig `answer` function for each received line. `&self`; `client` is the client number, which tracks who holds the transmitter; `line` is the received line.
Returns a boxed future of `(reply text, close the connection)`. The command is parsed and executed in `answer`, where the transmit gate, band check and lock apply to any command that keys the radio. No errors of its own.

### `fn disconnect(&self, client: u64) -> BoxFuture<'_, ()>` [@ANCHOR: radioctl:radioctld_server_rig_disconnect]

The server's hook for a client going away. `&self`; `client` is the client number. It calls the rig service's own `disconnect`, which releases the transmitter if that client held it, unkeying the rig.
So a client that keys the radio and then disconnects, or crashes, does not leave the radio transmitting. Unkeying needs no gate. Returns a future that completes when the release is done; cannot fail.


## `main.rs`

### `fn parse_args(args: &[String]) -> Result<Cli, String>` [@ANCHOR: radioctl:radioctld_main_parse_args]

Parses the command line of the `radioctld` program, without the program name. `args` is borrowed. Options: `--config FILE` or `-c FILE` (the configuration file, given at most once), `--check` (validate the configuration and exit) and `--help` or `-h`.
Errors (`Err(String)`): `--config` without a value, `--config` twice, or an unknown argument. With `--help` it returns early without requiring a config. What happens when no `--config` is given and no help was asked is in the source beyond this entry (`unknown`: check it). Pure.

### `fn load(path: &Path) -> Result<(Config, Vec<radioctl_core::description::RigDescription>), String>` [@ANCHOR: radioctl:radioctld_main_load]

Reads the configuration file and every rig description it names. `path` is the config file. It reads the file (an unreadable file gives `cannot read <path>: <error>`), parses it with `Config::parse` using the file's directory as base, then loads each rig's description file.
Returns the `Config` and the descriptions in rig order, or the first `Err(String)`. Blocking file reads; done before the runtime starts. No network.

### `fn run(cli: &Cli) -> Result<(), String>` [@ANCHOR: radioctl:radioctld_main_run]

Runs the daemon until a stop signal arrives, then stops it. `cli` is the parsed command line. It loads the configuration and descriptions (`load`); with `--check` it prints `configuration ok` with the rig and rotator counts and returns without starting anything.
Otherwise it builds a tokio runtime with 2 worker threads named `radioctld`, starts the daemon (`daemon::start`), prints each listening address, waits for `stop_signal` and then runs `Daemon::stop`, which unkeys every rig. Errors from load, runtime creation or start are returned as `Err(String)`. Blocks the calling thread until stopped.

### `async fn stop_signal() -> Result<&'static str, String>` [@ANCHOR: radioctl:radioctld_main_stop_signal]

Waits for a signal that asks the daemon to stop and returns its name: `interrupt` for SIGINT (ctrl-c), and on Unix also `terminate` for SIGTERM (`kill`, `systemctl stop`) and `hangup` for SIGHUP (the terminal or SSH session went away). Async.
Each leads to the orderly stop, which unkeys every rig. Only SIGINT used to be caught, so a SIGTERM or SIGHUP ended the process at once and left a keyed radio keyed (found on a KX3, 2026-11-05). Errors (`Err(String)`) if a signal handler cannot be installed. On other platforms only ctrl-c is waited for. SIGKILL cannot be caught.

### `fn main() -> ExitCode` [@ANCHOR: radioctl:radioctld_main]

The entry point of the `radioctld` program. It takes the process arguments, parses them (`parse_args`) and, on a bad argument, prints the error and the usage text and exits with code 2. With `--help` it prints the usage and exits 0.
Otherwise it runs the daemon (`run`): exit code 0 after an orderly stop or a successful `--check`, and 1 after printing `radioctld: <error>` if loading or starting failed. It runs until a stop signal; the stop unkeys every rig. No arguments, returns an `ExitCode`.


## `names.rs`

### `fn mode_from_hamlib(name: &str) -> Option<Mode>` [@ANCHOR: radioctl:radioctld_names_mode_from_hamlib]

Finds the radioctl mode that a Hamlib mode name stands for. `name` is borrowed and compared without regard to case against the `MODES` table, so `USB`, `usb` and `Usb` are the same.
Returns `Some(Mode)` or `None` for a name the table does not have, which callers turn into an invalid-argument error. Pure.

### `fn mode_to_hamlib(mode: Mode) -> &'static str` [@ANCHOR: radioctl:radioctld_names_mode_to_hamlib]

Gives Hamlib's name for a radioctl mode, such as `USB`. `mode` is by value. A mode that is not in the `MODES` table gives `NONE`.
It is the inverse of `mode_from_hamlib` for the modes in the table and is used for the mode text in replies, `\dump_caps` and the `\dump_state` per-mode lines. Pure, returns a `'static` string, no errors.

### `fn mode_mask(modes: &[Mode]) -> u64` [@ANCHOR: radioctl:radioctld_names_mode_mask]

Builds the bit mask of a set of modes as `\dump_state` writes it. `modes` is borrowed. Each mode found in the `MODES` table contributes `1 << bit` of its table entry; a mode not in the table is skipped.
The result is a `u64` used for the range lines and the all-modes lines of `\dump_state`. An empty slice gives 0. Pure, no errors.
