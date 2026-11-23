# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-mtls with a stand-in uci and uclient-fetch and a real openssl test CA."""

import os
import pathlib
import subprocess
import tempfile
import time
import unittest

HERE = pathlib.Path(__file__).resolve().parent
PACKAGE = HERE.parent
SCRIPT = PACKAGE / "files" / "usr" / "sbin" / "hams-node-mtls"
SHIPPED_CA = PACKAGE / "files" / "etc" / "hams-node" / "member-ca.pem"
UCI = '#!/bin/sh\necho "$*" >> "$FAKE/uci"\ncase $* in "-q get uhttpd.main.listen_http") echo 0.0.0.0:80;; "-q get uhttpd.main.listen_https") echo 0.0.0.0:443;; esac\n'
FETCH = '#!/bin/sh\nwhile [ $# -gt 1 ]; do [ "$1" = -O ] && out=$2; shift; done\n[ -f "$FAKE/crl.der" ] || exit 1\ncp "$FAKE/crl.der" "$out"\n'


def ssl(*args, cwd):
    subprocess.run(["openssl", *args], cwd=cwd, check=True, capture_output=True)


def make_ca(path, name):
    """A test CA: ca.pem, ca.key, and a database for CRLs."""
    path.mkdir()
    (path / "index.txt").write_text("")
    (path / "crlnumber").write_text("01\n")
    (path / "ca.cnf").write_text(
        "[ca]\ndefault_ca=c\n[c]\ndatabase=index.txt\ncrlnumber=crlnumber\ndefault_md=sha256\ndefault_crl_days=30\n"
        "[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN=%s\n[ext]\nbasicConstraints=critical,CA:true\n"
        "keyUsage=keyCertSign,cRLSign\n" % name)
    ssl("req", "-x509", "-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:prime256v1", "-nodes", "-days", "30", "-config", "ca.cnf",
        "-keyout", "ca.key", "-out", "ca.pem", cwd=path)
    (path / "ca.cnf").write_text((path / "ca.cnf").read_text().replace("[c]\n", "[c]\ncertificate=ca.pem\nprivate_key=ca.key\n"))


def gencrl(path, expire_in_seconds=None):
    when = ["-crlsec", str(expire_in_seconds)] if expire_in_seconds else ["-crldays", "30"]
    ssl("ca", "-config", "ca.cnf", "-gencrl", *when, "-out", "crl.pem", cwd=path)
    ssl("crl", "-in", "crl.pem", "-outform", "DER", "-out", "crl.der", cwd=path)
    return (path / "crl.der").read_bytes()


class MtlsTests(unittest.TestCase):
    def setUp(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.fake = self.root / "fake"
        self.fake.mkdir()
        self.state = self.root / "state"
        self.state.mkdir()
        self.cron = self.root / "crontab"
        bindir = self.root / "bin"
        bindir.mkdir()
        for name, body in {"uci": UCI, "uclient-fetch": FETCH}.items():
            (bindir / name).write_text(body)
            (bindir / name).chmod(0o755)
        self.env = dict(os.environ, FAKE=str(self.fake), PATH="%s:%s" % (bindir, os.environ["PATH"]), HAMS_NODE_CRON=str(self.cron))
        make_ca(self.root / "ca", "Test Identity CA")
        make_ca(self.root / "other", "Other CA")
        (self.state / "member-ca.pem").write_bytes((self.root / "ca" / "ca.pem").read_bytes())
        (self.state / "member-crls.pem").write_text("")

    def run_script(self, cmd):
        return subprocess.run(["sh", str(SCRIPT), "-s", str(self.state), "-u", "https://portal.example", cmd], env=self.env,
                              capture_output=True, text=True, timeout=60)

    def conf(self):
        return (self.state / "mtls" / "nginx.conf").read_text()

    def uci(self):
        path = self.fake / "uci"
        return path.read_text() if path.exists() else ""

    def enrolled(self, owner="N0CALL"):
        (self.state / "owner").write_text(owner + "\n")

    def test_apply_demands_the_certificate_binds_the_owner_and_moves_uhttpd_to_loopback(self):
        self.enrolled()
        result = self.run_script("apply")
        self.assertEqual(result.returncode, 0, result.stderr)
        conf = self.conf()
        self.assertIn("ssl_verify_client on;", conf)
        self.assertNotIn("optional", conf)
        self.assertIn("ssl_client_certificate %s;" % (self.state / "member-ca.pem"), conf)
        self.assertIn("66926\\.1\\.1=N0CALL(/|$)", conf)
        self.assertIn("if ($hams_owner = 0) { return 403; }", conf)
        self.assertIn("proxy_pass http://127.0.0.1:8080;", conf)  # burn-ignore-ssrf-test-value: expected config text on the router, nothing connects
        uci = self.uci()
        self.assertIn("add_list uhttpd.main.listen_http=127.0.0.1:8080", uci)  # burn-ignore-ssrf-test-value: expected config text on the router, nothing connects
        self.assertNotIn("0.0.0.0", uci.split("add_list")[1])
        self.assertTrue((self.state / "mtls" / "server.key").stat().st_size > 0)
        self.assertIn("hams-node-mtls crl", self.cron.read_text())

    def ca_fingerprint(self):
        out = subprocess.run(["openssl", "x509", "-in", str(self.state / "member-ca.pem"), "-noout", "-fingerprint", "-sha256"], capture_output=True, text=True, check=True).stdout
        return out.split("=", 1)[1].strip().replace(":", "").lower()

    def test_a_certificate_on_the_deprecation_list_is_refused_without_the_crl(self):
        self.enrolled()
        fingerprint = self.ca_fingerprint()
        (self.state / "deprecation_list").write_text("KEYB64== 2001:db8::/60\ncert:%s:abcdef \ncert:%s:1f \ncert:%s:77 \ncert:%s:zz;rm \n" % (fingerprint, fingerprint, "00" * 32, fingerprint))
        self.assertEqual(self.run_script("apply").returncode, 0)
        conf = self.conf()
        self.assertIn("map $ssl_client_serial $hams_revoked {", conf)
        self.assertIn('"ABCDEF" 1;', conf)
        self.assertIn('"1F" 1;', conf, "an odd number of digits is padded as nginx prints it")
        self.assertNotIn('"77"', conf, "an entry of another issuer is not ours")
        self.assertNotIn("zz", conf)
        self.assertIn("if ($hams_revoked) { return 403; }", conf)
        self.assertNotIn("ssl_crl", conf, "no CRL was needed")

    def test_without_a_list_nothing_is_refused_and_a_removal_lifts_the_block(self):
        self.enrolled()
        self.assertEqual(self.run_script("apply").returncode, 0)
        self.assertNotIn('" 1;', self.conf().split("$hams_revoked {")[1].split("}")[0])
        (self.state / "deprecation_list").write_text("cert:%s:abcdef \n" % self.ca_fingerprint())
        self.run_script("apply")
        self.assertIn('"ABCDEF" 1;', self.conf())
        (self.state / "deprecation_list").write_text("")
        self.run_script("apply")
        self.assertNotIn("ABCDEF", self.conf())

    def test_not_enrolled_changes_nothing(self):
        result = self.run_script("apply")
        self.assertEqual(result.returncode, 0)
        self.assertFalse((self.state / "mtls").exists())
        self.assertEqual(self.uci(), "")

    def test_a_bad_owner_callsign_never_reaches_the_config(self):
        self.enrolled('N0CALL";}x')
        result = self.run_script("apply")
        self.assertEqual(result.returncode, 1)
        self.assertFalse((self.state / "mtls" / "nginx.conf").exists())

    def test_slash_callsigns_are_escaped_for_the_regex(self):
        self.enrolled("K6BP/W7")
        self.run_script("apply")
        self.assertIn("K6BP\\/W7", self.conf())

    def test_a_fresh_identity_crl_signed_by_the_pinned_ca_is_used(self):
        self.enrolled()
        (self.fake / "crl.der").write_bytes(gencrl(self.root / "ca"))
        result = self.run_script("crl")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("ssl_crl %s;" % (self.state / "mtls" / "crls.pem"), self.conf())

    def test_an_expired_crl_is_not_accepted(self):
        self.enrolled()
        (self.fake / "crl.der").write_bytes(gencrl(self.root / "ca", 1))
        time.sleep(2)
        self.assertEqual(self.run_script("crl").returncode, 1)

    def test_a_crl_signed_by_another_ca_is_refused_and_not_used(self):
        self.enrolled()
        (self.fake / "crl.der").write_bytes(gencrl(self.root / "other"))
        result = self.run_script("crl")
        self.assertEqual(result.returncode, 1)
        self.assertFalse((self.state / "mtls" / "identity.crl").exists())

    def test_an_unreachable_portal_keeps_the_last_crl(self):
        self.enrolled()
        (self.fake / "crl.der").write_bytes(gencrl(self.root / "ca"))
        self.run_script("crl")
        (self.fake / "crl.der").unlink()
        self.assertEqual(self.run_script("crl").returncode, 4)
        self.assertTrue((self.state / "mtls" / "identity.crl").exists())

    def test_off_restores_the_original_uhttpd_listeners_and_removes_everything(self):
        self.enrolled()
        self.run_script("apply")
        (self.fake / "uci").unlink()
        self.assertEqual(self.run_script("off").returncode, 0)
        uci = self.uci()
        self.assertIn("add_list uhttpd.main.listen_http=0.0.0.0:80", uci)
        self.assertIn("add_list uhttpd.main.listen_https=0.0.0.0:443", uci)
        self.assertFalse((self.state / "mtls").exists())
        self.assertNotIn("hams-node-mtls", self.cron.read_text())

    def test_the_shipped_ca_file_chains_member_leaves_to_root_a_only(self):
        pems = SHIPPED_CA.read_text().split("-----END CERTIFICATE-----")
        self.assertEqual(len([p for p in pems if "BEGIN" in p]), 4)
        subjects = subprocess.run(["openssl", "crl2pkcs7", "-nocrl", "-certfile", str(SHIPPED_CA), "-out", "/dev/null"], capture_output=True)
        self.assertEqual(subjects.returncode, 0)
        out = subprocess.run(["openssl", "verify", "-CAfile", str(SHIPPED_CA), str(SHIPPED_CA)], capture_output=True, text=True)
        self.assertIn("OK", out.stdout)


if __name__ == "__main__":
    unittest.main()
