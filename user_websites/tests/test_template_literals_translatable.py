# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Visible English must not hide inside a QWeb expression.

Odoo translates template text and literal title/aria-label/alt/placeholder attributes, never a string
literal inside t-out or t-esc. `<t t-out="'Published' if x else 'Draft'"/>` shows English whatever the
member's language. Write the words as text (`<t t-if="x">Published</t><t t-else="">Draft</t>`).
Tests [@ANCHOR: user_websites:template_literals_translatable]
"""
import glob
import os
import re

import lxml.etree as etree

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.modules import get_module_path
from odoo.tests.common import tagged

_EXPR_ATTRS = ("t-out", "t-esc", "t-att-title", "t-att-aria-label", "t-att-placeholder", "t-att-alt", "t-att-label")
_WORDS = re.compile(r"""['"][A-Z][a-z]+(?: [a-z]+)*['"]""")
_CHOICE = re.compile(r"""\sif\s|\sor\s['"]|['"][A-Z][a-z]+ [a-z]+""")


def literal_findings(root):
    found = []
    for path in sorted(glob.glob(os.path.join(root, "*", "views", "**", "*.xml"), recursive=True)):
        try:
            tree = etree.parse(path)
        except etree.XMLSyntaxError:
            continue
        for el in tree.iter():
            if not isinstance(el.tag, str):
                continue
            for attr in _EXPR_ATTRS:
                value = el.get(attr)
                if value and re.findall(_WORDS, value) and re.findall(_CHOICE, value):
                    found.append("%s:%s %s=%s" % (os.path.relpath(path, root), el.sourceline, attr, value))
    return found


@tagged("post_install", "-at_install")
class TestTemplateLiteralsTranslatable(HamsTransactionCase):
    def test_no_english_literal_inside_expressions(self):
        root = os.path.dirname(get_module_path("user_websites"))
        self.assertEqual(literal_findings(root), [], "visible English inside a QWeb expression is never translated")
