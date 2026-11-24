# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-proxyca: the name-constrained CA, the private key staying put, and the PAC file (evaluated with node)."""

import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-proxyca"
UCI = '#!/bin/sh\nkey=${3#hams_node_proxyca.main.}\ngrep "^$key=" "$FAKE/uci" | head -n 1 | cut -d= -f2-\n'
WHITELIST = "# sites\nexample.org\n*.Wiki.Example.net\n  \n"
BASE = {"proxy": "192.168.1.1:8443", "net_prefix": "2001:db8:ab00:", "net_cidr": "2001:db8:ab00::/40"}


class Node:
    def __init__(self, test, **over):
        tmp = tempfile.TemporaryDirectory()
        test.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        (self.root / "bin").mkdir()
        (self.root / "bin" / "uci").write_text(UCI)
        (self.root / "bin" / "uci").chmod(0o755)
        (self.root / "whitelist").write_text(WHITELIST)
        cfg = dict(BASE, whitelist=str(self.root / "whitelist"), **over)
        (self.root / "uci").write_text("".join("%s=%s\n" % kv for kv in cfg.items()))
        self.dir = self.root / "proxy"

    def run_cmd(self, *args):
        env = dict(os.environ, PATH="%s:%s" % (self.root / "bin", os.environ["PATH"]), FAKE=str(self.root), HAMS_PROXY_DIR=str(self.dir))
        return subprocess.run(["sh", str(SCRIPT), *args], env=env, capture_output=True, text=True)

    def leaf(self, name, ip=None):
        """A leaf certificate for NAME signed by this node's CA (what the proxy's ssl-bump would make)."""
        san = "IP:%s" % ip if ip else "DNS:%s" % name
        key, csr, crt = (self.root / n for n in ("leaf.key", "leaf.csr", "leaf.crt"))
        ext = self.root / "leaf.ext"
        ext.write_text("subjectAltName=%s\nbasicConstraints=CA:FALSE\nkeyUsage=digitalSignature\nextendedKeyUsage=serverAuth\n" % san)
        for cmd in (["openssl", "ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(key)],
                    ["openssl", "req", "-new", "-key", str(key), "-subj", "/CN=x", "-out", str(csr)],
                    ["openssl", "x509", "-req", "-in", str(csr), "-CA", str(self.dir / "ca.pem"), "-CAkey", str(self.dir / "ca.key"),
                     "-CAcreateserial", "-days", "30", "-extfile", str(ext), "-out", str(crt)]):
            subprocess.run(cmd, check=True, capture_output=True)
        return subprocess.run(["openssl", "verify", "-CAfile", str(self.dir / "ca.pem"), str(crt)], capture_output=True, text=True)

    def pac(self, host):
        out = self.root / "hams.pac"
        r = self.run_cmd("pac", str(out))
        assert r.returncode == 0, r.stderr
        js = out.read_text() + "\nconsole.log(FindProxyForURL('http://%s/', '%s'));" % (host, host)
        return subprocess.run(["node", "-e", js], capture_output=True, text=True, check=True).stdout.strip()


@unittest.skipUnless(shutil.which("openssl"), "needs the openssl command line tool")
class CaTests(unittest.TestCase):
    def test_ca_signs_only_ham_and_whitelisted_names_and_the_ham_network(self):
        n = Node(self)
        self.assertEqual(n.run_cmd("ca").returncode, 0)
        for name in ("w1aw.ham", "example.org", "www.example.org", "a.wiki.example.net"):
            r = n.leaf(name)
            self.assertEqual(r.returncode, 0, "%s: %s" % (name, r.stdout + r.stderr))
        self.assertEqual(n.leaf("x", ip="2001:db8:ab00:1::5").returncode, 0)
        for name in ("evil.com", "bank.example.com", "example.org.evil.com", "hamfake"):
            r = n.leaf(name)
            self.assertNotEqual(r.returncode, 0, "%s was accepted" % name)
            self.assertIn("permitted subtree violation", r.stdout + r.stderr)
        r = n.leaf("x", ip="2001:db8:ff00::1")
        self.assertNotEqual(r.returncode, 0, "an address outside the ham network was accepted")

    def test_the_private_key_is_private_and_never_in_the_certificate_output(self):
        n = Node(self)
        r = n.run_cmd("ca")
        self.assertNotIn("PRIVATE", r.stdout + r.stderr)
        self.assertEqual(oct((n.dir / "ca.key").stat().st_mode & 0o777), "0o600")
        cert = n.run_cmd("cert")
        self.assertEqual(cert.returncode, 0)
        self.assertIn("BEGIN CERTIFICATE", cert.stdout)
        self.assertNotIn("PRIVATE", cert.stdout + cert.stderr)
        key = (n.dir / "ca.key").read_text()
        body = "".join(key.splitlines()[1:-1])
        self.assertNotIn(body, cert.stdout.replace("\n", ""))

    def test_ca_is_kept_until_renewed(self):
        n = Node(self)
        n.run_cmd("ca")
        first = (n.dir / "ca.pem").read_text()
        n.run_cmd("ca")
        self.assertEqual((n.dir / "ca.pem").read_text(), first)
        n.run_cmd("ca", "--renew")
        self.assertNotEqual((n.dir / "ca.pem").read_text(), first)

    def test_bad_configuration_is_refused(self):
        r = Node(self, proxy="x;rm -rf /").run_cmd("ca")
        self.assertNotEqual(r.returncode, 0)


@unittest.skipUnless(shutil.which("node"), "needs node to evaluate the PAC file")
class PacTests(unittest.TestCase):
    def test_ham_names_ham_addresses_and_whitelisted_sites_go_to_the_proxy(self):
        n = Node(self)
        for host in ("w1aw.ham", "HAM", "[2001:db8:ab00:1::5]", "2001:db8:ab00:2::1", "example.org", "www.example.org", "x.wiki.example.net", "wiki.example.net"):
            self.assertEqual(n.pac(host), "PROXY 192.168.1.1:8443", host)

    def test_everything_else_is_direct(self):
        n = Node(self)
        for host in ("www.google.com", "notexample.org", "example.org.evil.com", "hamfake", "2001:db8:ff00::1", "192.168.1.5"):
            self.assertEqual(n.pac(host), "DIRECT", host)

    def test_a_missing_whitelist_leaves_only_ham(self):
        n = Node(self)
        (n.root / "whitelist").unlink()
        self.assertEqual(n.pac("example.org"), "DIRECT")
        self.assertEqual(n.pac("a.ham"), "PROXY 192.168.1.1:8443")


class DocsTests(unittest.TestCase):
    def test_install_instructions_cover_every_system_and_browser(self):
        text = (HERE.parent / "INSTALL_CA.md").read_text()
        for word in ("Windows", "macOS", "Linux", "Firefox", "Chrome", "Android"):
            self.assertIn(word, text)


if __name__ == "__main__":
    unittest.main()
