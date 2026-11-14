# Copyright © Bruce Perens K6BP. All Rights Reserved. This software is proprietary and confidential.
# -*- coding: utf-8 -*-
"""Every visible form control in static/src/components/log_viewer/log_viewer.xml has an accessible name: a label for= matching its id, a wrapping label, or an aria-label."""
import os

from lxml import etree

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

HIDDEN_TYPES = {"hidden", "submit", "button", "image", "reset"}


@tagged("post_install", "-at_install")
class TestLogViewerFormLabels(HamsTransactionCase):
    def test_controls_have_accessible_names(self):
        path = os.path.join(os.path.dirname(__file__), "..", "static", "src", "components", "log_viewer", "log_viewer.xml")
        root = etree.parse(path).getroot()
        label_targets = {label.get("for") for label in root.iter("label") if label.get("for")}
        checked = 0
        for el in root.iter("input", "select", "textarea"):
            if el.get("type") in HIDDEN_TYPES:
                continue
            checked += 1
            ident = el.get("name") or el.get("id") or el.tag
            if el.get("aria-label") or el.get("aria-labelledby") or el.get("t-att-aria-label") or el.get("t-attf-aria-label"):
                continue
            if any(True for _ in el.iterancestors("label")):
                continue
            if el.get("t-att-id") or el.get("t-attf-id"):
                continue  # id built at render time, its label for is built the same way
            self.assertIn(el.get("id"), label_targets, "no label for control %s" % ident)
        self.assertTrue(checked, "no controls found")
