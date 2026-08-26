#!/usr/bin/env python3
# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later

import os
import sys
import unittest
from unittest.mock import patch

sys.path.append(os.path.dirname(os.path.abspath(__file__)))
import main as backup_worker  # noqa: E402


class TestRabbitMQCredentialFailFast(unittest.TestCase):
    def test_01_missing_both_raises(self):
        with patch.object(backup_worker, "RMQ_USER", None), patch.object(
            backup_worker, "RMQ_PASS", None
        ):
            with self.assertRaises(RuntimeError):
                backup_worker._require_rabbitmq_credentials()

    def test_02_missing_user_only_raises(self):
        with patch.object(backup_worker, "RMQ_USER", None), patch.object(
            backup_worker, "RMQ_PASS", "somepass"
        ):
            with self.assertRaises(RuntimeError):
                backup_worker._require_rabbitmq_credentials()

    def test_03_missing_pass_only_raises(self):
        with patch.object(backup_worker, "RMQ_USER", "someuser"), patch.object(
            backup_worker, "RMQ_PASS", None
        ):
            with self.assertRaises(RuntimeError):
                backup_worker._require_rabbitmq_credentials()

    def test_04_never_falls_back_to_guest_guest(self):
        # The whole point of the check: even the literal string "guest"
        # for both must pass through unmodified if explicitly set -- this
        # test only asserts the function doesn't itself substitute a
        # default; it does not endorse guest/guest as a real credential.
        with patch.object(backup_worker, "RMQ_USER", "guest"), patch.object(
            backup_worker, "RMQ_PASS", "guest"
        ):
            backup_worker._require_rabbitmq_credentials()  # must not raise

    def test_05_both_set_does_not_raise(self):
        with patch.object(backup_worker, "RMQ_USER", "real_user"), patch.object(
            backup_worker, "RMQ_PASS", "real_pass"
        ):
            backup_worker._require_rabbitmq_credentials()  # must not raise


if __name__ == "__main__":
    unittest.main()
