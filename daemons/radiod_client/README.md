# radiod_client

A receive-only Rust client for the multicast control and status protocol of
[ka9q-radio](https://github.com/ka9q/ka9q-radio) `radiod`, the multichannel receive engine planned for the
shared receiver service (`hams_com/docs/proposals/SDR_RECEIVE_ENGINE_AND_TRANSMIT_PATH.md`). Prototype from the
radiod trial; findings and numbers are in `hams_com/docs/reviews/RADIOD_TRIAL_2026-10-30.md`.

* `tlv`: bounds-checked parser and a crate-private writer for the type/length/value format.
* `command`: the **closed command schema**. Typed audio and spectrum channels, validated against a `Policy`
  (frequency range, bin count, bin width, averaging, lifetime). Every channel command carries a non-zero
  lifetime (a bare poll of an unknown SSRC makes `radiod` create an immortal channel). There is no field for a
  data destination, no raw-bytes entry point, and nothing that touches the front end.
* `status`: decoded channel status, including float and byte spectrum bins.
* `rtp`: RTP header parsing, loss accounting, 16-bit PCM and Opus decoding.
* `net`: multicast sockets (TTL 0, a named interface), request with tag acknowledgement.
* `radiod_probe`: a trial driver (`audio`, `spectrum`, `poll`, `remove`).

Tested against `radiod` commit 8c50dbbe9958c59a24e392fa1785501350e833f9 with its `sig_gen` pseudo front end, on
loopback in a private network namespace. Not wired into the relay. The protocol is not authenticated: run `radiod`
on loopback or in a namespace and let this client be its only sender (proposal section 11).
