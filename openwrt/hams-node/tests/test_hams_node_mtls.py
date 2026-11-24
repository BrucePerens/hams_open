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
UCI = '#!/bin/sh\necho "$*" >> "$FAKE/uci"\ncase $* in "-q show rpcd") printf "rpcd.@rpcd[0]=rpcd\\nrpcd.cfg1=login\\nrpcd.cfg2=login\\nrpcd.hamsowner=login\\n";; "-q get uhttpd.main.listen_http") echo 0.0.0.0:80;; "-q get uhttpd.main.listen_https") echo 0.0.0.0:443;; esac\n'
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
        self.assertIn("ssl_verify_client optional_no_ca;", conf, "the verify CGI is the check, so the dates get a grace")
        self.assertNotIn("\n\t\tauth_request ", conf, "OpenWrt's nginx-ssl is built without the auth_request module")
        self.assertIn("verify?app:$ssl_client_escaped_cert;", conf)
        self.assertIn("ssl_client_certificate %s;" % (self.state / "member-ca.pem"), conf)
        self.assertIn("proxy_pass http://127.0.0.1:8080;", conf)  # burn-ignore-ssrf-test-value: expected config text on the router, nothing connects
        uci = self.uci()
        self.assertIn("add_list uhttpd.main.listen_http=127.0.0.1:8080", uci)  # burn-ignore-ssrf-test-value: expected config text on the router, nothing connects
        self.assertNotIn("0.0.0.0", uci.split("add_list")[1])
        self.assertTrue((self.state / "mtls" / "server.key").stat().st_size > 0)
        self.assertIn("hams-node-mtls crl", self.cron.read_text())

    def test_apply_sets_up_single_sign_on_and_keeps_the_stock_logins_until_the_first_sign_in(self):
        self.enrolled()
        self.assertEqual(self.run_script("apply").returncode, 0)
        conf, uci = self.conf(), self.uci()
        self.assertIn("location = /hams-sso", conf)
        self.assertIn("error_page 403 = @sso;", conf)
        self.assertIn("return 302 /hams-sso;", conf)
        self.assertIn("set rpcd.hamsowner.username=hams-owner", uci)
        self.assertIn("add_list uhttpd.hamssso.listen_http=127.0.0.1:8081", uci)  # burn-ignore-ssrf-test-value: expected config text on the router, nothing connects
        self.assertNotIn("delete rpcd.cfg1", uci)
        secret = (self.state / "mtls" / "sso.secret").read_text().strip()
        self.assertEqual(len(secret), 48)
        self.assertNotIn(secret, conf + uci)

    def test_lock_removes_every_stock_login_but_the_sso_one_and_stays_locked_on_reapply(self):
        self.enrolled()
        self.run_script("apply")
        self.assertEqual(self.run_script("lock").returncode, 0)
        uci = self.uci()
        self.assertIn("delete rpcd.cfg1", uci)
        self.assertIn("delete rpcd.cfg2", uci)
        self.assertNotIn("delete rpcd.hamsowner\n", uci.split("delete rpcd.cfg2")[1])
        self.assertTrue((self.state / "mtls" / "seen").exists())
        (self.fake / "uci").unlink()
        self.run_script("apply")
        self.assertIn("delete rpcd.cfg1", self.uci())

    def test_off_restores_the_stock_rpcd_logins(self):
        self.enrolled()
        self.run_script("apply")
        self.run_script("lock")
        (self.fake / "uci").unlink()
        self.run_script("off")
        self.assertIn("import rpcd", self.uci())
        self.assertIn("delete uhttpd.hamssso", self.uci())

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

    def test_the_owner_and_the_deprecation_list_are_read_by_the_verifier_not_baked_into_the_config(self):
        self.enrolled("K6BP/W7")
        (self.state / "deprecation_list").write_text("cert:%s:abcdef \n" % ("00" * 32))
        self.assertEqual(self.run_script("apply").returncode, 0)
        before = self.conf()
        for gone in ("K6BP", "66926", "hams_owner", "hams_revoked", "ABCDEF", "ssl_crl", "ssl_verify_client on"):
            self.assertNotIn(gone, before)
        (self.state / "deprecation_list").write_text("")
        self.run_script("apply")
        self.assertEqual(self.conf(), before, "a list change needs no nginx reload: the verify CGI reads the list on every request")

    def test_every_request_goes_through_the_verify_gate_before_the_application_without_auth_request(self):
        self.enrolled()
        self.run_script("apply")
        conf = self.conf()
        for place, target in (("location / {", "app"), ("location = /hams-sso {", "sso")):
            gate = conf.split(place)[1].split("}")[0]
            for line in ("proxy_pass $hams_verify;", "proxy_method GET;", "proxy_pass_request_body off;",
                         "set $hams_verify http://127.0.0.1:8081/cgi-bin/verify?%s:$ssl_client_escaped_cert;" % target):  # burn-ignore-ssrf-test-value: expected config text on the router, nothing connects
                self.assertIn(line, gate, place)
        self.assertNotIn("error_page 399", conf)
        self.assertIn("error_page 495 = @expired;", conf, "nginx rejects an expired certificate itself; it goes through the same gate")
        expired = conf.split("location @expired {")[1].split("}")[0]
        self.assertIn("verify?$hams_target:$ssl_client_escaped_cert;", expired)
        self.assertIn("proxy_pass_request_body off;", expired)
        app = conf.split("location @app {")[1].split("}")[0]
        self.assertIn("proxy_pass http://127.0.0.1:8080;", app)  # burn-ignore-ssrf-test-value: expected config text on the router, nothing connects
        self.assertIn("error_page 403 = @sso;", app, "LuCI's login page is replaced by the single sign-on redirect")

    def test_a_slash_callsign_is_accepted_and_a_callsign_with_other_characters_is_not(self):
        self.enrolled("K6BP/W7")
        self.assertEqual(self.run_script("apply").returncode, 0)
        self.enrolled("k6bp;touch x")
        self.assertNotEqual(self.run_script("apply").returncode, 0, "the verify CGI compares this text, so only callsign characters are allowed")

    def test_a_fresh_identity_crl_signed_by_the_pinned_ca_is_used(self):
        self.enrolled()
        (self.fake / "crl.der").write_bytes(gencrl(self.root / "ca"))
        result = self.run_script("crl")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.state / "mtls" / "crls.pem").stat().st_size > 0, "the verify CGI reads it")

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
