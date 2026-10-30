// SPDX-License-Identifier: AGPL-3.0-or-later
//! The byte transport a rig actor talks over.
//!
//! Any tokio `AsyncRead + AsyncWrite` is a transport: an in-memory duplex stream in tests, and
//! later a serial port, a TCP stream or a pty. Reads must be cancel-safe (tokio's are). Serial
//! control lines (RTS/DTR keying) are not part of this trait yet; no real transport exists.

use tokio::io::{AsyncRead, AsyncWrite};

/// A transport to a rig.
pub trait Transport: AsyncRead + AsyncWrite + Send + Unpin + 'static {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Transport for T {}
