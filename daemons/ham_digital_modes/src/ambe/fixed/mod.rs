//! Fixed-point AMBE implementations, mirroring [`super::float`]'s per-mode layout
//! (`ratet27`/`dstar`/`ambe_plus_2`, plus `general` for shared code). Every function in this tree
//! must compile and run correctly using only integer and fixed-point arithmetic -- no `f32`/`f64`,
//! no soft-float trap/emulation reliance -- so the resulting code is suitable for a CPU with no
//! floating-point unit at all. Each module's own doc comment states its fixed-point format (e.g.
//! Q-format word width and fractional bits) and cites the floating-point sibling it was derived
//! from, so the two can be cross-checked against the same real chip captures and frozen fixtures
//! the floating-point implementations already use.
