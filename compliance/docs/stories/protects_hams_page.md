# Story: How Hams.com Protects Hams

## User Persona
A prospective or existing member wondering what hams.com actually does, technically, to protect
their credentials, their station, and their data -- not a marketing claim, a concrete answer.

## Scenario
A visitor navigates to `/protects-hams`, a single, central, public (no login required) page that
indexes the real, technical protections already documented elsewhere on the site -- LoTW (Logbook of The World) trust
and custody (`/lotw-trust`; an ARRL (American Radio Relay League) Logbook of the World certificate is used only to become a
Verified Ham or add a licence, never to sign in), transmitter safety (`/transmitter-safety`), and the relay daemon's
published source (`/relay/source`) -- rather than re-explaining each one.

## Story
1. The visitor lands on `/protects-hams`, reachable without an account, registered in the shared
   `compliance.document` registry alongside Privacy/Terms/LoTW Trust/Transmitter Safety so it
   surfaces via `/compliance` like every other real trust page.
2. The page links out to the existing, real trust pages named above (`/lotw-trust`, `/transmitter-safety`, and the relay source at `/relay/source`) rather than duplicating their
   content, so each protection has exactly one canonical explanation.
   *(Reference: [@ANCHOR: compliance:protects_hams_page])*
   The page is the QWeb template `protects_hams_template` in `data/protects_hams_data.xml`, with a published `website.page` at `/protects-hams` and a `compliance.document` row so `/compliance` lists it. Its sections are LoTW certificate custody, that the hams.com service can never remotely control a transmitter, the disclosed relay source, signed releases and reproducible builds, how a revoked daemon version warns instead of being disabled, the backup and ransomware posture, and a closing summary. It is public static text with no controller and no arguments. Test: `test_protects_hams_page_reachable_and_registered`.
3. When this repository edits the page, the edit reaches an already-running site on the next
   `-u compliance`, not only fresh installs. The page's template sits outside the data file's
   `noupdate` block, and the 1.1 pre-migration clears the stale `noupdate` flag that the original
   install left on its `ir_model_data` row (hams.com's own production site missed the 2026-09-23
   ransomware section for this reason).
   *(Reference: [@ANCHOR: compliance:clear_stale_template_noupdate_flags])*
   The migration is `migrate(cr, version)` in `migrations/1.1/pre-migration.py`: `cr` is the upgrade cursor and `version` the installed version (empty on a fresh install, when it returns at once). It runs one static `UPDATE ir_model_data SET noupdate = false` for the xmlid `protects_hams_template` of module `compliance` where the flag is still set, so it is idempotent and returns `None`. It lists only records that sit outside a `noupdate` block; clearing the flag of the page record or document row would let an upgrade overwrite site-owner edits. Test: `tests/test_noupdate_migration.py`.
