#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-2.0-or-later
"""Router management by member client certificate, in the QEMU test image, with a TEST CA (never the real member CA).

  mtls_test.py fixtures DIR           write the test CA, the image overlay (owner, CA file, CRLs) and the client certificates
  mtls_test.py run IMAGE FIXTURES     boot IMAGE (built with build_test_image.sh -t FIXTURES/overlay), apply, and try the HTTPS port with openssl s_client inside the guest

Exit 0 = all checks passed. Run `run` as an agent_procs job."""

import pathlib
import subprocess
import sys
import time

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import smoke_test  # noqa: E402


def ssl(*args, cwd):
    subprocess.run(["openssl", *args], cwd=cwd, check=True, capture_output=True)


def make_ca(path, name, issuer=None):
    """A CA in path (a root when issuer is None). Returns nothing; files ca.pem, ca.key, ca.cnf, index.txt."""
    path.mkdir(parents=True)
    (path / "index.txt").write_text("")
    (path / "crlnumber").write_text("01\n")
    (path / "serial").write_text("1000\n")
    (path / "ca.cnf").write_text(
        "oid_section=oids\n[oids]\nhamsCallsign=1.3.6.1.4.1.66926.1.1\n[ca]\ndefault_ca=c\n[c]\ncertificate=ca.pem\nprivate_key=ca.key\ndatabase=index.txt\ncrlnumber=crlnumber\nserial=serial\nnew_certs_dir=.\n"
        "default_md=sha256\ndefault_days=30\ncopy_extensions=none\npolicy=p\nunique_subject=no\n[p]\nCN=optional\nhamsCallsign=optional\n"
        "[req]\ndistinguished_name=dn\nprompt=no\n[dn]\nCN=%s\n[ca_ext]\nbasicConstraints=critical,CA:true,pathlen:1\nkeyUsage=critical,keyCertSign,cRLSign\nsubjectKeyIdentifier=hash\n"
        "[sub_ext]\nbasicConstraints=critical,CA:true,pathlen:0\nkeyUsage=critical,keyCertSign,cRLSign\nsubjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid\n"
        "[leaf_ext]\nbasicConstraints=critical,CA:false\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=clientAuth\n"
        "authorityKeyIdentifier=keyid\n" % name)
    ssl("ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", "ca.key", cwd=path)
    if issuer is None:
        ssl("req", "-x509", "-new", "-key", "ca.key", "-days", "30", "-config", "ca.cnf", "-extensions", "ca_ext", "-out", "ca.pem", cwd=path)
    else:
        ssl("req", "-new", "-key", "ca.key", "-config", "ca.cnf", "-out", "ca.csr", cwd=path)
        ssl("ca", "-batch", "-config", "ca.cnf", "-extensions", "sub_ext", "-in", str(path / "ca.csr"), "-out", str(path / "ca.pem"), "-notext", cwd=issuer)


def issue(ca, outdir, name, callsign):
    """A client certificate (leaf first, then the issuing CA in the same file) with the callsign Subject attribute."""
    subj = "/CN=%s/hamsCallsign=%s" % (name, callsign)
    ssl("ecparam", "-name", "prime256v1", "-genkey", "-noout", "-out", str(outdir / (name + ".key")), cwd=ca)
    ssl("req", "-new", "-config", "ca.cnf", "-key", str(outdir / (name + ".key")), "-subj", subj, "-out", str(outdir / (name + ".csr")), cwd=ca)
    ssl("ca", "-batch", "-config", "ca.cnf", "-extensions", "leaf_ext", "-in", str(outdir / (name + ".csr")), "-out", str(outdir / (name + ".leaf")), "-notext", cwd=ca)
    (outdir / (name + ".pem")).write_bytes((outdir / (name + ".leaf")).read_bytes() + (ca / "ca.pem").read_bytes())


def crl(ca, out):
    ssl("ca", "-config", "ca.cnf", "-gencrl", "-crldays", "30", "-out", str(out), cwd=ca)


def fixtures(d):
    d = pathlib.Path(d)
    d.mkdir(parents=True, exist_ok=True)
    root, issuing, rogue_root, rogue_issuing = d / "root", d / "issuing", d / "rogue_root", d / "rogue_issuing"
    make_ca(root, "Test Root")
    make_ca(issuing, "Test Identity Issuing CA", root)
    make_ca(rogue_root, "Rogue Root")
    make_ca(rogue_issuing, "Rogue Issuing CA", rogue_root)
    clients = d / "clients"
    clients.mkdir()
    issue(issuing, clients, "owner", "N0CALL")
    issue(issuing, clients, "other", "K1ABC")
    issue(issuing, clients, "revoked", "N0CALL")
    issue(rogue_issuing, clients, "rogue", "N0CALL")
    ssl("ca", "-config", "ca.cnf", "-revoke", str(clients / "revoked.leaf"), cwd=issuing)
    guest = d / "overlay" / "root" / "clients"
    guest.mkdir(parents=True)
    for f in clients.glob("*.pem"):
        (guest / f.name).write_bytes(f.read_bytes())
    for f in clients.glob("*.key"):
        (guest / f.name).write_bytes(f.read_bytes())
    over = d / "overlay" / "etc" / "hams-node"
    (over / "mtls").mkdir(parents=True)
    (over / "owner").write_text("N0CALL\n")
    (over / "member-ca.pem").write_bytes((root / "ca.pem").read_bytes() + (issuing / "ca.pem").read_bytes())
    crl(root, over / "member-crls.pem")
    crl(issuing, over / "mtls" / "identity.crl")


def fetch(con, client, path):
    """The HTTP status line (or the TLS failure) openssl s_client in the guest gets for GET path with that client certificate."""
    tls = "-cert /root/clients/%s.pem -key /root/clients/%s.key" % (client, client) if client else ""
    res = con.run("printf 'GET %s HTTP/1.0\\r\\nHost: x\\r\\n\\r\\n' | openssl s_client -quiet -connect 127.0.0.1:443 %s 2>&1 | grep -E '^HTTP/1|alert' | head -n 3" % (path, tls), 60)
    return (res[0] if res else "").replace("\r", "")


def request(con, client, method, path, cookie="", body=""):
    """Status, Location, Set-Cookie lines and a marker for LuCI's login form, as one string, for one request with that client certificate."""
    tls = "-cert /root/clients/%s.pem -key /root/clients/%s.key" % (client, client) if client else ""
    head = "%s %s HTTP/1.0\\r\\nHost: x\\r\\n" % (method, path)
    if cookie:
        head += "Cookie: %s\\r\\n" % cookie
    if body:
        head += "Content-Type: application/x-www-form-urlencoded\\r\\nContent-Length: %d\\r\\n" % len(body)
    res = con.run("printf '%s\\r\\n%s' | openssl s_client -quiet -connect 127.0.0.1:443 %s 2>&1 | grep -i -E '^HTTP/1|^Location|^Set-Cookie|luci_password|alert' | cut -c1-200" % (head, body, tls), 60)
    return (res[0] if res else "").replace("\r", "")


def run(image, d):
    d = pathlib.Path(d)
    con = smoke_test.Console(image)
    failures = []

    def check(name, ok, detail=""):
        print("%s %s %s" % ("PASS" if ok else "FAIL", name, detail))
        if not ok:
            failures.append(name)

    try:
        if con.wait_for(lambda b: smoke_test.BOOT_PROMPT in b or None, 300) is None:
            print("FAIL boot: no console prompt")
            return 1
        con.send("\n")
        time.sleep(2)
        con.run("for i in $(seq 40); do /etc/init.d/uhttpd running && break; sleep 3; done; sleep 5", 200)  # boot settle
        res = con.run("/usr/sbin/hams-node-mtls apply 2>&1; echo rc=$?; sleep 5; /etc/init.d/hams-node-mtls status", 120)
        out = res[0] if res else ""
        check("hams-node-mtls apply", bool(res) and "rc=0" in out and "running" in out, out.strip().replace("\r\n", " | ")[-300:])
        res = con.run("netstat -tln | grep -E ':(80|8080|443) '", 30)
        out = res[0] if res else ""
        check("uhttpd only on loopback, nginx on 443", "127.0.0.1:8080" in out and "0.0.0.0:80 " not in out and ":443" in out, out.strip().replace("\r\n", " | "))
        static = "/luci-static/resources/view/hams-node/status.js"
        out = fetch(con, "owner", static)
        check("owner certificate is served", "HTTP/1.1 200" in out, out.strip().replace("\n", " | ")[:160])
        out = fetch(con, "other", static)
        check("another member's certificate gets 403", "HTTP/1.1 403" in out, out.strip().replace("\n", " | ")[:160])
        for name, text in (("", "no certificate"), ("revoked", "revoked certificate"), ("rogue", "certificate from another CA")):
            out = fetch(con, name, static)
            check("%s is refused (400 or a TLS alert, never 200)" % text, "HTTP/1.1 200" not in out, out.strip().replace("\n", " | ")[:160])
        # single sign-on: the verified owner reaches LuCI without a password; nobody else does
        con.run("h=$(openssl passwd -1 rootpw); sed -i \"s|^root:[^:]*|root:$h|\" /etc/shadow", 30)
        page = "/cgi-bin/luci/admin/status/overview"
        out = request(con, "owner", "GET", page)
        check("owner without a session is sent to sign-in", "302" in out and "/hams-sso" in out, out.strip().replace("\n", " | ")[:200])
        out = request(con, "owner", "GET", "/hams-sso")
        cookie = [c.split(":", 1)[1].split(";")[0].strip() for c in out.splitlines() if c.lower().startswith("set-cookie: sysauth_http")]
        check("owner sign-in sets the LuCI session cookie", "302" in out and bool(cookie), out.strip().replace("\n", " | ")[:200])
        out = request(con, "owner", "GET", page, cookie[0] if cookie else "x=y")
        check("owner opens LuCI with no password prompt", "HTTP/1.1 200" in out and "luci_password" not in out, out.strip().replace("\n", " | ")[:200])
        for name, text in (("other", "another member's certificate"), ("", "no certificate"), ("rogue", "a certificate from another CA")):
            out = request(con, name, "GET", "/hams-sso")
            check("sign-in with %s is refused" % text, "Set-Cookie" not in out and "HTTP/1.1 302" not in out, out.strip().replace("\n", " | ")[:160])
        out = request(con, "other", "GET", page, cookie[0] if cookie else "x=y")
        check("a stolen session cookie without the owner certificate is refused", "HTTP/1.1 200" not in out, out.strip().replace("\n", " | ")[:160])
        res = con.run("ubus call session login '{\"username\":\"root\",\"password\":\"rootpw\"}' >/dev/null 2>&1; echo rpcd_root_rc=$?", 30)
        out = res[0] if res else ""
        check("root password login is refused once the owner has signed in", "rpcd_root_rc=0" not in out and "rpcd_root_rc=" in out, out.strip())
        out = request(con, "owner", "POST", "/cgi-bin/luci", "", "luci_username=root&luci_password=rootpw")
        check("LuCI password form does not log in", "Set-Cookie" not in out, out.strip().replace("\n", " | ")[:200])
        res = con.run("/usr/sbin/hams-node-mtls off 2>&1; echo rc=$?; sleep 3; netstat -tln | grep -E ':80 '", 60)
        out = res[0] if res else ""
        check("off restores uhttpd on port 80", bool(res) and "rc=0" in out and ":80 " in out, out.strip().replace("\r\n", " | ")[-200:])
        res = con.run("ubus call session login '{\"username\":\"root\",\"password\":\"rootpw\"}' >/dev/null 2>&1; echo rpcd_root_rc=$?", 30)
        check("off brings the root password login back", "rpcd_root_rc=0" in (res[0] if res else ""), res[0].strip() if res else "")
    finally:
        con.close()
    return 1 if failures else 0


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "fixtures":
        fixtures(sys.argv[2])
    elif len(sys.argv) == 4 and sys.argv[1] == "run":
        sys.exit(run(sys.argv[2], sys.argv[3]))
    else:
        sys.exit(__doc__)
