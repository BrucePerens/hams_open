# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged

from .. import tokens

KEY = "unit-test-signing-key"
# The vector tests/form_shield.test.js (and so static/src/pow/pow_core.js) must reproduce.
VECTOR_TOKEN = (
    "v1.pow.1000.1600.16.00112233aabbccdd.0123456789abcdef."
    "0000000000000000000000000000000000000000"
)
VECTOR_FORM = "contact"
VECTOR_DIFFICULTY = 16
VECTOR_COUNTER = 23661
VECTOR_DIFFICULTY_20_COUNTER = 1843084


@tagged("post_install", "-at_install", "form_shield")
class TestTokens(HamsTransactionCase):
    # [@ANCHOR: form_shield:test_pow_vector_matches_the_browser_solver]
    def test_pow_vector_matches_the_browser_solver(self):
        # Tests [@ANCHOR: form_shield:leading_zero_bits]
        # Tests [@ANCHOR: form_shield:pow_digest]
        # Tests [@ANCHOR: form_shield:solve_pow]
        # Tests [@ANCHOR: form_shield:pow_valid]
        self.assertEqual(
            tokens.solve_pow(VECTOR_TOKEN, VECTOR_FORM, VECTOR_DIFFICULTY),
            VECTOR_COUNTER,
        )
        self.assertTrue(
            tokens.pow_valid(VECTOR_TOKEN, VECTOR_FORM, VECTOR_COUNTER, VECTOR_DIFFICULTY)
        )
        self.assertEqual(
            tokens.solve_pow(VECTOR_TOKEN, VECTOR_FORM, 20), VECTOR_DIFFICULTY_20_COUNTER
        )

    def test_pow_is_bound_to_the_form_and_the_challenge(self):
        # Tests [@ANCHOR: form_shield:pow_valid]
        args = (VECTOR_COUNTER, VECTOR_DIFFICULTY)
        self.assertFalse(tokens.pow_valid(VECTOR_TOKEN, "report_violation", *args))
        other = VECTOR_TOKEN.replace("00112233", "00112234")
        self.assertFalse(tokens.pow_valid(other, VECTOR_FORM, *args))

    def test_pow_counter_must_be_canonical_decimal(self):
        # Tests [@ANCHOR: form_shield:pow_valid]
        for bad in ("023661", " 23661", "23661 ", "-23661", "２３６６１", "2.3e4", "", "x" * 30):
            self.assertFalse(
                tokens.pow_valid(VECTOR_TOKEN, VECTOR_FORM, bad, VECTOR_DIFFICULTY), repr(bad)
            )
        self.assertTrue(
            tokens.pow_valid(VECTOR_TOKEN, VECTOR_FORM, str(VECTOR_COUNTER), VECTOR_DIFFICULTY)
        )

    def test_leading_zero_bits(self):
        self.assertEqual(tokens.leading_zero_bits(bytes([0xFF] + [0] * 31)), 0)
        self.assertEqual(tokens.leading_zero_bits(bytes([0x01] + [0] * 31)), 7)
        self.assertEqual(tokens.leading_zero_bits(bytes([0, 0x80] + [0] * 30)), 8)
        self.assertEqual(tokens.leading_zero_bits(bytes(32)), 256)

    # [@ANCHOR: form_shield:test_tampered_challenge_is_refused]
    def test_tampered_challenge_is_refused(self):
        # Tests [@ANCHOR: form_shield:mac]
        # Tests [@ANCHOR: form_shield:make_challenge]
        # Tests [@ANCHOR: form_shield:parsed_challenge]
        # Tests [@ANCHOR: form_shield:parse_challenge]
        token = tokens.make_challenge(KEY, "203.0.113.9", 1000, 12, tokens.KIND_POW, 600)
        parsed, reason = tokens.parse_challenge(KEY, token)
        self.assertIsNone(reason)
        self.assertEqual((parsed.issued, parsed.expires, parsed.difficulty), (1000, 1600, 12))
        fields = token.split(".")
        # Changing any signed field, the signature, or the key breaks it.
        for index, replacement in ((1, "q"), (2, "1"), (3, "9999999999"), (4, "1"), (5, "ab" * 8), (6, "0" * 16), (7, "0" * 40)):
            forged = list(fields)
            forged[index] = replacement
            self.assertEqual(
                tokens.parse_challenge(KEY, ".".join(forged))[1], tokens.BAD_SIGNATURE, index
            )
        self.assertEqual(tokens.parse_challenge("another key", token)[1], tokens.BAD_SIGNATURE)
        for malformed in ("", "v1", "v2." + ".".join(fields[1:]), ".".join(fields[:-1]), token + ".x"):
            self.assertEqual(tokens.parse_challenge(KEY, malformed)[1], tokens.BAD_FORMAT, malformed)

    # [@ANCHOR: form_shield:test_ip_tag_uses_prefix_and_keeps_no_address]
    def test_ip_tag_uses_prefix_and_keeps_no_address(self):
        # Tests [@ANCHOR: form_shield:ip_tag_hash]
        # Tests [@ANCHOR: form_shield:ip_tag]
        self.assertEqual(tokens.ip_prefix("203.0.113.9"), "203.0.113.0/24")
        self.assertEqual(tokens.ip_prefix("2001:db8:1:2:3:4:5:6"), "2001:db8:1::/48")
        self.assertEqual(tokens.ip_prefix(None), "none")
        self.assertEqual(tokens.ip_prefix("not an address"), "none")
        same = tokens.ip_tag(KEY, "203.0.113.9") == tokens.ip_tag(KEY, "203.0.113.200")
        self.assertTrue(same)
        self.assertNotEqual(tokens.ip_tag(KEY, "203.0.113.9"), tokens.ip_tag(KEY, "203.0.114.9"))
        self.assertNotEqual(tokens.ip_tag(KEY, "203.0.113.9"), tokens.ip_tag("other", "203.0.113.9"))
        token = tokens.make_challenge(KEY, "203.0.113.9", 1000, 12, tokens.KIND_POW, 600)
        self.assertNotIn("203.0.113", token)

    # [@ANCHOR: form_shield:test_question_fallback_accepts_the_right_answer]
    def test_question_fallback_accepts_the_right_answer(self):
        # Tests [@ANCHOR: form_shield:question_numbers]
        # Tests [@ANCHOR: form_shield:question_text]
        # Tests [@ANCHOR: form_shield:question_valid]
        nonce = "0a0b0c0d0e0f0102"
        first, second = tokens.question_numbers(nonce)
        self.assertEqual((first, second), (2 + 10 % 9, 2 + 11 % 9))
        self.assertTrue(tokens.question_valid(nonce, str(first + second)))
        self.assertTrue(tokens.question_valid(nonce, " %d " % (first + second)))
        self.assertFalse(tokens.question_valid(nonce, str(first + second + 1)))
        self.assertFalse(tokens.question_valid(nonce, ""))
        self.assertFalse(tokens.question_valid("zz", "4"))
        text = tokens.question_text(nonce)
        self.assertIn("plus", text)
        self.assertNotIn(str(first + second), text)
