# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Real tests for `zero_sudo/daemon/robots_txt_policy.py` -- the one shared
robots.txt verdict policy behind every crawler that fetches robots.txt through
`urlopen_ssrf_safe()`. The function is pure (no network), so these call it
directly with the exact inputs a real caller hands it.
"""

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.zero_sudo.daemon import robots_txt_policy as policy

_UA = "ExampleCrawler/1.0"
_URL = "https://club.example/events/hamfest.html"


@tagged("post_install", "-at_install")
class TestRobotsTxtPolicy(HamsTransactionCase):

    def test_a_disallow_all_body_disallows(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        body = b"User-agent: *\nDisallow: /\n"
        self.assertFalse(policy.robots_txt_verdict(_URL, _UA, body=body))

    def test_an_allow_body_allows(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        body = b"User-agent: *\nAllow: /\n"
        self.assertTrue(policy.robots_txt_verdict(_URL, _UA, body=body))

    def test_an_empty_body_allows(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        self.assertTrue(policy.robots_txt_verdict(_URL, _UA, body=b""))

    def test_a_path_specific_disallow_only_blocks_that_path(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        body = b"User-agent: *\nDisallow: /private/\n"
        self.assertTrue(policy.robots_txt_verdict(_URL, _UA, body=body))
        private_url = "https://club.example/private/roster.html"
        self.assertFalse(policy.robots_txt_verdict(private_url, _UA, body=body))

    def test_a_user_agent_specific_disallow_applies_to_that_agent_only(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        body = b"User-agent: ExampleCrawler\nDisallow: /\n\nUser-agent: *\nAllow: /\n"
        self.assertFalse(policy.robots_txt_verdict(_URL, _UA, body=body))
        self.assertTrue(policy.robots_txt_verdict(_URL, "OtherBot/2.0", body=body))

    def test_an_undecodable_byte_does_not_raise(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        body = b"# club w\xe9bmaster\nUser-agent: *\nDisallow: /\n"
        self.assertFalse(policy.robots_txt_verdict(_URL, _UA, body=body))

    def test_401_and_403_disallow_everything(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        for status in (401, 403):
            with self.subTest(status=status):
                self.assertFalse(policy.robots_txt_verdict(_URL, _UA, http_status=status))

    def test_other_4xx_allow_everything(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        for status in (400, 404, 410, 429):
            with self.subTest(status=status):
                self.assertTrue(policy.robots_txt_verdict(_URL, _UA, http_status=status))

    def test_5xx_allows_unlike_the_stdlib_read(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        # A deliberate deviation from RobotFileParser.read(), which leaves
        # can_fetch() returning False after a 5xx.
        for status in (500, 502, 503):
            with self.subTest(status=status):
                self.assertTrue(policy.robots_txt_verdict(_URL, _UA, http_status=status))

    def test_exactly_one_outcome_is_required(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_robots_txt_verdict]
        with self.assertRaises(ValueError):
            policy.robots_txt_verdict(_URL, _UA)
        with self.assertRaises(ValueError):
            policy.robots_txt_verdict(_URL, _UA, body=b"", http_status=404)

    def test_the_longest_matching_rule_wins_whatever_the_file_order(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_longest_match]
        # urllib.robotparser took the first matching rule on older Pythons and the longest on 3.13.
        for name, body, expected in (
            ("a longer Allow after a Disallow", b"User-agent: *\nDisallow: /\nAllow: /events/\n", True),
            ("the same file with the Allow first", b"User-agent: *\nAllow: /events/\nDisallow: /\n", True),
            ("a longer Disallow beats a shorter Allow", b"User-agent: *\nAllow: /events/\nDisallow: /events/hamfest\n", False),
            ("equal length: the Allow wins", b"User-agent: *\nDisallow: /events/\nAllow: /events/\n", True),
            ("a star in the pattern", b"User-agent: *\nDisallow: /*.html$\n", False),
            ("a dollar anchors the end", b"User-agent: *\nDisallow: /*.htm$\n", True),
            ("an empty Disallow allows everything", b"User-agent: *\nDisallow:\n", True),
            ("our own group replaces the wildcard group", b"User-agent: *\nDisallow: /\n\nUser-agent: ExampleCrawler\nAllow: /events/\n", True),
            ("our own group with no matching rule does not fall back to the wildcard", b"User-agent: *\nDisallow: /\n\nUser-agent: ExampleCrawler\nDisallow: /private/\n", True),
            ("the token is matched exactly, not by substring", b"User-agent: Example\nDisallow: /\n", True),
            ("consecutive User-agent lines share one group", b"User-agent: Other\nUser-agent: ExampleCrawler\nDisallow: /\n", False),
            ("percent escapes are normalised", b"User-agent: *\nDisallow: /events/ham%66est.html\n", False),
            ("a byte-order mark and CRLF line ends are read", b"\xef\xbb\xbfUser-agent: *\r\nDisallow: /\r\n", False),
            ("binary garbage allows", b"\x00\x01\x02: \x03\nUser-agent\n:::\n", True),
        ):
            with self.subTest(name):
                self.assertIs(policy.robots_txt_verdict(_URL, _UA, body=body), expected)

    def test_the_verdict_never_raises_on_odd_urls(self):
        # Tests [@ANCHOR: zero_sudo:robots_txt_policy_longest_match]
        body = b"User-agent: *\nDisallow: /\n"
        self.assertIs(policy.robots_txt_verdict("http://[bad", _UA, body=body), False)
        self.assertIs(policy.longest_match_verdict("User-agent: *\nDisallow: /\n", None, _UA), False)
