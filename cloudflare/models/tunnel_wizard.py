# -*- coding: utf-8 -*-
# Copyright © HAMS project. AGPL-3.0-or-later.
from odoo import api, models, fields


class CloudflareTunnelWizard(models.TransientModel):
    _name = "cloudflare.tunnel.wizard"
    _description = "Cloudflare Tunnel Setup Wizard"
    name = fields.Char(string="Name", default=lambda self: self._description)

    # [@ANCHOR: cloudflare:COMM_tunnel_wizard_command_not_persisted]
    # `command` embeds a real, one-time Cloudflare tunnel install token
    # (see action_generate_tunnel_command() in res_config_settings.py).
    # A plain stored fields.Text written via create() would land the
    # token verbatim in this model's own DB column at rest, with no
    # redaction or expiry (a plaintext secret at rest). So it is never written to a stored column at
    # all: this field is computed on the fly, purely from the
    # "install_command" key of the *action's own context* -- never a
    # DB write. The one-time token therefore only ever exists in the
    # single HTTP response that opens this wizard for the admin who
    # just generated it; re-opening this same transient record later
    # (with no "install_command" in context) renders empty, and a
    # direct SQL read of this table never has the plaintext token to
    # find, by construction -- see
    # [@ANCHOR: test_cf_tunnel_command_not_persisted].
    command = fields.Text(
        string="Installation Command",
        compute="_compute_command",
        readonly=True,
        help="Copy and execute this command in your terminal to install the Cloudflare Tunnel.",
    )

    # [@ANCHOR: cloudflare:COMM_tunnel_wizard_compute_command]
    @api.depends_context("install_command")
    def _compute_command(self):
        for wizard in self:
            wizard.command = self.env.context.get("install_command") or ""
