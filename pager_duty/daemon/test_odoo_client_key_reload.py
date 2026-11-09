#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
"""The pager monitor's Odoo client picks up a rotated key after a 401 (ticket 116: a daemon that read its key once failed
every call for nine days after the 2026-10-27 key rotation)."""

import io
import os
import sys
import tempfile
import unittest
import urllib.error
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


class TestOdooClientReloadsARotatedKey(unittest.TestCase):
    OLD, NEW = "old-key-value", "new-key-value"

    def setUp(self):
        self.seen = []
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.env_file = os.path.join(tmp.name, "pager_duty.env")
        patcher = mock.patch.object(gm, "ODOO_KEY_ENV_FILE", self.env_file)
        patcher.start()
        self.addCleanup(patcher.stop)
        patcher = mock.patch.object(gm._OPENER, "open", side_effect=self._odoo)
        patcher.start()
        self.addCleanup(patcher.stop)
        self.client = gm.OdooClient("http://odoo:8069", "db", "pager_service_internal", self.OLD)

    def _odoo(self, request, timeout=None):
        token = request.get_header("Authorization").split(" ", 1)[1]
        self.seen.append(token)
        if token != self.NEW:
            raise urllib.error.HTTPError(request.full_url, 401, "Unauthorized", {}, io.BytesIO(b'{"message": "Invalid apikey"}'))
        response = mock.MagicMock()
        response.__enter__.return_value = response
        response.read.return_value = b"[1]"
        return response

    def _write(self, text):
        with open(self.env_file, "w", encoding="utf-8") as handle:
            handle.write(text)

    # [@ANCHOR: pager_duty:test_odoo_client_reloads_a_rotated_key]
    def test_odoo_client_reloads_a_rotated_key(self):
        # Tests [@ANCHOR: pager_duty:odoo_client_reload_key]
        # Tests [@ANCHOR: pager_duty:odoo_client_execute]
        self._write(f"ODOO_RPC_LOGIN=x\nODOO_RPC_KEY='{self.NEW}'\n")
        self.assertEqual(self.client.execute("pager.check", "search_read"), [1])
        self.assertEqual(self.seen, [self.OLD, self.NEW])
        self.seen.clear()
        self.client.execute("pager.check", "search_read")
        self.assertEqual(self.seen, [self.NEW])

    def test_a_401_with_an_unchanged_or_unreadable_key_file_raises_without_looping(self):
        # Tests [@ANCHOR: pager_duty:odoo_client_reload_key]
        for content in (f"ODOO_RPC_KEY={self.OLD}\n", "NOTHING=here\n", None):
            with self.subTest(content=content):
                self.seen.clear()
                if content is None:
                    if os.path.exists(self.env_file):
                        os.remove(self.env_file)
                else:
                    self._write(content)
                with self.assertRaises(Exception) as caught:
                    self.client.execute("pager.check", "search_read")
                self.assertIn("401", str(caught.exception))
                self.assertEqual(self.seen, [self.OLD])


if __name__ == "__main__":
    unittest.main()
