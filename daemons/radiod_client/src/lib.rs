// SPDX-License-Identifier: AGPL-3.0-or-later
//! A receive-only client for the multicast control and status protocol of `radiod` (ka9q-radio).
//!
//! Written from the published protocol description (`docs/ka9q-api.md` in ka9q-radio) and from
//! observed behaviour of an unmodified `radiod` on loopback; see
//! `hams_com/docs/reviews/RADIOD_TRIAL_2026-10-30.md` for what was read and measured.
//!
//! Design rules:
//! * **Closed command schema.** The only way to build bytes for `radiod` is [`command::Command`]
//!   and [`command::Command::encode`]. There is no function that sends caller-supplied bytes, no
//!   field for a data destination (so a client cannot redirect a stream to another host), and every
//!   channel carries a non-zero lifetime so an orphan expires on its own.
//! * **Every incoming packet is untrusted.** [`tlv`] bounds-checks every length and never indexes
//!   past the datagram.
//! * **Tokio only, no runtime of our own.** Sockets are `tokio::net` sockets registered with a
//!   [`tokio::runtime::Handle`] the caller passes in; every wait is an `async fn`. The parsers
//!   ([`tlv`], [`status`], [`rtp`]) and the acknowledgement rules in [`net`] are sans-IO.
//! * **Bounded spectrum requests.** `radiod` crashes on a spectrum reply over its 65,536-byte
//!   buffer, so a request never exceeds [`command::MAX_FLOAT_BINS`] float or
//!   [`command::MAX_BYTE_BINS`] byte bins, whatever the site policy says.
//! * **Receive only.** Nothing here can key a transmitter; `radiod` has no transmit path.
#![doc = "Modules: [`tlv`] wire format, [`command`] closed schema, [`status`] parsed state, [`rtp`] audio decoding, [`net`] sockets."]

pub mod command;
pub mod net;
pub mod rtp;
pub mod status;
pub mod tlv;
