# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for the ntp / hamnet-ntp clock source: a one-shot SNTP query against a local fake server."""

import os
import pathlib
import shutil
import socket
import struct
import subprocess
import tempfile
import threading
import unittest

HERE = pathlib.Path(__file__).resolve().parent
SCRIPT = HERE.parent / "files" / "usr" / "libexec" / "hams-time" / "ntp"
UCI = '#!/bin/sh\nkey=${3#hams_node_time.main.}\ngrep "^$key=" "$FAKE/uci" | cut -d= -f2-\n'
EPOCH = 1800000000


def packet(first=0x24, stratum=2, epoch=EPOCH):
    return struct.pack("!BB38xI4x", first, stratum, epoch + 2208988800)


class FakeServer:
    def __init__(self, reply):
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.settimeout(8)
        self.port = self.sock.getsockname()[1]
        self.reply = reply
        threading.Thread(target=self.serve, daemon=True).start()

    def serve(self):
        try:
            data, addr = self.sock.recvfrom(64)
        except OSError:
            return
        if self.reply is not None and len(data) == 48:
            self.sock.sendto(self.reply, addr)

    def close(self):
        self.sock.close()


class NtpSourceTests(unittest.TestCase):
    def query(self, servers, name="ntp"):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        root = pathlib.Path(tmp.name)
        (root / "bin").mkdir()
        (root / "bin" / "uci").write_text(UCI)
        (root / "bin" / "uci").chmod(0o755)
        script = root / "bin" / name
        shutil.copy(SCRIPT, script)
        key = name.replace("-", "_") + "_server"
        (root / "uci").write_text("".join("%s=%s\n" % (key, s) for s in servers))
        env = dict(os.environ, PATH="%s:%s" % (root / "bin", os.environ["PATH"]), FAKE=str(root))
        return subprocess.run(["sh", str(script)], env=env, capture_output=True, text=True, timeout=60)

    def serve(self, reply):
        server = FakeServer(reply)
        self.addCleanup(server.close)
        return "127.0.0.1:%d" % server.port

    def test_a_good_answer_gives_the_epoch_and_a_small_error(self):
        result = self.query([self.serve(packet())])
        self.assertEqual(result.returncode, 0, result.stderr)
        epoch, error = result.stdout.split()
        self.assertEqual(int(epoch), EPOCH)
        self.assertLessEqual(int(error), 2)

    def test_no_answer_fails_so_the_next_source_is_tried(self):
        self.assertNotEqual(self.query([self.serve(None)]).returncode, 0)

    def test_unsynchronised_wrong_mode_or_zero_stratum_answers_are_refused(self):
        for bad in (packet(first=0xE4), packet(first=0x23), packet(stratum=0), packet(epoch=5), b"short"):
            self.assertNotEqual(self.query([self.serve(bad)]).returncode, 0, bad)

    def test_the_next_server_is_used_when_the_first_is_dead(self):
        result = self.query([self.serve(None), self.serve(packet())])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(int(result.stdout.split()[0]), EPOCH)

    def test_the_ham_network_source_reads_its_own_server_list(self):
        self.assertEqual(self.query([self.serve(packet())], name="hamnet-ntp").returncode, 0)

    def test_a_server_name_with_shell_characters_is_skipped(self):
        self.assertNotEqual(self.query(["127.0.0.1;touch x"]).returncode, 0)


if __name__ == "__main__":
    unittest.main()
