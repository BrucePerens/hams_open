# radiod_client

A receive-only Rust client for the multicast control and status protocol of
[ka9q-radio](https://github.com/ka9q/ka9q-radio) `radiod`, the multichannel receive engine planned for the
shared receiver service (`hams_com/docs/proposals/SDR_RECEIVE_ENGINE_AND_TRANSMIT_PATH.md`). Prototype from the
radiod trial; findings and numbers are in `hams_com/docs/reviews/RADIOD_TRIAL_2026-10-30.md`.

* `tlv`: bounds-checked parser and a crate-private writer for the type/length/value format.
* `command`: the **closed command schema**. Typed audio and spectrum channels, validated against a `Policy`
  (frequency range, bin count, bin width, averaging, lifetime). Every channel command carries a non-zero
  lifetime (a bare poll of an unknown SSRC (the RTP stream identifier) makes `radiod` create an immortal channel). A spectrum request never exceeds 16,000 float or 60,000 byte bins whatever the policy
  says (larger requests segfault `radiod`). There is no field for a
  data destination, no raw-bytes entry point, and nothing that touches the front end.
* `status`: decoded channel status, including float and byte spectrum bins.
* `rtp`: RTP (Real-time Transport Protocol) header parsing, loss accounting, 16-bit PCM (pulse-code modulation, plain digital audio samples) and Opus decoding.
* `net`: tokio multicast sockets (TTL 0 (time to live zero, so packets stay on the local machine), a named interface), request with tag acknowledgement. No runtime
  of its own: the caller passes a `tokio::runtime::Handle` when a socket is opened, and the acknowledgement
  rules (`next_tag`, `is_ack`, `classify`) are sans-IO.

Tested against `radiod` commit 8c50dbbe9958c59a24e392fa1785501350e833f9 with its `sig_gen` pseudo front end, on
loopback in a private network namespace. Not wired into the relay. The protocol is not authenticated: run `radiod`
on loopback or in a namespace and let this client be its only sender (proposal section 11).

Licence: AGPL-3.0-or-later. The toolchain is pinned in `rust-toolchain.toml` (as for `daemons/radioctl`).
Build and test: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`.
The `radiod_probe` trial driver stayed in the trial scratch copy: it needed a runtime of its own.
The tests open real multicast sockets on `127.0.0.1` (TTL 0); nothing leaves the host and no `radiod` is needed.

## Known `radiod` hazards the tests pin

* A spectrum request over 16,000 float or 60,000 byte bins crashes `radiod` (stack overflow); refused by the
  client whatever the `Policy` says (`oversized_bin_requests_are_refused_even_by_a_permissive_policy`).
* Any controller can move a channel's RTP stream to any address with `OUTPUT_DATA_DEST_SOCKET`; no command
  here can carry it (`encoded_command_never_names_a_destination`).
* A bare poll of an unknown or expired SSRC creates an immortal channel at 0 Hz; every poll carries a lifetime
  of at least one frame and a zero lifetime is refused before anything is sent
  (`a_bare_unknown_ssrc_poll_cannot_be_built`, `refused_commands_never_reach_the_wire`).

## Anchors

Anchors (ADR 0090): a function carries `// [@ANCHOR: id]` in source, its test carries
`// Tests [@ANCHOR: id]`, and this README cites it as `[@ANCHOR: radiod_client:id]` below.
The tag is the crate name, a colon and the function id; the code form drops the crate prefix. A README entry is a
`###` heading that carries the tag, followed by what the function does, its arguments, what it returns, its errors,
its async and locking behaviour, and the tests that cite it, so a reader never has to open the source to use it.

Each entry below documents one function or constant of the crate from its source in `src/`. Every function is
synchronous and pure unless marked async; the async ones run on the caller's tokio runtime. Where the
behaviour in the RX-888 sections further down (from "RX-888 additions") covers the same anchor, that section
holds the full table and this part holds the rest.

### `Ssrc::new(v: u32) -> Result<Ssrc, CommandError>` [@ANCHOR: radiod_client:radiod_client_command_ssrc_new]

The only way to build an `Ssrc`, the channel identifier of a command (`src/command.rs`). `v` is a number the
gateway chose. `0` addresses `radiod`'s front end and `0xffff_ffff` means "all channels" to `radiod`, so both are
refused with `CommandError::ReservedSsrc(v)`; every other value gives `Ok`. Pure, no I/O, no locking. Because
the tuple field is private, a `Command` can never name the front end or every channel by accident (the one
front-end request is `encode_front_end_poll`, which does not use an `Ssrc`). Test: `reserved_ssrcs_are_not_constructible`.

### `Ssrc::get(self) -> u32` [@ANCHOR: radiod_client:radiod_client_command_ssrc_get]

Returns the number inside an `Ssrc`. `Ssrc` is `Copy`, so it is taken by value, and the call cannot fail. The
value is what `Command::encode` writes as `OUTPUT_SSRC` and what `Control::request` compares with the SSRC of
a received status (`is_ack`). It is never 0 or `u32::MAX`, because `Ssrc::new` refused those. Pure.
Test: `reserved_ssrcs_are_not_constructible`.

### `Policy::default() -> Policy` [@ANCHOR: radiod_client:radiod_client_command_policy_default]

The site policy used when the caller does not choose one: the limits `Command::validate` checks before anything
reaches the wire. Constant data, no side effects.

| Field | Default | Meaning |
|---|---|---|
| `min_hz`, `max_hz` | 10,000.0 and 30,000,000.0 Hz (`f64`) | Allowed carrier or centre frequency, inclusive |
| `max_bins` | 8,192 | Spectrum bin count; can only lower the hard ceilings (16,000 float, 60,000 byte) |
| `min_bin_hz` | 1.0 Hz (`f32`) | Narrowest spectrum bin |
| `max_lifetime_frames` | 15,000 | Longest channel lifetime: 5 minutes of 20 ms frames |
| `max_averaging` | 64 | Most FFTs averaged per response |
| `max_span_hz` | `None` | No span check until the front end is known (see `max_spectrum_span_hz`) |

Test: `default_policy_is_inside_the_hard_ceilings`.

### `impl Display for CommandError` (`fmt(&self, f) -> fmt::Result`) [@ANCHOR: radiod_client:radiod_client_command_error_fmt]

The text of a refused command. It writes the `Debug` form of the variant, for example `Lifetime(0)` or
`Frequency(NaN)`, so the message carries the offending value but no prose; callers that want a sentence map the
variant themselves. `CommandError` also implements `std::error::Error`, so it works with `?` and `Box<dyn Error>`.
Pure and allocation-free apart from the formatter. Tests: `errors_display`.

### `check_freq(p: &Policy, f: f64) -> Result<(), CommandError>` [@ANCHOR: radiod_client:radiod_client_command_check_freq]

Private frequency rule used by `Command::validate` for the carrier of an audio channel and the centre of a
spectrum channel. `Ok(())` when `f` is finite and lies in `p.min_hz..=p.max_hz` (both ends allowed); otherwise
`Err(CommandError::Frequency(f))`. `NaN` and infinities are refused because every comparison with them is
false or they are not finite. Pure. It keeps a gateway from steering the receiver outside the band the site
allows. Test: `rejects_unsafe_values`.

### `check_life(p: &Policy, l: u32) -> Result<(), CommandError>` [@ANCHOR: radiod_client:radiod_client_command_check_life]

Private lifetime rule, in 20 ms frames, applied to every command that creates or touches a channel. `Ok(())`
for `1..=p.max_lifetime_frames`; `Err(CommandError::Lifetime(l))` for 0 or anything above. Zero is refused on
purpose: `radiod` treats lifetime 0 as "never expires" (its default), so a channel made with it would live
until `radiod` restarts. The value is later sent as `LIFETIME`. Pure. Test: `rejects_unsafe_values`.

### `Policy::bin_limit(&self, byte_format: bool) -> u32` [@ANCHOR: radiod_client:radiod_client_command_policy_bin_limit]

The largest spectrum bin count this policy allows for a format: `min(max_bins, hard)` where `hard` is
`MAX_BYTE_BINS` (60,000) when `byte_format` is true and `MAX_FLOAT_BINS` (16,000) otherwise. The hard ceilings
exist because `radiod` builds a spectrum reply in a 65,536-byte stack buffer and crashes when it is overrun
(17,000 float bins and 66,000 byte bins crashed it in the trial), so a permissive site policy cannot raise
the limit. Pure, cannot fail. Test: `oversized_bin_requests_are_refused_even_by_a_permissive_policy`.

### `Command::ssrc(&self) -> Ssrc` [@ANCHOR: radiod_client:radiod_client_command_ssrc]

The channel a command addresses: the `ssrc` field of `Audio`, `Spectrum`, `Poll`, `Filter` and `Gain`, and the
payload of `Remove`. Every variant has one, so the function always returns a value and cannot fail. Pure,
by reference, no copy beyond the small `Ssrc`. `Command::encode` writes it as `OUTPUT_SSRC` and
`Control::request` uses it to recognise the acknowledgement. Test: `every_command_names_its_channel`.

### `Command::validate(&self, p: &Policy) -> Result<(), CommandError>` [@ANCHOR: radiod_client:radiod_client_command_validate]

Checks a command against the site policy without building bytes; `encode` and `Control::send` call it first.
It returns the first failing rule, in the order shown, and `Ok(())` when none fails. Pure.

| Variant | Checks, in order | Error |
|---|---|---|
| `Audio` | frequency (`check_freq`); lifetime; Opus bit rate 6,000 to 128,000; bandwidth, when set, within 100 to 24,000 Hz and edges acceptable to `check_filter` | `Frequency`, `Lifetime`, `Bitrate`, `Bandwidth` |
| `Spectrum` | centre frequency; lifetime; bin count 1 to `bin_limit`; bin width finite and at least `min_bin_hz`; averaging 1 to `max_averaging`; span (bins times width) within `max_span_hz` when it is set | `Frequency`, `Lifetime`, `BinCount`, `BinWidth`, `Averaging`, `Span` |
| `Poll` | lifetime | `Lifetime` |
| `Remove` | none | always `Ok` |
| `Filter` | lifetime; edges (`check_filter`) | `Lifetime`, `Filter` |
| `Gain` | lifetime; a fixed gain finite and inside `GAIN_RANGE_DB`; `None` (AGC) is always fine | `Lifetime`, `Gain` |

Tests: `rejects_unsafe_values`, `spectrum_limits`, `a_bandwidth_outside_the_limits_is_refused_before_the_wire`,
`a_spectrum_wider_than_the_front_end_can_fill_is_refused_when_the_policy_knows_it`.

### `Command::encode(&self, p: &Policy, tag: u32) -> Result<Vec<u8>, CommandError>` [@ANCHOR: radiod_client:radiod_client_command_encode]

Validates the command, then builds the one datagram that carries it: a command packet (kind byte 1) of TLV
entries ending with the end marker. `tag` goes in `COMMAND_TAG`; `radiod` echoes it in the status it replies
with, which is the only acknowledgement it gives. Errors are those of `validate`; nothing is built when it
fails. Pure; the datagram is never sent here (see `Control::send`). Every datagram starts with `OUTPUT_SSRC` and
`COMMAND_TAG`, then:

| Variant | Entries written after those two |
|---|---|
| `Poll` | `LIFETIME` |
| `Remove` | `LIFETIME` 1 (expires at the next frame) |
| `Filter` | `LIFETIME`, `LOW_EDGE`, `HIGH_EDGE` |
| `Gain` | `LIFETIME`, then `AGC_ENABLE` 0 and `GAIN` for a fixed gain, or `AGC_ENABLE` 1 |
| `Audio` | `LIFETIME`, `RADIO_FREQUENCY`, `OUTPUT_CHANNELS` (2 for `Iq`, else 1), the mode's demodulator parameters, `OUTPUT_ENCODING` (2 for 16-bit PCM, 3 for Opus plus `OPUS_BIT_RATE`) |
| `Spectrum` | `LIFETIME`, `DEMOD_TYPE` (3 float bins, 4 byte bins), `RADIO_FREQUENCY`, `BIN_COUNT`, `RESOLUTION_BW`, `SPECTRUM_AVG` |

Security: no variant can write `OUTPUT_DATA_DEST_SOCKET` or any front end entry; the tests
`encoded_command_never_names_a_destination` and `no_public_path_takes_caller_bytes` pin that. Other tests:
`audio_command_carries_frequency_and_lifetime`, `a_bare_unknown_ssrc_poll_cannot_be_built`.

### `impl Display for NetError` (`fmt(&self, f) -> fmt::Result`) [@ANCHOR: radiod_client:radiod_client_net_error_fmt]

The text of a network or protocol failure (`src/net.rs`), one prefix per variant: `io: <error>` for a socket
failure (`Io`), `command: <error>` for a command the policy refused (`Command`), `tlv: <error>` for a malformed
datagram (`Tlv`), and `<address> is not a multicast address` for `NotMulticast`. The wrapped error's own
`Display` supplies the rest. Pure. `NetError` also implements `std::error::Error`. Test: `errors_display`.

### `impl From<io::Error> for NetError` (`from_io(e: io::Error) -> NetError`) [@ANCHOR: radiod_client:radiod_client_net_error_from_io]

Wraps an operating system I/O error as `NetError::Io(e)` without losing its kind or message. It exists so the
socket calls in `open_group`, `register`, `send` and `recv` can use `?` and still return a `NetError`. Pure,
cannot fail. The other `NetError` variants are built explicitly: `Command` by `send`, `NotMulticast` by
`open_group`, and `Tlv` is defined for decoding failures but the receive helpers count and skip those instead of
returning it. Test: `errors_display`.

### `next_tag(tag: u32) -> u32` [@ANCHOR: radiod_client:radiod_client_net_next_tag]

The command tag that follows `tag`. It adds 1 with wrap-around and never returns 0, so `u32::MAX` goes to 1. A
zero tag is written as an empty integer on the wire and could not be told from a missing tag, which would make
an acknowledgement ambiguous. `Control` starts at 1 and calls this after each datagram. Pure, sans-IO, no
errors. Test: `acknowledgement_rules_are_pure`.

### `is_ack(status: &ChannelStatus, ssrc: u32, tag: u32) -> bool` [@ANCHOR: radiod_client:radiod_client_net_is_ack]

True when `status` answers the command sent to channel `ssrc` with command tag `tag`: the status's `ssrc`
equals `ssrc` and its echoed `tag` is `Some(tag)`. A status with no tag, or another channel's, gives false.
Pure, sans-IO. It is the only acknowledgement test for channel commands; a front-end reply carries no tag, so
`is_front_end` is used for it. `Control::request` calls it for every tag it has sent. Test:
`acknowledgement_rules_are_pure`.

### `classify(datagram: &[u8], malformed: &mut u64) -> Option<ChannelStatus>` [@ANCHOR: radiod_client:radiod_client_net_classify]

Turns one received datagram into a status for the receive loop. A status decodes to `Some`. A command packet
(our own, looped back by multicast) is skipped: `None`, nothing counted. A datagram `ChannelStatus::decode`
rejects is skipped too, and `*malformed` is increased by one so the damage stays visible. It never panics on
hostile input. Pure, sans-IO; `Control::recv_status` passes its public `malformed` field. Test:
`acknowledgement_rules_are_pure`.

### `open_group(group: Ipv4Addr, port: u16, iface: Ipv4Addr) -> Result<std::net::UdpSocket, NetError>` [@ANCHOR: radiod_client:radiod_client_net_open_group]

Private. Builds the non-blocking multicast receive socket both `Control` and `RtpSocket` use.

| Argument | Meaning |
|---|---|
| `group` | IPv4 multicast group; anything else gives `NetError::NotMulticast(group)` |
| `port` | UDP port to bind, on all addresses |
| `iface` | Local interface address to join on (`127.0.0.1` for loopback) |

It sets address and port reuse, binds, joins the group on `iface`, sets the outgoing interface, TTL 0 (so
nothing it sends leaves the host), multicast loopback on, and a 4 MiB receive buffer (the kernel may clamp it).
Any failing socket call returns `NetError::Io`. Synchronous. Security: the protocol has no authentication and the
bind accepts datagrams from any sender, so `radiod` runs on loopback or in a namespace. Test:
`only_multicast_groups_open`.

### `register(handle: &Handle, sock: std::net::UdpSocket) -> Result<UdpSocket, NetError>` [@ANCHOR: radiod_client:radiod_client_net_register]

Private. Hands a standard socket to the tokio runtime behind `handle` and returns the async `UdpSocket`. It
enters the handle's context for the call, because `UdpSocket::from_std` needs a runtime context to find the I/O
driver, and leaves it again at once. The crate owns no runtime, so every socket goes through here. The socket must
already be non-blocking (`open_group` sets that). Errors: `NetError::Io`. Synchronous; no task is spawned.
Test: `refused_commands_never_reach_the_wire`.

### `Control::open(handle: &Handle, group: Ipv4Addr, iface: Ipv4Addr, policy: Policy) -> Result<Control, NetError>` [@ANCHOR: radiod_client:radiod_client_net_control_open]

Opens the control and status link to one `radiod`: joins `group` on port `STATUS_PORT` (5006), registers the
socket with the runtime behind `handle`, and keeps `policy` for every later `send`. Commands go to the same
group and port the status arrives on. Returns a `Control` whose tag counter starts at 1 and whose public
`malformed` counter is 0. Errors: `NotMulticast` for a non-multicast group and `Io` for socket failures.
Synchronous, but `handle` must belong to a running runtime. It is the same as `open_on_port` with the default
port. Test: `only_multicast_groups_open`.

### `Control::open_on_port(handle: &Handle, group: Ipv4Addr, port: u16, iface: Ipv4Addr, policy: Policy) -> Result<Control, NetError>` [@ANCHOR: radiod_client:radiod_client_net_control_open_on_port]

The same as `Control::open` with the status port given, for a `radiod` configured for a port other than 5006.
`port` is both the port bound for receiving and the destination port of every command. Arguments, return value
and errors are as for `open`. It is where the work is done: `open_group` builds the socket, `register` makes it
async, and the struct is filled in. No locking; a `Control` belongs to one task. Test: `refused_commands_never_reach_the_wire`.

### `Control::send(&mut self, cmd: &Command) -> Result<u32, NetError>` (async) [@ANCHOR: radiod_client:radiod_client_net_control_send]

Validates `cmd` against the policy given at open, encodes it with the next tag, and sends one datagram to the
group. Returns the tag, which `radiod` will echo in its status reply. A refused command returns
`NetError::Command` and puts nothing on the wire, and the tag counter does not move; a failed send returns
`NetError::Io` (the tag is already used). It does not wait for an answer: use `request` for that. `&mut self`, so
one send at a time per `Control`. Tests: `refused_commands_never_reach_the_wire`, `request_is_acknowledged_by_tag`.

### `Control::recv_status(&mut self, wait: Duration) -> Result<Option<ChannelStatus>, NetError>` (async) [@ANCHOR: radiod_client:radiod_client_net_control_recv_status]

Waits up to `wait` for the next status datagram and returns it. Datagrams that are our own looped-back commands
are skipped, and malformed ones are skipped and counted in `self.malformed`; the wait keeps its original
deadline across skips. Returns `Ok(None)` when the time runs out and `Err(NetError::Io)` on a socket error.
Each call allocates a 64 KiB buffer (the largest datagram `radiod` sends). The source address is not checked,
so any host on the group is believed. Async on the caller's runtime. Tests: `request_is_acknowledged_by_tag`,
`unanswered_request_times_out_and_retries`.

### `Control::request(&mut self, cmd: &Command, wait: Duration) -> Result<Option<ChannelStatus>, NetError>` (async) [@ANCHOR: radiod_client:radiod_client_net_control_request]

Sends `cmd` and waits for the status that carries its tag, which is the only acknowledgement `radiod` gives. It
repeats the send every 200 ms until `wait` has passed; sends are idempotent, so a reply to any tag this call has
sent (not only the newest) counts, and a slow reply is not lost. Statuses that do not match, including other
clients' channels, are discarded. Returns `Ok(Some(status))` on acknowledgement, `Ok(None)` on timeout, and
`NetError::Command` or `Io` for a refused command or socket failure. `radiod` may therefore receive the
command several times. Tests: `request_is_acknowledged_by_tag`,
`a_reply_slower_than_the_retry_interval_still_acknowledges`, `unanswered_request_times_out_and_retries`.

### `RtpSocket::open(handle: &Handle, group: Ipv4Addr, iface: Ipv4Addr) -> Result<RtpSocket, NetError>` [@ANCHOR: radiod_client:radiod_client_net_rtp_open]

Opens a receiver for `radiod`'s audio data: joins the data `group` on port `RTP_PORT` (5004) on `iface` and
registers the socket with the runtime behind `handle`. The socket only receives; this type has no send method.
Errors: `NotMulticast` for a non-multicast group, `Io` for socket failures. Synchronous. It is
`open_on_port` with the default port. The `group` is the data destination `radiod` was configured with, learned
from channel status (`data_dest`), not chosen by this client. Test: `only_multicast_groups_open`.

### `RtpSocket::open_on_port(handle: &Handle, group: Ipv4Addr, port: u16, iface: Ipv4Addr) -> Result<RtpSocket, NetError>` [@ANCHOR: radiod_client:radiod_client_net_rtp_open_on_port]

As `RtpSocket::open` for a `radiod` whose data port is not 5004. `port` is the UDP port bound. The multicast
settings are those of `open_group`: TTL 0, 4 MiB receive buffer, loopback on. Errors are `NotMulticast` and `Io`.
Synchronous and without side effects beyond the socket joining the group. Test: `rtp_socket_receives_and_times_out`.

### `RtpSocket::recv(&self, buf: &mut [u8], wait: Duration) -> Result<Option<usize>, NetError>` (async) [@ANCHOR: radiod_client:radiod_client_net_rtp_recv]

Receives one datagram into `buf` and returns its length, or `Ok(None)` when `wait` passes with nothing
received. A datagram longer than `buf` is cut to fit (the kernel discards the rest); size `buf` for the largest
packet expected. Errors: `NetError::Io`. Takes `&self`. No parsing happens here: pass the bytes to `RtpPacket::parse`. Test:
`rtp_socket_receives_and_times_out`.
- [@ANCHOR: radiod_client:radiod_client_rtp_error_fmt] (rtp) Display text of an RTP rejection.
- [@ANCHOR: radiod_client:radiod_client_rtp_packet_parse] (rtp) Parse a datagram.
- [@ANCHOR: radiod_client:radiod_client_rtp_encoding_from_radiod] (rtp) Map `radiod`'s encoding number.
- [@ANCHOR: radiod_client:radiod_client_rtp_decoder_debug_fmt] (rtp) Debug text of an audio decoder (encoding and channels, never the codec state).
- [@ANCHOR: radiod_client:radiod_client_rtp_decoder_new] (rtp) Build a decoder from the channel status (`channels` for PCM; Opus is always 2).
- [@ANCHOR: radiod_client:radiod_client_rtp_decoder_decode] (rtp) Decode a payload to mono samples in -1.0..=1.0.
- [@ANCHOR: radiod_client:radiod_client_rtp_seq_push] (rtp) `SeqTracker::push(&mut self, seq: u16)`: account for one packet in the counters `received`, `lost`, `out_of_order` and `restarts` (all `u64`). One ahead is in order; 2 to 32767 ahead adds the skipped packets to `lost`; the same number, or at most `REORDER_WINDOW` (32) behind the newest, is `out_of_order` and leaves the newest unchanged; further back (including exactly 32768) is a stream restart: `restarts` grows by one, nothing is added to `lost` (the new numbering says nothing about how many old packets were missed), and tracking follows the new sequence so the packets after it are in order. `radiod` re-creating a channel restarts its sequence (200 lower on the RX-888, 2026-11-05); before this the whole new stream counted as out of order and the outage showed nowhere. A restart that lands ahead of the old sequence counts as loss.
- [@ANCHOR: radiod_client:radiod_client_status_decode] (status) Decode a datagram.
- [@ANCHOR: radiod_client:radiod_client_status_spectrum_db] (status) Spectrum bins in ascending frequency order with their frequencies, in dB.
- [@ANCHOR: radiod_client:radiod_client_tlv_error_fmt] (tlv) Display text of a rejected datagram.
- [@ANCHOR: radiod_client:radiod_client_tlv_entry_uint] (tlv) Unsigned integer (up to 8 bytes, big-endian, zero length is 0).
- [@ANCHOR: radiod_client:radiod_client_tlv_entry_bool] (tlv) Boolean (any non-zero integer).
- [@ANCHOR: radiod_client:radiod_client_tlv_entry_f32] (tlv) 32-bit float; a value longer than four bytes is read as a double, as `radiod` does.
- [@ANCHOR: radiod_client:radiod_client_tlv_entry_f64] (tlv) 64-bit float; a value of four bytes or fewer is read as a float.
- [@ANCHOR: radiod_client:radiod_client_tlv_entry_text] (tlv) UTF-8 text (lossy, so a hostile description cannot fail the parse).
- [@ANCHOR: radiod_client:radiod_client_tlv_entry_socket] (tlv) Socket: 6 bytes IPv4 plus port, 18 bytes IPv6 plus port, or empty (null socket).
- [@ANCHOR: radiod_client:radiod_client_tlv_entry_f32_vec] (tlv) A vector of big-endian `f32` (`BIN_DATA`).
- [@ANCHOR: radiod_client:radiod_client_tlv_parse] (tlv) Parse a datagram into its kind and entries.
- [@ANCHOR: radiod_client:radiod_client_tlv_writer_command] (tlv) Starts a command datagram (crate-private).
- [@ANCHOR: radiod_client:radiod_client_tlv_writer_header] (tlv) Writes one entry header (crate-private).
- [@ANCHOR: radiod_client:radiod_client_tlv_writer_uint] (tlv) Writes an integer with leading zero bytes removed (crate-private).
- [@ANCHOR: radiod_client:radiod_client_tlv_writer_bool] (tlv) Writes a boolean as an integer (crate-private).
- [@ANCHOR: radiod_client:radiod_client_tlv_writer_f32] (tlv) Writes a big-endian 32-bit float (crate-private).
- [@ANCHOR: radiod_client:radiod_client_tlv_writer_f64] (tlv) Writes a big-endian 64-bit float (crate-private).
- [@ANCHOR: radiod_client:radiod_client_tlv_writer_finish] (tlv) Terminates the datagram with the end-of-list entry (crate-private).

## RX-888 additions (2026-11-05): front end, filter, gain, I/Q

Written against the RX-888 MkII on `radiod` 8c50dbbe on loopback (bring-up report:
`hams_com/docs/reviews/RX888_BRINGUP_2026-11-05.md`). Every fact marked "observed" below was seen on that
hardware with no antenna connected. The additions keep the schema closed: nothing here can name a
destination or a TTL, or change the front end's RF gain, attenuation or tuner. They are additive (except `AudioChannel::bandwidth_hz`, below): the
existing `SpectrumChannel` and `ChannelStatus` literals of other crates still compile; `AudioChannel` literals must name the new field.
Every function below is synchronous and pure unless marked async. The async ones run on the caller's
tokio runtime (the one whose `Handle` opened the socket) and never spawn a task.

### Data structures

#### `radiod_client_command_limits` - filter and gain bounds [@ANCHOR: radiod_client:radiod_client_command_limits]

Constants in `src/command.rs`, checked by `Command::validate` before anything is sent.

| Constant | Value | Meaning |
|---|---|---|
| `MAX_FILTER_EDGE_HZ` | `12_000.0` Hz (`f32`) | Largest magnitude of a `Command::Filter` edge: half the widest output rate the schema asks for (24 kHz, FM). An edge past the output Nyquist frequency could only narrow the passband, so nothing is lost. |
| `GAIN_RANGE_DB` | `(-20.0, 160.0)` dB (`(f32, f32)`, inclusive) | Range of a fixed channel gain in `Command::Gain`. Observed: AGC chose about 61 dB on an AM channel; I/Q noise needed about 100 dB to reach -35 dBFS. |

Plain compile-time constants: no ownership, no wire form, no locking.

#### `radiod_client_command_mode` - `Mode` [@ANCHOR: radiod_client:radiod_client_command_mode]

The demodulation of an audio channel, `Copy`. Each variant is sent as explicit parameters
(`DEMOD_TYPE`, edges, output rate, envelope, AGC), never as a server-side preset name.

| Variant | Passband (Hz from carrier) | Output | Notes |
|---|---|---|---|
| `Usb` | 50 to 3000 | 12 kHz mono | AGC on |
| `Lsb` | -3000 to -50 | 12 kHz mono | AGC on |
| `Cw` | 400 to 800 | 12 kHz mono | AGC on |
| `Am` | -5000 to 5000 | 12 kHz mono | envelope detection, AGC on |
| `Fm` | -8000 to 8000 | 24 kHz mono | threshold extension, de-emphasis 530.5 us and 12 dB |
| `Iq` (new) | -5000 to 5000 | 12 kHz, two channels: I then Q in each sample pair | linear, no detection, AGC on, the parameters of `radiod`'s `[iq]` preset. Observed: RTP payload type 123, 2 channels, 0 lost. For a fixed level set a gain with `Command::Gain`. |

#### `radiod_client_command_command` - `Command` (new variants) [@ANCHOR: radiod_client:radiod_client_command_command]

Everything a client may ask of `radiod`. The earlier variants (`Audio`, `Spectrum`, `Poll`, `Remove`) are
unchanged. Two retune commands were added. Both carry a lifetime, like every channel command: sent to an
SSRC `radiod` does not know, they create a channel from its template defaults, and the lifetime makes
that channel expire.

| Variant | Fields | Wire form | Effect |
|---|---|---|---|
| `Filter` | `ssrc: Ssrc`; `low_hz: f32`, `high_hz: f32` (Hz from the carrier, finite, magnitude at most `MAX_FILTER_EDGE_HZ`, `low_hz < high_hz`); `lifetime_frames: u32` (1 to `Policy::max_lifetime_frames`, 20 ms frames) | `OUTPUT_SSRC`, `COMMAND_TAG`, `LIFETIME`, `LOW_EDGE`, `HIGH_EDGE` | Sets the passband of an existing channel. Observed: baseband noise power followed 10 log10(width) within 0.6 dB from 500 Hz to 10 kHz. A passband wider than the output rate (12 kHz) is clipped by it. A later `Audio` command restores the mode's own passband. |
| `Gain` | `ssrc: Ssrc`; `gain_db: Option<f32>` (`Some(dB)` inside `GAIN_RANGE_DB` for a fixed gain, `None` for AGC); `lifetime_frames: u32` | `LIFETIME` plus `AGC_ENABLE` 0 and `GAIN` for `Some`, or `AGC_ENABLE` 1 alone for `None` | The channel's own audio gain, after demodulation. It is not the front end's RF gain (`RF_GAIN`, `RF_ATTEN` and `RF_AGC` cannot be sent). A later `Audio` command turns AGC back on. |

`Command::ssrc()` returns the `ssrc` of both. `Command::validate` and `Command::encode` check them as the
table says. The refusals are `CommandError::Filter` and `CommandError::Gain`, plus `CommandError::Lifetime`
for a zero or too-long lifetime.

#### `radiod_client_command_error` - `CommandError` (new variants) [@ANCHOR: radiod_client:radiod_client_command_error]

Why a command was refused; nothing was sent. New variants:

| Variant | Payload | When |
|---|---|---|
| `Filter(f32, f32)` | the low and high edges asked for | an edge is not finite or is beyond `MAX_FILTER_EDGE_HZ`, or low is not below high |
| `Gain(f32)` | the gain asked for, dB | not finite, or outside `GAIN_RANGE_DB` |
| `Bandwidth(u32)` | the width asked for, Hz | outside `MIN_BANDWIDTH_HZ` to `MAX_BANDWIDTH_HZ`, or the mode's computed edge is past `MAX_FILTER_EDGE_HZ` |

`Display` prints the `Debug` form (`Filter(1.0, 0.0)`).

#### `radiod_client_status_channel_status` - `ChannelStatus` (new fields) [@ANCHOR: radiod_client:radiod_client_status_channel_status]

A decoded status datagram (`ChannelStatus::decode`). Every field is `None` when absent, and unknown types
are skipped. A field whose value has an impossible length fails the whole decode with a `TlvError` rather
than giving a wrong number. Fields added on 2026-11-05:

| Field | Type, unit | Type number | Meaning |
|---|---|---|---|
| `low_edge_hz`, `high_edge_hz` | `Option<f32>`, Hz | 39, 40 | Channel passband edges from the carrier. |
| `agc` | `Option<bool>` | 62 | Channel AGC on. |
| `gain_db` | `Option<f32>`, dB | 68 | Channel audio gain (the AGC's choice when AGC is on). |
| `output_level_db` | `Option<f32>`, dB re full scale | 69 | Level of the channel's output. |
| `if_power_db` | `Option<f32>`, dBFS | 45 | Front end: A/D input power. `-inf` while the front end is stopped, which is while no channel exists. Observed -52.5 dBFS on noise at 34 dB RF gain. |
| `rf_gain_db` | `Option<f32>`, dB | 98 | Front end RF gain now. Under the RX-888's RF AGC it moved from 10 to 34 dB on noise. |
| `rf_atten_db` | `Option<f32>`, dB | 97 | Front end attenuation now. Observed: sent as an empty value for 0. |
| `rf_agc` | `Option<bool>` | 99 | Front end RF AGC on. |
| `rf_level_cal_db` | `Option<f32>`, dB | 110 | Front end level calibration (dBm to dBFS); 1.4 on the RX-888. |
| `fe_low_edge_hz`, `fe_high_edge_hz` | `Option<f32>`, Hz | 100, 101 | Usable front end range: 15 kHz to 30.456 MHz on the RX-888 at 64.8 Msps. |
| `fe_is_real` | `Option<bool>` | 102 | Real (direct-sampling) samples; true for the RX-888. Decides the spectrum span limit (`max_spectrum_span_hz`). |
| `ad_bits` | `Option<u64>` | 82 | A/D bits per sample (16). |
| `input_samples` | `Option<u64>`, samples | 13 | Samples taken since start, in channel statuses. Two readings over a measured interval give the real rate: observed 64,798,620 S/s over 20 s. |
| `samples_since_over` | `Option<u64>`, samples | 108 | Samples since the last A/D overrange, when sent. |

The earlier `noise_bw_bins` keeps its name, but its unit is Hz: observed 45,000 for a 30 kHz bin and 150 for a
100 Hz bin.

Channel levels (`baseband_power_db`, `noise_density_db`, spectrum bins) are referred to the input: they
already have the front end's RF gain removed. Observed: the channel noise density of -161.4 dB/Hz plus
10 log10(32.4 MHz) is 34 dB below `if_power_db`, which is the RF gain.

### Functions

#### `radiod_client_command_check_filter` - `check_filter(low: f32, high: f32) -> Result<(), CommandError>` [@ANCHOR: radiod_client:radiod_client_command_check_filter]

Crate-private. The `Command::Filter` edge rule: both edges finite with magnitude at most
`MAX_FILTER_EDGE_HZ`, and `low < high`. Returns `Err(CommandError::Filter(low, high))` otherwise. Pure. `NaN` and infinite edges fail the finite test, and equal edges fail `low < high`. It is called by
`Command::validate` for `Command::Filter` and for the edges that `Mode::passband` computes from an audio `bandwidth_hz`
(a failure there is reported as `CommandError::Bandwidth`). Test: `filter_command_is_bounded_and_carries_a_lifetime`.

#### `radiod_client_command_audio_bandwidth` - `AudioChannel::bandwidth_hz: Option<u32>` [@ANCHOR: radiod_client:radiod_client_command_audio_bandwidth]

The width of an audio channel's passband in Hz. `None` (what every earlier caller meant) sends the mode's own
passband from the `Mode` table. `Some(width)` sends edges from `Mode::passband`, so the gateway can pass a
listener's chosen bandwidth to `radiod` instead of only using it as a sharing key. `Command::validate` and
`encode` refuse a width below `MIN_BANDWIDTH_HZ` (100) or above `MAX_BANDWIDTH_HZ` (24,000), or one whose
computed edge is past `MAX_FILTER_EDGE_HZ` for the mode (for example `Cw` wider than 22,800 Hz), with
`CommandError::Bandwidth(width)`. Nothing is sent when it is refused. A later `Command::Filter` still overrides
the passband of an existing channel, and the next `Audio` command puts back the one named here. Other crates'
`AudioChannel` literals must now name the field (`bandwidth_hz: None` keeps the old behaviour).

#### `radiod_client_command_mode_passband` - `Mode::passband(self, bandwidth_hz: Option<u32>) -> (f32, f32)` [@ANCHOR: radiod_client:radiod_client_command_mode_passband]

The `(low, high)` passband edges, Hz from the carrier, that `Command::encode` writes as `LOW_EDGE` and
`HIGH_EDGE`. Pure; takes `self` by value (`Mode` is `Copy`); no errors (the range check is
`Command::validate`'s). With `None` it returns the table on `Mode`. With `Some(width)`:

| Mode | Edges for `Some(width)` |
|---|---|
| `Usb` | `50` to `width` (a 50 Hz guard by the carrier; the far edge is `width` from the carrier, never beyond it) |
| `Lsb` | `-width` to `-50` |
| `Cw` | `600 - width/2` to `600 + width/2` (centred on the 600 Hz offset the default 400 to 800 uses) |
| `SamLower` | `-width` to `100` (the carrier stays inside so the PLL tracks) |
| `SamUpper` | `-100` to `width` |
| `Am`, `Fm`, `Iq`, `Sam` | `-width/2` to `width/2` |

See also `radiod_client_command_check_filter` and the test
`a_bandwidth_sets_the_sent_passband_and_none_keeps_the_mode_default`.

#### `radiod_client_command_encode_front_end_poll` - `encode_front_end_poll(tag: u32) -> Vec<u8>` [@ANCHOR: radiod_client:radiod_client_command_encode_front_end_poll]

The datagram that asks for the front end's status: a command packet holding only `OUTPUT_SSRC` 0 and
`COMMAND_TAG tag`, then the end marker. SSRC 0 is the front end, not a channel, so no channel is created and
no lifetime applies. This is the one command not built from a `Command`, because a `Ssrc` can never be 0.
`tag` is any value; it is not echoed (below). Pure. `Control::front_end` is the usual caller.

#### `radiod_client_command_max_spectrum_span_hz` - `max_spectrum_span_hz(input_samprate: u64, real: bool) -> f64`

[@ANCHOR: radiod_client:radiod_client_command_max_spectrum_span_hz]

The widest spectrum span (`bin_count * bin_hz`, Hz) a front end fills. It is half of `input_samprate` (Hz)
when `real` is true (a direct-sampling front end such as the RX-888) and all of it when false. Feed it the
front end's `input_samprate` and `fe_is_real`. Observed at 64.8 Msps: spans up to 32.41 MHz came back with
every bin filled, and 32.44 MHz and wider came back with about half the bins exactly zero, so callers keep
requests inside this bound. Pure; returns 0.0 for a rate of 0.

#### `radiod_client_net_is_front_end` - `is_front_end(status: &ChannelStatus) -> bool`

[@ANCHOR: radiod_client:radiod_client_net_is_front_end]

True when `status` is about the front end (SSRC 0). Observed: `radiod` does not echo the command tag in a
front-end reply, and sends one only when polled. So any SSRC-0 status counts as the answer, whoever asked:
the front end is the same for every client. Pure.

#### `radiod_client_net_control_front_end` - `async fn Control::front_end(&mut self, wait: Duration) -> Result<Option<ChannelStatus>, NetError>`

[@ANCHOR: radiod_client:radiod_client_net_control_front_end]

Sends `encode_front_end_poll` to the status group, repeating every 200 ms, and returns the first
front-end status (`is_front_end`) within `wait`. Channel statuses received meanwhile are skipped.

| Argument | Type | Meaning |
|---|---|---|
| `wait` | `Duration` | Total time to keep asking; 1 to 2 s is ample on loopback (the trial's replies took about 20 ms). |

Returns `Ok(Some(status))` with the front-end fields (`input_samprate`, `ad_over`, `rf_gain_db`,
`rf_atten_db`, `rf_agc`, `if_power_db`, edges, `description`), `Ok(None)` if nothing answered, or
`Err(NetError::Io)` if a send or receive failed. Async on the caller's runtime; it needs `&mut self`, so
one `Control` serves one request at a time. It advances the tag counter once per send. Creates no channel.
It is safe to call at any rate, but while no channel exists the front end is stopped and `if_power_db`
reads `-inf`.

## Synchronous AM (2026-11-06)

Synchronous AM for the shared receiver and the `radiod-channel` virtual rig (hams_com to-do
`synchronous-am-for-sdr-sources-radiod-and-sdrconnect-8e1f4a62`). `radiod`'s linear demodulator has a carrier
phase-locked loop of its own, so synchronous detection is done by `radiod`, not by the client. The facts come
from `radiod`'s documentation only (clean room, as for the trial): `docs/ka9q-radio-3.md` at the pinned commit
(the `pll`, `square`, `pll-bw`, `envelope` and squelch sections), the `[sam]` and `[ame]` entries of
`share/presets.conf`, and the type numbers of `src/status.h`, compiled to read them (51 to 55, 58, 83, 84,
109). **Not yet run against a real `radiod`**: no signal source is connected to the RX-888 (bring-up report
section 8), so lock behaviour on a real carrier is a hardware follow-up.

### Data structures

#### `radiod_client_command_mode` - `Mode` (synchronous variants)

[@ANCHOR: radiod_client:radiod_client_command_mode]

Three variants added to the table above. All are sent as explicit parameters:

| Variant | Passband (Hz from carrier) | Output | Wire parameters |
|---|---|---|---|
| `Sam` | -5000 to 5000 | 12 kHz mono | `DEMOD_TYPE` 0, `ENVELOPE` 0, `PLL_ENABLE` 1, `PLL_SQUARE` 0, `SNR_SQUELCH` 0, `SQUELCH_OPEN` and `SQUELCH_CLOSE` -1000 dB, `AGC_ENABLE` 1 |
| `SamLower` | -5000 to +100 | 12 kHz mono | as `Sam` |
| `SamUpper` | -100 to +5000 | 12 kHz mono | as `Sam` |

- **Why these values.** `pll = on` makes the PLL track a carrier to 0 Hz, and with envelope detection off the
  mono output is the in-phase (real) part of the locked baseband, which is synchronous detection. The
  single-sideband variants keep 100 Hz of the other side so the carrier stays inside the passband (the
  placement of `radiod`'s `[ame]` preset, mirrored for the lower sideband). The PLL also turns on `radiod`'s
  carrier squelch; its documentation says an opening threshold of -1000 dB keeps it open, and a listener
  should hear a fading carrier rather than silence, so both thresholds are sent as -1000 and the SNR squelch
  is off.
- **`PLL_BW` is not sent.** The documented default (100 Hz) is used. The wire form of a written `PLL_BW` was
  not seen in the trial, so the client leaves it alone rather than guess; the status reports it (`pll_bw_hz`).
- **Every other linear mode now sends `PLL_ENABLE` 0** (`Usb`, `Lsb`, `Cw`, `Am`, `Iq`). `radiod` keeps a
  channel's parameters between commands, so a channel switched from `Sam` back to `Am` would otherwise keep
  its PLL (and squelch). `Fm` is not linear and sends no PLL field.
- **Open question for the hardware run.** The keep-alive of `radioctl`'s bridge re-sends the whole `Audio`
  command every 2 s; whether re-sending `PLL_ENABLE` 1 to a channel that already has it disturbs the lock is
  not documented and must be watched on a real carrier (lock should stay steady across refreshes).

#### `radiod_client_command_sam_squelch` - `SAM_SQUELCH_OPEN_DB`

[@ANCHOR: radiod_client:radiod_client_command_sam_squelch]

A compile-time constant (no ownership, no locking): `pub const SAM_SQUELCH_OPEN_DB: f32 = -1000.0`, dB SNR, sent as
both `SQUELCH_OPEN` (83) and `SQUELCH_CLOSE` (84) with every synchronous `Audio` command.

#### `radiod_client_status_channel_status` - `ChannelStatus` (PLL fields)

[@ANCHOR: radiod_client:radiod_client_status_channel_status]

Fields added on 2026-11-06. Each is `None` when the status does not carry it. All are set only by
`ChannelStatus::decode`; nobody else writes them.

| Field | Type, unit | Type number | Meaning |
|---|---|---|---|
| `pll_enable` | `Option<bool>` | 51 | The linear channel's carrier PLL is on. |
| `pll_lock` | `Option<bool>` | 52 | The PLL is locked to a carrier. Meaningful only while the PLL is on; read it through `carrier_lock`. |
| `pll_square` | `Option<bool>` | 53 | Square-law carrier recovery (DSB-SC); implies the PLL. Never sent on by this client. |
| `pll_bw_hz` | `Option<f32>`, Hz | 55 | PLL loop bandwidth (documented default 100). |
| `pll_phase_rad` | `Option<f32>`, radians | 54 | PLL phase. |
| `pll_snr_db` | `Option<f32>`, dB | 58 | SNR of the carrier the PLL sees (linear) or of the signal (FM). |
| `freq_offset_hz` | `Option<f32>`, Hz | 59 | Carrier offset the PLL measured (linear) or the FM demodulator's offset. |
| `pll_wraps` | `Option<u64>` | 109 | Complete PLL rotations since the channel started; a carrier offset makes it count. |
| `envelope` | `Option<bool>` | 56 | Envelope detection on. |

The decode rule is unchanged: a value of impossible length fails the decode with `TlvError`.

### Functions

#### `radiod_client_command_mode_is_synchronous` - `Mode::is_synchronous(self) -> bool`

[@ANCHOR: radiod_client:radiod_client_command_mode_is_synchronous]

True for `Sam`, `SamLower` and `SamUpper`: the modes that turn `radiod`'s carrier PLL on, so the channel's
status carries a carrier lock. Callers use it to decide whether to report a lock at all (a plain AM channel has
none). Takes `self` by value (`Mode` is `Copy`); pure, no errors, no allocation.

#### `radiod_client_status_carrier_lock` - `ChannelStatus::carrier_lock(&self) -> Option<bool>`

[@ANCHOR: radiod_client:radiod_client_status_carrier_lock]

The carrier lock of a synchronous channel, for a console's lock indicator.

| Returns | When |
|---|---|
| `Some(true)` | the status says the PLL is on (`pll_enable` or `pll_square` true) and `PLL_LOCK` is set |
| `Some(false)` | the PLL is on and `PLL_LOCK` is clear (or sent as an empty value) |
| `None` | the PLL is off, or the status says nothing about it (a front-end or spectrum status, an older `radiod`), or the PLL is on but `PLL_LOCK` is absent |

Borrows `self`; pure, no errors. A status is a snapshot: the lock is as fresh as the last status `radiod` sent
for the channel (one per acknowledged command or poll). See also `radiod_client_status_decode` and the test
`decodes_the_pll_fields_and_the_carrier_lock`.
