// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `rigctld` text protocol, response side: the `RPRT` error codes, the body of a successful
//! answer, and the plain and extended (`+`) response formats. Sans-IO.
//!
//! Plain format: a get answers with its values, one per line; a set answers `RPRT 0`; any failure
//! answers `RPRT <negative code>`. Extended format: a header line `name: arguments`, labelled
//! `Label: value` lines, then `RPRT <code>`. Both were observed against Hamlib 4.6.2 `rigctld`.

use radioctl_core::error::{ErrorKind, RigError};

use crate::names::VfoSel;

/// Invalid parameter.
pub const EINVAL: i32 = -1;
/// Hamlib `RIG_ECONF`: an invalid configuration.
pub const ECONF: i32 = -2;
/// Function not implemented.
pub const ENIMPL: i32 = -4;
/// Communication timed out.
pub const ETIMEOUT: i32 = -5;
/// IO error, including the link to the rig being down.
pub const EIO: i32 = -6;
/// Protocol error: the rig said something that makes no sense.
pub const EPROTO: i32 = -8;
/// Command rejected by the rig; also a refused transmit (gate closed, or another client owns it).
pub const ERJCTED: i32 = -9;
/// Feature not available on this rig.
pub const ENAVAIL: i32 = -11;
/// Bus busy.
pub const EBUSBUSY: i32 = -14;

/// What a successful command answers with.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    /// A set, or any command with no value to return.
    Done,
    /// Values: a label for the extended format (empty for none) and the value text.
    Fields(Vec<(&'static str, String)>),
    /// Free text that is its own answer (`\dump_state`, `\dump_caps`); `rprt` appends `RPRT 0`.
    Text {
        /// The text, ending in a newline.
        text: String,
        /// Whether `RPRT 0` follows.
        rprt: bool,
    },
    /// Answer `RPRT 0` and close the connection.
    Close,
}

/// The parts of a request that shape its response header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// The extended (`+`) format was asked for.
    pub extended: bool,
    /// The command's long name.
    pub name: &'static str,
    /// The VFO to echo: the token the client sent, or `currVFO` in VFO mode.
    pub vfo: Option<String>,
    /// The arguments to echo.
    pub args: String,
}

// [@ANCHOR: radioctld_reply_error_code]
/// The `RPRT` code for a failure of the rig actor.
pub fn error_code(error: &RigError) -> i32 {
    match error.kind {
        ErrorKind::Timeout => ETIMEOUT,
        ErrorKind::Rejected | ErrorKind::TransmitGateClosed => ERJCTED,
        ErrorKind::Unsupported => ENAVAIL,
        ErrorKind::LinkDown | ErrorKind::Cancelled => EIO,
        ErrorKind::Malformed => EPROTO,
        ErrorKind::QueueFull => EBUSBUSY,
    }
}

// [@ANCHOR: radioctld_reply_vfo_echo]
/// The VFO text a response header echoes: what the client sent, else `currVFO` in VFO mode.
pub fn vfo_echo(sent: Option<&(String, VfoSel)>, vfo_mode: bool) -> Option<String> {
    match sent {
        Some((text, _)) => Some(text.clone()),
        None if vfo_mode => Some("currVFO".to_string()),
        None => None,
    }
}

// [@ANCHOR: radioctld_reply_header_line]
/// The extended-format header line: `name:` then the VFO and arguments.
fn header_line(header: &Header) -> String {
    let echo = match (&header.vfo, header.args.is_empty()) {
        (Some(vfo), true) => vfo.clone(),
        (Some(vfo), false) => format!("{vfo}:{}", header.args),
        (None, _) => header.args.clone(),
    };
    if echo.is_empty() {
        format!("{}:\n", header.name)
    } else {
        format!("{}: {echo}\n", header.name)
    }
}

// [@ANCHOR: radioctld_reply_render]
/// Renders the answer to one request.
pub fn render(header: &Header, outcome: &Result<Body, i32>) -> String {
    if header.extended && header.name == "chk_vfo" {
        if let Ok(Body::Fields(fields)) = outcome {
            let value = fields.first().map_or("0", |(_, v)| v.as_str());
            return format!("ChkVFO: {value}\n");
        }
    }
    let mut out = String::new();
    if header.extended && !header.name.is_empty() {
        out.push_str(&header_line(header));
    }
    match outcome {
        Err(code) => out.push_str(&format!("RPRT {code}\n")),
        Ok(Body::Done | Body::Close) => out.push_str("RPRT 0\n"),
        Ok(Body::Fields(fields)) => {
            for (label, value) in fields {
                if header.extended && !label.is_empty() {
                    out.push_str(&format!("{label}: {value}\n"));
                } else {
                    out.push_str(&format!("{value}\n"));
                }
            }
            if header.extended || header.name == "get_lock_mode" {
                out.push_str("RPRT 0\n");
            }
        }
        Ok(Body::Text { text, rprt }) => {
            out.push_str(text);
            if *rprt {
                out.push_str("RPRT 0\n");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(extended: bool, name: &'static str, vfo: Option<&str>, args: &str) -> Header {
        Header {
            extended,
            name,
            vfo: vfo.map(str::to_string),
            args: args.to_string(),
        }
    }

    // Tests [@ANCHOR: radioctld_reply_error_code]
    #[test]
    fn rig_errors_map_to_hamlib_codes() {
        let code = |kind| error_code(&RigError::new(kind, "x"));
        assert_eq!(code(ErrorKind::Timeout), ETIMEOUT);
        assert_eq!(code(ErrorKind::TransmitGateClosed), ERJCTED);
        assert_eq!(code(ErrorKind::Rejected), ERJCTED);
        assert_eq!(code(ErrorKind::Unsupported), ENAVAIL);
        assert_eq!(code(ErrorKind::LinkDown), EIO);
        assert_eq!(code(ErrorKind::Malformed), EPROTO);
        assert_eq!(code(ErrorKind::QueueFull), EBUSBUSY);
    }

    // Tests [@ANCHOR: radioctld_reply_vfo_echo]
    #[test]
    fn the_echoed_vfo_is_the_one_sent_or_currvfo_in_vfo_mode() {
        let sent = ("VFOB".to_string(), VfoSel::B);
        assert_eq!(vfo_echo(Some(&sent), false), Some("VFOB".to_string()));
        assert_eq!(vfo_echo(None, true), Some("currVFO".to_string()));
        assert_eq!(vfo_echo(None, false), None);
    }

    // Tests [@ANCHOR: radioctld_reply_render]
    // Tests [@ANCHOR: radioctld_reply_header_line]
    #[test]
    fn plain_format_matches_hamlib() {
        let get = header(false, "get_freq", None, "");
        let fields = Ok(Body::Fields(vec![("Frequency", "14074000".into())]));
        assert_eq!(render(&get, &fields), "14074000\n");
        let mode = Ok(Body::Fields(vec![
            ("Mode", "USB".into()),
            ("Passband", "2400".into()),
        ]));
        assert_eq!(render(&get, &mode), "USB\n2400\n");
        assert_eq!(render(&get, &Ok(Body::Done)), "RPRT 0\n");
        assert_eq!(render(&get, &Err(ENAVAIL)), "RPRT -11\n");
        assert_eq!(render(&get, &Ok(Body::Close)), "RPRT 0\n");
    }

    // Tests [@ANCHOR: radioctld_reply_render]
    // Tests [@ANCHOR: radioctld_reply_header_line]
    #[test]
    fn extended_format_matches_hamlib() {
        let get = header(true, "get_freq", None, "");
        let fields = Ok(Body::Fields(vec![("Frequency", "14074000".into())]));
        assert_eq!(
            render(&get, &fields),
            "get_freq:\nFrequency: 14074000\nRPRT 0\n"
        );
        let set = header(true, "set_freq", None, "14074000");
        assert_eq!(
            render(&set, &Ok(Body::Done)),
            "set_freq: 14074000\nRPRT 0\n"
        );
        assert_eq!(render(&set, &Err(EINVAL)), "set_freq: 14074000\nRPRT -1\n");
        let level = header(true, "get_level", None, "AF");
        let value = Ok(Body::Fields(vec![("", "0.5".into())]));
        assert_eq!(render(&level, &value), "get_level: AF\n0.5\nRPRT 0\n");
    }

    // Tests [@ANCHOR: radioctld_reply_render]
    // Tests [@ANCHOR: radioctld_reply_header_line]
    #[test]
    fn vfo_mode_headers_echo_the_vfo() {
        let get = header(true, "get_freq", Some("VFOA"), "");
        let fields = Ok(Body::Fields(vec![("Frequency", "7074000".into())]));
        assert_eq!(
            render(&get, &fields),
            "get_freq: VFOA\nFrequency: 7074000\nRPRT 0\n"
        );
        let set = header(true, "set_freq", Some("VFOA"), "14074000");
        assert_eq!(
            render(&set, &Ok(Body::Done)),
            "set_freq: VFOA:14074000\nRPRT 0\n"
        );
        let level = header(true, "get_level", Some("currVFO"), "AF");
        assert!(render(&level, &Err(EINVAL)).starts_with("get_level: currVFO:AF\n"));
    }

    // Tests [@ANCHOR: radioctld_reply_render]
    #[test]
    fn get_lock_mode_keeps_hamlibs_trailing_rprt_in_the_plain_format() {
        let plain = header(false, "get_lock_mode", None, "");
        let locked = Ok(Body::Fields(vec![("Locked", "0".into())]));
        assert_eq!(render(&plain, &locked), "0\nRPRT 0\n");
        let ext = header(true, "get_lock_mode", None, "");
        assert_eq!(render(&ext, &locked), "get_lock_mode:\nLocked: 0\nRPRT 0\n");
    }

    // Tests [@ANCHOR: radioctld_reply_render]
    #[test]
    fn chk_vfo_and_free_text_have_their_own_shapes() {
        let chk = header(true, "chk_vfo", None, "");
        let zero = Ok(Body::Fields(vec![("ChkVFO", "0".into())]));
        assert_eq!(render(&chk, &zero), "ChkVFO: 0\n");
        assert_eq!(render(&header(false, "chk_vfo", None, ""), &zero), "0\n");
        let text = Ok(Body::Text {
            text: "a\nb\n".into(),
            rprt: true,
        });
        let caps = header(true, "dump_caps", None, "");
        assert_eq!(render(&caps, &text), "dump_caps:\na\nb\nRPRT 0\n");
        let state = Ok(Body::Text {
            text: "1\ndone\n".into(),
            rprt: false,
        });
        assert_eq!(
            render(&header(false, "dump_state", None, ""), &state),
            "1\ndone\n"
        );
    }
}
