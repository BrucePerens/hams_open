# This software is distributed under the terms of the Affero General Public License (AGPL-3).
{
    "name": "Hams AI Safety Reporting",
    "version": "1.0",
    "category": "Operations/Helpdesk",
    "summary": "Ticket category and narrow service account for AI safety-classification self-reporting.",
    "description": """
        docs/proposals/CHILD_SAFETY_COMMUNICATIONS_CONSENT.md (hams_com), section G, Phase 7
        item 1 ("bot self-reporting"): adds a general AI-safety-concern ticket_type to
        hams_helpdesk.ticket (self-harm / sexual content / uncivil interaction -- distinct from
        the CSAM/enticement/trafficking mandatory-report category, which is a core hams_helpdesk
        feature, not an extension of one, and already lives directly in that module) and a
        narrow, create-only service account for whatever caller files these reports. Today that
        caller is an LLM safety-classification daemon over bot QSO transcripts
        (daemons/hams_simulated_bots, hams_com); Phase 7 item 2's "Official Observer" agent is
        expected to reuse the same reporting path later, which is why the group/account are
        named generically rather than after "bot".

        Lives in hams_open (not alongside ham_repeater_dir's own equivalent extension in
        hams_com) because this feature's Odoo-side pieces are open per the task that created it,
        even though it follows exactly the same _inherit + selection_add pattern ham_repeater_dir
        already establishes for extending hams_helpdesk.ticket from a satellite module.
    """,
    "author": "Bruce Perens K6BP",
    "website": "https://perens.com/",
    "license": "AGPL-3",
    "depends": ["hams_helpdesk"],
    "data": [
        "security/security.xml",
        "security/ir.model.access.csv",
    ],
    "installable": True,
}
