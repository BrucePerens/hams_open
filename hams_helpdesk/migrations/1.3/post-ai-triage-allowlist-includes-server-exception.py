# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""hams_helpdesk 1.3: the AI triage allow-list rule on an installed database gets the current domain.

The rule record is noupdate, so a database installed before server_exception and the suspicious
clause were added kept the old domain (general and hams_local_relay only): the triage service
account could not see a New server_exception ticket, and --count-pending answered 0 for it.
Idempotent."""

DOMAIN = (
    "[('company_id', 'in', company_ids), ('ticket_type', 'in', ['general', 'hams_local_relay', "
    "'server_exception']), ('suspicious', '=', False)]"
)


def migrate(cr, version):
    if not version:
        return
    cr.execute(
        "UPDATE ir_rule SET domain_force = %s WHERE id IN (SELECT res_id FROM ir_model_data "
        "WHERE module = 'hams_helpdesk' AND name = 'rule_helpdesk_ticket_ai_triage_external_allowlist' "
        "AND model = 'ir.rule')",
        (DOMAIN,),
    )
