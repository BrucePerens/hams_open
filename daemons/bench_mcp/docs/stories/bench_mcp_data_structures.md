# Data structures

### `Tier` [@ANCHOR: bench_mcp:bench_mcp_config_tier]

**Purpose and location.** The permission tier of a tool or instrument: what a caller may do through it. Defined in `daemons/bench_mcp/src/config.rs:59` as a pub `enum`. It derives PartialEq, PartialOrd, Clone, Copy, Debug.

| Variant | Definition | Meaning |
|---|---|---|
| `ReadOnly` | `ReadOnly = 0` | Only reading, no change to any instrument. |
| `ReadConfig` | `ReadConfig = 1` | The middle tier: reading plus the configuration and capture actions that are safe to repeat; actions such as switching a port or output off, power cycling or keying need `Full` (see the module manuals for each tool's tier). |
| `Full` | `Full = 2` | The top tier: everything the instrument supports, including outputs, power switching and, with an attestation, keyed transmit tests. |

**Invariants, ownership and concurrency.** Ordered (`ReadOnly` < `ReadConfig` < `Full`); a tool above an instrument's tier is refused before any command is sent.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tier_of` (src/config.rs:130), `tier_name` (src/procedure/tiers.rs:27), `parse_tier` (src/procedure/tiers.rs:37), `build` (src/procedure/tiers.rs:51).

**See also:** test `parses_both_drivers_with_defaults (src/config.rs)`.

### `Kind` [@ANCHOR: bench_mcp:bench_mcp_config_kind]

**Purpose and location.** The kind of instrument an entry in the configuration describes; it selects the module that drives it. Defined in `daemons/bench_mcp/src/config.rs:66` as a pub `enum`. It derives Clone, Copy, PartialEq, Debug.

| Variant | Definition | Meaning |
|---|---|---|
| `Scope` | `Scope` | An oscilloscope. |
| `Psu` | `Psu` | A bench power supply. |
| `Vna` | `Vna` | The DG8SAQ VNWA vector network analyser, driven directly by the `vnwa_open` crate (USB and its two codecs). |
| `Sdr` | `Sdr` | A software-defined radio driven through SoapySDR. |
| `UsbHub` | `UsbHub` | The host's USB hubs and ports (`usb_hub` module): read from sysfs, switched with uhubctl. |
| `Audio` | `Audio` | The host's sound devices (`audio` module): PipeWire, wpctl and the speech synthesizers. |
| `Radioctl` | `Radioctl` | A radio driven through the `radioctld` daemon (`radioctl` module): receivers, and transmitters into an attested dummy load. |
| `Rotator` | `Rotator` | An antenna rotator driven through a `radioctld` rotator listener (`rotator` module). |
| `Sa` | `Sa` | A Siglent SSA3000X spectrum analyzer with tracking generator (`sa` module), SCPI over the LAN. |
| `Elad` | `Elad` | An ELAD FDM-DUO, FDM-S2 or FDM-S1 receiver through the `elad_usb` crate (receive only). |
| `Example` | `Example` | The skeleton instrument (`example` module): an imaginary SCPI signal source, the starting point for a new driver. |
| `Procedure` | `Procedure` | The procedure runner (`procedure` module): programs that call bench tools in loops, next to the server. |
| `Rx888` | `Rx888` | The RX-888 MkII HF receiver through ka9q-radio's `radiod` (`rx888` module), receive only. |
| `Glasgow` | `Glasgow` | The Glasgow Interface Explorer (`glasgow` module): digital I/O (UART, I2C, SPI, JTAG, SWD, logic analyzer, GPIO) through the pinned `glasgow` tool. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `spec` (src/fake_scope.rs:265).

**See also:** test `parses_both_drivers_with_defaults (src/config.rs)`.

### `Instrument` [@ANCHOR: bench_mcp:bench_mcp_config_instrument]

**Purpose and location.** One instrument entry of the configuration: its name, kind, driver, tier, connection data, safety limits and the per-module settings. Defined in `daemons/bench_mcp/src/config.rs:98` as a pub `struct`. It derives Clone, Debug.

| Field | Type | Meaning |
|---|---|---|
| `name` | `String` | The instrument's name, used in tool arguments. |
| `kind` | `Kind` | The kind, which selects the driving module. |
| `driver` | `String` | The driver name within the kind (for example `nanovna`). |
| `tier` | `Tier` | The highest tier this instrument allows. |
| `host` | `Option<String>` | The network host, for LAN instruments. |
| `port` | `u16` | The TCP port (0 when unused). |
| `serial` | `Option<String>` | The serial device path, for serial instruments. |
| `baud` | `u32` | The serial baud rate. |
| `max_volts` | `f64` | Supply only: the highest voltage a tool may set. |
| `max_amps` | `f64` | Supply only: the highest current limit a tool may set. |
| `max_on_s` | `f64` | Supply only: the longest the output may stay on after one switch-on, seconds. |
| `connect_waits_s` | `Vec<f64>` | Scope only: the pause after each failed connection attempt (`connect_waits_s`), seconds; one more attempt than there are pauses. |
| `vna` | `vnwa_open::sweep::Config` | VNA only: the settings of `vnwa_open::sweep::Config`. |
| `nanovna` | `crate::nanovna::Config` | The NanoVNA module's settings. |
| `sdr` | `crate::sdr::Config` | The SDR module's settings. |
| `usb` | `crate::usb_hub::Config` | The USB hub module's settings. |
| `audio` | `crate::audio::Config` | The audio module's settings. |
| `radioctl` | `crate::radioctl::Config` | The radio control module's settings. |
| `rotator` | `crate::rotator::Config` | The rotator module's settings. |
| `elad` | `crate::elad::Config` | The ELAD module's settings. |
| `example` | `crate::example::Config` | The example module's settings. |
| `sa` | `crate::sa::Config` | The spectrum-analyzer module's settings. |
| `procedure` | `crate::procedure::Config` | The procedure runner's settings. |
| `rx888` | `crate::rx888::Config` | The RX-888 module's settings. |
| `glasgow` | `crate::glasgow::Config` | The Glasgow module's settings. |

**Invariants, ownership and concurrency.** Read once from the configuration file and cloned to each module; every kind's settings are present, the ones of other kinds hold defaults and are unused. The limits (`max_volts`, `max_amps`, `max_on_s`) are protections enforced before a command is sent.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `reject_shared_claims` (src/config.rs:233), `parse` (src/config.rs:303), `load` (src/config.rs:395), `connect` (src/main.rs:342), `survey` (src/main.rs:395), `call` (src/main.rs:412), `tool_defs` (src/main.rs:624), `insts` (src/main.rs:774).

**See also:** test `the_vna_sweep_tool_is_listed_and_allowed_only_at_read_config (src/main.rs)`.

### `Iq` [@ANCHOR: bench_mcp:bench_mcp_dsp_iq]

**Purpose and location.** One complex sample, I then Q, as the radios deliver them (full scale is 1.0). Defined in `daemons/bench_mcp/src/dsp.rs:8` as a pub `type`. It derives no traits.

**Definition.** `pub type Iq = (f32, f32);` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `block_fft` (src/dsp.rs:81), `welch` (src/dsp.rs:98), `stats` (src/dsp.rs:261), `tone` (src/dsp.rs:283), `capture` (src/sdr.rs:509), `write_iq` (src/sdr.rs:674).

**See also:** test `stats_report_rms_peak_dc_and_clipping (src/dsp.rs)`.

### `Spectrum` [@ANCHOR: bench_mcp:bench_mcp_dsp_spectrum]

**Purpose and location.** An averaged power spectrum of complex samples, with bin 0 at -rate/2 (the DC bin is at `len / 2`). Defined in `daemons/bench_mcp/src/dsp.rs:12` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `bin_hz` | `f64` | Width of one bin in hertz. |
| `power_db` | `Vec<f64>` | Power of each bin in dB relative to a full-scale complex tone. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `welch` (src/dsp.rs:98), `noise_floor_db` (src/dsp.rs:134), `spectrum` (src/spectrum.rs:99), `peaks` (src/spectrum.rs:133), `spectrum` (src/audio/analysis.rs:140), `peaks` (src/audio/analysis.rs:187), `dominant` (src/audio/analysis.rs:227), `median_db` (src/audio/analysis.rs:232).

**See also:** test `a_bin_centred_tone_reads_its_amplitude_and_offset (src/dsp.rs)`.

### `Peak` [@ANCHOR: bench_mcp:bench_mcp_dsp_peak]

**Purpose and location.** A spectral peak: its offset from the centre frequency and its power. Defined in `daemons/bench_mcp/src/dsp.rs:143` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `offset_hz` | `f64` | Offset from the centre frequency in hertz. |
| `power_db` | `f64` | Power in dB relative to full scale. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `peaks` (src/spectrum.rs:133), `peaks` (src/audio/analysis.rs:187), `dominant` (src/audio/analysis.rs:227), `find_peaks` (src/sa/sweep.rs:161), `trace_summary` (src/sa/sweep.rs:358).

**See also:** test `peaks_are_ordered_floor_is_the_median_and_dc_is_skipped (src/dsp.rs)`.

### `PhaseReport` [@ANCHOR: bench_mcp:bench_mcp_dsp_phase_report]

**Purpose and location.** What two receive channels sharing one oscillator show at the strongest tone: their phase offset and its stability, gain difference and coherence. Defined in `daemons/bench_mcp/src/dsp.rs:190` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `tone_offset_hz` | `f64` | The offset of the analysed tone from the centre frequency. |
| `phase_deg` | `f64` | Circular mean of the phase of channel A minus channel B over the blocks, in degrees (-180 to 180). |
| `phase_std_deg` | `f64` | Circular standard deviation of that phase over the blocks, in degrees: how stable the offset is. |
| `gain_db` | `f64` | Amplitude of A over amplitude of B at the tone, in dB. |
| `coherence` | `f64` | Magnitude-squared coherence at the tone (1.0 is perfectly related). |
| `blocks` | `usize` | Number of blocks averaged. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_phase_between_two_channels_and_its_stability_are_measured (src/dsp.rs)`.

### `Kind` [@ANCHOR: bench_mcp:bench_mcp_fake_scope_kind]

**Purpose and location.** How a setting of the fake scope is stored and validated (a test double of an oscilloscope's command set). Defined in `daemons/bench_mcp/src/fake_scope.rs:15` as a private `enum`. It derives Clone, Copy.

| Variant | Definition | Meaning |
|---|---|---|
| `Enum` | `Enum(&'static [&'static str])` | One of these words (compared in upper case); anything else is ignored, as the scope does. |
| `Num` | `Num(&'static str)` | A number with an optional SI prefix; stored and answered in E-notation followed by this unit. |
| `Pairs` | `Pairs` | `KEY,value,KEY,value` pairs; any subset may be set, the answer lists all keys in this order. |
| `Raw` | `Raw` | Stored as sent. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `spec` (src/fake_scope.rs:265).

**See also:** test `parses_both_drivers_with_defaults (src/config.rs)`.

### `Opts` [@ANCHOR: bench_mcp:bench_mcp_fake_scope_opts]

**Purpose and location.** What the fake scope does besides answering. Defined in `daemons/bench_mcp/src/fake_scope.rs:120` as a pub `struct`. It derives Clone, Default.

| Field | Type | Meaning |
|---|---|---|
| `stale_for` | `Duration` | New connections made before this long has passed are reset on their first line, as the real scope did while it held a dead session. |
| `stat_one_line` | `bool` | Answer `PAVA? STATn` in one line instead of the guide's two (the real unit's framing is unconfirmed). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `answer` (src/fake_scope.rs:320), `start_with` (src/fake_scope.rs:722), `new` (src/usb_hub/power.rs:150), `opts` (src/usb_hub/power.rs:431).

**See also:** test `a_new_connection_is_reset_while_it_pretends_to_hold_a_dead_session (src/fake_scope.rs)`.

### `State` [@ANCHOR: bench_mcp:bench_mcp_fake_scope_state]

**Purpose and location.** The fake scope's stored settings and bookkeeping. Defined in `daemons/bench_mcp/src/fake_scope.rs:128` as a private `struct`. It derives Clone.

| Field | Type | Meaning |
|---|---|---|
| `vals` | `HashMap<String, String>` | Text settings by key. |
| `nums` | `HashMap<String, f64>` | Numeric settings by key. |
| `custom` | `Vec<(String, String)>` | Custom (name, value) settings. |
| `frame` | `u32` | Frame counter. |
| `pf` | `(bool, u32, u32)` | The pass/fail test state as (enabled, passed count, failed count). |
| `mask` | `bool` | Whether a mask test is on. |
| `ref_saved` | `Vec<String>` | Saved reference waveforms. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `volts` (src/fake_scope.rs:188), `pava` (src/fake_scope.rs:213), `num` (src/fake_scope.rs:235), `new` (src/fake_scope.rs:240), `set` (src/fake_scope.rs:271), `answer` (src/fake_scope.rs:320), `serialise` (src/fake_scope.rs:667), `restore` (src/fake_scope.rs:678).

**See also:** test `a_scope_holding_a_dead_session_is_waited_out_for_as_long_as_connect_waits_s_says (src/main.rs)`.

### `FakeScope` [@ANCHOR: bench_mcp:bench_mcp_fake_scope_fake_scope]

**Purpose and location.** A running fake scope: the port it listens on and every line received, in order. Defined in `daemons/bench_mcp/src/fake_scope.rs:139` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `port` | `u16` | The TCP port it listens on. |
| `seen` | `Arc<Mutex<Vec<String>>>` | Every command line received, in order (shared with the serving thread under a mutex). |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `start` (src/fake_scope.rs:718), `start_with` (src/fake_scope.rs:722), `link` (src/scope_guide.rs:1160).

**See also:** test `it_answers_like_the_guide_for_identity_settings_and_units (src/fake_scope.rs)`.

### `Serial` [@ANCHOR: bench_mcp:bench_mcp_link_serial]

**Purpose and location.** A serial device at a fixed baud rate, 8 data bits, no parity, 1 stop bit, no flow control. Defined in `daemons/bench_mcp/src/link.rs:25` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `f` | `File` | The open device file. |
| `timeout` | `Duration` | The read timeout. |

**Invariants, ownership and concurrency.** Plain data: its fields hold File (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `open` (src/link.rs:31), `open_shell` (src/nanovna.rs:501).

**See also:** test `a_serial_port_that_is_not_one_or_has_an_unsupported_baud_rate_is_refused (src/link.rs)`.

### `Link` [@ANCHOR: bench_mcp:bench_mcp_link_link]

**Purpose and location.** A command and reply link to an instrument over a byte port, with a terminator and a default timeout. Defined in `daemons/bench_mcp/src/link.rs:129` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `p` | `Box<dyn Port>` | The byte port (serial or network, boxed). |
| `eol` | `&'static str` | Command terminator: `\n` for the scope, `\r\n` for the serial supply. |
| `default_timeout` | `Duration` | The default time to wait for a reply. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tcp` (src/link.rs:137), `serial` (src/link.rs:150), `new` (src/link.rs:224), `connect` (src/main.rs:342), `state` (src/psu.rs:34), `measure` (src/psu.rs:47), `set` (src/psu.rs:66), `safe_state` (src/psu.rs:96).

**See also:** test `value_handles_both_header_settings (src/link.rs)`.

### `Page` [@ANCHOR: bench_mcp:bench_mcp_manual_page]

**Purpose and location.** One page of the manual: its path, front matter fields and body. Defined in `daemons/bench_mcp/src/manual.rs:51` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `path` | `String` | Path from the root without `.md`; a directory's page has the directory's path. |
| `dir` | `String` | Directory the file is in (empty for the root). |
| `is_index` | `bool` | Whether it is a directory index page. |
| `title` | `String` | Title from the front matter. |
| `summary` | `String` | Summary from the front matter. |
| `see_also` | `Vec<String>` | Paths of related pages. |
| `tools` | `Vec<String>` | Names of tools the page documents. |
| `instruments` | `Vec<String>` | Names of instruments the page covers. |
| `tier` | `Option<String>` | The tier named in the front matter, if any. |
| `verified` | `String` | The `verified` status text of the page. |
| `body` | `String` | The page text after the front matter. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse_page` (src/manual.rs:135), `get` (src/manual.rs:234), `children_of` (src/manual.rs:243), `up_of` (src/manual.rs:272), `brief` (src/manual.rs:288), `page_text_for_resource` (src/manual.rs:1072).

**See also:** test `every_file_in_the_manual_is_a_page_with_the_required_front_matter (src/manual/tests.rs)`.

### `Manual` [@ANCHOR: bench_mcp:bench_mcp_manual_manual]

**Purpose and location.** The loaded manual: its pages, a path index, the rendered text of each page and a search index. Defined in `daemons/bench_mcp/src/manual.rs:192` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `pages` | `Vec<Page>` | All pages. |
| `by_path` | `HashMap<String, usize>` | Index from page path to position in `pages`. |
| `rendered` | `Vec<String>` | The rendered text of each page. |
| `search` | `SearchIndex` | The search index over the pages. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `manual` (src/manual.rs:202), `build` (src/manual.rs:210), `build` (src/manual.rs:515), `index` (src/manual.rs:814), `read` (src/manual.rs:842), `not_found` (src/manual.rs:884), `search` (src/manual.rs:909), `for_tool` (src/manual.rs:948).

**See also:** test `the_manual_has_the_documented_top_level (src/manual/tests.rs)`.

### `ToolInfo` [@ANCHOR: bench_mcp:bench_mcp_manual_tool_info]

**Purpose and location.** What a registered tool looks like to the manual checks: tier spelled as in the front matter, and its schema. Defined in `daemons/bench_mcp/src/manual.rs:331` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `tier` | `String` | The tier word. |
| `schema` | `Arc<Map<String, Value>>` | The tool's input schema (shared). |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `tool_registry` (src/manual.rs:336), `registered_tools` (src/manual.rs:362).

**See also:** test `every_registered_tool_has_a_page_and_every_tools_entry_names_a_registered_tool (src/manual/tests.rs)`.

### `SearchIndex` [@ANCHOR: bench_mcp:bench_mcp_manual_search_index]

**Purpose and location.** A term-weighted index of the pages for the manual search. Defined in `daemons/bench_mcp/src/manual.rs:489` as a private `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `docs` | `Vec<HashMap<String, f32>>` | Per page, the weight of each term. |
| `lens` | `Vec<f32>` | Per page, the document length. |
| `avg_len` | `f32` | The average length. |
| `df` | `HashMap<String, usize>` | How many pages each term occurs in. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build` (src/manual.rs:515).

**See also:** test `tokens_split_names_and_bm25_prefers_rarer_words (src/manual/tests.rs)`.

### `Heading` [@ANCHOR: bench_mcp:bench_mcp_manual_heading]

**Purpose and location.** A heading found in a page. Defined in `daemons/bench_mcp/src/manual.rs:645` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `level` | `usize` | The heading level (number of `#`). |
| `text` | `String` | The heading text. |
| `line` | `usize` | The line number in the page. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `headings` (src/manual.rs:651).

**See also:** test `read_returns_text_metadata_children_sections_and_cuts_long_pages (src/manual/tests.rs)`.

### `Spec` [@ANCHOR: bench_mcp:bench_mcp_manual_spec]

**Purpose and location.** The static description of one manual tool: name, description, schema properties and required arguments. Defined in `daemons/bench_mcp/src/manual.rs:724` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | The tool name. |
| `desc` | `&'static str` | The description. |
| `props` | `&'static str` | The JSON schema `properties` text. |
| `required` | `&'static [&'static str]` | The required argument names. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `the_manual_tools_are_listed_whatever_the_bench_has_and_are_read_only (src/manual/tests.rs)`.

### `Error` [@ANCHOR: bench_mcp:bench_mcp_nanovna_error]

**Purpose and location.** The failures of the NanoVNA driver, each with its own wording prefix so a caller can tell them apart. Each variant carries the message text. Defined in `daemons/bench_mcp/src/nanovna.rs:20` as a pub `enum`. It derives Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `NotSupported` | `NotSupported(String)` | The instrument cannot do this, or this driver does not do it. |
| `NotPresent` | `NotPresent(String)` | The serial port is missing or went away. |
| `Timeout` | `Timeout(String)` | No prompt within the time allowed. |
| `Protocol` | `Protocol(String)` | The reply is not what the shell protocol says. |
| `UnknownCommand` | `UnknownCommand(String)` | The shell answered `name ?`: the firmware lacks the command. |
| `Args` | `Args(String)` | A bad argument of a tool call. |
| `Io` | `Io(String)` | A read or write on the port failed. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `main` (src/main.rs:724), `from` (src/nanovna.rs:52), `parse_reply` (src/nanovna.rs:126), `command_within` (src/nanovna.rs:177), `command` (src/nanovna.rs:233), `is_wait` (src/nanovna.rs:239), `identify` (src/nanovna.rs:285), `parse_num` (src/nanovna.rs:320).

**See also:** test `replies_lose_the_echo_and_prompt_and_survive_partial_reads (src/nanovna.rs)`.

### `Config` [@ANCHOR: bench_mcp:bench_mcp_nanovna_config]

**Purpose and location.** The `nanovna` settings of an instrument entry. Defined in `daemons/bench_mcp/src/nanovna.rs:59` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `serial` | `Option<String>` | The serial device (use the /dev/serial/by-id name). |
| `min_hz` | `Option<f64>` | Override of the lowest frequency otherwise taken from the board name in `info`. |
| `max_hz` | `Option<f64>` | Override of the highest frequency. |
| `max_points` | `Option<usize>` | Override of the most points. |
| `settle_ms` | `f64` | Wait per point after a sweep is set, before the data is read (the unit sweeps continuously). |
| `use_scan` | `bool` | Use `scan` when `help` lists it (default); false always uses `sweep` and `data`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `vna_config` (src/config.rs:142), `sdr_config` (src/config.rs:181), `default` (src/nanovna.rs:73), `parse_config` (src/nanovna.rs:86), `limits` (src/nanovna.rs:273), `open_shell` (src/nanovna.rs:501), `sweep_args` (src/nanovna.rs:557), `sweep_tool` (src/nanovna.rs:588).

**See also:** test `identity_and_limits_come_from_the_shell (src/nanovna.rs)`.

### `Shell` [@ANCHOR: bench_mcp:bench_mcp_nanovna_shell]

**Purpose and location.** A conversation over a byte port with the NanoVNA's text shell. Defined in `daemons/bench_mcp/src/nanovna.rs:162` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `port` | `P` | The byte port. |
| `timeout` | `Duration` | Default time for one reply. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/nanovna.rs:169), `identify` (src/nanovna.rs:285), `open_shell` (src/nanovna.rs:501), `shell` (src/nanovna.rs:978).

**See also:** test `the_shell_works_against_a_pty_with_a_firmware_without_scan (src/nanovna.rs)`.

### `Identity` [@ANCHOR: bench_mcp:bench_mcp_nanovna_identity]

**Purpose and location.** What `version`, `info` and `help` said. Defined in `daemons/bench_mcp/src/nanovna.rs:250` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `version` | `String` | The firmware version text. |
| `board` | `Option<String>` | The board name, if `info` gave one. |
| `info` | `Vec<String>` | The lines of `info`. |
| `commands` | `Vec<String>` | The commands `help` listed. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `identify` (src/nanovna.rs:285), `sweep_args` (src/nanovna.rs:557).

**See also:** test `identity_and_limits_come_from_the_shell (src/nanovna.rs)`.

### `Limits` [@ANCHOR: bench_mcp:bench_mcp_nanovna_limits]

**Purpose and location.** The range this unit can sweep. Defined in `daemons/bench_mcp/src/nanovna.rs:259` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `min_hz` | `f64` | Lowest frequency in hertz. |
| `max_hz` | `f64` | Highest frequency in hertz. |
| `max_points` | `usize` | Most points in one sweep. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `limits` (src/nanovna.rs:273), `check_range` (src/nanovna.rs:387), `set` (src/psu.rs:66), `on_seconds` (src/psu.rs:119), `check_stored_setpoints` (src/psu.rs:138), `plan_on` (src/psu.rs:151), `lim` (src/psu.rs:255), `limits` (src/sdr.rs:109).

**See also:** test `psu_output_needs_confirmation_honours_dry_run_and_switches_off_by_the_safe_state (src/main.rs)`.

### `Limits` [@ANCHOR: bench_mcp:bench_mcp_psu_limits]

**Purpose and location.** The limits one supply may be driven to; tools refuse a request above them before any command is sent. Defined in `daemons/bench_mcp/src/psu.rs:15` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `max_volts` | `f64` | The highest voltage. |
| `max_amps` | `f64` | The highest current limit. |
| `max_on_s` | `f64` | The longest the output may stay on after one `bench_psu_output` call, in seconds. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `limits` (src/nanovna.rs:273), `check_range` (src/nanovna.rs:387), `set` (src/psu.rs:66), `on_seconds` (src/psu.rs:119), `check_stored_setpoints` (src/psu.rs:138), `plan_on` (src/psu.rs:151), `lim` (src/psu.rs:255), `limits` (src/sdr.rs:109).

**See also:** test `psu_output_needs_confirmation_honours_dry_run_and_switches_off_by_the_safe_state (src/main.rs)`.

### `Timer` [@ANCHOR: bench_mcp:bench_mcp_psu_timer]

**Purpose and location.** One supply's switch-off timer state: its instrument lock, and a counter that increases whenever an output command supersedes a pending timed switch-off. Defined in `daemons/bench_mcp/src/psu.rs:169` as a pub `struct`. It derives Clone, Default.

| Field | Type | Meaning |
|---|---|---|
| `lock` | `Arc<Mutex<()>>` | The instrument lock (shared). |
| `generation` | `Arc<AtomicU64>` | The generation counter (shared atomic); a timed switch-off that finds the counter changed does nothing. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, AtomicU64, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `psu_timer` (src/main.rs:552), `safe_state` (src/psu.rs:96).

**See also:** test `safe_state_reads_off_back_and_fails_loudly_when_the_supply_stays_on (src/psu.rs)`.

### `Analysis` [@ANCHOR: bench_mcp:bench_mcp_scope_analysis]

**Purpose and location.** The numbers taken from one scope waveform. Defined in `daemons/bench_mcp/src/scope.rs:104` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `min` | `f32` | Lowest sample. |
| `max` | `f32` | Highest sample. |
| `mean` | `f64` | Mean value. |
| `edges` | `u32` | Number of edges counted. |
| `freq` | `Option<f64>` | Frequency estimated from the edges, if there were enough. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `analyse` (src/scope.rs:114).

**See also:** test `square_wave_edges_and_frequency (src/scope.rs)`.

### `Sv` [@ANCHOR: bench_mcp:bench_mcp_scope_guide_sv]

**Purpose and location.** How one serial-trigger parameter's value is checked and written. Defined in `daemons/bench_mcp/src/scope_guide.rs:723` as a private `enum`. It derives Clone, Copy.

| Variant | Definition | Meaning |
|---|---|---|
| `Src` | `Src` | A source, `C1` or `C2`, optionally followed by `,` and a threshold in volts. |
| `Words` | `Words(&'static [&'static str])` | One of these words, or a number written as its digits. |
| `Int` | `Int(i64, i64)` | A whole number in this range. |
| `Num` | `Num(f64, f64, &'static str)` | A number in this range, sent in E-notation followed by the unit (`S` or `V`). |
| `Flag` | `Flag` | A JSON boolean, sent as ON or OFF. |
| `Bits` | `Bits` | A data pattern of `0`, `1` and `X`, up to 96 of them. |
| `Baud` | `Baud(&'static [&'static str], i64, i64)` | A baud rate: one of the standard words, or any whole number in the range (sent as `CUSTOM,<n>`). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serial_value` (src/scope_guide.rs:838).

**See also:** test `serial_and_pattern_triggers_are_set_checked_and_read_back (src/scope_guide.rs)`.

### `Ty` [@ANCHOR: bench_mcp:bench_mcp_scope_more_ty]

**Purpose and location.** How a tool argument is checked and written into a scope command. Defined in `daemons/bench_mcp/src/scope_more.rs:176` as a pub `enum`. It derives Clone, Copy.

| Variant | Definition | Meaning |
|---|---|---|
| `Flag` | `Flag` | A JSON boolean, sent as ON or OFF. |
| `Words` | `Words(&'static [&'static str])` | One of these words (compared in upper case; a whole number counts as its digits), sent as it is. |
| `Num` | `Num(f64, f64, &'static str)` | A number from the first to the second value, sent in E-notation followed by the unit (`S`, `V`, `Hz` or none). |
| `Int` | `Int(i64, i64)` | A whole number from the first to the second value. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `settings_send_the_documented_commands_read_them_back_and_refuse_bad_values_unsent (src/scope_more.rs)`.

### `Item` [@ANCHOR: bench_mcp:bench_mcp_scope_more_item]

**Purpose and location.** One setting of a scope tool: the tool argument that names it, the command it is written with (a `{c}` stands for the channel) and how the value is checked. Defined in `daemons/bench_mcp/src/scope_more.rs:188` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `arg` | `&'static str` | The tool argument name. |
| `cmd` | `&'static str` | The scope command text. |
| `ty` | `Ty` | How the value is checked. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `read_items` (src/scope_more.rs:241), `best` (src/audio/discover.rs:88).

**See also:** test `settings_send_the_documented_commands_read_them_back_and_refuse_bad_values_unsent (src/scope_more.rs)`.

### `Config` [@ANCHOR: bench_mcp:bench_mcp_sdr_config]

**Purpose and location.** Which radio, and what its tools may ask of it. Defined in `daemons/bench_mcp/src/sdr.rs:19` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `driver` | `String` | `bladerf`, `limesdr` or `rtlsdr` (the config file's driver name). |
| `serial` | `Option<String>` | Hex serial (or any prefix of it) to pick one of several boards. |
| `xb200` | `Option<String>` | bladeRF only: the XB-200 transverter setting passed to SoapySDR. |
| `direct_samp` | `Option<String>` | RTL-SDR only: direct sampling mode `1` (I branch) or `2` (Q branch). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `vna_config` (src/config.rs:142), `sdr_config` (src/config.rs:181), `default` (src/nanovna.rs:73), `parse_config` (src/nanovna.rs:86), `limits` (src/nanovna.rs:273), `open_shell` (src/nanovna.rs:501), `sweep_args` (src/nanovna.rs:557), `sweep_tool` (src/nanovna.rs:588).

**See also:** test `the_soapy_argument_string_and_config_checks (src/sdr.rs)`.

### `Limits` [@ANCHOR: bench_mcp:bench_mcp_sdr_limits_type]

**Purpose and location.** The tuning and rate limits the tools enforce before the radio is touched. Defined in `daemons/bench_mcp/src/sdr.rs:99` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `min_hz` | `f64` | Lowest tuning frequency. |
| `max_hz` | `f64` | Highest tuning frequency. |
| `min_rate` | `f64` | Lowest sample rate. |
| `max_rate` | `f64` | Highest sample rate. |
| `max_gain` | `f64` | Highest gain in dB. |
| `survey_rate` | `f64` | The sample rate a survey uses when none is given. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `limits` (src/nanovna.rs:273), `check_range` (src/nanovna.rs:387), `set` (src/psu.rs:66), `on_seconds` (src/psu.rs:119), `check_stored_setpoints` (src/psu.rs:138), `plan_on` (src/psu.rs:151), `lim` (src/psu.rs:255), `limits` (src/sdr.rs:109).

**See also:** test `psu_output_needs_confirmation_honours_dry_run_and_switches_off_by_the_safe_state (src/main.rs)`.

### `Capture` [@ANCHOR: bench_mcp:bench_mcp_sdr_capture_type]

**Purpose and location.** A tuned capture: centre frequency, sample rate, gain, length and channels. Defined in `daemons/bench_mcp/src/sdr.rs:201` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | Centre frequency in hertz. |
| `rate_hz` | `f64` | Sample rate in hertz. |
| `gain_db` | `f64` | Gain in dB. |
| `samples` | `usize` | Number of samples. |
| `channels` | `Vec<usize>` | Channel numbers. |
| `antenna` | `Option<String>` | The LimeSDR receive input (`LNAH`, `LNAL` or `LNAW`); `None` leaves the driver's default. |
| `ppm` | `f64` | RTL-SDR frequency correction in parts per million (0 leaves the dongle as it is). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `capture_args` (src/sdr.rs:245), `capture` (src/sdr.rs:509).

**See also:** test `capture_arguments_are_checked_against_each_radios_limits (src/sdr.rs)`.

### `Survey` [@ANCHOR: bench_mcp:bench_mcp_sdr_survey_type]

**Purpose and location.** A swept survey: every step is one capture, stitched into one spectrum. Defined in `daemons/bench_mcp/src/sdr.rs:272` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `start_hz` | `f64` | First frequency in hertz. |
| `stop_hz` | `f64` | Last frequency in hertz. |
| `rate_hz` | `f64` | Sample rate per step. |
| `gain_db` | `f64` | Gain in dB. |
| `fft_len` | `usize` | FFT length per step. |
| `samples_per_step` | `usize` | Samples per step. |
| `antenna` | `Option<String>` | The antenna input, if set. |
| `ppm` | `f64` | Frequency correction in ppm. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `survey_args` (src/sdr.rs:293).

**See also:** test `survey_arguments_bound_the_number_of_steps (src/sdr.rs)`.

### `Kwargs` [@ANCHOR: bench_mcp:bench_mcp_sdr_kwargs]

**Purpose and location.** The C layout of SoapySDR's `SoapySDRKwargs` (a list of key and value strings) for calls into the library. Defined in `daemons/bench_mcp/src/sdr.rs:334` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `size` | `usize` | Number of pairs. |
| `keys` | `*mut *mut c_char` | Pointer to the key strings. |
| `vals` | `*mut *mut c_char` | Pointer to the value strings. |

**Invariants, ownership and concurrency.** `#[repr(C)]`; the strings belong to the library and are freed with `kwargs_clear`. Used only inside unsafe calls.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `f_set_freq` (src/sdr.rs:1176), `f_hw_info` (src/sdr.rs:1260), `f_kwargs_clear` (src/sdr.rs:1294).

**See also:** test `opening_a_radio_passes_the_soapy_args_and_every_failure_carries_the_drivers_message (src/sdr.rs)`.

### `Dev` [@ANCHOR: bench_mcp:bench_mcp_sdr_dev]

**Purpose and location.** Opaque SoapySDR device handle (a raw pointer). Defined in `daemons/bench_mcp/src/sdr.rs:340` as a private `type`. It derives no traits.

**Definition.** `type Dev = *mut c_void;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `f_make` (src/sdr.rs:1156), `f_unmake` (src/sdr.rs:1169), `f_set_rate` (src/sdr.rs:1172), `f_set_freq` (src/sdr.rs:1176), `f_set_gain` (src/sdr.rs:1180), `f_set_ppm` (src/sdr.rs:1183), `f_set_antenna` (src/sdr.rs:1186), `f_close_stream` (src/sdr.rs:1210).

**See also:** test `opening_a_radio_passes_the_soapy_args_and_every_failure_carries_the_drivers_message (src/sdr.rs)`.

### `Stream` [@ANCHOR: bench_mcp:bench_mcp_sdr_stream]

**Purpose and location.** Opaque SoapySDR stream handle (a raw pointer). Defined in `daemons/bench_mcp/src/sdr.rs:341` as a private `type`. It derives no traits.

**Definition.** `type Stream = *mut c_void;` (a type alias, no fields of its own).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `f_close_stream` (src/sdr.rs:1210), `f_activate` (src/sdr.rs:1213), `f_deactivate` (src/sdr.rs:1217), `potential_streams` (src/usb_hub/bandwidth.rs:73), `potential_load` (src/usb_hub/bandwidth.rs:132).

**See also:** test `a_capture_tunes_streams_and_reports_the_tone_it_was_given (src/sdr.rs)`.

### `Api` [@ANCHOR: bench_mcp:bench_mcp_sdr_api_type]

**Purpose and location.** The SoapySDR functions this module calls, resolved by name from the library at run time: make and unmake a device, set rate, frequency, gain, correction and antenna, set up and close a stream, activate, deactivate and read it, query keys and info, and free memory. Defined in `daemons/bench_mcp/src/sdr.rs:343` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `make` | `unsafe extern "C" fn(*const c_char) -> Dev, unmake: unsafe extern "C" fn(Dev) -> c_int, set_rate: unsafe extern "C" fn(Dev, c_int, usize, f64) -> c_int, set_freq: unsafe extern "C" fn(Dev, c_int, usize, f64, *const Kwargs) -> c_int, set_gain: unsafe extern "C" fn(Dev, c_int, usize, f64) -> c_int, set_ppm: unsafe extern "C" fn(Dev, c_int, usize, f64) -> c_int, set_antenna: unsafe extern "C" fn(Dev, c_int, usize, *const c_char) -> c_int, setup_stream: unsafe extern "C" fn( Dev, c_int, *const c_char, *const usize, usize, *const Kwargs, ) -> Stream, close_stream: unsafe extern "C" fn(Dev, Stream) -> c_int, activate: unsafe extern "C" fn(Dev, Stream, c_int, i64, usize) -> c_int, deactivate: unsafe extern "C" fn(Dev, Stream, c_int, i64) -> c_int, read: unsafe extern "C" fn( Dev, Stream, *const *mut c_void, usize, *mut c_int, *mut i64, c_long, ) -> c_int, hw_key: unsafe extern "C" fn(Dev) -> *mut c_char, driver_key: unsafe extern "C" fn(Dev) -> *mut c_char, hw_info: unsafe extern "C" fn(Dev) -> Kwargs, num_channels: unsafe extern "C" fn(Dev, c_int) -> usize, last_error: unsafe extern "C" fn() -> *const c_char, free: unsafe extern "C" fn(*mut c_void), kwargs_clear: unsafe extern "C" fn(*mut Kwargs),` | Opens a device from its kwargs string. |

**Invariants, ownership and concurrency.** Function pointers loaded once from the SoapySDR shared library; each is `unsafe extern "C"`. The device is receive only here; nothing keys a transmitter.

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `load_api` (src/sdr.rs:380), `load_api_from` (src/sdr.rs:385), `api` (src/sdr.rs:432), `fake_api` (src/sdr.rs:442), `fake_api` (src/sdr.rs:447), `last_error` (src/sdr.rs:451), `fake_api` (src/sdr.rs:1137).

**See also:** test `a_capture_tunes_streams_and_reports_the_tone_it_was_given (src/sdr.rs)`.

### `Roots` [@ANCHOR: bench_mcp:bench_mcp_serial_guard_roots]

**Purpose and location.** The two directories the serial-device checks read, so tests can use a fake tree. Defined in `daemons/bench_mcp/src/serial_guard.rs:14` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `by_id` | `&'a Path` | The `/dev/serial/by-id` directory. |
| `sys` | `&'a Path` | The sysfs root. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `real_roots` (src/serial_guard.rs:20), `reserved_reason` (src/serial_guard.rs:82), `by_id_name` (src/serial_guard.rs:96), `check_supply` (src/serial_guard.rs:110).

**See also:** test `a_reserved_usb_port_or_by_id_path_is_found_whatever_name_the_device_has (src/serial_guard.rs)`.

### `Window` [@ANCHOR: bench_mcp:bench_mcp_spectrum_window]

**Purpose and location.** The window applied before the FFT of a waveform. Defined in `daemons/bench_mcp/src/spectrum.rs:10` as a pub `enum`. It derives Clone, Copy, Debug, PartialEq.

| Variant | Definition | Meaning |
|---|---|---|
| `Rect` | `Rect` | No window. |
| `Hanning` | `Hanning` | Hann window. |
| `Hamming` | `Hamming` | Hamming window. |
| `Blackman` | `Blackman` | Blackman window. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `parse` (src/spectrum.rs:18), `spectrum` (src/spectrum.rs:99).

**See also:** test `a_tone_is_found_at_its_frequency_and_level_in_every_window (src/spectrum.rs)`.

### `Peak` [@ANCHOR: bench_mcp:bench_mcp_spectrum_peak]

**Purpose and location.** A spectral peak of a real waveform: frequency and level in several units. Defined in `daemons/bench_mcp/src/spectrum.rs:83` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `freq_hz` | `f64` | Frequency in hertz. |
| `vrms` | `f64` | Level in volts rms. |
| `dbv` | `f64` | Level in dB relative to 1 V rms. |
| `dbm_50` | `f64` | Level in dBm into 50 ohms. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `peaks` (src/spectrum.rs:133), `peaks` (src/audio/analysis.rs:187), `dominant` (src/audio/analysis.rs:227), `find_peaks` (src/sa/sweep.rs:161), `trace_summary` (src/sa/sweep.rs:358).

**See also:** test `a_tone_is_found_at_its_frequency_and_level_in_every_window (src/spectrum.rs)`.

### `Spectrum` [@ANCHOR: bench_mcp:bench_mcp_spectrum_spectrum]

**Purpose and location.** The rms spectrum of a real waveform from 0 to Nyquist. Defined in `daemons/bench_mcp/src/spectrum.rs:90` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `bin_hz` | `f64` | Width of one bin in hertz. |
| `vrms` | `Vec<f64>` | rms amplitude of each bin from 0 to Nyquist (a sine's whole power is in the bin or bins around it; see `peaks`). |
| `enbw_bins` | `f64` | The window's noise bandwidth in bins, for turning a bin sum into power. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `welch` (src/dsp.rs:98), `noise_floor_db` (src/dsp.rs:134), `spectrum` (src/spectrum.rs:99), `peaks` (src/spectrum.rs:133), `spectrum` (src/audio/analysis.rs:140), `peaks` (src/audio/analysis.rs:187), `dominant` (src/audio/analysis.rs:227), `median_db` (src/audio/analysis.rs:232).

**See also:** test `a_tone_is_found_at_its_frequency_and_level_in_every_window (src/spectrum.rs)`.

### `TestDir` [@ANCHOR: bench_mcp:bench_mcp_testdir_test_dir]

**Purpose and location.** A scratch directory that removes itself when dropped. Defined in `daemons/bench_mcp/src/testdir.rs:17` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `path` | `PathBuf` | The directory path. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/testdir.rs:24), `vna_sweep_ok` (src/vna.rs:757).

**See also:** test `a_test_dir_is_fresh_unique_and_removed_with_its_contents_on_drop (src/testdir.rs)`.

### `SweepArgs` [@ANCHOR: bench_mcp:bench_mcp_vna_sweep_args_type]

**Purpose and location.** The validated arguments of a VNA sweep tool. Defined in `daemons/bench_mcp/src/vna.rs:17` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `start_hz` | `f64` | Start frequency in hertz. |
| `stop_hz` | `f64` | Stop frequency in hertz. |
| `points` | `usize` | Number of points. |
| `config` | `Config` | The sweep configuration for the `vnwa_open` driver. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `sweep_args` (src/vna.rs:37).

**See also:** test `sweep_arguments_are_checked_and_override_the_instrument_settings (src/vna.rs)`.

### `Hardware` [@ANCHOR: bench_mcp:bench_mcp_vna_hardware]

**Purpose and location.** The real VNWA on this machine: the production implementation of the VNA backend, which opens the USB unit and its two codecs through `vnwa_open`. A unit struct with no state. Defined in `daemons/bench_mcp/src/vna.rs:163` as a pub `struct`. It derives no traits.

**Fields.** none (unit or opaque definition).

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `measuring_a_standard_needs_a_valid_session_standard_and_cable (src/vna_tools.rs)`.

### `SweepPlan` [@ANCHOR: bench_mcp:bench_mcp_elad_measure_sweep_plan]

**Purpose and location.** What an ELAD sweep needs from its arguments, checked before the receiver is opened. Defined in `daemons/bench_mcp/src/elad/measure.rs:107` as a private `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `start` | `f64` | First frequency in hertz. |
| `stop` | `f64` | Last frequency in hertz. |
| `step` | `f64` | Step in hertz. |
| `centres` | `Vec<f64>` | The centre frequencies visited, in order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `plan` (src/elad/measure.rs:114).

**See also:** test `sweep_finds_each_tone_at_its_frequency_across_steps_and_writes_the_csv (src/elad/tests.rs)`.

### `FakeBench` [@ANCHOR: bench_mcp:bench_mcp_elad_testkit_fake_bench]

**Purpose and location.** A fake ELAD bench for tests: found devices, a synthetic tone and noise, and the transports handed out. Defined in `daemons/bench_mcp/src/elad/testkit.rs:14` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `devices` | `Vec<Found>` | The devices a scan finds. |
| `tone_hz` | `f64` | The tone: offset from the centre in Hz. |
| `amplitude` | `f64` | Tone amplitude (full scale 1.0). |
| `tones_rf` | `Vec<(f64, f64)>` | Tones at absolute RF frequencies (Hz, amplitude): when not empty the bulk endpoint is synthesised on demand from the tuning the bench tool last sent. |
| `noise_rms` | `f64` | Gaussian noise rms per component (full scale 1.0), with `tones_rf`. |
| `silent` | `bool` | When true the bulk endpoint never answers (an FDM-S2 with no FPGA image). |
| `opened` | `Mutex<Vec<FakeTransport>>` | The transports handed out, to inspect what was sent (behind a mutex). |

**Invariants, ownership and concurrency.** Plain data: its fields hold Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/elad/testkit.rs:40), `noisy_bench` (src/elad/tests.rs:384).

**See also:** test `list_reports_the_receivers_and_whether_they_stream (src/elad/tests.rs)`.

### `Synth` [@ANCHOR: bench_mcp:bench_mcp_elad_testkit_synth]

**Purpose and location.** A receiver that synthesises its samples on demand: tones at absolute RF frequencies seen through the tuning word the driver last sent, plus deterministic Gaussian noise. Control requests go to a recording `FakeTransport`. Defined in `daemons/bench_mcp/src/elad/testkit.rs:144` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `ctl` | `FakeTransport` | The recording control transport. |
| `rate` | `u32` | Sample rate. |
| `tones` | `Vec<(f64, f64)>` | The tones as (frequency, amplitude). |
| `noise` | `f64` | Noise rms. |
| `centre_hz` | `f64` | The centre frequency set by the last tuning. |
| `n` | `u64` | The count of samples produced. |
| `rng` | `u64` | The state of the deterministic random generator. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/elad/testkit.rs:155).

**See also:** test `capture_writes_cf32_and_a_summary_that_finds_the_tone (src/elad/tests.rs)`.

### `Reply` [@ANCHOR: bench_mcp:bench_mcp_example_testkit_reply]

**Purpose and location.** What the far end of the example instrument's fake does with one command. Defined in `daemons/bench_mcp/src/example/testkit.rs:18` as a pub `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Text` | `Text(String)` | Answer with this text. |
| `Silent` | `Silent` | Say nothing (a lost reply, a hung instrument). |
| `Hangup` | `Hangup` | Close the connection. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serve` (src/example/testkit.rs:32), `handle` (src/example/testkit.rs:147), `command` (src/radioctl/proto.rs:189), `exchange` (src/radioctl/proto.rs:206), `read` (src/radioctl/proto.rs:267), `dump_state` (src/radioctl/proto.rs:280), `ok` (src/radioctl/testkit.rs:26), `err` (src/radioctl/testkit.rs:31).

**See also:** test `a_rotator_that_does_not_answer_or_refuses_is_an_error_with_a_next_action_for_a_rotator (src/rotator/mod.rs)`.

### `FakeDevice` [@ANCHOR: bench_mcp:bench_mcp_example_testkit_fake_device]

**Purpose and location.** A fake example instrument listening on a local port, logging every line it received. Defined in `daemons/bench_mcp/src/example/testkit.rs:26` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `port` | `u16` | The port it listens on. |
| `log` | `Arc<Mutex<Vec<String>>>` | Every line received (shared with the serving thread under a mutex). |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `fake_supply` (src/psu.rs:236), `count` (src/psu.rs:263), `serve` (src/example/testkit.rs:32), `start` (src/example/testkit.rs:65), `sim` (src/example/testkit.rs:69), `from_transcript` (src/example/testkit.rs:75), `serve` (src/sa/testkit.rs:32), `start` (src/sa/testkit.rs:65).

**See also:** test `status_reads_the_recorded_session_and_a_stranger_is_sent_nothing (src/example/mod.rs)`.

### `Sim` [@ANCHOR: bench_mcp:bench_mcp_example_testkit_sim]

**Purpose and location.** A simulator of the example dialect, with fault switches. Defined in `daemons/bench_mcp/src/example/testkit.rs:126` as a pub `struct`. It derives Default.

| Field | Type | Meaning |
|---|---|---|
| `on` | `bool` | Whether the output is on. |
| `level` | `f64` | The programmed level. |
| `meter_reply` | `Option<String>` | The meter answers this text instead of a number. |
| `stuck_on` | `bool` | `OUTP OFF` is accepted but the output stays on. |
| `hangup_on` | `Option<String>` | The connection is closed when this command arrives. |
| `silent_queries` | `u32` | The first this many queries get no answer. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `healthy` (src/vna.rs:598), `vna_sweep_ok` (src/vna.rs:757), `sim` (src/example/testkit.rs:69), `dropping_first_replies` (src/example/testkit.rs:140), `new` (src/glasgow/testkit.rs:55), `new` (src/radioctl/testkit.rs:217), `serve_sim` (src/radioctl/testkit.rs:418), `snap` (src/radioctl/tests.rs:1476).

**See also:** test `a_silent_instrument_is_retried_a_bounded_number_of_times_then_reported_with_the_next_step (src/example/mod.rs)`.

### `Config` [@ANCHOR: bench_mcp:bench_mcp_rotator_config]

**Purpose and location.** The `rotator` settings of an instrument entry. Defined in `daemons/bench_mcp/src/rotator/mod.rs:25` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `host` | `String` | The `radioctld` rotator listener host. |
| `port` | `u16` | Its port. |
| `reply_timeout_s` | `u64` | Seconds to wait for a reply. |
| `min_az` | `Option<f64>` | The owner's lowest azimuth in degrees; narrows what the rotator reports. `None` uses the rotator's own. |
| `max_az` | `Option<f64>` | The highest azimuth in degrees. |
| `min_el` | `Option<f64>` | The lowest elevation in degrees. |
| `max_el` | `Option<f64>` | The highest elevation in degrees. |
| `tolerance_deg` | `f64` | How close in degrees the reported position must be to the target for a move to count as arrived. |
| `max_wait_s` | `f64` | The longest a call waits for a move, in seconds. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `vna_config` (src/config.rs:142), `sdr_config` (src/config.rs:181), `default` (src/nanovna.rs:73), `parse_config` (src/nanovna.rs:86), `limits` (src/nanovna.rs:273), `open_shell` (src/nanovna.rs:501), `sweep_args` (src/nanovna.rs:557), `sweep_tool` (src/nanovna.rs:588).

**See also:** test `a_dongle_with_the_codec_id_is_reserved_only_beside_the_vnwa_control_chip (src/audio/devices.rs)`.

### `Spec` [@ANCHOR: bench_mcp:bench_mcp_rotator_spec]

**Purpose and location.** The static description of one rotator tool: name, tier, description, schema properties and required arguments. Defined in `daemons/bench_mcp/src/rotator/mod.rs:131` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `name` | `&'static str` | The tool name. |
| `tier` | `Tier` | The tier it needs. |
| `desc` | `&'static str` | The description. |
| `props` | `&'static str` | The schema `properties` text. |
| `required` | `&'static [&'static str]` | The required argument names. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `every_tool_schema_is_valid_json (src/rotator/mod.rs)`.

### `Seen` [@ANCHOR: bench_mcp:bench_mcp_rx888_testkit_seen]

**Purpose and location.** One command the fake radiod received. Defined in `daemons/bench_mcp/src/rx888/testkit.rs:25` as a pub `struct`. It derives Clone, Debug, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `ssrc` | `u32` | The channel's SSRC. |
| `lifetime` | `Option<u64>` | The lifetime value sent, if any. |
| `demod` | `Option<u64>` | The demodulator type sent, if any. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `probe` (src/usb_hub/power.rs:61).

**See also:** test `tune_reports_levels_snr_and_rate_and_removes_its_channel (src/rx888/mod.rs)`.

### `Chan` [@ANCHOR: bench_mcp:bench_mcp_rx888_testkit_chan]

**Purpose and location.** One channel of the fake radiod as it was last configured. Defined in `daemons/bench_mcp/src/rx888/testkit.rs:32` as a private `struct`. It derives Clone, Debug, Default.

| Field | Type | Meaning |
|---|---|---|
| `freq` | `f64` | Centre frequency in hertz. |
| `lo` | `f32` | Low edge of the passband. |
| `hi` | `f32` | High edge of the passband. |
| `channels` | `u64` | Number of output channels. |
| `agc` | `bool` | Whether AGC is on. |
| `gain` | `f32` | Gain setting. |
| `demod` | `u64` | Demodulator type. |
| `bins` | `u64` | Number of spectrum bins. |
| `bin_hz` | `f32` | Width of a spectrum bin in hertz. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `tune_reports_levels_snr_and_rate_and_removes_its_channel (src/rx888/mod.rs)`.

### `FakeRadiod` [@ANCHOR: bench_mcp:bench_mcp_rx888_testkit_fake_radiod]

**Purpose and location.** A fake ka9q-radio `radiod` for tests: multicast groups and ports, and a record of the commands received. Defined in `daemons/bench_mcp/src/rx888/testkit.rs:44` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `status_group` | `Ipv4Addr` | The status multicast group. |
| `data_group` | `Ipv4Addr` | The data multicast group. |
| `status_port` | `u16` | The status port. |
| `data_port` | `u16` | The data port. |
| `seen` | `Arc<Mutex<Vec<Seen>>>` | The commands received (shared under a mutex). |
| `stop` | `Arc<AtomicBool>` | Flag that ends the serving thread. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, AtomicBool, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `start` (src/rx888/testkit.rs:94).

**See also:** test `status_and_frontend_read_the_usb_tree_and_the_front_end (src/rx888/mod.rs)`.

### `Reply` [@ANCHOR: bench_mcp:bench_mcp_sa_testkit_reply]

**Purpose and location.** What the far end of the spectrum analyzer fake does with one command. Defined in `daemons/bench_mcp/src/sa/testkit.rs:18` as a pub `enum`. It derives no traits.

| Variant | Definition | Meaning |
|---|---|---|
| `Text` | `Text(String)` | Answer with this text. |
| `Silent` | `Silent` | Say nothing (a lost reply, a hung instrument). |
| `Hangup` | `Hangup` | Close the connection. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `serve` (src/example/testkit.rs:32), `handle` (src/example/testkit.rs:147), `command` (src/radioctl/proto.rs:189), `exchange` (src/radioctl/proto.rs:206), `read` (src/radioctl/proto.rs:267), `dump_state` (src/radioctl/proto.rs:280), `ok` (src/radioctl/testkit.rs:26), `err` (src/radioctl/testkit.rs:31).

**See also:** test `a_rotator_that_does_not_answer_or_refuses_is_an_error_with_a_next_action_for_a_rotator (src/rotator/mod.rs)`.

### `FakeDevice` [@ANCHOR: bench_mcp:bench_mcp_sa_testkit_fake_device]

**Purpose and location.** A fake SSA3000X listening on a local port, logging every line it received. Defined in `daemons/bench_mcp/src/sa/testkit.rs:26` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `port` | `u16` | The port it listens on. |
| `log` | `Arc<Mutex<Vec<String>>>` | Every line received (shared with the serving thread under a mutex). |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `fake_supply` (src/psu.rs:236), `count` (src/psu.rs:263), `serve` (src/example/testkit.rs:32), `start` (src/example/testkit.rs:65), `sim` (src/example/testkit.rs:69), `from_transcript` (src/example/testkit.rs:75), `serve` (src/sa/testkit.rs:32), `start` (src/sa/testkit.rs:65).

**See also:** test `psu_output_needs_confirmation_honours_dry_run_and_switches_off_by_the_safe_state (src/main.rs)`.

### `Sim` [@ANCHOR: bench_mcp:bench_mcp_sa_testkit_sim]

**Purpose and location.** A simulator of the SSA3000X dialect, with fault switches. It keeps the settings it is given, rounds the RBW up like the unit, clamps the stop frequency to 2.1 GHz and makes a 751-point trace. Defined in `daemons/bench_mcp/src/sa/testkit.rs:117` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `on` | `bool` | Whether the tracking generator is on. |
| `level` | `f64` | The generator level. |
| `start` | `f64` | Start frequency. |
| `stop` | `f64` | Stop frequency. |
| `clamp_stop` | `f64` | The highest stop frequency the simulated unit accepts (it clamps to it). |
| `rbw` | `f64` | Resolution bandwidth. |
| `vbw` | `f64` | Video bandwidth. |
| `ref_level` | `f64` | Reference level. |
| `atten` | `f64` | Attenuation. |
| `detector` | `String` | Detector name. |
| `continuous` | `bool` | Whether sweeping is continuous. |
| `sweep_time` | `f64` | Sweep time. |
| `trace` | `Vec<f64>` | The trace points. |
| `marker` | `(f64, f64)` | The marker as (frequency, level). |
| `dut_db` | `std::sync::Arc<Mutex<f64>>` | The loss of the "device under test" between the TG output and the RF input, dB; shared so a test can change it. |
| `stuck_on` | `bool` | `OUTPut:STATe OFF` is accepted but the output stays on. |
| `hangup_on` | `Option<String>` | The connection is closed when this command arrives. |
| `silent_queries` | `u32` | The first this many queries get no answer. |
| `garbage` | `Option<(String, String)>` | This query gets this reply instead of the true one, as (query, reply). |
| `options` | `String` | The options reply. |

**Invariants, ownership and concurrency.** Plain data: its fields hold Arc, Mutex (shared or owned resources), so it is not a pure value. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `healthy` (src/vna.rs:598), `vna_sweep_ok` (src/vna.rs:757), `sim` (src/example/testkit.rs:69), `dropping_first_replies` (src/example/testkit.rs:140), `new` (src/glasgow/testkit.rs:55), `new` (src/radioctl/testkit.rs:217), `serve_sim` (src/radioctl/testkit.rs:418), `snap` (src/radioctl/tests.rs:1476).

**See also:** test `tools_are_listed_by_tier_and_refused_below_it_before_any_connection (src/sa/mod.rs)`.
