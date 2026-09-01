# -*- coding: utf-8 -*-
from odoo.tests import common, tagged


@tagged("post_install", "-at_install")
class TestViewRenderingAuditCluster(common.TransactionCase):
    """
    Real, honest test coverage for four audit-ignore-view claims that
    named tests which didn't exist anywhere (view_dmarc_record_tree,
    view_dmarc_report_form, view_dmarc_report_tree,
    res_config_settings_view_form) -- caught by verify_anchors.py's
    ADR-0054 check once that gate was actually re-enabled (previously
    silently disabled, see hams_shared commit 1309d42). Each test
    renders the real view via get_view(), matching this codebase's own
    established test_all_xpaths_render convention elsewhere.
    """

    def test_dmarc_record_tree_view_renders(self):
        # Tests [@ANCHOR: view_dmarc_record_tree]
        list_view = self.env["hams_base.dmarc.record"].get_view(
            view_id=self.env.ref("hams_base.view_dmarc_record_tree").id
        )
        arch = list_view.get("arch")
        self.assertIn("dkim_alignment", arch)
        self.assertIn("spf_alignment", arch)

    def test_dmarc_report_form_view_renders(self):
        # Tests [@ANCHOR: view_dmarc_report_form]
        form_view = self.env["hams_base.dmarc.report"].get_view(
            view_id=self.env.ref("hams_base.view_dmarc_report_form").id
        )
        arch = form_view.get("arch")
        self.assertIn("record_ids", arch)
        self.assertIn("oe_chatter", arch)

    def test_dmarc_report_tree_view_renders(self):
        # Tests [@ANCHOR: view_dmarc_report_tree]
        list_view = self.env["hams_base.dmarc.report"].get_view(
            view_id=self.env.ref("hams_base.view_dmarc_report_tree").id
        )
        arch = list_view.get("arch")
        self.assertIn("org_name", arch)
        self.assertIn("domain", arch)

    def test_res_config_settings_view_form_renders(self):
        # Tests [@ANCHOR: res_config_settings_view_form]
        settings_view = self.env["res.config.settings"].get_view(
            view_id=self.env.ref("base.res_config_settings_view_form").id
        )
        arch = settings_view.get("arch")
        self.assertIn("hams_base_compliance", arch)
        self.assertIn("compliance_org_name", arch)
