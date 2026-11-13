# radioctl function anchors, batch 34

Function anchors for the `radioctld` stdin reader (`selftest_cli.rs`), rig start-up (`rigs.rs`), TCP framing configuration (`config.rs`) and TCP server (`server.rs`), and for three Python maintenance tools in `daemons/radioctl/tools`: `rig_support_matrix.py`, `doc_counts.py` and `wire_to_transcript.py`. Each entry follows ADR 0113 and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
The Python tools only read and write documentation and test data and never touch a radio. `build_rig` is the one function here that can lead to a keyed transmitter: it always sends an unkey first and opens the transmit gate only when the rig's configuration says `transmit = true`. The server entries bound clients and release the transmitter when a client leaves.

## `crates/radioctld/src/selftest_cli.rs`

### `pub fn spawn() -> (StdinLines, impl FnMut() -> bool + Send)` [@ANCHOR: radioctl:radioctld_cli_stdin_lines_spawn]

Starts one background thread that reads the process's standard input a line at a time and sends each line (with its newline) down a channel, then returns the `StdinLines` reader and the Enter-poll closure built by `from_receiver`. One thread owning stdin lets a prompt (which blocks for a line) and the knob check's "press Enter to skip" (which only asks whether a line has arrived) share the same input without fighting over it. The thread ends at end of input, on a read error or when the channel is closed. It is called once at a terminal; no arguments, no error return, and the thread is never joined.

### `fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize>` [@ANCHOR: radioctl:radioctld_cli_stdin_lines_read]

The `Read` implementation of `StdinLines`, so it can be used wherever a reader is expected. It fills `out` from the current line: it calls `fill_buf` (which may block until a line arrives), copies up to `out.len()` bytes of the unread part, marks them consumed and returns the count. Returns 0 at end of input, when no more lines will come. The error type is for the trait; this implementation does not produce one. Blocking. It is not used by the prompts, which call `read_line` on the `BufRead` side.

### `fn fill_buf(&mut self) -> std::io::Result<&[u8]>` [@ANCHOR: radioctl:radioctld_cli_stdin_lines_fill_buf]

The `BufRead` implementation of `StdinLines`. If the current line is used up it waits on the shared receiver for the next line (taking the lock, so the Enter poll cannot take it at the same moment) and makes it the buffer; the unread part of the buffer is returned. A closed channel or a poisoned lock leaves the buffer empty, so the caller sees end of input (an empty slice) and `read_line` returns 0. Blocks while waiting. The paired `consume` advances the position. Never returns an error.

## `tools/rig_support_matrix.py`

### `def load_descriptions(rigs_dir: Path = RIGS_DIR) -> dict[str, dict]` [@ANCHOR: radioctl:radioctl_support_load_descriptions]

Reads every `*.toml` in `rigs_dir` (default `rigs/` of the repository), in sorted order, and returns a dictionary keyed by rig id. Each value has `name`, `family`, `highest` (the highest verification state among the description's facts, using the order unverified, manual-checked, transcript-checked, hardware-verified; `unverified` when there are no facts) and `hardware` (the verified records whose state is hardware-verified). Raises `KeyError` if a file lacks `[rig]` fields and `tomllib.TOMLDecodeError` for bad TOML. Reads files only.

### `def load_reports(path: Path = RIGS_DIR / "reports" / "verified.toml") -> list[dict]` [@ANCHOR: radioctl:radioctl_support_load_reports]

Returns the `[[report]]` entries of the human-edited `rigs/reports/verified.toml`, one dictionary per report with keys such as `rig`, `radio`, `firmware`, `date`, `reporter` and `issue`, or an empty list when the file has no report table. It does not validate them; `check_consistency` does that. Raises `FileNotFoundError` if the file is missing and `tomllib.TOMLDecodeError` for bad TOML. Reads one file only.

### `def check_consistency(descriptions: dict[str, dict], reports: list[dict], models: list[dict]) -> list[str]` [@ANCHOR: radioctl:radioctl_support_check_consistency]

Finds the disagreements that would make the generated table lie, and returns them as one message each (an empty list means all is consistent). It checks that every description file has a row in the `HAMLIB_IDS` table and the reverse, that each Hamlib model id named there exists in `models` (the Hamlib capability matrix), that each report has `rig`, `radio`, `firmware`, `date`, `reporter` and `issue`, that a report's rig has a description with a hardware-verified fact, and that every description with a hardware-verified fact has a report. It does not modify its arguments. `generate` raises if the list is not empty.

### `def classify(model: dict) -> tuple[str, str, str | None]` [@ANCHOR: radioctl:radioctl_support_classify]

Puts one Hamlib model into a planning bucket and returns `(family, tier, engine)`. The family is the capability matrix's derived family hint, or `unclassified`. A family in `FAMILY_TIERS` supplies its tier and engine (tier C may name a model-specific engine from `MODEL_ENGINES`); otherwise a manufacturer in `TIER_C_MAKERS` makes it tier C (new engine needed) and everything else tier D (odd device); the engine is the model's entry in `MODEL_ENGINES`, or `None`. These are this tool's planning hints, not statements of a radio's wire protocol. Pure; raises `KeyError` if the model lacks `rig_list` data.

### `def status_of(tier: str, engine: str | None, described: bool, not_applicable: bool = False) -> str` [@ANCHOR: radioctl:radioctl_support_status]

Computes the status word for one model row. A model with a description is `described`; otherwise a model marked not applicable is `not applicable`; a tier C model that has an engine is `engine only`; otherwise the tier decides: A gives `engine only`, B `needs variant`, C `needs engine`, D `odd device`. The description check wins over everything else. Raises `KeyError` for a tier other than A to D. Pure; no side effects.

### `def build_rows(models: list[dict], descriptions: dict[str, dict], reports: list[dict]) -> list[dict]` [@ANCHOR: radioctl:radioctl_support_build_rows]

Builds one table row per Hamlib model, sorted by manufacturer, then model (case-insensitive), then id. Each row holds the Hamlib id, manufacturer, model, family, tier, engine, the ids of descriptions that cover it (from `HAMLIB_IDS`), the highest verification level among those descriptions, a `who` string (`reporter, date` from `reports`, joined by `; `, or `-`) and the status from `status_of`. The inputs are not modified. Raises `KeyError` if a covering description is not in `descriptions`. Pure.

### `def render(rows: list[dict], descriptions: dict[str, dict]) -> str` [@ANCHOR: radioctl:radioctl_support_render]

Renders the whole `docs/RIG_SUPPORT.md` Markdown file from the rows. It writes the heading and policy text, a Counts section (models, not applicable, real radios, those with a description and the percentage, hardware-verified), tables by status, verification level, tier and family, the descriptions that map to no Hamlib model, the not-applicable models with their reasons, and a table of every model with pipes in cells escaped. The percentage is integer division and would raise `ZeroDivisionError` if every model were not applicable. The count lines are matched by regular expressions in `doc_counts.py`, so their wording must stay in step. Returns the text ending in a newline; pure.

### `def generate() -> str` [@ANCHOR: radioctl:radioctl_support_generate]

Produces the Markdown for the repository as it is now: reads the Hamlib 4.6.2 capability matrix JSON, the descriptions and the reports, runs `check_consistency` and renders. If there are any inconsistencies it raises `ValueError` whose message lists them, one per line, and writes nothing. No arguments; reads files under the repository root only. Used by `main` and by the unit test that compares the committed file with a fresh render.

### `def main(argv: list[str] | None = None) -> int` [@ANCHOR: radioctl:radioctl_support_main]

The command-line entry point. With no options it writes `docs/RIG_SUPPORT.md`; with `--check` it writes nothing and returns 1 when the committed file differs from a fresh render (printing `docs/RIG_SUPPORT.md is stale: run python3 tools/rig_support_matrix.py` to standard error) or when the file is missing. Returns 1 as well, with the messages on standard error, when `generate` finds inconsistencies. Returns 0 on success. `argv` is the argument list without the program name (default `sys.argv`); an unknown option makes argparse exit with 2.

## `tools/doc_counts.py`

### `def compute(root: Path = ROOT) -> dict[str, str]` [@ANCHOR: radioctl:radioctl_doc_counts_compute]

Gathers every count that a marker in the documentation may name, as text, keyed by marker name. Six counts (`models`, `not_applicable`, `real_radios`, `described`, `description_files`, `hardware_verified`) are read from lines of the generated `docs/RIG_SUPPORT.md` by regular expression; `facts` is the number of `[[facts]]` tables in `rigs/*.toml`; and `cap_<name>` (hyphens become underscores) is the number of descriptions listing each capability. Raises `ValueError` naming the count when RIG_SUPPORT.md lacks its line, and `FileNotFoundError` if files are missing. Reads only; `root` is the repository root.

### `def pages(root: Path = ROOT) -> list[Path]` [@ANCHOR: radioctl:radioctl_doc_counts_pages]

Lists the pages that may carry count markers: `README.md` first, then every `docs/manual/*.md` chapter in sorted order. The paths are built under `root` and are not checked for existence (a missing README would fail later, when it is read). Pure; no I/O at all for `README.md`, one directory glob for the chapters.

### `def check(root: Path = ROOT) -> list[str]` [@ANCHOR: radioctl:radioctl_doc_counts_check]

Compares every marker `<!--count:NAME-->TEXT<!--/count-->` on every line of every page with the computed counts and returns one message per problem, each beginning with `file:line:`. A marker whose name is not a known count gives `unknown count 'NAME'`; one whose text differs gives `'NAME' is X, the data says Y`. An empty list means all markers are current. Changes nothing. Raises what `compute` raises. Markers split across two lines are not seen.

### `def rewrite(root: Path = ROOT) -> list[Path]` [@ANCHOR: radioctl:radioctl_doc_counts_rewrite]

Rewrites, in place, the number inside every count marker whose name is known, using the values from `compute`, and returns the list of pages that actually changed (a page already correct is not written). A marker with an unknown name is left as it is. Writes the page files, so run it only on a clean tree and review the diff. Raises what `compute` raises. Idempotent: a second call returns an empty list.

### `def main(argv: list[str] | None = None) -> int` [@ANCHOR: radioctl:radioctl_doc_counts_main]

The command line of `doc_counts.py`. With `--check` it prints each stale or unknown marker to standard error and returns 1 if there is any, else 0, changing nothing. Without options it rewrites the pages and prints `rewrote <page>` for each one changed, returning 0. A `ValueError` from `compute` (for example RIG_SUPPORT.md has no line for a count) is printed to standard error and gives 1. `argv` excludes the program name; unknown options make argparse exit with 2.

## `tools/wire_to_transcript.py`

### `def parse_wire(text: str) -> list[Frame]` [@ANCHOR: radioctl:radioctl_wire_parse_wire]

Parses the contents of a `bench_console --wire` capture. Each non-blank line must be `SECONDS tx|rx HEX` (a decimal time with a fraction, the direction, and zero or more two-digit hexadecimal bytes, spaces allowed) and becomes a `Frame` with the time as a float, the direction and the bytes. Blank lines are skipped. Any other line raises `ValueError` giving the line number and the line, so a damaged capture is never silently half converted. Pure.

### `def parse_log(text: str) -> list[Command]` [@ANCHOR: radioctl:radioctl_wire_parse_log]

Parses the console `--log` text into `Command` records. A line `SECONDS > text` starts a command; the next line `SECONDS = result` ends it, setting its finish time and its result text with a trailing `(N ms)` removed. Event lines, notes, statistics and lines without a leading time are ignored, and a result line with no open command is ignored. A command that gets no result before the next command keeps `done = None` and is not converted later. Never raises on odd lines. Pure.

### `def exchanges(frames: list[Frame]) -> tuple[list[Exchange], list[Frame]]` [@ANCHOR: radioctl:radioctl_wire_exchanges]

Pairs wire frames into request and answer exchanges. Each `tx` frame starts an `Exchange`; the `rx` frames that follow it, up to the next `tx`, are appended to its answer and set its answer time. An `rx` frame before any `tx` has no request and is returned in the second list as unsolicited (for example a front-panel change). Returns both lists in time order. Pure; the frames are not modified. Which exchange belongs to which console command is decided later, in `convert`.

### `def op_of(command: str) -> str | None` [@ANCHOR: radioctl:radioctl_wire_op_of]

Translates a console command line into the inline TOML `op` table of a conformance transcript step. It knows `mode`, `signal`, `split`, `rit`, `ritoffset` and `filter` (reads), `freq [a|b]`, and the writes `setfreq a|b HZ`, `setmode MODE`, `setsplit 0|1`, `setritoffset HZ` and `setfilter HZ`, with the VFO (variable frequency oscillator, the tuning control) defaulting to `a` for reads and the mode lower-cased. Returns `None` for an empty line, an unknown command or arguments of the wrong shape, and the caller then leaves the step out with a comment. Pure; no I/O. It can emit write operations, but only as test data, never to a radio.

### `def expect_of(result: str) -> str | None` [@ANCHOR: radioctl:radioctl_wire_expect_of]

Translates a console result line into the inline TOML `expect` table of a transcript step: `ok` becomes an acknowledgement, `Frequency(A, Hz(n))` a frequency expectation with the VFO lower-cased, `Mode(X)` a mode (lower-cased), `Signal(n)` a signal value and `Split(true|false)` a split state. Anything else, including error results, returns `None`, so the step is not converted. Pure; matching is on the whole text.

### `def redact(data: bytes, patterns: list[bytes]) -> bytes` [@ANCHOR: radioctl:radioctl_wire_redact]

Hides sensitive bytes such as serial numbers or addresses before a capture is kept. Every occurrence of each pattern in `data` is replaced with zero bytes of the same length, so frame lengths and offsets stay valid; patterns are applied in order, and an empty pattern is skipped. Returns new bytes, not modifying the argument. Matching is plain bytes with no overlap handling beyond sequential replacement. Pure.

### `def toml_text(data: bytes) -> str | None` [@ANCHOR: radioctl:radioctl_wire_toml_text]

Gives a TOML basic string for frames that are plain ASCII text, as many Kenwood and Elecraft commands are: printable bytes 0x20 to 0x7E plus carriage return and line feed. Backslash and double quote are escaped, and CR and LF are written as `\r` and `\n`. Empty data, or any other byte (so any binary frame), returns `None` and the caller writes a hexadecimal string instead. The result includes the surrounding double quotes. Pure.

### `def convert(rig: str, wire: str, log: str, redactions: list[bytes] | None = None) -> str` [@ANCHOR: radioctl:radioctl_wire_convert]

Joins a wire capture and a console log into the text of a conformance transcript for rig `rig`. Each completed console command is paired with the last unclaimed exchange that fell between its `>` line and its result line (with 0.5 ms of slack), since the actor serves one request at a time; when several exchanges fall in the window a `# check:` comment says how many. A step carries the op, the request and answer (as text or hexadecimal, after redaction) and the expectation. Commands or results that are unknown, or with no answer, are left out with a `# not converted` comment, and unclaimed exchanges and stray frames are listed as comments. Raises `ValueError` (from the parsers, or `no console command could be paired with a frame and converted`). The output starts a person's review; it is not final. Pure.

### `def main(argv: list[str] | None = None) -> int` [@ANCHOR: radioctl:radioctl_wire_main]

The command line of `wire_to_transcript.py`: `--rig ID`, `--wire FILE` and `--log FILE` are required, `--redact HEX` may be repeated (spaces allowed in the hexadecimal), and `--out FILE` writes the transcript there instead of standard output. Reads the two capture files and converts them with `convert`. Returns 0 on success. A bad redaction hex string, a conversion error or an unreadable or unwritable file prints `wire_to_transcript: <error>` to standard error and returns 1. Writes only the output file, if named.

## `crates/radioctld/src/rigs.rs`

### `pub fn service_config(rig: &RigConfig, server: &ServerConfig) -> ServiceConfig` [@ANCHOR: radioctl:radioctld_rigs_service_config]

Builds the per-rig settings of the service that answers clients, from the configuration. From the rig section it takes VFO mode, the advertised Hamlib model number, the advertised receive range (low and high, in hertz) and the transmit power advertised (in milliwatts); from the server section it takes the cache maximum age and the request timeout. Both arguments are borrowed and copied field by field; no validation (the config loader already did it), no I/O and no failure. The values only shape what `\dump_state` and replies say, not what the rig does.

### `pub async fn build_rig(runtime: &Handle, rig: &RigConfig, description: RigDescription, server: &ServerConfig, options: RigOptions) -> Result<BuiltRig, String>` [@ANCHOR: radioctl:radioctld_rigs_build_rig]

Brings one rig up on `runtime`: protocol engine for the description's family, transport, rig actor and client service. The transport comes from the configuration: a `radiod` channel, a virtual rig (or virtual USB board, or virtual DRT1 twin), a real USB board (through its loader if the board needs the user's firmware), a serial port (fixed baud or the baud probe), or TCP (raw or TS-890S LAN framing). `options` is the actor's setting; `rig.max_transmit` replaces its transmit limit when set. Errors are strings prefixed `rig <name>:` (no engine, wrong description family for the transport, port or board cannot be opened once, baud probe failed). Transmit safety: if the rig can switch PTT (push to talk) it first sends an unkey, because a killed predecessor may have left the radio keyed, and a failure to unkey stops start-up with `the radio may be keyed`. The transmit gate is opened, with a permit naming the configuration, only when `rig.transmit` is true. Returns the `BuiltRig` with the service and the link monitors for the transport. Async; opens real hardware or network links; uses one attempt at start, then links reopen without limit.

### `fn serial_config(target: &PortTarget, baud: u32, lines: LineControl, description: &RigDescription) -> SerialConfig` [@ANCHOR: radioctl:radioctld_rigs_serial_config]

Builds the serial port configuration of a rig at `baud`: the description's line settings (data bits, parity, stop bits and flow control; 8N1 when it states none), the configured control-line handling (`lines`: managed or left alone), and the default single open attempt. Used for both the baud probe and the real link so the port is opened identically in both. `target` is borrowed and cloned; `baud` is in bits per second and is replaced at each probe open. Private, pure, no I/O (the port is not opened here) and no failure.

### `pub fn tcp_framing(rig: &RigConfig, server: &ServerConfig, framing: &TcpFraming, idle_timeout: Option<Duration>) -> Result<(Box<dyn Framing>, Option<Duration>), String>` [@ANCHOR: radioctl:radioctld_rigs_tcp_framing]

Chooses the framing of a TCP rig and the silence limit that goes with it. `Raw` is a plain byte stream with the configured `idle_timeout` (or none). For the TS-890S LAN session it reads the account password now from the secret store (`server.secrets_dir`, a file the owner alone can read) under the configured secret name, builds the login credentials and returns the LAN framing with the configured limit or, if none, three keep-alive intervals (the radio hangs up after 10 s of silence). Errors name the rig and the secret, never a secret value: no `secrets_dir`, the secret missing or unreadable, or an account or password that cannot be sent. Blocking file read at start-up.

## `crates/radioctld/src/config.rs`

### `fn tcp_framing(framing: Option<&str>, lan_account: Option<String>, lan_administrator: Option<bool>, lan_password_secret: Option<String>, has_secrets_dir: bool) -> Result<TcpFraming, String>` [@ANCHOR: radioctl:radioctld_config_tcp_framing]

Validates a TCP transport's `framing` key and the `lan_*` keys while the configuration file is loaded. `framing` of `raw` (the default when absent) accepts none of `lan_account`, `lan_administrator` or `lan_password_secret` and returns `TcpFraming::Raw`. `ts890-lan` requires a non-empty `lan_account` and a non-empty `lan_password_secret`, which is a secret name and never the password itself, and requires `has_secrets_dir` to be true because the password is read from that directory; `lan_administrator` defaults to false. Any other framing text is an error. Each error is a sentence naming the key. Pure; nothing is read.

## `crates/radioctld/src/server.rs`

### `pub fn from_config(config: &ServerConfig) -> Limits` [@ANCHOR: radioctl:radioctld_server_limits_from_config]

Copies the four server bounds out of the loaded server configuration into a `Limits` value: the most simultaneous clients, how long a connection may be silent before it is closed, how long a started line may take to end, and the longest request line in bytes. The configuration is borrowed; no validation (the loader has done it), no I/O, cannot fail. The result is `Copy` and is handed to `serve`.

### `pub fn serve(handle: &Handle, listener: TcpListener, service: Arc<dyn LineService>, access: Arc<AllowList>, limits: Limits) -> std::io::Result<ServerHandle>` [@ANCHOR: radioctl:radioctld_server_serve]

Starts serving a rig (or rotator) on a TCP listener that the caller has already bound, on loopback unless the configuration allows otherwise. It records the listener's local address, creates the shutdown signal and the shared counters, and spawns the accept task on the runtime `handle`; the server creates no runtime of its own. Peers outside the allow-list are dropped before any byte is read and connections beyond `max_clients` are dropped at once; each accepted client gets its own task bounded by `limits`. Returns the `ServerHandle`, or the I/O error if the listener's address cannot be read. The service sees `answer` for each line and `disconnect` once per client, which unkeys the transmitter if that client owned it. Not async; returns at once.

### `pub fn local_addr(&self) -> SocketAddr` [@ANCHOR: radioctl:radioctld_server_local_addr]

Returns the socket address the server's listener is bound to, remembered when `serve` started. It is what clients connect to; when the caller bound port 0 for a test, this is how the real port is found. A copy is returned, with no I/O, no lock and no failure, and the value never changes for the life of the handle.

### `pub fn stats(&self) -> ServerStats` [@ANCHOR: radioctl:radioctld_server_stats]

Returns a snapshot of the server's counters: connections accepted, refused by the allow-list (`denied`), refused because the client limit was reached (`busy`), connected now (`active`), dropped for an over-long line, and dropped for silence, a stalled line or a stalled write (`timeouts`). Each is read atomically with relaxed ordering, so the values are individually correct but not a consistent set at one instant. Cheap, lock-free, never blocks and cannot fail; safe to call from any thread.
