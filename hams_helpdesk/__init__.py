# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

import logging

from . import controllers
from . import models

_logger = logging.getLogger(__name__)


def post_init_hook(env):
    """Claim the "info" mail alias for hams_helpdesk.ticket, but only if nothing
    else already has. Found live 2026-08-29 installing this module alongside crm
    for the first time: crm ships its own built-in "info" alias on the default
    Sales Team (crm/data/crm_team_data.xml), and mail.alias.alias_name is
    globally unique, so a plain data/mail_alias_data.xml <record> for "info"
    hard-crashes this module's own install the instant crm is present. Whichever
    module claims it first wins; if that's not this one, log it rather than fail.
    """
    if env["mail.alias"].search_count([("alias_name", "=", "info")]):
        _logger.warning(
            "hams_helpdesk: the 'info' mail alias already belongs to another "
            "record (e.g. crm's default Sales Team); info@hams.com will NOT "
            "route to hams_helpdesk.ticket. Only 'support' and 'admin' do."
        )
        return
    env["mail.alias"].create(
        {
            "alias_name": "info",
            "alias_model_id": env.ref("hams_helpdesk.model_hams_helpdesk_ticket").id,
            "alias_contact": "everyone",
            "alias_defaults": "{'priority': '1'}",
        }
    )
