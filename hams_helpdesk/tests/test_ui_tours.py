from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

@tagged('post_install', '-at_install', 'ui', 'standard')
class TestHelpdeskTours(RealTransactionCase):
    pass
