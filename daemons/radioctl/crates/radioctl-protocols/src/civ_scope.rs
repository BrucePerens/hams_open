// SPDX-License-Identifier: AGPL-3.0-or-later
//! Icom CI-V spectrum scope (band scope) data: command 0x27.
//!
//! **Provenance.** Icom's CI-V reference guides for the IC-7300, IC-705, IC-7610 and IC-9700 were
//! NOT used: the download service of icomjapan.com is licensed to individual use only and forbids
//! reuse of the manuals' data (no commercial use, no transfer of data), and radioctl's standing
//! rule is to skip sources whose terms forbid reuse. Every fact here was read from Hamlib 4.6.2
//! (LGPL; facts only, no code or table copied) in `rigs/icom/icom_defs.h` (the `S_SCP_*`
//! sub-command numbers, the `SCOPE_MODE_*` and `SCOPE_SPEED_*` values) and `rigs/icom/icom.c`
//! (`icom_parse_spectrum_frame` for the waveform frame; the `RIG_LEVEL_SPECTRUM_*` cases of
//! `icom_set_level` and `icom_get_level` for the setting payloads; the scope cases of
//! `icom_set_func` for on/off). Nothing here is verified against a radio.
//!
//! Layout, as Hamlib reads it (all multi-digit numbers are BCD):
//!
//! * `27 00 <rx> <div> <max> ...` is one waveform frame. `rx` is 0 (Main) or 1 (Sub). `div` and
//!   `max` are 1-byte BCD: this frame is number `div` of `max` for one sweep.
//! * Frame 1 carries `<mode> <edge1: 5 bytes, least significant pair first> <edge2: 5 bytes>
//!   <out-of-range>` and, when `max` is 1, the samples that follow. In centre mode edge1 is the
//!   centre frequency and edge2 the half-span; in the fixed modes they are the lower and upper
//!   edge. When `max` is greater than 1 frame 1 carries no samples and frames 2 to `max` carry
//!   them, `samples_per_frame` each, one byte per pixel.
//! * Settings: `27 10 <on>` scope on or off, `27 11 <on>` waveform output, `27 14 <rx> <mode>`,
//!   `27 15 <rx> <half-span: 5 bytes>`, `27 17 <rx> <on>` hold, `27 19 <rx> <2 bytes BCD
//!   big-endian, hundredths of a dB> <sign>`, `27 1A <rx> <speed>`, `27 1E <range> <edge no.>
//!   <low: 5 bytes> <high: 5 bytes>` (fixed edges). Reading is the same command without the value.
//!
//! The pieces are pure functions and a reassembler. `CivEngine` reaches them from the core API's
//! scope operations (`Op::SetScopeSpan` and the others) and returns each complete sweep as
//! `Inbound::ScopeFrame`.

use radioctl_core::description::ScopeParams;
use radioctl_core::scope::ScopeAxis;
use radioctl_core::units::Hz;

use crate::civ::{decode_bcd_freq, encode_bcd_freq};

/// The CI-V command number of the scope family.
pub const SCOPE_CMD: u8 = 0x27;
/// Sub-command: one waveform frame.
pub const SUB_WAVEFORM: u8 = 0x00;
/// Sub-command: scope display on or off.
pub const SUB_DISPLAY: u8 = 0x10;
/// Sub-command: waveform data output on or off.
pub const SUB_OUTPUT: u8 = 0x11;
/// Sub-command: centre, fixed or scroll mode.
pub const SUB_MODE: u8 = 0x14;
/// Sub-command: span.
pub const SUB_SPAN: u8 = 0x15;
/// Sub-command: hold.
pub const SUB_HOLD: u8 = 0x17;
/// Sub-command: reference level.
pub const SUB_REFERENCE: u8 = 0x19;
/// Sub-command: sweep speed.
pub const SUB_SPEED: u8 = 0x1A;
/// Sub-command: fixed-mode edge frequencies.
pub const SUB_EDGES: u8 = 0x1E;

/// How the scope's frequency axis is anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeMode {
    /// Centred on the receive frequency.
    Center,
    /// Between fixed edges.
    Fixed,
    /// Scrolling, centred.
    ScrollCenter,
    /// Scrolling, fixed edges.
    ScrollFixed,
}

impl ScopeMode {
    // [@ANCHOR: radioctl_scope_mode_code]
    /// The wire code of the mode.
    pub fn code(self) -> u8 {
        match self {
            ScopeMode::Center => 0,
            ScopeMode::Fixed => 1,
            ScopeMode::ScrollCenter => 2,
            ScopeMode::ScrollFixed => 3,
        }
    }

    // [@ANCHOR: radioctl_scope_mode_from_code]
    /// The mode for a wire code.
    pub fn from_code(code: u8) -> Option<ScopeMode> {
        match code {
            0 => Some(ScopeMode::Center),
            1 => Some(ScopeMode::Fixed),
            2 => Some(ScopeMode::ScrollCenter),
            3 => Some(ScopeMode::ScrollFixed),
            _ => None,
        }
    }
}

impl From<ScopeMode> for ScopeAxis {
    // [@ANCHOR: radioctl_scope_axis_from_mode]
    fn from(mode: ScopeMode) -> ScopeAxis {
        match mode {
            ScopeMode::Center => ScopeAxis::Center,
            ScopeMode::Fixed => ScopeAxis::Fixed,
            ScopeMode::ScrollCenter => ScopeAxis::ScrollCenter,
            ScopeMode::ScrollFixed => ScopeAxis::ScrollFixed,
        }
    }
}

impl From<ScopeAxis> for ScopeMode {
    // [@ANCHOR: radioctl_scope_mode_from_axis]
    fn from(axis: ScopeAxis) -> ScopeMode {
        match axis {
            ScopeAxis::Center => ScopeMode::Center,
            ScopeAxis::Fixed => ScopeMode::Fixed,
            ScopeAxis::ScrollCenter => ScopeMode::ScrollCenter,
            ScopeAxis::ScrollFixed => ScopeMode::ScrollFixed,
        }
    }
}

/// Sweep speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeSpeed {
    /// Fast sweep.
    Fast,
    /// Medium sweep.
    Mid,
    /// Slow sweep.
    Slow,
}

impl ScopeSpeed {
    // [@ANCHOR: radioctl_scope_speed_code]
    /// The wire code of the speed.
    pub fn code(self) -> u8 {
        match self {
            ScopeSpeed::Fast => 0,
            ScopeSpeed::Mid => 1,
            ScopeSpeed::Slow => 2,
        }
    }

    // [@ANCHOR: radioctl_scope_speed_from_code]
    /// The speed for a wire code.
    pub fn from_code(code: u8) -> Option<ScopeSpeed> {
        match code {
            0 => Some(ScopeSpeed::Fast),
            1 => Some(ScopeSpeed::Mid),
            2 => Some(ScopeSpeed::Slow),
            _ => None,
        }
    }
}

/// A scope setting or query. `receiver` is 0 (Main) or 1 (Sub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeCommand {
    /// Scope display on or off.
    SetDisplay(bool),
    /// Read the display state.
    GetDisplay,
    /// Waveform data output on or off (the rig then sends sweeps on its own).
    SetOutput(bool),
    /// Read the output state.
    GetOutput,
    /// Centre or fixed mode.
    SetMode(u8, ScopeMode),
    /// Read the mode.
    GetMode(u8),
    /// Full span in hertz (sent as the half-span the rig expects).
    SetSpan(u8, Hz),
    /// Read the span.
    GetSpan(u8),
    /// Hold on or off.
    SetHold(u8, bool),
    /// Reference level in hundredths of a dB.
    SetReference(u8, i16),
    /// Read the reference level.
    GetReference(u8),
    /// Sweep speed.
    SetSpeed(u8, ScopeSpeed),
    /// Read the sweep speed.
    GetSpeed(u8),
    /// Fixed-mode edges: range number, edge number (1 based), lower and upper edge.
    SetEdges(u8, u8, Hz, Hz),
}

impl ScopeCommand {
    // [@ANCHOR: radioctl_scope_command_is_query]
    /// True for the commands that read a setting (the rig answers with data, not OK).
    pub fn is_query(self) -> bool {
        matches!(
            self,
            ScopeCommand::GetDisplay
                | ScopeCommand::GetOutput
                | ScopeCommand::GetMode(_)
                | ScopeCommand::GetSpan(_)
                | ScopeCommand::GetReference(_)
                | ScopeCommand::GetSpeed(_)
        )
    }
}

/// The 1-byte BCD form of 0 to 99.
fn bcd_byte(value: u8) -> Option<u8> {
    (value <= 99).then_some(((value / 10) << 4) | (value % 10))
}

/// The number in a 1-byte BCD, or `None` if a nibble is not a digit.
fn from_bcd_byte(byte: u8) -> Option<u8> {
    let (high, low) = (byte >> 4, byte & 15);
    (high <= 9 && low <= 9).then_some(high * 10 + low)
}

// [@ANCHOR: radioctl_scope_command_data]
/// The sub-command and data bytes (after the `27`) for a scope command, or `None` when a value
/// does not fit its field.
pub fn command_data(command: ScopeCommand) -> Option<Vec<u8>> {
    let rx_ok = |rx: u8| (rx <= 1).then_some(rx);
    Some(match command {
        ScopeCommand::SetDisplay(on) => vec![SUB_DISPLAY, u8::from(on)],
        ScopeCommand::GetDisplay => vec![SUB_DISPLAY],
        ScopeCommand::SetOutput(on) => vec![SUB_OUTPUT, u8::from(on)],
        ScopeCommand::GetOutput => vec![SUB_OUTPUT],
        ScopeCommand::SetMode(rx, mode) => vec![SUB_MODE, rx_ok(rx)?, mode.code()],
        ScopeCommand::GetMode(rx) => vec![SUB_MODE, rx_ok(rx)?],
        ScopeCommand::SetSpan(rx, span) => {
            let mut out = vec![SUB_SPAN, rx_ok(rx)?];
            out.extend(encode_bcd_freq(Hz(span.0 / 2))?);
            out
        }
        ScopeCommand::GetSpan(rx) => vec![SUB_SPAN, rx_ok(rx)?],
        ScopeCommand::SetHold(rx, on) => vec![SUB_HOLD, rx_ok(rx)?, u8::from(on)],
        ScopeCommand::SetReference(rx, centi_db) => {
            let magnitude = centi_db.unsigned_abs();
            if magnitude > 9999 {
                return None;
            }
            let high = bcd_byte((magnitude / 100) as u8)?;
            let low = bcd_byte((magnitude % 100) as u8)?;
            vec![SUB_REFERENCE, rx_ok(rx)?, high, low, u8::from(centi_db < 0)]
        }
        ScopeCommand::GetReference(rx) => vec![SUB_REFERENCE, rx_ok(rx)?],
        ScopeCommand::SetSpeed(rx, speed) => vec![SUB_SPEED, rx_ok(rx)?, speed.code()],
        ScopeCommand::GetSpeed(rx) => vec![SUB_SPEED, rx_ok(rx)?],
        ScopeCommand::SetEdges(range, edge, low, high) => {
            let mut out = vec![SUB_EDGES, bcd_byte(range)?, bcd_byte(edge)?];
            out.extend(encode_bcd_freq(low)?);
            out.extend(encode_bcd_freq(high)?);
            out
        }
    })
}

// [@ANCHOR: radioctl_scope_decode_reference]
/// The reference level, in hundredths of a dB, from the 3 data bytes after `27 19 <rx>`.
pub fn decode_reference(bytes: &[u8]) -> Option<i16> {
    let [high, low, sign] = bytes else {
        return None;
    };
    let magnitude = i16::from(from_bcd_byte(*high)?) * 100 + i16::from(from_bcd_byte(*low)?);
    Some(if *sign == 0 { magnitude } else { -magnitude })
}

/// One waveform frame, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeSegment {
    /// 0 (Main) or 1 (Sub).
    pub receiver: u8,
    /// Frame number, from 1.
    pub division: u8,
    /// Number of frames in the sweep.
    pub divisions: u8,
    /// Present in frame 1 only.
    pub header: Option<ScopeHeader>,
    /// The amplitude samples this frame carries.
    pub samples: Vec<u8>,
}

/// The first frame's description of the sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeHeader {
    /// How the axis is anchored.
    pub mode: ScopeMode,
    /// Lower edge, or centre frequency in the centred modes.
    pub edge_a: Hz,
    /// Upper edge, or half-span in the centred modes.
    pub edge_b: Hz,
    /// True when the receive frequency is outside a fixed scope.
    pub out_of_range: bool,
}

// [@ANCHOR: radioctl_scope_parse_segment]
/// Decodes the data after `27 00` (receiver, frame number, frame count, then the rest).
/// `None` for a malformed frame.
pub fn parse_segment(data: &[u8]) -> Option<ScopeSegment> {
    let (receiver, division, divisions, rest) = match data {
        [rx, div, max, rest @ ..] => (*rx, from_bcd_byte(*div)?, from_bcd_byte(*max)?, rest),
        _ => return None,
    };
    if receiver > 1 || division == 0 || division > divisions {
        return None;
    }
    if division != 1 {
        return Some(ScopeSegment {
            receiver,
            division,
            divisions,
            header: None,
            samples: rest.to_vec(),
        });
    }
    let [mode, a0, a1, a2, a3, a4, b0, b1, b2, b3, b4, out_of_range, samples @ ..] = rest else {
        return None;
    };
    let header = ScopeHeader {
        mode: ScopeMode::from_code(*mode)?,
        edge_a: decode_bcd_freq(&[*a0, *a1, *a2, *a3, *a4])?,
        edge_b: decode_bcd_freq(&[*b0, *b1, *b2, *b3, *b4])?,
        out_of_range: *out_of_range != 0,
    };
    Some(ScopeSegment {
        receiver,
        division,
        divisions,
        header: Some(header),
        samples: samples.to_vec(),
    })
}

/// One complete sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeLine {
    /// 0 (Main) or 1 (Sub).
    pub receiver: u8,
    /// How the axis was anchored.
    pub mode: ScopeMode,
    /// Lowest frequency on the axis.
    pub low: Hz,
    /// Highest frequency on the axis.
    pub high: Hz,
    /// True when the receive frequency is outside a fixed scope.
    pub out_of_range: bool,
    /// One amplitude per pixel, 0 to the model's `amplitude_max`.
    pub samples: Vec<u8>,
}

/// Rebuilds sweeps from waveform frames, one receiver at a time. A frame out of order discards the
/// sweep in progress; a sweep with the wrong number of samples is never delivered.
#[derive(Debug, Clone)]
pub struct ScopeAssembler {
    params: ScopeParams,
    current: [Option<Partial>; 2],
    dropped: u64,
}

#[derive(Debug, Clone)]
struct Partial {
    header: ScopeHeader,
    next_division: u8,
    samples: Vec<u8>,
}

impl ScopeAssembler {
    // [@ANCHOR: radioctl_scope_assembler_new]
    /// An assembler for a model's scope.
    pub fn new(params: ScopeParams) -> ScopeAssembler {
        ScopeAssembler {
            params,
            current: [None, None],
            dropped: 0,
        }
    }

    // [@ANCHOR: radioctl_scope_assembler_dropped]
    /// How many frames or sweeps were thrown away as out of order, oversized or short.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    // [@ANCHOR: radioctl_scope_assembler_note_malformed]
    /// Counts a waveform frame that could not be decoded.
    pub fn note_malformed(&mut self) {
        self.dropped += 1;
    }

    // [@ANCHOR: radioctl_scope_assembler_push]
    /// Adds a frame; returns the sweep it completes, if any.
    pub fn push(&mut self, segment: ScopeSegment) -> Option<ScopeLine> {
        let slot = usize::from(segment.receiver);
        if segment.receiver >= self.params.receivers {
            self.dropped += 1;
            return None;
        }
        if segment.division == 1 {
            let header = segment.header?;
            self.current[slot] = Some(Partial {
                header,
                next_division: 2,
                samples: segment.samples,
            });
        } else {
            let in_order = self.current[slot]
                .as_ref()
                .is_some_and(|p| p.next_division == segment.division);
            if !in_order {
                self.current[slot] = None;
                self.dropped += 1;
                return None;
            }
            if let Some(partial) = self.current[slot].as_mut() {
                partial.samples.extend(segment.samples);
                partial.next_division += 1;
            }
        }
        let too_long = self.current[slot]
            .as_ref()
            .is_some_and(|p| p.samples.len() > usize::from(self.params.line_length));
        if too_long {
            self.current[slot] = None;
            self.dropped += 1;
            return None;
        }
        if segment.division < segment.divisions {
            return None;
        }
        let partial = self.current[slot].take()?;
        if partial.samples.len() != usize::from(self.params.line_length) {
            self.dropped += 1;
            return None;
        }
        Some(line_from(segment.receiver, partial))
    }
}

// [@ANCHOR: radioctl_scope_line_from]
fn line_from(receiver: u8, partial: Partial) -> ScopeLine {
    let header = partial.header;
    let (low, high) = match header.mode {
        ScopeMode::Center | ScopeMode::ScrollCenter => (
            Hz(header.edge_a.0.saturating_sub(header.edge_b.0)),
            Hz(header.edge_a.0.saturating_add(header.edge_b.0)),
        ),
        ScopeMode::Fixed | ScopeMode::ScrollFixed => (header.edge_a, header.edge_b),
    };
    ScopeLine {
        receiver,
        mode: header.mode,
        low,
        high,
        out_of_range: header.out_of_range,
        samples: partial.samples,
    }
}

/// A virtual scope's answer to a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeAnswer {
    /// Accepted; the CI-V OK frame follows.
    Ok,
    /// A reply: send as `27 <data>`.
    Data(Vec<u8>),
    /// Refused; the CI-V NG frame follows.
    Refused,
}

/// State of one receiver's scope in a virtual rig.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SimScope {
    mode: ScopeMode,
    half_span: Hz,
    hold: bool,
    reference_centi_db: i16,
    speed: ScopeSpeed,
}

/// The scope of a virtual rig: settings per receiver and a synthetic sweep generator.
#[derive(Debug, Clone)]
pub struct ScopeSim {
    params: ScopeParams,
    display: bool,
    output: bool,
    receivers: Vec<SimScope>,
    /// The last fixed-mode edges set (any range and edge number): the sweep of a fixed scope
    /// uses them instead of the tuned frequency's neighbourhood.
    edges: Option<(Hz, Hz)>,
    sweeps: u32,
}

impl ScopeSim {
    // [@ANCHOR: radioctl_scope_sim_new]
    /// A scope in its power-on state: display on, output off, centred, narrowest span.
    pub fn new(params: &ScopeParams) -> ScopeSim {
        let narrowest = params.spans_hz.iter().copied().min().unwrap_or(10_000);
        let one = SimScope {
            mode: ScopeMode::Center,
            half_span: Hz(narrowest / 2),
            hold: false,
            reference_centi_db: 0,
            speed: ScopeSpeed::Mid,
        };
        ScopeSim {
            params: params.clone(),
            display: true,
            output: false,
            receivers: vec![one; usize::from(params.receivers)],
            edges: None,
            sweeps: 0,
        }
    }

    // [@ANCHOR: radioctl_scope_sim_output_on]
    /// True when waveform output is switched on.
    pub fn output_on(&self) -> bool {
        self.output
    }

    // [@ANCHOR: radioctl_scope_sim_handle]
    /// Answers a scope command (the bytes after `27`).
    pub fn handle(&mut self, data: &[u8]) -> ScopeAnswer {
        match self.try_handle(data) {
            Ok(Some(reply)) => ScopeAnswer::Data(reply),
            Ok(None) => ScopeAnswer::Ok,
            Err(()) => ScopeAnswer::Refused,
        }
    }

    // [@ANCHOR: radioctl_scope_sim_try_handle]
    fn try_handle(&mut self, data: &[u8]) -> Result<Option<Vec<u8>>, ()> {
        let (sub, rest) = data.split_first().ok_or(())?;
        let rx_of = |byte: Option<&u8>| -> Result<usize, ()> {
            let rx = usize::from(*byte.ok_or(())?);
            (rx < self.receivers.len()).then_some(rx).ok_or(())
        };
        match (*sub, rest) {
            (SUB_DISPLAY, []) => Ok(Some(vec![SUB_DISPLAY, u8::from(self.display)])),
            (SUB_DISPLAY, [on]) => {
                self.display = *on != 0;
                Ok(None)
            }
            (SUB_OUTPUT, []) => Ok(Some(vec![SUB_OUTPUT, u8::from(self.output)])),
            (SUB_OUTPUT, [on]) => {
                self.output = *on != 0;
                Ok(None)
            }
            (SUB_MODE, [rx]) => {
                let at = rx_of(Some(rx))?;
                Ok(Some(vec![SUB_MODE, *rx, self.receivers[at].mode.code()]))
            }
            (SUB_MODE, [rx, code]) => {
                let at = rx_of(Some(rx))?;
                self.receivers[at].mode = ScopeMode::from_code(*code).ok_or(())?;
                Ok(None)
            }
            (SUB_SPAN, [rx]) => {
                let at = rx_of(Some(rx))?;
                let bcd = encode_bcd_freq(self.receivers[at].half_span).ok_or(())?;
                let mut out = vec![SUB_SPAN, *rx];
                out.extend(bcd);
                Ok(Some(out))
            }
            (SUB_SPAN, [rx, bcd @ ..]) if bcd.len() == 5 => {
                let at = rx_of(Some(rx))?;
                self.receivers[at].half_span = decode_bcd_freq(bcd).ok_or(())?;
                Ok(None)
            }
            (SUB_EDGES, [_range, _edge, bcd @ ..]) if bcd.len() == 10 => {
                let low = decode_bcd_freq(bcd.get(..5).ok_or(())?).ok_or(())?;
                let high = decode_bcd_freq(bcd.get(5..).ok_or(())?).ok_or(())?;
                if low.0 >= high.0 {
                    return Err(());
                }
                self.edges = Some((low, high));
                Ok(None)
            }
            (SUB_HOLD, [rx, on]) => {
                let at = rx_of(Some(rx))?;
                self.receivers[at].hold = *on != 0;
                Ok(None)
            }
            (SUB_REFERENCE, [rx]) => {
                let at = rx_of(Some(rx))?;
                let cmd = command_data(ScopeCommand::SetReference(
                    *rx,
                    self.receivers[at].reference_centi_db,
                ))
                .ok_or(())?;
                Ok(Some(cmd))
            }
            (SUB_REFERENCE, [rx, bcd @ ..]) if bcd.len() == 3 => {
                let at = rx_of(Some(rx))?;
                self.receivers[at].reference_centi_db = decode_reference(bcd).ok_or(())?;
                Ok(None)
            }
            (SUB_SPEED, [rx]) => {
                let at = rx_of(Some(rx))?;
                Ok(Some(vec![SUB_SPEED, *rx, self.receivers[at].speed.code()]))
            }
            (SUB_SPEED, [rx, code]) => {
                let at = rx_of(Some(rx))?;
                self.receivers[at].speed = ScopeSpeed::from_code(*code).ok_or(())?;
                Ok(None)
            }
            _ => Err(()),
        }
    }

    // [@ANCHOR: radioctl_scope_sim_sweep]
    /// The frames (each `27 00 ...` data, without the CI-V envelope) of one sweep of a receiver
    /// whose tuned frequency is `tuned`, or none when output is off, the receiver does not exist
    /// or hold is on. Samples are a deterministic ramp that moves each sweep, so a reassembled
    /// line can be checked sample by sample.
    pub fn sweep(&mut self, receiver: u8, tuned: Hz) -> Vec<Vec<u8>> {
        let at = usize::from(receiver);
        let Some(scope) = self.receivers.get(at) else {
            return Vec::new();
        };
        if !self.output || scope.hold {
            return Vec::new();
        }
        self.sweeps = self.sweeps.wrapping_add(1);
        let mode = scope.mode;
        let (edge_a, edge_b) = match mode {
            ScopeMode::Center | ScopeMode::ScrollCenter => (tuned, scope.half_span),
            ScopeMode::Fixed | ScopeMode::ScrollFixed => self.edges.unwrap_or((
                Hz(tuned.0.saturating_sub(scope.half_span.0)),
                Hz(tuned.0.saturating_add(scope.half_span.0)),
            )),
        };
        let per_frame = usize::from(self.params.samples_per_frame);
        let line = usize::from(self.params.line_length);
        let samples: Vec<u8> = (0..line)
            .map(|i| sample_value(i, self.sweeps, self.params.amplitude_max))
            .collect();
        let data_frames = line.div_ceil(per_frame);
        let divisions = if data_frames > 1 { data_frames + 1 } else { 1 };
        let head = |division: u8| -> Option<Vec<u8>> {
            Some(vec![
                receiver,
                bcd_byte(division)?,
                bcd_byte(u8::try_from(divisions).ok()?)?,
            ])
        };
        let mut frames = Vec::new();
        let first = head(1).and_then(|mut frame| {
            frame.push(mode.code());
            frame.extend(encode_bcd_freq(edge_a)?);
            frame.extend(encode_bcd_freq(edge_b)?);
            frame.push(0);
            Some(frame)
        });
        let Some(mut first) = first else {
            return Vec::new();
        };
        if divisions == 1 {
            first.extend(&samples);
            return vec![first];
        }
        frames.push(first);
        for (index, chunk) in samples.chunks(per_frame).enumerate() {
            let division = u8::try_from(index + 2).unwrap_or(u8::MAX);
            if let Some(mut frame) = head(division) {
                frame.extend(chunk);
                frames.push(frame);
            }
        }
        frames
    }
}

// [@ANCHOR: radioctl_scope_sample_value]
/// The deterministic synthetic amplitude of pixel `index` in sweep `sweep`.
pub fn sample_value(index: usize, sweep: u32, amplitude_max: u16) -> u8 {
    let span = u32::from(amplitude_max) + 1;
    let value = (index as u32).wrapping_mul(7).wrapping_add(sweep) % span;
    u8::try_from(value).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> ScopeParams {
        ScopeParams {
            receivers: 1,
            line_length: 475,
            samples_per_frame: 50,
            amplitude_max: 160,
            spans_hz: vec![5_000, 10_000, 20_000],
        }
    }

    // Tests [@ANCHOR: radioctl_scope_mode_code]
    // Tests [@ANCHOR: radioctl_scope_mode_from_code]
    // Tests [@ANCHOR: radioctl_scope_speed_code]
    // Tests [@ANCHOR: radioctl_scope_speed_from_code]
    #[test]
    fn codes_round_trip_and_unknown_codes_are_refused() {
        for mode in [
            ScopeMode::Center,
            ScopeMode::Fixed,
            ScopeMode::ScrollCenter,
            ScopeMode::ScrollFixed,
        ] {
            assert_eq!(ScopeMode::from_code(mode.code()), Some(mode));
        }
        for speed in [ScopeSpeed::Fast, ScopeSpeed::Mid, ScopeSpeed::Slow] {
            assert_eq!(ScopeSpeed::from_code(speed.code()), Some(speed));
        }
        assert_eq!(ScopeMode::from_code(4), None);
        assert_eq!(ScopeSpeed::from_code(3), None);
    }

    // Tests [@ANCHOR: radioctl_scope_command_is_query]
    #[test]
    fn queries_are_told_from_settings() {
        assert!(ScopeCommand::GetSpan(0).is_query());
        assert!(ScopeCommand::GetDisplay.is_query());
        assert!(!ScopeCommand::SetSpan(0, Hz(10_000)).is_query());
        assert!(!ScopeCommand::SetHold(0, true).is_query());
    }

    // Tests [@ANCHOR: radioctl_scope_command_data]
    // Tests [@ANCHOR: radioctl_scope_decode_reference]
    #[test]
    fn commands_have_the_documented_layout() {
        assert_eq!(
            command_data(ScopeCommand::SetDisplay(true)),
            Some(vec![0x10, 1])
        );
        assert_eq!(command_data(ScopeCommand::GetOutput), Some(vec![0x11]));
        assert_eq!(
            command_data(ScopeCommand::SetMode(0, ScopeMode::Fixed)),
            Some(vec![0x14, 0, 1])
        );
        assert_eq!(
            command_data(ScopeCommand::SetSpan(0, Hz(50_000))),
            Some(vec![0x15, 0, 0x00, 0x50, 0x02, 0x00, 0x00])
        );
        assert_eq!(
            command_data(ScopeCommand::SetReference(1, -1050)),
            Some(vec![0x19, 1, 0x10, 0x50, 1])
        );
        assert_eq!(decode_reference(&[0x10, 0x50, 1]), Some(-1050));
        assert_eq!(decode_reference(&[0x20, 0x00, 0]), Some(2000));
        assert_eq!(decode_reference(&[0x2A, 0x00, 0]), None);
        assert_eq!(decode_reference(&[0x20, 0x00]), None);
        assert_eq!(
            command_data(ScopeCommand::SetEdges(3, 2, Hz(7_000_000), Hz(7_300_000))),
            Some(vec![
                0x1E, 0x03, 0x02, 0x00, 0x00, 0x00, 0x07, 0x00, 0x00, 0x00, 0x30, 0x07, 0x00
            ])
        );
        assert_eq!(command_data(ScopeCommand::GetSpan(2)), None);
        assert_eq!(command_data(ScopeCommand::SetReference(0, 10_000)), None);
    }

    // Tests [@ANCHOR: radioctl_scope_parse_segment]
    #[test]
    fn a_header_frame_and_a_data_frame_parse_and_garbage_does_not() {
        let mut header = vec![0x00, 0x01, 0x11, 0x00];
        header.extend([0x00, 0x40, 0x07, 0x14, 0x00]);
        header.extend([0x00, 0x50, 0x02, 0x00, 0x00]);
        header.push(0x01);
        let segment = parse_segment(&header).expect("header");
        assert_eq!(
            (segment.receiver, segment.division, segment.divisions),
            (0, 1, 11)
        );
        let head = segment.header.expect("header block");
        assert_eq!(head.mode, ScopeMode::Center);
        assert_eq!(head.edge_a, Hz(14_074_000));
        assert_eq!(head.edge_b, Hz(25_000));
        assert!(head.out_of_range);
        assert!(segment.samples.is_empty());
        let data = parse_segment(&[0x00, 0x02, 0x11, 9, 8, 7]).expect("data");
        assert_eq!(data.samples, vec![9, 8, 7]);
        assert!(data.header.is_none());
        assert_eq!(parse_segment(&[0x00, 0x01]), None);
        assert_eq!(parse_segment(&[0x02, 0x01, 0x01, 0, 0]), None);
        assert_eq!(parse_segment(&[0x00, 0x03, 0x02, 1]), None);
        assert_eq!(
            parse_segment(&[0x00, 0x01, 0x02, 0x09, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
            None
        );
    }

    // Tests [@ANCHOR: radioctl_scope_sim_new]
    // Tests [@ANCHOR: radioctl_scope_sim_output_on]
    // Tests [@ANCHOR: radioctl_scope_sim_handle]
    // Tests [@ANCHOR: radioctl_scope_sim_try_handle]
    #[test]
    fn the_simulated_scope_keeps_its_settings() {
        let mut sim = ScopeSim::new(&params());
        assert!(!sim.output_on());
        assert_eq!(sim.handle(&[0x11, 1]), ScopeAnswer::Ok);
        assert!(sim.output_on());
        assert_eq!(sim.handle(&[0x11]), ScopeAnswer::Data(vec![0x11, 1]));
        assert_eq!(
            sim.handle(&[0x15, 0]),
            ScopeAnswer::Data(vec![0x15, 0, 0x00, 0x25, 0x00, 0x00, 0x00])
        );
        assert_eq!(
            sim.handle(&[0x15, 0, 0x00, 0x00, 0x05, 0x00, 0x00]),
            ScopeAnswer::Ok
        );
        assert_eq!(sim.handle(&[0x14, 0, 3]), ScopeAnswer::Ok);
        assert_eq!(sim.handle(&[0x14, 0]), ScopeAnswer::Data(vec![0x14, 0, 3]));
        assert_eq!(sim.handle(&[0x19, 0, 0x20, 0x00, 0]), ScopeAnswer::Ok);
        assert_eq!(
            sim.handle(&[0x19, 0]),
            ScopeAnswer::Data(vec![0x19, 0, 0x20, 0x00, 0])
        );
        assert_eq!(sim.handle(&[0x1A, 0, 2]), ScopeAnswer::Ok);
        assert_eq!(sim.handle(&[0x1A, 0]), ScopeAnswer::Data(vec![0x1A, 0, 2]));
        assert_eq!(sim.handle(&[0x17, 0, 1]), ScopeAnswer::Ok);
        assert_eq!(sim.handle(&[0x14, 1]), ScopeAnswer::Refused);
        assert_eq!(sim.handle(&[0x14, 0, 9]), ScopeAnswer::Refused);
        assert_eq!(sim.handle(&[0x77]), ScopeAnswer::Refused);
        assert_eq!(sim.handle(&[]), ScopeAnswer::Refused);
    }

    // Tests [@ANCHOR: radioctl_scope_sim_sweep]
    // Tests [@ANCHOR: radioctl_scope_sample_value]
    // Tests [@ANCHOR: radioctl_scope_assembler_new]
    // Tests [@ANCHOR: radioctl_scope_assembler_push]
    // Tests [@ANCHOR: radioctl_scope_assembler_dropped]
    // Tests [@ANCHOR: radioctl_scope_line_from]
    #[test]
    fn a_simulated_sweep_reassembles_into_the_line_the_rig_drew() {
        let mut sim = ScopeSim::new(&params());
        assert!(sim.sweep(0, Hz(14_074_000)).is_empty(), "output is off");
        assert_eq!(sim.handle(&[0x11, 1]), ScopeAnswer::Ok);
        let frames = sim.sweep(0, Hz(14_074_000));
        assert_eq!(
            frames.len(),
            11,
            "one header and ten data frames of 50 (the last of 25)"
        );
        let mut assembler = ScopeAssembler::new(params());
        let mut done = None;
        for frame in &frames {
            done = assembler.push(parse_segment(frame).expect("frame"));
        }
        let line = done.expect("complete line");
        assert_eq!(
            (line.low, line.high),
            (Hz(14_074_000 - 2_500), Hz(14_074_000 + 2_500))
        );
        assert_eq!(line.samples.len(), 475);
        for (index, sample) in line.samples.iter().enumerate() {
            assert_eq!(*sample, sample_value(index, 1, 160));
        }
        assert_eq!(assembler.dropped(), 0);
        assert_eq!(sim.handle(&[0x17, 0, 1]), ScopeAnswer::Ok);
        assert!(sim.sweep(0, Hz(14_074_000)).is_empty(), "held");
        assert!(sim.sweep(1, Hz(14_074_000)).is_empty(), "no Sub scope");
    }

    // Tests [@ANCHOR: radioctl_scope_assembler_push]
    // Tests [@ANCHOR: radioctl_scope_assembler_dropped]
    // Tests [@ANCHOR: radioctl_scope_assembler_note_malformed]
    #[test]
    fn out_of_order_oversized_and_short_sweeps_are_never_delivered() {
        let mut sim = ScopeSim::new(&params());
        assert_eq!(sim.handle(&[0x11, 1]), ScopeAnswer::Ok);
        let frames = sim.sweep(0, Hz(7_100_000));
        let segments: Vec<ScopeSegment> = frames
            .iter()
            .map(|f| parse_segment(f).expect("frame"))
            .collect();
        let mut assembler = ScopeAssembler::new(params());
        assert!(assembler.push(segments[0].clone()).is_none());
        assert!(
            assembler.push(segments[2].clone()).is_none(),
            "frame 2 skipped"
        );
        assert_eq!(assembler.dropped(), 1);
        for segment in &segments[3..] {
            assert!(assembler.push(segment.clone()).is_none());
        }
        let mut short = ScopeAssembler::new(params());
        for segment in &segments[..segments.len() - 1] {
            short.push(segment.clone());
        }
        let mut last = segments[segments.len() - 1].clone();
        last.samples.truncate(10);
        assert!(short.push(last).is_none());
        assert!(short.dropped() >= 1);
        let mut oversized = ScopeAssembler::new(params());
        let mut big = segments[0].clone();
        big.samples = vec![0; 500];
        assert!(oversized.push(big).is_none());
        assert_eq!(oversized.dropped(), 1);
        oversized.note_malformed();
        assert_eq!(oversized.dropped(), 2);
        let mut wrong_rx = segments[0].clone();
        wrong_rx.receiver = 1;
        assert!(ScopeAssembler::new(params()).push(wrong_rx).is_none());
    }
}
