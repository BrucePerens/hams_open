# -*- coding: utf-8 -*-
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
from odoo.tests import tagged
from odoo.tools.babel import extract_python

from odoo.addons.hams_base.models import res_users
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestTranslationExtraction(HamsTransactionCase):
    def test_01_the_email_change_alert_texts_are_extracted(self):
        # Tests [@ANCHOR: hams_base:COMM_res_users_write]
        # Odoo's exporter finds a text only when the call is spelled `_(` or `env._(` right at the call; a call through
        # an alias (`t = env._` then `t('...')`) is not found and the text never reaches a translator.
        with open(res_users.__file__, "rb") as source:
            found = {message for _line, _func, message, _comments in extract_python(source, {"_": None, "_lt": None}, [], {})}
        self.assertIn("Security Alert: Your HAMS Email Address was Changed", found)
        self.assertTrue(any(isinstance(m, str) and m.startswith("<p>Hello %s,</p>") for m in found))
