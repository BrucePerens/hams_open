# SPDX-License-Identifier: GPL-2.0-or-later
"""The serial-console command wrapper: output and exit code come back between unique markers."""

import pathlib
import socket
import threading
import importlib.util
import unittest

_spec = importlib.util.spec_from_file_location("smoke_test", pathlib.Path(__file__).resolve().parents[1] / "tools" / "smoke_test.py")
smoke_test = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(smoke_test)


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



class HttpAnswersTests(unittest.TestCase):
    """http_answers tells a dropped connection (accepted, silent: how QEMU's host forward looks) from a real reply."""

    def serve(self, reply):
        srv = socket.socket()
        srv.bind(("0.0.0.0", 0))
        srv.listen(1)
        self.addCleanup(srv.close)

        def run():
            c, _ = srv.accept()
            with c:
                c.recv(64)
                if reply:
                    c.sendall(reply)
                else:
                    c.settimeout(3)
                    try:
                        c.recv(1)
                    except OSError:
                        return  # the client gave up first

        threading.Thread(target=run, daemon=True).start()
        return srv.getsockname()[1]

    def test_reply_is_true(self):
        self.assertTrue(smoke_test.http_answers(self.serve(b"HTTP/1.0 403 Forbidden\r\n\r\n"), timeout=2))

    def test_silent_accept_is_false(self):
        self.assertFalse(smoke_test.http_answers(self.serve(b""), timeout=1))

    def test_refused_is_false(self):
        self.assertFalse(smoke_test.http_answers(smoke_test.free_port(), timeout=1))


if __name__ == "__main__":
    unittest.main()
