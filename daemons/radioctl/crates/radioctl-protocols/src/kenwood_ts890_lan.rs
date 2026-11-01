// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-890S LAN session as a [`radioctl_core::net::Framing`]: the connection request, the
//! login, and the keep-alive, in front of the ordinary TS-890S CAT stream.
//!
//! **Source.** The same Kenwood guide as [`crate::kenwood_ts890`] (January/30/2019, sha256 e106050c...,
//! "LAN COMMUNICATION PROCEDURES" on the first page and "LAN EXCLUSIVE COMMAND TABLES" on PDF
//! pages 70 and 71): the radio listens on TCP, text is UTF-8, the PC sends `##CN;`, waits for the
//! connection answer (`##CN1;` authorised, `##CN0;` denied: a LAN connection already exists), sends
//! `##ID<type><len><len><account><password>;`, and the connection is complete when the answer is
//! `##ID1;` (`##ID0;` is an authorisation failure). "If there are no communications for 10 seconds,
//! the TCP/IP connection with the transceiver is terminated", so a keep-alive is part of the
//! session: this one is the read-only command `ID;` (answer `ID024;`) every 4 seconds, and the
//! answer is swallowed here, not passed to the engine. After the login the stream is the plain
//! TS-890S CAT stream of [`crate::kenwood_ts890::Ts890Engine`]. Never run against a radio.
//!
//! The account and password are the owner's; this type keeps them in a [`LanCredentials`] whose
//! `Debug` hides the password, and sends them only in the login frame. Where they come from (a
//! secret store, never a configuration file or a log) is the application's business.
//!
//! The LAN-only `##DD2` and `##DD3` bandscope frames pass through here as ordinary payload (this
//! layer only swallows the keep-alive answer); [`crate::kenwood_ts890::Ts890Engine`] decodes them.

use std::fmt;
use std::time::Duration;

use radioctl_core::error::RigError;
use radioctl_core::net::{Decoded, Framing};

use crate::kenwood_ts890::{lan_connect_request, lan_login, lan_reply};

/// The longest a frame may be without its `;` before the stream is called corrupt. The longest
/// real frame is the bandscope (1286 bytes).
const MAX_FRAME: usize = 4096;
/// The keep-alive interval: well inside the radio's 10 seconds without communication.
pub const KEEPALIVE_EVERY: Duration = Duration::from_secs(4);

/// A LAN account for the radio.
#[derive(Clone)]
pub struct LanCredentials {
    administrator: bool,
    account: String,
    password: String,
}

impl LanCredentials {
    // [@ANCHOR: radioctl_ts890lan_credentials_new]
    /// An account (`administrator` or a user account) and its password: 1 to 32 printable ASCII
    /// characters without `;` each.
    ///
    /// # Errors
    ///
    /// `Unsupported` for an account or password the login frame cannot carry.
    pub fn new(
        administrator: bool,
        account: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<LanCredentials, RigError> {
        let credentials = LanCredentials {
            administrator,
            account: account.into(),
            password: password.into(),
        };
        lan_login(administrator, &credentials.account, &credentials.password)?;
        Ok(credentials)
    }
}

impl fmt::Debug for LanCredentials {
    // [@ANCHOR: radioctl_ts890lan_credentials_debug]
    /// The account type and name; the password is never shown.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "LanCredentials {{ administrator: {}, account: {:?}, password: <hidden> }}",
            self.administrator, self.account
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitConnect,
    AwaitLogin,
    Up,
}

/// The TS-890S LAN session framing.
#[derive(Debug)]
pub struct Ts890LanFraming {
    credentials: LanCredentials,
    keepalive_every: Duration,
    phase: Phase,
    pending: Vec<u8>,
    probes_out: u32,
}

impl Ts890LanFraming {
    // [@ANCHOR: radioctl_ts890lan_new]
    /// A session with the account, keeping the connection alive every [`KEEPALIVE_EVERY`].
    pub fn new(credentials: LanCredentials) -> Ts890LanFraming {
        Ts890LanFraming {
            credentials,
            keepalive_every: KEEPALIVE_EVERY,
            phase: Phase::AwaitConnect,
            pending: Vec::new(),
            probes_out: 0,
        }
    }

    // [@ANCHOR: radioctl_ts890lan_with_keepalive]
    /// Changes the keep-alive interval (for tests against a fake radio with a short idle limit).
    pub fn with_keepalive(mut self, every: Duration) -> Ts890LanFraming {
        self.keepalive_every = every;
        self
    }

    // [@ANCHOR: radioctl_ts890lan_handle_frame]
    /// Handles one complete `;`-terminated frame in the current phase.
    fn frame(&mut self, frame: &[u8], out: &mut Decoded) {
        let text = String::from_utf8_lossy(frame);
        match self.phase {
            Phase::AwaitConnect => match lan_reply(&text).filter(|_| text.starts_with("##CN")) {
                Some(true) => {
                    match lan_login(
                        self.credentials.administrator,
                        &self.credentials.account,
                        &self.credentials.password,
                    ) {
                        Ok(login) => out.reply = login,
                        Err(e) => out.failure = Some(e.detail),
                    }
                    self.phase = Phase::AwaitLogin;
                }
                Some(false) => {
                    out.failure = Some(
                        "the radio denied the LAN connection (one already exists)".to_string(),
                    );
                }
                None => {}
            },
            Phase::AwaitLogin => match lan_reply(&text).filter(|_| text.starts_with("##ID")) {
                Some(true) => {
                    out.ready = true;
                    self.phase = Phase::Up;
                }
                Some(false) => {
                    out.failure =
                        Some("the radio refused the login: wrong account or password".to_string());
                }
                None => {}
            },
            Phase::Up => {
                if self.probes_out > 0 && text.starts_with("ID") && text.ends_with(';') {
                    self.probes_out -= 1;
                } else {
                    out.payload.extend_from_slice(frame);
                }
            }
        }
    }
}

impl Framing for Ts890LanFraming {
    // [@ANCHOR: radioctl_ts890lan_reset]
    fn reset(&mut self) {
        self.phase = Phase::AwaitConnect;
        self.pending.clear();
        self.probes_out = 0;
    }

    // [@ANCHOR: radioctl_ts890lan_greeting]
    fn greeting(&mut self) -> Vec<u8> {
        lan_connect_request()
    }

    // [@ANCHOR: radioctl_ts890lan_needs_handshake]
    fn needs_handshake(&self) -> bool {
        true
    }

    // [@ANCHOR: radioctl_ts890lan_encode]
    fn encode(&mut self, payload: &[u8]) -> Vec<u8> {
        payload.to_vec()
    }

    // [@ANCHOR: radioctl_ts890lan_decode]
    fn decode(&mut self, wire: &[u8]) -> Decoded {
        self.pending.extend_from_slice(wire);
        let mut out = Decoded::default();
        while let Some(end) = self.pending.iter().position(|b| *b == b';') {
            let frame: Vec<u8> = self.pending.drain(..=end).collect();
            self.frame(&frame, &mut out);
            if out.failure.is_some() {
                self.pending.clear();
                return out;
            }
        }
        if self.pending.len() > MAX_FRAME {
            self.pending.clear();
            out.failure = Some("a LAN frame has no end".to_string());
        }
        out
    }

    // [@ANCHOR: radioctl_ts890lan_keepalive]
    fn keepalive(&self) -> Option<(Duration, Vec<u8>)> {
        Some((self.keepalive_every, b"ID;".to_vec()))
    }

    // [@ANCHOR: radioctl_ts890lan_keepalive_sent]
    fn keepalive_sent(&mut self) {
        self.probes_out = self.probes_out.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Ts890LanFraming {
        Ts890LanFraming::new(LanCredentials::new(true, "kenwood", "admin").unwrap())
    }

    // Tests [@ANCHOR: radioctl_ts890lan_credentials_new]
    // Tests [@ANCHOR: radioctl_ts890lan_credentials_debug]
    #[test]
    fn credentials_are_checked_and_never_shown() {
        let c = LanCredentials::new(false, "ops", "hunter2-secret").unwrap();
        let shown = format!("{c:?}");
        assert!(shown.contains("ops") && !shown.contains("hunter2"));
        for (a, p) in [
            ("", "x"),
            ("x", ""),
            ("a;b", "x"),
            ("x", "p;q"),
            (&"a".repeat(33), "x"),
        ] {
            assert!(LanCredentials::new(true, a, p).is_err(), "{a:?} {p:?}");
        }
    }

    // Tests [@ANCHOR: radioctl_ts890lan_new]
    // Tests [@ANCHOR: radioctl_ts890lan_reset]
    // Tests [@ANCHOR: radioctl_ts890lan_greeting]
    // Tests [@ANCHOR: radioctl_ts890lan_needs_handshake]
    // Tests [@ANCHOR: radioctl_ts890lan_encode]
    // Tests [@ANCHOR: radioctl_ts890lan_decode]
    // Tests [@ANCHOR: radioctl_ts890lan_handle_frame]
    #[test]
    fn the_session_follows_the_guides_procedure() {
        let mut s = session();
        assert_eq!(s.greeting(), b"##CN;");
        assert!(s.needs_handshake());
        // Authorised: the login frame is the guide's own example.
        let d = s.decode(b"##CN1;");
        assert_eq!(d.reply, b"##ID00705kenwoodadmin;");
        assert!(!d.ready && d.failure.is_none());
        // Login accepted, and the answer may arrive in pieces and with CAT data after it.
        assert!(!s.decode(b"##I").ready);
        let d = s.decode(b"D1;FA00014074000;");
        assert!(d.ready);
        assert_eq!(d.payload, b"FA00014074000;");
        // Up: CAT passes both ways unchanged.
        assert_eq!(s.encode(b"FA;"), b"FA;");
        assert_eq!(
            s.decode(b"FA00007074000;ID024;").payload,
            b"FA00007074000;ID024;"
        );
        // A reset starts the session over.
        s.reset();
        assert_eq!(s.decode(b"##CN1;").reply, b"##ID00705kenwoodadmin;");
    }

    #[test]
    fn denial_refusal_and_junk_fail_the_session() {
        let d = session().decode(b"##CN0;");
        assert!(d.failure.unwrap().contains("denied"));
        let mut s = session();
        s.decode(b"##CN1;");
        let d = s.decode(b"##ID0;");
        assert!(d.failure.unwrap().contains("wrong account or password"));
        // Frames of the wrong sort before the login are ignored, not trusted.
        let mut s = session();
        assert!(s.decode(b"FA00014074000;##ID1;").reply.is_empty());
        let d = session().decode(&[b'x'; 5000]);
        assert!(d.failure.unwrap().contains("no end"));
    }

    // Tests [@ANCHOR: radioctl_ts890lan_with_keepalive]
    // Tests [@ANCHOR: radioctl_ts890lan_keepalive]
    // Tests [@ANCHOR: radioctl_ts890lan_keepalive_sent]
    // Tests [@ANCHOR: radioctl_core_net_framing_keepalive_sent]
    #[test]
    fn the_keepalive_is_a_read_and_its_answer_is_swallowed() {
        let mut s = session().with_keepalive(Duration::from_millis(100));
        let (every, probe) = s.keepalive().unwrap();
        assert_eq!(every, Duration::from_millis(100));
        assert_eq!(probe, b"ID;");
        s.decode(b"##CN1;");
        s.decode(b"##ID1;");
        // An ID answer with no probe outstanding is the engine's own business.
        assert_eq!(s.decode(b"ID024;").payload, b"ID024;");
        s.keepalive_sent();
        let d = s.decode(b"ID024;FA00014074000;");
        assert_eq!(d.payload, b"FA00014074000;");
        // Only one probe was outstanding.
        assert_eq!(s.decode(b"ID024;").payload, b"ID024;");
        assert_eq!(session().keepalive().unwrap().0, KEEPALIVE_EVERY);
    }
}
