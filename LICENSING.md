# Licensing

This repository is **not uniformly licensed**. Most of it is AGPL-3.0-or-later, but several
real, deliberate exceptions exist, plus some real gaps. This document exists so nobody has to
guess, or assume the majority license applies everywhere it doesn't. When in doubt, the license
header at the top of the actual file governs, not this summary.

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

## Survey (as of 2026-08-24)

File counts (not header occurrences -- one file, one header) from `SPDX-License-Identifier`
across the tracked source tree (`*.py`, `*.pyi`, `*.rs`, `*.xml`, `*.js`, `*.ts`):

| License | File count | Where |
|---|---|---|
| AGPL-3.0-or-later | 269 | The default across almost all Odoo addon modules (models, controllers, views) |
| AGPL-3.0-only | 6 | `compliance/` module only (manifest, hooks, controllers, data files) |
| LGPL-3.0-or-later | 7 | `daemons/ham_digital_modes/` only (the Rust digital-modes decoder daemon; matches its `Cargo.toml` `license` field) |
| GPL-3.0-or-later | 4 | `hams_shared/tools/odoo_mypy_plugin.py`, its test, and `hams_shared/tools/odoo_type_stubs/odoo/*.pyi` |
| No SPDX header | 226 of 461 `.py` files | See "Unlabeled files" below |

**AGPL-3.0-or-later** is the default: this is a network-accessed Odoo application, and the
AGPL's network-copyleft clause is the deliberate choice for that reason. If a file has no header
and its containing module's other files are AGPL-3.0-or-later, treat it as AGPL-3.0-or-later --
but the correct fix, when you touch such a file, is to add the real header rather than lean on
that assumption indefinitely (see "Unlabeled files" below).

**`compliance/` is AGPL-3.0-only** (not "or-later") -- narrower than the rest of the repo on
purpose. This module handles legal/compliance pages and data; treat this as intentional unless
you have a specific reason to believe otherwise, and don't casually relicense it to match the
surrounding default.

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
