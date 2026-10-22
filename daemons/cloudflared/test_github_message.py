"""
Tests for github_message.py, the script that writes the Workers KV sha256 checksums into a GitHub
release's notes. Network access is replaced by recorded fake responses; no real GitHub or
Cloudflare call is made.
"""

import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from release_script_testing import FakeResponse, load_script  # noqa: E402

gm = load_script("github_message")


class RaiseForKvErrorTest(unittest.TestCase):
    # Tests [@ANCHOR: github_message:_raise_for_kv_error]
    def test_a_200_response_is_accepted(self):
        self.assertIsNone(gm._raise_for_kv_error(FakeResponse(200), "get checksums"))

    # Tests [@ANCHOR: github_message:_raise_for_kv_error]
    def test_a_non_200_with_an_errors_list_names_the_first_error(self):
        response = FakeResponse(403, {"errors": [{"code": 10000, "message": "bad token"}]})
        with self.assertRaises(Exception) as ctx:
            gm._raise_for_kv_error(response, "get checksums")
        self.assertIn("failed to get checksums", str(ctx.exception))
        self.assertIn("bad token", str(ctx.exception))

    # Tests [@ANCHOR: github_message:_raise_for_kv_error]
    def test_a_non_200_with_an_empty_errors_array_still_raises_with_the_body_text(self):
        # The original bug: this shape fell through and the caller took the error text as data.
        response = FakeResponse(500, {"errors": []}, text="upstream exploded")
        with self.assertRaises(Exception) as ctx:
            gm._raise_for_kv_error(response, "get checksums")
        self.assertIn("upstream exploded", str(ctx.exception))

    # Tests [@ANCHOR: github_message:_raise_for_kv_error]
    def test_a_non_json_error_page_still_raises_with_its_text(self):
        with self.assertRaises(Exception) as ctx:
            gm._raise_for_kv_error(FakeResponse(502, None, text="<html>Bad Gateway</html>"), "get checksums")
        self.assertIn("Bad Gateway", str(ctx.exception))


class KvReadTest(unittest.TestCase):
    # Tests [@ANCHOR: github_message:kv_get_keys]
    def test_keys_are_listed_with_the_prefix_and_the_bearer_token(self):
        response = FakeResponse(200, {"result": [{"name": "update_1_a"}, {"name": "update_1_b"}]})
        with mock.patch.object(gm.requests, "get", return_value=response) as get:
            keys = gm.kv_get_keys("update_1_", "acct", "ns", "tok")
        self.assertEqual([k["name"] for k in keys], ["update_1_a", "update_1_b"])
        args, kwargs = get.call_args
        self.assertEqual(args[0], gm.BASE_KV_URL + "acct/storage/kv/namespaces/ns/keys")
        self.assertEqual(kwargs["params"], {"prefix": "update_1_"})
        self.assertEqual(kwargs["headers"]["Authorization"], "Bearer tok")

    # Tests [@ANCHOR: github_message:kv_get_keys]
    def test_a_failed_listing_raises_instead_of_returning_nothing(self):
        with mock.patch.object(gm.requests, "get", return_value=FakeResponse(500, None, text="down")):
            with self.assertRaises(Exception):
                gm.kv_get_keys("p", "acct", "ns", "tok")

    # Tests [@ANCHOR: github_message:kv_get_value]
    def test_a_value_is_fetched_by_its_percent_encoded_key(self):
        with mock.patch.object(gm.requests, "get", return_value=FakeResponse(200, text="abc123")) as get:
            value = gm.kv_get_value("update_1/x y", "acct", "ns", "tok")
        self.assertEqual(value, "abc123")
        self.assertEqual(get.call_args[0][0],
                         gm.BASE_KV_URL + "acct/storage/kv/namespaces/ns/values/update_1%2Fx%20y")

    # Tests [@ANCHOR: github_message:kv_get_value]
    def test_a_failed_value_fetch_never_returns_the_error_text_as_a_checksum(self):
        with mock.patch.object(gm.requests, "get", return_value=FakeResponse(404, None, text="not found")):
            with self.assertRaises(Exception):
                gm.kv_get_value("k", "acct", "ns", "tok")


class UpdateOrAddMessageTest(unittest.TestCase):
    # Tests [@ANCHOR: github_message:update_or_add_message]
    def test_the_first_checksum_creates_the_heading_and_the_code_block(self):
        msg = gm.update_or_add_message("", "cloudflared-linux-amd64", "aaa")
        self.assertIn("### SHA256 Checksums:", msg)
        self.assertIn("```\ncloudflared-linux-amd64: aaa\n```", msg)

    # Tests [@ANCHOR: github_message:update_or_add_message]
    def test_a_second_checksum_is_added_inside_the_existing_block(self):
        msg = gm.update_or_add_message("", "a", "1")
        msg = gm.update_or_add_message(msg, "b", "2")
        self.assertEqual(msg.count("```"), 2)
        self.assertLess(msg.index("a: 1"), msg.index("b: 2"))
        self.assertLess(msg.index("b: 2"), msg.rindex("```"))

    # Tests [@ANCHOR: github_message:update_or_add_message]
    def test_an_existing_checksum_is_replaced_not_duplicated(self):
        msg = gm.update_or_add_message("", "a", "old")
        msg = gm.update_or_add_message(msg, "a", "new")
        self.assertIn("a: new", msg)
        self.assertNotIn("old", msg)
        self.assertEqual(msg.count("a: "), 1)

    # Tests [@ANCHOR: github_message:update_or_add_message]
    def test_a_name_that_is_a_prefix_of_another_does_not_clobber_the_longer_one(self):
        msg = gm.update_or_add_message("", "cloudflared-linux-amd64-fips", "fips-sum")
        msg = gm.update_or_add_message(msg, "cloudflared-linux-amd64", "plain-sum")
        self.assertIn("cloudflared-linux-amd64-fips: fips-sum", msg)
        self.assertIn("cloudflared-linux-amd64: plain-sum", msg)


class GetReleaseTest(unittest.TestCase):
    # Tests [@ANCHOR: github_message:get_release]
    def test_an_existing_release_is_returned(self):
        repo = mock.Mock()
        repo.get_release.return_value = "the-release"
        self.assertEqual(gm.get_release(repo, "2026.1.0"), "the-release")
        repo.get_release.assert_called_once_with("2026.1.0")

    # Tests [@ANCHOR: github_message:get_release]
    def test_a_missing_release_gives_none(self):
        repo = mock.Mock()
        repo.get_release.side_effect = gm.UnknownObjectException(404, {})
        self.assertIsNone(gm.get_release(repo, "2026.1.0"))


class ParseArgsTest(unittest.TestCase):
    ENV_KEYS = ("API_KEY", "KV_NAMESPACE", "KV_ACCOUNT", "KV_API_TOKEN", "VERSION")

    def _parse(self, argv, env=None):
        clean = {k: v for k, v in os.environ.items() if k not in self.ENV_KEYS}
        clean.update(env or {})
        with mock.patch.dict(os.environ, clean, clear=True), mock.patch.object(sys, "argv", ["github_message.py"] + argv):
            return gm.parse_args()

    # Tests [@ANCHOR: github_message:parse_args]
    def test_every_value_can_come_from_the_command_line(self):
        args = self._parse(["--api-key", "k", "--kv-namespace-id", "n", "--kv-account-id", "a",
                            "--kv-api-token", "t", "--release-version", "2026.1.0", "--dry-run"])
        self.assertEqual((args.api_key, args.kv_namespace_id, args.kv_account_id, args.kv_api_token,
                          args.release_version, args.dry_run), ("k", "n", "a", "t", "2026.1.0", True))

    # Tests [@ANCHOR: github_message:parse_args]
    def test_every_value_can_come_from_the_environment(self):
        args = self._parse([], {"API_KEY": "k", "KV_NAMESPACE": "n", "KV_ACCOUNT": "a",
                                "KV_API_TOKEN": "t", "VERSION": "2026.1.0"})
        self.assertEqual(args.release_version, "2026.1.0")
        self.assertFalse(args.dry_run)

    # Tests [@ANCHOR: github_message:parse_args]
    def test_a_missing_required_value_exits_with_status_1(self):
        with self.assertRaises(SystemExit) as ctx:
            self._parse(["--api-key", "k"])
        self.assertEqual(ctx.exception.code, 1)


class MainTest(unittest.TestCase):
    def _run(self, argv, release, keys, values):
        repo = mock.Mock()
        client = mock.Mock()
        client.get_repo.return_value = repo
        with mock.patch.object(sys, "argv", ["github_message.py", "--api-key", "k", "--kv-namespace-id", "n",
                                             "--kv-account-id", "a", "--kv-api-token", "t",
                                             "--release-version", "2026.1.0"] + argv), \
                mock.patch.object(gm, "Github", return_value=client), \
                mock.patch.object(gm, "get_release", return_value=release), \
                mock.patch.object(gm, "kv_get_keys", return_value=[{"name": k} for k in keys]), \
                mock.patch.object(gm, "kv_get_value", side_effect=lambda key, *a: values[key]):
            gm.main()
        return client, repo

    # Tests [@ANCHOR: github_message:main]
    def test_the_release_notes_get_one_checksum_line_per_binary(self):
        release = mock.Mock()
        self._run([], release, ["update_2026.1.0_one", "update_2026.1.0_two"],
                  {"update_2026.1.0_one": "s1", "update_2026.1.0_two": "s2"})
        version, msg = release.update_release.call_args[0]
        self.assertEqual(version, "2026.1.0")
        self.assertIn("one: s1", msg)
        self.assertIn("two: s2", msg)

    # Tests [@ANCHOR: github_message:main]
    def test_a_dry_run_changes_nothing(self):
        release = mock.Mock()
        self._run(["--dry-run"], release, ["update_2026.1.0_one"], {"update_2026.1.0_one": "s1"})
        release.update_release.assert_not_called()

    # Tests [@ANCHOR: github_message:main]
    def test_a_missing_release_fails_with_a_clear_exit_not_an_attribute_error(self):
        with self.assertRaises(SystemExit) as ctx:
            self._run([], None, [], {})
        self.assertEqual(ctx.exception.code, 1)


if __name__ == "__main__":
    unittest.main()
