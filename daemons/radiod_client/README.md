# radiod_client

A receive-only Rust client for the multicast control and status protocol of
[ka9q-radio](https://github.com/ka9q/ka9q-radio) `radiod`, the multichannel receive engine planned for the
shared receiver service (`hams_com/docs/proposals/SDR_RECEIVE_ENGINE_AND_TRANSMIT_PATH.md`). Prototype from the
radiod trial; findings and numbers are in `hams_com/docs/reviews/RADIOD_TRIAL_2026-10-30.md`.

* `tlv`: bounds-checked parser and a crate-private writer for the type/length/value format.
* `command`: the **closed command schema**. Typed audio and spectrum channels, validated against a `Policy`
  (frequency range, bin count, bin width, averaging, lifetime). Every channel command carries a non-zero
  lifetime (a bare poll of an unknown SSRC makes `radiod` create an immortal channel). A spectrum request never exceeds 16,000 float or 60,000 byte bins whatever the policy
  says (larger requests segfault `radiod`). There is no field for a
  data destination, no raw-bytes entry point, and nothing that touches the front end.
* `status`: decoded channel status, including float and byte spectrum bins.
* `rtp`: RTP header parsing, loss accounting, 16-bit PCM and Opus decoding.
* `net`: tokio multicast sockets (TTL 0, a named interface), request with tag acknowledgement. No runtime
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

- [@ANCHOR: radiod_client:radiod_client_command_ssrc_new] (command) Validate a gateway-chosen SSRC.
- [@ANCHOR: radiod_client:radiod_client_command_ssrc_get] (command) The numeric value.
- [@ANCHOR: radiod_client:radiod_client_command_policy_default] (command) The default site policy (10 kHz to 30 MHz, 8,192 bins, 5 minute lifetime cap).
- [@ANCHOR: radiod_client:radiod_client_command_error_fmt] (command) Display text of a refused command.
- [@ANCHOR: radiod_client:radiod_client_command_check_freq] (command) Refuses a frequency that is not finite or is outside the policy range.
- [@ANCHOR: radiod_client:radiod_client_command_check_life] (command) Refuses a lifetime of 0 (radiod never expires such a channel) or above the policy limit.
- [@ANCHOR: radiod_client:radiod_client_command_policy_bin_limit] (command) The largest bin count this policy permits for a format: the site limit, never above the hard ceiling that keeps `radiod` from crashing.
- [@ANCHOR: radiod_client:radiod_client_command_ssrc] (command) The channel this command addresses.
- [@ANCHOR: radiod_client:radiod_client_command_validate] (command) Check the command against the site policy.
- [@ANCHOR: radiod_client:radiod_client_command_encode] (command) Validate and build the datagram.
- [@ANCHOR: radiod_client:radiod_client_net_error_fmt] (net) Display text of a network or protocol failure.
- [@ANCHOR: radiod_client:radiod_client_net_error_from_io] (net) Wraps an I/O error as a network failure.
- [@ANCHOR: radiod_client:radiod_client_net_next_tag] (net) The tag after `tag`: it increments and never becomes 0 (a zero tag is an empty integer on the wire and cannot be told from an absent one).
- [@ANCHOR: radiod_client:radiod_client_net_is_ack] (net) True when `status` is `radiod`'s reply to the command with this channel and tag.
- [@ANCHOR: radiod_client:radiod_client_net_classify] (net) Decode one received datagram: a status is returned, our own looped-back commands are skipped, and a malformed datagram is counted in `malformed` and skipped.
- [@ANCHOR: radiod_client:radiod_client_net_open_group] (net) Open a multicast receive socket bound to `port` and joined to `group` on `iface`, with a TTL of 0 so nothing it sends leaves the host.
- [@ANCHOR: radiod_client:radiod_client_net_register] (net) Register a socket with the runtime behind `handle`.
- [@ANCHOR: radiod_client:radiod_client_net_control_open] (net) Join the status group (port `STATUS_PORT`) and prepare to send commands to it.
- [@ANCHOR: radiod_client:radiod_client_net_control_open_on_port] (net) As `Control::open`, for a `radiod` configured with a status port other than 5006.
- [@ANCHOR: radiod_client:radiod_client_net_control_send] (net) Validate and send a command; returns the tag `radiod` will echo in its reply.
- [@ANCHOR: radiod_client:radiod_client_net_control_recv_status] (net) Wait up to `wait` for the next status datagram, skipping our own commands and malformed datagrams.
- [@ANCHOR: radiod_client:radiod_client_net_control_request] (net) Send `cmd` and wait for the status carrying its tag (the only acknowledgement `radiod` gives).
- [@ANCHOR: radiod_client:radiod_client_net_rtp_open] (net) Join the data group (port `RTP_PORT`); the socket is registered with the runtime behind `handle`.
- [@ANCHOR: radiod_client:radiod_client_net_rtp_open_on_port] (net) As `RtpSocket::open`, for a `radiod` configured with a data port other than 5004.
- [@ANCHOR: radiod_client:radiod_client_net_rtp_recv] (net) Receive one datagram into `buf`; `Ok(None)` after `wait` with nothing received.
- [@ANCHOR: radiod_client:radiod_client_rtp_error_fmt] (rtp) Display text of an RTP rejection.
- [@ANCHOR: radiod_client:radiod_client_rtp_packet_parse] (rtp) Parse a datagram.
- [@ANCHOR: radiod_client:radiod_client_rtp_encoding_from_radiod] (rtp) Map `radiod`'s encoding number.
- [@ANCHOR: radiod_client:radiod_client_rtp_decoder_debug_fmt] (rtp) Debug text of an audio decoder (encoding and channels, never the codec state).
- [@ANCHOR: radiod_client:radiod_client_rtp_decoder_new] (rtp) Build a decoder from the channel status (`channels` for PCM; Opus is always 2).
- [@ANCHOR: radiod_client:radiod_client_rtp_decoder_decode] (rtp) Decode a payload to mono samples in -1.0..=1.0.
- [@ANCHOR: radiod_client:radiod_client_rtp_seq_push] (rtp) Account for one packet.
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
