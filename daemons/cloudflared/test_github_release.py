"""
Tests for github_release.py, the script that uploads release binaries to a GitHub release and
records their sha256 in Workers KV. GitHub, Workers KV and `go version -m` are replaced by mocks;
files are real, in a temporary directory.
"""

import hashlib
import os
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from release_script_testing import FakeResponse, load_script  # noqa: E402

gr = load_script("github_release")


class InTempDir(unittest.TestCase):
    """upload_asset, move_asset and assert_asset_version work in the current directory."""

    def setUp(self):
        self._old = os.getcwd()
        self._tmp = tempfile.TemporaryDirectory()
        os.chdir(self._tmp.name)
        self.addCleanup(self._cleanup)

    def _cleanup(self):
        os.chdir(self._old)
        self._tmp.cleanup()

    def write(self, name, data=b"payload"):
        with open(name, "wb") as f:
            f.write(data)
        return os.path.join(os.getcwd(), name)


class Sha256Test(InTempDir):
    # Tests [@ANCHOR: github_release:get_sha256]
    def test_the_digest_matches_hashlib_for_a_file_larger_than_one_block(self):
        data = os.urandom(10000)
        path = self.write("blob", data)
        self.assertEqual(gr.get_sha256(path), hashlib.sha256(data).hexdigest())

    # Tests [@ANCHOR: github_release:get_sha256]
    def test_the_digest_of_an_empty_file_is_the_well_known_empty_digest(self):
        path = self.write("empty", b"")
        self.assertEqual(gr.get_sha256(path), hashlib.sha256(b"").hexdigest())


class SendHashTest(unittest.TestCase):
    # Tests [@ANCHOR: github_release:send_hash]
    def test_the_checksum_is_put_under_the_update_version_name_key(self):
        with mock.patch.object(gr.requests, "put", return_value=FakeResponse(200)) as put:
            gr.send_hash("deadbeef", "cloudflared-linux-amd64", "2026.1.0", "acct", "ns", "tok")
        args, kwargs = put.call_args
        self.assertEqual(args[0], gr.BASE_KV_URL +
                         "acct/storage/kv/namespaces/ns/values/update_2026.1.0_cloudflared-linux-amd64")
        self.assertEqual(kwargs["data"], "deadbeef")
        self.assertEqual(kwargs["headers"]["Authorization"], "Bearer tok")

    # Tests [@ANCHOR: github_release:send_hash]
    def test_a_rejected_upload_raises_even_when_the_errors_array_is_empty(self):
        with mock.patch.object(gr.requests, "put",
                               return_value=FakeResponse(500, {"errors": []}, text="kv down")):
            with self.assertRaises(Exception) as ctx:
                gr.send_hash("h", "n", "v", "a", "ns", "t")
        self.assertIn("failed to upload checksum", str(ctx.exception))
        self.assertIn("kv down", str(ctx.exception))

    # Tests [@ANCHOR: github_release:send_hash]
    def test_a_rejected_upload_names_the_first_error_from_the_body(self):
        with mock.patch.object(gr.requests, "put",
                               return_value=FakeResponse(403, {"errors": [{"message": "no access"}]})):
            with self.assertRaises(Exception) as ctx:
                gr.send_hash("h", "n", "v", "a", "ns", "t")
        self.assertIn("no access", str(ctx.exception))


class AssertTagExistsTest(unittest.TestCase):
    @staticmethod
    def _repo(*names):
        repo = mock.Mock()
        tags = []
        for n in names:
            t = mock.Mock()
            t.name = n
            tags.append(t)
        repo.get_tags.return_value = tags
        return repo

    # Tests [@ANCHOR: github_release:assert_tag_exists]
    def test_a_tag_that_is_not_first_in_the_list_is_found(self):
        gr.assert_tag_exists(self._repo("2026.2.0", "2026.1.0"), "2026.1.0")

    # Tests [@ANCHOR: github_release:assert_tag_exists]
    def test_a_missing_tag_raises_naming_the_tag(self):
        with self.assertRaises(Exception) as ctx:
            gr.assert_tag_exists(self._repo("2026.2.0"), "2026.1.0")
        self.assertIn("2026.1.0", str(ctx.exception))

    # Tests [@ANCHOR: github_release:assert_tag_exists]
    def test_a_repository_with_no_tags_raises_the_not_found_error(self):
        with self.assertRaises(Exception) as ctx:
            gr.assert_tag_exists(self._repo(), "2026.1.0")
        self.assertIn("not found", str(ctx.exception))


class GetOrCreateReleaseTest(unittest.TestCase):
    @staticmethod
    def _repo(existing=None, tags=("2026.1.0",)):
        repo = mock.Mock()
        if existing is None:
            repo.get_release.side_effect = gr.UnknownObjectException(404, {})
        else:
            repo.get_release.return_value = existing
        tag_mocks = []
        for n in tags:
            t = mock.Mock()
            t.name = n
            tag_mocks.append(t)
        repo.get_tags.return_value = tag_mocks
        return repo

    # Tests [@ANCHOR: github_release:get_or_create_release]
    def test_an_existing_release_is_returned_without_creating_one(self):
        repo = self._repo(existing="rel")
        self.assertEqual(gr.get_or_create_release(repo, "2026.1.0"), "rel")
        repo.create_git_release.assert_not_called()

    # Tests [@ANCHOR: github_release:get_or_create_release]
    def test_a_new_release_is_created_for_an_existing_tag(self):
        repo = self._repo()
        repo.create_git_release.return_value = "new"
        self.assertEqual(gr.get_or_create_release(repo, "2026.1.0", is_draft=True), "new")
        repo.create_git_release.assert_called_once_with("2026.1.0", "2026.1.0", "", True)

    # Tests [@ANCHOR: github_release:get_or_create_release]
    def test_no_release_is_created_for_a_tag_that_does_not_exist(self):
        repo = self._repo(tags=("other",))
        with self.assertRaises(Exception):
            gr.get_or_create_release(repo, "2026.1.0")
        repo.create_git_release.assert_not_called()

    # Tests [@ANCHOR: github_release:get_or_create_release]
    def test_a_dry_run_creates_nothing(self):
        repo = self._repo()
        self.assertIsNone(gr.get_or_create_release(repo, "2026.1.0", dry_run=True))
        repo.create_git_release.assert_not_called()

    # Tests [@ANCHOR: github_release:get_or_create_release]
    def test_losing_a_creation_race_fetches_the_release_the_other_build_made(self):
        repo = self._repo()
        repo.create_git_release.side_effect = gr.GithubException(
            422, {"errors": [{"code": "already_exists"}]})
        repo.get_release.side_effect = [gr.UnknownObjectException(404, {}), "winner"]
        self.assertEqual(gr.get_or_create_release(repo, "2026.1.0"), "winner")

    # Tests [@ANCHOR: github_release:get_or_create_release]
    def test_any_other_creation_error_propagates(self):
        repo = self._repo()
        repo.create_git_release.side_effect = gr.GithubException(500, {"errors": []})
        with self.assertRaises(gr.GithubException):
            gr.get_or_create_release(repo, "2026.1.0")


class ParseArgsTest(unittest.TestCase):
    ENV_KEYS = ("API_KEY", "VERSION", "ASSET_PATH", "ASSET_NAME", "KV_NAMESPACE", "KV_ACCOUNT", "KV_API_TOKEN")
    FULL = ["--api-key", "k", "--release-version", "2026.1.0", "--path", "/p", "--name", "n",
            "--namespace-id", "ns", "--kv-account-id", "a", "--kv-api-token", "t"]

    def _parse(self, argv, env=None):
        clean = {k: v for k, v in os.environ.items() if k not in self.ENV_KEYS}
        clean.update(env or {})
        with mock.patch.dict(os.environ, clean, clear=True), mock.patch.object(sys, "argv", ["github_release.py"] + argv):
            return gr.parse_args()

    # Tests [@ANCHOR: github_release:parse_args]
    def test_all_values_given_on_the_command_line_are_returned(self):
        args = self._parse(self.FULL + ["--draft"])
        self.assertEqual((args.release_version, args.path, args.name, args.draft, args.dry_run),
                         ("2026.1.0", "/p", "n", True, False))

    # Tests [@ANCHOR: github_release:parse_args]
    def test_values_come_from_the_environment_when_no_flag_is_given(self):
        args = self._parse([], {"API_KEY": "k", "VERSION": "v", "ASSET_PATH": "/p", "ASSET_NAME": "n",
                                "KV_NAMESPACE": "ns", "KV_ACCOUNT": "a", "KV_API_TOKEN": "t"})
        self.assertEqual((args.api_key, args.namespace_id, args.kv_account_id), ("k", "ns", "a"))

    # Tests [@ANCHOR: github_release:parse_args]
    def test_a_missing_asset_name_is_fine_when_the_path_is_a_directory(self):
        with tempfile.TemporaryDirectory() as d:
            argv = [a for a in self.FULL]
            argv[argv.index("/p")] = d
            i = argv.index("--name")
            del argv[i:i + 2]
            self.assertEqual(self._parse(argv).path, d)

    # Tests [@ANCHOR: github_release:parse_args]
    def test_a_missing_required_value_exits_with_status_1(self):
        argv = [a for a in self.FULL]
        i = argv.index("--kv-api-token")
        del argv[i:i + 2]
        with self.assertRaises(SystemExit) as ctx:
            self._parse(argv)
        self.assertEqual(ctx.exception.code, 1)


class UploadAssetTest(InTempDir):
    @staticmethod
    def _release(already=()):
        release = mock.Mock()
        assets = []
        for n in already:
            a = mock.Mock()
            a.name = n
            assets.append(a)
        release.get_assets.return_value = assets
        return release

    # Tests [@ANCHOR: github_release:upload_asset]
    def test_a_new_asset_is_uploaded_and_its_checksum_sent(self):
        path = self.write("cloudflared-linux-amd64", b"binary")
        release = self._release()
        with mock.patch.object(gr, "send_hash") as send:
            gr.upload_asset(release, path, "cloudflared-linux-amd64", "2026.1.0", "a", "ns", "t")
        release.upload_asset.assert_called_once_with(path, name="cloudflared-linux-amd64")
        send.assert_called_once_with(hashlib.sha256(b"binary").hexdigest(), "cloudflared-linux-amd64",
                                     "2026.1.0", "a", "ns", "t")

    # Tests [@ANCHOR: github_release:upload_asset]
    def test_an_already_uploaded_asset_is_not_uploaded_again_but_its_checksum_is_still_sent(self):
        path = self.write("cloudflared-linux-amd64", b"binary")
        release = self._release(already=["cloudflared-linux-amd64"])
        with mock.patch.object(gr, "send_hash") as send:
            gr.upload_asset(release, path, "cloudflared-linux-amd64", "2026.1.0", "a", "ns", "t")
        release.upload_asset.assert_not_called()
        send.assert_called_once()

    # Tests [@ANCHOR: github_release:upload_asset]
    def test_a_tgz_asset_has_the_checksum_of_the_binary_inside_it(self):
        inner = self.write("cloudflared", b"mac-binary")
        tgz = os.path.join(os.getcwd(), "cloudflared-darwin-amd64.tgz")
        with tarfile.open(tgz, "w:gz") as tar:
            tar.add(inner, arcname="cloudflared")
        os.remove(inner)
        release = self._release()
        with mock.patch.object(gr, "send_hash") as send:
            gr.upload_asset(release, tgz, "cloudflared-darwin-amd64.tgz", "2026.1.0", "a", "ns", "t")
        self.assertEqual(send.call_args[0][0], hashlib.sha256(b"mac-binary").hexdigest())
        release.upload_asset.assert_called_once()


class MoveAssetTest(InTempDir):
    # Tests [@ANCHOR: github_release:move_asset]
    def test_the_file_is_copied_into_a_new_artifacts_directory(self):
        src = self.write("built-binary", b"x")
        gr.move_asset(src, "cloudflared-linux-amd64")
        with open(os.path.join("artifacts", "cloudflared-linux-amd64"), "rb") as f:
            self.assertEqual(f.read(), b"x")
        self.assertTrue(os.path.exists(src))

    # Tests [@ANCHOR: github_release:move_asset]
    def test_a_file_already_in_the_artifacts_directory_is_left_alone(self):
        os.mkdir("artifacts")
        src = self.write(os.path.join("artifacts", "same"), b"y")
        gr.move_asset(src, "same")
        with open(src, "rb") as f:
            self.assertEqual(f.read(), b"y")


def _go_version_output(text):
    process = mock.Mock()
    process.communicate.return_value = (text.encode(), b"")
    return process


class GetBinaryVersionTest(unittest.TestCase):
    OUTPUT = ('path\tgithub.com/cloudflare/cloudflared/cmd/cloudflared\n'
              'build\t-ldflags="-X \\"main.Version=2024.8.3-6-gec072691\\" -X \\"main.BuildTime=now\\" "\n')

    # Tests [@ANCHOR: github_release:get_binary_version]
    def test_the_version_is_cut_out_between_the_marker_and_the_backslash(self):
        with mock.patch.object(gr.subprocess, "Popen", return_value=_go_version_output(self.OUTPUT)) as popen:
            self.assertEqual(gr.get_binary_version("/bin/cf"), "2024.8.3-6-gec072691")
        self.assertEqual(popen.call_args[0][0], ["go", "version", "-m", "/bin/cf"])

    # Tests [@ANCHOR: github_release:get_binary_version]
    def test_output_without_the_marker_raises_instead_of_returning_garbage(self):
        with mock.patch.object(gr.subprocess, "Popen", return_value=_go_version_output("no version here")):
            with self.assertRaises(Exception) as ctx:
                gr.get_binary_version("/bin/cf")
        self.assertIn("main.Version=", str(ctx.exception))


class AssertAssetVersionTest(InTempDir):
    # Tests [@ANCHOR: github_release:assert_asset_version]
    def test_a_matching_extensionless_binary_passes(self):
        with mock.patch.object(gr, "get_binary_version", return_value="2026.1.0") as gbv:
            gr.assert_asset_version("/x/cloudflared-linux-amd64", "2026.1.0")
        gbv.assert_called_once_with("/x/cloudflared-linux-amd64")

    # Tests [@ANCHOR: github_release:assert_asset_version]
    def test_a_mismatching_version_exits_with_status_1(self):
        with mock.patch.object(gr, "get_binary_version", return_value="2025.1.0"):
            with self.assertRaises(SystemExit) as ctx:
                gr.assert_asset_version("/x/cloudflared.exe", "2026.1.0")
        self.assertEqual(ctx.exception.code, 1)

    # Tests [@ANCHOR: github_release:assert_asset_version]
    def test_a_file_with_another_extension_is_not_checked(self):
        with mock.patch.object(gr, "get_binary_version") as gbv:
            gr.assert_asset_version("/x/cloudflared.deb", "2026.1.0")
        gbv.assert_not_called()

    # Tests [@ANCHOR: github_release:assert_asset_version]
    def test_a_tgz_is_unpacked_and_the_binary_inside_is_checked(self):
        inner = self.write("cloudflared", b"mac")
        tgz = os.path.join(os.getcwd(), "cf.tgz")
        with tarfile.open(tgz, "w:gz") as tar:
            tar.add(inner, arcname="cloudflared")
        os.remove(inner)
        with mock.patch.object(gr, "get_binary_version", return_value="2026.1.0") as gbv:
            gr.assert_asset_version(tgz, "2026.1.0")
        checked = gbv.call_args[0][0]
        self.assertEqual(checked, os.path.join(os.getcwd(), "tmp", "cloudflared"))
        self.assertTrue(os.path.exists(checked))


class MainTest(InTempDir):
    BASE = ["github_release.py", "--api-key", "k", "--release-version", "2026.1.0",
            "--namespace-id", "ns", "--kv-account-id", "a", "--kv-api-token", "t"]

    def _main(self, extra, **patches):
        with mock.patch.object(sys, "argv", self.BASE + extra):
            for name, value in patches.items():
                patcher = mock.patch.object(gr, name, value)
                patcher.start()
                self.addCleanup(patcher.stop)
            gr.main()

    # Tests [@ANCHOR: github_release:main]
    def test_a_single_file_dry_run_checks_that_files_version_without_a_name_error(self):
        path = self.write("cloudflared-linux-amd64")
        check = mock.Mock()
        self._main(["--dry-run", "--path", path, "--name", "n"], assert_asset_version=check)
        check.assert_called_once_with(path, "2026.1.0")

    # Tests [@ANCHOR: github_release:main]
    def test_a_directory_dry_run_checks_every_file_and_uploads_nothing(self):
        os.mkdir("built")
        self.write(os.path.join("built", "a"))
        self.write(os.path.join("built", "b"))
        check, github = mock.Mock(), mock.Mock()
        self._main(["--dry-run", "--path", "built"], assert_asset_version=check, Github=github)
        self.assertEqual(check.call_count, 2)
        github.assert_not_called()

    # Tests [@ANCHOR: github_release:main]
    def test_a_real_run_checks_versions_then_makes_the_release_then_uploads_and_moves_each_file(self):
        os.mkdir("built")
        self.write(os.path.join("built", "a"))
        order = []
        check = mock.Mock(side_effect=lambda *a: order.append("check"))
        release = object()
        get_release = mock.Mock(side_effect=lambda *a: order.append("release") or release)
        upload = mock.Mock(side_effect=lambda *a: order.append("upload"))
        move = mock.Mock(side_effect=lambda *a: order.append("move"))
        self._main(["--path", "built"], assert_asset_version=check, get_or_create_release=get_release,
                   upload_asset=upload, move_asset=move, Github=mock.Mock())
        self.assertEqual(order, ["check", "release", "upload", "move"])
        self.assertIs(upload.call_args[0][0], release)

    # Tests [@ANCHOR: github_release:main]
    def test_a_real_run_with_a_single_file_path_exits_with_status_1(self):
        path = self.write("cloudflared-linux-amd64")
        with self.assertRaises(SystemExit) as ctx:
            self._main(["--path", path, "--name", "n"], Github=mock.Mock())
        self.assertEqual(ctx.exception.code, 1)


if __name__ == "__main__":
    unittest.main()
