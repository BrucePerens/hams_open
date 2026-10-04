# Copyright © HAMS project. AGPL-3.0-or-later.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase


@tagged("post_install", "-at_install")
class TestCloudflareSettingsBlock(HamsHttpCase):
    def test_03_xpath_rendering(self):
        # [@ANCHOR: test_xpath_rendering_cf_settings]

        # Tests [@ANCHOR: COMM_xpath_rendering_cf_settings]
        """Verify the Cloudflare settings block successfully injects into the global website config."""
        res = self.env["res.config.settings"].get_view(
            view_id=self.env.ref("base.res_config_settings_view_form").id,
            view_type="form",
        )
        self.assertIn(
            "cloudflare_edge",
            res["arch"],
            "The injected settings block must exist in the compiled arch.",
        )
