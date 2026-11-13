# -*- coding: utf-8 -*-
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Public pages name themselves and carry a meta description (shared check: zero_sudo.tests.layout_seo)."""
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.tests.layout_seo import assert_layout_seo
from odoo.tests.common import tagged

LAYOUT_TEMPLATES = [
    "compliance.backup_policy_template",
    "compliance.certification_authority_template",
    "compliance.compliance_accessibility_statement_template",
    "compliance.compliance_cookie_policy_template",
    "compliance.compliance_index_template",
    "compliance.compliance_privacy_policy_template",
    "compliance.compliance_terms_of_service_template",
    "compliance.dmca_template",
    "compliance.intellectual_property_template",
    "compliance.protects_hams_template",
]


@tagged("post_install", "-at_install")
class TestLayoutSeo(HamsTransactionCase):
    def test_pages_have_title_and_description(self):
        assert_layout_seo(self, LAYOUT_TEMPLATES)
