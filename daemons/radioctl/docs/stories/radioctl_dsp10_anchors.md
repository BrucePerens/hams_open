# radioctl: TAPR DSP-10 anchors (`radioctl-protocols/src/dsp10.rs`)

Source of every fact: Larkin W7PUA, "Notes on the PC/DSP interface for the DSP-10 transceiver", Version 2.00, 10 Feb 2001, section "PC to DSP Commands".

## `Dsp10Engine` [@ANCHOR: radioctl:radioctl_dsp10_dsp10_engine]

**Purpose.** The DSP-10 client engine (mode and frequency; no PTT).

**Definition.** `pub struct Dsp10Engine` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/dsp10.rs`; derives `Debug`.

| Field | Type | Meaning |
|---|---|---|
| `desc` | `RigDescription` | The rig description the engine was built from; returned by `description()`. |
| `side` | `Side` | The sideband class of the last mode set; picks the 5 kHz rounding. |
| `hz` | `Option<u64>` | The last frequency set, resent when the mode changes. |

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
| `hz` | `Hz` | The description's default frequency, reported until the synthesizer and third LO have been loaded. |
| `shift` | `Vec<u16>` | The Command 8 words shifted in since the last latch. |
| `dph` | `u32` | The 24-bit third-LO phase increment of the last Command 7. |
| `pf7` | `bool` | The enable line (Command 7 `data` bit 0). |
| `synth_hz` | `Option<u64>` | The synthesizer frequency last latched. |
| `r_word` | `Option<u32>` | The reference-divider word last latched. |

## `Side` [@ANCHOR: radioctl:radioctl_dsp10_side]

**Purpose.** Which sideband class the first-conversion 5 kHz rounding is for.

**Definition.** `pub enum Side` (enum) in `daemons/radioctl/crates/radioctl-protocols/src/dsp10.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Variant | Type | Meaning |
|---|---|---|
| `Usb` | - | USB or CW on air; rounding offset 4000 Hz. |
| `Lsb` | - | LSB on air; offset 250 Hz. |
| `Fm` | - | FM; offset 2500 Hz. |

## `Tuning` [@ANCHOR: radioctl:radioctl_dsp10_tuning_struct]

**Purpose.** The words that tune the radio to one frequency.

**Definition.** `pub struct Tuning` (struct) in `daemons/radioctl/crates/radioctl-protocols/src/dsp10.rs`; derives `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`.

| Field | Type | Meaning |
|---|---|---|
| `synth_word` | `u32` | The 19-bit synthesizer word `(a << 1) | (b << 8)`. |
| `dph` | `u32` | The 24-bit third-LO phase increment. |

## Functions

### `pub fn tuning(hz: u64, side: Side) -> Option<Tuning>` [@ANCHOR: radioctl:radioctl_dsp10_tuning]

Computes the tuning words for `hz` from the PC program (UHFA.C v3.96 `new_freq`, GPL, UHF396.ZIP): round to a 5 kHz step with the sideband's offset, synthesizer N = (step - 19.665 MHz) / 5 kHz split as b = N / 64 and a = N % 64, third LO increment 349.5253333 * (step - hz + 15000). `None` outside 144 to 148 MHz. Pure.

### `pub fn synth_frames(t: &Tuning) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_dsp10_synth_frames]

The 36 bytes that load the synthesizer and set the third LO: a Command 7 with the increment, three Command 8 words (3 bits, 16 bits, control byte 0x04 and RF gain byte 0x88), then Command 7 with the enable low and high (UHFA.C `new_freq`, `send_shift_data`). Pure.

### `fn open_requests(&mut self) -> Vec<Request>` [@ANCHOR: radioctl:radioctl_dsp10_engine_open_requests]

One request that loads the reference divider word `R10MHZ` as UHFA.C `initialize` does, with the receive control byte; never keys the radio.

### `pub fn reference_word(&self) -> Option<u32>` [@ANCHOR: radioctl:radioctl_dsp10_twin_reference_word]

The reference-divider word the twin's synthesizer was last loaded with.

### `pub fn mode_bytes(code: u8) -> Option<(u8, u8)>` [@ANCHOR: radioctl:radioctl_dsp10_mode_bytes]

Maps the description's mode code (`0` LSB, `1` USB, `2` CW, `3` FM) to the `mode` and `sb` bytes of Command 77. On 2 m the DSP's I-F sideband is the opposite of the one on air: LSB sends `sb` 1, USB, CW and FM send 0 (PC program UHFA.C v3.96, `set_mode`). Returns `None` for another code. Pure.

### `pub fn mode_frame(mode: u8, sb: u8) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_dsp10_mode_frame]

Builds the six bytes `ESC 77 mode sb mikel mikeh`; the microphone word is `MIKE_WORD` (1, ours). Pure.

### `pub fn new(desc: RigDescription) -> Result<Dsp10Engine, RigError>` [@ANCHOR: radioctl:radioctl_dsp10_engine_new]

Builds the engine; refuses a description whose vendor dialect is not `dsp10` with `Unsupported`.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_dsp10_engine_encode]

Encodes `SetMode` as Command 77 with reply `None` Every other operation, a mode the description lacks, or a capability the description does not claim is refused with `Unsupported`; a frequency outside 144 to 148 MHz is refused; PTT is not encoded (the PC program's keying sequence needs waits between stages).

### `fn feed(&mut self, _bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_dsp10_engine_feed]

Ignores the status stream the DSP sends (`ESC 'A'` blocks) and returns nothing.

### `pub fn new(desc: &RigDescription) -> Result<Dsp10Twin, RigError>` [@ANCHOR: radioctl:radioctl_dsp10_twin_new]

Builds a twin in SSB, USB, at the description's default frequency; refuses another dialect.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_dsp10_twin_snapshot]

Reports the frequency the latched synthesizer word and third LO make (the default until both are loaded), the mode code derived from the mode and sideband bytes, and PTT false.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_dsp10_twin_feed]

Collects bytes into six-byte frames starting at ESC (bytes before an ESC are dropped), applies Command 77 (mode and sideband bytes in range), Command 8 (shift words) and Command 7 (third LO; a rising enable latches the shifted words as the synthesizer or reference register), and answers nothing.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_dsp10_twin_front_panel]

Always an error: the twin has no knobs.

### `fn shift_frames(word: u32, dph: u32, lo_first: bool) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_dsp10_shift_frames]

Builds the three Command 8 words (3 bits, 16 bits, receive control byte and RF gain byte) and the enable low and high Command 7 frames for a 19-bit `word`, optionally preceded by the third-LO Command 7 (`lo_first`). Used for the tuning word and, with the reference word, at open. Pure.

### `fn tuned(&self) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_dsp10_twin_tuned]

The twin's receive frequency: latched synthesizer step plus 19.665 MHz plus 15 kHz minus the third LO, rounded to Hz; `None` until a synthesizer word has been latched.

### `fn apply(&mut self, f: &[u8])` [@ANCHOR: radioctl:radioctl_dsp10_twin_apply]

Applies one complete six-byte frame: records the command byte, takes Command 77, collects Command 8 words, and on Command 7 stores the increment and latches on a rising enable bit.

### `fn latch(&mut self)` [@ANCHOR: radioctl:radioctl_dsp10_twin_latch]

Turns three shifted words into the synthesizer register (N counter word, bit 0 clear) or the reference register (bit 0 set); anything but three words is dropped.
