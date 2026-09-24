# This software is distributed under the terms of the Affero General Public License (AGPL-3).

from odoo import fields, models


class HelpdeskTicket(models.Model):
    _inherit = "hams_helpdesk.ticket"

    # docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md (hams_com), section G, Phase 7 item 1
    # ("bot self-reporting"): an LLM safety-classification pass over bot QSO transcripts
    # (daemons/hams_simulated_bots, hams_com) flags self_harm, sexual_content, or uncivil
    # interactions -- see hams_com's safety_classifier.py SafetyClassification for the exact
    # four-category shape. The fourth category, csam_enticement_trafficking, already has its own
    # distinct, faster-SLA ticket_type and mandatory NCMEC-reporting workflow directly in
    # hams_helpdesk itself (see _CSAM_TICKET_TYPE there) -- that one is a core, legally-mandated
    # hams_helpdesk feature, not an extension of one, which is why it lives in the base selection
    # rather than here.
    #
    # This is deliberately ONE combined category covering all three non-CSAM concerns, not three
    # separate ticket_type values: the proposal's own section G text contrasts the CSAM
    # category's distinct routing against "ordinary incivility/general-concern flags" for the
    # other three as a group, not as three individually-routed categories. The transcript excerpt
    # and recording-playback-link text a caller wants a human to see goes in the ticket's
    # existing `description` (Html) field -- no new structured fields are needed for this
    # category, unlike the elaborate CSAM/NCMEC packet fields.
    #
    # selection_add, never a bare selection= -- see ham_repeater_dir/models/helpdesk_ticket.py's
    # own comment (hams_com) for the bug that convention exists to prevent (a second module's
    # `selection=` silently replacing the base field's whole list, so whichever module loads
    # last wins and the other's value disappears from the installed database).
    # Verified by [@ANCHOR: test_ai_safety_concern_ticket_type_survives_alongside_other_modules]
    ticket_type = fields.Selection(
        selection_add=[
            (
                "ai_safety_concern",
                "AI Safety Concern: Self-Harm / Sexual Content / Uncivil Interaction",
            )
        ],
        ondelete={"ai_safety_concern": "set default"},
    )
