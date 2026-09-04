# Provenance

Vendored from `https://github.com/M17-Project/Codec2-mod`, commit `680a0b192ca114770cfe3af65226aaace2b3aacb`
("Revert \"reduced the number of sincosf calls\"", 2026-01-12), on 2026-09-04.

Only `src/`, `inc/`, `LICENSE`, and `README.md` are vendored (no `.git`, no build artifacts).

**License: not uniform across this tree -- checked per-file header, not assumed from the repo-level
`LICENSE` alone, 2026-09-04:**

- **`src/kiss_fft.c`, `src/kiss_fftr.c`, `inc/kiss_fft.h`, `inc/kiss_fftr.h`, `inc/kiss_fft_log.h`,
  `inc/_kiss_fft_guts.h` are BSD-3-Clause** (Mark Borgerding's KISS FFT,
  `github.com/mborgerding/kissfft`, bundled by Codec2-mod the same way upstream `codec2` bundles it)
  -- each carries its own explicit `SPDX-License-Identifier: BSD-3-Clause` header, which overrides
  the repo-level `LICENSE` for these six files specifically.
- **Every other file in `src/`/`inc/` carries no per-file header of its own**, so the repo-level
  `LICENSE` in this directory governs them: **GNU Lesser General Public License, version 2.1
  (LGPL-2.1), with no "or later version" grant** -- checked directly against upstream `codec2`'s own
  header convention (`drowe67/codec2/src/lpc.c`: "the GNU Lesser General Public License version 2.1,
  as published by the Free Software Foundation," no "or (at your option) any later version" clause),
  which Codec2-mod's own README states it inherits ("Codec 2 is... (LGPL 2.1)"). This is
  **LGPL-2.1-only, not LGPL-2.1-or-later.**

Neither of these is relicensed by vendoring -- both sit here unmodified, the same way `../ft8_lib`'s
own `LICENSE-MIT` sits unmodified alongside its vendored source.

**Open compatibility question, not yet resolved, and not yet load-bearing since nothing here is
linked into this crate's build (see below)**: this crate itself is LGPL-3.0-or-later (see
`../../LICENSE` and `LICENSING.md`), stated there as chosen partly to stay "license-compatible with
being linked into other programs more permissively than AGPL would allow." LGPL-2.1-only code is
**not** automatically combinable into an LGPL-3.0-or-later work -- FSF's own compatibility guidance
treats this as a real, known trap, not a formality; the "or later version" clause is what lets a
work travel to a newer license family, and that clause is specifically absent here. Whoever wires
any Codec2-mod-derived algorithm into this crate's actual build needs to resolve this properly first
(e.g. write the new fixed-point code as an independent implementation informed by, but not copied
from, this vendored source, keeping this directory reference-only) -- do not assume "vendored
alongside an LGPL-3.0-or-later crate" already answers it.

Codec2-mod is itself a derivative work: "This code is based heavily and directly on the Codec2
speech codec by ... David Rowe ... et al." (`README.md`, "Important notice: derivative work").
Original project: `https://github.com/drowe67/codec2`, also LGPL-2.1-only (verified directly against
its own source headers, not assumed from its README's "(LGPL 2.1)" line alone).

Vendored for reference and analysis, not (yet) linked into this crate's own build --
see `../../docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md` for the actual engineering work this
supports: a fixed-point domain characterization of Codec2-mod's 3200bps mode, aimed at running it
on embedded targets without a hardware FPU. Nothing in this vendored tree has been modified; any
new fixed-point implementation this project builds from studying it will be original code living
elsewhere in this crate, not an edit to these vendored files.
