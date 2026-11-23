# SPDX-License-Identifier: GPL-2.0-or-later
"""The serial-console command wrapper: output and exit code come back between unique markers."""

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1]))
import smoke_test  # noqa: E402


class ParseTests(unittest.TestCase):
    def test_wrap_and_parse_roundtrip(self):
        wrapped = smoke_test.wrap("apk list -I", "abc")
        self.assertIn("apk list -I", wrapped)
        # the console echoes the command line (markers inside it), then prints the real output
        echoed = wrapped + "\r\n"
        real = "BEGIN-abc\r\nhams-node-0.1.0-r1\r\nEND-abc-0\r\n"
        out, rc = smoke_test.parse(echoed + real, "abc")
        self.assertEqual(out.strip(), "hams-node-0.1.0-r1")
        self.assertEqual(rc, 0)

    def test_nonzero_exit_code(self):
        out, rc = smoke_test.parse("BEGIN-x\r\nEND-x-3\r\n", "x")
        self.assertEqual((out.strip(), rc), ("", 3))

    def test_incomplete_returns_none(self):
        self.assertIsNone(smoke_test.parse("BEGIN-x\r\npartial", "x"))


if __name__ == "__main__":
    unittest.main()
