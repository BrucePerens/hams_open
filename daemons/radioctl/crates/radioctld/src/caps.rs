// SPDX-License-Identifier: AGPL-3.0-or-later
//! `\dump_state` and `\dump_caps`: what a rig can do, generated from its description. The layout
//! of `\dump_state` is the one Hamlib 4.6.2's `rigctld` prints (the order of the lines and the
//! `key=value` tail were observed on its output, and the differential test reads the result back
//! through Hamlib's own `rigctl -m 2` client).
//!
//! Fail closed: a transmit range is advertised only when the rig's transmit gate was opened by
//! the configuration, so a client that plans from the ranges never plans to transmit on a rig
//! that would refuse.

use radioctl_core::capability::Capability;
use radioctl_core::description::RigDescription;
use radioctl_core::units::Mode;

use crate::names::{
    func_mask, level_mask, mode_bit, mode_mask, nominal_width, served_levels, LevelInfo,
    LevelSource,
};

/// Everything the text needs besides the description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapsView<'a> {
    /// The rig.
    pub description: &'a RigDescription,
    /// The model number to report.
    pub hamlib_model: u32,
    /// Lowest frequency covered, in hertz.
    pub rx_low_hz: u64,
    /// Highest frequency covered, in hertz.
    pub rx_high_hz: u64,
    /// Maximum transmit power, in milliwatts.
    pub tx_power_mw: u32,
    /// True when the rig's transmit gate is open.
    pub transmit_enabled: bool,
}

/// The largest RIT or XIT offset advertised, in hertz.
const MAX_RIT_HZ: u32 = 9999;
/// How many entries `level_gran` and `parm_gran` carry.
const GRAN_ENTRIES: u32 = 64;

// [@ANCHOR: radioctld_caps_modes]
/// The modes of the description, in a stable order.
fn modes(description: &RigDescription) -> Vec<Mode> {
    let mut modes: Vec<Mode> = description.mode_codes.iter().map(|(m, _)| *m).collect();
    modes.sort();
    modes.dedup();
    modes
}

// [@ANCHOR: radioctld_caps_range_line]
/// One frequency range line of `\dump_state`.
fn range_line(low: u64, high: u64, modes: u64, power: (i64, i64), vfos: u32) -> String {
    format!(
        "{low}.000000 {high}.000000 {modes:#x} {} {} {vfos:#x} 0x1\n",
        power.0, power.1
    )
}

// [@ANCHOR: radioctld_caps_gran]
/// The `level_gran` text: min, max and step of every level, zeros for the ones not served.
fn level_gran(levels: &[LevelInfo], description: &RigDescription) -> String {
    let mut out = String::new();
    for index in 0..GRAN_ENTRIES {
        let entry = levels.iter().find(|l| l.bit == index);
        let (min, max, step) = match entry {
            Some(info) => granularity(info, description),
            None => ("0".to_string(), "0".to_string(), "0".to_string()),
        };
        out.push_str(&format!("{index}={min},{max},{step};"));
    }
    out
}

// [@ANCHOR: radioctld_caps_wire_float]
/// A value as the `level_gran` text of Hamlib's `rigctld` writes it for the first levels (AF, RF):
/// the bit pattern of the 32-bit float, as a decimal integer. Observed: Hamlib's own client reads
/// `4=0,1065353216,998277249` as the range 0 to 1 in steps of 1/255.
fn wire_float_bits(value: f64) -> String {
    (value as f32).to_bits().to_string()
}

// [@ANCHOR: radioctld_caps_wire_int]
/// An integer level value as the text Hamlib's client expects for an integer level: the integer's
/// bit pattern read as a 32-bit float, in scientific notation (`8` is `1.1210388e-44`).
fn wire_int_as_float(value: u32) -> String {
    format!("{:e}", f32::from_bits(value))
}

// [@ANCHOR: radioctld_caps_granularity]
/// Min, max and step of one served level in the form the client parses for that level.
fn granularity(info: &LevelInfo, description: &RigDescription) -> (String, String, String) {
    let float = |min: f64, max: f64, step: f64| {
        if info.bit <= 4 {
            (
                wire_float_bits(min),
                wire_float_bits(max),
                wire_float_bits(step),
            )
        } else {
            (
                format!("{min:.6}"),
                format!("{max:.6}"),
                format!("{step:.6}"),
            )
        }
    };
    match info.source {
        LevelSource::Radio(level) if !info.fraction => {
            let (min, max) = crate::names::level_range(description, level).unwrap_or((0, 0));
            (
                wire_int_as_float(u32::from(min)),
                wire_int_as_float(u32::from(max)),
                wire_int_as_float(1),
            )
        }
        LevelSource::Radio(level) => {
            let span = crate::names::level_range(description, level)
                .map_or(1, |(min, max)| max.saturating_sub(min).max(1));
            float(0.0, 1.0, 1.0 / f64::from(span))
        }
        LevelSource::Swr => float(0.0, 10.0, 0.1),
        LevelSource::RawStr => (
            wire_int_as_float(0),
            wire_int_as_float(255),
            wire_int_as_float(1),
        ),
    }
}

// [@ANCHOR: radioctld_caps_dump_state]
/// The `\dump_state` text.
pub fn dump_state(view: &CapsView<'_>) -> String {
    let desc = view.description;
    let caps = &desc.capabilities;
    let mode_list = modes(desc);
    let all_modes = mode_mask(&mode_list);
    let vfos = if caps.has(Capability::SecondVfo) {
        0x3
    } else {
        0x1
    };
    let levels = served_levels(desc);
    let rit_xit = caps.has(Capability::RitXit);
    let max_rit = if rit_xit { MAX_RIT_HZ } else { 0 };
    let mut out = String::new();
    out.push_str(&format!("1\n{}\n0\n", view.hamlib_model));
    out.push_str(&range_line(
        view.rx_low_hz,
        view.rx_high_hz,
        all_modes,
        (-1, -1),
        vfos,
    ));
    out.push_str("0 0 0 0 0 0 0\n");
    if view.transmit_enabled && caps.has(Capability::SetPtt) {
        out.push_str(&range_line(
            view.rx_low_hz,
            view.rx_high_hz,
            all_modes,
            (1000, i64::from(view.tx_power_mw)),
            vfos,
        ));
    }
    out.push_str("0 0 0 0 0 0 0\n");
    out.push_str(&format!("{all_modes:#x} 1\n{all_modes:#x} 0\n0 0\n"));
    for mode in &mode_list {
        out.push_str(&format!(
            "{:#x} {}\n",
            mode_bit(*mode),
            nominal_width(*mode)
        ));
    }
    out.push_str("0 0\n");
    out.push_str(&format!("{max_rit}\n{max_rit}\n0\n0\n0\n0\n"));
    out.push_str(&format!(
        "{:#x}\n{:#x}\n",
        func_mask(rit_xit),
        func_mask(rit_xit)
    ));
    out.push_str(&format!(
        "{:#x}\n{:#x}\n0x0\n0x0\n",
        level_mask(&levels, false),
        level_mask(&levels, true)
    ));
    let has_freq = caps.has(Capability::ReadFrequency);
    let flag = |on: bool| u8::from(on);
    out.push_str("vfo_ops=0x0\n");
    out.push_str(&format!(
        "ptt_type={:#x}\n",
        flag(view.transmit_enabled && caps.has(Capability::SetPtt))
    ));
    out.push_str("targetable_vfo=0x0\nhas_set_vfo=1\nhas_get_vfo=1\n");
    out.push_str(&format!(
        "has_set_freq={}\nhas_get_freq={}\n",
        flag(caps.has(Capability::SetFrequency)),
        flag(has_freq)
    ));
    out.push_str("has_set_conf=0\nhas_get_conf=0\nhas_power2mW=0\nhas_mW2power=0\n");
    out.push_str("has_get_ant=0\nhas_set_ant=0\ntimeout=0\n");
    out.push_str(&format!("rig_model={}\n", view.hamlib_model));
    out.push_str(&format!(
        "rigctld_version=radioctld {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    out.push_str("agc_levels=\nctcss_list=\ndcs_list=\n");
    out.push_str(&format!("level_gran={}\n", level_gran(&levels, desc)));
    out.push_str(&format!("parm_gran={}\n", level_gran(&[], desc)));
    out.push_str(&format!("rig_model={}\n", view.hamlib_model));
    out.push_str(&format!(
        "hamlib_version=radioctld {} (rigctld text protocol)\ndone\n",
        env!("CARGO_PKG_VERSION")
    ));
    out
}

// [@ANCHOR: radioctld_caps_dump_caps]
/// The `\dump_caps` text: a readable summary, ended by a blank line.
pub fn dump_caps(view: &CapsView<'_>) -> String {
    let desc = view.description;
    let yes = |on: bool| if on { "Y" } else { "N" };
    let has = |c: Capability| desc.capabilities.has(c);
    let mut out = String::new();
    out.push_str(&format!("Caps dump for model: {}\n", view.hamlib_model));
    out.push_str(&format!(
        "Model name:\t{}\nMfg name:\tradioctl\n",
        desc.name
    ));
    out.push_str(&format!(
        "Hamlib version:\tradioctld {} (rigctld text protocol)\n",
        env!("CARGO_PKG_VERSION")
    ));
    out.push_str(&format!("Rig id:\t{}\n", desc.id));
    out.push_str(&format!(
        "Transmit gate:\t{}\n",
        if view.transmit_enabled {
            "open"
        } else {
            "closed"
        }
    ));
    out.push_str("Mode list:");
    for mode in modes(desc) {
        out.push_str(&format!(" {}", crate::names::mode_to_hamlib(mode)));
    }
    out.push('\n');
    out.push_str(&format!(
        "Can set Frequency:\t{}\nCan get Frequency:\t{}\n",
        yes(has(Capability::SetFrequency)),
        yes(has(Capability::ReadFrequency))
    ));
    out.push_str(&format!(
        "Can set Mode:\t{}\nCan get Mode:\t{}\n",
        yes(has(Capability::SetMode)),
        yes(has(Capability::ReadMode))
    ));
    out.push_str(&format!(
        "Can set PTT:\t{}\nCan get PTT:\t{}\n",
        yes(view.transmit_enabled && has(Capability::SetPtt)),
        yes(has(Capability::ReadPtt))
    ));
    out.push_str(&format!(
        "Can set Split VFO:\t{}\n",
        yes(has(Capability::Split))
    ));
    out.push_str(&format!("Can set RIT:\t{}\n", yes(has(Capability::RitXit))));
    out.push_str(&format!(
        "Can send Morse:\t{}\n",
        yes(view.transmit_enabled && has(Capability::SendCw))
    ));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use radioctl_core::capability::CapabilitySet;
    use radioctl_core::description::Family;

    fn view(desc: &RigDescription, tx: bool) -> CapsView<'_> {
        CapsView {
            description: desc,
            hamlib_model: 1,
            rx_low_hz: 30_000,
            rx_high_hz: 60_000_000,
            tx_power_mw: 100_000,
            transmit_enabled: tx,
        }
    }

    fn kx3_like() -> RigDescription {
        let mut desc = RigDescription::ic7300();
        desc.family = Family::ElecraftAscii;
        desc.capabilities = CapabilitySet::of(&[
            Capability::ReadFrequency,
            Capability::SetFrequency,
            Capability::SecondVfo,
            Capability::SetPtt,
            Capability::ReadPtt,
            Capability::Levels,
            Capability::RitXit,
            Capability::ReadSwr,
        ]);
        desc
    }

    // Tests [@ANCHOR: radioctld_caps_modes]
    // Tests [@ANCHOR: radioctld_caps_range_line]
    // Tests [@ANCHOR: radioctld_caps_dump_state]
    #[test]
    fn dump_state_has_hamlibs_layout_and_ends_with_done() {
        let desc = RigDescription::ic7300();
        let text = dump_state(&view(&desc, false));
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "1");
        assert_eq!(lines[1], "1");
        assert_eq!(lines[2], "0");
        assert!(lines[3].starts_with("30000.000000 60000000.000000 0x"));
        assert_eq!(lines[4], "0 0 0 0 0 0 0");
        assert_eq!(
            lines[5], "0 0 0 0 0 0 0",
            "no transmit range with the gate closed"
        );
        assert_eq!(lines.last().copied(), Some("done"));
        assert!(text.contains("\nptt_type=0x0\n"));
        assert!(text.contains("\nrig_model=1\n"));
        let level_gran = lines
            .iter()
            .find(|l| l.starts_with("level_gran="))
            .expect("gran");
        assert_eq!(level_gran.matches(';').count(), 64);
    }

    // Tests [@ANCHOR: radioctld_caps_dump_state]
    // Tests [@ANCHOR: radioctld_caps_gran]
    // Tests [@ANCHOR: radioctld_caps_granularity]
    #[test]
    fn an_open_gate_advertises_a_transmit_range_and_levels_follow_the_rig() {
        let desc = kx3_like();
        let closed = dump_state(&view(&desc, false));
        let open = dump_state(&view(&desc, true));
        assert_eq!(open.lines().count(), closed.lines().count() + 1);
        assert!(open.contains(" 1000 100000 0x3 0x1\n"));
        assert!(!closed.contains(" 1000 100000 "));
        assert!(open.contains("\nptt_type=0x1\n"));
        assert!(
            open.contains(&format!(
                "14={},{},{};",
                wire_int_as_float(8),
                wire_int_as_float(50),
                wire_int_as_float(1)
            )),
            "{open}"
        );
        assert!(
            open.contains(&format!("3=0,{},", 1.0f32.to_bits())),
            "{open}"
        );
        assert!(open.contains("28=0.000000,10.000000,0.100000;"), "{open}");
        assert!(open.contains("12=0.000000,1.000000,"), "{open}");
        assert!(open.contains("\n0x81000000\n"), "RIT and XIT functions");
        assert!(open.contains("\n9999\n9999\n"));
    }

    // Tests [@ANCHOR: radioctld_caps_wire_float]
    // Tests [@ANCHOR: radioctld_caps_wire_int]
    #[test]
    fn level_granularity_uses_the_encodings_hamlibs_client_reads() {
        assert_eq!(wire_float_bits(1.0), "1065353216");
        assert_eq!(wire_float_bits(0.0), "0");
        let eight = wire_int_as_float(8);
        let parsed: f32 = eight.parse().expect("float text");
        assert_eq!(parsed.to_bits(), 8);
        assert_eq!(
            wire_int_as_float(0).parse::<f32>().expect("zero").to_bits(),
            0
        );
    }

    // Tests [@ANCHOR: radioctld_caps_dump_caps]
    #[test]
    fn dump_caps_summarises_the_rig_and_the_gate() {
        let desc = kx3_like();
        let closed = dump_caps(&view(&desc, false));
        assert!(closed.starts_with("Caps dump for model: 1\n"));
        assert!(closed.contains("Transmit gate:\tclosed"));
        assert!(closed.contains("Can set PTT:\tN"));
        assert!(closed.ends_with("\n\n"));
        let open = dump_caps(&view(&desc, true));
        assert!(open.contains("Can set PTT:\tY"));
        assert!(open.contains("Mode list: "));
    }
}
