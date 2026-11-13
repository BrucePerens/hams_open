# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free boundary tests of `zero_sudo.css_sanitizer`, added to kill the mutation survivors of the second
mutation pass (2026-11-13): size and count limits at their exact edges, quote and parenthesis balance, nesting depth,
the characters that stop a scan, and whitespace normalisation."""
from odoo.tests.common import BaseCase, tagged

from odoo.addons.zero_sudo import css_sanitizer as css

SHEET = css.sanitize_stylesheet
ATTRIBUTE = css.sanitize_style_attribute


@tagged("post_install", "-at_install")
class TestCssSanitizerBoundaries(BaseCase):
    # Tests [@ANCHOR: zero_sudo:css_stylesheet_filter]
    # Tests [@ANCHOR: zero_sudo:css_style_attribute_filter]

    def test_the_scans_treat_capital_x_and_other_letters_as_ordinary_text(self):
        self.assertEqual(SHEET(".aXb{font-family:Xyz}"), (".aXb{font-family:Xyz}", False))
        self.assertEqual(ATTRIBUTE("font-family:Xyz;color:red"), ("font-family:Xyz;color:red", False))

    def test_nested_parentheses_hide_a_semicolon_until_the_outer_one_closes(self):
        self.assertEqual(SHEET("a{width:f(g(1);2)}"), ("a{width:f(g(1);2)}", False))
        self.assertEqual(ATTRIBUTE("width:f(g(1);2)"), ("width:f(g(1);2)", False))

    def test_a_statement_at_rule_is_dropped_and_the_next_rule_survives(self):
        self.assertEqual(SHEET('@import "x";a{color:red}'), ("a{color:red}", True))
        self.assertEqual(SHEET('@charset "x";@import "y";a{color:red}'), ("a{color:red}", True))

    def test_whitespace_is_normalised_in_values_selectors_and_media_conditions(self):
        self.assertEqual(SHEET("a{margin:1px   2px\n\t3px}"), ("a{margin:1px 2px 3px}", False))
        self.assertEqual(SHEET("a    b  >  c{color:red}"), ("a b > c{color:red}", False))
        self.assertEqual(SHEET("@media   screen   and  (min-width:1px){a{color:red}}"),
                         ("@media screen and (min-width:1px){a{color:red}}", False))
        self.assertEqual(ATTRIBUTE("margin:1px   2px"), ("margin:1px 2px", False))

    def test_a_value_may_be_500_characters_and_not_501(self):
        self.assertEqual(SHEET("a{font-family:%s}" % ("a" * 500)), ("a{font-family:%s}" % ("a" * 500), False))
        self.assertEqual(SHEET("a{font-family:%s}" % ("a" * 501)), ("", True))
        self.assertEqual(SHEET("a{color:}"), ("", True))

    def test_unbalanced_quotes_and_parentheses_drop_the_declaration(self):
        for bad in ('content:"it\'s"', "content:'say \"hi'", "width:f(1px", "width:f(1px))"):
            self.assertEqual(SHEET("a{color:red;%s}" % bad), ("a{color:red}", True), bad)
        self.assertEqual(SHEET("a{content:\"a'b\";color:red}"), ("a{color:red}", True))  # one apostrophe inside a string is odd
        self.assertEqual(SHEET("a{content:\"a''b\";color:red}"), ("a{content:\"a''b\";color:red}", False))
        self.assertEqual(SHEET("a{content:'a\"\"b';color:red}"), ("a{content:'a\"\"b';color:red}", False))
        self.assertEqual(SHEET("a{width:f(1px) g(2px)}"), ("a{width:f(1px) g(2px)}", False))

    def test_the_attribute_with_unbalanced_quotes_is_dropped_whole(self):
        for bad in ("font-family:'a", 'font-family:"a', "font-family:'a'b'", 'font-family:"a"b"', "width:f(1px", "width:f(1px))",
                    "content:\"{\"", "content:\"}\""):
            self.assertEqual(ATTRIBUTE("color:red;" + bad), ("", True), bad)

    def test_media_nesting_stops_at_depth_two(self):
        two = "@media screen{@media print{x{color:red}}}"
        self.assertEqual(SHEET(two), ("@media screen{@media print{x{color:red}}}", False))
        three = "@media screen{@media print{@media tv{x{color:red}}}}"
        self.assertEqual(SHEET(three), ("", True))
        self.assertEqual(SHEET("@media screen{x{color:red}@media print{@media tv{y{color:red}}}}"),
                         ("@media screen{x{color:red}}", True))

    def test_an_at_rule_with_nothing_left_inside_leaves_nothing(self):
        self.assertEqual(SHEET("@media screen{a{color:url(http://e/x)}}"), ("", True))
        self.assertEqual(SHEET("@keyframes k{from{background:url(http://e/x)}}"), ("", True))

    def test_other_block_at_rules_and_misplaced_keyframes_are_dropped(self):
        self.assertEqual(SHEET("@font-face{font-family:x}"), ("", True))
        self.assertEqual(SHEET("@supports (a:b){a{color:red}}"), ("", True))
        self.assertEqual(SHEET("@media screen{@keyframes k{from{color:red}}}"), ("", True))
        self.assertEqual(SHEET("@keyframes k{@keyframes j{from{color:red}}}"), ("", True))
        self.assertEqual(SHEET("@keyframes k{@media screen{from{color:red}}}"), ("", True))

    def test_keyframe_blocks_accept_only_keyframe_selectors(self):
        self.assertEqual(SHEET("@keyframes k{from{color:red}50%{color:blue}to{color:green}}"),
                         ("@keyframes k{from{color:red}\n50%{color:blue}\nto{color:green}}", False))
        self.assertEqual(SHEET("@-webkit-keyframes k{from{color:red}}"), ("@keyframes k{from{color:red}}", False))
        self.assertEqual(SHEET("@keyframes k{.a{color:red}from{color:blue}}"), ("@keyframes k{from{color:blue}}", True))
        self.assertEqual(SHEET(".a{color:red}from{color:blue}"), (".a{color:red}\nfrom{color:blue}", False))

    def test_a_rule_whose_body_holds_a_brace_is_dropped_whole(self):
        self.assertEqual(SHEET("a{color:red;b{x:y}}"), ("", True))
        self.assertEqual(SHEET('a{content:"}";color:red}'), ("", True))
        self.assertEqual(SHEET('a{content:"{";color:red}'), ("", True))

    def test_an_empty_rule_is_dropped_silently_unless_it_held_something(self):
        self.assertEqual(SHEET("a{}"), ("", False))
        self.assertEqual(SHEET("a{ \n }"), ("", False))
        self.assertEqual(SHEET("a{;}"), ("", True))
        self.assertEqual(SHEET("a{;;color:red}"), ("a{color:red}", False))

    def test_nothing_in_nothing_out(self):
        for empty in (None, "", "   \n\t"):
            self.assertEqual(SHEET(empty), ("", False))
            self.assertEqual(ATTRIBUTE(empty), ("", False))

    def test_the_sheet_size_limit_is_exact(self):
        body = "a{color:red}"
        edge = body + "/*" + "x" * (css.MAX_SHEET_CHARS - len(body) - 4) + "*/"
        self.assertEqual(len(edge), css.MAX_SHEET_CHARS)
        self.assertEqual(SHEET(edge), (body, False))
        self.assertEqual(SHEET(edge + " "), ("", True))

    def test_the_attribute_size_limit_is_exact(self):
        body = "color:red"
        edge = body + "/*" + "x" * (css.MAX_ATTRIBUTE_CHARS - len(body) - 4) + "*/"
        self.assertEqual(len(edge), css.MAX_ATTRIBUTE_CHARS)
        self.assertEqual(ATTRIBUTE(edge), (body, False))
        self.assertEqual(ATTRIBUTE(edge + " "), ("", True))

    def test_comment_termination_is_judged_on_the_text_after_the_opener(self):
        self.assertEqual(SHEET("/**/a{color:red}"), ("a{color:red}", False))
        self.assertEqual(SHEET("/*x*/a{color:red}"), ("a{color:red}", False))
        self.assertEqual(SHEET("a{color:red}/*/"), ("a{color:red}", True))
        self.assertEqual(SHEET("a{color:red}/*"), ("a{color:red}", True))
        self.assertEqual(ATTRIBUTE("color:red/**/"), ("color:red", False))
        self.assertEqual(ATTRIBUTE("color:red/*/"), ("", True))
        self.assertEqual(ATTRIBUTE("color:red/*"), ("", True))

    def test_a_control_character_anywhere_refuses_the_whole_sheet(self):
        for char in ("\x01", "\x0b", "\x7f", "￾", "￿", "\x00"):
            self.assertEqual(SHEET("a{color:red}%sb{color:blue}" % char), ("", True), repr(char))
            self.assertEqual(ATTRIBUTE("color:red;%s" % char), ("", True), repr(char))

    def test_braces_in_an_attribute_refuse_the_whole_attribute(self):
        self.assertEqual(ATTRIBUTE('color:red;content:"{"'), ("", True))
        self.assertEqual(ATTRIBUTE('color:red;content:"}"'), ("", True))

    def test_at_most_200_declarations_per_rule_and_per_attribute(self):
        many = ";".join("p-%d:1" % n for n in range(201))
        out, dropped = SHEET("a{%s}" % many)
        self.assertTrue(dropped)
        self.assertEqual(out.count(":"), 200)
        self.assertNotIn("p-200", out)
        exact = ";".join("p-%d:1" % n for n in range(200))
        self.assertEqual(SHEET("a{%s}" % exact), ("a{%s}" % exact, False))
        out, dropped = ATTRIBUTE(many)
        self.assertTrue(dropped)
        self.assertEqual(out.count(":"), 200)
        self.assertEqual(ATTRIBUTE(exact), (exact, False))

    def test_bad_properties_are_dropped_in_an_attribute_too(self):
        self.assertEqual(ATTRIBUTE("-moz-binding:url(#a);color:red"), ("", True))  # a leading single dash is no property name at all
        self.assertEqual(ATTRIBUTE("behavior:url(#a);color:red"), ("color:red", True))
        self.assertEqual(SHEET("a{-ms-behavior:url(#a);color:red}"), ("a{color:red}", True))

    def test_at_most_2000_rules_are_kept(self):
        self.assertEqual(SHEET("a{color:red}" * 2000), ("\n".join(["a{color:red}"] * 2000), False))
        self.assertEqual(SHEET("a{color:red}" * 2001), ("\n".join(["a{color:red}"] * 2000), True))
        self.assertEqual(SHEET("a{color:red}\nb{color:blue}"), ("a{color:red}\nb{color:blue}", False))
