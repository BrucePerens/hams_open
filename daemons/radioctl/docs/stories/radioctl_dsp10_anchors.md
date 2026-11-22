# radioctl: TAPR DSP-10 anchors (`radioctl-protocols/src/dsp10.rs`)

Source of every fact: Larkin W7PUA, "Notes on the PC/DSP interface for the DSP-10 transceiver", Version 2.00, 10 Feb 2001, section "PC to DSP Commands".

## `Dsp10Engine` [@ANCHOR: radioctl:radioctl_dsp10_dsp10_engine]

**Purpose.** The DSP-10 client engine (mode setting only).

**Definition.** `pub struct Dsp10Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/dsp10.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the engine was built from; returned by `description()`. |

**Notes.** The DSP sends no answer to a command, so a request waits for nothing.

## `Dsp10Twin` [@ANCHOR: radioctl:radioctl_dsp10_dsp10_twin]

**Purpose.** A virtual DSP-10 for tests: it assembles six-byte frames and keeps the mode.

**Definition.** `pub struct Dsp10Twin` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/dsp10.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `frame` | `Vec<u8>` | The frame being assembled, starting at an ESC byte. |
| `mode` | `u8` | Command 77 mode byte (0 SSB, 1 CW, 2 FM). |
| `sb` | `u8` | Command 77 sideband byte. |
| `mike` | `u16` | The microphone word of the last mode command. |
| `last_cmd` | `u8` | The command byte of the last complete frame. |
| `hz` | `Hz` | The description's default frequency, reported by the snapshot (the DSP-10 has no tuning command here). |

## Functions

### `pub fn mode_bytes(code: u8) -> Option<(u8, u8)>` [@ANCHOR: radioctl:radioctl_dsp10_mode_bytes]

Maps the description's mode code (`0` LSB, `1` USB, `2` CW, `3` FM) to the `mode` and `sb` bytes of Command 77. On 2 m the DSP's I-F sideband is the opposite of the one on air: LSB sends `sb` 1, USB, CW and FM send 0 (PC program UHFA.C v3.96, `set_mode`). Returns `None` for another code. Pure.

### `pub fn mode_frame(mode: u8, sb: u8) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_dsp10_mode_frame]

Builds the six bytes `ESC 77 mode sb mikel mikeh`; the microphone word is `MIKE_WORD` (1, ours). Pure.

### `pub fn new(desc: RigDescription) -> Result<Dsp10Engine, RigError>` [@ANCHOR: radioctl:radioctl_dsp10_engine_new]

Builds the engine; refuses a description whose vendor dialect is not `dsp10` with `Unsupported`.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_dsp10_engine_encode]

Encodes `SetMode` as Command 77 with reply `None` Every other operation, a mode the description lacks, or a capability the description does not claim is refused with `Unsupported`; there is no frequency or PTT command in the document.

### `fn feed(&mut self, _bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_dsp10_engine_feed]

Ignores the status stream the DSP sends (`ESC 'A'` blocks) and returns nothing.

### `pub fn new(desc: &RigDescription) -> Result<Dsp10Twin, RigError>` [@ANCHOR: radioctl:radioctl_dsp10_twin_new]

Builds a twin in SSB, USB, at the description's default frequency; refuses another dialect.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_dsp10_twin_snapshot]

Reports the default frequency, the mode code derived from the mode and sideband bytes, and PTT false.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_dsp10_twin_feed]

Collects bytes into six-byte frames starting at ESC (bytes before an ESC are dropped), applies a Command 77 whose mode and sideband bytes are in range, and answers nothing.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_dsp10_twin_front_panel]

Always an error: the twin has no knobs.
