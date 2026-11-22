# TS-990S LAN session (`kenwood_ts990_lan`)

The Kenwood TS-990S LAN stream as a network framing, written from Kenwood's PC command reference (PDF pages 1 and 62). Library only; never run against a radio.

### `fn utf16(ascii: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ts990_lan_utf16]

Widens ASCII text to the UTF-16 little-endian bytes the LAN port speaks (each byte followed by a zero). Pure.

### `fn lan_connect_request() -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ts990_lan_connect_request]

The first LAN frame, `##CN;`, as ASCII.

### `fn lan_login(account: &str, password: &str) -> Result<Vec<u8>, RigError>` [@ANCHOR: radioctl:radioctl_ts990_lan_login]

The login frame `##ID<len><len><account><password>;` with one-digit lengths. Account and password must each be 1 to 8 printable ASCII characters without `;`, else `Unsupported`.

## `Phase` [@ANCHOR: radioctl:radioctl_kenwood_ts990_lan_phase]

**Purpose.** How far the TS-990S LAN session's login has got.

**Definition.** `enum Phase` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts990_lan.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `AwaitConnect` | - | Waiting for the radio's `##CN` reply to the connect request. |
| `AwaitLogin` | - | Waiting for the radio's `##ID` reply to the login. |
| `Up` | - | The session is up; CAT frames pass through and keep-alive probes run. |

## `Ts990LanFraming` [@ANCHOR: radioctl:radioctl_kenwood_ts990_lan_framing]

**Purpose.** The TS-990S LAN session framing: connect, login, then the CAT stream with a keep-alive.

**Definition.** `pub struct Ts990LanFraming` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/kenwood_ts990_lan.rs`; implements `Debug` by hand (the login frame holding the password is hidden).

| Field | Type | Meaning |
|---|---|---|
| `login` | `Vec<u8>` | The ready `##ID` login frame (ASCII) sent after `##CN1;`. |
| `phase` | `Phase` | Where the login handshake stands (`Phase`). |
| `unit` | `Vec<u8>` | Wire bytes not yet a whole UTF-16 unit. |
| `text` | `Vec<u8>` | Decoded ASCII text not yet a whole `;`-terminated frame. |
| `probes_out` | `u32` | Keep-alive probes sent and not yet answered. |

### `fn new(account: &str, password: &str) -> Result<Ts990LanFraming, RigError>` [@ANCHOR: radioctl:radioctl_ts990lan_new]

Builds a session from the radio's LAN account; fails if the login frame cannot carry it.

### `fn frame(&mut self, frame: &[u8], out: &mut Decoded)` [@ANCHOR: radioctl:radioctl_ts990lan_handle_frame]

Handles one complete frame in the current phase: `##CN1;` triggers the login, `##CN0;` and `##ID0;` fail the handshake, `##ID1;` marks the link ready, later frames are CAT payload except the answer to a keep-alive probe.

### `fn reset(&mut self)` [@ANCHOR: radioctl:radioctl_ts990lan_reset]

Returns to the start of the handshake and drops buffered bytes.

### `fn greeting(&mut self) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ts990lan_greeting]

The `##CN;` request as UTF-16.

### `fn needs_handshake(&self) -> bool` [@ANCHOR: radioctl:radioctl_ts990lan_needs_handshake]

Always true: the link is up only after `##ID1;`.

### `fn encode(&mut self, payload: &[u8]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_ts990lan_encode]

Widens CAT text from the actor to UTF-16 for the wire.

### `fn decode(&mut self, wire: &[u8]) -> Decoded` [@ANCHOR: radioctl:radioctl_ts990lan_decode]

Reads wire bytes (any split, including inside a UTF-16 unit), reduces them to ASCII, and handles each `;`-terminated frame. More than 4096 bytes without a `;` is a failure.

### `fn keepalive(&self) -> Option<(Duration, Vec<u8>)>` [@ANCHOR: radioctl:radioctl_ts990lan_keepalive]

The probe `ID;` (UTF-16) every 4 seconds; the radio closes after 10 seconds without traffic.

### `fn keepalive_sent(&mut self)` [@ANCHOR: radioctl:radioctl_ts990lan_keepalive_sent]

Counts an outstanding probe so its `ID` answer is swallowed.
