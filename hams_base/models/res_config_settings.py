# -*- coding: utf-8 -*-
from odoo import models, fields, api

class ResConfigSettings(models.TransientModel):
    _inherit = 'res.config.settings'

    compliance_org_name = fields.Char("Compliance Organization Name", config_parameter="hams_base.compliance_org_name", default="HAMS Organization")
    # Must be Char, not Text: any res.config.settings field that declares
    # config_parameter= is passed through _get_classified_fields(), which
    # only allows boolean/integer/float/char/selection/many2one/datetime
    # (see base/models/res_config.py) -- a Text field there raises
    # unconditionally on every res.config.settings.create() across the
    # WHOLE installation, not just when this field is touched. The
    # corresponding view field now sets widget="text" to keep the
    # multi-line textarea rendering a Text field got by default.
    compliance_mailing_address = fields.Char("Compliance Mailing Address", config_parameter="hams_base.compliance_mailing_address", default="123 Main St, Anytown USA")

    dns_spf_record = fields.Text("SPF Record (TXT)", compute="_compute_dns_records")
    enable_dmarc_instructions = fields.Boolean("Enable Custom DMARC", config_parameter="hams_base.enable_dmarc_instructions", default=False, help="Disable if using AWS SES or another provider that manages DMARC natively.")
    dns_dmarc_record = fields.Text("DMARC Record (TXT)", compute="_compute_dns_records")

    @api.depends('company_id')
    def _compute_dns_records(self):
        for record in self:
            domain = self.env['zero_sudo.security.utils']._get_system_param('mail.catchall.domain')
            if not domain:
                domain = self.env['zero_sudo.security.utils']._get_system_param('web.base.url') or "hams.com"
            # Strip http:// or https://
            if "://" in domain:
                domain = domain.split("://")[1].split("/")[0]
            
            record.dns_spf_record = "v=spf1 include:mailgun.org ~all  (replace include with your actual provider)"
            record.dns_dmarc_record = f"v=DMARC1; p=quarantine; rua=mailto:not-read@{domain};"
