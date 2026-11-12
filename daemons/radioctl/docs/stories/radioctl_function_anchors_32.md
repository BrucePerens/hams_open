# radioctl function anchors, batch 32

Function anchors for the `radioctld` crate's volunteer self-test program (`radioctl-selftest`): candidate listing (`candidates.rs`), embedded description sources (`builtin.rs`), radio discovery (`discovery.rs`), the command-line driver (`selftest_cli.rs`) and the report upload to hams.com (`upload.rs`). Each entry follows ADR 0113 and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
None of this code can key a transmitter or open the transmit gate: discovery and checks use read-only operations only (identify, frequency reads), the rig is opened with default options (gate closed), and the network code only listens for broadcasts or POSTs a report.

## `crates/radioctld/src/candidates.rs`

### `pub fn candidates_for_product<'a>(product: &str, rigs: &'a [RigSummary]) -> Vec<&'a RigSummary>` [@ANCHOR: radioctl:radioctld_candidates_for_product]

Lists which rig descriptions a USB device's own product string names. For each summary it takes the display name without its first word (the maker, so `Icom IC-7300` gives `IC-7300`), squashes both that model and the product string with `squash` (letters and digits only, lower case) and keeps the description when the product string contains the model. A model shorter than four characters never matches, to avoid false hits. Returns references into `rigs` in their order; empty means no evidence, one means a unique name, several means ambiguous. Pure, no errors. The USB ids are deliberately not used because they identify only the bridge chip.

### `pub fn render_listing(ports: &[PortInfo], rigs: &[RigSummary]) -> String` [@ANCHOR: radioctl:radioctld_candidates_render]

Builds the text that `radioctl-selftest --list` prints. For each port it shows the device path, the USB ids (hexadecimal), the chip class, the product string and the `usb:VID:PID` target to pass to the program; then one of three lines: nothing is named (choose from the list), exactly one description is named (check it is your radio), or AMBIGUOUS with the candidate ids. After the ports comes a table of every description (id, name, family, highest verified state) and a note that every state other than hardware-verified is unconfirmed. With no ports it says so. It never picks one rig unless the device's own text names exactly one. Pure; returns a new string.

## `crates/radioctld/src/builtin.rs`

### `pub fn embedded_texts() -> Vec<(String, String)>` [@ANCHOR: radioctl:radioctld_builtin_embedded_texts]

Returns every rig description file that `build.rs` compiled into the program, as owned `(label, text)` pairs, where the label is `embedded:` followed by the file stem (such as `embedded:icom-ic-7300`) and the text is the whole TOML. The order is that of the embedded table, sorted by file stem. No arguments, no I/O and no error: the data is a constant, so the program works with no side files. Callers parse the text with the rig-file parser, and the label is used in parse errors.

### `pub fn choose_source(flag: Option<&Path>, env: Option<&OsStr>) -> RigSource` [@ANCHOR: radioctl:radioctld_builtin_choose_source]

Decides where the descriptions come from. `flag` is the `--rigs-dir` value and `env` is the value of the `RADIOCTL_RIGS` environment variable, both optional and borrowed. The flag always wins and gives `RigSource::Dir`; otherwise a non-empty environment value gives a `Dir`; otherwise the embedded set. An empty environment value counts as unset. It does not check that the directory exists (that error appears later in `load_texts`) and does not guess `./rigs` or a source checkout, because a shipped program has none. Pure.

### `pub fn load_texts(source: &RigSource) -> Result<Vec<(String, String)>, String>` [@ANCHOR: radioctl:radioctld_builtin_load_texts]

Returns every description of the chosen source as `(label, text)` pairs. For `Embedded` it returns `embedded_texts()` and cannot fail. For a directory it lists the entries (not recursive) and reads each file whose extension is `toml`, using the full path as the label; the order is that of the directory listing, which is not sorted, so callers that need order must sort (the summaries code sorts by id). Errors as strings: the directory cannot be listed, or a `.toml` file cannot be read, both naming the path. Entries that cannot be listed are skipped silently. Blocking file I/O; read-only.

## `crates/radioctld/src/discovery.rs`

### `pub fn not_radio_reason(port: &PortInfo) -> Option<String>` [@ANCHOR: radioctl:radioctld_discovery_not_radio_reason]

Spots serial ports that are clearly not a radio so discovery skips them without opening them. It uses only the device's own ids and text: a USB vendor id in the board list (Arduino 2341, u-blox 1546, 1366, STMicro 0483, 03eb) gives `USB vendor xxxx makes boards, not radios`, and a squashed product string containing one of `gps`, `gnss`, `arduino`, `programmer`, `jtag`, `bootloader`, `ublox`, `pickit` or `stlink` gives `the product string says <word>`. Returns `None` when neither applies, including when there is no product string. The port is borrowed; pure; no I/O.

### `pub fn expected_id(description: &RigDescription) -> Option<u16>` [@ANCHOR: radioctl:radioctld_discovery_expected_id]

Finds the model id a radio should answer to an identify read, taken from whichever protocol block the description has: Icom CI-V (the 8-bit `model_id` widened to 16 bits), Kenwood, Elecraft or Yaesu, checked in that order. Returns `None` for a family with no stated id (so the radio cannot be asked), and the first block present wins. The description is borrowed read-only; pure, no errors. `ask_radio` compares the id the radio answers with this value.

### `pub async fn ask_radio(description: &RigDescription, path: &Path, unmanaged: bool) -> bool` [@ANCHOR: radioctl:radioctld_discovery_ask_radio]

Asks the radio on the serial port at `path` whether it is the one `description` describes, using read-only commands only. It returns false at once if the description has no expected id, lacks the Identify capability or has no engine. Otherwise it runs the baud probe (a frequency read at each default rate), writes a throw-away wire-log file in the temp directory, opens the rig through the self-test `open` (gate closed) at the found rate and sends `GetModelId`. True only when the answered id equals the expected one; a failed probe, open, timeout or different id gives false, never an error. `unmanaged` leaves the DTR and RTS lines alone. The temp file is deleted afterwards. Async; opens a real serial port; nothing is transmitted.

### `pub async fn discover_ports(ports: &[PortInfo], descriptions: &[RigDescription], rigs: &[RigSummary], unmanaged: bool) -> Vec<Finding>` [@ANCHOR: radioctl:radioctld_discovery_discover_ports]

Classifies every serial port, in order, into one `Finding` with an `Outcome`. A port that `not_radio_reason` rejects is `NotRadio` and is never opened. If the product string names no description it is `Unidentified` (not opened); one description gives `Identified` by product string. Several candidates are asked in turn with `ask_radio` (each opens the port and sends an identify read): exactly one yes gives `Identified` (answered its identify read), none gives `Ambiguous` listing all candidates, several yes gives `Ambiguous` with the answering ones. The slices are borrowed; `descriptions` supply the engines and `rigs` the names. Sequential, so slow for many candidates; there are no errors. Read-only, no transmit.

### `pub fn net_from_packet(packet: &[u8], rigs: &[RigSummary]) -> Option<NetFinding>` [@ANCHOR: radioctl:radioctld_discovery_net_from_packet]

Turns one FlexRadio discovery datagram into a network finding. The packet bytes are parsed by the Flex protocol's `parse_discovery`; a packet that is not a valid discovery message returns `None`. A valid one gives the address as `ip:port`, the model the radio announced, and the description id when `candidates_for_product` finds exactly one match for the model, otherwise `None` for the description. Borrowed inputs; pure, no I/O, no panic on garbage bytes.

### `pub async fn listen_flex(window: Duration, rigs: &[RigSummary]) -> Vec<NetFinding>` [@ANCHOR: radioctl:radioctld_discovery_listen_flex]

Listens on UDP port `DISCOVERY_PORT` (all interfaces) for FlexRadio discovery broadcasts for the length of `window`, and returns each distinct radio once (de-duplicated by address), in arrival order. Receive only: nothing is ever sent. If the port cannot be bound (already in use, no network) it returns an empty list rather than an error, and a receive error ends the wait early. Packets that do not parse are ignored. Async; uses a 2048-byte receive buffer; the call always takes the full window unless the socket fails.

### `pub fn render_table(findings: &[Finding], net: &[NetFinding]) -> String` [@ANCHOR: radioctl:radioctld_discovery_render_table]

Formats the discovery result for the screen and the run summary: a `Discovery:` heading, one line per serial port (path, USB ids or `no usb ids`, product string, and the verdict: skipped as not a radio with the reason, unidentified with the hint to use `--port` and `--description`, the chosen id and how it was decided, or ambiguous with the candidates), then one line per network radio (listed only, since network rigs are not self-tested yet). When both lists are empty it says `nothing found`. Pure; returns a new string. No serial numbers are printed.

### `pub fn identified(findings: &[Finding]) -> Vec<(String, PathBuf, PortInfo)>` [@ANCHOR: radioctl:radioctld_discovery_identified]

Picks the findings that `discover_ports` settled and returns, for each, the description id, the serial path and a copy of the port information, in the same order as the findings. `NotRadio`, `Unidentified` and `Ambiguous` outcomes are dropped. The result is the list of rigs the self-test will actually test when the user named nothing. Borrowed input, owned output, pure, no errors. An empty vector means no radio was identified.

## `crates/radioctld/src/selftest_cli.rs`

### `pub fn parse_args(args: &[String]) -> Result<Options, String>` [@ANCHOR: radioctl:radioctld_cli_parse_args]

Parses the command line of `radioctl-selftest` (without the program name) into an `Options` record. Flags: `--list`, `--version`, `--no-discover`, `--unmanaged`, `--no-upload`, `--update`, `--no-update-check`, `--all` (accepted and ignored), and valued ones `--rigs-dir`, `--description` (or `--rig`), `--port`, `--baud`, `--out`, `--out-dir`, `--name`, `--email`, `--server`. The default baud is 19200. Two positionals `ID PORT [BAUD]` are the old spelling of description, port and baud. Errors (strings with the usage text): a flag missing its value, an unknown `--` option, a baud rate that `parse_baud` rejects, or a wrong count of positionals. Pure; it does not check that files or ports exist.

### `pub fn parse_baud(text: &str) -> Result<BaudArg, String>` [@ANCHOR: radioctl:radioctld_cli_parse_baud]

Reads the `--baud` value. The word `auto` (exact, lower case) gives `BaudArg::Auto`, meaning try the usual rates with the description's own frequency read. A decimal integer from 300 to 4,000,000 inclusive gives `BaudArg::Fixed(rate)`. Anything else (words, negative or fractional numbers, 299, 4,000,001) returns the error `<text>: the baud rate is a number or auto`. Pure; it does not check that the radio supports the rate.

### `pub fn find_description<'a>(loaded: &'a [LoadedDescription], id: &str) -> Result<&'a LoadedDescription, String>` [@ANCHOR: radioctl:radioctld_cli_find_description]

Looks up the loaded description whose id equals `id` (exact, case-sensitive) and returns a reference into the slice. If there is none the error is `no description <id>; there are: ` followed by all available ids comma-separated, so a volunteer who mistyped sees the choices. Borrowed input; pure; the first match wins (ids are expected to be unique).

### `pub fn port_identity(target: &str, ports: &[PortInfo]) -> Option<PortInfo>` [@ANCHOR: radioctl:radioctld_cli_port_identity]

Finds the USB identity of the port the user named, so it can go in the report without the serial number. `target` is either `usb:VID:PID` (hexadecimal, anything after is ignored) matched against each port's ids, or a device path matched against each port's path. Returns a copy of the first matching `PortInfo` with the `serial` field cleared, or `None` for no match or for `virtual`. A VID or PID that does not parse as hexadecimal matches only ports with no such id. Pure.

### `pub fn describe_identity(port: &PortInfo) -> String` [@ANCHOR: radioctl:radioctld_cli_describe_identity]

Formats the one-line port identity for the report header: `usb 10c4:ea60, product "..." (serial number not recorded)`, with ids as four lowercase hexadecimal digits and the product in Rust debug quotes (`(none)` if absent). Returns an empty string when the port lacks USB ids (a plain serial device), which makes the report skip the line. The serial number is never included. Pure; the port is borrowed.

### `pub fn report_path(o: &Options, id: &str, index: usize, total: usize) -> PathBuf` [@ANCHOR: radioctl:radioctld_cli_report_path]

Chooses the file where one rig's local report is written. When exactly one rig is tested (`total == 1`) and `--out` was given, that path is used. Otherwise the name is `radioctl-selftest-<id>.txt` for the first rig with that id and `radioctl-selftest-<id>-<index+1>.txt` for later ones with the same id (`index` counts earlier tests of this id, from 0), placed in `--out-dir`, or in the current directory when none was given. Pure; it neither creates directories nor checks for existing files, so an existing report is overwritten later.

### `pub fn render_summary(table: &str, tested: &[(String, PathBuf, usize, usize, String)]) -> String` [@ANCHOR: radioctl:radioctld_cli_render_summary]

Builds the run summary text written to `radioctl-selftest-summary.txt`. It starts with a title, then the discovery table `table` as given, then one line per tested rig from the tuples (id, report path, pass count, did-not-pass count, upload status): `ID: N pass, M did not pass; report PATH; upload: STATUS`. When the list is empty it ends with `no rig was tested`. All inputs are borrowed and nothing is validated; pure, no I/O.

### `pub fn choose_targets(o: &Options, ports: &[PortInfo], findings: &[Finding]) -> Result<Vec<(String, String, Option<PortInfo>)>, String>` [@ANCHOR: radioctl:radioctld_cli_choose_targets]

Decides which rigs to test, returning (description id, port target, port identity) triples. With both `--description` and `--port` it returns that one rig (identity from `port_identity`). With only a description it uses the sole serial port, or fails with `--description <id> needs --port: N ports found` if there is not exactly one. With only a port it needs the discovery finding for that path to be `Identified`, otherwise the error says the device does not name one description and to use `--description`. With neither it returns everything `identified` found (possibly nothing). Pure; no hardware access.

### `pub async fn run(o: Options, mut host: Host, input: &mut dyn BufRead, out: &mut dyn Write) -> i32` [@ANCHOR: radioctl:radioctld_cli_run]

Runs the whole self-test and returns the process exit status: 0 when every check of every rig passed or was not applicable, 1 when a check failed or a rig could not be opened, 2 for a usage or description-loading error (including `--no-discover` without a description and port). `o` is the parsed options and `host` supplies the machine facts (ports, interactivity, contact file, update channel) so tests need no hardware; it reads answers from `input` and writes everything to `out`. Steps: version print, load descriptions, `--list`, optional update, discovery and optional LAN listen, choosing targets, then for each rig the read-only checks, local report, optional contact prompt and upload. A failed upload only prints a line and never changes the status. It never transmits or opens the transmit gate; the rig is opened gate-closed and only read-only checks are planned. Async, one rig at a time; writes local report files and the run summary.

## `crates/radioctld/src/upload.rs`

### `pub fn uuid_v4(mut bytes: [u8; 16]) -> String` [@ANCHOR: radioctl:radioctld_upload_uuid_v4]

Formats 16 bytes as a version 4 UUID in the usual 8-4-4-4-12 lowercase hexadecimal layout. It sets the version nibble (the high four bits of byte 6 become 4) and the variant bits (the two high bits of byte 8 become 10), so 6 of the 128 bits are not random. The array is taken by value and modified locally. Pure and deterministic for a given input, and cannot fail; the randomness comes from the caller, `new_report_id`.

### `pub fn new_report_id() -> Result<String, String>` [@ANCHOR: radioctl:radioctld_upload_new_report_id]

Makes a fresh random report id for one upload by filling 16 bytes from the operating system's random source (`getrandom`) and formatting them with `uuid_v4`. The id lets the server tell reports apart and carries no information about the volunteer. The only error is the system having no randomness, returned as `no random numbers: <cause>`; `run` then falls back to an empty id. Not idempotent: every call differs. No other side effects.

### `pub fn rfc3339(unix_seconds: u64) -> String` [@ANCHOR: radioctl:radioctld_upload_rfc3339]

Formats a Unix time (whole seconds, UTC) as `YYYY-MM-DDTHH:MM:SSZ`, the timestamp form used in the upload JSON `sent_at` field. It reuses `utc_stamp` from the self-test module and rewrites its `YYYY-MM-DD HH:MM:SS UTC` text, replacing the space with `T` and the ` UTC` suffix with `Z`. No fractional seconds, no time-zone offset. Pure and total for any `u64`.

### `pub fn build_payload(input: &PayloadInput<'_>) -> Vec<u8>` [@ANCHOR: radioctl:radioctld_upload_build_payload]

Builds the JSON document that is POSTed to hams.com, as UTF-8 bytes. Fields: `schema_version` 1, `report_id`, `sent_at` (RFC 3339), radioctl version, description id, rig maker and model (the display name split at its first space) and the firmware text if the firmware check passed (capped at 80 characters), OS and architecture, link (transport, baud, USB ids when known), the optional baud-probe record (tried rates and chosen), the contact name (120 characters) and email (254) when given, and one entry per check: name (80), ok, summary (300) and raw hex of the wire (2048 characters). The input has no field for paths, serial numbers or host names. The body is kept to at most `MAX_BODY` (256 KiB) by dropping raw hex from the last check backwards, then blanking summaries, and if serialisation fails the result is empty. Pure.

### `pub fn config_dir(var: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf>` [@ANCHOR: radioctl:radioctld_upload_config_dir]

Finds the per-user configuration directory for radioctl: on Windows `%APPDATA%`, on macOS `$HOME/Library/Application Support`, elsewhere `$XDG_CONFIG_HOME` (if non-empty) or `$HOME/.config`, with `radioctl` appended. `var` looks up an environment variable by name, injected so tests do not touch the real environment. Returns `None` when the needed variable is missing (for example no HOME), in which case no contact answers are saved. Pure; it does not create the directory.

### `pub fn load_saved(path: &Path) -> Option<SavedContact>` [@ANCHOR: radioctl:radioctld_upload_load_saved]

Reads the contact answers saved by an earlier run. The file is JSON with optional string keys `name` and `email`. Returns `Some` with whatever fields are present (either may be absent, meaning the volunteer pressed Enter) when the file exists and parses, and `None` when it cannot be read or is not valid JSON, which tells the caller that the volunteer has not been asked yet and may be asked. A key with a non-string value counts as absent. Blocking file read; no other effect, no panics.

### `pub fn save(path: &Path, saved: &SavedContact) -> Result<(), String>` [@ANCHOR: radioctl:radioctld_upload_save]

Writes the contact answers as JSON (`name` and `email`, null when absent) to `path`, creating the parent directories first, and replaces any existing file. On Unix the file is created with mode 0600 so only the owner can read it; the mode applies when the file is newly created, an existing file keeps its mode (check `unknown` for other platforms). Errors are strings naming the path: cannot create the directory, cannot open the file or cannot write it. The caller (`run`) prints `could not save your answers` and carries on. Blocking I/O.

### `pub fn valid_email(text: &str) -> bool` [@ANCHOR: radioctl:radioctld_upload_valid_email]

A deliberately plain shape check that an answer looks like an email address before it is stored and sent; it does not verify that the address exists. True only if there is an `@` with a non-empty part before it, the part after it contains a dot and neither starts nor ends with a dot, the whole text is at most 254 bytes and contains no whitespace. Only the first `@` splits the text, so a second `@` stays in the domain part and is accepted. Pure, no errors.

### `pub fn prompt_contact(input: &mut dyn BufRead, output: &mut dyn Write) -> SavedContact` [@ANCHOR: radioctl:radioctld_upload_prompt_contact]

Asks the volunteer, once, for a name and then an email address, with a sentence saying the reason (so hams.com can contact them about their rig; sent with the report). Each prompt is written to `output` and flushed; one line is read from `input` and trimmed. An empty line, or end of input, skips that item (`None`). An email that fails `valid_email` is dropped silently. Returns the answers as a `SavedContact`. No error value: write and read failures are treated as a skip. Blocks on input, so only call it when a person is at the terminal.

### `pub fn resolve_contact(flags: Contact, env: Contact, saved: Option<SavedContact>, interactive: bool, ask: &mut dyn FnMut() -> SavedContact) -> (Contact, Option<SavedContact>)` [@ANCHOR: radioctl:radioctld_upload_resolve_contact]

Settles which contact details are sent. Each field separately takes the command-line flag first, then the environment value, then the saved answers. The volunteer is asked through `ask` only if the run is interactive, nothing was supplied by flag or environment and nothing is saved; those fresh answers become the second return value (`Some`, to be saved) and the first. If answers were saved, the second value is `None`. A non-interactive run never asks and ends with empty contact. Takes ownership of its arguments; no I/O of its own.

### `pub fn parse_server(url: &str) -> Result<ServerUrl, String>` [@ANCHOR: radioctl:radioctld_upload_parse_server]

Splits an upload URL into TLS flag, host, port and path. `https://HOST[:PORT]/PATH` is always accepted (default port 443, path `/` if none); `http://` (default port 80) is accepted only for a loopback host, `localhost`, `127.0.0.1` or `::1` (bracketed form `[::1]` supported), so a report can never travel in the clear to another machine. Errors, all strings naming the URL: not starting with `https://` or `http://`, bad bracketed address, a port that is not a number up to 65535, an empty host or one with whitespace, and plain http to a non-loopback host. Pure; no name lookup.

### `pub async fn post_report(url: &str, body: &[u8]) -> Result<(), String>` [@ANCHOR: radioctl:radioctld_upload_post_report]

POSTs the report body to `url` through `request`, with a 4096-byte response cap and the whole exchange limited to `UPLOAD_TIMEOUT` (10 seconds). Returns `Ok(())` only when the server answers with a status from 200 to 299. Every failure is an `Err` holding one short line: a bad URL from `parse_server`, no connection, a TLS failure, the timeout (`timed out after 10 s`), no HTTP answer, or `the server answered <code>` for any other status. The caller shows the line and keeps the local report, and the program's exit status is unchanged. Async network I/O; the body is borrowed and sent once, so a retry means calling again.

### `pub fn notice(url: &str, local_copy: &Path, contact: &Contact) -> String` [@ANCHOR: radioctl:radioctld_upload_notice]

Writes the disclosure printed before an upload: the destination `url`, a list of what is sent (check results, the bytes exchanged with the radio, the radioctl version, this computer's OS and CPU type, and the contact details), the statement that no host name, user name, file path or USB serial number is sent, and the path of the local copy. The contact wording is `no contact details` when both name and email are absent, otherwise `your name and email as you gave them`. Pure; returns a new string and sends nothing.
