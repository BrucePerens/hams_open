// SPDX-License-Identifier: AGPL-3.0-or-later
//! Receive attenuator and preamplifier levels: the checks every engine and virtual rig share.
//!
//! [`Level::Attenuator`] is decibels of attenuation and [`Level::Preamp`] a step number; which
//! values a model has comes from its description (`[levels]`), never from a table in an engine.
//! A model without the block has neither level and every function here refuses.

use std::collections::BTreeMap;

use radioctl_core::description::RigDescription;
use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::units::Level;

// [@ANCHOR: radioctl_att_is_level]
/// True for the two levels this module handles.
pub fn is_front_end_level(level: Level) -> bool {
    matches!(level, Level::Attenuator | Level::Preamp)
}

// [@ANCHOR: radioctl_att_step]
/// The wire step of a front-end level value: for the attenuator the 1-based step whose
/// attenuation is `value` dB (0 is off), for the preamplifier the step itself. `Unsupported` when
/// the model has no such control or no such step.
///
/// # Errors
///
/// `Unsupported` for a level that is not a front-end level, a model without the control and a
/// value the model has no step for.
pub fn step_of(desc: &RigDescription, level: Level, value: u16) -> Result<u8, RigError> {
    let refuse = |what: &str| Err(RigError::new(ErrorKind::Unsupported, what.to_string()));
    match level {
        Level::Attenuator if desc.levels.max_attenuation_db().is_none() => {
            refuse("the description lists no attenuator steps")
        }
        Level::Attenuator => desc
            .levels
            .attenuator_step(value)
            .map_or_else(|| refuse("no attenuator step of that many decibels"), Ok),
        Level::Preamp if desc.levels.preamp_steps == 0 => {
            refuse("the description lists no preamplifier steps")
        }
        Level::Preamp => u8::try_from(value)
            .ok()
            .filter(|step| *step <= desc.levels.preamp_steps)
            .map_or_else(|| refuse("no preamplifier step that high"), Ok),
        _ => refuse("not a front-end level"),
    }
}

// [@ANCHOR: radioctl_att_value]
/// The level value of a wire step reported by the rig, the inverse of [`step_of`]; `None` for a
/// step the description does not have (the reply is then unrecognised, not guessed at).
pub fn value_of(desc: &RigDescription, level: Level, step: u8) -> Option<u16> {
    match level {
        Level::Attenuator => desc.levels.attenuator_db_of(step),
        Level::Preamp => (step <= desc.levels.preamp_steps && desc.levels.preamp_steps > 0)
            .then_some(u16::from(step)),
        _ => None,
    }
}

// [@ANCHOR: radioctl_att_virtual_get]
/// What a virtual rig answers to a read of a front-end level: its stored value, `None` when the
/// model has no such control.
pub fn virtual_get(
    desc: &RigDescription,
    levels: &BTreeMap<Level, u16>,
    level: Level,
) -> Option<u16> {
    step_of(desc, level, 0).ok()?;
    Some(levels.get(&level).copied().unwrap_or(0))
}

// [@ANCHOR: radioctl_att_virtual_set]
/// Stores a front-end level in a virtual rig; false (nothing stored) for a value the model has no
/// step for.
pub fn virtual_set(
    desc: &RigDescription,
    levels: &mut BTreeMap<Level, u16>,
    level: Level,
    value: u16,
) -> bool {
    if step_of(desc, level, value).is_err() {
        return false;
    }
    levels.insert(level, value);
    true
}

// [@ANCHOR: radioctl_att_virtual_kenwood]
/// The `RA` and `PA` commands of a virtual Kenwood-style rig: `RA;` answers `RAnn00;` (step, then
/// the always-zero second field) and `RAnn;` sets; `PA;` answers `PAn0;` and `PAn;` sets. A refusal
/// is `?;`.
pub fn virtual_kenwood(
    desc: &RigDescription,
    levels: &mut BTreeMap<Level, u16>,
    name: &str,
    arg: &str,
) -> Option<String> {
    virtual_ascii(desc, levels, name, arg, true)
}

// [@ANCHOR: radioctl_att_virtual_elecraft]
/// The `RA` and `PA` commands of a virtual Elecraft rig: `RA;` answers `RAnn;`, `PA;` answers
/// `PAn;` and the same forms set (Programmer's Reference Rev. G5, p. 22). A refusal is `?;`.
pub fn virtual_elecraft(
    desc: &RigDescription,
    levels: &mut BTreeMap<Level, u16>,
    name: &str,
    arg: &str,
) -> Option<String> {
    virtual_ascii(desc, levels, name, arg, false)
}

/// The shared body: `second_field` appends the always-zero second parameter Kenwood answers carry.
fn virtual_ascii(
    desc: &RigDescription,
    levels: &mut BTreeMap<Level, u16>,
    name: &str,
    arg: &str,
    second_field: bool,
) -> Option<String> {
    let (level, width) = if name == "RA" {
        (Level::Attenuator, 2)
    } else {
        (Level::Preamp, 1)
    };
    let refuse = || Some("?;".to_string());
    let Some(stored) = virtual_get(desc, levels, level) else {
        return refuse();
    };
    if arg.is_empty() {
        let step = step_of(desc, level, stored).unwrap_or(0);
        let tail = if second_field {
            format!("{:0width$}", 0)
        } else {
            String::new()
        };
        return Some(format!("{name}{step:0width$}{tail};"));
    }
    let step = (arg.len() == width && arg.bytes().all(|b| b.is_ascii_digit()))
        .then(|| arg.parse::<u8>().ok())
        .flatten()
        .and_then(|step| value_of(desc, level, step));
    match step {
        Some(value) => {
            levels.insert(level, value);
            None
        }
        None => refuse(),
    }
}

// [@ANCHOR: radioctl_att_virtual_yaesu]
/// The `RA` and `PA` commands of a virtual Yaesu newcat rig: `RA0;` answers `RA0n;` (the leading 0
/// is the main receiver) and `RA0n;` sets; the same for `PA`. A refusal is `?;`.
pub fn virtual_yaesu(
    desc: &RigDescription,
    levels: &mut BTreeMap<Level, u16>,
    name: &str,
    arg: &str,
) -> Option<String> {
    let level = if name == "RA" {
        Level::Attenuator
    } else {
        Level::Preamp
    };
    let refuse = || Some("?;".to_string());
    let Some(stored) = virtual_get(desc, levels, level) else {
        return refuse();
    };
    if arg == "0" {
        let step = step_of(desc, level, stored).unwrap_or(0);
        return Some(format!("{name}0{step};"));
    }
    let step = arg
        .strip_prefix('0')
        .filter(|rest| rest.len() == 1 && rest.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|rest| rest.parse::<u8>().ok())
        .and_then(|step| value_of(desc, level, step));
    match step {
        Some(value) => {
            levels.insert(level, value);
            None
        }
        None => refuse(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn desc() -> RigDescription {
        let mut d = RigDescription::ic7300();
        d.levels.attenuator_db = vec![6, 12, 18];
        d.levels.preamp_steps = 2;
        d
    }

    // Tests [@ANCHOR: radioctl_att_is_level]
    #[test]
    fn only_the_two_front_end_levels_qualify() {
        assert!(is_front_end_level(Level::Attenuator));
        assert!(is_front_end_level(Level::Preamp));
        assert!(!is_front_end_level(Level::RfGain));
    }

    // Tests [@ANCHOR: radioctl_att_step]
    // Tests [@ANCHOR: radioctl_att_value]
    #[test]
    fn steps_and_values_are_inverse_and_bounded_by_the_description() {
        let d = desc();
        assert_eq!(step_of(&d, Level::Attenuator, 0).ok(), Some(0));
        assert_eq!(step_of(&d, Level::Attenuator, 12).ok(), Some(2));
        assert!(step_of(&d, Level::Attenuator, 10).is_err());
        assert_eq!(step_of(&d, Level::Preamp, 2).ok(), Some(2));
        assert!(step_of(&d, Level::Preamp, 3).is_err());
        assert!(step_of(&d, Level::Preamp, 300).is_err());
        assert!(step_of(&d, Level::RfGain, 0).is_err());
        assert_eq!(value_of(&d, Level::Attenuator, 3), Some(18));
        assert_eq!(value_of(&d, Level::Attenuator, 4), None);
        assert_eq!(value_of(&d, Level::Preamp, 1), Some(1));
        assert_eq!(value_of(&d, Level::Preamp, 3), None);
        assert_eq!(value_of(&d, Level::MicGain, 0), None);
        let none = RigDescription::ic7300();
        assert!(step_of(&none, Level::Attenuator, 0).is_err());
        assert!(step_of(&none, Level::Preamp, 0).is_err());
        assert_eq!(value_of(&none, Level::Preamp, 0), None);
    }

    // Tests [@ANCHOR: radioctl_att_virtual_kenwood]
    // Tests [@ANCHOR: radioctl_att_virtual_elecraft]
    // Tests [@ANCHOR: radioctl_att_virtual_yaesu]
    #[test]
    fn the_ascii_virtual_handlers_read_set_and_refuse() {
        let d = desc();
        let mut levels = BTreeMap::new();
        assert_eq!(
            virtual_kenwood(&d, &mut levels, "RA", ""),
            Some("RA0000;".to_string())
        );
        assert_eq!(virtual_kenwood(&d, &mut levels, "RA", "02"), None);
        assert_eq!(
            virtual_kenwood(&d, &mut levels, "RA", ""),
            Some("RA0200;".to_string())
        );
        assert_eq!(virtual_kenwood(&d, &mut levels, "PA", "2"), None);
        assert_eq!(
            virtual_kenwood(&d, &mut levels, "PA", ""),
            Some("PA20;".to_string())
        );
        for bad in ["04", "0", "+1", "x1"] {
            assert_eq!(
                virtual_kenwood(&d, &mut levels, "RA", bad),
                Some("?;".to_string())
            );
        }
        assert_eq!(virtual_elecraft(&d, &mut levels, "RA", "01"), None);
        assert_eq!(
            virtual_elecraft(&d, &mut levels, "RA", ""),
            Some("RA01;".to_string())
        );
        assert_eq!(
            virtual_elecraft(&d, &mut levels, "PA", ""),
            Some("PA2;".to_string())
        );
        assert_eq!(virtual_yaesu(&d, &mut levels, "RA", "03"), None);
        assert_eq!(
            virtual_yaesu(&d, &mut levels, "RA", "0"),
            Some("RA03;".to_string())
        );
        assert_eq!(
            virtual_yaesu(&d, &mut levels, "PA", "09"),
            Some("?;".to_string())
        );
        let none = RigDescription::ic7300();
        assert_eq!(
            virtual_yaesu(&none, &mut levels, "RA", "0"),
            Some("?;".to_string())
        );
    }

    // Tests [@ANCHOR: radioctl_att_virtual_get]
    // Tests [@ANCHOR: radioctl_att_virtual_set]
    #[test]
    fn a_virtual_rig_stores_only_values_the_model_has() {
        let d = desc();
        let mut levels = BTreeMap::new();
        assert_eq!(virtual_get(&d, &levels, Level::Attenuator), Some(0));
        assert!(virtual_set(&d, &mut levels, Level::Attenuator, 18));
        assert!(!virtual_set(&d, &mut levels, Level::Attenuator, 7));
        assert_eq!(virtual_get(&d, &levels, Level::Attenuator), Some(18));
        let none = RigDescription::ic7300();
        assert_eq!(virtual_get(&none, &levels, Level::Attenuator), None);
        assert!(!virtual_set(&none, &mut levels, Level::Preamp, 1));
    }
}
