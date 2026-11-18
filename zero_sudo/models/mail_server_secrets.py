# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Outgoing mail server passwords served from the secret store (group `mail` of the host option
`hams_module_secrets_from_store`, see module_secrets.py).

With the group on, the SMTP password of `ir.mail_server` record N is the store secret `mail.smtp_pass.N`; the
`smtp_pass` column is never written (a non-empty value is refused) and a rollout step empties it. With the
group off nothing here changes Odoo's behaviour and the store is never read. Story:
zero_sudo/docs/stories/secret_store_tier_a.md ("Module secrets").
"""

from odoo import api, models
from odoo.exceptions import UserError

from odoo.addons.zero_sudo.models.module_secrets import group_enabled, store_secret

STORE_GROUP = "mail"


# [@ANCHOR: zero_sudo:mail_server_store_name]
def smtp_store_name(server_id):
    """The store name of mail server `server_id`'s SMTP password: `mail.smtp_pass.<id>`."""
    return f"mail.smtp_pass.{int(server_id)}"


# [@ANCHOR: zero_sudo_mail_server_secrets_ir_mail_server]
class IrMailServerSecrets(models.Model):
    _inherit = "ir.mail_server"

    # [@ANCHOR: zero_sudo:mail_server_smtp_login]
    def _smtp_login__(self, connection, smtp_user, smtp_password):
        """With the group on and a stored server, logs in with the store's password for this server (SecretNotFound
        when the store lacks it: a mail server with a user and no password must fail loudly). A call without a
        record (a CLI-configured server), a server that does not authenticate by user name and password (an
        OAuth server such as Gmail or Outlook has a user and no password) and every call with the group off use
        Odoo's own login."""
        if self and self.smtp_authentication == "login" and group_enabled(STORE_GROUP) and smtp_user:
            smtp_password = store_secret(smtp_store_name(self.id))
        return super()._smtp_login__(connection, smtp_user, smtp_password)

    # [@ANCHOR: zero_sudo:mail_server_refuse_typed]
    @api.model
    def _refuse_typed_smtp_pass(self, vals):
        if group_enabled(STORE_GROUP) and vals.get("smtp_pass"):
            raise UserError(self.env._(
                "SMTP passwords are served from the secret store on this host (hams_module_secrets_from_store). "
                "Put it in the store first, name mail.smtp_pass.<server id>."))

    # [@ANCHOR: zero_sudo:mail_server_create]
    @api.model_create_multi
    def create(self, vals_list):
        for vals in vals_list:
            self._refuse_typed_smtp_pass(vals)
        return super().create(vals_list)

    # [@ANCHOR: zero_sudo:mail_server_write]
    def write(self, vals):
        self._refuse_typed_smtp_pass(vals)
        return super().write(vals)

    # [@ANCHOR: zero_sudo:mail_server_clear_from_database]
    @api.model
    def _hams_mail_clear_smtp_pass_from_database(self):
        """Rollout step after the flip: empties `smtp_pass` on every server that has one. Writes nothing unless the
        group is on and the store holds `mail.smtp_pass.<id>` for every such server. Returns the server ids."""
        if not group_enabled(STORE_GROUP):
            raise UserError(self.env._("List the group mail in hams_module_secrets_from_store first."))
        servers = self.with_context(active_test=False).search([("smtp_pass", "!=", False)], limit=10000)
        for server in servers:
            store_secret(smtp_store_name(server.id))
        servers.write({"smtp_pass": False})
        return sorted(servers.ids)
