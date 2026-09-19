//! AMBE (Advanced Multi-Band Excitation) vocoder family, organized by numeric representation first,
//! then by radio mode: [`float`] holds this codebase's original floating-point implementations
//! (RATET(27)/P25 full-rate, D-STAR, and AMBE+2 half-rate, plus [`float::general`] for code shared
//! across more than one mode -- currently just the Golay/Hamming FEC codes both D-STAR and AMBE+2
//! half-rate's frame layer depend on). [`fixed`] mirrors the same per-mode layout with fixed-point
//! implementations intended to run on a CPU with no floating-point unit at all -- see that module's
//! own doc comment for the numeric representation and porting status.
pub mod fixed;
pub mod float;
