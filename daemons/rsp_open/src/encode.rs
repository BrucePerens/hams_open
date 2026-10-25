// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pure control-to-register encoding: the chipset layer shared by every model (tuner words,
//! ADC clock, stream formats) and the RSP2pro's board layer (the RSP1A's is in `rsp1a`).
//!
//! Every function here turns a receiver setting into the register value or board byte that the
//! device expects, following `docs/PROTOCOL.md` (section numbers in comments refer to it).
//! Nothing in this module touches USB, so all of it is unit-tested against captured values.

use crate::Error;

/// Reference crystal of the receiver, Hz (24 MHz TCXO).
pub const F_REF_HZ: u64 = 24_000_000;

/// MSi2500 register that forwards a 24-bit word to the MSi001 tuner over its SPI port (1.6).
pub const REG_TUNER: u8 = 0x09;

/// Antenna input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Antenna {
    A,
    B,
    /// The high-impedance HF input (the MSi001's other AM input). Only below 60 MHz.
    HiZ,
}

/// Tuner IF mode (MSi001 r0 bits 13:12, 1.6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IfMode {
    Zero,
    /// 450 kHz low IF (used with a 2.000 MHz ADC clock).
    Low450k,
    /// 1.620 MHz low IF (used with a 6.000 MHz ADC clock).
    Low1620k,
    /// 2.048 MHz low IF (used with an 8.192 MHz ADC clock).
    Low2048k,
}

impl IfMode {
    pub fn code(self) -> u32 {
        // [@ANCHOR: rsp_encode_ifmode_code]
        match self {
            IfMode::Zero => 3,
            IfMode::Low450k => 2,
            IfMode::Low1620k => 1,
            IfMode::Low2048k => 0,
        }
    }
    /// IF frequency in Hz (0 for zero IF).
    pub fn hz(self) -> u32 {
        // [@ANCHOR: rsp_encode_ifmode_hz]
        match self {
            IfMode::Zero => 0,
            IfMode::Low450k => 450_000,
            IfMode::Low1620k => 1_620_000,
            IfMode::Low2048k => 2_048_000,
        }
    }
    /// The only ADC clock each low-IF mode was observed with.
    pub fn required_adc_hz(self) -> Option<u32> {
        // [@ANCHOR: rsp_encode_ifmode_required_adc_hz]
        match self {
            IfMode::Zero => None,
            IfMode::Low450k => Some(2_000_000),
            IfMode::Low1620k => Some(6_000_000),
            IfMode::Low2048k => Some(8_192_000),
        }
    }
}

/// Tuner IF bandwidth (MSi001 r0 bits 16:14, 1.6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bandwidth {
    K200,
    K300,
    K600,
    K1536,
    M5,
    M6,
    M7,
    M8,
}

impl Bandwidth {
    pub fn code(self) -> u32 {
        // [@ANCHOR: rsp_encode_bandwidth_code]
        match self {
            Bandwidth::K200 => 0,
            Bandwidth::K300 => 1,
            Bandwidth::K600 => 2,
            Bandwidth::K1536 => 3,
            Bandwidth::M5 => 4,
            Bandwidth::M6 => 5,
            Bandwidth::M7 => 6,
            Bandwidth::M8 => 7,
        }
    }
    pub fn hz(self) -> u32 {
        [
            200_000, 300_000, 600_000, 1_536_000, 5_000_000, 6_000_000, 7_000_000, 8_000_000,
        ][self.code() as usize]
    }
    /// Widest bandwidth not above `hz` (at least 200 kHz).
    pub fn at_most(hz: u32) -> Bandwidth {
        // [@ANCHOR: rsp_encode_bandwidth_at_most]
        use Bandwidth::*;
        let all = [K200, K300, K600, K1536, M5, M6, M7, M8];
        let mut best = K200;
        for b in all {
            if b.hz() <= hz {
                best = b;
            }
        }
        best
    }
}

/// MSi001 operating mode (band mode byte in r0 bits 11:4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TunerMode {
    /// AM mode with the first IF of 120 MHz: up-conversion below 60 MHz,
    /// down-conversion from 250 MHz. Used with Antenna A/B (AM port 2).
    Am,
    Vhf,
    B3,
    B45,
    L,
}

impl TunerMode {
    /// Local-oscillator output divider (1.6.2).
    pub fn lo_div(self) -> u64 {
        // [@ANCHOR: rsp_encode_tunermode_lo_div]
        match self {
            TunerMode::Am => 16,
            TunerMode::Vhf => 32,
            TunerMode::B3 => 16,
            TunerMode::B45 => 4,
            TunerMode::L => 2,
        }
    }
}

/// How the first local oscillator relates to RF in AM mode (1.6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoPlan {
    /// f_LO = f_RF (no first IF).
    Direct,
    /// f_LO = f_RF + 120 MHz (below 60 MHz).
    UpBy120M,
    /// f_LO = f_RF - 120 MHz (250 to 375 MHz).
    DownBy120M,
    /// f_LO = f_RF - 144 MHz (about 375 to 395 MHz; first IF 144 MHz, r0 bits 19:17 = 1).
    DownBy144M,
    /// f_LO = f_RF - 168 MHz (about 395 to 420 MHz; first IF 168 MHz, r0 bits 19:17 = 0).
    DownBy168M,
}

/// Which LNA-state table applies (1.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GainTable {
    /// AM mode bands (below 60 MHz and, on the RSP2pro, 250-420 MHz) on Antenna A/B.
    Am,
    /// AM-mode down-conversion band 250-420 MHz where it has its own table (RSP1A, 2.8).
    /// The RSP2pro's band table never selects it; its LNA table maps it to the AM table.
    AmDown,
    /// 60-250 MHz.
    Vhf,
    /// 420-1000 MHz.
    Uhf,
    /// 1000-2000 MHz.
    L,
    /// Hi-Z port below 60 MHz.
    HiZ,
}

/// Everything that depends only on the RF frequency band (1.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    /// Inclusive lower edge, Hz.
    pub lo_hz: u64,
    /// Exclusive upper edge, Hz (the top band includes 2 GHz itself).
    pub hi_hz: u64,
    pub mode: TunerMode,
    pub lo_plan: LoPlan,
    /// Byte 0x12 bits 5:3.
    pub filter: u8,
    pub gain_table: GainTable,
}

/// Lowest and highest RF accepted, Hz.
pub const RF_MIN_HZ: u64 = 1_000;
pub const RF_MAX_HZ: u64 = 2_000_000_000;
/// Upper edge of the range where the Hi-Z port is usable.
pub const HIZ_MAX_HZ: u64 = 60_000_000;

const M: u64 = 1_000_000;

/// Band table (1.9). In 375-420 MHz the AM-mode first IF moves to 144 MHz (from about 375 MHz)
/// and 168 MHz (from about 395 MHz); the plan edges are only bracketed (PROTOCOL.md 1.6.1).
/// The vendor API runs a calibration handshake at each plan change (1.6.6); this driver does
/// not, and the 24 MHz reference spurs still land where expected (PROTOCOL.md 5.6).
pub const BANDS: [Band; 13] = [
    Band {
        lo_hz: RF_MIN_HZ,
        hi_hz: 12 * M,
        mode: TunerMode::Am,
        lo_plan: LoPlan::UpBy120M,
        filter: 0b000,
        gain_table: GainTable::Am,
    },
    Band {
        lo_hz: 12 * M,
        hi_hz: 30 * M,
        mode: TunerMode::Am,
        lo_plan: LoPlan::UpBy120M,
        filter: 0b100,
        gain_table: GainTable::Am,
    },
    Band {
        lo_hz: 30 * M,
        hi_hz: 60 * M,
        mode: TunerMode::Am,
        lo_plan: LoPlan::UpBy120M,
        filter: 0b010,
        gain_table: GainTable::Am,
    },
    Band {
        lo_hz: 60 * M,
        hi_hz: 120 * M,
        mode: TunerMode::Vhf,
        lo_plan: LoPlan::Direct,
        filter: 0b011,
        gain_table: GainTable::Vhf,
    },
    Band {
        lo_hz: 120 * M,
        hi_hz: 250 * M,
        mode: TunerMode::B3,
        lo_plan: LoPlan::Direct,
        filter: 0b111,
        gain_table: GainTable::Vhf,
    },
    Band {
        lo_hz: 250 * M,
        hi_hz: 300 * M,
        mode: TunerMode::Am,
        lo_plan: LoPlan::DownBy120M,
        filter: 0b110,
        gain_table: GainTable::Am,
    },
    Band {
        lo_hz: 300 * M,
        hi_hz: 375 * M,
        mode: TunerMode::Am,
        lo_plan: LoPlan::DownBy120M,
        filter: 0b001,
        gain_table: GainTable::Am,
    },
    Band {
        lo_hz: 375 * M,
        hi_hz: 380 * M,
        mode: TunerMode::Am,
        lo_plan: LoPlan::DownBy144M,
        filter: 0b001,
        gain_table: GainTable::Am,
    },
    Band {
        lo_hz: 380 * M,
        hi_hz: 395 * M,
        mode: TunerMode::Am,
        lo_plan: LoPlan::DownBy144M,
        filter: 0b101,
        gain_table: GainTable::Am,
    },
    Band {
        lo_hz: 395 * M,
        hi_hz: 420 * M,
        mode: TunerMode::Am,
        lo_plan: LoPlan::DownBy168M,
        filter: 0b101,
        gain_table: GainTable::Am,
    },
    Band {
        lo_hz: 420 * M,
        hi_hz: 1000 * M,
        mode: TunerMode::B45,
        lo_plan: LoPlan::Direct,
        filter: 0b011,
        gain_table: GainTable::Uhf,
    },
    Band {
        lo_hz: 1000 * M,
        hi_hz: RF_MAX_HZ + 1,
        mode: TunerMode::L,
        lo_plan: LoPlan::Direct,
        filter: 0b111,
        gain_table: GainTable::L,
    },
    // Sentinel never matched (keeps the array length documenting 10 hardware filter ranges).
    Band {
        lo_hz: u64::MAX,
        hi_hz: u64::MAX,
        mode: TunerMode::L,
        lo_plan: LoPlan::Direct,
        filter: 0,
        gain_table: GainTable::L,
    },
];

/// Find the band for an RF frequency.
pub fn band_for(rf_hz: u64) -> Result<Band, Error> {
    // [@ANCHOR: rsp_encode_band_for]
    band_in(&BANDS, rf_hz)
}

/// Find the band for an RF frequency in a model's band table. Refuses frequencies outside
/// 1 kHz..=2 GHz before the table is searched.
pub fn band_in(bands: &[Band], rf_hz: u64) -> Result<Band, Error> {
    // [@ANCHOR: rsp_encode_band_in]
    if !(RF_MIN_HZ..=RF_MAX_HZ).contains(&rf_hz) {
        return Err(Error::InvalidArgument(format!(
            "frequency {rf_hz} Hz outside {RF_MIN_HZ}..={RF_MAX_HZ} Hz"
        )));
    }
    bands
        .iter()
        .find(|b| rf_hz >= b.lo_hz && rf_hz < b.hi_hz)
        .copied()
        .ok_or_else(|| Error::InvalidArgument(format!("no band for {rf_hz} Hz")))
}

/// Band-mode byte for r0 (1.6.1); the Hi-Z port uses the MSi001's other AM input.
pub fn mode_byte(mode: TunerMode, antenna: Antenna) -> u32 {
    // [@ANCHOR: rsp_encode_mode_byte]
    match (mode, antenna) {
        (TunerMode::Am, Antenna::HiZ) => 0x61,
        (TunerMode::Am, _) => 0xe1,
        (TunerMode::Vhf, _) => 0x42,
        (TunerMode::B3, _) => 0x44,
        (TunerMode::B45, _) => 0x48,
        (TunerMode::L, _) => 0x50,
    }
}

/// The gain table that applies, given band and antenna.
pub fn gain_table(band: &Band, antenna: Antenna) -> GainTable {
    // [@ANCHOR: rsp_encode_gain_table]
    if antenna == Antenna::HiZ && band.mode == TunerMode::Am && band.hi_hz <= HIZ_MAX_HZ {
        GainTable::HiZ
    } else {
        band.gain_table
    }
}

/// Tuner SPI word: payload in bits 23:4, register address in bits 3:0 (1.6).
pub fn tuner_word(addr: u8, payload: u32) -> u32 {
    ((payload & 0xf_ffff) << 4) | (addr as u32 & 0xf)
}

/// MSi001 r0: band mode, IF mode, bandwidth, first-LO plan bits (1.6.1).
/// Bits 19:17 are 2 for the 120 MHz first IF and for the non-AM modes.
pub fn r0(mode_byte: u32, if_mode: IfMode, bw: Bandwidth) -> u32 {
    r0_plan(mode_byte, LoPlan::Direct, if_mode, bw)
}

/// r0 with the first-LO plan bits of `plan` (bits 19:17: 2 for 120 MHz and the non-AM modes,
/// 1 for 144 MHz, 0 for 168 MHz).
pub fn r0_plan(mode_byte: u32, plan: LoPlan, if_mode: IfMode, bw: Bandwidth) -> u32 {
    // [@ANCHOR: rsp_encode_r0_plan]
    let lo_bits: u32 = match plan {
        LoPlan::DownBy144M => 1,
        LoPlan::DownBy168M => 0,
        _ => 2,
    };
    (lo_bits << 17) | (bw.code() << 14) | (if_mode.code() << 12) | (mode_byte << 4)
}

/// r0 value that puts the tuner in standby.
pub const R0_STANDBY: u32 = 0x000000;
/// r5: PLL fraction modulus 3000 (constant in every capture).
pub const R5_THRESH: u32 = 0x28bb85;
/// r6: constant.
pub const R6: u32 = 0x200016;
/// r12: written before standby at close.
pub const R12_CLOSE: u32 = 0x00001c;
/// r13/r14 values written at every full tune.
pub const R13_TUNE: u32 = 0x00000d;
pub const R14_TUNE: u32 = 0x00000e;

/// Fraction modulus of the tuner PLL.
pub const PLL_MOD: u64 = 3000;
/// Extra fraction resolution carried in r3 bits 15:4 (12 bits).
pub const PLL_SUB: u64 = 4096;

/// Result of the PLL calculation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pll {
    pub n: u32,
    pub frac: u32,
    pub sub: u32,
    /// Tuner word for r2.
    pub r2: u32,
    /// Tuner word for r3.
    pub r3: u32,
    /// LO actually produced by the words, Hz (nominal reference, before ppm).
    pub lo_hz: f64,
}

/// First-LO frequency for an RF frequency in a band.
pub fn lo_hz(band: &Band, rf_hz: u64) -> u64 {
    // [@ANCHOR: rsp_encode_lo_hz]
    match band.lo_plan {
        LoPlan::Direct => rf_hz,
        LoPlan::UpBy120M => rf_hz + 120 * M,
        LoPlan::DownBy120M => rf_hz - 120 * M,
        LoPlan::DownBy144M => rf_hz - 144 * M,
        LoPlan::DownBy168M => rf_hz - 168 * M,
    }
}

/// Tuner PLL words for a first-LO frequency (1.6.2 plus the r3 fit below).
///
/// The VCO runs at `lo * div`; one PLL unit is 96 MHz / 3000 (= 32 kHz) of VCO, and r3 bits
/// 15:4 add 12 more fraction bits. Fitting every captured (frequency, r2, r3) pair: the value
/// written is the largest integer strictly below the exact quotient, so an exact multiple shows
/// as `frac - 1` with r3 = 0xfff (e.g. 100 MHz -> r2 0x213e72, r3 0x00fff3). `ppb` is the
/// reference correction in parts per billion (positive = reference runs fast).
pub fn pll(lo_hz: u64, div: u64, ppb: i64) -> Result<Pll, Error> {
    // [@ANCHOR: rsp_encode_pll]
    // q = ceil(lo * div * 3000 * 4096 / (96e6 * (1 + ppb/1e9))) - 1, in exact integers.
    let num: u128 = lo_hz as u128 * div as u128 * (PLL_MOD * PLL_SUB) as u128 * 1_000_000_000u128;
    let den_i: i128 = 96_000_000i128 * (1_000_000_000i128 + ppb as i128);
    if den_i <= 0 {
        return Err(Error::InvalidArgument("ppm correction out of range".into()));
    }
    let den = den_i as u128;
    let q = num.div_ceil(den) - 1;
    let units = q / PLL_SUB as u128;
    let sub = (q % PLL_SUB as u128) as u32;
    let n = (units / PLL_MOD as u128) as u32;
    let frac = (units % PLL_MOD as u128) as u32;
    if !(1..=0xff).contains(&n) {
        return Err(Error::InvalidArgument(format!(
            "PLL integer {n} out of range"
        )));
    }
    let r2 = tuner_word(2, (n << 12) | frac);
    let r3 = tuner_word(3, sub);
    let vco =
        96e6 * (n as f64 + (frac as f64 + (sub as f64 + 1.0) / PLL_SUB as f64) / PLL_MOD as f64);
    Ok(Pll {
        n,
        frac,
        sub,
        r2,
        r3,
        lo_hz: vco / div as f64,
    })
}

/// MSi001 r1 gain word: IF gain reduction (bits 9:4), AM buffer step (11:10), mixer GR (12),
/// LNA GR (13), bits 16:14 = 3 (1.6.3).
pub fn r1(if_gr_db: u8, fe: &FrontEndStep) -> u32 {
    let payload = (3u32 << 10)
        | ((fe.lna_gr as u32) << 9)
        | ((fe.mixer_gr as u32) << 8)
        | ((fe.am_buf as u32 & 3) << 6)
        | (if_gr_db as u32 & 0x3f);
    tuner_word(1, payload)
}

pub const IF_GR_MAX: u8 = 59;

/// One LNA state: the board front-end step (byte 0x13 bits 5:2) and the tuner gain bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrontEndStep {
    /// Byte 0x13 bits 5:2.
    pub board_step: u8,
    /// r1 bits 11:10.
    pub am_buf: u8,
    pub mixer_gr: bool,
    pub lna_gr: bool,
}

const fn st(board_step: u8, am_buf: u8, mixer_gr: bool, lna_gr: bool) -> FrontEndStep {
    FrontEndStep {
        board_step,
        am_buf,
        mixer_gr,
        lna_gr,
    }
}

// Board step codes seen in byte 0x13 bits 5:2 (0xb9, 0xb5, 0xa5, 0x95 with notch/bias-T off).
const S_E: u8 = 0b1110;
const S_D: u8 = 0b1101;
const S_9: u8 = 0b1001;
const S_5: u8 = 0b0101;

/// LNA-state tables (1.8, gain and toggle captures).
pub fn lna_table(t: GainTable) -> &'static [FrontEndStep] {
    // [@ANCHOR: rsp_encode_lna_table]
    const AM: [FrontEndStep; 9] = [
        st(S_E, 0, false, false),
        st(S_D, 0, false, false),
        st(S_9, 0, false, false),
        st(S_5, 0, false, false),
        st(S_E, 3, false, false),
        st(S_D, 3, false, false),
        st(S_9, 3, false, false),
        st(S_5, 3, false, false),
        st(S_5, 3, true, false),
    ];
    const VHF: [FrontEndStep; 9] = [
        st(S_E, 0, false, false),
        st(S_D, 0, false, false),
        st(S_9, 0, false, false),
        st(S_5, 0, false, false),
        st(S_E, 0, false, true),
        st(S_D, 0, false, true),
        st(S_9, 0, false, true),
        st(S_5, 0, false, true),
        st(S_5, 0, true, true),
    ];
    const UHF: [FrontEndStep; 6] = [
        st(S_E, 0, false, false),
        st(S_E, 0, false, true),
        st(S_D, 0, false, false),
        st(S_D, 0, false, true),
        st(S_9, 0, false, true),
        st(S_9, 0, true, true),
    ];
    const L: [FrontEndStep; 6] = [
        st(S_E, 0, false, false),
        st(S_E, 0, false, true),
        st(S_5, 0, false, false),
        st(S_9, 0, false, false),
        st(S_D, 0, false, true),
        st(S_D, 0, true, true),
    ];
    const HIZ: [FrontEndStep; 5] = [
        st(S_E, 0, false, false),
        st(S_E, 1, false, false),
        st(S_E, 2, false, false),
        st(S_E, 3, false, false),
        st(S_E, 3, true, false),
    ];
    match t {
        GainTable::Am | GainTable::AmDown => &AM,
        GainTable::Vhf => &VHF,
        GainTable::Uhf => &UHF,
        GainTable::L => &L,
        GainTable::HiZ => &HIZ,
    }
}

/// Board-control register addresses written with request 0x49 (1.8, 1.8.1). The board's
/// control bits sit on a Microchip MCP23S18 SPI I/O expander (identified from a photograph of
/// the board); in its default register map (IOCON.BANK = 0, datasheet DS22103A Table 1-1) these
/// addresses are the port registers below. Request 0x49's wIndex 0x40 is the expander's SPI
/// write control byte.
/// GPIOA: antenna, band filter, reference output ("byte 0x12" in the protocol document).
pub const MCP_GPIOA: u8 = 0x12;
/// GPIOB: notch, front-end step, bias-T ("byte 0x13").
pub const MCP_GPIOB: u8 = 0x13;
/// IODIRA: written 0x00 (all port A pins outputs) after the port values, as the API does.
pub const MCP_IODIRA: u8 = 0x00;
/// IODIRB: written 0x00 (all port B pins outputs).
pub const MCP_IODIRB: u8 = 0x01;
/// The expander's SPI write control byte, carried in request 0x49's wIndex.
pub const MCP_SPI_WRITE: u16 = 0x0040;

/// MCP23S18 GPIOA ("byte 0x12"): antenna (GPA1:0), band filter (GPA5:3), GPA2 always set,
/// ext-ref output (GPA6, 0 = output enabled; the expander's outputs are open-drain).
pub fn board_band_byte(filter: u8, antenna: Antenna, ext_ref_out: bool) -> u8 {
    // [@ANCHOR: rsp_encode_board_band_byte]
    let ant = match antenna {
        Antenna::A | Antenna::HiZ => 0b01,
        Antenna::B => 0b10,
    };
    let refbit = if ext_ref_out { 0 } else { 1 << 6 };
    refbit | ((filter & 7) << 3) | (1 << 2) | ant
}

/// MCP23S18 GPIOB ("byte 0x13"): notch (GPB7:6, 10 = off, 01 = on), front-end step (GPB5:2),
/// bias-T (GPB0, 0 = on).
pub fn board_front_byte(board_step: u8, notch: bool, bias_t: bool) -> u8 {
    // [@ANCHOR: rsp_encode_board_front_byte]
    let notch_bits = if notch { 0b01 } else { 0b10 };
    let bias = if bias_t { 0 } else { 1 };
    (notch_bits << 6) | ((board_step & 0xf) << 2) | bias
}

/// The transient front-end byte written during every full retune (0xa5 with bias-T off).
/// The bias-T bit follows the current setting so a retune does not glitch the bias supply
/// (the API's value with bias-T on was not captured).
pub fn board_front_transient(bias_t: bool) -> u8 {
    board_front_byte(S_9, false, bias_t)
}

/// Stream sample format (1.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// 252 IQ pairs per block, 16-bit words carrying 14-bit signed samples.
    S14,
    /// 336 IQ pairs per block, 12-bit packed.
    S12,
    /// 384 IQ pairs per block, 10-bit packed with a per-group shift.
    S10x384,
    /// 504 IQ pairs per block, 8-bit signed.
    S8,
}

impl SampleFormat {
    pub fn pairs_per_block(self) -> u32 {
        // [@ANCHOR: rsp_encode_sampleformat_pairs_per_block]
        match self {
            SampleFormat::S14 => 252,
            SampleFormat::S12 => 336,
            SampleFormat::S10x384 => 384,
            SampleFormat::S8 => 504,
        }
    }
}

/// Streaming configuration chosen for an ADC clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamConfig {
    pub alt: u8,
    pub format: SampleFormat,
    /// MSi2500 register 0x07 value.
    pub reg07: u32,
    /// Device decimates by 2 (low-IF modes, reg 0x07 bit 10).
    pub decim2: bool,
}

impl StreamConfig {
    /// Advance of the block sample counter per 1024-byte block.
    pub fn counter_step(&self) -> u32 {
        // [@ANCHOR: rsp_encode_streamconfig_counter_step]
        self.format.pairs_per_block() * if self.decim2 { 2 } else { 1 }
    }
}

/// ADC clock limits accepted by this driver, Hz.
pub const ADC_MIN_HZ: u32 = 2_000_000;
pub const ADC_MAX_HZ: u32 = 10_000_000;

/// Pick the alternate setting and sample format (1.7). Alt 2 carries one 1024-byte block per
/// microframe (8000/s), alt 1 three (24000/s); the API picks the widest format whose block
/// rate fits. Only the (alt, format, reg 0x07) combinations seen on the wire are produced.
pub fn stream_config(adc_hz: u32, if_mode: IfMode) -> Result<StreamConfig, Error> {
    // [@ANCHOR: rsp_encode_stream_config]
    if !(ADC_MIN_HZ..=ADC_MAX_HZ).contains(&adc_hz) {
        return Err(Error::InvalidArgument(format!(
            "ADC clock {adc_hz} Hz outside {ADC_MIN_HZ}..={ADC_MAX_HZ} Hz"
        )));
    }
    if let Some(req) = if_mode.required_adc_hz() {
        if req != adc_hz {
            return Err(Error::Unsupported(format!(
                "{:?} was only observed with a {} Hz ADC clock",
                if_mode, req
            )));
        }
        return Ok(if adc_hz < 3_000_000 {
            StreamConfig {
                alt: 2,
                format: SampleFormat::S14,
                reg07: 0x000414,
                decim2: true,
            }
        } else {
            StreamConfig {
                alt: 1,
                format: SampleFormat::S14,
                reg07: 0x000494,
                decim2: true,
            }
        });
    }
    let f = adc_hz as u64;
    let fits = |pairs: u64, blocks_per_s: u64| f <= pairs * blocks_per_s;
    let c = if fits(252, 8000) {
        StreamConfig {
            alt: 2,
            format: SampleFormat::S14,
            reg07: 0x000014,
            decim2: false,
        }
    } else if fits(336, 8000) {
        StreamConfig {
            alt: 2,
            format: SampleFormat::S12,
            reg07: 0x000005,
            decim2: false,
        }
    } else if f < 3_000_000 {
        return Err(Error::Unsupported(format!(
            "ADC clock {adc_hz} Hz between 2.688 and 3 MHz was not observed"
        )));
    } else if fits(252, 24000) {
        StreamConfig {
            alt: 1,
            format: SampleFormat::S14,
            reg07: 0x000094,
            decim2: false,
        }
    } else if fits(336, 24000) {
        StreamConfig {
            alt: 1,
            format: SampleFormat::S12,
            reg07: 0x000085,
            decim2: false,
        }
    } else if fits(384, 24000) {
        StreamConfig {
            alt: 1,
            format: SampleFormat::S10x384,
            reg07: 0x0000a5,
            decim2: false,
        }
    } else {
        StreamConfig {
            alt: 1,
            format: SampleFormat::S8,
            reg07: 0x000c94,
            decim2: false,
        }
    };
    Ok(c)
}

/// ADC clock synthesiser words (MSi2500 registers 0x03 and 0x04, 1.7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdcClock {
    pub reg03: u32,
    pub reg04: u32,
    /// Clock actually produced, Hz (nominal reference).
    pub actual_hz: f64,
}

/// Written to reg 0x03 between STOP and the alt-setting change on a sample-rate change.
pub const REG03_RATE_CHANGE: u32 = 0x011300;
/// Written to reg 0x03 at close.
pub const REG03_CLOSE: u32 = 0x091300;

/// ADC clock words. `f = 4 MHz * (N + frac/2^21) / (2(d+1))`. The divider is the observed one
/// for every rate seen; for other rates the same rule (largest even divider <= 16 that keeps
/// `f * divider` within 32..40 MHz, else within 32..42 MHz) is applied. `flag12` is bit 12, set
/// by the API on the write made at open and on ppm changes, clear on sample-rate changes.
pub fn adc_clock(adc_hz: u32, ppb: i64, flag12: bool) -> Result<AdcClock, Error> {
    // [@ANCHOR: rsp_encode_adc_clock]
    if !(ADC_MIN_HZ..=ADC_MAX_HZ).contains(&adc_hz) {
        return Err(Error::InvalidArgument(format!(
            "ADC clock {adc_hz} Hz out of range"
        )));
    }
    let f = adc_hz as u64;
    let pick = |hi: u64| {
        (1..=8u64)
            .rev()
            .map(|k| 2 * k)
            .find(|&div| f * div >= 32 * M && f * div <= hi)
    };
    let div = pick(40 * M)
        .or_else(|| pick(42 * M))
        .ok_or_else(|| Error::Unsupported(format!("no ADC clock divider for {adc_hz} Hz")))?;
    let d = (div / 2 - 1) as u32;
    // N + frac/2^21 = f * div / 4 MHz / (1 + ppb/1e9); frac rounds down.
    let num: u128 = f as u128 * div as u128 * (1u128 << 21) * 1_000_000_000u128;
    let den: u128 = (4 * M) as u128 * (1_000_000_000i128 + ppb as i128) as u128;
    let q = num / den;
    let n = (q >> 21) as u32;
    let frac = (q & 0x1f_ffff) as u32;
    if !(1..=15).contains(&n) {
        return Err(Error::InvalidArgument(
            "ADC synthesiser out of range".into(),
        ));
    }
    let range: u32 = if adc_hz < 6_000_000 {
        0
    } else if adc_hz < 7_000_000 {
        1
    } else if adc_hz <= 8_000_000 {
        2
    } else {
        3
    };
    let reg03 = (0x01 << 16)
        | (range << 14)
        | ((flag12 as u32) << 12)
        | (n << 8)
        | (((frac >> 20) & 1) << 7)
        | (d << 2)
        | 3;
    let reg04 = frac & 0xf_ffff;
    let actual_hz = 4e6 * (n as f64 + frac as f64 / (1u64 << 21) as f64) / div as f64;
    Ok(AdcClock {
        reg03,
        reg04,
        actual_hz,
    })
}

/// Encode a 24-bit write to an MSi2500 register as request 0x41 (wValue, wIndex) (1.4).
pub fn reg_write_args(reg: u8, value: u32) -> (u16, u16) {
    let value = value & 0xff_ffff;
    (
        (((value & 0xff) << 8) | reg as u32) as u16,
        (value >> 8) as u16,
    )
}

/// Encode a write to an MCP23S18 register as request 0x49 (wValue, wIndex) (1.8):
/// wValue = register << 8 | data, wIndex = the expander's SPI write control byte.
pub fn board_write_args(addr: u8, byte: u8) -> (u16, u16) {
    (((addr as u16) << 8) | byte as u16, MCP_SPI_WRITE)
}

/// Convert a ppm correction to parts per billion, rejecting absurd values.
pub fn ppm_to_ppb(ppm: f64) -> Result<i64, Error> {
    // [@ANCHOR: rsp_encode_ppm_to_ppb]
    if !ppm.is_finite() || ppm.abs() > 1000.0 {
        return Err(Error::InvalidArgument(format!(
            "ppm {ppm} out of range (|ppm| <= 1000)"
        )));
    }
    Ok((ppm * 1000.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t2(rf: u64) -> (u32, u32) {
        let b = band_for(rf).unwrap();
        let p = pll(lo_hz(&b, rf), b.mode.lo_div(), 0).unwrap();
        (p.r2, p.r3)
    }

    /// (RF Hz, r2, r3) pairs from the frequency-sweep and band-edge captures (r3 only where it was written differently).
    #[test]
    // Tests [@ANCHOR: rsp_encode_pll]
    // Tests [@ANCHOR: rsp_encode_band_for]
    // Tests [@ANCHOR: rsp_encode_lo_hz]
    // Tests [@ANCHOR: rsp_encode_tunermode_lo_div]
    fn pll_matches_captures() {
        let cases: &[(u64, u32)] = &[
            (1_000, 0x140002),
            (10_000, 0x140042),
            (100_000, 0x140312),
            (200_000, 0x140632),
            (500_000, 0x140f92),
            (1_000_000, 0x141f32),
            (2_000_000, 0x143e72),
            (5_000_000, 0x149c32),
            (10_000_000, 0x157cf2),
            (10_100_000, 0x158012),
            (11_900_000, 0x15b852),
            (12_000_000, 0x15bb72),
            (12_100_000, 0x160312),
            (14_000_000, 0x163e72),
            (20_000_000, 0x173e72),
            (29_500_000, 0x18abd2),
            (30_000_000, 0x18bb72),
            (40_000_000, 0x1a7cf2),
            (55_250_000, 0x1d2702),
            (59_750_000, 0x1db3a2),
            (60_000_000, 0x13bb72),
            (60_250_000, 0x140f92),
            (61_000_000, 0x143e72),
            (100_000_000, 0x213e72),
            (107_000_000, 0x237cf2),
            (119_000_000, 0x277cf2),
            (120_000_000, 0x13bb72),
            (121_000_000, 0x141f32),
            (200_000_000, 0x213e72),
            (247_500_000, 0x292ed2),
            (250_000_000, 0x157cf2),
            (260_000_000, 0x173e72),
            (300_000_000, 0x1dbb72),
            (330_000_000, 0x22bb72),
            (331_000_000, 0x231f32),
            (420_000_000, 0x115db2),
            (421_000_000, 0x116582),
            (422_500_000, 0x117142),
            (430_000_000, 0x11abd2),
            (500_000_000, 0x149c32),
            (800_000_000, 0x213e72),
            (900_000_000, 0x255db2),
            (952_500_000, 0x2780e2),
            (961_000_000, 0x2807c2),
            (997_500_000, 0x296972),
            (1_000_000_000, 0x149c32),
            (1_002_500_000, 0x14a602),
            (1_005_000_000, 0x14afc2),
            (1_007_500_000, 0x14b982),
            (1_100_000_000, 0x16abd2),
            (1_500_000_000, 0x1f2ed2),
            (1_800_000_000, 0x255db2),
            (2_000_000_000, 0x297cf2),
        ];
        for &(rf, r2) in cases {
            assert_eq!(t2(rf).0, r2, "r2 at {rf} Hz");
        }
        let r3_cases: &[(u64, u32)] = &[
            (1_000, 0x007ff3),
            (10_000, 0x00fff3),
            (100_000_000, 0x00fff3),
            (422_500_000, 0x007ff3),
            (425_000_000, 0x00fff3),
            (952_500_000, 0x007ff3),
            (955_000_000, 0x00fff3),
            (1_000_000_000, 0x00fff3),
            (1_002_500_000, 0x003ff3),
            (1_005_000_000, 0x007ff3),
            (1_007_500_000, 0x00bff3),
            (1_010_000_000, 0x00fff3),
        ];
        for &(rf, r3) in r3_cases {
            assert_eq!(t2(rf).1, r3, "r3 at {rf} Hz");
        }
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_pll]
    fn pll_ppm_matches_capture() {
        // ppm +10 at 100 MHz: r2 unchanged, r3 = 000003 (toggle capture).
        let p = pll(100_000_000, 32, 10_000).unwrap();
        assert_eq!(p.r2, 0x213e72);
        assert_eq!(p.r3, 0x000003);
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_pll]
    // Tests [@ANCHOR: rsp_encode_lo_hz]
    fn pll_lo_is_close() {
        for rf in [
            1_000u64,
            7_074_000,
            95_800_000,
            143_800_000,
            446_000_000,
            1_296_000_000,
        ] {
            let b = band_for(rf).unwrap();
            let lo = lo_hz(&b, rf);
            let p = pll(lo, b.mode.lo_div(), 0).unwrap();
            assert!((p.lo_hz - lo as f64).abs() < 1.0, "{rf}: {}", p.lo_hz);
        }
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_band_for]
    // Tests [@ANCHOR: rsp_encode_board_band_byte]
    // Tests [@ANCHOR: rsp_encode_mode_byte]
    // Tests [@ANCHOR: rsp_encode_ifmode_code]
    // Tests [@ANCHOR: rsp_encode_bandwidth_code]
    fn band_table_matches_capture() {
        // (RF, byte 0x12 with antenna A, r0 with zero IF and 200 kHz BW) from 1.9 (frequency-sweep and band-edge captures).
        let cases: &[(u64, u8, u32)] = &[
            (1_000, 0x45, 0x043e10),
            (11_900_000, 0x45, 0x043e10),
            (12_000_000, 0x65, 0x043e10),
            (29_500_000, 0x65, 0x043e10),
            (30_000_000, 0x55, 0x043e10),
            (59_750_000, 0x55, 0x043e10),
            (60_000_000, 0x5d, 0x043420),
            (119_000_000, 0x5d, 0x043420),
            (120_000_000, 0x7d, 0x043440),
            (247_500_000, 0x7d, 0x043440),
            (250_000_000, 0x75, 0x043e10),
            (297_500_000, 0x75, 0x043e10),
            (300_000_000, 0x4d, 0x043e10),
            (330_000_000, 0x4d, 0x043e10),
            (420_000_000, 0x5d, 0x043480),
            (997_500_000, 0x5d, 0x043480),
            (1_000_000_000, 0x7d, 0x043500),
            (2_000_000_000, 0x7d, 0x043500),
        ];
        for &(rf, b12, r0v) in cases {
            let b = band_for(rf).unwrap();
            assert_eq!(
                board_band_byte(b.filter, Antenna::A, false),
                b12,
                "B12 at {rf}"
            );
            let w = tuner_word(
                0,
                r0(mode_byte(b.mode, Antenna::A), IfMode::Zero, Bandwidth::K200) >> 4,
            );
            assert_eq!(w, r0v, "r0 at {rf}");
        }
        assert_eq!(band_for(375_000_000).unwrap().lo_plan, LoPlan::DownBy144M);
        assert_eq!(band_for(394_999_999).unwrap().lo_plan, LoPlan::DownBy144M);
        assert_eq!(band_for(395_000_000).unwrap().lo_plan, LoPlan::DownBy168M);
        assert_eq!(band_for(419_999_999).unwrap().lo_plan, LoPlan::DownBy168M);
        assert!(band_for(374_999_999).is_ok());
        assert!(band_for(999).is_err());
        assert!(band_for(2_000_000_001).is_err());
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_ifmode_code]
    // Tests [@ANCHOR: rsp_encode_bandwidth_code]
    // Tests [@ANCHOR: rsp_encode_mode_byte]
    fn r0_variants() {
        let v = |m, i, b| r0(m, i, b);
        assert_eq!(v(0x42, IfMode::Zero, Bandwidth::K1536), 0x04f420);
        assert_eq!(v(0x42, IfMode::Zero, Bandwidth::K300), 0x047420);
        assert_eq!(v(0x42, IfMode::Zero, Bandwidth::K600), 0x04b420);
        assert_eq!(v(0x42, IfMode::Zero, Bandwidth::M5), 0x053420);
        assert_eq!(v(0x42, IfMode::Zero, Bandwidth::M6), 0x057420);
        assert_eq!(v(0x42, IfMode::Zero, Bandwidth::M7), 0x05b420);
        assert_eq!(v(0x42, IfMode::Zero, Bandwidth::M8), 0x05f420);
        assert_eq!(v(0x42, IfMode::Low450k, Bandwidth::K200), 0x042420);
        assert_eq!(v(0x42, IfMode::Low450k, Bandwidth::K600), 0x04a420);
        assert_eq!(v(0x42, IfMode::Low1620k, Bandwidth::K1536), 0x04d420);
        assert_eq!(v(0x42, IfMode::Low2048k, Bandwidth::K1536), 0x04c420);
        assert_eq!(v(0x42, IfMode::Low2048k, Bandwidth::M5), 0x050420);
        assert_eq!(
            v(
                mode_byte(TunerMode::Am, Antenna::HiZ),
                IfMode::Zero,
                Bandwidth::K200
            ),
            0x043610
        );
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_lna_table]
    // Tests [@ANCHOR: rsp_encode_board_front_byte]
    fn gain_words_match_capture() {
        let vhf = lna_table(GainTable::Vhf);
        assert_eq!(r1(40, &vhf[0]), 0x00c281);
        for (gr, w) in [
            (20, 0x00c141),
            (21, 0x00c151),
            (25, 0x00c191),
            (30, 0x00c1e1),
            (45, 0x00c2d1),
            (50, 0x00c321),
        ] {
            assert_eq!(r1(gr, &vhf[0]), w);
        }
        let hiz = lna_table(GainTable::HiZ);
        let want = [0x00c281, 0x00c681, 0x00ca81, 0x00ce81, 0x00de81];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(r1(40, &hiz[i]), *w, "Hi-Z state {i}");
            assert_eq!(board_front_byte(hiz[i].board_step, false, false), 0xb9);
        }
        // Front-end bytes per state (1.8 table).
        let fe = |t| {
            lna_table(t)
                .iter()
                .map(|s| board_front_byte(s.board_step, false, false))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            fe(GainTable::Am),
            vec![0xb9, 0xb5, 0xa5, 0x95, 0xb9, 0xb5, 0xa5, 0x95, 0x95]
        );
        assert_eq!(
            fe(GainTable::Vhf),
            vec![0xb9, 0xb5, 0xa5, 0x95, 0xb9, 0xb5, 0xa5, 0x95, 0x95]
        );
        assert_eq!(fe(GainTable::Uhf), vec![0xb9, 0xb9, 0xb5, 0xb5, 0xa5, 0xa5]);
        assert_eq!(fe(GainTable::L), vec![0xb9, 0xb9, 0x95, 0xa5, 0xb5, 0xb5]);
        // AM states 4..7 set AM buffer 3; state 8 adds mixer GR; VHF uses the LNA GR bit.
        assert_eq!(r1(40, &lna_table(GainTable::Am)[4]) & 0x3c00, 0x0c00);
        assert_eq!(r1(40, &lna_table(GainTable::Am)[8]) & 0x3c00, 0x1c00);
        assert_eq!(r1(40, &vhf[4]) & 0x3c00, 0x2000);
        assert_eq!(r1(40, &vhf[8]) & 0x3c00, 0x3000);
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_board_band_byte]
    // Tests [@ANCHOR: rsp_encode_board_front_byte]
    fn board_bytes() {
        assert_eq!(board_band_byte(0b011, Antenna::A, false), 0x5d);
        assert_eq!(board_band_byte(0b011, Antenna::B, false), 0x5e);
        assert_eq!(board_band_byte(0b011, Antenna::A, true), 0x1d);
        assert_eq!(board_front_byte(S_E, false, false), 0xb9);
        assert_eq!(board_front_byte(S_E, true, false), 0x79);
        assert_eq!(board_front_byte(S_E, false, true), 0xb8);
        assert_eq!(board_front_transient(false), 0xa5);
        assert_eq!(board_write_args(0x12, 0x5d), (0x125d, 0x0040));
        assert_eq!(board_write_args(0x01, 0x00), (0x0100, 0x0040));
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_adc_clock]
    fn adc_clock_matches_capture() {
        let cases: &[(u32, u32, u32)] = &[
            (2_000_000, 0x01081f, 0),
            (2_048_000, 0x01081f, 0x0624dd),
            (3_000_000, 0x010917, 0),
            (4_000_000, 0x010a13, 0),
            (5_000_000, 0x010a0f, 0),
            (6_000_000, 0x01490b, 0),
            (7_000_000, 0x018a8b, 0),
            (8_000_000, 0x018807, 0),
            (8_192_000, 0x01c807, 0x0624dd),
            (9_000_000, 0x01c907, 0),
            (10_000_000, 0x01ca07, 0),
        ];
        for &(f, r3, r4) in cases {
            let c = adc_clock(f, 0, false).unwrap();
            assert_eq!((c.reg03, c.reg04), (r3, r4), "ADC {f}");
            assert!((c.actual_hz - f as f64).abs() < 1.0);
        }
        // Writes made at open carry bit 12.
        assert_eq!(adc_clock(2_000_000, 0, true).unwrap().reg03, 0x01181f);
        assert_eq!(adc_clock(8_000_000, 0, true).unwrap().reg03, 0x019807);
        assert_eq!(adc_clock(6_000_000, 0, true).unwrap().reg03, 0x01590b);
        assert_eq!(adc_clock(8_192_000, 0, true).unwrap().reg03, 0x01d807);
        // ppm +10 at 2 MHz (toggle capture).
        let c = adc_clock(2_000_000, 10_000, true).unwrap();
        assert_eq!((c.reg03, c.reg04), (0x01179f, 0x0fff58));
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_stream_config]
    // Tests [@ANCHOR: rsp_encode_streamconfig_counter_step]
    // Tests [@ANCHOR: rsp_encode_ifmode_required_adc_hz]
    // Tests [@ANCHOR: rsp_encode_sampleformat_pairs_per_block]
    fn stream_configs_match_capture() {
        let z = IfMode::Zero;
        let c = |f| stream_config(f, z).unwrap();
        assert_eq!((c(2_000_000).alt, c(2_000_000).reg07), (2, 0x14));
        assert_eq!((c(2_048_000).alt, c(2_048_000).reg07), (2, 0x05));
        for f in [3_000_000, 4_000_000, 5_000_000, 6_000_000] {
            assert_eq!((c(f).alt, c(f).reg07), (1, 0x94), "{f}");
        }
        for f in [7_000_000, 8_000_000] {
            assert_eq!((c(f).alt, c(f).reg07), (1, 0x85), "{f}");
        }
        for f in [8_192_000, 9_000_000] {
            assert_eq!((c(f).alt, c(f).reg07), (1, 0xa5), "{f}");
        }
        assert_eq!((c(10_000_000).alt, c(10_000_000).reg07), (1, 0xc94));
        assert_eq!(c(10_000_000).counter_step(), 504);
        let l = stream_config(2_000_000, IfMode::Low450k).unwrap();
        assert_eq!((l.alt, l.reg07, l.counter_step()), (2, 0x414, 504));
        let l = stream_config(6_000_000, IfMode::Low1620k).unwrap();
        assert_eq!((l.alt, l.reg07), (1, 0x494));
        assert!(stream_config(4_000_000, IfMode::Low1620k).is_err());
        assert!(stream_config(2_800_000, z).is_err());
        assert!(stream_config(1_536_000, z).is_err());
        assert!(stream_config(10_660_000, z).is_err());
    }

    #[test]
    fn register_write_encoding() {
        assert_eq!(reg_write_args(0x08, 0x00ee80), (0x8008, 0x00ee));
        assert_eq!(reg_write_args(REG_TUNER, 0x213e72), (0x7209, 0x213e));
        assert_eq!(reg_write_args(0x03, 0x01181f), (0x1f03, 0x0118));
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_ifmode_hz]
    // Tests [@ANCHOR: rsp_encode_bandwidth_at_most]
    // Tests [@ANCHOR: rsp_encode_ifmode_required_adc_hz]
    fn if_and_bandwidth_helpers() {
        assert_eq!(IfMode::Zero.hz(), 0);
        assert_eq!(IfMode::Low450k.hz(), 450_000);
        assert_eq!(IfMode::Low1620k.hz(), 1_620_000);
        assert_eq!(IfMode::Low2048k.hz(), 2_048_000);
        assert_eq!(IfMode::Low1620k.required_adc_hz(), Some(6_000_000));
        assert_eq!(IfMode::Zero.required_adc_hz(), None);
        assert_eq!(Bandwidth::at_most(100_000), Bandwidth::K200);
        assert_eq!(Bandwidth::at_most(1_600_000), Bandwidth::K1536);
        assert_eq!(Bandwidth::at_most(6_000_000), Bandwidth::M6);
        assert_eq!(Bandwidth::at_most(20_000_000), Bandwidth::M8);
        assert_eq!(Bandwidth::M7.hz(), 7_000_000);
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_ppm_to_ppb]
    fn ppm_conversion_and_limits() {
        assert_eq!(ppm_to_ppb(10.0).unwrap(), 10_000);
        assert_eq!(ppm_to_ppb(-3.5).unwrap(), -3_500);
        assert!(ppm_to_ppb(1000.1).is_err());
        assert!(ppm_to_ppb(f64::NAN).is_err());
    }

    #[test]
    // Tests [@ANCHOR: rsp_encode_gain_table]
    fn hiz_uses_its_own_gain_table_only_below_60_mhz() {
        let hf = band_for(10_000_000).unwrap();
        assert_eq!(gain_table(&hf, Antenna::HiZ), GainTable::HiZ);
        assert_eq!(gain_table(&hf, Antenna::A), GainTable::Am);
        let am_down = band_for(264_000_000).unwrap();
        assert_eq!(gain_table(&am_down, Antenna::HiZ), GainTable::Am);
        let vhf = band_for(100_000_000).unwrap();
        assert_eq!(gain_table(&vhf, Antenna::B), GainTable::Vhf);
    }
}
