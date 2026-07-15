from odoo.tests.common import TransactionCase, tagged
import os
import gc
from unittest.mock import patch, MagicMock

@tagged('post_install', '-at_install')
class TestCMLeak(TransactionCase):
    async def test_leak(self):
        pass
