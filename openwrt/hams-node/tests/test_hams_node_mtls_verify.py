# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the member-certificate verify CGI of hams-node-mtls: real openssl certificates, a stand-in for the clock."""

import os
import pathlib
import shutil
import subprocess
import tempfile
import time
import unittest
import urllib.parse

HERE = pathlib.Path(__file__).resolve().parent
VERIFY = HERE.parent / "files" / "usr" / "lib" / "hams-node" / "sso" / "cgi-bin" / "verify"
DAY = 86400
OWNER_OID = "1.3.6.1.4.1.66926.1.1"
DATE = '#!/bin/sh\ncase "$*" in "-u +%%s") echo "$NOW";; *) exec %s "$@";; esac\n' % shutil.which("date")


def ssl(*args, cwd):
    subprocess.run(["openssl", *args], cwd=cwd, check=True, capture_output=True)


def make_ca(path, name):
    path.mkdir()
    (path / "ca.cnf").write_text("[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN=%s\n[ext]\nbasicConstraints=critical,CA:true\nkeyUsage=keyCertSign,cRLSign\n" % name)
    ssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "30", "-config", "ca.cnf", "-keyout", "ca.key", "-out", "ca.pem", cwd=path)


def make_leaf(ca, callsign, serial="0x1f"):
    (ca / "leaf.cnf").write_text("oid_section=oids\n[oids]\nmembercall=%s\n[req]\ndistinguished_name=dn\nprompt=no\n[dn]\nmembercall=%s\nCN=member\n" % (OWNER_OID, callsign))
    ssl("req", "-new", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-config", "leaf.cnf", "-keyout", "leaf.key", "-out", "leaf.csr", cwd=ca)
    ssl("x509", "-req", "-in", "leaf.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-days", "365", "-set_serial", serial, "-out", "leaf-%s.pem" % callsign.replace("/", "_"), cwd=ca)
    return (ca / ("leaf-%s.pem" % callsign.replace("/", "_"))).read_text()


class VerifyTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.state = self.root / "state"
        (self.state / "mtls").mkdir(parents=True)
        (self.root / "bin").mkdir()
        (self.root / "bin" / "date").write_text(DATE)
        (self.root / "bin" / "date").chmod(0o755)
        make_ca(self.root / "ca", "Test Identity CA")
        make_ca(self.root / "other", "Other CA")
        shutil.copy(self.root / "ca" / "ca.pem", self.state / "member-ca.pem")
        (self.state / "owner").write_text("N0CALL\n")
        self.leaf = make_leaf(self.root / "ca", "N0CALL")
        self.fingerprint = subprocess.run(["openssl", "x509", "-in", str(self.root / "ca" / "ca.pem"), "-noout", "-fingerprint", "-sha256"],
                                          capture_output=True, text=True, check=True).stdout.split("=", 1)[1].strip().replace(":", "").lower()

    def verify(self, pem, days=0):
        env = dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), NOW=str(int(time.time()) + days * DAY),
                   HAMS_NODE_STATE=str(self.state), HTTP_X_CLIENT_CERT=urllib.parse.quote(pem, safe=""))
        out = subprocess.run(["sh", str(VERIFY)], env=env, capture_output=True, text=True, timeout=60).stdout
        return out.splitlines()[0]

    def test_an_in_date_owner_certificate_passes(self):
        self.assertEqual(self.verify(self.leaf), "Status: 200 OK")

    def test_ten_days_after_expiry_still_passes_and_forty_days_is_refused(self):
        self.assertEqual(self.verify(self.leaf, 365 + 10), "Status: 200 OK")
        self.assertEqual(self.verify(self.leaf, 365 + 40), "Status: 403 Forbidden")

    def test_a_clock_far_behind_passes_within_thirty_days_and_not_beyond(self):
        self.assertEqual(self.verify(self.leaf, -10), "Status: 200 OK")
        self.assertEqual(self.verify(self.leaf, -40), "Status: 403 Forbidden")

    def test_a_certificate_on_the_deprecation_list_is_refused_even_in_date(self):
        (self.state / "deprecation_list").write_text("KEYB64== 2001:db8::/60\ncert:%s:1f \n" % self.fingerprint)
        self.assertEqual(self.verify(self.leaf), "Status: 403 Forbidden")
        (self.state / "deprecation_list").write_text("cert:%s:0001F \ncert:%s:77 \n" % ("00" * 32, self.fingerprint))
        self.assertEqual(self.verify(self.leaf), "Status: 200 OK", "another issuer's entry and another serial do not match")
        (self.state / "deprecation_list").write_text("")
        self.assertEqual(self.verify(self.leaf), "Status: 200 OK", "a removal lifts the block")

    def test_a_certificate_from_another_chain_is_refused(self):
        self.assertEqual(self.verify(make_leaf(self.root / "other", "N0CALL")), "Status: 403 Forbidden")

    def test_another_members_certificate_is_refused(self):
        self.assertEqual(self.verify(make_leaf(self.root / "ca", "W1AW", "0x20")), "Status: 403 Forbidden")
        self.assertEqual(self.verify(make_leaf(self.root / "ca", "N0CALL2", "0x21")), "Status: 403 Forbidden")

    def test_no_certificate_garbage_or_not_enrolled_are_refused(self):
        for pem in ("", "x", "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n", self.leaf + self.leaf, "$(touch x)"):
            self.assertEqual(self.verify(pem), "Status: 403 Forbidden", pem)
        (self.state / "owner").unlink()
        self.assertEqual(self.verify(self.leaf), "Status: 403 Forbidden")


if __name__ == "__main__":
    unittest.main()
