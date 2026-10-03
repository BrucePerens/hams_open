# Story: NCMEC Mandatory Child-Safety Reporting [@ANCHOR: hams_helpdesk:COMM_story_ncmec_mandatory_reporting]

As a **hams.com operator**,
I want every apparent-CSAM / online-enticement / child-trafficking flag to become a fully
assembled, fast-tracked, mandatory-report ticket automatically,
so that the federal reporting obligation (18 U.S.C. Sec. 2258A, REPORT Act) is never missed or
delayed by ordinary queue triage.

Full design context: `docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md` (hams_com), section G
/ Phase 8.

## Scenario: A flagged interaction becomes a ticket

1. The intended creation entrypoint for a flagged interaction -- what bot self-reporting and the
   Official Observer (Phase 7, developed privately in hams_com) are expected to call -- is
   idempotent by recording: a second flag on a still-open ticket for the same recording attaches a
   note to the existing ticket instead of opening a duplicate; a flag on a recording whose prior
   ticket was already closed still opens a fresh one, since a closed ticket may have been a false
   positive nobody is still watching
   `[@ANCHOR: hams_helpdesk:COMM_ncmec_report_ticket_for_recording]`.
2. Creating a CSAM-category ticket forces `priority` to the highest value unconditionally, not just
   as a default, so a caller cannot under-prioritize a mandatory-report ticket by omitting or
   overriding it `[@ANCHOR: hams_helpdesk:COMM_ncmec_force_priority]`.
3. The moment the ticket row exists, its report packet is assembled and its recording's legal hold
   is attempted, both elevated to the `hams_helpdesk.user_helpdesk_service` identity so even a
   genuinely create-only caller (e.g. Phase 7's narrow bot-reporting account) can complete
   `create()` for this ticket type
   `[@ANCHOR: hams_helpdesk:COMM_ncmec_packet_and_legal_hold]`.
4. Every `group_helpdesk_manager` member is notified immediately and unconditionally, on top of
   (never instead of) the ordinary on-duty assignment -- a mandatory federal report can't wait out
   a gap in on-duty coverage the way an ordinary ticket reasonably can
   `[@ANCHOR: hams_helpdesk:COMM_ncmec_notify_all_managers]`.

## Scenario: The report packet an admin actually reviews

5. The whole report packet (reported user, QSO recording playback URL, hams.com reporting contact)
   is built as a snapshot on the ticket itself the moment it's created, deliberately never
   recomputed later, so a later config or data change can't retroactively rewrite what an
   already-filed report actually said `[@ANCHOR: hams_helpdesk:COMM_ncmec_assemble_report_packet]`.
6. Whether the cross-repo `ham_communications_consent` recording model (hams_com) is even installed
   alongside this module is its own queryable boolean, so a hams_open-only test run can exercise
   every branch of the best-effort legal-hold flow below without that model actually being present
   `[@ANCHOR: hams_helpdesk:COMM_ncmec_recording_model_installed]`.
7. The actual cross-repo `search()` + `action_apply_legal_hold()` call is factored into its own
   method purely so it can be patched as a seam in tests -- the real production call can only be
   exercised from hams_com itself, since the target model does not exist in a hams_open-only
   deployment `[@ANCHOR: hams_helpdesk:COMM_ncmec_attempt_legal_hold_call]`.
8. Applying the legal hold "before, not after, a human reviewer confirms it" is attempted under the
   AMBIENT caller's identity (deliberately not a new hams_helpdesk service account, since the
   cross-repo grant that call needs lives in hams_com's own `ir.model.access.csv`); for most
   callers today this is expected to fail gracefully and loudly (a note on the ticket, a WARNING
   log line), with a real admin able to apply it by hand instead
   `[@ANCHOR: hams_helpdesk:COMM_ncmec_apply_recording_legal_hold_best_effort]`.

## Scenario: Filing the report itself

9. An administrator (`base.group_system`) can apply the legal hold manually when the automatic
   best-effort attempt above failed `[@ANCHOR: hams_helpdesk:COMM_ncmec_action_apply_legal_hold_manually]`,
   and the ticket form's header/page gate all of this admin-only NCMEC surface behind the same
   group, with no separate UI tour -- the server-side guards are what's actually enforced and are
   unit-tested directly `[@ANCHOR: hams_helpdesk:COMM_ncmec_report_buttons]`
   `[@ANCHOR: hams_helpdesk:COMM_ncmec_report_page]`.
10. Filing the report (`base.group_system` only) refuses outright, server-side, once the ticket is
    already `report_submitted`/`report_confirmed`, so NCMEC itself can never be double-filed for the
    same incident `[@ANCHOR: hams_helpdesk:COMM_ncmec_action_report]`.
11. With no real NCMEC API credentials configured, the already-assembled packet is emailed to a
    real human fallback contact instead of just blocking the admin with an error -- the resulting
    state is `emailed_fallback`, never `report_submitted`, so nothing here could later be mistaken
    for an actual NCMEC filing `[@ANCHOR: hams_helpdesk:COMM_ncmec_email_report_fallback]`.
12. Once real API credentials exist, the same button instead submits the packet directly to
    NCMEC's own CyberTipline Reporting API
    `[@ANCHOR: hams_helpdesk:COMM_ncmec_submit_report_via_api]`.
13. After an admin pastes the packet into NCMEC's own portal by hand (the no-credentials path),
    marking the report filed manually records that the incident has been reported, with the same
    double-file guard `action_ncmec_report` itself uses
    `[@ANCHOR: hams_helpdesk:COMM_ncmec_action_mark_report_filed_manually]`.

**Status:** Verified by the whole of `hams_helpdesk/tests/test_ncmec_report.py`, in particular
`test_csam_ticket_creation_assembles_packet_and_forces_priority`,
`test_csam_ticket_creation_notifies_every_helpdesk_manager`,
`test_legal_hold_best_effort_succeeds_when_call_succeeds`,
`test_action_ncmec_report_requires_group_system`, and
`test_action_ncmec_report_without_credentials_emails_the_fallback_contact`.
