# SPDX-License-Identifier: AGPL-3.0-or-later
"""Database-free edge tests of the form_shield token guards (tokens.py): what a forged, malformed or oversized visitor
response must be refused for, and the exact limits. Written to kill the survivors of a mutation run of tokens.py."""
import hashlib
import hmac

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from .. import tokens

KEY = "unit-test-signing-key"
TOKEN = "v1.pow.1000.1600.16.00112233aabbccdd.0123456789abcdef.0000000000000000000000000000000000000000"
FORM = "contact"
COUNTER = 23661


def _sign(key, body):
    return hmac.new(key.encode(), body.encode(), hashlib.sha256).hexdigest()[:40]


@tagged("post_install", "-at_install", "form_shield")
class TestTokenGuards(HamsTransactionCase):
    def test_ip_tag_and_challenge_shape(self):
        # Tests [@ANCHOR: form_shield:ip_tag]
        # Tests [@ANCHOR: form_shield:ip_tag_hash]
        # Tests [@ANCHOR: form_shield:make_challenge]
        full = hmac.new(KEY.encode(), b"ip|203.0.113.0/24", hashlib.sha256).hexdigest()
        self.assertEqual(tokens.ip_tag(KEY, "203.0.113.9"), full[:16])
        token = tokens.make_challenge(KEY, "203.0.113.9", 1000, 12, tokens.KIND_POW, 600)
        fields = token.split(".")
        self.assertEqual(len(fields), 8)
        self.assertEqual(len(fields[5]), 16)
        self.assertEqual(fields[6], full[:16])
        self.assertEqual(len(fields[7]), 40)
        self.assertEqual(fields[7], _sign(KEY, ".".join(fields[:7])))
        self.assertNotEqual(token, tokens.make_challenge(KEY, "203.0.113.9", 1000, 12, tokens.KIND_POW, 600))

    def test_ip_prefix_networks(self):
        # Tests [@ANCHOR: form_shield:ip_tag]
        self.assertEqual(tokens.ip_prefix("  203.0.113.9 "), "203.0.113.0/24")
        self.assertEqual(tokens.ip_prefix("2001:db8:1:ffff::1"), "2001:db8:1::/48")
        # An IPv4 address seen through a dual-stack socket keeps its own /24 rather than all sharing "::/48".
        self.assertEqual(tokens.ip_prefix("::ffff:203.0.113.9"), "203.0.113.0/24")
        self.assertEqual(tokens.ip_prefix("::1"), "::/48")
        self.assertEqual(tokens.ip_prefix(""), "none")
        self.assertEqual(tokens.ip_prefix("203.0.113.9/24"), "none")

    def test_split_response_limits(self):
        # Tests [@ANCHOR: form_shield:split_response]
        self.assertEqual(tokens.split_response("abc~42"), ("abc", "42"))
        self.assertEqual(tokens.split_response("  abc~ 42  "), ("abc", "42"))
        self.assertEqual(tokens.split_response("abc~"), ("abc", ""))
        self.assertEqual(tokens.split_response("abc~x~y"), ("abc", "x~y"))
        for bad in (None, "", "   ", "abc", "~42"):
            self.assertEqual(tokens.split_response(bad), (None, None), repr(bad))
        # 400 characters (after trimming) is the longest accepted response, 401 is refused.
        self.assertEqual(tokens.split_response("a" * 395 + "~" + "1234"), ("a" * 395, "1234"))
        self.assertEqual(tokens.split_response("a" * 396 + "~" + "1234"), (None, None))

    def test_parse_challenge_refusals_and_fields(self):
        # Tests [@ANCHOR: form_shield:parse_challenge]
        # Tests [@ANCHOR: form_shield:parsed_challenge]
        token = tokens.make_challenge(KEY, "203.0.113.9", 1000, 12, tokens.KIND_QUESTION, 600)
        parsed, reason = tokens.parse_challenge(KEY, token)
        fields = token.split(".")
        self.assertIsNone(reason)
        self.assertEqual((parsed.token, parsed.kind, parsed.nonce, parsed.tag), (token, "q", fields[5], fields[6]))
        self.assertEqual((parsed.issued, parsed.expires, parsed.difficulty), (1000, 1600, 12))
        for bad in (None, "", "v1", ".".join(fields[:7]), token + ".x", token + "."):
            self.assertEqual(tokens.parse_challenge(KEY, bad), (None, tokens.BAD_FORMAT), repr(bad))
        # Another version is a format error even when it is signed with the right key.
        other = ".".join(["v2"] + fields[1:7])
        self.assertEqual(tokens.parse_challenge(KEY, other + "." + _sign(KEY, other)), (None, tokens.BAD_FORMAT))
        # A wrong key and a non-ASCII signature are a bad signature (no exception), never a pass.
        self.assertEqual(tokens.parse_challenge("other", token), (None, tokens.BAD_SIGNATURE))
        self.assertEqual(tokens.parse_challenge(KEY, ".".join(fields[:7]) + "." + "é" * 40), (None, tokens.BAD_SIGNATURE))
        # A signature holding a lone surrogate (not encodable) is a bad signature too, not an exception.
        self.assertEqual(tokens.parse_challenge(KEY, ".".join(fields[:7]) + "." + "\ud800" * 40), (None, tokens.BAD_SIGNATURE))
        # A correctly signed token whose numbers are not numbers is refused as a bad format, one field at a time.
        for index in (2, 3, 4):
            broken = list(fields[:7])
            broken[index] = "x"
            body = ".".join(broken)
            self.assertEqual(tokens.parse_challenge(KEY, body + "." + _sign(KEY, body)), (None, tokens.BAD_FORMAT), index)

    def test_pow_edges(self):
        # Tests [@ANCHOR: form_shield:leading_zero_bits]
        # Tests [@ANCHOR: form_shield:pow_valid]
        # Tests [@ANCHOR: form_shield:solve_pow]
        self.assertEqual(tokens.leading_zero_bits(b"\x80"), 0)
        self.assertEqual(tokens.leading_zero_bits(b"\x00\x01"), 15)
        self.assertEqual(tokens.leading_zero_bits(b"\x00\x00"), 16)
        bits = tokens.leading_zero_bits(tokens.pow_digest(TOKEN, FORM, COUNTER))
        self.assertTrue(tokens.pow_valid(TOKEN, FORM, COUNTER, bits))
        self.assertFalse(tokens.pow_valid(TOKEN, FORM, COUNTER, bits + 1))
        # The counter is a plain decimal: an int is accepted, a padded, signed, non-ASCII or 13-digit one is not.
        self.assertTrue(tokens.pow_valid(TOKEN, FORM, str(COUNTER), 0))
        for bad in ("0" + str(COUNTER), "00", "-1", "+1", "1.5", "", " 1", "٣", "1" * 13):
            self.assertIs(tokens.pow_valid(TOKEN, FORM, bad, 0), False, repr(bad))
        self.assertIs(tokens.pow_valid(TOKEN, FORM, "1" * 12, 0), True)
        self.assertIs(tokens.pow_valid(TOKEN, FORM, "0", 0), True)
        # solve_pow starts at `start`, and finds the smallest counter from there.
        self.assertEqual(tokens.solve_pow(TOKEN, FORM, 0), 0)
        self.assertEqual(tokens.solve_pow(TOKEN, FORM, 0, start=7), 7)
        self.assertEqual(tokens.solve_pow(TOKEN, FORM, 16, start=COUNTER), COUNTER)
        later = tokens.solve_pow(TOKEN, FORM, 16, start=COUNTER + 1)
        self.assertGreater(later, COUNTER)
        self.assertTrue(tokens.pow_valid(TOKEN, FORM, later, 16))
        # Nothing between `start` and the answer solves it (the search does not skip a counter).
        self.assertFalse(any(tokens.pow_valid(TOKEN, FORM, c, 16) for c in range(COUNTER + 1, later)))

    def test_question_edges(self):
        # Tests [@ANCHOR: form_shield:question_numbers]
        # Tests [@ANCHOR: form_shield:question_text]
        # Tests [@ANCHOR: form_shield:question_valid]
        self.assertEqual(tokens.question_numbers("0000" + "ff" * 6), (2, 2))
        self.assertEqual(tokens.question_numbers("ff00" + "ff" * 6), (2 + 255 % 9, 2))
        self.assertEqual(tokens.question_numbers("00ff" + "00" * 6), (2, 2 + 255 % 9))
        self.assertEqual(tokens.question_numbers("0800" + "00" * 6), (10, 2))
        self.assertEqual(tokens.question_text("0803" + "00" * 6), "What is ten plus five? Type your answer as digits.")
        nonce = "0803" + "00" * 6
        self.assertIs(tokens.question_valid(nonce, "15"), True)
        self.assertIs(tokens.question_valid("zz", "4"), False)
        for bad in (None, "", "1 5", "015", "16", "14", "fifteen"):
            self.assertIs(tokens.question_valid(nonce, bad), False, repr(bad))
