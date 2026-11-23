# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for hams-node-ondemand with recording stand-ins for wg, uclient-fetch, nft and date."""

import json
import os
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "sbin" / "hams-node-ondemand"
KEY_A = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
KEY_B = "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="


def record(prefix, key=KEY_A, status="success"):
    if status != "success":
        return {"result": {"status": "error", "message": "not found"}}
    return {"result": {"status": "success", "prefix60": prefix, "public_key": key, "endpoint": "[2001:db8::9]:51820"}}


class Harness:
    def __init__(self, test):
        tmp = tempfile.TemporaryDirectory()
        test.addCleanup(tmp.cleanup)
        self.root = pathlib.Path(tmp.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "calls"
        self.state = self.root / "state"
        self.now = 1000
        self.stub("wg", 'echo "wg $*" >> "%s"' % self.log)
        self.stub("date", 'echo "$HAMS_NOW"')
        self.replies = {}
        self.active = []
        self.set_lookup({})

    def stub(self, name, body):
        path = self.bin / name
        path.write_text("#!/bin/sh\n%s\n" % body)
        path.chmod(0o755)

    def set_lookup(self, replies):
        """replies: query address -> reply dict (anything else gets 'not found')."""
        (self.root / "replies.json").write_text(json.dumps(replies))
        self.stub("uclient-fetch", 'echo "lookup $*" >> "%s"\n'
                  'python3 -I -c "import json,sys;a=sys.argv[1];d=json.load(open(sys.argv[2]));'
                  'print(json.dumps(d.get(json.loads(a)[\'params\'][\'query\'],{\'result\':{\'status\':\'error\'}})))" '
                  '"$(echo \"$*\" | sed \'s/.*--post-data //; s/ http.*//\')" "%s"' % (self.log, self.root / "replies.json"))

    def set_active(self, prefixes):
        """The /60s in the nft 'seen' set."""
        self.stub("nft", 'echo "elements = { %s }"' % ", ".join(prefixes))

    def run(self, *args, link="internet", iface="wg-hams", max_peers=None):
        cmd = ["sh", str(SCRIPT), "-i", iface, "-t", link, "-u", "https://x/lookup", "-s", str(self.state)]
        if max_peers:
            cmd += ["-m", str(max_peers)]
        env = dict(os.environ, PATH="%s:%s:%s" % (self.bin, HERE, os.environ["PATH"]), HAMS_NOW=str(self.now),
                   HAMS_NODE_DEPRECATION_LIST=str(self.root / "deprecation_list"))
        return subprocess.run(cmd + list(args), env=env, capture_output=True, text=True)

    def wg_calls(self):
        return [c[3:] for c in (self.log.read_text().splitlines() if self.log.exists() else []) if c.startswith("wg ")]

    def lookups(self):
        return [c for c in (self.log.read_text().splitlines() if self.log.exists() else []) if c.startswith("lookup ")]


class OnDemandTests(unittest.TestCase):
    def test_first_packet_creates_peer_then_no_second_lookup(self):
        h = Harness(self)
        h.set_lookup({"2001:db8:1:20::5": record("2001:db8:1:20::/60")})
        self.assertEqual(h.run("packet", "2001:db8:1:20::5").returncode, 0)
        self.assertEqual(h.wg_calls(), ["set wg-hams peer %s endpoint [2001:db8::9]:51820 allowed-ips 2001:db8:1:20::/60" % KEY_A])
        h.run("packet", "2001:db8:1:2f::77")  # same /60, different spelling and host part
        self.assertEqual(len(h.wg_calls()), 1)
        self.assertEqual(len(h.lookups()), 1)

    def test_unknown_or_suspended_creates_nothing_and_is_not_asked_again_for_a_minute(self):
        h = Harness(self)
        self.assertEqual(h.run("packet", "2001:db8:1:30::1").returncode, 0)
        self.assertEqual(h.wg_calls(), [])
        h.now += 30
        h.run("packet", "2001:db8:1:30::1")
        self.assertEqual(len(h.lookups()), 1)
        h.now += 31
        h.run("packet", "2001:db8:1:30::1")
        self.assertEqual(len(h.lookups()), 2)

    def test_cached_directory_entry_is_used_when_hams_com_is_unreachable(self):
        h = Harness(self)
        h.set_lookup({"2001:db8:1:20::5": record("2001:db8:1:20::/60")})
        h.run("packet", "2001:db8:1:20::5")
        h.set_active([])
        h.run("expire")  # idle: the peer is removed
        h.stub("uclient-fetch", "exit 1")  # internet gone
        h.now += 120
        self.assertEqual(h.run("packet", "2001:db8:1:20::5").returncode, 0)
        self.assertEqual(h.wg_calls()[-1], "set wg-hams peer %s endpoint [2001:db8::9]:51820 allowed-ips 2001:db8:1:20::/60" % KEY_A)

    def test_an_explicit_not_found_answer_drops_the_cached_entry(self):
        h = Harness(self)
        h.set_lookup({"2001:db8:1:20::5": record("2001:db8:1:20::/60")})
        h.run("packet", "2001:db8:1:20::5")
        h.set_active([])
        h.run("expire")
        h.set_lookup({})  # hams.com answers: no such member
        h.now += 120
        h.run("packet", "2001:db8:1:20::5")
        h.stub("uclient-fetch", "exit 1")
        h.now += 120
        h.run("packet", "2001:db8:1:20::5")
        self.assertEqual(len(h.wg_calls()), 2)  # set, remove, and nothing more

    def test_a_peer_the_directory_no_longer_returns_is_evicted_even_while_busy(self):
        h = Harness(self)
        h.set_lookup({"2001:db8:1:20::5": record("2001:db8:1:20::/60", KEY_A)})
        h.run("packet", "2001:db8:1:20::5")
        h.set_active(["2001:db8:1:20::"])
        h.now += 100
        h.set_lookup({})  # suspended or disabled meanwhile: the lookup says not found
        h.run("expire")
        self.assertEqual(h.wg_calls()[1:], [], "checked only every five minutes")
        h.now += 250
        h.run("expire")
        self.assertEqual(h.wg_calls()[1:], ["set wg-hams peer %s remove" % KEY_A], "busy, but no longer in the directory: evicted")

    def test_a_busy_peer_still_in_the_directory_stays_and_a_changed_key_goes(self):
        h = Harness(self)
        h.set_lookup({"2001:db8:1:20::5": record("2001:db8:1:20::/60", KEY_A), "2001:db8:1:20::": record("2001:db8:1:20::/60", KEY_A)})
        h.run("packet", "2001:db8:1:20::5")
        h.set_active(["2001:db8:1:20::"])
        h.now += 400
        h.run("expire")
        self.assertEqual(len(h.wg_calls()), 1, "still returned: stays")
        h.set_lookup({"2001:db8:1:20::": record("2001:db8:1:20::/60", KEY_B)})
        h.now += 400
        h.run("expire")
        self.assertEqual(h.wg_calls()[1:], ["set wg-hams peer %s remove" % KEY_A], "the member re-keyed: the old tunnel goes")

    def test_a_deprecated_key_gets_no_new_tunnel_and_an_existing_one_is_dropped(self):
        h = Harness(self)
        h.set_lookup({"2001:db8:1:20::5": record("2001:db8:1:20::/60", KEY_A)})
        (h.root / "deprecation_list").write_text("%s 2001:db8:1:20::/60\n" % KEY_A)
        h.run("packet", "2001:db8:1:20::5")
        self.assertEqual(h.wg_calls(), [], "refused although the directory returned it")
        (h.root / "deprecation_list").unlink()
        h.now += 61  # past the one-minute memory of the refusal
        h.run("packet", "2001:db8:1:20::5")
        h.now += 100
        h.set_active(["2001:db8:1:20::"])
        self.assertEqual(h.wg_calls(), ["set wg-hams peer %s endpoint [2001:db8::9]:51820 allowed-ips 2001:db8:1:20::/60" % KEY_A])
        (h.root / "deprecation_list").write_text("%s 2001:db8:1:20::/60\n" % KEY_A)
        h.run("expire")
        self.assertEqual(h.wg_calls()[1:], ["set wg-hams peer %s remove" % KEY_A])

    def test_record_for_a_different_prefix_is_refused(self):
        h = Harness(self)
        h.set_lookup({"2001:db8:1:20::5": record("2001:db8:9:90::/60")})
        h.run("packet", "2001:db8:1:20::5")
        self.assertEqual(h.wg_calls(), [])

    def test_idle_peer_is_removed_active_peer_stays(self):
        h = Harness(self)
        h.set_lookup({"2001:db8:1:20::5": record("2001:db8:1:20::/60", KEY_A), "2001:db8:1:30::5": record("2001:db8:1:30::/60", KEY_B)})
        h.run("packet", "2001:db8:1:20::5")
        h.run("packet", "2001:db8:1:30::5")
        h.set_active(["2001:db8:1:30::"])
        self.assertEqual(h.run("expire").returncode, 0)
        self.assertEqual(h.wg_calls()[2:], ["set wg-hams peer %s remove" % KEY_A])
        h.set_active([])
        h.run("expire")
        self.assertEqual(h.wg_calls()[3:], ["set wg-hams peer %s remove" % KEY_B])

    def test_table_is_bounded_and_the_oldest_idle_peer_goes_first(self):
        h = Harness(self)
        prefixes = {"2001:db8:1:%x0::5" % i: record("2001:db8:1:%x0::/60" % i, ("%s" % chr(65 + i)) * 43 + "=") for i in range(3)}
        h.set_lookup(prefixes)
        h.set_active(["2001:db8:1:00::", "2001:db8:1:20::"])  # peer 0 and 2 are busy, peer 1 idle
        for address in prefixes:
            h.run("packet", address, max_peers=2)
        # third peer needs room: the idle one (index 1) goes, not the oldest busy one
        self.assertEqual(h.wg_calls()[2], "set wg-hams peer %s remove" % ("B" * 43 + "="))
        self.assertEqual(len(h.wg_calls()), 4)

    def test_rf_link_is_refused_before_anything_happens(self):
        for link in ("rf", "", "mesh"):
            h = Harness(self)
            h.set_lookup({"2001:db8:1:20::5": record("2001:db8:1:20::/60")})
            self.assertNotEqual(h.run("packet", "2001:db8:1:20::5", link=link).returncode, 0)
            self.assertEqual(h.wg_calls(), [])
            self.assertEqual(h.lookups(), [])

    def test_bad_address_is_refused(self):
        h = Harness(self)
        self.assertNotEqual(h.run("packet", "1::; reboot").returncode, 0)
        self.assertEqual(h.lookups(), [])


class ConfigTests(unittest.TestCase):
    def test_package_installs_the_daemon_files(self):
        makefile = "".join(p.read_text() for p in sorted((HERE.parent / "pkg").glob("*.mk")))
        for name in ("sbin/hams-node-ondemand", "init.d/hams-node-ondemand", "config/hams_node_ondemand"):
            self.assertIn(name, makefile)
        self.assertIn("option link_type 'internet'", (HERE.parent / "files/etc/config/hams_node_ondemand").read_text())
        self.assertIn("option max_peers '256'", (HERE.parent / "files/etc/config/hams_node_ondemand").read_text())

    def test_init_script_syntax_and_ruleset_refreshes_on_traffic(self):
        init = HERE.parent / "files/etc/init.d/hams-node-ondemand"
        self.assertEqual(subprocess.run(["sh", "-n", str(init)]).returncode, 0)
        self.assertIn("update @seen { ip6 daddr & ffff:ffff:ffff:fff0:: }", init.read_text())

    def test_ruleset_avoids_nft_keyword_chain_name_and_ends_statement_before_brace(self):
        # nft 1.1 on AREDN 4.26.10.0 rejected `chain fwd` (a keyword) and a statement closed by `} }` on one line
        text = (HERE.parent / "files/etc/init.d/hams-node-ondemand").read_text()
        self.assertNotIn("chain fwd ", text)
        self.assertIn("chain forward_seen {", text)
        self.assertIn("ffff:fff0:: }\n\t}\n}\nNFT", text)


if __name__ == "__main__":
    unittest.main()
