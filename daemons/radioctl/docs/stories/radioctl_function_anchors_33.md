# radioctl function anchors, batch 33

Function anchors for the `radioctld` crate's self-test program and calibration code: candidate summaries (`candidates.rs`), the self-test logger (`selftest.rs`), the command-line driver (`selftest_cli.rs`), the upload transport (`upload.rs`), the self-updater (`update.rs`), the knob check (`knob.rs`) and the Icom meter calibration (`meter_cal.rs`). Each entry follows ADR 0113 and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`.
Nothing here keys a transmitter. The self-test opens rigs with the transmit gate closed and plans only read-only operations; the knob check sends nothing at all; the meter functions are pure arithmetic on raw meter readings.

## `crates/radioctld/src/candidates.rs`

### `pub fn summaries_from(texts: &[(String, String)]) -> Result<Vec<RigSummary>, String>` [@ANCHOR: radioctl:radioctld_candidates_summaries_from]

Turns `(label, TOML text)` pairs into `RigSummary` records (id, display name, family name, highest verified state), sorted by id. Each text is parsed and validated with `rigfile::parse`, and the family is the debug name of the family enum. Both the embedded descriptions and a directory of files go through it. The first description that fails to parse stops the whole call with the error `<label>: <parse error>`, so the label should identify the file. Borrowed input, owned output; pure apart from the parse; no hardware.

### `pub fn auto_pick<'a>(ports: &'a [PortInfo], rigs: &'a [RigSummary]) -> Result<(&'a PortInfo, &'a RigSummary), String>` [@ANCHOR: radioctl:radioctld_candidates_auto_pick]

Chooses one port and one description only when the devices themselves make that unambiguous: exactly one USB port in total, and its product string names exactly one description (via `candidates_for_product`). Returns references into the inputs. The errors explain why no choice was made: `no USB serial port found` for an empty port list, `no port's own product string names exactly one rig description` when there is no evidence, and a message about several ports or a port fitting several descriptions otherwise. A port without a product string never qualifies. The caller prints the listing instead of guessing. Pure; no I/O.

## `crates/radioctld/src/selftest.rs`

### `pub async fn run_checks_logged(rig: &RigHandle, plan: Vec<(String, Op)>, wire_path: Option<&Path>) -> Vec<CheckResult>` [@ANCHOR: radioctl:radioctld_selftest_run_checks_logged]

Runs the planned checks one after another against the open rig and returns one `CheckResult` per entry, in plan order. When `wire_path` names the file that `wire_tap` appends to, the file size is noted before each check and the lines written during the check are stored in that result's `wire`, so the upload can carry raw bytes per check; with `None` the wire text is empty. An operation that is not read-only (`is_read_only`) is never sent and becomes a failure `refused: not a read-only operation`. An engine Unsupported error becomes not-applicable, any other error a failure naming the kind and detail, and a reply becomes a pass with its debug text. Awaits each check in turn; no error return. Cannot transmit.

## `crates/radioctld/src/selftest_cli.rs`

### `pub fn firmware_text(results: &[CheckResult]) -> Option<String>` [@ANCHOR: radioctl:radioctld_cli_firmware_text]

Extracts the radio's firmware version as readable text for the thank-you message. It looks for the result named `identify: firmware`; if that passed and its text has the debug form `Firmware(1, 2)` it returns `1.2`. Returns `None` when there is no such check, it did not pass, the text has another shape or either number is not a non-negative integer. Pure; the results are borrowed and nothing is printed.

### `pub fn thanks(rig: &str, results: &[CheckResult]) -> String` [@ANCHOR: radioctl:radioctld_cli_thanks]

Writes the thank-you printed after a successful upload. `rig` is the manufacturer and model; the firmware version from `firmware_text` is appended with one space when known and left out otherwise. Not-applicable checks are not counted. If no counted check failed the text says the volunteer verified the operation of the rig; otherwise it says the report is in, how many of the counted checks did not pass, and that this is a problem in the software and not the volunteer's fault. Both end with an invitation to re-run and a note that the project may make contact. Returns one line ending in a newline. Pure.

### `pub async fn update_flow(o: &Options, host: &Host, cfg: &UpdateConfig, input: &mut dyn BufRead, out: &mut dyn Write) -> Option<i32>` [@ANCHOR: radioctl:radioctld_cli_update_flow]

The version check at the start of a run. Returns `None` to carry on with the test, or `Some(status)` when the program was replaced and the run is over. It does nothing on a platform with no release (`platform()` is `None`). A fetch or version-parse failure prints one line and returns `None`; an equal or older release returns `None` silently. For a newer one, `--update` installs without asking, an interactive user is asked `[y/N]` (anything beginning with y or Y means yes), and otherwise a notice says to run with `--update`. Install is download, `verify_and_extract` (hash and signature), then `replace_exe`; any failure prints that the existing program is unchanged and returns `None`. On success it restarts (`restart`, status 0 in tests with restart off). Network, file and process effects; no hardware.

### `pub fn main_from_env() -> i32` [@ANCHOR: radioctl:radioctld_cli_main_from_env]

The complete program for a binary's `main`, with no update bundle: it calls `main_with_bundle(None)`. Reads the real command line, terminal state, environment variables and serial ports, runs the self-test and returns the exit status for the process to exit with (0 all passed or not applicable, 1 a check failed or a rig could not be opened, 2 usage or setup error). The relay's `radioctl-selftest` and radioctl's own program both use it. Blocks until the run ends; opens real ports but never transmits.

### `pub fn main_with_bundle(bundle: Option<(&str, Vec<[u8; 32]>)>) -> i32` [@ANCHOR: radioctl:radioctld_cli_main_with_bundle]

The program body for a self-test shipped inside a signed release bundle. `bundle` carries the bundle's version and its 32-byte ed25519 release keys; with `Some` the start-of-run update check is enabled against the default release channel, unless the environment variable `RADIOCTL_SELFTEST_UPDATED` is set (a run just restarted by an update does not check again). It parses the arguments (a parse error prints and returns 2), decides whether a terminal is attached, starts the stdin reader thread when interactive, reads contact details from `RADIOCTL_SELFTEST_NAME` and `RADIOCTL_SELFTEST_EMAIL`, lists the real serial ports, builds a two-worker tokio runtime (failure returns 2) and blocks on `run`. Listens for network radios for 3 seconds. Returns the exit status. Never transmits.

### `async fn probe_baud(description: &RigDescription, target: &str, unmanaged: bool) -> Result<Vec<u32>, String>` [@ANCHOR: radioctl:radioctld_cli_probe_baud]

Finds the baud rate for `--baud auto`. It builds the engine for the description, parses the target, opens a probe port on it (control lines left alone when `unmanaged` is true) and runs the baud probe: a read-only frequency request at each default rate until one answers. Returns the rates that were tried, in order, with the winner last; the caller takes the last one. Errors as strings: no engine for the family, an unparseable target, or `baud probe: <cause>` when no rate answered. Async; opens and closes a real serial port repeatedly; sends only reads, never a transmit.

### `async fn test_one(o: &Options, t: &Target, loaded: &LoadedDescription, report: &Path, interactive: bool, enter: &mut dyn FnMut() -> bool, out: &mut dyn Write) -> Result<Tested, String>` [@ANCHOR: radioctl:radioctld_cli_test_one]

Tests one rig end to end and writes its local report file at `report`. It creates a temporary `.wire.tmp` file beside the report, picks the baud (fixed, 19200 for a virtual rig on auto, or by `probe_baud`), opens the rig through `selftest::open` with the transmit gate closed, runs `plan_checks` with per-check wire logs, adds the interactive knob check unless the target is `virtual`, builds the report header (redacted target, identity, verified-fact counts, probe record), writes the report text and deletes the temporary file. Returns what the upload needs (results, transport `virtual`, `usb` or `serial`, baud, USB ids, tried rates). Errors only for a rig that cannot be opened or a file that cannot be written; check failures are results, not errors. Async; hardware reads only.

### `pub fn from_receiver(rx: std::sync::mpsc::Receiver<String>) -> (StdinLines, impl FnMut() -> bool + Send)` [@ANCHOR: radioctl:radioctld_cli_stdin_lines_from]

Builds the paired reader and poll over a channel of input lines (the method `StdinLines::from_receiver`). The returned `StdinLines` implements `BufRead` and blocks for the next line when its current one is used up; the returned closure is true when a line was waiting and consumes it, which is how the knob check notices an Enter press without blocking. Both share the receiver through an `Arc<Mutex<..>>`, so a prompt and the poll never read the same line twice; a poisoned lock makes the poll false and the reader return end of input. Test code feeds it lines through a channel.

## `crates/radioctld/src/upload.rs`

### `pub fn decode_body(response: &[u8]) -> Option<(u16, Vec<u8>)>` [@ANCHOR: radioctl:radioctld_upload_decode_body]

Reads a complete HTTP/1.1 response that was read to the end of the connection and returns the status code and the body bytes. It splits at the first blank line (`\r\n\r\n`), takes the code from the first line (`HTTP/1.1 204 No Content` gives 204) and, when the head contains `transfer-encoding: chunked` (any case), undoes the chunking: hexadecimal size lines (extensions after `;` ignored), data, and the final zero chunk. Returns `None` when there is no complete head, the status line has no number, or a chunk is truncated or malformed. Pure; never panics on garbage, since every slice access is checked.

### `pub async fn request(method: &str, url: &str, body: &[u8], max_response: usize, timeout: Duration) -> Result<(u16, Vec<u8>), String>` [@ANCHOR: radioctl:radioctld_upload_request]

Makes one HTTP request and returns the status and body. `method` is `POST` (with a JSON `body`) or `GET` (with an empty one); `url` follows `parse_server` (https anywhere, http only to loopback); at most `max_response` bytes of the answer are read; the whole exchange must finish within `timeout`. Errors are one short line: a URL that does not parse, `cannot connect`, TLS trouble, a timeout (`timed out after N s`) or no HTTP answer. Async network I/O that opens a new connection with `Connection: close` each time, with no retries and no redirects. Shared by the report upload and the update check.

### `async fn exchange(server: &ServerUrl, method: &str, body: &[u8], max_response: usize) -> Result<(u16, Vec<u8>), String>` [@ANCHOR: radioctl:radioctld_upload_exchange]

Performs the request on an already parsed `ServerUrl`: connects TCP, optionally wraps it in TLS (rustls with the bundled web PKI roots; the host name is checked), writes an HTTP/1.1 head with `Host`, `Content-Type: application/json`, `Content-Length`, a `User-Agent` of radioctl-selftest and the version, and `Connection: close`, then the body, and reads the answer until the server closes, at most `max_response + 65,536` bytes (the extra allows for the head). The answer is decoded with `decode_body`. Errors: cannot connect, TLS setup, bad host name, TLS handshake, write failure, or no HTTP answer. It has no timeout of its own; `request` supplies it. Private.

## `crates/radioctld/src/update.rs`

### `pub fn platform() -> Option<&'static str>` [@ANCHOR: radioctl:radioctld_update_platform]

The release platform name of this build as the relay's release channel names it: `linux-x86_64`, `linux-aarch64`, `macos` (Apple silicon only) or `windows` (x86-64 only), chosen at compile time. Returns `None` on any other system (for example Intel macOS or 32-bit Linux), in which case the start-of-run update check is skipped. No arguments, no I/O, no failure.

### `pub fn is_newer(candidate: &str, current: &str) -> Result<bool, String>` [@ANCHOR: radioctl:radioctld_update_is_newer]

Compares two versions of the form `X.Y.Z` numerically, field by field, and is true when `candidate` is strictly higher than `current`; equal or lower gives false (so `1.10.0` is newer than `1.9.0`). Surrounding whitespace is trimmed. Each version must be exactly three dot-separated non-negative integers; anything else (two or four fields, letters, a pre-release suffix, empty) returns the error `"<text>" is not a version like 1.2.3`. Pure. There is no version floor and no rollback protection by design.

### `pub async fn fetch_latest(base_url: &str, platform: &str) -> Result<Latest, String>` [@ANCHOR: radioctl:radioctld_update_fetch_latest]

Asks the release channel which version is newest for `platform`: it POSTs a JSON-RPC style body to `<base_url>/api/relay_bridge/binary/latest_info` within `CHECK_TIMEOUT` (5 seconds), reading at most 64 KiB. Returns the version string and the bundle's sha256 from the answer. Errors, one line each: any `request` failure, a status other than 200 (`the server answered N`), a body that is not JSON, no `result`, a `status` other than `success` (`the server has no release for this platform`), or a missing `version` or `sha256`. Async network I/O, read-only on the server; the version text is not validated here (`is_newer` does that).

### `pub async fn download(base_url: &str, platform: &str) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctld_update_download]

Downloads the signed release bundle for `platform` with a GET to `<base_url>/api/relay_bridge/binary/download/<platform>`. The answer is limited to `MAX_BUNDLE` (256 MiB) and the whole download to `DOWNLOAD_TIMEOUT` (180 seconds). Returns the bytes in memory. Errors are strings: a request failure or timeout, or `the download answered N` for any status other than 200 or an oversize body. The bytes are not trusted yet: the caller must pass them through `verify_and_extract` before use. Async network I/O; writes no files.

### `pub fn verify_and_extract(bundle: &[u8], sha256: &str, keys: &[[u8; 32]]) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctld_update_verify_and_extract]

Accepts a downloaded bundle only if it is the published one, and then pulls the self-test executable out of it. The bundle's SHA-256 (compared with `sha256` ignoring case) must match, and its zipsign signature (empty context, as the relay's publishing script signs) must verify against one of the ed25519 public keys in `keys`. Then it reads the entry `radioctl-selftest` (`radioctl-selftest.exe` on Windows) from the zip. Errors: `the download does not match the published sha256`, `bad release key`, `the signature does not verify`, an unreadable archive, or `<name> is not in the bundle`. Pure CPU work on memory; never touches the disk.

### `pub fn replace_exe(exe: &Path, new_bytes: &[u8]) -> Result<PathBuf, String>` [@ANCHOR: radioctl:radioctld_update_replace_exe]

Replaces the program file `exe` with `new_bytes`, keeping the old one as `NAME.prev` in the same directory. It writes `NAME.new` beside it, sets mode 0755 on Unix, deletes any older `.prev`, copies the current file to `.prev`, then renames `.new` over `exe`, so a crash leaves the old or the new program, never a half-written one. Returns the path of the kept old copy. On any failure it removes the temp file, leaves the old program in place and returns a message such as `cannot write the new program: <cause>` or `cannot put the new program in place: <cause>`. Needs write access to the program's directory. Blocking file I/O.

### `pub fn restart(exe: &Path) -> i32` [@ANCHOR: radioctl:radioctld_update_restart]

Starts the replaced program with the same command-line arguments and the environment variable `RADIOCTL_SELFTEST_UPDATED=1`, so the new run does not offer another update. On Unix it replaces the current process (`exec`), so it returns only if that failed, printing `cannot restart after the update` and returning 1. On other systems it runs the program, waits, and returns its exit code (1 if it has none or could not start). Process side effect; a successful Unix call never returns.

### `fn entry_name() -> &'static str` [@ANCHOR: radioctl:radioctld_update_entry_name]

The name of the self-test executable inside a release bundle: `radioctl-selftest.exe` when the program is built for Windows, `radioctl-selftest` otherwise, chosen at compile time. `verify_and_extract` looks the entry up by this name. Private, pure, no arguments and no failure; if the publishing script ever renames the file, this must change with it.

## `crates/radioctld/src/knob.rs`

### `pub fn claims_unsolicited(description: &RigDescription) -> bool` [@ANCHOR: radioctl:radioctld_knob_claims_unsolicited]

True when the rig description claims the `Transceive` capability, meaning the radio reports changes (such as a turned VFO knob) on its own without being polled: Icom CI-V transceive, Elecraft and Kenwood auto-information, Yaesu or Flex subscriptions. The knob check runs only for such descriptions. The description is borrowed; pure; no errors.

### `pub async fn watch_knob(rig: &RigHandle, wait: Duration, enter: &mut dyn FnMut() -> bool) -> KnobOutcome` [@ANCHOR: radioctl:radioctld_knob_watch]

Watches the rig's event stream for up to `wait` for a frequency change reported by the radio itself. Events from the answers to our own polls or commands (any source other than `Transceive`) are ignored. `enter` is polled about ten times a second (each wait on the stream lasts at most 100 ms) and returns true once the volunteer pressed Enter, giving `Skipped`. A frequency change gives `Seen(delta_hz)`, new minus old frequency (zero when no old value is known); if time runs out the result is `NotSeen`. Async; sends nothing to the radio, so it cannot transmit or change a setting.

### `pub fn knob_result(outcome: &KnobOutcome, millis: u128) -> CheckResult` [@ANCHOR: radioctl:radioctld_knob_result]

Turns a knob outcome into the report line. The check is named `unsolicited status (turn the VFO knob)` and has the operation text `(none: listens only)`, with `millis` as its duration and no wire text. `Seen(delta)` is a pass saying the frequency changed by the signed amount in hertz without being asked. `NotSeen` is a failure (did not pass) carrying the hint about enabling auto-information or CI-V Transceive in the radio's menu. `Skipped` is not-applicable (`skipped by you (Enter pressed)`) and so is not counted as a failure. Pure.

### `pub async fn knob_check(description: &RigDescription, rig: &RigHandle, interactive: bool, enter: &mut dyn FnMut() -> bool, out: &mut dyn Write, wait: Duration) -> Option<CheckResult>` [@ANCHOR: radioctl:radioctld_knob_check]

The whole interactive check. Returns `None` (no line in the report) when the description does not claim the Transceive capability. Without a terminal (`interactive` false) it returns a not-applicable result `skipped: no terminal` with 0 ms, never a failure. At a terminal it prints to `out` a request to turn the knob a little, with the number of seconds it will wait, runs `watch_knob` and returns `knob_result` with the elapsed milliseconds. `enter` reports an Enter press so the volunteer can skip. Async; the real program passes the 15-second `KNOB_WAIT`. Reads only; no transmit.

## `crates/radioctld/src/meter_cal.rs`

### `fn clamped(points: &[(f64, f64)], raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_clamped]

Piecewise-linear lookup used for the compression, supply-voltage, drain-current and power scales. `points` are ascending `(raw deflection, value)` pairs; `raw` is the 0 to 255 meter reading, capped at 255. A reading at or below the first point returns the first value; between two points it interpolates linearly; above the last point it returns the last value (nothing beyond the printed points is documented). An empty table returns 0.0. Private, pure, no panic. The tables are unverified estimates (`CALIBRATION_VERIFIED` is false).

### `pub fn comp_db(cal: &MeterCal, raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_comp_db]

Converts the raw compression meter reading (0 to 255 deflection, CI-V `15 14`) to decibels of speech compression, in the range 0 to about 30 (a rig's table may end lower, for example 25.5 on the IC-705). `cal` is the model's scale from `for_rig` and `raw` is the deflection. It uses the clamped linear lookup, so it never exceeds the last table value. Pure; an unverified estimate that no radio has confirmed.

### `pub fn id_amps(cal: &MeterCal, raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_id_amps]

Converts the raw drain-current meter reading (0 to 255) to amperes of final-amplifier current, from 0 up to the last point of the model's table (25 A for the default, 30 A for the IC-7610, 4 A for the IC-705). Linear between printed points and clamped beyond them. `cal` comes from `for_rig`. Pure and unverified; the IC-9100 prints no such meter, so it uses the default table.

### `pub fn level(cal: &MeterCal, meter: radioctl_core::units::Meter, raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_level]

The single entry point that converts a raw deflection into the value a `rigctl` client expects for a given meter: SWR gives a ratio of 1.0 or more, ALC (automatic level control) a fraction from 0 to 1, COMP decibels, Vd volts and Id amperes. It dispatches to `swr_ratio`, `alc_fraction`, `comp_db`, `vd_volts` and `id_amps`. The match over `Meter` is exhaustive. Used when serving `get_level` for the meter levels. Pure; an unverified estimate.

### `pub fn full_scale(cal: &MeterCal, meter: radioctl_core::units::Meter) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_range]

The largest value a client is told a meter can read, used for the advertised level range in `\dump_state` and for scaling: SWR is the ceiling 10.0, ALC is 1.0, and COMP, Vd and Id are the last value of that model's table (0 if the table is empty). The model's table is selected through `cal`. Pure; no errors. It is the top of the advertised range, not a measured maximum.

### `pub fn vd_volts(cal: &MeterCal, raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_vd_volts]

Converts the raw supply-voltage meter reading (0 to 255) to volts, from 0 up to the last point of the model's table (16 V for the default, IC-7610 and IC-705). It interpolates linearly between printed points and clamps beyond them, so it never reports more than the table's last value. The model's table comes from `for_rig`; the IC-9100 prints none and uses the default. Pure; an unverified estimate.

### `pub fn for_rig(id: &str) -> &'static MeterCal` [@ANCHOR: radioctl:radioctld_meter_cal_for_rig]

Selects the scale tables for a rig description id: `icom-ic-9100`, `icom-ic-7300`, `icom-ic-7610` and `icom-ic-705` have their own points (taken from each model's printed CI-V table), and any other id gets the shared `DEFAULT` (the IC-7850 points for SWR and ALC, an estimate for the rest). The reference is to a static, so it is free to copy and never fails. The id is matched exactly and case-sensitively. Pure. All tables are unverified.

### `pub fn swr_ratio(cal: &MeterCal, raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_swr_ratio]

Converts the raw SWR meter reading (0 to 255, capped at 255) to a standing-wave ratio of 1.0 or more. It interpolates between the model's printed points (for the default: 0 is 1.0, 48 is 1.5, 80 is 2.0, 120 is 3.0); above the last point it continues the last segment's slope, and the result is capped at 10.0, the top of the range `\dump_caps` advertises. `cal` comes from `for_rig`. Pure and unverified, which is why the raw value stays available through `Op::GetMeter`. Every shipped table has four points; a table with fewer than two would index out of range, so one must never be added.

### `pub fn alc_fraction(cal: &MeterCal, raw: u16) -> f64` [@ANCHOR: radioctl:radioctld_meter_cal_alc_fraction]

Converts the raw ALC meter reading to a fraction from 0.0 to 1.0 by dividing by the model's full-scale deflection (120 for every model here) and clamping, so a reading at or above that deflection gives 1.0 and a larger raw value is not an error. `cal` supplies `alc_full`; `raw` is the 0 to 255 deflection. Pure; an unverified estimate, and the value is relative, not a calibrated decibel figure.
