// SPDX-License-Identifier: AGPL-3.0-or-later
//! Hamlib's `rotctld` text protocol for a radioctl rotator: the request parser, the service that
//! answers a line, and the response renderer (the rig protocol's [`crate::reply::render`], which
//! has the same shape: an optional `+` for the extended format, values one per line, `RPRT n`).
//!
//! The protocol was observed from Hamlib 4.6.2's `rotctld -m 1` (the dummy rotator) on this
//! machine and the `rotctld(1)` manual page. Served: `P`/`\set_pos`, `p`/`\get_pos`, `S`/`\stop`,
//! `K`/`\park`, `_`/`\get_info`, `\dump_state`, `q`/`Q`. Everything else Hamlib knows answers
//! `RPRT -4` (not implemented), unknown words `RPRT -1`, and the raw `w`/`\send_cmd` is never
//! served (`RPRT -11`). Arguments are checked strictly: a missing, surplus or malformed argument is
//! `RPRT -1`, never a wait for another line. A position outside the description's travel is
//! refused by the engine before anything is written, and answers `RPRT -1` as Hamlib's
//! `rot_set_position` does. Nothing here is verified against a real rotator.

use std::sync::Arc;

use radioctl_station::error::{StationError, StationErrorKind};
use radioctl_station::rotator::Rotator;

use crate::command::Line;
use crate::reply::{render, Body, Header, ECONF, EINVAL, EIO, ENAVAIL, ENIMPL, EPROTO, ETIMEOUT};
use crate::server::{BoxFuture, LineService};

/// A rotator command the server executes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RotCommand {
    /// `P az el`: move.
    SetPosition {
        /// Azimuth, degrees.
        az: f32,
        /// Elevation, degrees.
        el: f32,
    },
    /// `p`: read the position.
    GetPosition,
    /// `S`: stop.
    Stop,
    /// `K`: go to the park position.
    Park,
    /// `_`: the rotator's name.
    GetInfo,
    /// `\dump_state`: what `rotctl -m 2` reads on connect.
    DumpState,
    /// `q`: close the connection.
    Quit,
}

/// What a request line turned into.
#[derive(Debug, Clone, PartialEq)]
pub struct RotParsed {
    /// The extended (`+`) response format was asked for.
    pub extended: bool,
    /// The long name of the command, for the response header; empty when it was not recognised.
    pub name: &'static str,
    /// The arguments, blank-normalised, for the response header.
    pub args: String,
    /// The command, or the `RPRT` code to answer with.
    pub command: Result<RotCommand, i32>,
}

/// Hamlib's short and long rotator command names, and whether radioctld serves them.
const NAMES: &[(&str, &str, bool)] = &[
    ("P", "set_pos", true),
    ("p", "get_pos", true),
    ("S", "stop", true),
    ("K", "park", true),
    ("_", "get_info", true),
    ("q", "quit", true),
    ("Q", "quit", true),
    ("", "dump_state", true),
    ("M", "move", false),
    ("R", "reset", false),
    ("C", "set_conf", false),
    ("", "get_conf", false),
    ("1", "dump_caps", false),
    ("", "get_status", false),
    ("", "set_level", false),
    ("", "get_level", false),
    ("", "set_func", false),
    ("", "get_func", false),
    ("", "set_parm", false),
    ("", "get_parm", false),
    ("L", "lonlat2loc", false),
    ("l", "loc2lonlat", false),
    ("D", "dms2dec", false),
    ("d", "dec2dms", false),
    ("E", "dmmm2dec", false),
    ("e", "dec2dmmm", false),
    ("B", "qrb", false),
    ("A", "a_sp2a_lp", false),
    ("a", "d_sp2d_lp", false),
    ("", "pause", false),
];

// [@ANCHOR: radioctld_rotctl_resolve]
/// The long name of a command word and whether it is served; `None` for a word Hamlib does not
/// know. `w`/`send_cmd` is known and never served.
fn resolve(word: &str, long_form: bool) -> Option<(&'static str, bool)> {
    if word == "w" || word == "send_cmd" {
        return Some(("send_cmd", false));
    }
    NAMES
        .iter()
        .find(|(short, long, _)| {
            if long_form {
                *long == word
            } else {
                !short.is_empty() && *short == word
            }
        })
        .map(|(_, long, served)| (*long, *served))
}

// [@ANCHOR: radioctld_rotctl_degrees]
/// A finite degree value from an argument.
fn degrees(token: &str) -> Result<f32, i32> {
    token
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or(EINVAL)
}

// [@ANCHOR: radioctld_rotctl_parse_line]
/// Parses one request line. `None` for a blank line (nothing to answer).
pub fn parse_rot_line(line: &str) -> Option<RotParsed> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (extended, line) = match line.strip_prefix('+') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, line),
    };
    let (word, long_form, rest) = if let Some(long) = line.strip_prefix('\\') {
        let (word, rest) = long.split_once(char::is_whitespace).unwrap_or((long, ""));
        (word, true, rest.trim_start())
    } else {
        let end = line.chars().next().map_or(0, char::len_utf8);
        (
            line.get(..end).unwrap_or(""),
            false,
            line.get(end..).unwrap_or("").trim_start(),
        )
    };
    let args = rest.split_whitespace().collect::<Vec<_>>().join(" ");
    let Some((name, served)) = resolve(word, long_form) else {
        return Some(RotParsed {
            extended,
            name: "",
            args,
            command: Err(EINVAL),
        });
    };
    if !served {
        let code = if name == "send_cmd" { ENAVAIL } else { ENIMPL };
        return Some(RotParsed {
            extended,
            name,
            args,
            command: Err(code),
        });
    }
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let command = match (name, tokens.as_slice()) {
        ("set_pos", [az, el]) => match (degrees(az), degrees(el)) {
            (Ok(az), Ok(el)) => Ok(RotCommand::SetPosition { az, el }),
            _ => Err(EINVAL),
        },
        ("get_pos", []) => Ok(RotCommand::GetPosition),
        ("stop", []) => Ok(RotCommand::Stop),
        ("park", []) => Ok(RotCommand::Park),
        ("get_info", []) => Ok(RotCommand::GetInfo),
        ("dump_state", []) => Ok(RotCommand::DumpState),
        ("quit", []) => Ok(RotCommand::Quit),
        _ => Err(EINVAL),
    };
    Some(RotParsed {
        extended,
        name,
        args,
        command,
    })
}

// [@ANCHOR: radioctld_rotctl_error_code]
/// The `RPRT` code for a rotator error. A refused position is `-1` (`RIG_EINVAL`), as Hamlib's
/// `rot_set_position` answers one outside the limits; a refused park (no park position) is `-11`.
pub fn rot_error_code(error: &StationError, command: RotCommand) -> i32 {
    match error.kind {
        StationErrorKind::Refused if command == RotCommand::Park => ENAVAIL,
        StationErrorKind::Refused => EINVAL,
        StationErrorKind::Timeout => ETIMEOUT,
        StationErrorKind::Config => ECONF,
        StationErrorKind::Driver | StationErrorKind::Store | StationErrorKind::KeyedUnknown => EIO,
        StationErrorKind::NotConfirmed => EPROTO,
        StationErrorKind::Unknown => ENAVAIL,
    }
}

/// The service behind a rotator's listener: one [`Rotator`] handle, shared by every client.
pub struct RotatorService {
    rotator: Rotator,
}

impl RotatorService {
    // [@ANCHOR: radioctld_rotctl_service_new]
    /// A service for `rotator`.
    pub fn new(rotator: Rotator) -> RotatorService {
        RotatorService { rotator }
    }

    // [@ANCHOR: radioctld_rotctl_service_rotator]
    /// The rotator handle.
    pub fn rotator(&self) -> &Rotator {
        &self.rotator
    }

    // [@ANCHOR: radioctld_rotctl_dump_state]
    /// The `\dump_state` text in the shape Hamlib 4.6.2's `rotctld` prints and its `rotctl -m 2`
    /// reads: protocol version 1, the rotator model, the travel as `key=value` lines, `south_zero`,
    /// the rotator type, `done`.
    pub fn dump_state(&self) -> String {
        let d = self.rotator.description();
        let rot_type = if d.has_elevation { "AzEl" } else { "Az" };
        format!(
            "1\n1\nmin_az={:.6}\nmax_az={:.6}\nmin_el={:.6}\nmax_el={:.6}\nsouth_zero=0\nrot_type={rot_type}\ndone\n",
            d.az_min, d.az_max, d.el_min, d.el_max
        )
    }

    // [@ANCHOR: radioctld_rotctl_service_execute]
    /// Executes one command. Every wait is bounded by the description's reply timeout, in the
    /// rotator handle.
    pub async fn execute(&self, command: RotCommand) -> Result<Body, i32> {
        let code = |e: StationError| rot_error_code(&e, command);
        match command {
            RotCommand::SetPosition { az, el } => {
                self.rotator.set_position(az, el).await.map_err(code)?;
                Ok(Body::Done)
            }
            RotCommand::GetPosition => {
                let (az, el) = self.rotator.position().await.map_err(code)?;
                Ok(Body::Fields(vec![
                    ("Azimuth", format!("{az:.2}")),
                    ("Elevation", format!("{el:.2}")),
                ]))
            }
            RotCommand::Stop => {
                self.rotator.stop().await.map_err(code)?;
                Ok(Body::Done)
            }
            RotCommand::Park => {
                self.rotator.park().await.map_err(code)?;
                Ok(Body::Done)
            }
            RotCommand::GetInfo => Ok(Body::Fields(vec![(
                "Info",
                self.rotator.description().name.clone(),
            )])),
            RotCommand::DumpState => Ok(Body::Text {
                text: self.dump_state(),
                rprt: false,
            }),
            RotCommand::Quit => Ok(Body::Close),
        }
    }
}

// [@ANCHOR: radioctld_rotctl_answer]
/// The text to send for one request line, and whether the connection closes after it.
pub async fn rot_answer(service: &RotatorService, line: &Line) -> (String, bool) {
    let text = match line {
        Line::Invalid => return (format!("RPRT {EINVAL}\n"), false),
        Line::Text(text) => text,
    };
    let Some(parsed) = parse_rot_line(text) else {
        return (String::new(), false);
    };
    let header = Header {
        extended: parsed.extended,
        name: parsed.name,
        vfo: None,
        args: parsed.args,
    };
    let outcome = match parsed.command {
        Err(code) => Err(code),
        Ok(command) => service.execute(command).await,
    };
    let close = matches!(outcome, Ok(Body::Close));
    (render(&header, &outcome), close)
}

impl LineService for RotatorService {
    // [@ANCHOR: radioctld_rotctl_service_answer]
    /// [`rot_answer`].
    fn answer<'a>(&'a self, _client: u64, line: &'a Line) -> BoxFuture<'a, (String, bool)> {
        Box::pin(rot_answer(self, line))
    }

    // [@ANCHOR: radioctld_rotctl_service_disconnect]
    /// Nothing is held per client: a rotator has no owner and no transmit path.
    fn disconnect(&self, _client: u64) -> BoxFuture<'_, ()> {
        Box::pin(async {})
    }
}

/// A shared rotator service.
pub type SharedRotatorService = Arc<RotatorService>;

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(line: &str) -> RotParsed {
        parse_rot_line(line).expect("a line")
    }

    // Tests [@ANCHOR: radioctld_rotctl_parse_line]
    // Tests [@ANCHOR: radioctld_rotctl_resolve]
    // Tests [@ANCHOR: radioctld_rotctl_degrees]
    #[test]
    fn requests_parse_strictly() {
        assert_eq!(parse_rot_line("  "), None);
        assert_eq!(
            parsed("P 180 45.5").command,
            Ok(RotCommand::SetPosition {
                az: 180.0,
                el: 45.5
            })
        );
        let p = parsed("+\\set_pos  10   20");
        assert!(p.extended);
        assert_eq!((p.name, p.args.as_str()), ("set_pos", "10 20"));
        assert_eq!(parsed("p").command, Ok(RotCommand::GetPosition));
        assert_eq!(parsed("\\get_pos").command, Ok(RotCommand::GetPosition));
        assert_eq!(parsed("S").command, Ok(RotCommand::Stop));
        assert_eq!(parsed("K").command, Ok(RotCommand::Park));
        assert_eq!(parsed("_").command, Ok(RotCommand::GetInfo));
        assert_eq!(parsed("\\dump_state").command, Ok(RotCommand::DumpState));
        assert_eq!(parsed("q").command, Ok(RotCommand::Quit));
        assert_eq!(parsed("Q").command, Ok(RotCommand::Quit));
        // Missing, surplus or malformed arguments: -1, never a wait.
        for bad in [
            "P",
            "P 10",
            "P 10 20 30",
            "P x 20",
            "P 10 nan",
            "p 1",
            "S 1",
            "K 0",
            "q 1",
        ] {
            assert_eq!(parsed(bad).command, Err(EINVAL), "{bad}");
        }
        // Hamlib commands radioctld does not serve: -4, named in the header; raw: -11.
        assert_eq!(parsed("M 16 0").command, Err(ENIMPL));
        assert_eq!(parsed("\\get_level").name, "get_level");
        assert_eq!(parsed("R 1").command, Err(ENIMPL));
        assert_eq!(parsed("w X").command, Err(ENAVAIL));
        assert_eq!(parsed("\\send_cmd X").command, Err(ENAVAIL));
        // Unknown words: -1, no name.
        let unknown = parsed("\\bogus");
        assert_eq!((unknown.command, unknown.name), (Err(EINVAL), ""));
        assert_eq!(parsed("Z").command, Err(EINVAL));
        // A long name without its backslash is a short command (`d`) and its arguments.
        assert_eq!(parsed("dump_state").command, Err(ENIMPL));
        assert_eq!(parsed("dump_state").name, "dec2dms");
        assert_eq!(resolve("", false), None);
    }

    // Tests [@ANCHOR: radioctld_rotctl_error_code]
    #[test]
    fn rotator_errors_map_to_hamlib_codes() {
        let e = |kind| StationError::new(kind, "x");
        let set = RotCommand::SetPosition { az: 0.0, el: 0.0 };
        assert_eq!(rot_error_code(&e(StationErrorKind::Refused), set), EINVAL);
        assert_eq!(
            rot_error_code(&e(StationErrorKind::Refused), RotCommand::Park),
            ENAVAIL
        );
        assert_eq!(rot_error_code(&e(StationErrorKind::Timeout), set), ETIMEOUT);
        assert_eq!(rot_error_code(&e(StationErrorKind::Driver), set), EIO);
        assert_eq!(rot_error_code(&e(StationErrorKind::Config), set), ECONF);
        assert_eq!(
            rot_error_code(&e(StationErrorKind::NotConfirmed), set),
            EPROTO
        );
        assert_eq!(rot_error_code(&e(StationErrorKind::Unknown), set), ENAVAIL);
        assert_eq!(rot_error_code(&e(StationErrorKind::Store), set), EIO);
        assert_eq!(rot_error_code(&e(StationErrorKind::KeyedUnknown), set), EIO);
    }
}
