# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for ham_dane: DANE-EE (usage 3) TLSA checks for .ham servers."""

import hashlib
import ssl
import tempfile
import unittest

import ham_dane
from test_ham_tls import make_cert


class DaneTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.TemporaryDirectory()
        cert, _ = make_cert(cls.tmp.name, "node.ham")
        cls.der = ssl.PEM_cert_to_DER_cert(open(cert).read())

    @classmethod
    def tearDownClass(cls):
        cls.tmp.cleanup()

    def test_full_certificate_sha256_record_matches(self):
        # Tests [@ANCHOR: ham_dane:tlsa_matches]
        record = (3, 0, 1, hashlib.sha256(self.der).digest())
        self.assertTrue(ham_dane.tlsa_matches(self.der, [record]))

    def test_exact_certificate_record_matches(self):
        # Tests [@ANCHOR: ham_dane:tlsa_matches]
        self.assertTrue(ham_dane.tlsa_matches(self.der, [(3, 0, 0, self.der)]))

    def test_sha512_record_matches(self):
        # Tests [@ANCHOR: ham_dane:tlsa_matches]
        self.assertTrue(ham_dane.tlsa_matches(self.der, [(3, 0, 2, hashlib.sha512(self.der).digest())]))

    def test_missing_records_refuse(self):
        # Tests [@ANCHOR: ham_dane:tlsa_matches]
        self.assertFalse(ham_dane.tlsa_matches(self.der, []))

    def test_wrong_digest_refuses(self):
        # Tests [@ANCHOR: ham_dane:tlsa_matches]
        self.assertFalse(ham_dane.tlsa_matches(self.der, [(3, 0, 1, hashlib.sha256(b"other").digest())]))

    def test_unsupported_usage_or_selector_does_not_match(self):
        # Tests [@ANCHOR: ham_dane:tlsa_matches]
        digest = hashlib.sha256(self.der).digest()
        self.assertFalse(ham_dane.tlsa_matches(self.der, [(1, 0, 1, digest), (3, 1, 1, digest)]))


if __name__ == "__main__":
    unittest.main()
