"""
Tests for release_pkgs.py, the script that builds signed apt and yum repositories from the release
.deb/.rpm files and uploads them to Cloudflare R2. reprepro, createrepo_c, rpm, gpg, python-gnupg
and boto3 are replaced by mocks; files are real, in a temporary directory.
"""

import base64
import os
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from release_script_testing import load_script  # noqa: E402

rp = load_script("release_pkgs")


class InTempDir(unittest.TestCase):
    """The script writes ./conf, ./rpm and pb.key relative to the current directory."""

    def setUp(self):
        self._old = os.getcwd()
        self._tmp = tempfile.TemporaryDirectory()
        os.chdir(self._tmp.name)
        self.addCleanup(self._cleanup)

    def _cleanup(self):
        os.chdir(self._old)
        self._tmp.cleanup()

    def touch(self, path, data=b"x"):
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)


def _process(returncode=0, out=b"", err=b""):
    process = mock.Mock()
    process.returncode = returncode
    process.communicate.return_value = (out, err)
    return process


class PkgUploaderTest(unittest.TestCase):
    # Tests [@ANCHOR: release_pkgs:PkgUploader::__init__]
    def test_the_constructor_keeps_the_four_r2_settings(self):
        up = rp.PkgUploader("acct", "bucket", "id", "secret")
        self.assertEqual((up.account_id, up.bucket_name, up.client_id, up.client_secret),
                         ("acct", "bucket", "id", "secret"))

    # Tests [@ANCHOR: release_pkgs:PkgUploader::upload_pkg_to_r2]
    def test_a_file_is_uploaded_to_the_accounts_r2_endpoint_with_the_clients_keys(self):
        up = rp.PkgUploader("acct", "bucket", "id", "secret")
        s3 = mock.Mock()
        with mock.patch.object(rp.boto3, "client", return_value=s3) as client:
            up.upload_pkg_to_r2("local/file.deb", "cloudflared/pool/file.deb")
        args, kwargs = client.call_args
        self.assertEqual(args[0], "s3")
        self.assertEqual(kwargs["endpoint_url"], "https://acct.r2.cloudflarestorage.com")
        self.assertEqual(kwargs["aws_access_key_id"], "id")
        self.assertEqual(kwargs["aws_secret_access_key"], "secret")
        s3.upload_file.assert_called_once_with("local/file.deb", "bucket", "cloudflared/pool/file.deb")

    # Tests [@ANCHOR: release_pkgs:PkgUploader::upload_pkg_to_r2]
    def test_an_r2_client_error_propagates_to_the_caller(self):
        up = rp.PkgUploader("acct", "bucket", "id", "secret")
        s3 = mock.Mock()
        s3.upload_file.side_effect = rp.ClientError("denied")
        with mock.patch.object(rp.boto3, "client", return_value=s3):
            with self.assertRaises(rp.ClientError):
                up.upload_pkg_to_r2("f", "k")


class DistributionConfTest(InTempDir):
    # Tests [@ANCHOR: release_pkgs:PkgCreator::create_distribution_conf]
    def test_one_stanza_is_written_per_release_with_the_signing_key(self):
        path = os.path.join(os.getcwd(), "distributions")
        rp.PkgCreator().create_distribution_conf(path, "cloudflared", "cloudflared", ["bookworm", "noble"],
                                                 ["amd64", "arm64"], "main", "apt repository", "KEYID")
        with open(path) as f:
            text = f.read()
        stanzas = [s for s in text.split("\n\n") if s.strip()]
        self.assertEqual(len(stanzas), 2)
        self.assertIn("Codename: bookworm\n", stanzas[0])
        self.assertIn("Codename: noble\n", stanzas[1])
        for s in stanzas:
            self.assertIn("Architectures: amd64 arm64\n", s)
            self.assertIn("Components: main\n", s)
            self.assertIn("SignWith: KEYID", s)
        self.assertIn("Description: apt repository - bookworm\n", stanzas[0])


class DebPackagesTest(unittest.TestCase):
    # Tests [@ANCHOR: release_pkgs:PkgCreator::create_deb_pkgs]
    def test_reprepro_is_asked_to_include_the_deb_in_the_release(self):
        with mock.patch.object(rp, "Popen", return_value=_process()) as popen:
            rp.PkgCreator().create_deb_pkgs("bookworm", "./artifacts/cloudflared-linux-amd64.deb")
        self.assertEqual(popen.call_args[0][0],
                         ["reprepro", "includedeb", "bookworm", "./artifacts/cloudflared-linux-amd64.deb"])

    # Tests [@ANCHOR: release_pkgs:PkgCreator::create_deb_pkgs]
    def test_a_reprepro_failure_raises_with_the_exit_code_and_stderr(self):
        with mock.patch.object(rp, "Popen", return_value=_process(2, b"", b"no such file")):
            with self.assertRaises(RuntimeError) as ctx:
                rp.PkgCreator().create_deb_pkgs("bookworm", "x.deb")
        self.assertIn("exit 2", str(ctx.exception))
        self.assertIn("no such file", str(ctx.exception))


class RpmPackagesTest(unittest.TestCase):
    # Tests [@ANCHOR: release_pkgs:PkgCreator::create_rpm_pkgs]
    def test_the_rpms_are_set_up_the_repo_is_built_and_the_metadata_is_signed_in_that_order(self):
        creator = rp.PkgCreator()
        order = []
        with mock.patch.object(creator, "_setup_rpm_pkg_directories", side_effect=lambda *a: order.append("setup")), \
                mock.patch.object(creator, "_sign_repomd", side_effect=lambda: order.append("sign")), \
                mock.patch.object(rp, "Popen", side_effect=lambda *a, **k: order.append(a[0][0]) or _process()):
            creator.create_rpm_pkgs("./artifacts", "KeyName")
        self.assertEqual(order, ["setup", "createrepo_c", "sign"])

    # Tests [@ANCHOR: release_pkgs:PkgCreator::create_rpm_pkgs]
    def test_a_createrepo_failure_raises_and_nothing_is_signed(self):
        creator = rp.PkgCreator()
        with mock.patch.object(creator, "_setup_rpm_pkg_directories"), \
                mock.patch.object(creator, "_sign_repomd") as sign, \
                mock.patch.object(rp, "Popen", return_value=_process(1, b"", b"broken")):
            with self.assertRaises(RuntimeError) as ctx:
                creator.create_rpm_pkgs("./artifacts", "KeyName")
        self.assertIn("broken", str(ctx.exception))
        sign.assert_not_called()


class RepoFileTest(InTempDir):
    # Tests [@ANCHOR: release_pkgs:PkgCreator::create_repo_file]
    def test_the_dot_repo_file_points_yum_at_the_rpm_directory_and_requires_a_signature(self):
        path = rp.PkgCreator().create_repo_file(os.getcwd(), "cloudflared", "https://pkg.example/cloudflared",
                                                "https://pkg.example/key.gpg")
        self.assertEqual(path, os.path.join(os.getcwd(), "cloudflared.repo"))
        with open(path) as f:
            self.assertEqual(f.read(), "[cloudflared-stable]\nname=cloudflared-stable\n"
                                       "baseurl=https://pkg.example/cloudflared/rpm\nenabled=1\ntype=rpm\n"
                                       "gpgcheck=1\ngpgkey=https://pkg.example/key.gpg\n")


class RpmSigningTest(unittest.TestCase):
    # Tests [@ANCHOR: release_pkgs:PkgCreator::_sign_rpms]
    def test_rpm_addsign_is_run_with_the_named_key(self):
        with mock.patch.object(rp, "Popen", return_value=_process()) as popen:
            rp.PkgCreator()._sign_rpms("./rpm/x86_64/a.rpm", "Key Name")
        self.assertEqual(popen.call_args[0][0],
                         ["rpm", "--define", "_gpg_name Key Name", "--addsign", "./rpm/x86_64/a.rpm"])

    # Tests [@ANCHOR: release_pkgs:PkgCreator::_sign_rpms]
    def test_a_signing_failure_raises_with_the_real_error(self):
        with mock.patch.object(rp, "Popen", return_value=_process(1, b"", b"no secret key")):
            with self.assertRaises(RuntimeError) as ctx:
                rp.PkgCreator()._sign_rpms("a.rpm", "K")
        self.assertIn("no secret key", str(ctx.exception))

    # Tests [@ANCHOR: release_pkgs:PkgCreator::_sign_repomd]
    def test_the_repository_metadata_gets_an_armored_detached_signature(self):
        with mock.patch.object(rp, "Popen", return_value=_process()) as popen:
            rp.PkgCreator()._sign_repomd()
        self.assertEqual(popen.call_args[0][0],
                         ["gpg", "--batch", "--yes", "--detach-sign", "--armor", "./rpm/repodata/repomd.xml"])

    # Tests [@ANCHOR: release_pkgs:PkgCreator::_sign_repomd]
    def test_a_metadata_signing_failure_raises_with_the_real_error(self):
        with mock.patch.object(rp, "Popen", return_value=_process(2, b"", b"gpg: failed")):
            with self.assertRaises(RuntimeError) as ctx:
                rp.PkgCreator()._sign_repomd()
        self.assertIn("gpg: failed", str(ctx.exception))


class SetupRpmDirectoriesTest(InTempDir):
    # Tests [@ANCHOR: release_pkgs:PkgCreator::_setup_rpm_pkg_directories]
    def test_each_rpm_is_copied_under_its_architecture_and_signed(self):
        self.touch("art/sub/cloudflared-linux-x86_64.rpm", b"a")
        self.touch("art/cloudflared-linux-aarch64.rpm", b"b")
        self.touch("art/cloudflared-linux-amd64.deb", b"not an rpm")
        creator = rp.PkgCreator()
        with mock.patch.object(creator, "_sign_rpms") as sign:
            creator._setup_rpm_pkg_directories("art", "Key")
        self.assertTrue(os.path.exists("rpm/x86_64/cloudflared-linux-x86_64.rpm"))
        self.assertTrue(os.path.exists("rpm/aarch64/cloudflared-linux-aarch64.rpm"))
        self.assertEqual(sorted(os.listdir("rpm")), ["aarch64", "x86_64"])
        self.assertEqual(sorted(c[0][0] for c in sign.call_args_list),
                         ["./rpm/aarch64/cloudflared-linux-aarch64.rpm", "./rpm/x86_64/cloudflared-linux-x86_64.rpm"])
        self.assertEqual({c[0][1] for c in sign.call_args_list}, {"Key"})


class GpgImportTest(unittest.TestCase):
    PRIVATE = base64.b64encode(b"PRIVATE KEY").decode()
    PUBLIC = base64.b64encode(b"PUBLIC KEY").decode()

    @staticmethod
    def _gpg(fingerprints, listed):
        gpg = mock.Mock()
        gpg.import_keys.return_value = mock.Mock(fingerprints=fingerprints)
        gpg.list_keys.return_value = listed
        return gpg

    # Tests [@ANCHOR: release_pkgs:PkgCreator::import_gpg_keys]
    def test_the_decoded_keys_are_imported_and_the_fingerprint_and_uid_are_returned(self):
        gpg = self._gpg(["FP1"], [{"fingerprint": "OTHER", "uids": ["x"]},
                                  {"fingerprint": "FP1", "uids": ["Signer <s@example.com>"]}])
        with mock.patch.object(rp.gnupg, "GPG", return_value=gpg):
            result = rp.PkgCreator().import_gpg_keys(self.PRIVATE, self.PUBLIC)
        self.assertEqual(result, ("FP1", "Signer <s@example.com>"))
        self.assertEqual([c[0][0] for c in gpg.import_keys.call_args_list], [b"PRIVATE KEY", b"PUBLIC KEY"])

    # Tests [@ANCHOR: release_pkgs:PkgCreator::import_gpg_keys]
    def test_a_private_key_that_imports_no_fingerprint_is_rejected(self):
        with mock.patch.object(rp.gnupg, "GPG", return_value=self._gpg([], [])):
            with self.assertRaises(Exception) as ctx:
                rp.PkgCreator().import_gpg_keys(self.PRIVATE, self.PUBLIC)
        self.assertIn("Failed to import private key", str(ctx.exception))

    # Tests [@ANCHOR: release_pkgs:PkgCreator::import_gpg_keys]
    def test_an_imported_key_missing_from_the_secret_key_list_is_an_error(self):
        gpg = self._gpg(["FP1"], [{"fingerprint": "OTHER", "uids": ["x"]}])
        with mock.patch.object(rp.gnupg, "GPG", return_value=gpg):
            with self.assertRaises(Exception) as ctx:
                rp.PkgCreator().import_gpg_keys(self.PRIVATE, self.PUBLIC)
        self.assertIn("FP1", str(ctx.exception))

    # Tests [@ANCHOR: release_pkgs:PkgCreator::import_multiple_gpg_keys]
    def test_the_primary_key_comes_first_and_the_secondary_second(self):
        creator = rp.PkgCreator()
        with mock.patch.object(creator, "import_gpg_keys", side_effect=[("P", "primary"), ("S", "secondary")]) as imp:
            result = creator.import_multiple_gpg_keys("p-priv", "p-pub", "s-priv", "s-pub")
        self.assertEqual(result, [("P", "primary"), ("S", "secondary")])
        self.assertEqual([c[0] for c in imp.call_args_list], [("p-priv", "p-pub"), ("s-priv", "s-pub")])

    # Tests [@ANCHOR: release_pkgs:PkgCreator::import_multiple_gpg_keys]
    def test_a_half_given_pair_is_ignored_and_no_keys_gives_an_empty_list(self):
        creator = rp.PkgCreator()
        with mock.patch.object(creator, "import_gpg_keys", return_value=("P", "primary")) as imp:
            self.assertEqual(creator.import_multiple_gpg_keys("p-priv", "p-pub", "s-priv", None), [("P", "primary")])
            imp.reset_mock()
            self.assertEqual(creator.import_multiple_gpg_keys(None, None), [])
            imp.assert_not_called()


class ImportRpmKeyTest(InTempDir):
    # Tests [@ANCHOR: release_pkgs:PkgCreator::import_rpm_key]
    def test_the_decoded_public_key_is_written_and_given_to_rpm_import(self):
        with mock.patch.object(rp, "Popen", return_value=_process()) as popen:
            rp.PkgCreator().import_rpm_key(base64.b64encode(b"PUBLIC").decode())
        with open("pb.key", "rb") as f:
            self.assertEqual(f.read(), b"PUBLIC")
        self.assertEqual(popen.call_args[0][0], ["rpm", "--import", "pb.key"])

    # Tests [@ANCHOR: release_pkgs:PkgCreator::import_rpm_key]
    def test_an_rpm_import_failure_raises_with_the_real_error(self):
        with mock.patch.object(rp, "Popen", return_value=_process(1, b"", b"bad key")):
            with self.assertRaises(RuntimeError) as ctx:
                rp.PkgCreator().import_rpm_key(base64.b64encode(b"K").decode())
        self.assertIn("bad key", str(ctx.exception))


class UploadFromDirectoriesTest(InTempDir):
    # Tests [@ANCHOR: release_pkgs:upload_from_directories]
    def test_every_file_is_uploaded_under_the_binary_name_and_no_release_prefix(self):
        self.touch("dists/a/Release")
        self.touch("dists/InRelease")
        uploader = mock.Mock()
        rp.upload_from_directories(uploader, "dists", None, "cloudflared")
        sent = sorted((c[0][0], c[0][1]) for c in uploader.upload_pkg_to_r2.call_args_list)
        self.assertEqual(sent, [("dists/InRelease", "cloudflared/dists/InRelease"),
                                ("dists/a/Release", "cloudflared/dists/a/Release")])

    # Tests [@ANCHOR: release_pkgs:upload_from_directories]
    def test_a_release_argument_adds_a_versioned_prefix(self):
        self.touch("pool/x.deb")
        uploader = mock.Mock()
        rp.upload_from_directories(uploader, "pool", "2026.1.0", "cloudflared")
        uploader.upload_pkg_to_r2.assert_called_once_with("pool/x.deb", "2026.1.0/cloudflared/pool/x.deb")

    # Tests [@ANCHOR: release_pkgs:upload_from_directories]
    def test_a_failed_upload_stops_the_run_instead_of_being_swallowed(self):
        self.touch("pool/x.deb")
        uploader = mock.Mock()
        uploader.upload_pkg_to_r2.side_effect = rp.ClientError("denied")
        with self.assertRaises(rp.ClientError):
            rp.upload_from_directories(uploader, "pool", None, "cloudflared")


class CreateDebPackagingTest(InTempDir):
    # Tests [@ANCHOR: release_pkgs:create_deb_packaging]
    def test_the_conf_lists_both_signing_keys_in_rollover_and_each_release_arch_deb_is_included(self):
        creator, uploader = mock.Mock(), mock.Mock()
        with mock.patch.object(rp, "upload_from_directories") as upload:
            rp.create_deb_packaging(creator, uploader, ["bookworm", "noble"], "PRIMARY", "SECONDARY", "mybin",
                                    ["amd64", "arm64"], "main", None)
        conf_args = creator.create_distribution_conf.call_args[0]
        self.assertEqual(conf_args[0], "./conf/distributions")
        self.assertEqual(conf_args[-1], "PRIMARY SECONDARY")
        self.assertTrue(os.path.isdir("conf"))
        debs = [c[0] for c in creator.create_deb_pkgs.call_args_list]
        self.assertEqual(debs, [("bookworm", "./artifacts/mybin-linux-amd64.deb"),
                                ("bookworm", "./artifacts/mybin-linux-arm64.deb"),
                                ("noble", "./artifacts/mybin-linux-amd64.deb"),
                                ("noble", "./artifacts/mybin-linux-arm64.deb")])
        self.assertEqual([c[0][1:] for c in upload.call_args_list],
                         [("dists", None, "mybin"), ("pool", None, "mybin")])

    # Tests [@ANCHOR: release_pkgs:create_deb_packaging]
    def test_one_key_signs_alone_and_a_release_version_also_makes_a_versioned_copy(self):
        creator, uploader = mock.Mock(), mock.Mock()
        with mock.patch.object(rp, "upload_from_directories") as upload:
            rp.create_deb_packaging(creator, uploader, ["any"], "PRIMARY", None, "cloudflared", ["amd64"],
                                    "main", "2026.1.0")
        self.assertEqual(creator.create_distribution_conf.call_args[0][-1], "PRIMARY")
        self.assertEqual([c[0][1:] for c in upload.call_args_list],
                         [("dists", None, "cloudflared"), ("pool", None, "cloudflared"),
                          ("dists", "2026.1.0", "cloudflared"), ("pool", "2026.1.0", "cloudflared")])


class CreateRpmPackagingTest(unittest.TestCase):
    # Tests [@ANCHOR: release_pkgs:create_rpm_packaging]
    def test_the_repo_file_is_uploaded_and_the_rpm_tree_is_uploaded_as_latest(self):
        creator, uploader = mock.Mock(), mock.Mock()
        creator.create_repo_file.return_value = "art/cloudflared.repo"
        with mock.patch.object(rp, "upload_from_directories") as upload:
            rp.create_rpm_packaging(creator, uploader, "art", "2026.1.0", "cloudflared", "Key",
                                    "https://base", "https://key")
        creator.create_rpm_pkgs.assert_called_once_with("art", "Key")
        creator.create_repo_file.assert_called_once_with("art", "cloudflared", "https://base", "https://key")
        uploader.upload_pkg_to_r2.assert_called_once_with("art/cloudflared.repo", "cloudflared.repo")
        self.assertEqual([c[0][1:] for c in upload.call_args_list], [("rpm", None, "cloudflared")])

    # Tests [@ANCHOR: release_pkgs:create_rpm_packaging]
    def test_upload_repo_file_adds_the_versioned_copy(self):
        creator, uploader = mock.Mock(), mock.Mock()
        with mock.patch.object(rp, "upload_from_directories") as upload:
            rp.create_rpm_packaging(creator, uploader, "art", "2026.1.0", "cloudflared", "Key",
                                    "https://base", "https://key", upload_repo_file=True)
        self.assertEqual([c[0][1:] for c in upload.call_args_list],
                         [("rpm", None, "cloudflared"), ("rpm", "2026.1.0", "cloudflared")])


class ParseArgsTest(unittest.TestCase):
    ENV_KEYS = ("R2_BUCKET", "R2_CLIENT_ID", "R2_CLIENT_SECRET", "R2_ACCOUNT_ID", "RELEASE_VERSION", "BINARY_NAME",
                "LINUX_SIGNING_PRIVATE_KEY", "LINUX_SIGNING_PUBLIC_KEY", "LINUX_SIGNING_PRIVATE_KEY_2",
                "LINUX_SIGNING_PUBLIC_KEY_2", "GPG_PUBLIC_KEY_URL", "PKG_URL")
    FULL = ["--bucket", "b", "--id", "i", "--secret", "s", "--account", "a", "--binary", "cloudflared"]

    def _parse(self, argv, env=None):
        clean = {k: v for k, v in os.environ.items() if k not in self.ENV_KEYS}
        clean.update(env or {})
        with mock.patch.dict(os.environ, clean, clear=True), mock.patch.object(sys, "argv", ["release_pkgs.py"] + argv):
            return rp.parse_args()

    # Tests [@ANCHOR: release_pkgs:parse_args]
    def test_the_required_values_are_returned_with_the_default_distributions_and_architectures(self):
        args = self._parse(self.FULL + ["--upload-repo-file"])
        self.assertEqual((args.bucket, args.id, args.secret, args.account, args.binary), ("b", "i", "s", "a", "cloudflared"))
        self.assertTrue(args.upload_repo_file)
        self.assertIn("bookworm", args.deb_based_releases)
        self.assertIn("amd64", args.archs)

    # Tests [@ANCHOR: release_pkgs:parse_args]
    def test_values_may_come_from_the_environment(self):
        args = self._parse([], {"R2_BUCKET": "b", "R2_CLIENT_ID": "i", "R2_CLIENT_SECRET": "s",
                                "R2_ACCOUNT_ID": "a", "BINARY_NAME": "cloudflared", "RELEASE_VERSION": "2026.1.0"})
        self.assertEqual(args.release_tag, "2026.1.0")

    # Tests [@ANCHOR: release_pkgs:parse_args]
    def test_each_missing_required_value_exits_with_status_1(self):
        for flag in ("--bucket", "--id", "--secret", "--account", "--binary"):
            argv = list(self.FULL)
            i = argv.index(flag)
            del argv[i:i + 2]
            with self.subTest(missing=flag), self.assertRaises(SystemExit) as ctx:
                self._parse(argv)
            self.assertEqual(ctx.exception.code, 1)


if __name__ == "__main__":
    unittest.main()
