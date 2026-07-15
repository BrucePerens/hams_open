from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged

@tagged('post_install', '-at_install')
class TestCMLeak(HamsTransactionCase):
    pass
