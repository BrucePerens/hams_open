# Story: How Hams.com Protects Hams

## User Persona
A prospective or existing member wondering what hams.com actually does, technically, to protect
their credentials, their station, and their data -- not a marketing claim, a concrete answer.

## Scenario
A visitor navigates to `/protects-hams`, a single, central, public (no login required) page that
indexes the real, technical protections already documented elsewhere on the site -- LoTW trust
and custody (`/lotw-trust`), transmitter safety (`/transmitter-safety`), and the relay daemon's
published source (`/relay/source`) -- rather than re-explaining each one.

## Story
1. The visitor lands on `/protects-hams`, reachable without an account, registered in the shared
   `compliance.document` registry alongside Privacy/Terms/LoTW Trust/Transmitter Safety so it
   surfaces via `/compliance` like every other real trust page.
2. The page links out to the two existing, real trust pages rather than duplicating their
   content, so each protection has exactly one canonical explanation.
   *(Reference: [@ANCHOR: compliance:protects_hams_page])*
3. When this repository edits the page, the edit reaches an already-running site on the next
   `-u compliance`, not only fresh installs. The page's template sits outside the data file's
   `noupdate` block, and the 1.1 pre-migration clears the stale `noupdate` flag that the original
   install left on its `ir_model_data` row (hams.com's own production site missed the 2026-09-23
   ransomware section for this reason).
   *(Reference: [@ANCHOR: compliance:clear_stale_template_noupdate_flags])*
