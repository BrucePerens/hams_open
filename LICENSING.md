# Licensing

This repository is **not uniformly licensed**. Most of it is AGPL-3.0-or-later, but a couple of
real, deliberate exceptions exist (different license families entirely, not AGPL variants), plus
some real gaps. This document exists so nobody has to guess, or assume the majority license
applies everywhere it doesn't. When in doubt, the license header at the top of the actual file
governs, not this summary.

Full license texts are in [`licenses/`](licenses/): [`AGPL-3.0.txt`](licenses/AGPL-3.0.txt),
[`GPL-3.0.txt`](licenses/GPL-3.0.txt), [`LGPL-3.0.txt`](licenses/LGPL-3.0.txt). LGPL-3.0 is short
because it's legally an addendum to GPL-3.0 ("this version of the GNU Lesser General Public
License incorporates the terms and conditions of version 3 of the GNU General Public License,
supplemented by the additional permissions listed below") -- both files apply together wherever
a file is marked LGPL-3.0-or-later.

`hams_shared/` is its own separate git submodule (`.gitmodules`, tracking
`github.com/BrucePerens/hams_shared`), not just a subdirectory -- its files carry whatever license
their own headers state, same as everything else here, but its commit history and versioning are
independent of this repository's.

## Survey (as of 2026-08-25)

File counts (not header occurrences -- one file, one header) from `SPDX-License-Identifier`
across the tracked source tree (`*.py`, `*.pyi`, `*.rs`, `*.xml`, `*.js`, `*.ts`):

| License | File count | Where |
|---|---|---|
| AGPL-3.0-or-later | 275 | The default across every Odoo addon module (models, controllers, views), including `compliance/` |
| LGPL-3.0-or-later | 7 | `daemons/ham_digital_modes/` only (the Rust digital-modes decoder daemon; matches its `Cargo.toml` `license` field) |
| GPL-3.0-or-later | 4 | `hams_shared/tools/odoo_mypy_plugin.py`, its test, and `hams_shared/tools/odoo_type_stubs/odoo/*.pyi` |
| No SPDX header | 226 of 461 `.py` files | See "Unlabeled files" below |

**AGPL-3.0-or-later** is the default, and as of 2026-08-25 the *only* AGPL variant used anywhere
in this repo: this is a network-accessed Odoo application, and the AGPL's network-copyleft clause
is the deliberate choice for that reason. If a file has no header and its containing module's
other files are AGPL-3.0-or-later, treat it as AGPL-3.0-or-later -- but the correct fix, when you
touch such a file, is to add the real header rather than lean on that assumption indefinitely
(see "Unlabeled files" below).

**`compliance/` was previously AGPL-3.0-only** -- narrower than the rest of the repo on purpose,
since it handles legal/compliance pages and data. That carve-out was deliberately reversed on
2026-08-25: running a second AGPL variant for one module wasn't worth the ongoing inconsistency
it caused (headers within the same file sometimes disagreeing with each other on which variant
applied). `compliance/` is AGPL-3.0-or-later now, same as everything else.

**`daemons/ham_digital_modes/` is LGPL-3.0-or-later** -- a standalone Rust daemon (PSK31/FT8/WSPR
decoding), not an Odoo addon, and license-compatible with being linked into other programs more
permissively than AGPL would allow. Its `Cargo.toml` license field agrees.

**The mypy Odoo-awareness tooling is GPL-3.0-or-later**, not AGPL-3.0-or-later like the rest of
`hams_shared/tools/`. This was a deliberate, one-off choice, not a mistake left uncorrected: an
early draft of the accompanying type stub was assembled by directly reading a real, existing
GPL-3.0-licensed public project's stub files for reference before an original implementation was
written and the reference-derived files deleted; out of caution the *tool itself* (not just the
stub) was kept GPL-3.0-or-later rather than reverted to this repo's usual AGPL-3.0-or-later. It is
dev-time tooling (a mypy plugin, never deployed or run as a network service), so AGPL's network
clause was never relevant to it either way.

## Vendored third-party assets

`external/` hosts locally-served copies of third-party JavaScript/ML libraries, mostly fetched by
`external/fetch_assets.py` rather than committed by hand. None of these are AGPL/GPL/LGPL --
verified against each project's own published `package.json` `license` field, not assumed:

| Library | Version | License |
|---|---|---|
| Leaflet.js | 1.9.4 | BSD-2-Clause |
| Transformers.js (`@xenova/transformers`) | 2.16.1 | Apache-2.0 |
| D3.js | 7.9.0 | ISC |
| topojson-client | 3.1.0 | ISC |
| `@noble/curves`, `@noble/hashes`, `@noble/ciphers` | 2.3.0 | MIT |

See `external/README.md` for full detail per library, including a real, open gap found while
verifying this: D3.js and topojson-client aren't wired into `fetch_assets.py` like the others are,
and the currently-vendored files don't byte-match a fresh download of the same published version
-- not yet resolved.

**`daemons/ham_digital_modes/vendor/` also vendors two C libraries, missing from the table above
because that table was scoped to `external/`'s own JS/ML assets -- added here 2026-09-04 while
vendoring a second one, a real pre-existing gap for the first, not a new omission:**

| Library | License | Notes |
|---|---|---|
| `ft8_lib` | MIT (`vendor/ft8_lib/LICENSE-MIT`) | FT8/FT4 decode reference, predates this survey |
| `codec2-mod` | LGPL-2.1 (`vendor/codec2-mod/LICENSE`) | M17 project's Codec2 3200bps fork, itself derived from David Rowe's LGPL-2.1 `drowe67/codec2`; see `vendor/codec2-mod/VENDORED_FROM.md` for the exact commit, and `docs/references/CODEC2_MOD_FIXED_POINT_PLAN.md` for the fixed-point analysis it supports |

Both are compatible with this crate's own LGPL-3.0-or-later: vendoring preserves each library's own
license file unmodified rather than relicensing it, the same relationship `external/`'s table
above has with the rest of this repo's AGPL-3.0-or-later.

## Unlabeled files

Just under half of this repo's Python files (226 of 461) carry no `SPDX-License-Identifier` at
all. This is a real gap, not a second license category -- an unlabeled file's actual license is
ambiguous, not silently AGPL, even though AGPL-3.0-or-later is far and away the most likely intent
given the surrounding codebase. Only 2 of the ~25 addon modules (`hams_s3`, `ses_webhook`) even
carry a `license` key in their own `__manifest__.py`. If you're relying on a specific file's
license for anything that matters (packaging, distribution, a legal question), check that file's
own header first rather than this document's aggregate counts, and if it has none, that's worth
flagging or fixing rather than assuming.

## Adding a new file

Add an `SPDX-License-Identifier` header. Match whichever license already governs the directory
you're adding to (check a sibling file); if there isn't one, use AGPL-3.0-or-later, this
repository's default, and say so explicitly rather than leaving the file unlabeled.
