// SPDX-License-Identifier: AGPL-3.0-or-later
//! Rig descriptions: the per-model data that drives both a protocol engine and the virtual rig
//! that stands in for the model in tests.
//!
//! This is the in-memory form. The TOML file format, with provenance and `verified` fields, is
//! [`crate::rigfile`], which deserialises into these structs (the shipped files are in `rigs/`).
//! The two built-in descriptions below come from `tools/rig_simulator.py` and the relay's CI-V
//! and TS-2000 bridges (our own code); they are **not** verified against any radio.

use std::time::Duration;

use crate::capability::{Capability, CapabilitySet};
use crate::units::{Hz, Level, Mode};

/// A protocol family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// Icom CI-V, binary frames.
    IcomCiv,
    /// Kenwood-style ASCII commands ended by `;`.
    KenwoodAscii,
    /// Elecraft K3/KX3/KX2 ASCII commands ended by `;` (Kenwood-like, with its own replies).
    ElecraftAscii,
    /// Yaesu "newcat" ASCII commands ended by `;` (FT-991A, FTDX10, FTDX101).
    YaesuNewcat,
    /// Yaesu legacy five-byte binary CAT blocks (FT-817, FT-857, FT-897).
    YaesuLegacy,
    /// FlexRadio SmartSDR API: line text over TCP (FLEX-6000 and later).
    Flex,
    /// OpenHPSDR (ANAN, Hermes) over UDP, control plane only (Protocol 1 or 2).
    Hpsdr,
    /// Kenwood handheld and mobile ASCII commands ended by a carriage return (TH-D7, TM-D710, ...).
    KenwoodHh,
    /// Vendor serial protocols of the smaller makers (Ten-Tec, Drake, Racal, ...): one engine
    /// per protocol, chosen by [`VendorParams::protocol`].
    SerialVendor,
    /// Receivers, scanners and marine radios with a line or frame protocol of their own
    /// (AOR, Uniden, JRC, Rohde and Schwarz, Icom PCR and marine); see [`ReceiverProtocol`].
    Receiver,
    /// USB radios and SDR boards that speak vendor control requests or HID reports (Si570 kits,
    /// FUNcube, FiFi-SDR, ...). Described, but without an engine: radioctl has no USB control or
    /// HID transport yet (`night_shift_todo/medium/radioctl-usb-hid-transport.md`).
    UsbDevice,
}

/// The protocol of a [`Family::SerialVendor`] rig; each has its own engine file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VendorProtocol {
    /// Ten-Tec (`tentec.rs`): Orion/Eagle ASCII, Argonaut/Jupiter/Omni VII binary.
    Tentec,
    /// Drake R-8A and R-8B receivers (`drake.rs`).
    Drake,
    /// Alinco DX-77 and DX-SR8 (`alinco.rs`).
    Alinco,
    /// Racal RA6790/GM, RA3702 and Ten-Tec RX-331 and RX-340 receivers (`racal.rs`).
    Racal,
    /// CommRadio CTX-10: CRC-framed binary on a 3 Mbaud line (`commradio.rs`).
    Commradio,
    /// ADAT ADT-200A: `$`-prefixed ASCII commands (`adat.rs`).
    Adat,
    /// GomSpace GS100 ground-station transceiver: a shell with a prompt (`gomspace.rs`).
    Gomspace,
    /// HobbyPCB RS-HFIQ: `*`-prefixed ASCII commands (`rshfiq.rs`).
    Rshfiq,
    /// RFT EKD-500 receiver: a set-only ASCII frequency command (`rft.rs`).
    Rft,
    /// Philips PRM8060 / PRM80 family: a prompt-driven console (`prm80.rs`).
    Prm80,
    /// Skanti TRP8000 and TRP 8255 S R (`skanti.rs`).
    Skanti,
    /// Barrett 950, 2050 and 4050 (`barrett.rs`).
    Barrett,
    /// CODAN Envoy and NGT (`codan.rs`).
    Codan,
    /// Kachina 505DSP (`kachina.rs`).
    Kachina,
    /// Lowe HF-235 (`lowe.rs`).
    Lowe,
    /// Guohe PMR-171 (`guohe.rs`).
    Guohe,
    /// Dorji DRA818U and DRA818V modules (`dorji.rs`).
    Dorji,
    /// MDS 4710 and 9710 data radios (`mds.rs`).
    Mds,
    /// Elecraft XG3 signal source (`xg3.rs`).
    Xg3,
    /// SigFox Transfox transverter controller (`transfox.rs`).
    Transfox,
    /// Ten-Tec TT-585 Paragon (`paragon.rs`).
    Paragon,
    /// Motorola MICOM 2 and 3 (`micom.rs`).
    Micom,
    /// N2ADR HiQSDR UDP control frame (`hiqsdr.rs`).
    Hiqsdr,
    /// Watkins-Johnson WJ-8888 (`wj.rs`).
    Wj,
}

impl VendorProtocol {
    // [@ANCHOR: radioctl_core_vendorprotocol_from_name]
    /// The protocol with a file-format name.
    pub fn from_name(name: &str) -> Option<VendorProtocol> {
        match name {
            "tentec" => Some(VendorProtocol::Tentec),
            "drake" => Some(VendorProtocol::Drake),
            "alinco" => Some(VendorProtocol::Alinco),
            "racal" => Some(VendorProtocol::Racal),
            "commradio" => Some(VendorProtocol::Commradio),
            "adat" => Some(VendorProtocol::Adat),
            "gomspace" => Some(VendorProtocol::Gomspace),
            "rshfiq" => Some(VendorProtocol::Rshfiq),
            "rft" => Some(VendorProtocol::Rft),
            "prm80" => Some(VendorProtocol::Prm80),
            "skanti" => Some(VendorProtocol::Skanti),
            "barrett" => Some(VendorProtocol::Barrett),
            "codan" => Some(VendorProtocol::Codan),
            "kachina" => Some(VendorProtocol::Kachina),
            "lowe" => Some(VendorProtocol::Lowe),
            "guohe" => Some(VendorProtocol::Guohe),
            "dorji" => Some(VendorProtocol::Dorji),
            "mds" => Some(VendorProtocol::Mds),
            "xg3" => Some(VendorProtocol::Xg3),
            "transfox" => Some(VendorProtocol::Transfox),
            "paragon" => Some(VendorProtocol::Paragon),
            "micom" => Some(VendorProtocol::Micom),
            "hiqsdr" => Some(VendorProtocol::Hiqsdr),
            "wj" => Some(VendorProtocol::Wj),
            _ => None,
        }
    }

    // [@ANCHOR: radioctl_core_vendorprotocol_name]
    /// The file-format name.
    pub fn name(self) -> &'static str {
        match self {
            VendorProtocol::Tentec => "tentec",
            VendorProtocol::Drake => "drake",
            VendorProtocol::Alinco => "alinco",
            VendorProtocol::Racal => "racal",
            VendorProtocol::Commradio => "commradio",
            VendorProtocol::Adat => "adat",
            VendorProtocol::Gomspace => "gomspace",
            VendorProtocol::Rshfiq => "rshfiq",
            VendorProtocol::Rft => "rft",
            VendorProtocol::Prm80 => "prm80",
            VendorProtocol::Skanti => "skanti",
            VendorProtocol::Barrett => "barrett",
            VendorProtocol::Codan => "codan",
            VendorProtocol::Kachina => "kachina",
            VendorProtocol::Lowe => "lowe",
            VendorProtocol::Guohe => "guohe",
            VendorProtocol::Dorji => "dorji",
            VendorProtocol::Mds => "mds",
            VendorProtocol::Xg3 => "xg3",
            VendorProtocol::Transfox => "transfox",
            VendorProtocol::Paragon => "paragon",
            VendorProtocol::Micom => "micom",
            VendorProtocol::Hiqsdr => "hiqsdr",
            VendorProtocol::Wj => "wj",
        }
    }
}

/// Parameters of a [`Family::SerialVendor`] rig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorParams {
    /// The protocol (engine file).
    pub protocol: VendorProtocol,
    /// The model variant inside the protocol (`orion`, `argonaut`, ...); the engine validates it.
    pub dialect: String,
    /// Data bits on the serial line (8 unless the description says otherwise).
    pub data_bits: u8,
    /// Parity: `N`, `E` or `O`.
    pub parity: char,
    /// Stop bits.
    pub stop_bits: u8,
    /// Flow control: `none`, `xonxoff` or `rtscts`.
    pub flow: String,
}

/// The USB protocol of a [`Family::UsbDevice`] rig.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UsbProtocol {
    /// Si570 synthesizer boards on the V-USB shared id (SoftRock AVR-USB, KTH PIC-USB, Peaberry, FA-SDR):
    /// vendor control requests with a frequency word in 11.21 fixed point.
    Si570,
    /// FiFi-SDR: the Si570 requests plus its own read and write requests.
    Fifisdr,
    /// AMSAT-UK FUNcube Dongle and Pro+: 64-byte HID reports.
    Funcube,
    /// Elektor SDR-USB: an FTDI chip in bit-bang mode.
    Elektor507,
    /// Digital World Traveller DRM receiver.
    Dwt,
}

impl UsbProtocol {
    const ALL: [UsbProtocol; 5] = [
        UsbProtocol::Si570,
        UsbProtocol::Fifisdr,
        UsbProtocol::Funcube,
        UsbProtocol::Elektor507,
        UsbProtocol::Dwt,
    ];

    // [@ANCHOR: radioctl_core_usbprotocol_name]
    /// The name used in rig description files.
    pub fn name(self) -> &'static str {
        match self {
            UsbProtocol::Si570 => "si570",
            UsbProtocol::Fifisdr => "fifisdr",
            UsbProtocol::Funcube => "funcube",
            UsbProtocol::Elektor507 => "elektor507",
            UsbProtocol::Dwt => "dwt",
        }
    }

    // [@ANCHOR: radioctl_core_usbprotocol_from_name]
    /// The protocol with a file-format name.
    pub fn from_name(name: &str) -> Option<UsbProtocol> {
        UsbProtocol::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// Parameters of a [`Family::UsbDevice`] rig.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbParams {
    /// The device's protocol.
    pub protocol: UsbProtocol,
    /// USB vendor id.
    pub vendor_id: u16,
    /// USB product id.
    pub product_id: u16,
    /// The manufacturer string the device reports (V-USB boards share one id and are told apart by it); empty when none is used.
    pub manufacturer: String,
    /// The product string the device reports; empty when none is used.
    pub product: String,
    /// The Si570 frequency multiplier of the board (2 or 4), 0 for a device that has none.
    pub multiplier: u8,
}

/// Which protocol engine a [`Family::Receiver`] description uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverProtocol {
    /// AOR ASCII commands (AR8000, AR8200, AR8600, AR5000, AR3000A, AR3030, AR2700, SR2200).
    Aor,
    /// Uniden and Bearcat scanners (BC245, BC250, BC780, BC895, BC898, BCD-396T, BCD-996T).
    Uniden,
    /// JRC receivers and transceivers (NRD-525, NRD-535, NRD-545, JST-145, JST-245).
    Jrc,
    /// Rohde and Schwarz (EB200, ESMC, EK895/6, XK852, XK2100).
    RohdeSchwarz,
    /// Icom PCR computer-controlled receivers (PCR100, PCR1000, PCR1500, PCR2500).
    IcomPcr,
    /// Icom marine transceivers (IC-M700PRO, IC-M710, IC-M802, IC-M803).
    IcomMarine,
}

impl ReceiverProtocol {
    // [@ANCHOR: radioctl_core_receiver_protocol_name]
    /// The name used in rig description files.
    pub fn name(self) -> &'static str {
        match self {
            ReceiverProtocol::Aor => "aor",
            ReceiverProtocol::Uniden => "uniden",
            ReceiverProtocol::Jrc => "jrc",
            ReceiverProtocol::RohdeSchwarz => "rohde-schwarz",
            ReceiverProtocol::IcomPcr => "icom-pcr",
            ReceiverProtocol::IcomMarine => "icom-marine",
        }
    }

    // [@ANCHOR: radioctl_core_receiver_protocol_from_name]
    /// The protocol with a file-format name.
    pub fn from_name(name: &str) -> Option<ReceiverProtocol> {
        [
            ReceiverProtocol::Aor,
            ReceiverProtocol::Uniden,
            ReceiverProtocol::Jrc,
            ReceiverProtocol::RohdeSchwarz,
            ReceiverProtocol::IcomPcr,
            ReceiverProtocol::IcomMarine,
        ]
        .into_iter()
        .find(|p| p.name() == name)
    }
}

/// Receiver-family parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverParams {
    /// The protocol engine.
    pub protocol: ReceiverProtocol,
    /// The model's command set within the protocol (`ar8000`, `bc780`, ...); the engine's tables
    /// are keyed by it.
    pub dialect: String,
    /// The bus address (the Icom marine radios' and JRC's), when the protocol has one.
    pub address: Option<u8>,
    /// The default line speed in baud.
    pub baud: u32,
    /// Data bits on the serial line.
    pub data_bits: u8,
    /// Parity: `N`, `E` or `O`.
    pub parity: char,
    /// Stop bits.
    pub stop_bits: u8,
    /// Flow control: `none`, `xonxoff` or `rtscts`.
    pub flow: String,
}

/// CI-V parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivParams {
    /// The rig's CI-V address.
    pub address: u8,
    /// The controller's address (the software side), conventionally 0xE0.
    pub controller: u8,
    /// The byte the rig returns for the read-ID command.
    pub model_id: u8,
    /// True when the bus echoes every transmitted frame back (single-wire interface).
    pub echo: bool,
    /// Bytes of BCD in a frequency field: 5 (ten digits) for most models, 4 (eight digits, Hamlib's
    /// `civ_731_mode`: IC-735, IC-820H, IC-821H) and 6 for a model whose field is 5 bytes up to
    /// 5.85 GHz and 6 bytes above it (Hamlib's IC-905 rule). See [`CivParams::field_len`].
    pub frequency_bytes: u8,
    /// Which command set sets the frequency and the mode (see [`CivCommandSet`]).
    pub command_set: CivCommandSet,
}

/// The command numbers a CI-V model uses to set the frequency and to read and set the mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CivCommandSet {
    /// `05` sets the frequency, `04` reads the mode and `06` sets it (every model but one).
    #[default]
    Standard,
    /// The IC-F8101: `1A 35` sets the frequency, `1A 34` reads the mode and `1A 36` sets it, each
    /// with a two-byte mode field (`00`, code). The frequency is still read with `03`.
    F8101,
}

impl CivParams {
    // [@ANCHOR: radioctl_core_civparams_field_len]
    /// The number of BCD bytes the frequency field has for `hz` on this model.
    pub fn field_len(&self, hz: Hz) -> usize {
        match self.frequency_bytes {
            6 if hz.0 > 5_850_000_000 => 6,
            6 => 5,
            other => usize::from(other),
        }
    }
}

/// Which command set a Kenwood-style rig speaks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum KenwoodDialect {
    /// The TS-2000 and TS-590 command set: `IF`, `MD`, `AG0nnn`.
    #[default]
    Classic,
    /// The TS-890S command set: no `IF`, the mode through `OM`, `AGnnn`, split through `TB`,
    /// one-digit `RA` and `PA`.
    Ts890,
    /// The older Kenwood set of the TS-440S (IC-10 manual) and its relatives: `IF` as the classic
    /// set, but `MD` only sets (the mode is read from `IF`), `FN` and `SP` replace `FR` and `FT`,
    /// `RU` and `RD` take no parameter, and there is no `SM`, `AG`, `RG`, `PC` or `KS`.
    Ts440,
    /// The TS-450S and TS-690S set (their external-control manual): as `Ts440` but split is set by
    /// `FR` and `FT` (set only) and `AI` has one digit.
    Ts450,
    /// The TS-990S set (its PC command reference): classic `IF`, `FA`, `RT`, `XT`, `RC`, `RU`/`RD`,
    /// `SM0`, `AI2`, but the mode is `OM0` (read) and `OM0<m>` (set), there is no `MD`, `FR`, `FT` or
    /// transmit state in `IF`, and the sub band is a second receiver and not a VFO B.
    Ts990,
    /// The R-5000 set (Hamlib's IC-10 backend, no Kenwood document read): the TS-440S set with an
    /// `IF` answer five characters shorter (30 characters between `IF` and `;`: the first
    /// offset field is absent), split by `FN` and `SP`.
    R5000,
    /// The TRC-80 set (Hamlib's `kenwood_get_freq_if`): the classic set, but the frequency is read
    /// from `IF` and not from `FA`.
    Trc80,
}

impl KenwoodDialect {
    // [@ANCHOR: radioctl_core_kenwood_dialect_name]
    /// The name used in rig description files.
    pub fn name(self) -> &'static str {
        match self {
            KenwoodDialect::Classic => "classic",
            KenwoodDialect::Ts890 => "ts890",
            KenwoodDialect::Ts440 => "ts440",
            KenwoodDialect::Ts450 => "ts450",
            KenwoodDialect::Ts990 => "ts990",
            KenwoodDialect::R5000 => "r5000",
            KenwoodDialect::Trc80 => "trc80",
        }
    }

    // [@ANCHOR: radioctl_core_kenwood_dialect_from_name]
    /// The dialect with a file-format name.
    pub fn from_name(name: &str) -> Option<KenwoodDialect> {
        match name {
            "classic" => Some(KenwoodDialect::Classic),
            "ts890" => Some(KenwoodDialect::Ts890),
            "ts440" => Some(KenwoodDialect::Ts440),
            "ts450" => Some(KenwoodDialect::Ts450),
            "ts990" => Some(KenwoodDialect::Ts990),
            "r5000" => Some(KenwoodDialect::R5000),
            "trc80" => Some(KenwoodDialect::Trc80),
            _ => None,
        }
    }
}

/// Kenwood-style parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KenwoodParams {
    /// The number the rig returns for `ID;`.
    pub model_id: u16,
    /// The command set.
    pub dialect: KenwoodDialect,
}

/// Which command set a Kenwood handheld or mobile speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HhDialect {
    /// TH-D7, TH-F6/F7, TH-G71, TM-D700, TM-V7: `FQ`, `MD`, `BC`, `TX`/`RX`.
    Th,
    /// TM-D710 and TM-V71: the fixed-width `FO` line (47 characters, mode in the last).
    Tm710,
    /// TH-D72: the `FO` line of 52 characters, the mode in the last.
    Th72,
    /// TH-D74: the `FO` line of 72 characters, the mode at column 31, `MD v,k` to set it.
    Th74,
}

impl HhDialect {
    // [@ANCHOR: radioctl_core_hh_dialect_name]
    /// The name used in rig description files.
    pub fn name(self) -> &'static str {
        match self {
            HhDialect::Th => "th",
            HhDialect::Tm710 => "tm710",
            HhDialect::Th72 => "th72",
            HhDialect::Th74 => "th74",
        }
    }

    // [@ANCHOR: radioctl_core_hh_dialect_from_name]
    /// The dialect with a file-format name.
    pub fn from_name(name: &str) -> Option<HhDialect> {
        match name {
            "th" => Some(HhDialect::Th),
            "tm710" => Some(HhDialect::Tm710),
            "th72" => Some(HhDialect::Th72),
            "th74" => Some(HhDialect::Th74),
            _ => None,
        }
    }
}

/// Kenwood handheld and mobile parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KenwoodHhParams {
    /// The command set.
    pub dialect: HhDialect,
}

/// Elecraft-style parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElecraftParams {
    /// The number the rig returns for `ID;` (017 on the K3, KX3 and KX2).
    pub model_id: u16,
}

/// Spectrum scope (band scope) parameters of a model, for waveform data the rig sends on its own
/// (Icom CI-V command 0x27).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeParams {
    /// Independent scopes (receivers): 1, or 2 on a Main/Sub rig.
    pub receivers: u8,
    /// Amplitude samples in one complete sweep ("line").
    pub line_length: u16,
    /// Amplitude samples carried by one waveform frame.
    pub samples_per_frame: u16,
    /// Largest amplitude sample the rig sends (0 is the bottom of the display).
    pub amplitude_max: u16,
    /// The span choices in hertz (full width, as the rig's menu shows them).
    pub spans_hz: Vec<u64>,
}

/// The receive attenuator and preamplifier steps a model has. Empty and zero mean the model has
/// neither, or radioctl knows no command for them: the engines then refuse the levels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LevelParams {
    /// The attenuation in decibels of each attenuator step, ascending; step `n` (1-based) is the
    /// `n`th entry and step 0 is off. A one-entry list is an attenuator that is simply on or off.
    pub attenuator_db: Vec<u8>,
    /// For a step attenuator that takes any whole number of decibels from 0 up to this (an
    /// OpenHPSDR radio's 0 to 31 dB), the largest; 0 when `attenuator_db` lists the steps.
    pub attenuator_any_up_to_db: u8,
    /// How many preamplifier steps above off the model has (0 for none).
    pub preamp_steps: u8,
    /// Levels (of the plain `14` family: AF, RF, microphone, monitor, keyer, power) the model's
    /// command table does not have; the CI-V engine and virtual rig refuse them. Empty means the
    /// model has them all, which is what a description without `levels.absent` states.
    pub absent: Vec<Level>,
}

impl LevelParams {
    // [@ANCHOR: radioctl_core_levelparams_lacks]
    /// True when the description lists `level` as absent from the model.
    pub fn lacks(&self, level: Level) -> bool {
        self.absent.contains(&level)
    }

    // [@ANCHOR: radioctl_core_levelparams_max_attenuation]
    /// The largest attenuation in decibels, `None` when the model has no attenuator.
    pub fn max_attenuation_db(&self) -> Option<u8> {
        if self.attenuator_any_up_to_db > 0 {
            Some(self.attenuator_any_up_to_db)
        } else {
            self.attenuator_db.last().copied()
        }
    }

    // [@ANCHOR: radioctl_core_levelparams_attenuator_step]
    /// The 1-based step whose attenuation is `db`, or 0 for `db == 0` (off); `None` if no step has
    /// that attenuation.
    pub fn attenuator_step(&self, db: u16) -> Option<u8> {
        if db == 0 {
            return Some(0);
        }
        if self.attenuator_any_up_to_db > 0 {
            return u8::try_from(db)
                .ok()
                .filter(|d| *d <= self.attenuator_any_up_to_db);
        }
        let index = self
            .attenuator_db
            .iter()
            .position(|d| u16::from(*d) == db)?;
        u8::try_from(index + 1).ok()
    }

    // [@ANCHOR: radioctl_core_levelparams_attenuator_db]
    /// The attenuation in decibels of 1-based step `step` (0 is off, 0 dB); `None` for a step the
    /// model does not have.
    pub fn attenuator_db_of(&self, step: u8) -> Option<u16> {
        if step == 0 {
            return Some(0);
        }
        if self.attenuator_any_up_to_db > 0 {
            return (step <= self.attenuator_any_up_to_db).then_some(u16::from(step));
        }
        usize::from(step)
            .checked_sub(1)
            .and_then(|index| self.attenuator_db.get(index))
            .map(|d| u16::from(*d))
    }
}

/// Yaesu newcat parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YaesuParams {
    /// The number the rig returns for `ID;` (`ID0670;` is 670).
    pub model_id: u16,
    /// Digits of the `FA`/`FB` frequency and of the `IF` VFO-A frequency: 9 on the FT-991A,
    /// FT-891, FT-710, FTDX10 and FTDX101, 8 on the FT-450, FT-950, FT-2000, FTDX1200,
    /// FTDX3000, FTDX5000 and FTDX9000.
    pub frequency_digits: u8,
}

/// Yaesu legacy five-byte CAT parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YaesuLegacyParams {
    /// Stop bits on the CAT serial line (2 on the FT-817, FT-857 and FT-897).
    pub stop_bits: u8,
    /// True when a frequency from 1.2 to 1.3 GHz is sent as its seven low digits with the nibble C ahead
    /// (the FT-736R's 1.2 GHz module, as Hamlib's `ft736_set_freq` does); false for every other model.
    pub high_band_flag: bool,
    /// The block-protocol commands and status reads of the later models (FT-747 to FT-1000MP and
    /// the receivers), or `None` for the FT-817 family, whose commands the engine knows itself.
    pub block: Option<LegacyBlock>,
}

/// How a frequency is written into five-byte-block bytes (a command's parameters or a field of a
/// status record).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyFreqForm {
    /// Eight BCD digits in four bytes, most significant first, 10 Hz units (FT-817 family,
    /// FT-767GX status, FRG-9600 set).
    Bcd10HzMsb,
    /// Eight BCD digits in four bytes, least significant byte first, 10 Hz units (the set-frequency
    /// parameters of FT-747 to FT-1000MP and the FT-757GXII status).
    Bcd10HzLsb,
    /// Eight BCD digits in four bytes, most significant first, 1 Hz units (FT-747 status; the radio
    /// resolves 25 Hz).
    Bcd1HzMsb,
    /// Binary, three bytes, most significant first, 10 Hz units (FT-840, 890, 900, 990, 1000).
    Bin10HzMsb3,
    /// Binary, three bytes, least significant first, 10 Hz units (FRG-100).
    Bin10HzLsb3,
    /// Binary, four bytes, most significant first, 10 Hz units (VR-5000 set).
    Bin10HzMsb4,
    /// Binary, four bytes, most significant first, 1 Hz units (FT-920 status, as Hamlib reads it).
    Bin1HzMsb4,
    /// Binary, four bytes, most significant first, 1.25 Hz units (FT-100 status).
    Bin125Msb4,
    /// Binary, four bytes, most significant first, 0.625 Hz units (FT-1000MP status, as Hamlib reads it).
    Bin0625Msb4,
    /// The FRG-8800 form: BCD digits least significant first from the 100 Hz digit, the low nibble
    /// of the first byte a one-hot code of the 25 Hz step. Writes only.
    Frg8800,
}

impl LegacyFreqForm {
    // [@ANCHOR: radioctl_core_legacy_form_width]
    /// Bytes a frequency in this form occupies.
    pub fn width(self) -> usize {
        match self {
            LegacyFreqForm::Bin10HzMsb3 | LegacyFreqForm::Bin10HzLsb3 => 3,
            _ => 4,
        }
    }

    // [@ANCHOR: radioctl_core_legacy_form_unit]
    /// Hertz per count for the binary and BCD forms, in thousandths of a hertz.
    fn unit_millihz(self) -> u64 {
        match self {
            LegacyFreqForm::Bcd1HzMsb | LegacyFreqForm::Bin1HzMsb4 => 1_000,
            LegacyFreqForm::Bin125Msb4 => 1_250,
            LegacyFreqForm::Bin0625Msb4 => 625,
            _ => 10_000,
        }
    }

    fn is_bcd(self) -> bool {
        matches!(
            self,
            LegacyFreqForm::Bcd10HzMsb | LegacyFreqForm::Bcd10HzLsb | LegacyFreqForm::Bcd1HzMsb
        )
    }

    fn lsb_first(self) -> bool {
        matches!(
            self,
            LegacyFreqForm::Bcd10HzLsb | LegacyFreqForm::Bin10HzLsb3 | LegacyFreqForm::Frg8800
        )
    }

    // [@ANCHOR: radioctl_core_legacy_form_encode]
    /// The bytes for a frequency, or `None` when it is not a whole count of this form's unit or
    /// does not fit the width.
    pub fn encode(self, hz: Hz) -> Option<Vec<u8>> {
        if self == LegacyFreqForm::Frg8800 {
            // The BCD digits of the 10 Hz count, least significant byte first; the first byte keeps
            // the 100 Hz digit in its high nibble and a one-hot code of the 25 Hz step in the low.
            if !hz.0.is_multiple_of(25) || hz.0 > 999_999_975 {
                return None;
            }
            let mut rest = hz.0 / 10;
            let mut out = vec![0u8; 4];
            for byte in out.iter_mut() {
                let pair = (rest % 100) as u8;
                *byte = ((pair / 10) << 4) | (pair % 10);
                rest /= 100;
            }
            out[0] = (out[0] & 0xF0) | (1 << ((hz.0 % 100) / 25));
            return Some(out);
        }
        let millihz = hz.0.checked_mul(1_000)?;
        let unit = self.unit_millihz();
        if !millihz.is_multiple_of(unit) {
            return None;
        }
        let count = millihz / unit;
        let width = self.width();
        let mut bytes = vec![0u8; width];
        if self.is_bcd() {
            let mut rest = count;
            for byte in bytes.iter_mut().rev() {
                let pair = (rest % 100) as u8;
                *byte = ((pair / 10) << 4) | (pair % 10);
                rest /= 100;
            }
            if rest != 0 {
                return None;
            }
        } else {
            if count >> (8 * width) != 0 {
                return None;
            }
            for (i, byte) in bytes.iter_mut().rev().enumerate() {
                *byte = (count >> (8 * i)) as u8;
            }
        }
        if self.lsb_first() {
            bytes.reverse();
        }
        Some(bytes)
    }

    // [@ANCHOR: radioctl_core_legacy_form_decode]
    /// The frequency in hertz from the bytes (as many as [`LegacyFreqForm::width`]), or `None` on a
    /// non-digit BCD nibble, a bad step code or a wrong-length slice.
    pub fn decode(self, bytes: &[u8]) -> Option<Hz> {
        if bytes.len() != self.width() {
            return None;
        }
        if self == LegacyFreqForm::Frg8800 {
            let pair = |b: u8| {
                (b >> 4 <= 9 && b & 15 <= 9).then(|| u64::from(b >> 4) * 10 + u64::from(b & 15))
            };
            let step = match bytes[0] & 15 {
                1 => 0,
                2 => 25,
                4 => 50,
                8 => 75,
                _ => return None,
            };
            return Some(Hz(u64::from(bytes[0] >> 4) * 100
                + step
                + pair(bytes[1])? * 1_000
                + pair(bytes[2])? * 100_000
                + pair(bytes[3])? * 10_000_000));
        }
        let mut ordered = bytes.to_vec();
        if self.lsb_first() {
            ordered.reverse();
        }
        let mut count: u64 = 0;
        for byte in &ordered {
            if self.is_bcd() {
                let (high, low) = (byte >> 4, byte & 15);
                if high > 9 || low > 9 {
                    return None;
                }
                count = count * 100 + u64::from(high * 10 + low);
            } else {
                count = count * 256 + u64::from(*byte);
            }
        }
        // Whole hertz only: the 1.25 Hz and 0.625 Hz forms round down.
        Some(Hz(count * self.unit_millihz() / 1_000))
    }
}

/// A five-byte command with a slot for a parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyCommand {
    /// The block as sent, the slot bytes zero (or whatever a fixed byte there should be).
    pub template: [u8; 5],
    /// Index in the block of the first slot byte (the mode code, or the first frequency byte).
    pub at: u8,
    /// How a frequency fills the slot, for the set-frequency command.
    pub form: Option<LegacyFreqForm>,
}

/// A flag in a status record: a byte offset, a mask, and whether the feature is on when a bit of the mask is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyFlag {
    /// Byte offset in the reply.
    pub offset: u16,
    /// Bits of the byte that matter.
    pub mask: u8,
    /// True when the feature is on if any masked bit is set; false when it is on if they are all clear.
    pub on_when_set: bool,
}

/// One status read: the request, the reply length and where the fields are in the reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyRead {
    /// The five request bytes.
    pub request: [u8; 5],
    /// Reply length in bytes.
    pub length: u16,
    /// Offset and form of the frequency, if the reply has it.
    pub frequency: Option<(u16, LegacyFreqForm)>,
    /// Offset and mask of the mode byte, if the reply has it.
    pub mode: Option<(u16, u8)>,
    /// PTT flag.
    pub ptt: Option<LegacyFlag>,
    /// Split flag.
    pub split: Option<LegacyFlag>,
}

/// The commands and status reads of a block-protocol Yaesu legacy model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyBlock {
    /// Set the frequency.
    pub set_frequency: Option<LegacyCommand>,
    /// Set the mode; the slot is one byte of the description's mode code.
    pub set_mode: Option<LegacyCommand>,
    /// PTT on and PTT off blocks.
    pub ptt: Option<([u8; 5], [u8; 5])>,
    /// Split on and split off blocks.
    pub split: Option<([u8; 5], [u8; 5])>,
    /// Status reads.
    pub reads: Vec<LegacyRead>,
    /// Mode codes as the status reads return them (they differ from the set codes on most models).
    pub status_modes: Vec<(u8, Mode)>,
    /// True when the radio echoes each command block (FT-767GX) and the engine expects the echo.
    pub echo: bool,
}

/// FlexRadio SmartSDR parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlexParams {
    /// The model name the radio reports in `info` and in discovery (`FLEX-6300`).
    pub model: String,
    /// Spectrum capture units (receiver front ends): 1, or 2 on the FLEX-6700.
    pub scus: u8,
    /// The slice receiver (0 is slice A) the description's VFO A is; the SmartSDR slice A to H
    /// models of Hamlib are slices 0 to 7.
    pub slice: u8,
}

/// OpenHPSDR network radio parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HpsdrParams {
    /// Model name (`ANAN-100`).
    pub model: String,
    /// Wire protocol, 1 or 2.
    pub protocol: u8,
    /// True for the ANAN-10 power calibration constant, false for the ANAN-100.
    pub anan10: bool,
}

/// Command timing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// How long to wait for a reply.
    pub timeout: Duration,
    /// How many times the actor repeats a command that timed out.
    pub retries: u8,
    /// Minimum gap between commands.
    pub inter_command: Duration,
}

/// Everything radioctl knows about one model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RigDescription {
    /// A stable identifier, e.g. `icom-ic-7300`.
    pub id: String,
    /// A display name.
    pub name: String,
    /// The protocol family.
    pub family: Family,
    /// CI-V parameters, present for [`Family::IcomCiv`].
    pub civ: Option<CivParams>,
    /// Kenwood parameters, present for [`Family::KenwoodAscii`].
    pub kenwood: Option<KenwoodParams>,
    /// Elecraft parameters, present for [`Family::ElecraftAscii`].
    pub elecraft: Option<ElecraftParams>,
    /// Yaesu newcat parameters, present for [`Family::YaesuNewcat`].
    pub yaesu: Option<YaesuParams>,
    /// Yaesu legacy parameters, present for [`Family::YaesuLegacy`].
    pub yaesu_legacy: Option<YaesuLegacyParams>,
    /// Kenwood handheld and mobile parameters, present for [`Family::KenwoodHh`].
    pub kenwood_hh: Option<KenwoodHhParams>,
    /// FlexRadio parameters, present for [`Family::Flex`].
    pub flex: Option<FlexParams>,
    /// OpenHPSDR parameters, present for [`Family::Hpsdr`].
    pub hpsdr: Option<HpsdrParams>,
    /// Vendor serial parameters, present for [`Family::SerialVendor`].
    pub vendor: Option<VendorParams>,
    /// Receiver parameters, present for [`Family::Receiver`].
    pub receiver: Option<ReceiverParams>,
    /// USB device parameters, present for [`Family::UsbDevice`].
    pub usb: Option<UsbParams>,
    /// Spectrum scope parameters, for the models that have a scope over CAT.
    pub scope: Option<ScopeParams>,
    /// Attenuator and preamplifier steps.
    pub levels: LevelParams,
    /// What the rig can do.
    pub capabilities: CapabilitySet,
    /// Mode to wire code: a CI-V mode byte, or the ASCII byte of a Kenwood `MD` digit.
    pub mode_codes: Vec<(Mode, u8)>,
    /// Command timing.
    pub timing: Timing,
    /// Frequency a virtual rig of this model starts at.
    pub default_frequency: Hz,
    /// Mode a virtual rig of this model starts in.
    pub default_mode: Mode,
}

impl RigDescription {
    // [@ANCHOR: radioctl_core_description_mode_code]
    /// The wire code for a mode, if the model has one.
    pub fn mode_code(&self, mode: Mode) -> Option<u8> {
        self.mode_codes
            .iter()
            .find(|(m, _)| *m == mode)
            .map(|(_, code)| *code)
    }

    // [@ANCHOR: radioctl_core_description_code_mode]
    /// The mode for a wire code, if the model has one.
    pub fn code_mode(&self, code: u8) -> Option<Mode> {
        self.mode_codes
            .iter()
            .find(|(_, c)| *c == code)
            .map(|(mode, _)| *mode)
    }

    // [@ANCHOR: radioctl_core_description_ic7300]
    /// An Icom IC-7300 style CI-V description (address 0x94). Unverified; see the module note.
    pub fn ic7300() -> RigDescription {
        RigDescription {
            id: "icom-ic-7300".into(),
            name: "Icom IC-7300".into(),
            family: Family::IcomCiv,
            civ: Some(CivParams {
                address: 0x94,
                controller: 0xE0,
                model_id: 0x94,
                echo: false,
                frequency_bytes: 5,
                command_set: CivCommandSet::Standard,
            }),
            kenwood: None,
            elecraft: None,
            yaesu: None,
            yaesu_legacy: None,
            kenwood_hh: None,
            flex: None,
            hpsdr: None,
            vendor: None,
            usb: None,
            receiver: None,
            scope: None,
            levels: LevelParams::default(),
            capabilities: CapabilitySet::of(&[
                Capability::ReadFrequency,
                Capability::SetFrequency,
                Capability::ReadMode,
                Capability::SetMode,
                Capability::ReadPtt,
                Capability::SetPtt,
                Capability::ReadSignal,
                Capability::ReadPower,
                Capability::SendCw,
                Capability::Transceive,
            ]),
            mode_codes: vec![
                (Mode::Lsb, 0x00),
                (Mode::Usb, 0x01),
                (Mode::Am, 0x02),
                (Mode::Cw, 0x03),
                (Mode::Rtty, 0x04),
                (Mode::Fm, 0x05),
                (Mode::Wfm, 0x06),
                (Mode::CwR, 0x07),
                (Mode::RttyR, 0x08),
            ],
            timing: Timing {
                timeout: Duration::from_millis(1000),
                retries: 3,
                inter_command: Duration::ZERO,
            },
            default_frequency: Hz(14_074_000),
            default_mode: Mode::Usb,
        }
    }

    // [@ANCHOR: radioctl_core_description_ts2000]
    /// A Kenwood TS-2000 style ASCII description. Unverified; see the module note.
    pub fn ts2000() -> RigDescription {
        RigDescription {
            id: "kenwood-ts-2000".into(),
            name: "Kenwood TS-2000".into(),
            family: Family::KenwoodAscii,
            civ: None,
            kenwood: Some(KenwoodParams {
                model_id: 19,
                dialect: KenwoodDialect::Classic,
            }),
            elecraft: None,
            yaesu: None,
            yaesu_legacy: None,
            kenwood_hh: None,
            flex: None,
            hpsdr: None,
            vendor: None,
            usb: None,
            receiver: None,
            scope: None,
            levels: LevelParams::default(),
            capabilities: CapabilitySet::of(&[
                Capability::ReadFrequency,
                Capability::SetFrequency,
                Capability::SecondVfo,
                Capability::ReadMode,
                Capability::SetMode,
                Capability::SetPtt,
                Capability::ReadSignal,
                Capability::Transceive,
            ]),
            mode_codes: vec![
                (Mode::Lsb, b'1'),
                (Mode::Usb, b'2'),
                (Mode::Cw, b'3'),
                (Mode::Fm, b'4'),
                (Mode::Am, b'5'),
                (Mode::Rtty, b'6'),
                (Mode::CwR, b'7'),
                (Mode::RttyR, b'9'),
            ],
            timing: Timing {
                timeout: Duration::from_millis(200),
                retries: 3,
                inter_command: Duration::ZERO,
            },
            default_frequency: Hz(14_074_000),
            default_mode: Mode::Usb,
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests [@ANCHOR: radioctl_core_hh_dialect_name]
    // Tests [@ANCHOR: radioctl_core_hh_dialect_from_name]
    #[test]
    fn handheld_dialect_names_round_trip() {
        for d in [
            HhDialect::Th,
            HhDialect::Tm710,
            HhDialect::Th72,
            HhDialect::Th74,
        ] {
            assert_eq!(HhDialect::from_name(d.name()), Some(d));
        }
        assert_eq!(HhDialect::from_name("nope"), None);
    }

    use super::*;

    // Tests [@ANCHOR: radioctl_core_description_mode_code]
    // Tests [@ANCHOR: radioctl_core_description_code_mode]
    #[test]
    fn mode_tables_round_trip() {
        for desc in [RigDescription::ic7300(), RigDescription::ts2000()] {
            for (mode, code) in desc.mode_codes.clone() {
                assert_eq!(desc.mode_code(mode), Some(code));
                assert_eq!(desc.code_mode(code), Some(mode));
            }
        }
        assert_eq!(RigDescription::ic7300().mode_code(Mode::Usb), Some(0x01));
        assert_eq!(RigDescription::ts2000().mode_code(Mode::Usb), Some(b'2'));
        assert_eq!(RigDescription::ts2000().code_mode(b'8'), None);
    }

    // Tests [@ANCHOR: radioctl_core_usbprotocol_name]
    // Tests [@ANCHOR: radioctl_core_usbprotocol_from_name]
    #[test]
    fn usb_protocol_names_round_trip() {
        for p in UsbProtocol::ALL {
            assert_eq!(UsbProtocol::from_name(p.name()), Some(p));
        }
        assert_eq!(UsbProtocol::from_name("nope"), None);
    }

    // Tests [@ANCHOR: radioctl_core_vendorprotocol_from_name]
    // Tests [@ANCHOR: radioctl_core_vendorprotocol_name]
    #[test]
    fn vendor_protocol_names_round_trip() {
        assert_eq!(
            VendorProtocol::from_name("tentec"),
            Some(VendorProtocol::Tentec)
        );
        assert_eq!(VendorProtocol::Tentec.name(), "tentec");
        assert_eq!(
            VendorProtocol::from_name("drake"),
            Some(VendorProtocol::Drake)
        );
        assert_eq!(VendorProtocol::Drake.name(), "drake");
        assert_eq!(
            VendorProtocol::from_name("alinco"),
            Some(VendorProtocol::Alinco)
        );
        assert_eq!(VendorProtocol::Alinco.name(), "alinco");
        assert_eq!(
            VendorProtocol::from_name("racal"),
            Some(VendorProtocol::Racal)
        );
        assert_eq!(VendorProtocol::Racal.name(), "racal");
        for p in [
            VendorProtocol::Commradio,
            VendorProtocol::Adat,
            VendorProtocol::Gomspace,
            VendorProtocol::Rshfiq,
            VendorProtocol::Rft,
            VendorProtocol::Prm80,
        ] {
            assert_eq!(VendorProtocol::from_name(p.name()), Some(p));
        }
        assert_eq!(
            VendorProtocol::from_name("skanti"),
            Some(VendorProtocol::Skanti)
        );
        assert_eq!(VendorProtocol::Skanti.name(), "skanti");
        assert_eq!(
            VendorProtocol::from_name("barrett"),
            Some(VendorProtocol::Barrett)
        );
        assert_eq!(VendorProtocol::Barrett.name(), "barrett");
        assert_eq!(
            VendorProtocol::from_name("codan"),
            Some(VendorProtocol::Codan)
        );
        assert_eq!(VendorProtocol::Codan.name(), "codan");
        assert_eq!(
            VendorProtocol::from_name("kachina"),
            Some(VendorProtocol::Kachina)
        );
        assert_eq!(VendorProtocol::Kachina.name(), "kachina");
        assert_eq!(
            VendorProtocol::from_name("lowe"),
            Some(VendorProtocol::Lowe)
        );
        assert_eq!(VendorProtocol::Lowe.name(), "lowe");
        assert_eq!(
            VendorProtocol::from_name("guohe"),
            Some(VendorProtocol::Guohe)
        );
        assert_eq!(VendorProtocol::Guohe.name(), "guohe");
        assert_eq!(
            VendorProtocol::from_name("dorji"),
            Some(VendorProtocol::Dorji)
        );
        assert_eq!(VendorProtocol::Dorji.name(), "dorji");
        assert_eq!(VendorProtocol::from_name("mds"), Some(VendorProtocol::Mds));
        assert_eq!(VendorProtocol::Mds.name(), "mds");
        assert_eq!(VendorProtocol::from_name("xg3"), Some(VendorProtocol::Xg3));
        assert_eq!(VendorProtocol::Xg3.name(), "xg3");
        assert_eq!(
            VendorProtocol::from_name("transfox"),
            Some(VendorProtocol::Transfox)
        );
        assert_eq!(VendorProtocol::Transfox.name(), "transfox");
        assert_eq!(
            VendorProtocol::from_name("paragon"),
            Some(VendorProtocol::Paragon)
        );
        assert_eq!(VendorProtocol::Paragon.name(), "paragon");
        assert_eq!(
            VendorProtocol::from_name("micom"),
            Some(VendorProtocol::Micom)
        );
        assert_eq!(VendorProtocol::Micom.name(), "micom");
        assert_eq!(
            VendorProtocol::from_name("hiqsdr"),
            Some(VendorProtocol::Hiqsdr)
        );
        assert_eq!(VendorProtocol::Hiqsdr.name(), "hiqsdr");
        assert_eq!(VendorProtocol::from_name("wj"), Some(VendorProtocol::Wj));
        assert_eq!(VendorProtocol::Wj.name(), "wj");
        assert_eq!(VendorProtocol::from_name("nope"), None);
    }

    // Tests [@ANCHOR: radioctl_core_kenwood_dialect_name]
    // Tests [@ANCHOR: radioctl_core_kenwood_dialect_from_name]
    #[test]
    fn kenwood_dialect_names_round_trip() {
        for d in [
            KenwoodDialect::Classic,
            KenwoodDialect::Ts890,
            KenwoodDialect::Ts440,
            KenwoodDialect::Ts450,
            KenwoodDialect::Ts990,
            KenwoodDialect::R5000,
            KenwoodDialect::Trc80,
        ] {
            assert_eq!(KenwoodDialect::from_name(d.name()), Some(d));
        }
        assert_eq!(KenwoodDialect::from_name("ts2000"), None);
        assert_eq!(KenwoodDialect::default(), KenwoodDialect::Classic);
    }

    // Tests [@ANCHOR: radioctl_core_receiver_protocol_name]
    // Tests [@ANCHOR: radioctl_core_receiver_protocol_from_name]
    #[test]
    fn receiver_protocol_names_round_trip() {
        for p in [
            ReceiverProtocol::Aor,
            ReceiverProtocol::Uniden,
            ReceiverProtocol::Jrc,
            ReceiverProtocol::RohdeSchwarz,
            ReceiverProtocol::IcomPcr,
            ReceiverProtocol::IcomMarine,
        ] {
            assert_eq!(ReceiverProtocol::from_name(p.name()), Some(p));
        }
        assert_eq!(ReceiverProtocol::from_name("winradio"), None);
    }

    // Tests [@ANCHOR: radioctl_core_description_ic7300]
    #[test]
    fn ic7300_is_civ_at_0x94() {
        let desc = RigDescription::ic7300();
        assert_eq!(desc.family, Family::IcomCiv);
        assert_eq!(desc.civ.as_ref().map(|c| c.address), Some(0x94));
        assert!(desc.kenwood.is_none());
        assert!(desc.capabilities.has(Capability::ReadPtt));
    }

    // Tests [@ANCHOR: radioctl_core_description_ts2000]
    #[test]
    fn ts2000_is_kenwood_without_ptt_readback() {
        let desc = RigDescription::ts2000();
        assert_eq!(desc.family, Family::KenwoodAscii);
        assert_eq!(desc.kenwood.as_ref().map(|k| k.model_id), Some(19));
        assert!(!desc.capabilities.has(Capability::ReadPtt));
        assert!(desc.capabilities.has(Capability::SetPtt));
    }
}
