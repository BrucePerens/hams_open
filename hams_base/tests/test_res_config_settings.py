# -*- coding: utf-8 -*-
from odoo.tests import common, tagged


@tagged("post_install", "-at_install")
class TestResConfigSettings(common.TransactionCase):
    def test_settings_create_and_default_value(self):
        """res.config.settings is one merged model shared by every
        installed module -- a config_parameter= field here with a type
        _get_classified_fields() doesn't support (only boolean/integer/
        float/char/selection/many2one/datetime are allowed) breaks
        settings.create() for the ENTIRE installation, not just this
        module. Regression test for exactly this: compliance_mailing_address
        was declared fields.Text and broke every res.config.settings.create()
        anywhere hams_base is installed, caught incidentally by an
        unrelated module's own settings test."""
        settings = self.env["res.config.settings"].create({})
        self.assertEqual(
            settings.compliance_mailing_address,
            "123 Main St, Anytown USA",
            "[!] DIAGNOSTIC FOR AI: compliance_mailing_address default value did not load onto a newly created res.config.settings record.",
        )
