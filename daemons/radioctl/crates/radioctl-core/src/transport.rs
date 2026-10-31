// SPDX-License-Identifier: AGPL-3.0-or-later
//! The byte transport a rig actor talks over.
//!
//! Any tokio `AsyncRead + AsyncWrite` is a transport: an in-memory duplex stream in tests, the
//! supervised serial port of [`crate::serial`], the TCP and UDP links of [`crate::net`], the
//! splitter's ptys. Reads must be cancel-safe (tokio's are). Serial control lines (RTS/DTR
//! keying) are not part of this trait: [`crate::serial::SerialLink`] offers them beside it.

use tokio::io::{AsyncRead, AsyncWrite};

/// A transport to a rig.
pub trait Transport: AsyncRead + AsyncWrite + Send + Unpin + 'static {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Transport for T {}
