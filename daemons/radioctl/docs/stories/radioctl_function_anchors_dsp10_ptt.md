# radioctl function anchors, DSP-10 PTT

Function anchors for the TAPR DSP-10 keying sequence in `crates/radioctl-protocols/src/dsp10.rs`. Keying goes through the transmit gate (the actor refuses `SetPtt(true)` with the gate closed before any stage is written); the unkey needs no gate. The sequence follows the PC program's `t_r_toggle` (UHFA.C v3.96, hardware branch) and is unverified on a radio. Tests: `tests/dsp10_ptt.rs`.

## `crates/radioctl-protocols/src/dsp10.rs`

### `fn encode_stages(&mut self, op: &Op) -> Result<Option<Vec<Stage>>, RigError>` [@ANCHOR: radioctl:radioctl_dsp10_engine_encode_stages]

For `SetPtt(on)` returns the staged key-on or unkey sequence (`None` for every other op, which keeps its single request). Needs `set-ptt` in the description, else `Unsupported`. The third-LO increment is that of the current frequency and sideband (or the open-time default if no frequency was set yet); an unkey also retunes to the current frequency.

### `fn ptt_stages(on: bool, dph: u32, retune: &[u8]) -> Vec<Stage>` [@ANCHOR: radioctl:radioctl_dsp10_ptt_stages]

Key-on: audio gain 0, shift control byte 0x10, wait 50 ms, 0x18, wait 75 ms, 0x1A, Command 84 'T' (DSP transmit, power word 16421), wait 10 ms. Unkey: Command 84 (DSP receive), control bytes 0x18 (25 ms), 0x10 (25 ms), 0x00 (2 ms), 0x04 with the RF-gain byte restored, then `retune`. No error path of its own.

### `fn shift_stages(prefix: &[u8], control: u8, gain: u8, dph: u32, after: u64) -> [Stage; 2]` [@ANCHOR: radioctl:radioctl_dsp10_shift_stages]

One `send_shift_data(R10MHZ)` as two stages: `prefix` and the three Command 8 words and Command 7 enable low, a 10 ms wait, then Command 7 enable high and a wait of `after` ms.

### `fn shift_bytes(word: u32, control: u8, gain: u8, dph: u32, lo_first: bool) -> Vec<u8>` [@ANCHOR: radioctl:radioctl_dsp10_shift_bytes]

The frames of one synthesizer load: optionally the third-LO frame first, three Command 8 words (the 3-bit and 16-bit halves of `word`, then `control` and `gain`), then Command 7 with enable low and high. `shift_frames` calls it with the receive control byte and the 0 dB RF-gain byte; the PTT stages call it with the transmit control bytes. No error path.
