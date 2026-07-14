# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0


import logging
import psycopg2
from odoo.tests.test_cursor import TestCursor

from . import real_transaction
from . import test_integration
from . import test_json_rpc_client
from . import test_security_utils
from . import test_tdd_fixes
from . import test_views
from . import test_facility
from . import test_controllers
from . import common
from . import dummy_daemon

_logger = logging.getLogger(__name__)

# ====================================================================================
# THE CURSOR MONITOR TRAP
# ====================================================================================
# Dynamically monkey-patches Odoo's native TestCursor to detect transaction impedance.
# If a test requires RealTransactionCase but is using a standard HttpCase/TransactionCase,
# the resulting thread-locking or savepoint-bypass will throw specific psycopg2 errors.
# We catch these and emit a highly visible AI/Developer hint to instantly halt debugging.

# Fail-fast exact dependency enforcement for framework
test_cursor_cls = TestCursor

if test_cursor_cls:
    _original_test_cursor_execute = TestCursor.execute

    def _monitored_test_execute(self, query, params=None):
        try:
            return _original_test_cursor_execute(self, query, params)
        except psycopg2.Error as e:
            # 40001: SerializationFailure (Concurrent Update Deadlock)
            # 55P03: LockNotAvailable (Row-level lock held by another thread)
            # 23505: UniqueViolation (Often happens when raw SQL bypasses the savepoint)
            pgcode = e.pgcode
            if pgcode in ("40001", "55P03", "23505"):
                hint = (
                    "\n" + "=" * 80 + "\n"
                    "🚨 [FRAMEWORK HINT] TRANSACTION IMPEDANCE DETECTED 🚨\n"
                    f"A PostgreSQL conflict ({e.pgcode}) occurred inside the isolated TestCursor.\n"
                    "This almost always means a background thread, daemon, or raw SQL bypassed\n"
                    "the uncommitted test savepoint, creating an invisible data collision.\n\n"
                    "💡 FIX: Stop debugging the logic. Convert this test class to inherit from\n"
                    "         `zero_sudo.tests.real_transaction.RealTransactionCase`.\n"
                    + "=" * 80
                    + "\n"
                )
                # Emit directly to the logger so the FailureExtractor catches it instantly
                _logger.critical(hint)
            raise

    TestCursor.execute = _monitored_test_execute
