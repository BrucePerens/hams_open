// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-protocols`: protocol engines, one per family (Icom CI-V, Kenwood ASCII, ...), and
//! the virtual rigs generated from rig descriptions.
//!
//! Every engine is sans-IO: a pure state machine that takes bytes and a caller-supplied
//! clock and returns bytes and events. No engine touches a socket, a serial port or a runtime,
//! so each can be replayed from a transcript and fuzzed. Only the optional `io` module (feature
//! `tokio-io`, on by default) uses tokio, to run a virtual rig behind an in-memory stream.
//! Bytes are always handled with checked access (`get`, never indexing): reply parsing is the
//! attack surface.

pub mod adat;
pub mod alinco;
pub mod aor;
pub mod aor_classic;
pub mod ar7030;
pub mod attenuation;
pub mod barrett;
pub mod civ;
pub mod civ_scope;
pub mod codan;
pub mod commradio;
pub mod conformance;
pub mod dorji;
pub mod drake;
pub mod dwt;
pub mod elecraft;
pub mod flex;
pub mod funcube;
pub mod gomspace;
pub mod guohe;
pub mod hpsdr_net;
pub mod icom_marine;
pub mod icom_pcr;
pub mod jrc;
pub mod kachina;
pub mod kenwood;
pub mod kenwood_hh;
pub mod kenwood_ts890;
pub mod kenwood_ts890_lan;
pub mod lowe;
pub mod mds;
pub mod micom;
pub mod paragon;
pub mod prelude;
pub mod prm80;
pub mod racal;
pub mod receiver;
pub mod rft;
pub mod rs;
pub mod rs_hf;
pub mod rshfiq;
pub mod serial_vendor;
pub mod si570_usb;
pub mod skanti;
pub mod tentec;
pub mod tentec_rx320;
pub mod transfox;
pub mod uniden;
pub mod uniden_digital;
pub mod usb_family;
pub mod virtual_flex;
pub mod virtual_rig;
pub mod xg3;
pub mod yaesu;
pub mod yaesu_legacy;

#[cfg(feature = "tokio-io")]
pub mod easy;
#[cfg(feature = "tokio-io")]
pub mod flex_io;
#[cfg(feature = "tokio-io")]
pub mod hpsdr_io;
#[cfg(feature = "tokio-io")]
pub mod io;
