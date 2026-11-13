# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests.common import tagged

from .common import HamsTransactionCase
from .layout_seo import assert_layout_seo

DESCRIPTION = "x" * 150


@tagged("post_install", "-at_install")
class TestLayoutSeo(HamsTransactionCase):
    def _template(self, key, body):
        self.env["ir.ui.view"].create({
            "name": key, "key": "zero_sudo.%s" % key, "type": "qweb",
            "arch": '<t t-name="zero_sudo.%s"><t t-call="website.layout">%s<p>x</p></t></t>' % (key, body)})
        return "zero_sudo.%s" % key

    def test_layout_seo_helper_fails_without_title_or_description(self):
        # Tests [@ANCHOR: zero_sudo:assert_layout_seo]
        # Tests [@ANCHOR: zero_sudo:layout_seo_slot]
        # Tests [@ANCHOR: zero_sudo:layout_seo_slot_text]
        good = self._template("seo_ok", '<t t-set="additional_title">A page</t><t t-set="website_meta_description">%s</t>' % DESCRIPTION)
        assert_layout_seo(self, [good])
        for key, body in (
            ("seo_no_title", '<t t-set="website_meta_description">%s</t>' % DESCRIPTION),
            ("seo_no_description", '<t t-set="additional_title">A page</t>'),
            ("seo_empty_title", '<t t-set="additional_title"> </t><t t-set="website_meta_description">%s</t>' % DESCRIPTION),
            ("seo_short_description", '<t t-set="additional_title">A page</t><t t-set="website_meta_description">short</t>'),
        ):
            with self.assertRaises(AssertionError, msg=key):
                assert_layout_seo(self, [self._template(key, body)])
