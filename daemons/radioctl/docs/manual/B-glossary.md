# Appendix B. Glossary

| Term | Meaning |
|---|---|
| **Actor** | The task that owns one rig's transport and engine and runs its commands one at a time. |
| **AGPL-3.0-or-later** | radioctl's licence (chapter [12](12-provenance-and-licensing.md)). |
| **AI (auto-information)** | The Kenwood-style command that makes a radio report changes by itself; radioctl's `SetTransceive`. |
| **Anchor** | A marker comment with a unique id that links a function, its test and its documentation (ADR 0090; chapter [11](11-testing.md)). |
| **BCD** | Binary-coded decimal: two decimal digits per byte, as CI-V frequencies and meters. |
| **CAT** | Computer Aided Transceiver: a radio's serial control protocol. Not one protocol: each maker has its own. |
| **CI-V** | Icom's binary control protocol, `FE FE <to> <from> <cmd> [data] FD`. Xiegu radios speak a subset. |
| **Capability** | One named thing a rig can do (`Capability`); a description lists them. |
| **Event stream** | `rig.events()`: a `Stream` of status changes, one subscriber per call, driven by the application's own task. |
| **Clean room** | Working from documents and observations of behaviour rather than copying someone's code (chapter [12](12-provenance-and-licensing.md)). |
| **Conformance** | A transcript replayed against the engine and the virtual rig of a description. |
| **DTR, RTS** | Serial modem control lines. Some interfaces key a transmitter from them. |
| **Echo (CI-V)** | A single-wire CI-V bus returns every frame the controller sends; the engine skips it, the splitter can produce it. |
| **Engine** | A sans-IO protocol state machine for one family. |
| **Gate, transmit** | The per-rig switch, closed by default, that must be open for any command that can transmit. |
| **Handle** | `RigHandle`, the cloneable access to a rig actor. |
| **Hamlib** | The C library of rig control that radioctl aims to replace for the radios it covers; also the oracle it is tested against. |
| **Oracle** | Hamlib's `rigctld` or `rigctl` used in tests as an independent implementation to compare with. |
| **Observation** | A value a rig reported, with the field it updates. |
| **Panel change** | A knob turned on a virtual rig in a test, as opposed to a command over the wire. |
| **Personality** | A wire protocol the splitter speaks to one client (IC-7300 CI-V or TS-2000 CAT). |
| **Provenance** | For each datum of a rig description, where it came from. |
| **PTT** | Push to talk: keying the transmitter. |
| **Pty** | A pseudo-terminal: a virtual serial port made of a master and a slave end. |
| **RIT, XIT** | Receiver and transmitter incremental tuning: a small offset of the receive or transmit frequency. |
| **`rigctld`** | Hamlib's network server for rigs; `radioctld` speaks the same text protocol. |
| **Sans-IO** | Code that takes bytes and a time as input and returns bytes and events, never touching a device or clock itself. |
| **Scope** | A radio's spectrum display (band scope), delivered over CAT as waveform data on some Icom radios. |
| **Split** | Receiving on one VFO and transmitting on another. In radioctl: A receives, B transmits. |
| **Splitter** | The component that presents one radio to several programs as a virtual IC-7300 or TS-2000. |
| **Transceive** | A radio reporting changes (frequency, mode) unsolicited. |
| **Transcript** | A list of wire exchanges used for replay. |
| **Unverified** | A fact never checked against a real radio. |
| **VFO** | Variable frequency oscillator: a tuning register. Most radios have A and B. |
| **Virtual rig** | A simulated radio generated from a description. |
