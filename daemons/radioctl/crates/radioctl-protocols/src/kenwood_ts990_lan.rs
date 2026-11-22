// SPDX-License-Identifier: AGPL-3.0-or-later
//! The TS-990S LAN session as a [`radioctl_core::net::Framing`]: the connection request, the login
//! and a keep-alive in front of the ordinary TS-990S CAT stream.
//!
//! **Source.** Kenwood's "PC CONTROL COMMAND REFERENCE for the TS-990S" (March/29/2013; stored as
//! `kenwood/TS-990S/TS-990S_PC_Control_Command_Reference.pdf`): the LAN connector's "Character
//! Encoding Mode" is UTF-16 (PDF p. 1), the procedure is `##CN;`, the connection answer, `##ID`,
//! and "if there are no communications for 10 seconds" the connection is closed (PDF p. 1); `##CN`
//! answers 1 (authorised) or 0 (denied: a LAN connection already exists) and `##ID<P1><P2><account>
//! <password>;` carries the account length P1 and password length P2 (one digit each, 1 to 8) and
//! is answered `##ID1;` or `##ID0;` (PDF p. 62), e.g. `##ID75kenwoodadmin`. Unlike the TS-890S
//! there is no account-type digit and the lengths are one digit.
//!
//! **Not in the reference, so assumed and unverified:** the byte order of the UTF-16 (little
//! endian, no byte-order mark, is used) and the keep-alive (the read-only `ID;`, as the TS-890S
//! session does; its answer is swallowed). The scope frames `##DD2`/`##DD3` pass through as payload.
//! Never run against a radio.

use std::time::Duration;

use radioctl_core::error::{ErrorKind, RigError};
use radioctl_core::net::{Decoded, Framing};

use crate::kenwood_ts890::lan_reply;

/// The keep-alive interval: well inside the radio's 10 seconds without communication.
pub const KEEPALIVE_EVERY: Duration = Duration::from_secs(4);
/// The longest a frame may be without its `;` before the stream is called corrupt.
const MAX_FRAME: usize = 4096;

// [@ANCHOR: radioctl_ts990_lan_utf16]
/// ASCII text as the UTF-16 (little endian) the LAN port speaks.
pub fn utf16(ascii: &[u8]) -> Vec<u8> {
    ascii.iter().flat_map(|b| [*b, 0]).collect()
}

// [@ANCHOR: radioctl_ts990_lan_connect_request]
/// The first frame, `##CN;` (as ASCII; the framing sends it as UTF-16).
pub fn lan_connect_request() -> Vec<u8> {
    b"##CN;".to_vec()
}

// [@ANCHOR: radioctl_ts990_lan_login]
/// The login frame `##ID<len><len><account><password>;` (PDF p. 62).
///
/// # Errors
///
/// `Unsupported` unless account and password are each 1 to 8 printable ASCII characters without `;`.
pub fn lan_login(account: &str, password: &str) -> Result<Vec<u8>, RigError> {
    let fits = |s: &str| {
        (1..=8).contains(&s.len()) && s.bytes().all(|b| (0x20..0x7F).contains(&b) && b != b';')
    };
    if !fits(account) || !fits(password) {
        return Err(RigError::new(
            ErrorKind::Unsupported,
            "LAN account and password are 1 to 8 printable ASCII characters without ';'",
        ));
    }
    Ok(format!(
        "##ID{}{}{account}{password};",
        account.len(),
        password.len()
    )
    .into_bytes())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitConnect,
    AwaitLogin,
    Up,
}

/// The TS-990S LAN session framing. The password is kept only for the login frame.
// [@ANCHOR: radioctl_kenwood_ts990_lan_framing]
pub struct Ts990LanFraming {
    login: Vec<u8>,
    phase: Phase,
    /// Wire bytes not yet a whole UTF-16 unit, then decoded text not yet a whole frame.
    unit: Vec<u8>,
    text: Vec<u8>,
    probes_out: u32,
}

impl std::fmt::Debug for Ts990LanFraming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Ts990LanFraming {{ phase: {:?}, login: <hidden> }}",
            self.phase
        )
    }
}

impl Ts990LanFraming {
    // [@ANCHOR: radioctl_ts990lan_new]
    /// A session with the radio's LAN account.
    ///
    /// # Errors
    ///
    /// `Unsupported` for an account or password the login frame cannot carry.
    pub fn new(account: &str, password: &str) -> Result<Ts990LanFraming, RigError> {
        Ok(Ts990LanFraming {
            login: lan_login(account, password)?,
            phase: Phase::AwaitConnect,
            unit: Vec::new(),
            text: Vec::new(),
            probes_out: 0,
        })
    }

    fn frame(&mut self, frame: &[u8], out: &mut Decoded) {
        let text = String::from_utf8_lossy(frame);
        match self.phase {
            Phase::AwaitConnect => match lan_reply(&text).filter(|_| text.starts_with("##CN")) {
                Some(true) => {
                    out.reply = utf16(&self.login);
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
                if self.probes_out > 0 && text.starts_with("ID") {
                    self.probes_out -= 1;
                } else {
                    out.payload.extend_from_slice(frame);
                }
            }
        }
    }
}

impl Framing for Ts990LanFraming {
    // [@ANCHOR: radioctl_ts990lan_reset]
    fn reset(&mut self) {
        self.phase = Phase::AwaitConnect;
        self.unit.clear();
        self.text.clear();
        self.probes_out = 0;
    }

    // [@ANCHOR: radioctl_ts990lan_greeting]
    fn greeting(&mut self) -> Vec<u8> {
        utf16(&lan_connect_request())
    }

    // [@ANCHOR: radioctl_ts990lan_needs_handshake]
    fn needs_handshake(&self) -> bool {
        true
    }

    // [@ANCHOR: radioctl_ts990lan_encode]
    fn encode(&mut self, payload: &[u8]) -> Vec<u8> {
        utf16(payload)
    }

    // [@ANCHOR: radioctl_ts990lan_decode]
    fn decode(&mut self, wire: &[u8]) -> Decoded {
        self.unit.extend_from_slice(wire);
        // Keep the CAT text as ASCII; a unit above 0x7F is not part of any CAT form here.
        let whole = self.unit.len() / 2 * 2;
        for pair in self
            .unit
            .drain(..whole)
            .collect::<Vec<u8>>()
            .chunks_exact(2)
        {
            self.text.push(if pair[1] == 0 && pair[0] < 0x80 {
                pair[0]
            } else {
                b'?'
            });
        }
        let mut out = Decoded::default();
        while let Some(end) = self.text.iter().position(|b| *b == b';') {
            let frame: Vec<u8> = self.text.drain(..=end).collect();
            self.frame(&frame, &mut out);
            if out.failure.is_some() {
                self.text.clear();
                return out;
            }
        }
        if self.text.len() > MAX_FRAME {
            self.text.clear();
            out.failure = Some("a LAN frame has no end".to_string());
        }
        out
    }

    // [@ANCHOR: radioctl_ts990lan_keepalive]
    fn keepalive(&self) -> Option<(Duration, Vec<u8>)> {
        Some((KEEPALIVE_EVERY, utf16(b"ID;")))
    }

    // [@ANCHOR: radioctl_ts990lan_keepalive_sent]
    fn keepalive_sent(&mut self) {
        self.probes_out = self.probes_out.saturating_add(1);
    }
}
