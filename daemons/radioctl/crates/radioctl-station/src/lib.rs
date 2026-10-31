// SPDX-License-Identifier: AGPL-3.0-or-later
//! `radioctl-station`: the station-control layer of radioctl (external switches and power control).
//!
//! Placement: a new crate in the radioctl workspace (decided with Bruce, 2026-10-30; recorded in
//! `docs/DESIGN.md` and the to-do). It uses `radioctl-core` for the keyed-state of rigs and its
//! reconnect backoff, and the same rules: no global state, no runtime of its own (the caller
//! passes a tokio `Handle` where a task is needed), protocol code sans-IO, nothing switches at
//! start-up. Proposal: `RELAY_EXTERNAL_SWITCHES_AND_POWER_CONTROL.md`.
//!
//! - [`model`]: channels, devices, profiles and the station definition, with validation.
//! - [`gpio_map`]: the GPIO function-map format (TOML) and board models, with validation.
//! - [`channel`]: channel drivers, the virtual bank, the switchboard (commanded and confirmed
//!   state, break-before-make selectors, momentary pulses, debounced inputs).
//! - [`modbus`]: sans-IO Modbus framing and the coil relay driver (ASCII and RTU).
//! - [`http_relay`]: Shelly Gen2 and Tasmota channel drivers.
//! - [`lifecycle`]: the radio lifecycle state machine (sans-IO).
//! - [`manager`]: lifecycle per radio, power button, reconnect with backoff, start-up reconcile.
//! - [`record`]: the durable radio record (JSON, owner-only, atomic).
//! - [`station`]: the dependency graph, profiles, interlocks and the emergency all-off.

pub mod channel;
pub mod error;
pub mod gpio_map;
pub mod http_relay;
pub mod lifecycle;
pub mod manager;
pub mod modbus;
pub mod model;
pub mod record;
pub mod station;
