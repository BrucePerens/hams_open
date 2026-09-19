//! Floating-point AMBE implementations, one module per radio mode, plus [`general`] for code
//! genuinely shared across more than one mode. This is the original layout this codebase's AMBE
//! work was built in (each mode used to be its own top-level `crate::ambe`/`crate::ambe_dstar`/
//! `crate::ambe_plus_2` module) -- moved under `ambe::float::<mode>` so [`super::fixed`] can mirror
//! it mode-for-mode without a naming collision.
// Gated off by default, pending patent clearance for any real deployment use -- see this module's
// own doc comment and AMBE_PLUS_2_NOTES.md for the authorization history and scope. Build/test
// with `cargo build/test --features ambe_plus_2`.
#[cfg(feature = "ambe_plus_2")]
pub mod ambe_plus_2;
pub mod dstar;
pub mod general;
pub mod ratet27;
