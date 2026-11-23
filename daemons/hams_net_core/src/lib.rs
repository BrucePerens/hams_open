//! Shared core of Hams.com Net for the relay, hams-node and the phone apps.
//!
//! No platform networking lives here: each consumer supplies the directory transport, the clock and the
//! WireGuard backend. Behaviour mirrors the Python implementations; `tests/vectors/*.json` are shared with
//! their tests (parity).

pub mod boundary;
pub mod siit;
