//! Fixed-point arithmetic primitives shared across every mode's own fixed-point port -- the
//! integer/table-based equivalents of the handful of `f64` operations the floating-point
//! implementations call directly (`sin`/`cos`, `exp2`/`log2`-style operations via `.exp()`/`.log2()`
//! on a base-e or base-2 quantity, and `sqrt`). See [`super`]'s own doc comment for the numeric
//! convention (Q16.16 unless stated otherwise) and the tolerance each primitive is held to.

pub mod isqrt;
pub mod trig;
mod trig_table;
