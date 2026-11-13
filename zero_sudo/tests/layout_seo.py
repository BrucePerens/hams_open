# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Shared test helper: every public page that calls website.layout names itself (a browser-tab and search-result title)
and summarises itself (a meta description of 150 to 160 characters when it is plain text)."""
from lxml import etree

TITLE_NAMES = ("additional_title", "title")
DESCRIPTION_NAME = "website_meta_description"


# [@ANCHOR: zero_sudo:layout_seo_slot]
def _slot(call, names):
    for child in call:
        if child.tag == "t" and child.get("t-set") in names:
            return child
    return None


# [@ANCHOR: zero_sudo:layout_seo_slot_text]
def _slot_text(slot):
    """The literal text of a slot, or None when its value is an expression computed per request."""
    if slot.get("t-value"):
        return None
    return " ".join("".join(slot.itertext()).split())


# [@ANCHOR: zero_sudo:assert_layout_seo]
def assert_layout_seo(test, xmlids):
    """For each template xmlid, every website.layout call sets a non-empty title and meta description slot.

    A plain-text description is also checked for the 150 to 160 character length the site uses. Descriptions and titles
    taken from a record (t-value) are only checked to be non-empty expressions."""
    views = {v.key: v for v in test.env["ir.ui.view"].search([("key", "in", list(xmlids))])}
    for xmlid in xmlids:
        test.assertIn(xmlid, views, "%s is not a view" % xmlid)
        arch = etree.fromstring(views[xmlid].arch_db.encode())
        calls = arch.xpath("//t[@t-call='website.layout']")
        test.assertTrue(calls, "%s has no website.layout call" % xmlid)
        for call in calls:
            title = _slot(call, TITLE_NAMES)
            test.assertIsNotNone(title, "%s sets no page title" % xmlid)
            test.assertTrue(title.get("t-value") or _slot_text(title), "%s has an empty page title" % xmlid)
            description = _slot(call, (DESCRIPTION_NAME,))
            test.assertIsNotNone(description, "%s sets no meta description" % xmlid)
            test.assertTrue(description.get("t-value") or _slot_text(description), "%s has an empty meta description" % xmlid)
            text = _slot_text(description)
            if text is not None:
                test.assertTrue(150 <= len(text) <= 160, "%s meta description is %d characters, not 150 to 160" % (xmlid, len(text)))
