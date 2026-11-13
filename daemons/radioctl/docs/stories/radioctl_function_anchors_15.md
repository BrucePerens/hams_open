# radioctl function anchors, batch 15

Function anchors for the `radioctl-protocols` crate (AnyTone D578A twin, Skanti TRP8000/8255, Racal family and Ten-Tec RX-320/TT-550), written to the ADR 0113 standard. Each anchor names a function in the crate source and is exercised by a test that cites it with `Tests [@ANCHOR: ...]`. Engines only convert operations to bytes and bytes to observations; the transmit lock, band check and transmit gate are applied by the actor in `radioctl-core` that calls them, not here. Entries that can put a key-down on the wire (Skanti `XN` and `u`) say so. Twins are virtual radios for tests and never touch hardware.

## `crates/radioctl-protocols/src/anytone.rs`

### `pub fn new(desc: &RigDescription) -> Result<AnytoneTwin, RigError>` [@ANCHOR: radioctl:radioctl_anytone_twin_new]

Builds a virtual AnyTone D578A for tests. `desc` is borrowed; the dialect must be `d578a` or `Unsupported` is returned. Both VFOs start at the description's default frequency in hertz and the receive buffer is empty. No I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_anytone_twin_snapshot]

Reports the twin's state for oracle comparison. Always `Some`: `frequency` is the VFO A frequency in hertz (VFO B is not included), `mode_code` is `None` because the engine does not read modes, and `ptt` is always false because the engine and twin never transmit. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_anytone_twin_feed]

Consumes controller bytes and returns the radio's answers. Bytes are buffered with monotonic time `now`. In a loop the head of the buffer is examined: the `+ADATA:00,000` close record is removed and echoed back; the `+ADATA:00,016` mode announcement is removed up to the end of its second line (nothing is sent back, and it waits if that line has not arrived); the `+ADATA:00,006` read record, once all 25 bytes are held, is removed and answered with a 138 byte frequency answer for the VFO named in byte 16 (0x2D is B, anything else A), built by `frequency_answer`. Anything else drops one byte to resynchronise, except that an incomplete prefix of a header waits for more bytes. Mutates the buffer only; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_anytone_twin_front_panel]

Simulates tuning VFO A at the radio. `panel` is owned. `Panel::Frequency(hz)` sets VFO A to `hz` hertz with no range check; any other control returns `Err("an AnyTone twin has no <panel> knob")`. Success returns an empty vector because the radio does not report front-panel changes unprompted. VFO B is not changed. Nothing is transmitted.

## `crates/radioctl-protocols/src/skanti.rs`

### `pub fn from_name(name: &str) -> Option<Dialect>` [@ANCHOR: radioctl:radioctl_skanti_dialect_from_name]

Maps the dialect name of a Skanti description to the command set: `trp8000` gives `Dialect::Trp8000` (CR-ended text commands, a `>` prompt as the answer) and `trp8255` gives `Dialect::Trp8255` (single bytes, each answered with ACK 0x06 or NAK 0x15). The match is exact and case sensitive; any other name returns `None`, which the engine and twin turn into `Unsupported`. Pure.

### `pub fn units(hz: Hz) -> Option<u64>` [@ANCHOR: radioctl:radioctl_skanti_units]

Converts a frequency to the six-digit field of the Skanti command: units of 100 Hz (`hz / 100`, fraction dropped). `hz` is in hertz and must not exceed 99,999,999 Hz, otherwise `None` is returned (the field cannot hold it). Pure; the engine turns `None` into `Unsupported("frequency above 100 MHz")`.

### `fn mode_text(code: u8) -> String` [@ANCHOR: radioctl:radioctl_skanti_mode_text]

Gives the text the TRP8000 takes for a mode code of the description. `code` is the one byte ASCII code. The code `A` (CW) must be sent as the two characters `A1`; every other code is sent as that single character. Returns a new string, pure, no errors. The mode set itself comes from the rig description.

### `pub fn new(desc: RigDescription) -> Result<SkantiEngine, RigError>` [@ANCHOR: radioctl:radioctl_skanti_engine_new]

Builds the client engine for a Skanti transceiver. `desc` is owned and kept. The dialect name (`trp8000` or `trp8255`) selects the command set through `Dialect::from_name`; an unknown name, or a description that is not a vendor serial one, returns `Unsupported`. No acknowledgements are owed at start. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_skanti_engine_encode]

Builds the command bytes and records how many acknowledgements the radio owes. `op` is borrowed; the reply kind is always `Ack`. `SetFrequency(A, hz)` needs `units(hz)` (up to 99,999,999 Hz) or returns `Unsupported`; the TRP8000 gets `Z` plus six digits and CR (one acknowledgement), the TRP8255 gets `:` plus six digits and CR followed by `:;` and CR (one acknowledgement per byte sent). `SetMode` uses the description's code: `mode_text` and CR on the TRP8000, the bare code byte on the TRP8255; a mode not in the description returns `Unsupported`. `SetPtt(on)` sends `XN` CR to key and `XF` CR to unkey on the TRP8000, and the single byte `u` (key) or `v` (unkey) on the TRP8255; these put the transmitter on air, so the actor applies the transmit gate and fail-closed unkey around them. No reads exist, so any other operation or missing capability returns `Unsupported`.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_skanti_engine_feed]

Decodes the radio's answer bytes one by one; the time argument is unused. The good byte is `>` for the TRP8000 and ACK for the TRP8255: when acknowledgements are owed it decrements the count and, when the count reaches zero, produces one `Inbound::Ack`, so a multi-byte command is acknowledged only after all of its bytes were. On the TRP8255 a NAK byte clears the count and produces `Inbound::Nak` (the TRP8000 has no NAK). CR and LF are ignored; every other byte, including a good byte when nothing is owed, becomes `Inbound::Unrecognised` with that one byte.

### `pub fn new(desc: &RigDescription) -> Result<SkantiTwin, RigError>` [@ANCHOR: radioctl:radioctl_skanti_twin_new]

Builds a virtual Skanti radio for tests. `desc` is borrowed; the dialect selects the command set (unknown name gives `Unsupported`). Receive and transmit frequencies start at the description's default in 100 Hz units, the mode at the default mode's code, or the first code of the mode table, or ASCII `X`; the list of valid mode codes is kept. PTT (push to talk) is off and the line buffer is empty. No I/O.

### `fn apply_8000(&mut self, line: &[u8]) -> bool` [@ANCHOR: radioctl:radioctl_skanti_twin_apply_8000]

Applies one CR-stripped TRP8000 command line to the twin and returns whether the radio would accept it (true produces the `>` prompt). `Z` plus six digits sets the receive frequency and `T` plus six digits the transmit frequency (units of 100 Hz; non-digits are refused); `XN` keys and `XF` unkeys the modelled PTT; `A1` selects CW when the description has `A`, and a single mode code in the description (other than `A`) selects that mode. The filter, AGC (automatic gain control), RF and power commands Hamlib sends after a mode write (`I`, `V`, `N`, `W`, `AS`, `AA`, `AF`) are accepted but not kept. Any other line returns false. Mutates the twin only.

### `fn feed_8255(&mut self, b: u8) -> u8` [@ANCHOR: radioctl:radioctl_skanti_twin_feed_8255]

Handles one byte of the TRP8255 control-unit dialect and returns the single answer byte, ACK 0x06 or NAK 0x15. While a line is being collected every byte is appended and answered ACK until the CR, when the finished line goes to `apply_8255_line` and the answer is ACK for an accepted line and NAK for a refused one. A line starts with one of `:` `;` `<` `d` `y` (acknowledged). A set of single bytes the radio takes without effect (0x01, 0x02, 0x16 and several letters) is acknowledged; `u` and `v` set and clear the modelled PTT; a code in the description's mode table sets the mode; every other byte is NAK. Mutates the twin only.

### `fn apply_8255_line(&mut self, line: &[u8]) -> bool` [@ANCHOR: radioctl:radioctl_skanti_twin_apply_8255_line]

Applies a completed TRP8255 line, CR included, and says whether it is valid. `:` plus six digits sets the receive frequency; `;` plus six digits sets the transmit frequency (units of 100 Hz; non-digits are refused); `:;` makes the transmit frequency equal to the receive frequency; a line starting with `<`, `d` or `y` is accepted and ignored. Anything else returns false. Mutates the twin only.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_skanti_twin_snapshot]

Reports the twin's state for oracle comparison. Always `Some`: `frequency` is the receive frequency in hertz (stored 100 Hz units times 100), `mode_code` is the current ASCII mode code and `ptt` is the modelled key state. The transmit frequency is not included. Immutable, no side effects.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_skanti_twin_feed]

Consumes controller bytes and returns the radio's answers; the time argument is unused. For the TRP8255 every byte is passed to `feed_8255` and its one answer byte is appended, so the output has as many bytes as the input. For the TRP8000 bytes are collected in a line until a CR, then the line goes to `apply_8000` and, if accepted, one `>` prompt is appended; a refused line produces nothing. Mutates the twin only; no I/O.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_skanti_twin_front_panel]

Simulates a knob on the radio. `panel` is owned. `Panel::Frequency(hz)` sets both the receive and transmit frequency (hertz divided by 100); `Panel::Ptt(on)` sets the modelled key state. Other controls return `Err("a Skanti twin has no <panel> knob")`. Success returns an empty vector. No range check. Nothing is transmitted.

## `crates/radioctl-protocols/src/racal.rs`

### `fn words(self) -> (&'static str, &'static str, &'static str, &'static str)` [@ANCHOR: radioctl:radioctl_racal_dialect_words]

Gives the four command words of a dialect, as a tuple of ask-frequency, ask-mode, set-frequency and set-mode. The RA3702 packet dialect uses `QF`, `QM`, `F` and `M`; the Racal-6790 family (RA6790, RX-340 and RX-331) uses `TF`, `TDI`, `F` and `D`. The value is `Copy` and passed by value; pure, no errors. The engine adds the dialect's prefix, framing and terminator.

### `fn parse_frequency(self, text: &[u8]) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_racal_dialect_parse_frequency]

Reads the number in a frequency reply according to the dialect. `text` is the digits (and point) without the command letter, borrowed. The RA3702 reports whole hertz, so the text is parsed as a decimal integer. The Racal-6790 family reports megahertz as a decimal, parsed with `parse_mhz` (up to six decimals, giving hertz). Returns `None` when the text is not a valid number for that dialect, for example a point in a RA3702 reply. Pure.

## `crates/radioctl-protocols/src/tentec_rx320.rs`

### `pub fn default_filter(mode: Mode) -> u8` [@ANCHOR: radioctl:radioctl_tentec_rx320_default_filter]

Chooses the filter index the engine selects for a mode. `mode` is passed by value. The wanted width is 600 Hz for CW and CW reverse, 2400 Hz for LSB and USB, and 6000 Hz for every other mode; the result is the position of that width in the radio's 34 entry `FILTERS` table (the index byte the radio takes). Returns 0 if the width were missing from the table, which does not happen for these values. Pure.

### `fn sideband(mode: Mode) -> i64` [@ANCHOR: radioctl:radioctl_tentec_rx320_sideband]

Gives the sideband sign `m` of the tuning formula: -1 for LSB (lower sideband), CW and CW reverse (the lower side), +1 for USB, and 0 for every other mode (AM, FM and so on). `mode` is passed by value. Pure, no errors. The value multiplies the correction in `tuning_words_for` and `frequency_of_for`.

### `fn correction(variant: Variant, mode: Mode, width: u32) -> i64` [@ANCHOR: radioctl:radioctl_tentec_rx320_correction]

Gives the correction `c`, in hertz, of the tuning formula. `variant` selects the radio, `mode` is the operating mode and `width` is the selected filter width in hertz. For all non-CW modes it is half the width plus 200 Hz. In CW the RX-320 uses 0, while the Pegasus TT-550 uses half the width plus 200 Hz plus its fixed 700 Hz CW BFO offset. The width is not range checked. Pure; used for the tuning words and their inverse.

### `pub fn tuning_words(hz: Hz, mode: Mode, filter: u8) -> Option<[u16; 3]>` [@ANCHOR: radioctl:radioctl_tentec_rx320_tuning_words]

The tuning words of an RX-320 (the `Rx320` variant) for a frequency: the coarse, fine and BFO words, in that order, each 16 bits. `hz` is in hertz, `mode` and `filter` (an index into `FILTERS`, 0 to 33) fix the correction and sideband. Returns `None` for an unknown filter index or when a word cannot hold the value (the frequency is below the formula's offset or too large). It is the RX-320 case of `tuning_words_for`. Pure.

### `pub fn frequency_of(words: [u16; 3], mode: Mode, filter: u8) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_tentec_rx320_frequency_of]

The inverse of `tuning_words` for the RX-320: the frequency in hertz that coarse, fine and BFO `words` tune to in `mode` with the filter at index `filter`. Returns `None` for an unknown filter or a negative result. The BFO word is not used. Used by the twin to report what it was told. Pure; it is the RX-320 case of `frequency_of_for`.

### `pub fn from_name(name: &str) -> Option<Variant>` [@ANCHOR: radioctl:radioctl_tentec_rx320_variant_from_name]

Maps a dialect name to the radio variant: `rx320` gives `Variant::Rx320` and `tt550` gives `Variant::Tt550` (the Pegasus receive side). Case sensitive; any other name returns `None`, which the engine and twin turn into `Unsupported`. Pure.

### `pub fn tuning_words_for(variant: Variant, hz: Hz, mode: Mode, filter: u8) -> Option<[u16; 3]>` [@ANCHOR: radioctl:radioctl_tentec_rx320_tuning_words_for]

Computes the coarse, fine and BFO tuning words for a radio variant. `hz` is the frequency in hertz, `mode` and `filter` (index 0 to 33 into the filter table) select the correction `c` and sideband `m`. The adjusted frequency is `hz - 1250 + m * c`; the coarse word is `adjusted / 2500 + 18000`, the fine word is `(adjusted mod 2500) * 5.46` rounded down (done in integers as `* 546 / 100`) and the BFO word is `(c + 8000) * 2.73`. Returns `None` for a filter index beyond the table, a negative adjusted frequency or any word that does not fit in 16 bits. Pure; formulas from Hamlib and the Pegasus guide, not verified on a radio.

### `pub fn frequency_of_for(variant: Variant, words: [u16; 3], mode: Mode, filter: u8) -> Option<Hz>` [@ANCHOR: radioctl:radioctl_tentec_rx320_frequency_of_for]

The inverse of `tuning_words_for`: the frequency, in hertz, that the coarse and fine words tune to for a variant, mode and filter index. The fine word is floor(x * 5.46), so the smallest x with that product at least the word is recovered with integer arithmetic; the result is `(coarse - 18000) * 2500 + x + 1250 - m * c`. The BFO word is ignored. Returns `None` for an unknown filter or a negative result. Round-trips with `tuning_words_for` within one hertz of rounding. Pure.

### `fn tune_frame(words: [u16; 3]) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_tentec_rx320_frame]

Builds the tuning command: the letter `N`, the three words each as two big endian bytes (coarse, fine, BFO), then CR, eight bytes in all. `words` is copied. Returns a new vector; pure, no errors.

### `pub fn new(desc: RigDescription) -> Result<Rx320Engine, RigError>` [@ANCHOR: radioctl:radioctl_tentec_rx320_engine_new]

Builds the engine for an RX-320 or a Pegasus TT-550 receive side. `desc` is owned and kept. The dialect must be `rx320` or `tt550` (error `Unsupported` "not an RX-320 or TT-550 file" otherwise, also for a non-vendor description). The engine starts with the description's default frequency and default mode, which it needs to compute the words of later commands. No I/O.

### `fn encode(&mut self, op: &Op) -> Result<Request, RigError>` [@ANCHOR: radioctl:radioctl_tentec_rx320_engine_encode]

Builds the command bytes; no reply is expected. `op` is borrowed. `SetFrequency(A, hz)` computes the tuning words with `tuning_words_for` for the engine's variant, current mode and that mode's default filter, remembers `hz`, and returns the `N` tuning frame; a frequency the words cannot hold returns `Unsupported`. `SetMode(mode)` needs the mode in the description (else `Unsupported`), picks the mode's default filter, recomputes the words for the remembered frequency, remembers the mode, and sends three frames in a row: `W` with the filter index and CR, the `N` tune frame, and `M` with the mode code and CR (on the TT-550 the mode digit is sent twice, receive and transmit). Other operations or missing capabilities return `Unsupported`. This is a receive-side engine and nothing here keys a transmitter.

### `fn feed(&mut self, bytes: &[u8], _now: Duration) -> Vec<Inbound>` [@ANCHOR: radioctl:radioctl_tentec_rx320_engine_feed]

The radio sends nothing back in this protocol, so any received data is unexpected: an empty slice gives an empty vector and any other slice is returned as one `Inbound::Unrecognised` holding a copy of the bytes. The time argument is unused because nothing is buffered. No state changes and no errors.

### `pub fn new(desc: &RigDescription) -> Result<Rx320Twin, RigError>` [@ANCHOR: radioctl:radioctl_tentec_rx320_twin_new]

Builds a virtual RX-320 or TT-550 for tests. `desc` is borrowed and cloned; the dialect must be `rx320` or `tt550` or `Unsupported` is returned. The twin starts with the description's default frequency and default mode, the filter index that mode's default filter, no tuning words received yet, and an empty buffer. No I/O.

### `fn snapshot(&self) -> Option<TwinSnapshot>` [@ANCHOR: radioctl:radioctl_tentec_rx320_twin_snapshot]

Reports the twin's state for oracle comparison. Always `Some`. `frequency` is decoded from the last tuning words received, using the current mode and filter (so it shows what the controller really sent), or the held frequency if no words have arrived or they do not decode. `mode_code` is the description's code for the current mode (`None` if absent) and `ptt` is always false because this is a receiver. Immutable.

### `fn feed(&mut self, bytes: &[u8], now: Duration) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_tentec_rx320_twin_feed]

Consumes controller bytes and decodes frames; replies are rare. Bytes are buffered with monotonic time `now`. The frame length follows its first byte: `N` is 8 bytes, `W`, `M` and `G` are 3 (the TT-550 `M` is 4 and its `X` and `P` are 3), anything else is one byte. A frame that does not end in CR is not a frame: its first byte is dropped and the rest re-examined. `W` with an in-range index sets the filter; `N` stores the three words; `M` sets the mode through the description's code table. On the TT-550 `XX` is answered ` DSP START` and `P1` is answered ` RADIO START`, as the programmer's guide gives. Unknown frames are ignored. Mutates the twin.

### `fn front_panel(&mut self, panel: Panel) -> Result<Vec<u8>, String>` [@ANCHOR: radioctl:radioctl_tentec_rx320_twin_front_panel]

Simulates tuning or changing the mode at the radio. `panel` is owned. `Panel::Frequency(hz)` sets the frequency and recomputes the tuning words for the current mode and filter (they become unset if the frequency cannot be represented); `Panel::Mode(mode)` sets the mode without touching the words. Other controls return `Err("an RX-320 twin has no <panel> knob")`. Success returns an empty vector. Nothing is transmitted.
