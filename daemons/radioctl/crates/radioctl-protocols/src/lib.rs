// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-protocols`: protocol engines, one per family (Icom CI-V, Kenwood ASCII, ...).
//!
//! Every engine is sans-IO: a pure state machine that takes bytes and a caller-supplied
//! clock and returns bytes and events. No engine touches a socket, a serial port or a runtime,
//! so each can be replayed from a transcript and fuzzed. Engines arrive in PR 3.
