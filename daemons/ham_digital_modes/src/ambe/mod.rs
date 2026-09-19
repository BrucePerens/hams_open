//! AMBE (Advanced Multi-Band Excitation) vocoder family, organized by numeric representation first,
//! then by radio mode: [`float`] holds this codebase's original floating-point implementations
//! (RATET(27)/P25 full-rate, D-STAR, and AMBE+2 half-rate). [`fixed`] mirrors the same per-mode
//! layout with fixed-point implementations intended to run on a CPU with no floating-point unit at
//! all -- see that module's own doc comment for the numeric representation and porting status.
//! [`general`] sits beside both, not under either: it holds code that is genuinely
//! precision-independent (currently just the Golay/Hamming FEC codes both D-STAR and AMBE+2
//! half-rate's frame layer depend on, which is pure integer/bitwise arithmetic in the first place),
//! so both [`float`] and [`fixed`] share the exact same implementation rather than each carrying
//! their own copy of code that was never floating-point to begin with.
pub mod fixed;
pub mod float;
pub mod general;
