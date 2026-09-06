# AMBE+2 (DMR / Yaesu System Fusion generation) -- documentation only, NOT IMPLEMENTED

Per `docs/proposals/AMBE_CODEC_AND_DSTAR_IMPLEMENTATION_PLAN.md`: DMR and Yaesu System Fusion both use
AMBE+2, the half-rate codec covered by the 2009 addendum to TIA-102.BABA-1, which names 12 specific
patents. This file exists to record what's known about how AMBE+2 differs from the AMBE generation
`ambe/mod.rs` implements (D-STAR's own, from the 2003 base standard), so a future implementer doesn't
have to re-derive that context from scratch -- **it is not an invitation to implement AMBE+2**. Do not
add AMBE+2 encode/decode code to this crate until the patent-clearance question in the plan document
above has been revisited and resolved.

Real next step, if this is ever picked up: read the 2009 addendum (`hams_com/reference/ambe/TIA-102.
BABA-1 Final for Publication.pdf`) with the same page-by-page care `ambe/mod.rs`'s own doc comment
describes for the 2003 base standard, and record the real structural differences here before writing
any code -- not assumed to be a drop-in replacement for the 2003 generation's own frame structure.
