# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests of the stand-in classes in release_script_testing.py: they are installed only for packages that are missing, so each
test hides the real package (sys.modules entry None makes the import fail) and restores sys.modules afterwards."""
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import release_script_testing as rst  # noqa: E402

HIDDEN = {name: None for name in ("github", "boto3", "botocore", "botocore.client", "botocore.exceptions", "gnupg")}


class StandInTest(unittest.TestCase):
    def install(self):
        patcher = mock.patch.dict(sys.modules, HIDDEN)
        patcher.start()
        self.addCleanup(patcher.stop)
        for name in HIDDEN:
            sys.modules.pop(name, None)
        rst._install_missing_third_party_modules()

    # Tests [@ANCHOR: release_script_testing:FakeResponse]
    def test_fake_response_has_status_text_and_json_or_raises(self):
        ok = rst.FakeResponse(200, {"a": 1}, "body")
        self.assertEqual((ok.status_code, ok.text, ok.json()), (200, "body", {"a": 1}))
        with self.assertRaises(ValueError):
            rst.FakeResponse(500).json()

    # Tests [@ANCHOR: release_script_testing:GithubException]
    # Tests [@ANCHOR: release_script_testing:UnknownObjectException]
    def test_github_exceptions_keep_status_and_data_and_unknown_object_is_one(self):
        self.install()
        github = sys.modules["github"]
        err = github.GithubException(404, {"message": "nope"})
        self.assertEqual((err.status, err.data), (404, {"message": "nope"}))
        self.assertTrue(issubclass(github.UnknownObjectException, github.GithubException))
        self.assertEqual(github.GithubException(500).data, {})

    # Tests [@ANCHOR: release_script_testing:Github]
    # Tests [@ANCHOR: release_script_testing:GPG]
    def test_github_and_gpg_refuse_to_run_until_a_test_replaces_them(self):
        self.install()
        with self.assertRaises(AssertionError):
            sys.modules["github"].Github("token")
        with self.assertRaises(AssertionError):
            sys.modules["gnupg"].GPG()
        with self.assertRaises(AssertionError):
            sys.modules["boto3"].client("s3")

    # Tests [@ANCHOR: release_script_testing:Config]
    # Tests [@ANCHOR: release_script_testing:ClientError]
    def test_botocore_config_keeps_its_keywords_and_client_error_is_an_exception(self):
        self.install()
        config = sys.modules["botocore.client"].Config(region_name="auto")
        self.assertEqual(config.kwargs, {"region_name": "auto"})
        self.assertTrue(issubclass(sys.modules["botocore.exceptions"].ClientError, Exception))


if __name__ == "__main__":
    unittest.main()
