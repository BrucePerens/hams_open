# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests of the installer and the pin generator without a network and without installing anything. Run by `cargo test` (src/glasgow/tests.rs)
or by hand: python3 glasgow/test_glasgow_tools.py"""
import asyncio
import contextlib
import importlib.util
import io
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
import install_glasgow  # noqa: E402
import pin_requirements  # noqa: E402


class InstallerTests(unittest.TestCase):
    # Tests [@ANCHOR: glasgow_install_sha256]
    def test_sha256_of_a_file(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "f"
            p.write_bytes(b"abc")
            self.assertEqual(install_glasgow.sha256(p), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad")

    # Tests [@ANCHOR: glasgow_install_run]
    def test_run_runs_a_command_and_raises_on_failure(self):
        with contextlib.redirect_stdout(io.StringIO()):
            install_glasgow.run(["true"])
            with self.assertRaises(Exception):
                install_glasgow.run(["false"])

    # Tests [@ANCHOR: glasgow_install_copy_bounded]
    # Tests [@ANCHOR: glasgow_install_main]
    def test_the_archive_is_copied_into_dest_and_the_copy_is_what_is_extracted_and_a_huge_one_is_refused(self):
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "x.tar.gz"
            src.write_bytes(b"first")
            dest = Path(d) / "install"
            seen: dict[str, object] = {}

            def fake_sha(p):
                seen["path"] = Path(p)
                seen["bytes"] = Path(p).read_bytes()
                src.write_bytes(b"changed after the check")  # a writer of the original after the hash
                return "0" * 64

            with self.assertRaises(SystemExit), mock.patch.object(install_glasgow, "sha256", fake_sha), mock.patch.object(install_glasgow, "run"):
                install_glasgow.main(["--dest", str(dest), "--archive", str(src)])
            self.assertEqual(seen["path"], dest.resolve() / "glasgow.tar.gz")
            self.assertEqual(seen["bytes"], b"first")
            out = Path(d) / "copy"
            with mock.patch.object(install_glasgow, "ARCHIVE_BYTES", 10):
                with self.assertRaises(SystemExit) as e:
                    install_glasgow.copy_bounded(io.BytesIO(b"x" * (10 + (1 << 20) + 1)), out)
                self.assertIn("larger than", str(e.exception))
                install_glasgow.copy_bounded(io.BytesIO(b"ok"), out)
            self.assertEqual(out.read_bytes(), b"ok")

    # Tests [@ANCHOR: glasgow_install_main]
    def test_an_archive_with_the_wrong_hash_is_refused_before_anything_is_installed(self):
        with tempfile.TemporaryDirectory() as d:
            bad = Path(d) / "x.tar.gz"
            bad.write_bytes(b"not the archive")
            dest = Path(d) / "install"
            with self.assertRaises(SystemExit) as e, mock.patch.object(install_glasgow, "run") as run:
                install_glasgow.main(["--dest", str(dest), "--archive", str(bad)])
            self.assertIn("REFUSED", str(e.exception))
            run.assert_not_called()
            self.assertFalse(dest.exists(), "no venv was created and the directory was removed")

    def test_an_existing_destination_is_never_reused(self):
        with tempfile.TemporaryDirectory() as d:
            with self.assertRaises(SystemExit) as e:
                install_glasgow.main(["--dest", d])
            self.assertIn("exists", str(e.exception))

    def test_the_pin_names_the_commit_and_the_requirements_are_hashed(self):
        self.assertEqual(len(install_glasgow.ARCHIVE_SHA256), 64)
        self.assertIn(install_glasgow.COMMIT, install_glasgow.ARCHIVE_URL)
        text = (Path(__file__).resolve().parent / "requirements.txt").read_text()
        self.assertIn("--hash=sha256:", text)


FAKE = {"urls": [
    {"filename": "a-1-py3-none-any.whl", "packagetype": "bdist_wheel", "digests": {"sha256": "b" * 64}},
    {"filename": "a-1-cp313-win_amd64.whl", "packagetype": "bdist_wheel", "digests": {"sha256": "c" * 64}},
    {"filename": "a-1-cp313-macosx_11_0_arm64.whl", "packagetype": "bdist_wheel", "digests": {"sha256": "d" * 64}},
    {"filename": "a-1.tar.gz", "packagetype": "sdist", "digests": {"sha256": "a" * 64}},
]}


class _Resp(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *a):
        return False


class PinTests(unittest.TestCase):
    # Tests [@ANCHOR: glasgow_pin_hashes]
    def test_only_linux_files_are_kept_and_sorted(self):
        with mock.patch("urllib.request.urlopen", return_value=_Resp(json.dumps(FAKE).encode())):
            self.assertEqual(pin_requirements.hashes("a", "1"), ["a" * 64, "b" * 64])
        with mock.patch("urllib.request.urlopen", return_value=_Resp(json.dumps({"urls": []}).encode())):
            with self.assertRaises(SystemExit):
                pin_requirements.hashes("a", "1")

    # Tests [@ANCHOR: glasgow_pin_main]
    def test_main_prints_every_pin_with_its_hashes(self):
        out = io.StringIO()
        with mock.patch.object(pin_requirements, "hashes", return_value=["e" * 64]), contextlib.redirect_stdout(out):
            pin_requirements.main()
        lines = out.getvalue().splitlines()
        self.assertTrue(lines[0].startswith("# Generated"))
        self.assertEqual(sum(1 for line in lines if "==" in line), len(pin_requirements.PINS))
        self.assertTrue(all("--hash=sha256:" in line for line in lines if line.startswith("    ")))


def _load_ops():
    spec = importlib.util.spec_from_file_location("glasgow_ops", Path(__file__).resolve().parent.parent / "src" / "glasgow" / "ops.py")
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class _FakeUart:
    """A fake UART object (no device): returns one byte every `period` seconds, or never when `period` is None."""

    def __init__(self, period):
        self.period = period

    async def write(self, data, flush=False):
        pass

    async def read_all(self, flush=True):
        if self.period is None:
            await asyncio.sleep(3600)
        await asyncio.sleep(self.period)
        return b"x"

    async def get_baud(self):
        return 9600.0


class OpsTests(unittest.TestCase):
    def _transact(self, period, **payload):
        ops = _load_ops()
        base = {"send": [], "until": [], "timeout_s": 0.4, "idle_ms": 200, "max_bytes": 65536}
        base.update(payload)
        return asyncio.run(ops._uart_transact({"uart_iface": _FakeUart(period)}, base))

    # Tests [@ANCHOR: glasgow_ops_uart_transact]
    def test_a_device_still_sending_at_the_deadline_ends_by_timeout_not_idle(self):
        result = self._transact(0.05)
        self.assertEqual(result["ended_by"], "timeout")
        self.assertGreater(result["received"], 0)

    # Tests [@ANCHOR: glasgow_ops_uart_transact]
    def test_a_device_that_goes_quiet_ends_by_idle(self):
        result = self._transact(0.01, timeout_s=3, idle_ms=100, max_bytes=2)
        self.assertEqual(result["ended_by"], "max_bytes")
        ops = _load_ops()

        class Burst(_FakeUart):
            sent = 0

            async def read_all(self, flush=True):
                Burst.sent += 1
                if Burst.sent > 2:
                    await asyncio.sleep(3600)
                return b"y"

        idle = asyncio.run(
            ops._uart_transact({"uart_iface": Burst(None)}, {"send": [], "until": [], "timeout_s": 3, "idle_ms": 100, "max_bytes": 65536})
        )
        self.assertEqual((idle["ended_by"], idle["received"]), ("idle", 2))

    # Tests [@ANCHOR: glasgow_ops_uart_transact]
    def test_a_silent_device_ends_by_timeout_with_nothing_received(self):
        result = self._transact(None)
        self.assertEqual((result["ended_by"], result["received"]), ("timeout", 0))


if __name__ == "__main__":
    os.environ.pop("PYTHONPATH", None)
    unittest.main()
