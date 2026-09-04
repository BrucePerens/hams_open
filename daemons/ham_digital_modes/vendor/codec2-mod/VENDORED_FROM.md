# Provenance

Vendored from `https://github.com/M17-Project/Codec2-mod`, commit `680a0b192ca114770cfe3af65226aaace2b3aacb`
("Revert \"reduced the number of sincosf calls\"", 2026-01-12), on 2026-09-04.

Only `src/`, `inc/`, `LICENSE`, and `README.md` are vendored (no `.git`, no build artifacts).

**License: GNU Lesser General Public License, version 2.1** (see `LICENSE` in this directory,
copied verbatim and unmodified from the upstream commit above). This directory's files remain
under that original license, same as `../ft8_lib`'s own `LICENSE-MIT` sits unmodified alongside
its vendored source -- vendoring here does not relicense anything to this crate's own
LGPL-3.0-or-later.

Codec2-mod is itself a derivative work: "This code is based heavily and directly on the Codec2
speech codec by ... David Rowe ... et al." (`README.md`, "Important notice: derivative work").
Original project: `https://github.com/drowe67/codec2`, also LGPL-2.1.

Vendored for reference and analysis, not (yet) linked into this crate's own build --
see `../../docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md` for the actual engineering work this
supports: a fixed-point domain characterization of Codec2-mod's 3200bps mode, aimed at running it
on embedded targets without a hardware FPU. Nothing in this vendored tree has been modified; any
new fixed-point implementation this project builds from studying it will be original code living
elsewhere in this crate, not an edit to these vendored files.
