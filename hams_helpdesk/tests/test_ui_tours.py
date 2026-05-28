from odoo.tests import tagged
from odoo.addons.hams_test.tests.real_transaction import RealTransactionCase

@tagged('post_install', '-at_install', 'ui', 'integration')
class TestHelpdeskTours(RealTransactionCase):
    pass
