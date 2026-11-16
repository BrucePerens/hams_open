# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""hams_helpdesk 1.2: the portal rule no longer lets a member read a mandatory-report ticket.

The rule record is noupdate, so an installed database keeps the old domain; the ticket of a mandatory
report has the REPORTED member as its partner, and that member must not see it. Idempotent."""

_DOMAIN = (
    "[('company_id', 'in', company_ids), ('partner_id', '=', user.partner_id.id), "
    "('ticket_type', '!=', 'csam_enticement_trafficking'), "
    "'|', ('website_id', '=', False), ('website_id', '=', user.website_id.id)]"
)


def migrate(cr, version):
    if not version:
        return
    cr.execute(
        "UPDATE ir_rule SET domain_force = %s WHERE id IN (SELECT res_id FROM ir_model_data "
        "WHERE module = 'hams_helpdesk' AND name = 'rule_helpdesk_ticket_portal' AND model = 'ir.rule')",
        (_DOMAIN,),
    )
