# Data structures

### `Family` [@ANCHOR: radioctl:radioctl_core_description_family]

**Purpose and location.** A protocol family. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:17` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `IcomCiv` | `IcomCiv` | Icom CI-V, binary frames. |
| `KenwoodAscii` | `KenwoodAscii` | Kenwood-style ASCII commands ended by `;`. |
| `ElecraftAscii` | `ElecraftAscii` | Elecraft K3/KX3/KX2 ASCII commands ended by `;` (Kenwood-like, with its own replies). |
| `YaesuNewcat` | `YaesuNewcat` | Yaesu "newcat" ASCII commands ended by `;` (FT-991A, FTDX10, FTDX101). |
| `YaesuLegacy` | `YaesuLegacy` | Yaesu legacy five-byte binary CAT blocks (FT-817, FT-857, FT-897). |
| `Flex` | `Flex` | FlexRadio SmartSDR API: line text over TCP (FLEX-6000 and later). |
| `Hpsdr` | `Hpsdr` | OpenHPSDR (ANAN, Hermes) over UDP, control plane only (Protocol 1 or 2). |
| `Radiod` | `Radiod` | One receive channel of a ka9q-radio `radiod` (RX-888 and the other front ends it drives), a receive-only virtual rig. No family block; the `radiod` group and channel are transport settings (`radiod_io`). |
| `KenwoodHh` | `KenwoodHh` | Kenwood handheld and mobile ASCII commands ended by a carriage return (TH-D7, TM-D710, ...). |
| `SerialVendor` | `SerialVendor` | Vendor serial protocols of the smaller makers (Ten-Tec, Drake, Racal, ...): one engine per protocol, chosen by [`VendorParams::protocol`]. |
| `Receiver` | `Receiver` | Receivers, scanners and marine radios with a line or frame protocol of their own (AOR, Uniden, JRC, Rohde and Schwarz, Icom PCR and marine); see [`ReceiverProtocol`]. |
| `UsbDevice` | `UsbDevice` | USB radios and SDR (software-defined radio) boards that speak vendor control requests or HID reports (Si570 kits, FUNcube, FiFi-SDR, ...). Described, but without an engine: radioctl has no USB control or HID transport yet. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `ic7300_is_civ_at_0x94 (src/description.rs)`.

### `VendorProtocol` [@ANCHOR: radioctl:radioctl_core_description_vendor_protocol]

**Purpose and location.** The protocol of a [`Family::SerialVendor`] rig; each has its own engine file. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:52` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Tentec` | `Tentec` | Ten-Tec (`tentec.rs`): Orion/Eagle ASCII, Argonaut/Jupiter/Omni VII binary. |
| `Drake` | `Drake` | Drake R-8A and R-8B receivers (`drake.rs`). |
| `Alinco` | `Alinco` | Alinco DX-77 and DX-SR8 (`alinco.rs`). |
| `Racal` | `Racal` | Racal RA6790/GM, RA3702 and Ten-Tec RX-331 and RX-340 receivers (`racal.rs`). |
| `Commradio` | `Commradio` | CommRadio CTX-10: CRC-framed binary on a 3 Mbaud line (`commradio.rs`). |
| `Adat` | `Adat` | ADAT ADT-200A: `$`-prefixed ASCII commands (`adat.rs`). |
| `Gomspace` | `Gomspace` | GomSpace GS100 ground-station transceiver: a shell with a prompt (`gomspace.rs`). |
| `Rshfiq` | `Rshfiq` | HobbyPCB RS-HFIQ: `*`-prefixed ASCII commands (`rshfiq.rs`). |
| `Rft` | `Rft` | RFT EKD-500 receiver: a set-only ASCII frequency command (`rft.rs`). |
| `Drt1` | `Drt1` | SAT-Schneider DRT1 DRM receiver: tuned by clocking a DDS through the RTS, DTR and break lines, no bytes (`drt1.rs`). |
| `Prm80` | `Prm80` | Philips PRM8060 / PRM80 family: a prompt-driven console (`prm80.rs`). |
| `Skanti` | `Skanti` | Skanti TRP8000 and TRP 8255 S R (`skanti.rs`). |
| `Barrett` | `Barrett` | Barrett 950, 2050 and 4050 (`barrett.rs`). |
| `Codan` | `Codan` | CODAN Envoy and NGT (`codan.rs`). |
| `Kachina` | `Kachina` | Kachina 505DSP (`kachina.rs`). |
| `Lowe` | `Lowe` | Lowe HF-235 (`lowe.rs`). |
| `Guohe` | `Guohe` | Guohe PMR-171 (`guohe.rs`). |
| `Dorji` | `Dorji` | Dorji DRA818U and DRA818V modules (`dorji.rs`). |
| `Mds` | `Mds` | MDS 4710 and 9710 data radios (`mds.rs`). |
| `Xg3` | `Xg3` | Elecraft XG3 signal source (`xg3.rs`). |
| `Transfox` | `Transfox` | SigFox Transfox transverter controller (`transfox.rs`). |
| `Paragon` | `Paragon` | Ten-Tec TT-585 Paragon (`paragon.rs`). |
| `Micom` | `Micom` | Motorola MICOM 2 and 3 (`micom.rs`). |
| `Hiqsdr` | `Hiqsdr` | N2ADR HiQSDR UDP control frame (`hiqsdr.rs`). |
| `Wj` | `Wj` | Watkins-Johnson WJ-8888 (`wj.rs`). |
| `Anytone` | `Anytone` | AnyTone D578A (`anytone.rs`). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/description.rs:111).

**See also:** test `vendor_protocol_names_round_trip (src/description.rs)`.

### `VendorParams` [@ANCHOR: radioctl:radioctl_core_description_vendor_params]

**Purpose and location.** Parameters of a [`Family::SerialVendor`] rig. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:179` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `VendorProtocol` | The protocol (engine file). |
| `dialect` | `String` | The model variant inside the protocol (`orion`, `argonaut`, ...); the engine validates it. |
| `data_bits` | `u8` | Data bits on the serial line (8 unless the description says otherwise). |
| `parity` | `char` | Parity: `N`, `E` or `O`. |
| `stop_bits` | `u8` | Stop bits. |
| `flow` | `String` | Flow control: `none`, `xonxoff` or `rtscts`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `vendor_protocol_names_round_trip (src/description.rs)`.

### `UsbProtocol` [@ANCHOR: radioctl:radioctl_core_description_usb_protocol]

**Purpose and location.** The USB protocol of a [`Family::UsbDevice`] rig. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:196` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Si570` | `Si570` | Si570 synthesizer boards on the V-USB shared id (SoftRock AVR-USB, KTH PIC-USB, Peaberry, FA-SDR): vendor control requests with a frequency word in 11.21 fixed point. |
| `Si570Legacy` | `Si570Legacy` | The same boards with DG8SAQ firmware older than 0x0F00: the Si570 divider registers are solved on the host and sent with request 0x30 (read with 0x3F). |
| `Fifisdr` | `Fifisdr` | FiFi-SDR: the Si570 requests plus its own read and write requests. |
| `Funcube` | `Funcube` | AMSAT-UK FUNcube Dongle and Pro+: 64-byte HID reports. |
| `Elektor507` | `Elektor507` | Elektor SDR-USB: an FTDI chip in bit-bang mode. |
| `Dwt` | `Dwt` | Digital World Traveller DRM receiver. |
| `Perseus` | `Perseus` | Microtelecom Perseus: the user's FX2 firmware and FPGA image are loaded when the board is opened (`radioctl_protocols::perseus`); the engine sends front-end operations to that session. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/description.rs:244).

**See also:** test `usb_protocol_names_round_trip (src/description.rs)`.

### `UsbParams` [@ANCHOR: radioctl:radioctl_core_description_usb_params]

**Purpose and location.** Parameters of a [`Family::UsbDevice`] rig. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:251` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `UsbProtocol` | The device's protocol. |
| `vendor_id` | `u16` | USB vendor id. |
| `product_id` | `u16` | USB product id. |
| `manufacturer` | `String` | The manufacturer string the device reports (V-USB boards share one id and are told apart by it); empty when none is used. |
| `product` | `String` | The product string the device reports; empty when none is used. |
| `multiplier` | `u8` | The Si570 frequency multiplier of the board (2 or 4), 0 for a device that has none. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_usb` (src/rigfile.rs:850).

**See also:** test `usb_protocol_names_round_trip (src/description.rs)`.

### `ReceiverProtocol` [@ANCHOR: radioctl:radioctl_core_description_receiver_protocol]

**Purpose and location.** Which protocol engine a [`Family::Receiver`] description uses. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:268` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Aor` | `Aor` | AOR ASCII commands (AR8000, AR8200, AR8600, AR5000, AR3000A, AR3030, AR2700, SR2200). |
| `Uniden` | `Uniden` | Uniden and Bearcat scanners (BC245, BC250, BC780, BC895, BC898, BCD-396T, BCD-996T). |
| `Jrc` | `Jrc` | JRC receivers and transceivers (NRD-525, NRD-535, NRD-545, JST-145, JST-245). |
| `RohdeSchwarz` | `RohdeSchwarz` | Rohde and Schwarz (EB200, ESMC, EK895/6, XK852, XK2100). |
| `IcomPcr` | `IcomPcr` | Icom PCR computer-controlled receivers (PCR100, PCR1000, PCR1500, PCR2500). |
| `IcomMarine` | `IcomMarine` | Icom marine transceivers (IC-M700PRO, IC-M710, IC-M802, IC-M803). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/description.rs:299).

**See also:** test `receiver_protocol_names_round_trip (src/description.rs)`.

### `ReceiverParams` [@ANCHOR: radioctl:radioctl_core_description_receiver_params]

**Purpose and location.** Receiver-family parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:315` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `protocol` | `ReceiverProtocol` | The protocol engine. |
| `dialect` | `String` | The model's command set within the protocol (`ar8000`, `bc780`, ...); the engine's tables are keyed by it. |
| `address` | `Option<u8>` | The bus address (the Icom marine radios' and JRC's), when the protocol has one. |
| `baud` | `u32` | The default line speed in baud. |
| `data_bits` | `u8` | Data bits on the serial line. |
| `parity` | `char` | Parity: `N`, `E` or `O`. |
| `stop_bits` | `u8` | Stop bits. |
| `flow` | `String` | Flow control: `none`, `xonxoff` or `rtscts`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_receiver` (src/rigfile.rs:867).

**See also:** test `receiver_protocol_names_round_trip (src/description.rs)`.

### `CivParams` [@ANCHOR: radioctl:radioctl_core_description_civ_params]

**Purpose and location.** CI-V parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:337` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `address` | `u8` | The rig's CI-V address. |
| `controller` | `u8` | The controller's address (the software side), conventionally 0xE0. |
| `model_id` | `u8` | The byte the rig returns for the read-ID command. |
| `echo` | `bool` | True when the bus echoes every transmitted frame back (single-wire interface). |
| `frequency_bytes` | `u8` | Bytes of BCD in a frequency field: 5 (ten digits) for most models, 4 (eight digits, Hamlib's `civ_731_mode`: IC-735, IC-820H, IC-821H) and 6 for a model whose field is 5 bytes up to 5.85 GHz and 6 bytes above it (Hamlib's IC-905 rule). See [`CivParams::field_len`]. |
| `command_set` | `CivCommandSet` | Which command set sets the frequency and the mode (see [`CivCommandSet`]). |
| `main_sub` | `CivMainSub` | Which Main/Sub band commands (`07 D0`...) the model has (see [`CivMainSub`]). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `ic7300_is_civ_at_0x94 (src/description.rs)`.

### `CivMainSub` [@ANCHOR: radioctl:radioctl_core_description_civ_main_sub]

**Purpose and location.** The Main/Sub band commands of a dual-receiver Icom (command `07`, sub commands `B0` to `D2`). Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:358` as a pub `enum`. It derives Debug, Clone, Copy, Default, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `None` | `None` | None: the model has one receiver or no documented form. |
| `Select` | `Select` | `07 D0` selects the Main band and `07 D1` the Sub band; the book has no read of the selection and no dual watch command (IC-9100). |
| `Full` | `Full` | `Select` plus `07 D2` (read the selected band) and dual watch `07 C0` off, `07 C1` on, `07 C2` read (IC-7610). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `ic7300_is_civ_at_0x94 (src/description.rs)`.

### `CivCommandSet` [@ANCHOR: radioctl:radioctl_core_description_civ_command_set]

**Purpose and location.** The command numbers a CI-V model uses to set the frequency and to read and set the mode. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:372` as a pub `enum`. It derives Debug, Clone, Copy, Default, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Standard` | `Standard` | `05` sets the frequency, `04` reads the mode and `06` sets it (every model but one). |
| `F8101` | `F8101` | The IC-F8101: `1A 35` sets the frequency, `1A 34` reads the mode and `1A 36` sets it, each with a two-byte mode field (`00`, code). The frequency is still read with `03`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `ic7300_is_civ_at_0x94 (src/description.rs)`.

### `KenwoodDialect` [@ANCHOR: radioctl:radioctl_core_description_kenwood_dialect]

**Purpose and location.** Which command set a Kenwood-style rig speaks. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:395` as a pub `enum`. It derives Debug, Clone, Copy, Default, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Classic` | `Classic` | The TS-2000 and TS-590 command set: `IF`, `MD`, `AG0nnn`. |
| `Ts890` | `Ts890` | The TS-890S command set: no `IF`, the mode through `OM`, `AGnnn`, split through `TB`, one-digit `RA` and `PA`. |
| `Ts440` | `Ts440` | The older Kenwood set of the TS-440S (IC-10 manual) and its relatives: `IF` as the classic set, but `MD` only sets (the mode is read from `IF`), `FN` and `SP` replace `FR` and `FT`, `RU` and `RD` take no parameter, and there is no `SM`, `AG`, `RG`, `PC` or `KS`. |
| `Ts450` | `Ts450` | The TS-450S and TS-690S set (their external-control manual): as `Ts440` but split is set by `FR` and `FT` (set only) and `AI` has one digit. |
| `Ts990` | `Ts990` | The TS-990S set (its PC command reference): classic `IF`, `FA`, `RT`, `XT`, `RC`, `RU`/`RD`, `SM0`, `AI2`, but the mode is `OM0` (read) and `OM0<m>` (set), there is no `MD`, `FR`, `FT` or transmit state in `IF`, and the sub band is a second receiver and not a VFO B. |
| `R5000` | `R5000` | The R-5000 set (Hamlib's IC-10 backend, no Kenwood document read): the TS-440S set with an `IF` answer five characters shorter (30 characters between `IF` and `;`: the first offset field is absent), split by `FN` and `SP`. |
| `Trc80` | `Trc80` | The TRC-80 set (Hamlib's `kenwood_get_freq_if`): the classic set, but the frequency is read from `IF` and not from `FA`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/description.rs:439).

**See also:** test `kenwood_dialect_names_round_trip (src/description.rs)`.

### `KyForm` [@ANCHOR: radioctl:radioctl_core_description_ky_form]

**Purpose and location.** How a Kenwood model takes `KY` (send CW text), as its own manual documents it. The command is used only where a manual of that model documents it (Bruce's decision of 2026-11-02): the text field width, the characters and the stop differ per model. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:457` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Undocumented` | `Undocumented` | No manual read documents `KY` for this model: send-CW-text is refused as not supported. |
| `Padded` | `Padded` | `KY`, a space (P1) and a fixed field of `width` characters padded with spaces. |
| `Variable` | `Variable` | `KY2` and up to `width` characters with no padding (TS-890S P1 = 2). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `ts2000_is_kenwood_without_ptt_readback (src/description.rs)`.

### `KyStop` [@ANCHOR: radioctl:radioctl_core_description_ky_stop]

**Purpose and location.** How a documented `KY` message is stopped in flight. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:468` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `None` | `None` | No stop is documented for the model: send-CW-text is not offered for it, because a message that cannot be stopped is not allowed. |
| `Ky0` | `Ky0` | `KY0;` (Set 2 of the TS-590, TS-890 and TS-990 guides). |
| `Blank` | `Blank` | `KY`, a space and a field of only spaces (TS-480 guide: all-space P2 stops the message). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `ts2000_is_kenwood_without_ptt_readback (src/description.rs)`.

### `KyParams` [@ANCHOR: radioctl:radioctl_core_description_ky_params]

**Purpose and location.** The `KY` capability of one Kenwood model. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:480` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `form` | `KyForm` | The form of the command. |
| `width` | `u8` | Characters in the text field (24 in every guide read). |
| `lowercase` | `bool` | Whether lower-case letters are in the guide's character table. |
| `at_sign` | `bool` | Whether `@` is in the guide's character table. |
| `stop` | `KyStop` | How a message in flight is stopped. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_ky` (src/rigfile.rs:524).

**See also:** test `ts2000_is_kenwood_without_ptt_readback (src/description.rs)`.

### `KenwoodParams` [@ANCHOR: radioctl:radioctl_core_description_kenwood_params]

**Purpose and location.** Kenwood-style parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:559` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `model_id` | `u16` | The number the rig returns for `ID;`. |
| `dialect` | `KenwoodDialect` | The command set. |
| `ky` | `Option<KyParams>` | The model's `KY` (send CW text) record; `None` where the description does not record one. |
| `scope_serial` | `bool` | TS-890S only: the bandscope and filter scope come over the serial (COM/USB) link as split `DD2`/`DD3` frames (`scope_link = "serial"`) rather than as LAN `##DD2`/`##DD3` frames. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `ts2000_is_kenwood_without_ptt_readback (src/description.rs)`.

### `HhDialect` [@ANCHOR: radioctl:radioctl_core_description_hh_dialect]

**Purpose and location.** Which command set a Kenwood handheld or mobile speaks. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:573` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Th` | `Th` | TH-D7, TH-F6/F7, TH-G71, TM-D700, TM-V7: `FQ`, `MD`, `BC`, `TX`/`RX`. |
| `Tm710` | `Tm710` | TM-D710 and TM-V71: the fixed-width `FO` line (47 characters, mode in the last). |
| `Th72` | `Th72` | TH-D72: the `FO` line of 52 characters, the mode in the last. |
| `Th74` | `Th74` | TH-D74: the `FO` line of 72 characters, the mode at column 31, `MD v,k` to set it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/description.rs:598).

**See also:** test `handheld_dialect_names_round_trip (src/description.rs)`.

### `KenwoodHhParams` [@ANCHOR: radioctl:radioctl_core_description_kenwood_hh_params]

**Purpose and location.** Kenwood handheld and mobile parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:611` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `dialect` | `HhDialect` | The command set. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `kenwood_dialect_names_round_trip (src/description.rs)`.

### `ElecraftParams` [@ANCHOR: radioctl:radioctl_core_description_elecraft_params]

**Purpose and location.** Elecraft-style parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:618` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `model_id` | `u16` | The number the rig returns for `ID;` (017 on the K3, KX3 and KX2). |
| `level_ranges` | `Vec<(Level, u16, u16)>` | Per-model wire ranges `(level, min, max)` that replace the K3 ranges of the engine's `level_wire` (the KX3's `AG` is 0-60, `RG` 190-250, `MG` 0-80 and `PC` 0-15). Empty means the K3 ranges apply. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/description.rs:629), `build_elecraft` (src/rigfile.rs:942).

**See also:** test `the_elecraft_family_needs_its_own_block_and_fact (src/rigfile.rs)`.

### `ScopeParams` [@ANCHOR: radioctl:radioctl_core_description_scope_params]

**Purpose and location.** Spectrum scope (band scope) parameters of a model, for waveform data the rig sends on its own (Icom CI-V command 0x27). Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:649` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `receivers` | `u8` | Independent scopes (receivers): 1, or 2 on a Main/Sub rig. |
| `line_length` | `u16` | Amplitude samples in one complete sweep ("line"). |
| `samples_per_frame` | `u16` | Amplitude samples carried by one waveform frame. |
| `amplitude_max` | `u16` | Largest amplitude sample the rig sends (0 is the bottom of the display). |
| `spans_hz` | `Vec<u64>` | The span choices in hertz (full width, as the rig's menu shows them). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_scope_block_is_icom_only_sane_and_needs_its_facts (src/rigfile.rs)`.

### `PowerMeterParams` [@ANCHOR: radioctl:radioctl_core_description_power_meter_params]

**Purpose and location.** The calibration points of a rig's raw power meter: `(raw, milliwatts)`, ascending in both. A rig that reports only a relative bar has none, and radioctl then reports a relative value. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:665` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `points` | `Vec<(u16, u32)>` | Ascending `(raw reading, milliwatts)` points. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_power_meter_block_needs_a_calibration_and_its_fact (src/rigfile.rs)`.

### `LevelParams` [@ANCHOR: radioctl:radioctl_core_description_level_params]

**Purpose and location.** The receive attenuator and preamplifier steps a model has. Empty and zero mean the model has neither, or radioctl knows no command for them: the engines then refuse the levels. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:686` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `attenuator_db` | `Vec<u8>` | The attenuation in decibels of each attenuator step, ascending; step `n` (1-based) is the `n`th entry and step 0 is off. A one-entry list is an attenuator that is simply on or off. |
| `attenuator_any_up_to_db` | `u8` | For a step attenuator that takes any whole number of decibels from 0 up to this (an OpenHPSDR radio's 0 to 31 dB), the largest; 0 when `attenuator_db` lists the steps. |
| `preamp_steps` | `u8` | How many preamplifier steps above off the model has (0 for none). |
| `antenna_ports` | `u8` | How many antenna connectors the rig itself selects between (0 for none, or no known command). |
| `absent` | `Vec<Level>` | Levels (of the plain `14` family: AF, RF, microphone, monitor, keyer, power) the model's command table does not have; the CI-V engine and virtual rig refuse them. Empty means the model has them all, which is what a description without `levels.absent` states. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_levels` (src/rigfile.rs:1003).

**See also:** test `a_levels_block_is_sane_and_needs_its_facts (src/rigfile.rs)`.

### `YaesuParams` [@ANCHOR: radioctl:radioctl_core_description_yaesu_params]

**Purpose and location.** Yaesu newcat parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:758` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `model_id` | `u16` | The number the rig returns for `ID;` (`ID0670;` is 670). |
| `frequency_digits` | `u8` | Digits of the `FA`/`FB` frequency and of the `IF` VFO-A frequency: 9 on the FT-991A, FT-891, FT-710, FTDX10 and FTDX101, 8 on the FT-450, FT-950, FT-2000, FTDX1200, FTDX3000, FTDX5000 and FTDX9000. |
| `level_ranges` | `Vec<(Level, u16, u16)>` | Per-model wire ranges `(level, min, max)` that replace the engine's defaults (the FT-891's `RG` is 0-30, the FTDX5000's `MG` and `PC` are 0-255). Empty means the defaults apply. |
| `clarifier` | `YaesuClarifier` | How the clarifier is switched: by `RT`/`XT` (the default), or by `CF` on the FT-891. |
| `split_st` | `bool` | True when split is the `ST` command (FT-891, FT-710, FTDX10, FTDX101: `ST1;` is on) instead of `FT2;`/`FT3;` (FT-991A and the models where `FT` picks the transmit VFO). |
| `tx_band` | `bool` | True when `FT` picks the transmitting band, Main or Sub (`FT2;` Main, `FT3;` Sub; the read answers `FT0;` or `FT1;`) instead of the VFO: FTDX5000 and FTDX9000. Read and set through `Op::GetTxReceiver` and `Op::SetTxReceiver`. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `a_yaesu_frequency_width_is_8_or_9_and_a_non_default_needs_its_fact (src/rigfile.rs)`.

### `YaesuClarifier` [@ANCHOR: radioctl:radioctl_core_description_yaesu_clarifier]

**Purpose and location.** The command that switches the clarifier on a Yaesu newcat model. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:781` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, Default.

| Variant | Definition | Meaning |
|---|---|---|
| `RtXt` | `RtXt` | `RT` (receive) and `XT` (transmit), with `RC`/`RU`/`RD` for the offset: FT-991A and most others. |
| `CfReceive` | `CfReceive` | `CF P1 P2 P3` (P2 0 off, 1 on): the receive clarifier only, there is no transmit clarifier command; the offset is still `RC`/`RU`/`RD`. FT-891. |
| `CfRxTx` | `CfRxTx` | `CF P1 P2 P3 P4 P5 P6 P7 P8`: one command switches the receive (P4) and transmit (P5) clarifiers together (P3 0) and sets the offset (P3 1, a sign and four digits); there is no `RT`, `XT`, `RC`, `RU` or `RD`. FT-710. |
| `RtReceive` | `RtReceive` | `RT` (receive only) with `RC`/`RU`/`RD` for the offset, and no `XT`: the book has no transmit clarifier command. FT-450 and FT-450D. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_clarifier_cf_needs_a_fact_and_refuses_other_words (src/rigfile.rs)`.

### `YaesuLegacyParams` [@ANCHOR: radioctl:radioctl_core_description_yaesu_legacy_params]

**Purpose and location.** Yaesu legacy five-byte CAT parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:810` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `stop_bits` | `u8` | Stop bits on the CAT serial line (2 on the FT-817, FT-857 and FT-897). |
| `high_band_flag` | `bool` | True when a frequency from 1.2 to 1.3 GHz is sent as its seven low digits with the nibble C ahead (the FT-736R's 1.2 GHz module, as Hamlib's `ft736_set_freq` does); false for every other model. |
| `block` | `Option<LegacyBlock>` | The block-protocol commands and status reads of the later models (FT-747 to FT-1000MP and the receivers), or `None` for the FT-817 family, whose commands the engine knows itself. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyFreqForm` [@ANCHOR: radioctl:radioctl_core_description_legacy_freq_form]

**Purpose and location.** How a frequency is written into five-byte-block bytes (a command's parameters or a field of a status record). Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:824` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Bcd10HzMsb` | `Bcd10HzMsb` | Eight BCD digits in four bytes, most significant first, 10 Hz units (FT-817 family, FT-767GX status, FRG-9600 set). |
| `Bcd10HzLsb` | `Bcd10HzLsb` | Eight BCD digits in four bytes, least significant byte first, 10 Hz units (the set-frequency parameters of FT-747 to FT-1000MP and the FT-757GXII status). |
| `Bcd1HzMsb` | `Bcd1HzMsb` | Eight BCD digits in four bytes, most significant first, 1 Hz units (FT-747 status; the radio resolves 25 Hz). |
| `Bin10HzMsb3` | `Bin10HzMsb3` | Binary, three bytes, most significant first, 10 Hz units (FT-840, 890, 900, 990, 1000). |
| `Bin10HzLsb3` | `Bin10HzLsb3` | Binary, three bytes, least significant first, 10 Hz units (FRG-100). |
| `Bin10HzMsb4` | `Bin10HzMsb4` | Binary, four bytes, most significant first, 10 Hz units (VR-5000 set). |
| `Bin1HzMsb4` | `Bin1HzMsb4` | Binary, four bytes, most significant first, 1 Hz units (FT-920 status, as Hamlib reads it). |
| `Bin125Msb4` | `Bin125Msb4` | Binary, four bytes, most significant first, 1.25 Hz units (FT-100 status). |
| `Bin0625Msb4` | `Bin0625Msb4` | Binary, four bytes, most significant first, 0.625 Hz units (FT-1000MP status, as Hamlib reads it). |
| `Frg8800` | `Frg8800` | The FRG-8800 form: BCD digits least significant first from the 100 Hz digit, the low nibble of the first byte a one-hot code of the 25 Hz step. Writes only. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `legacy_form` (src/rigfile.rs:1087).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyCommand` [@ANCHOR: radioctl:radioctl_core_description_legacy_command]

**Purpose and location.** A five-byte command with a slot for a parameter. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:985` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `template` | `[u8; 5]` | The block as sent, the slot bytes zero (or whatever a fixed byte there should be). |
| `at` | `u8` | Index in the block of the first slot byte (the mode code, or the first frequency byte). |
| `form` | `Option<LegacyFreqForm>` | How a frequency fills the slot, for the set-frequency command. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyFlag` [@ANCHOR: radioctl:radioctl_core_description_legacy_flag]

**Purpose and location.** A flag in a status record: a byte offset, a mask, and whether the feature is on when a bit of the mask is set. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:996` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `offset` | `u16` | Byte offset in the reply. |
| `mask` | `u8` | Bits of the byte that matter. |
| `on_when_set` | `bool` | True when the feature is on if any masked bit is set; false when it is on if they are all clear. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyRead` [@ANCHOR: radioctl:radioctl_core_description_legacy_read]

**Purpose and location.** One status read: the request, the reply length and where the fields are in the reply. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:1007` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `request` | `[u8; 5]` | The five request bytes. |
| `length` | `u16` | Reply length in bytes. |
| `frequency` | `Option<(u16, LegacyFreqForm)>` | Offset and form of the frequency, if the reply has it. |
| `mode` | `Option<(u16, u8)>` | Offset and mask of the mode byte, if the reply has it. |
| `ptt` | `Option<LegacyFlag>` | PTT (push to talk) flag. |
| `split` | `Option<LegacyFlag>` | Split flag. |
| `vfo_mode` | `Option<LegacyFlag>` | "A VFO, not a memory, is displayed" flag, with VFO A the one active. radioctl never reads it; a controller that does (Hamlib's FT-840, FT-890 and FT-900 backends read it at open and refuse to set a mode or read a frequency when neither it nor a memory flag is set) needs a virtual rig to set it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `LegacyBlock` [@ANCHOR: radioctl:radioctl_core_description_legacy_block]

**Purpose and location.** The commands and status reads of a block-protocol Yaesu legacy model. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:1029` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `set_frequency` | `Option<LegacyCommand>` | Set the frequency. |
| `set_mode` | `Option<LegacyCommand>` | Set the mode; the slot is one byte of the description's mode code. |
| `ptt` | `Option<([u8; 5], [u8; 5])>` | PTT on and PTT off blocks. |
| `split` | `Option<([u8; 5], [u8; 5])>` | Split on and split off blocks. |
| `reads` | `Vec<LegacyRead>` | Status reads. |
| `status_modes` | `Vec<(u8, Mode)>` | Mode codes as the status reads return them (they differ from the set codes on most models). |
| `echo` | `bool` | True when the radio echoes each command block (FT-767GX) and the engine expects the echo. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `build_legacy_block` (src/rigfile.rs:1104).

**See also:** test `yaesu_legacy_in_split_refuses_a_key_on_because_vfo_b_is_unreadable (../radioctl-protocols/tests/tx_frequency.rs)`.

### `FlexParams` [@ANCHOR: radioctl:radioctl_core_description_flex_params]

**Purpose and location.** FlexRadio SmartSDR parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:1048` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `model` | `String` | The model name the radio reports in `info` and in discovery (`FLEX-6300`). |
| `scus` | `u8` | Spectrum capture units (receiver front ends): 1, or 2 on the FLEX-6700. |
| `slice` | `u8` | The slice receiver (0 is slice A) the description's VFO A is; the SmartSDR slice A to H models of Hamlib are slices 0 to 7. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `only_a_flex_description_builds_an_engine (../radioctl-protocols/src/flex.rs)`.

### `HpsdrParams` [@ANCHOR: radioctl:radioctl_core_description_hpsdr_params]

**Purpose and location.** OpenHPSDR network radio parameters. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:1060` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `model` | `String` | Model name (`ANAN-100`). |
| `protocol` | `u8` | Wire protocol, 1 or 2. |
| `anan10` | `bool` | True for the ANAN-10 power calibration constant, false for the ANAN-100. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `protocol_1_virtual_anan_tunes_reports_power_and_keeps_the_gate_closed (../radioctl-protocols/tests/hpsdr_actor.rs)`.

### `Timing` [@ANCHOR: radioctl:radioctl_core_description_timing]

**Purpose and location.** Command timing. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:1071` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `timeout` | `Duration` | How long to wait for a reply. |
| `retries` | `u8` | How many times the actor repeats a command that timed out. |
| `inter_command` | `Duration` | Minimum gap between commands. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `check_timing` (src/rigfile.rs:909).

**See also:** test `timing_that_would_delay_or_defeat_an_unkey_is_rejected (src/rigfile.rs)`.

### `RigDescription` [@ANCHOR: radioctl:radioctl_core_description_rig_description]

**Purpose and location.** Everything radioctl knows about one model. Defined in `daemons/radioctl/crates/radioctl-core/src/description.rs:1082` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `id` | `String` | A stable identifier, e.g. `icom-ic-7300`. |
| `name` | `String` | A display name. |
| `family` | `Family` | The protocol family. |
| `civ` | `Option<CivParams>` | CI-V parameters, present for [`Family::IcomCiv`]. |
| `kenwood` | `Option<KenwoodParams>` | Kenwood parameters, present for [`Family::KenwoodAscii`]. |
| `elecraft` | `Option<ElecraftParams>` | Elecraft parameters, present for [`Family::ElecraftAscii`]. |
| `yaesu` | `Option<YaesuParams>` | Yaesu newcat parameters, present for [`Family::YaesuNewcat`]. |
| `yaesu_legacy` | `Option<YaesuLegacyParams>` | Yaesu legacy parameters, present for [`Family::YaesuLegacy`]. |
| `kenwood_hh` | `Option<KenwoodHhParams>` | Kenwood handheld and mobile parameters, present for [`Family::KenwoodHh`]. |
| `flex` | `Option<FlexParams>` | FlexRadio parameters, present for [`Family::Flex`]. |
| `hpsdr` | `Option<HpsdrParams>` | OpenHPSDR parameters, present for [`Family::Hpsdr`]. |
| `vendor` | `Option<VendorParams>` | Vendor serial parameters, present for [`Family::SerialVendor`]. |
| `receiver` | `Option<ReceiverParams>` | Receiver parameters, present for [`Family::Receiver`]. |
| `usb` | `Option<UsbParams>` | USB device parameters, present for [`Family::UsbDevice`]. |
| `scope` | `Option<ScopeParams>` | Spectrum scope parameters, for the models that have a scope over CAT. |
| `levels` | `LevelParams` | Attenuator and preamplifier steps. |
| `power_meter` | `Option<PowerMeterParams>` | The calibration of the rig's raw power meter, when a source gives one. |
| `capabilities` | `CapabilitySet` | What the rig can do. |
| `mode_codes` | `Vec<(Mode, u8)>` | Mode to wire code: a CI-V mode byte, or the ASCII byte of a Kenwood `MD` digit. |
| `timing` | `Timing` | Command timing. |
| `default_frequency` | `Hz` | Frequency a virtual rig of this model starts at. |
| `default_mode` | `Mode` | Mode a virtual rig of this model starts in. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `ic7300` (src/description.rs:1150), `ts2000` (src/description.rs:1212), `description` (src/engine.rs:74), `description` (src/engine.rs:116), `description` (src/engine.rs:159), `build_description` (src/rigfile.rs:557), `expected_keys` (src/rigfile.rs:1296), `check_coverage` (src/rigfile.rs:1464).

**See also:** test `mode_tables_round_trip (src/description.rs)`.
### `ReplyKind` [@ANCHOR: radioctl:radioctl_core_engine_reply_kind]

**Purpose and location.** What the actor should wait for after sending a request. Defined in `daemons/radioctl/crates/radioctl-core/src/engine.rs:23` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `None` | `None` | Nothing comes back; the command is complete once written. |
| `Ack` | `Ack` | An acknowledgement or a refusal. |
| `Observation` | `Observation(Field)` | An observation of the given field. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `note_written` (src/rig.rs:1669).

**See also:** test `the_request_is_the_engines_own_civ_frequency_read (tests/baud_probe.rs)`.

### `Request` [@ANCHOR: radioctl:radioctl_core_engine_request]

**Purpose and location.** The bytes for one command and the reply to expect. Defined in `daemons/radioctl/crates/radioctl-core/src/engine.rs:34` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `bytes` | `Vec<u8>` | Bytes to write to the transport. |
| `reply` | `ReplyKind` | The reply to wait for. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `read_request` (src/baud_probe.rs:66), `encode` (src/engine.rs:77), `link_restored` (src/engine.rs:99), `open_requests` (src/engine.rs:108), `encode` (src/engine.rs:122), `link_restored` (src/engine.rs:140), `open_requests` (src/engine.rs:146), `encode` (src/engine.rs:162).

**See also:** test `the_request_is_the_engines_own_civ_frequency_read (tests/baud_probe.rs)`.

### `LineStep` [@ANCHOR: radioctl:radioctl_core_engine_line_step]

**Purpose and location.** One step of a control-line sequence: set a line, then hold for `hold` before the next step. Defined in `daemons/radioctl/crates/radioctl-core/src/engine.rs:43` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `line` | `ControlLine` | The line to set. |
| `asserted` | `bool` | True to assert it (RTS, DTR) or put the transmit line in break. |
| `hold` | `Duration` | How long to wait after setting it. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `encode_lines` (src/engine.rs:88), `encode_lines` (src/engine.rs:134).

**See also:** test `a_vendor_descriptions_seven_even_two_line_reaches_the_port (tests/transports.rs)`.

### `Inbound` [@ANCHOR: radioctl:radioctl_core_engine_inbound]

**Purpose and location.** Something decoded from received bytes. Defined in `daemons/radioctl/crates/radioctl-core/src/engine.rs:54` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Observation` | `Observation(Observation)` | A value, whether asked for or sent unsolicited. |
| `ScopeFrame` | `ScopeFrame(ScopeFrame)` | A complete scope sweep. It is not a cache field: the actor publishes it on the scope frame stream (`seq` is assigned there). |
| `FilterScopeFrame` | `FilterScopeFrame(FilterScopeFrame)` | A complete filter-scope sweep. Like [`Inbound::ScopeFrame`] it is not a cache field: the actor publishes it on the filter-scope stream (`seq` is assigned there). |
| `Ack` | `Ack` | The rig accepted the last command. |
| `Nak` | `Nak` | The rig refused the last command. |
| `Unrecognised` | `Unrecognised(Vec<u8>)` | Bytes that formed no known frame. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `feed` (src/engine.rs:80), `feed` (src/engine.rs:128), `feed` (src/engine.rs:168), `claim_reply` (src/rig.rs:1693), `feed` (tests/baud_probe.rs:188).

**See also:** test `a_boxed_engine_is_an_engine (src/engine.rs)`.
### `Capability` [@ANCHOR: radioctl:radioctl_core_capability_capability]

**Purpose and location.** One thing a rig can do. Defined in `daemons/radioctl/crates/radioctl-core/src/capability.rs:8` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `ReadFrequency` | `ReadFrequency` | Read the frequency. |
| `SetFrequency` | `SetFrequency` | Set the frequency. |
| `ReadMode` | `ReadMode` | Read the mode. |
| `SetMode` | `SetMode` | Set the mode. |
| `SecondVfo` | `SecondVfo` | Read the second VFO. |
| `ReadPtt` | `ReadPtt` | Read the transmit state. |
| `SetPtt` | `SetPtt` | Key and unkey the transmitter. Transmit-capable. |
| `ReadSignal` | `ReadSignal` | Read the S-meter. |
| `ReadPower` | `ReadPower` | Read the power meter. |
| `SendCw` | `SendCw` | Send Morse text. Transmit-capable. |
| `Transceive` | `Transceive` | Report changes without being polled (CI-V transceive, Kenwood auto-information). |
| `Split` | `Split` | Read and set split operation. |
| `RitXit` | `RitXit` | Read and set RIT (receiver incremental tuning), XIT (transmitter incremental tuning) and their offset. |
| `Filter` | `Filter` | Read and set the receive filter bandwidth. |
| `Levels` | `Levels` | Read and set numeric levels (gains, keyer speed, power setting). |
| `ReadSwr` | `ReadSwr` | Read the SWR meter. |
| `ReadMeters` | `ReadMeters` | Read the raw transmitter and supply meters (SWR, ALC, COMP, Vd, Id) in the rig's own scale. Read-only. |
| `Identify` | `Identify` | Ask the rig for its identifier and firmware revision. |
| `Scope` | `Scope` | Spectrum scope: settings and waveform frames. |
| `Receivers` | `Receivers` | Select and read the Main or Sub receiver (band) of a dual-receiver rig, the band the transmitter follows, and dual watch. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/capability.rs:90), `of` (src/capability.rs:108), `has` (src/capability.rs:114), `iter` (src/capability.rs:120), `required_capability` (src/op.rs:296).

**See also:** test `only_ptt_and_cw_transmit (src/capability.rs)`.

### `CapabilitySet` [@ANCHOR: radioctl:radioctl_core_capability_capability_set]

**Purpose and location.** The set of capabilities of one rig. Defined in `daemons/radioctl/crates/radioctl-core/src/capability.rs:103` as a pub `struct`. It derives Debug, Clone, Default, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `0` | `BTreeSet<Capability>` | The capabilities, ordered. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `of` (src/capability.rs:108), `capabilities` (src/rig.rs:415).

**See also:** test `set_membership_and_order (src/capability.rs)`.
### `KeyLine` [@ANCHOR: radioctl:radioctl_core_keyer_key_line]

**Purpose and location.** The serial control lines. Defined in `daemons/radioctl/crates/radioctl-core/src/keyer.rs:14` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Rts` | `Rts` | Request To Send. |
| `Dtr` | `Dtr` | Data Terminal Ready. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `set_line` (src/keyer.rs:36), `new` (src/keyer.rs:63), `set_line` (src/keyer.rs:111), `set_line` (src/serial.rs:296), `set_line` (tests/transports.rs:364).

**See also:** test `a_new_keyer_makes_its_line_inactive_and_release_and_drop_do_too (src/keyer.rs)`.

### `ControlLine` [@ANCHOR: radioctl:radioctl_core_keyer_control_line]

**Purpose and location.** A serial control line a protocol engine may drive as data (never a keying line): the two modem outputs and the break state of the transmit line. Defined in `daemons/radioctl/crates/radioctl-core/src/keyer.rs:24` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Rts` | `Rts` | Request To Send. |
| `Dtr` | `Dtr` | Data Terminal Ready. |
| `Break` | `Break` | The break state of the transmit data line (`true` is break, the line held at space). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `set_control` (src/keyer.rs:42), `set_control` (src/serial.rs:319).

**See also:** test `a_transport_that_does_not_drive_lines_as_data_refuses_cleanly (src/keyer.rs)`.

### `LineKeyer` [@ANCHOR: radioctl:radioctl_core_keyer_line_keyer]

**Purpose and location.** Keys a transmitter through one control line. Defined in `daemons/radioctl/crates/radioctl-core/src/keyer.rs:53` as a pub `struct`. It derives Debug.

| Field | Type | Meaning |
|---|---|---|
| `lines` | `C` | The control lines the keyer works on. |
| `line` | `KeyLine` | Which line keys the transmitter (RTS or DTR). |
| `active_high` | `bool` | Whether keying asserts the line (`true`) or releases it (`false`). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/keyer.rs:63).

**See also:** test `a_new_keyer_makes_its_line_inactive_and_release_and_drop_do_too (src/keyer.rs)`.
### `Power` [@ANCHOR: radioctl:radioctl_core_power_power]

**Purpose and location.** Transmit power in watts: a finite, non-negative number. Defined in `daemons/radioctl/crates/radioctl-core/src/power.rs:14` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, PartialOrd, Default.

| Field | Type | Meaning |
|---|---|---|
| `0` | `f64` | The power in watts; the constructor refuses non-finite and negative values. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_watts` (src/power.rs:22), `from_dbm` (src/power.rs:28), `from_dbw` (src/power.rs:34), `from_dbmv_50ohm` (src/power.rs:41), `with_offset_db` (src/power.rs:62), `to_power` (src/power.rs:109), `watts` (src/power.rs:191), `full_scale` (src/power.rs:210).

**See also:** test `dbm_points_convert_to_watts (src/power.rs)`.

### `PowerUnit` [@ANCHOR: radioctl:radioctl_core_power_power_unit]

**Purpose and location.** The unit a power meter reports in. Defined in `daemons/radioctl/crates/radioctl-core/src/power.rs:69` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Watts` | `Watts` | Watts. |
| `Dbm` | `Dbm` | Decibels relative to one milliwatt. |
| `Dbw` | `Dbw` | Decibels relative to one watt. |
| `DbmvOn50Ohm` | `DbmvOn50Ohm` | Decibels relative to one millivolt into 50 ohms. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `from_name` (src/power.rs:94).

**See also:** test `the_coupler_offset_is_applied_before_conversion (src/power.rs)`.

### `PowerOrigin` [@ANCHOR: radioctl:radioctl_core_power_power_origin]

**Purpose and location.** Where a power reading came from. Defined in `daemons/radioctl/crates/radioctl-core/src/power.rs:122` as a pub `enum`. It derives Debug, Clone, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Rig` | `Rig` | The rig's own power meter. |
| `External` | `External(String)` | A wattmeter attached to the shack, by its name in the station configuration. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `unit_and_origin_names_round_trip (src/power.rs)`.

### `PowerReading` [@ANCHOR: radioctl:radioctl_core_power_power_reading]

**Purpose and location.** One transmit-power reading. Defined in `daemons/radioctl/crates/radioctl-core/src/power.rs:142` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `forward` | `Power` | Forward power in watts, or a 0 to 100 relative bar when `relative` is set. |
| `reflected` | `Option<Power>` | Reflected power in watts, when the source measures it. |
| `origin` | `PowerOrigin` | Where it came from. |
| `valid_for` | `Duration` | How long the reading stays good after it was taken. |
| `relative` | `bool` | The figure is a relative 0 to 100 bar, not watts: the rig has no calibration. |
| `scale` | `Option<Power>` | The meter's full scale in watts, when known, so a display can range its bar. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `read` (src/power.rs:164), `rig_reading` (src/power.rs:222), `read` (src/power.rs:524).

**See also:** test `a_rig_reading_is_watts_only_with_a_calibration (src/power.rs)`.

### `PowerCal` [@ANCHOR: radioctl:radioctl_core_power_power_cal]

**Purpose and location.** Calibration of a rig's raw power meter: ascending (raw, watts) points, linear in between, flat beyond the last. Provenance lives in the rig description's `[[facts]]`. Defined in `daemons/radioctl/crates/radioctl-core/src/power.rs:170` as a pub `struct`. It derives Debug, Clone, PartialEq.

| Field | Type | Meaning |
|---|---|---|
| `points` | `Vec<(f64, f64)>` | The (raw, watts) points in ascending order. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `cal` (src/description.rs:693), `new` (src/power.rs:178), `rig_reading` (src/power.rs:222).

**See also:** test `a_calibration_interpolates_and_stops_at_the_ends (src/power.rs)`.

### `DisplayUnit` [@ANCHOR: radioctl:radioctl_core_power_display_unit]

**Purpose and location.** The unit a displayed power is shown in. Defined in `daemons/radioctl/crates/radioctl-core/src/power.rs:246` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Variant | Definition | Meaning |
|---|---|---|
| `Milliwatts` | `Milliwatts` | Milliwatts. |
| `Watts` | `Watts` | Watts. |
| `Kilowatts` | `Kilowatts` | Kilowatts. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `display_unit` (src/power.rs:262), `format_power` (src/power.rs:310), `text` (src/power.rs:456).

**See also:** test `the_formatter_picks_unit_and_decimals (src/power.rs)`.
### `PollConfig` [@ANCHOR: radioctl:radioctl_core_poll_poll_config]

**Purpose and location.** Polling intervals. Defined in `daemons/radioctl/crates/radioctl-core/src/poll.rs:19` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `enabled` | `bool` | Master switch. |
| `fast_min` | `Duration` | Fastest interval for frequency, mode and transmit state. |
| `fast_max` | `Duration` | Slowest interval for those fields. |
| `slow_min` | `Duration` | Fastest interval for meters. |
| `slow_max` | `Duration` | Slowest interval for meters. |
| `ptt_reserve` | `Duration` | The transmit state is read at least this often whatever the command traffic: a PTT poll not done for this long is run before the next queued command. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `default` (src/poll.rs:36).

**See also:** test `only_readable_fields_are_polled (src/poll.rs)`.

### `PollEntry` [@ANCHOR: radioctl:radioctl_core_poll_poll_entry]

**Purpose and location.** One polled field. Defined in `daemons/radioctl/crates/radioctl-core/src/poll.rs:50` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `field` | `Field` | The field. |
| `op` | `Op` | The read command. |
| `min` | `Duration` | The shortest interval between reads of this field. |
| `max` | `Duration` | The longest interval between reads of this field. |
| `interval` | `Duration` | The interval in use now, between `min` and `max`; it is lengthened after a failure or while transceive reports changes. |
| `due` | `Instant` | When this field is next to be read. |
| `polled_at` | `Instant` | When the field was last polled (the schedule's creation before the first poll). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `take_due` (src/poll.rs:133), `take_starved` (src/poll.rs:152), `poll_entry` (src/rig.rs:2032).

**See also:** test `only_readable_fields_are_polled (src/poll.rs)`.

### `PollSchedule` [@ANCHOR: radioctl:radioctl_core_poll_poll_schedule]

**Purpose and location.** The poll schedule of one rig. Defined in `daemons/radioctl/crates/radioctl-core/src/poll.rs:65` as a pub `struct`. It derives Debug, Clone.

| Field | Type | Meaning |
|---|---|---|
| `entries` | `Vec<PollEntry>` | The polled fields. |
| `transceive_active` | `bool` | Whether the rig reports changes itself (transceive), which pins polling to the maximum interval. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `schedule` (src/poll.rs:199).

**See also:** test `only_readable_fields_are_polled (src/poll.rs)`.
### `ProbeResult` [@ANCHOR: radioctl:radioctl_core_baud_probe_probe_result]

**Purpose and location.** What the probe found. Defined in `daemons/radioctl/crates/radioctl-core/src/baud_probe.rs:55` as a pub `struct`. It derives Debug, Clone, PartialEq, Eq.

| Field | Type | Meaning |
|---|---|---|
| `baud` | `u32` | The rate the radio answered at. |
| `tried` | `Vec<u32>` | Every rate opened, in order, the winner last. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** none by signature (it is built or read inline).

**See also:** test `an_open_error_ends_the_probe_with_that_error_not_with_no_answer (tests/baud_probe.rs)`.

### `SerialProbePort` [@ANCHOR: radioctl:radioctl_core_baud_probe_serial_probe_port]

**Purpose and location.** A real serial port as a [`ProbePort`]: each `open` is [`open_serial`] with the configured port at the given rate (lines managed or not as configured, so DTR and RTS are made inactive after every open exactly as for the real link), and `close` waits for the supervisor to release the device. Defined in `daemons/radioctl/crates/radioctl-core/src/baud_probe.rs:169` as a pub `struct`. It derives no traits.

| Field | Type | Meaning |
|---|---|---|
| `runtime` | `Handle` | The tokio runtime handle the port is opened on. |
| `config` | `SerialConfig` | The serial configuration (the rate is replaced at each open). |
| `monitor` | `Option<SerialMonitor>` | The monitor of the open link, `None` when closed. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `new` (src/baud_probe.rs:178).

**See also:** test `the_serial_probe_port_reports_a_missing_device_and_closes_idempotently (tests/baud_probe.rs)`.
### `Hz` [@ANCHOR: radioctl:radioctl_core_units_hz]

**Purpose and location.** A frequency in hertz. Defined in `daemons/radioctl/crates/radioctl-core/src/units.rs:6` as a pub `struct`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Field | Type | Meaning |
|---|---|---|
| `0` | `pub u64` | The frequency in hertz. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `field_len` (src/description.rs:394), `encode` (src/description.rs:914), `decode` (src/description.rs:966), `frequency` (src/rig.rs:670), `rx_frequency` (src/rig.rs:726), `tx_frequency` (src/rig.rs:736), `active_tx_frequency` (src/rig.rs:747), `set_frequency` (src/rig.rs:756).

**See also:** test `from_khz_scales_and_detects_overflow (src/units.rs)`.

### `Mode` [@ANCHOR: radioctl:radioctl_core_units_mode]

**Purpose and location.** An operating mode. Per-family code tables live in the rig description, not here. Defined in `daemons/radioctl/crates/radioctl-core/src/units.rs:24` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Lsb` | `Lsb` | Lower sideband. |
| `Usb` | `Usb` | Upper sideband. |
| `Am` | `Am` | Amplitude modulation. |
| `Cw` | `Cw` | CW (Morse). |
| `CwR` | `CwR` | CW, reverse sideband. |
| `Rtty` | `Rtty` | RTTY / FSK. |
| `RttyR` | `RttyR` | RTTY / FSK, reverse. |
| `Fm` | `Fm` | Narrow FM. |
| `Wfm` | `Wfm` | Wide FM. |
| `Data` | `Data` | Data mode (a sideband mode for digital signals, e.g. Elecraft DATA). |
| `DataR` | `DataR` | Data mode, reverse sideband. |
| `DStar` | `DStar` | D-STAR digital voice (Icom code 17); the D-STAR models only. |
| `Sam` | `Sam` | Synchronous AM, both sidebands (Hamlib SAM; older Hamlib models call it AMS): the receiver locks to the carrier. |
| `SamL` | `SamL` | Synchronous AM, lower sideband only (Hamlib SAL). |
| `SamU` | `SamU` | Synchronous AM, upper sideband only (Hamlib SAH). |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `mode_code` (src/description.rs:1165), `code_mode` (src/description.rs:1174), `mode` (src/rig.rs:762), `set_mode` (src/rig.rs:778), `mode` (src/transcript.rs:196), `from_name` (src/units.rs:88).

**See also:** test `mode_names_round_trip (src/units.rs)`.

### `Receiver` [@ANCHOR: radioctl:radioctl_core_units_receiver]

**Purpose and location.** A receiver (band) of a dual-receiver rig: the Main band or the Sub band (Icom Main/Sub, Yaesu Main/VFO-A band and Sub/VFO-B band of the FTDX 3000/5000/9000/1200). Defined in `daemons/radioctl/crates/radioctl-core/src/units.rs:96` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Main` | `Main` | The Main band. |
| `Sub` | `Sub` | The Sub band. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `watch` (src/net.rs:299), `watch` (src/net.rs:338), `link_changed` (src/rig.rs:2069), `watch` (src/serial.rs:363), `watch` (src/serial.rs:407), `watch` (src/usb_link.rs:111), `watch` (src/usb_link.rs:151).

**See also:** test `observation_maps_to_its_field (src/op.rs)`.

### `Vfo` [@ANCHOR: radioctl:radioctl_core_units_vfo]

**Purpose and location.** A VFO. Only the VFOs a description lists are usable on a given rig. Defined in `daemons/radioctl/crates/radioctl-core/src/units.rs:105` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `A` | `A` | VFO A (the receive/operating VFO unless split). |
| `B` | `B` | VFO B. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `frequency` (src/rig.rs:670), `tx_vfo` (src/rig.rs:720), `set_frequency` (src/rig.rs:756), `vfo` (src/transcript.rs:184), `rx_vfo` (src/txstate.rs:61), `tx_vfo` (src/txstate.rs:67), `frequency` (src/txstate.rs:77), `state` (src/txstate.rs:123).

**See also:** test `vfo_names_round_trip (src/units.rs)`.

### `Level` [@ANCHOR: radioctl:radioctl_core_units_level]

**Purpose and location.** A level the rig exposes as a number in its own units (see the rig description for ranges). Defined in `daemons/radioctl/crates/radioctl-core/src/units.rs:144` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `AfGain` | `AfGain` | Audio (AF) gain. |
| `RfGain` | `RfGain` | RF gain. |
| `MicGain` | `MicGain` | Microphone gain. |
| `MonitorLevel` | `MonitorLevel` | Monitor (sidetone) level. |
| `KeyerSpeed` | `KeyerSpeed` | Keyer speed in words per minute. |
| `RfPower` | `RfPower` | Requested transmit power setting, in the rig's units (watts on an Elecraft KX3). |
| `Attenuator` | `Attenuator` | Receive attenuator, in decibels of attenuation; 0 is off. The values a model accepts are the `levels.attenuator_db` list of its description. |
| `Preamp` | `Preamp` | Receive preamplifier as a step number: 0 is off (a Yaesu "IPO"), 1 and 2 are the rig's first and second stage. The gain of a stage differs by band and model and is not stated here; a model accepts steps up to `levels.preamp_steps` of its description. |
| `Dither` | `Dither` | ADC dither of a direct-sampling receiver (the Perseus): 0 off, 1 on. Only a model whose description says so has it. |
| `Preselector` | `Preselector` | Front-end preselector of a receiver that has one (the Perseus): 0 is the wide-band path, 1 selects the band filter for the tuned frequency automatically. |
| `Antenna` | `Antenna` | The rig's own antenna connector in use, numbered from 1 (ANT1, ANT2...); a model accepts 1 up to `levels.antenna_ports` of its description. Changing it moves the RF path, so the actor refuses the change unless the rig is known not to be keyed. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `level_range` (src/description.rs:656), `lacks` (src/description.rs:727), `level_range` (src/description.rs:823), `level` (src/transcript.rs:188), `from_name` (src/units.rs:254), `level` (src/rig/easy.rs:25), `set_level` (src/rig/easy.rs:43).

**See also:** test `level_names_round_trip (src/units.rs)`.

### `Meter` [@ANCHOR: radioctl:radioctl_core_units_meter]

**Purpose and location.** A transmitter or power-supply meter read in the rig's own raw scale. Unlike [`crate::op::Observation::Swr`] (tenths of a ratio) these are not calibrated: Icom radios report a 0 to 255 meter deflection and the mapping to a ratio, a fraction, volts or amperes is a per-model measurement table that no source radioctl may copy provides. A reading is meaningful only while the rig transmits (SWR, ALC, COMP, Id) or at any time (Vd); the read itself never transmits. Defined in `daemons/radioctl/crates/radioctl-core/src/units.rs:196` as a pub `enum`. It derives Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash.

| Variant | Definition | Meaning |
|---|---|---|
| `Swr` | `Swr` | SWR meter deflection. |
| `Alc` | `Alc` | ALC (automatic level control) meter deflection. |
| `Comp` | `Comp` | Speech compressor meter deflection. |
| `Vd` | `Vd` | Supply (drain) voltage meter deflection. |
| `Id` | `Id` | Final-stage drain current meter deflection. |

**Invariants, ownership and concurrency.** Plain data: no locks, atomics or handles inside, so it is copied or moved by value as its derives allow. No invariant beyond the field types is stated in the source (unknown whether callers must keep any relation between fields).

**Serialized form.** No serde derive; it is an in-memory type, and any byte layout is written by hand in the functions that use it.

**Functions that touch it.** `meter` (src/transcript.rs:192), `from_name` (src/units.rs:233), `meter` (src/rig/easy.rs:49).

**See also:** test `meter_names_round_trip (src/units.rs)`.
